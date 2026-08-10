package jp.mutsuna.echo

import android.app.*
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.*
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.net.Uri
import android.os.*
import android.provider.MediaStore
import androidx.core.app.NotificationCompat
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.time.LocalDateTime
import java.time.Instant
import java.time.format.DateTimeFormatter
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread
import kotlin.math.abs
import kotlin.math.max

class EchoRecordingService : Service() {
  private val stop = AtomicBoolean(false)
  private val cancel = AtomicBoolean(false)
  private var worker: Thread? = null
  private var projection: MediaProjection? = null

  override fun onBind(intent: Intent?) = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    when (intent?.action) {
      ACTION_STOP -> stop.set(true)
      ACTION_CANCEL -> { cancel.set(true); stop.set(true) }
      ACTION_START -> if (worker == null) startCapture(intent)
    }
    return START_NOT_STICKY
  }

  private fun startCapture(intent: Intent) {
    createChannel()
    val openIntent = PendingIntent.getActivity(
      this,
      0,
      Intent(this, MainActivity::class.java).addFlags(
        Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
      ),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    )
    val stopIntent = PendingIntent.getService(this, 1, Intent(this, EchoRecordingService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    val notification = NotificationCompat.Builder(this, CHANNEL)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setContentTitle("Mutsuna Echoで録音中")
      .setContentText("タップせずに停止するには「録音を停止」を選択します。")
      .setOngoing(true)
      .setContentIntent(openIntent)
      .addAction(0, "録音を停止", stopIntent)
      .build()
    if (Build.VERSION.SDK_INT >= 29) {
      startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE or
        if (intent.getBooleanExtra(EXTRA_SYSTEM, false)) ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION else 0)
    } else startForeground(NOTIFICATION_ID, notification)

    val config = JSONObject(intent.getStringExtra(EXTRA_CONFIG) ?: "{}")
    if (config.optBoolean("systemAudio")) {
      val data = intent.getParcelableExtra<Intent>(EXTRA_PROJECTION_DATA)
      projection = getSystemService(MediaProjectionManager::class.java)
        .getMediaProjection(intent.getIntExtra(EXTRA_RESULT_CODE, Activity.RESULT_CANCELED), data!!)
      projection?.registerCallback(object : MediaProjection.Callback() {
        override fun onStop() { stop.set(true) }
      }, Handler(Looper.getMainLooper()))
    }
    worker = thread(name = "mutsuna-android-recording") { capture(config) }
  }

  private fun capture(config: JSONObject) {
    val microphoneEnabled = config.optBoolean("microphone")
    val systemEnabled = config.optBoolean("systemAudio")
    val sessionId = "${System.currentTimeMillis()}-${android.os.Process.myPid()}"
    val session = File(filesDir, "recordings/in-progress/$sessionId").apply { mkdirs() }
    val mixedFile = File(session, "meeting.partial.m4a")
    val micFile = File(session, "microphone.partial.m4a")
    val systemFile = File(session, "system.partial.m4a")
    val manifest = File(session, "recording.json")
    var mic: AudioRecord? = null
    var system: AudioRecord? = null
    var micWriter: AacFragmentWriter? = null
    var systemWriter: AacFragmentWriter? = null
    var mixedWriter: AacFragmentWriter? = null
    var microphoneEnhancer: SonoraAudioEnhancer.Session? = null
    val startedAt = SystemClock.elapsedRealtime()
    try {
      if (microphoneEnabled) {
        mic = buildMicrophone().apply { startRecording() }
        micWriter = AacFragmentWriter(micFile, 96_000)
      }
      if (systemEnabled) {
        system = buildSystemAudio(projection ?: error("画面共有セッションがありません。")).apply { startRecording() }
        systemWriter = AacFragmentWriter(systemFile, 96_000)
      }
      mixedWriter = AacFragmentWriter(mixedFile, 64_000)
      if (microphoneEnabled) microphoneEnhancer = SonoraAudioEnhancer.Session(48_000)
      writeManifest(manifest, sessionId, startedAt, micFile, systemFile, mixedFile, microphoneEnabled, systemEnabled)
      RecordingBridge.update {
        put("phase", "recording"); put("sessionId", sessionId)
        put("microphone", microphoneEnabled); put("systemAudio", systemEnabled)
      }

      val micBuffer = ShortArray(FRAMES_PER_CHUNK)
      val sysBuffer = ShortArray(FRAMES_PER_CHUNK)
      var lastManifest = startedAt
      var noDataSince: Long? = null
      while (!stop.get()) {
        val elapsed = SystemClock.elapsedRealtime() - startedAt
        if (elapsed >= MAX_DURATION_MS) break
        val micRead = if (microphoneEnabled) readExact(mic!!, micBuffer) else 0
        val sysRead = if (systemEnabled) readExact(system!!, sysBuffer) else 0
        if ((microphoneEnabled && micRead < 0) || (systemEnabled && sysRead < 0)) {
          throw IllegalStateException("音声デバイスとの接続が失われました。途中までの録音を保存します。")
        }
        val frames = max(micRead, sysRead)
        if (frames == 0) {
          val stalledAt = noDataSince ?: SystemClock.elapsedRealtime().also { noDataSince = it }
          if (SystemClock.elapsedRealtime() - stalledAt >= 3_000) throw IllegalStateException("音声デバイスからデータが届かなくなりました。途中までの録音を保存します。")
          SystemClock.sleep(20)
          continue
        }
        noDataSince = null
        val enhancedMic = if (micRead > 0) {
          microphoneEnhancer?.process(micBuffer.copyOf(micRead)) ?: ShortArray(0)
        } else ShortArray(0)
        if (enhancedMic.isNotEmpty()) micWriter?.write(enhancedMic)
        if (sysRead > 0) systemWriter?.write(sysBuffer.copyOf(sysRead))
        val mixedFrames = max(enhancedMic.size, sysRead)
        val mixed = ShortArray(mixedFrames) { index ->
          val a = if (index < enhancedMic.size) enhancedMic[index] / 32768f else 0f
          val b = if (index < sysRead) sysBuffer[index] / 32768f else 0f
          val sum = a + b
          val limited = if (abs(sum) <= 0.95f) sum else kotlin.math.sign(sum) * (0.95f + 0.05f * kotlin.math.tanh(((abs(sum) - 0.95f) / 0.05f).toDouble()).toFloat())
          (limited * 32767.0f).toInt().coerceIn(-32768, 32767).toShort()
        }
        if (mixed.isNotEmpty()) mixedWriter.write(mixed)
        RecordingBridge.update {
          put("elapsedMs", elapsed)
          put("microphoneLevel", if (micRead > 0) peak(micBuffer, micRead) else 0.0)
          put("systemLevel", if (sysRead > 0) peak(sysBuffer, sysRead) else 0.0)
        }
        if (SystemClock.elapsedRealtime() - lastManifest >= 2_000) {
          writeManifest(manifest, sessionId, startedAt, micFile, systemFile, mixedFile, microphoneEnabled, systemEnabled)
          lastManifest = SystemClock.elapsedRealtime()
        }
      }

      RecordingBridge.update { put("phase", "finalizing"); put("microphoneLevel", 0.0); put("systemLevel", 0.0) }
      mic?.stop(); system?.stop()
      systemWriter?.close(); systemWriter = null
      val enhancedTail = microphoneEnhancer?.finish() ?: ShortArray(0); microphoneEnhancer = null
      if (enhancedTail.isNotEmpty()) {
        micWriter?.write(enhancedTail)
        mixedWriter.write(enhancedTail)
      }
      micWriter?.close(); micWriter = null
      mixedWriter.close(); mixedWriter = null
      if (cancel.get()) {
        session.deleteRecursively()
        RecordingBridge.update { put("phase", "idle"); put("outputPath", JSONObject.NULL) }
      } else {
        val name = LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyy-MM-dd_HH-mm-ss")) + ".m4a"
        val cacheOutput = uniqueCacheOutput(name).apply { parentFile?.mkdirs() }
        M4aFinalizer.finalize(mixedFile, cacheOutput)
        val trackDirectory = File(cacheDir, "recording-tracks/$sessionId").apply { mkdirs() }
        val finalizedMic = if (microphoneEnabled) File(trackDirectory, "microphone.m4a").also {
          M4aFinalizer.finalize(micFile, it)
        } else null
        val finalizedSystem = if (systemEnabled) File(trackDirectory, "system.m4a").also {
          M4aFinalizer.finalize(systemFile, it)
        } else null
        publishToMusic(cacheOutput, name)
        session.deleteRecursively()
        RecordingBridge.update {
          put("phase", "completed")
          put("elapsedMs", SystemClock.elapsedRealtime() - startedAt)
          put("outputPath", cacheOutput.absolutePath)
          put("microphoneTrackPath", finalizedMic?.absolutePath ?: JSONObject.NULL)
          put("systemTrackPath", finalizedSystem?.absolutePath ?: JSONObject.NULL)
          put("stopReason", if (SystemClock.elapsedRealtime() - startedAt >= MAX_DURATION_MS) "durationLimit" else "user")
        }
      }
    } catch (error: Throwable) {
      try {
        val enhancedTail = microphoneEnhancer?.finish() ?: ShortArray(0)
        microphoneEnhancer = null
        if (enhancedTail.isNotEmpty()) {
          micWriter?.write(enhancedTail)
          mixedWriter?.write(enhancedTail)
        }
      } catch (_: Throwable) {}
      try { micWriter?.close() } catch (_: Throwable) {}
      try { systemWriter?.close() } catch (_: Throwable) {}
      try { mixedWriter?.close() } catch (_: Throwable) {}
      if (!cancel.get() && mixedFile.exists() && mixedFile.length() > 0) {
        try {
          val name = LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyy-MM-dd_HH-mm-ss")) + ".m4a"
          val output = uniqueCacheOutput(name).apply { parentFile?.mkdirs() }
          M4aFinalizer.finalize(mixedFile, output)
          val trackDirectory = File(cacheDir, "recording-tracks/$sessionId").apply { mkdirs() }
          val finalizedMic = if (microphoneEnabled && micFile.length() > 0) File(trackDirectory, "microphone.m4a").also {
            M4aFinalizer.finalize(micFile, it)
          } else null
          val finalizedSystem = if (systemEnabled && systemFile.length() > 0) File(trackDirectory, "system.m4a").also {
            M4aFinalizer.finalize(systemFile, it)
          } else null
          publishToMusic(output, output.name); session.deleteRecursively()
          RecordingBridge.update {
            put("phase", "completed"); put("elapsedMs", SystemClock.elapsedRealtime() - startedAt)
            put("outputPath", output.absolutePath)
            put("microphoneTrackPath", finalizedMic?.absolutePath ?: JSONObject.NULL)
            put("systemTrackPath", finalizedSystem?.absolutePath ?: JSONObject.NULL)
            put("stopReason", "captureError")
            put("error", error.message ?: "音声の取得が停止したため、途中までの録音を保存しました。")
          }
        } catch (recoveryError: Throwable) {
          RecordingBridge.update { put("phase", "failed"); put("error", "録音は一時領域に残っていますが、自動確定できませんでした: ${recoveryError.message}"); put("stopReason", "captureError") }
        }
      } else RecordingBridge.update {
        put("phase", "failed"); put("error", error.message ?: "Androidの録音処理に失敗しました。")
        put("stopReason", "captureError")
      }
    } finally {
      try { microphoneEnhancer?.close() } catch (_: Throwable) {}
      try { mic?.release() } catch (_: Throwable) {}
      try { system?.release() } catch (_: Throwable) {}
      projection?.stop(); projection = null
      stopForeground(STOP_FOREGROUND_REMOVE)
      stopSelf()
    }
  }

  private fun buildMicrophone(): AudioRecord = AudioRecord.Builder()
    .setAudioSource(MediaRecorder.AudioSource.VOICE_RECOGNITION)
    .setAudioFormat(audioFormat()).setBufferSizeInBytes(bufferBytes()).build()

  private fun buildSystemAudio(mediaProjection: MediaProjection): AudioRecord {
    val capture = AudioPlaybackCaptureConfiguration.Builder(mediaProjection)
      .addMatchingUsage(AudioAttributes.USAGE_MEDIA).addMatchingUsage(AudioAttributes.USAGE_GAME).build()
    return AudioRecord.Builder().setAudioPlaybackCaptureConfig(capture)
      .setAudioFormat(audioFormat()).setBufferSizeInBytes(bufferBytes()).build()
  }

  private fun audioFormat() = AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT)
    .setSampleRate(48_000).setChannelMask(AudioFormat.CHANNEL_IN_MONO).build()
  private fun bufferBytes() = max(FRAMES_PER_CHUNK * 4, AudioRecord.getMinBufferSize(48_000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT))
  private fun readExact(record: AudioRecord, buffer: ShortArray): Int = record.read(buffer, 0, buffer.size, AudioRecord.READ_BLOCKING)
  private fun peak(samples: ShortArray, count: Int): Double = (0 until count).maxOfOrNull { abs(samples[it].toInt()) }?.div(32768.0) ?: 0.0

  private fun writeManifest(file: File, sessionId: String, startedAt: Long, mic: File, system: File, mixed: File, micOn: Boolean, systemOn: Boolean) {
    val elapsed = SystemClock.elapsedRealtime() - startedAt
    val finalName = LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyy-MM-dd_HH-mm-ss")) + ".m4a"
    val json = JSONObject().apply {
      put("version", 1); put("sessionId", sessionId)
      put("startedAt", Instant.ofEpochMilli(System.currentTimeMillis() - elapsed).toString())
      put("updatedAt", Instant.now().toString()); put("durationMs", elapsed)
      put("microphone", micOn); put("systemAudio", systemOn)
      put("microphoneFile", if (micOn) mic.absolutePath else JSONObject.NULL)
      put("systemFile", if (systemOn) system.absolutePath else JSONObject.NULL)
      put("mixedFile", mixed.absolutePath)
      put("finalFile", File(cacheDir, "recordings/$finalName").absolutePath)
      put("finalized", false); put("stopReason", JSONObject.NULL)
    }
    val temporary = File(file.parentFile, "recording.json.tmp")
    val backup = File(file.parentFile, "recording.json.backup")
    FileOutputStream(temporary).use { output ->
      output.write(json.toString().toByteArray(Charsets.UTF_8)); output.fd.sync()
    }
    if (file.exists()) {
      backup.delete()
      if (!file.renameTo(backup)) throw IllegalStateException("録音の復旧情報を更新できません。")
    }
    if (!temporary.renameTo(file)) {
      if (backup.exists()) backup.renameTo(file)
      throw IllegalStateException("録音の復旧情報を保存できません。")
    }
    backup.delete()
  }

  private fun uniqueCacheOutput(preferredName: String): File {
    val directory = File(cacheDir, "recordings")
    var candidate = File(directory, preferredName)
    var suffix = 2
    val stem = preferredName.removeSuffix(".m4a")
    while (candidate.exists()) candidate = File(directory, "${stem}_${suffix++}.m4a")
    return candidate
  }

  private fun publishToMusic(source: File, displayName: String): Uri? {
    val values = ContentValues().apply {
      put(MediaStore.Audio.Media.DISPLAY_NAME, displayName); put(MediaStore.Audio.Media.MIME_TYPE, "audio/mp4")
      put(MediaStore.Audio.Media.RELATIVE_PATH, "Music/Mutsuna Echo"); put(MediaStore.Audio.Media.IS_PENDING, 1)
    }
    val uri = contentResolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values) ?: return null
    try {
      contentResolver.openOutputStream(uri)?.use { output -> source.inputStream().use { it.copyTo(output) } }
        ?: throw IllegalStateException("Musicへ録音を書き込めませんでした。")
      contentResolver.update(uri, ContentValues().apply { put(MediaStore.Audio.Media.IS_PENDING, 0) }, null, null)
      return uri
    } catch (error: Throwable) { contentResolver.delete(uri, null, null); throw error }
  }

  private fun createChannel() {
    getSystemService(NotificationManager::class.java).createNotificationChannel(
      NotificationChannel(CHANNEL, "録音", NotificationManager.IMPORTANCE_LOW))
  }

  companion object {
    const val ACTION_START = "jp.mutsuna.echo.START_RECORDING"
    const val ACTION_STOP = "jp.mutsuna.echo.STOP_RECORDING"
    const val ACTION_CANCEL = "jp.mutsuna.echo.CANCEL_RECORDING"
    private const val EXTRA_CONFIG = "config"
    private const val EXTRA_RESULT_CODE = "resultCode"
    private const val EXTRA_PROJECTION_DATA = "projectionData"
    private const val EXTRA_SYSTEM = "systemAudio"
    private const val CHANNEL = "recording"
    private const val NOTIFICATION_ID = 41
    private const val FRAMES_PER_CHUNK = 960
    private const val MAX_DURATION_MS = 36_000_000L

    fun start(context: Context, config: String, resultCode: Int, projectionData: Intent) {
      val intent = Intent(context, EchoRecordingService::class.java).setAction(ACTION_START)
        .putExtra(EXTRA_CONFIG, config).putExtra(EXTRA_RESULT_CODE, resultCode)
        .putExtra(EXTRA_PROJECTION_DATA, projectionData).putExtra(EXTRA_SYSTEM, JSONObject(config).optBoolean("systemAudio"))
      context.startForegroundService(intent)
    }
  }
}
