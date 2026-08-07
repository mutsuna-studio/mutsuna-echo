package jp.mutsuna.echo

import android.content.Context
import android.content.ContentValues
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.provider.MediaStore
import org.json.JSONObject

object RecordingBridge {
  @Volatile internal var activity: MainActivity? = null
  private val lock = Any()
  private var status = JSONObject(defaultStatus())

  @JvmStatic fun capabilities(@Suppress("UNUSED_PARAMETER") context: Context): String = JSONObject().apply {
    put("platform", "android")
    put("supported", true)
    put("microphoneSupported", true)
    put("systemAudioSupported", true)
    put("systemAudioLimited", true)
    put("limitation", "Androidでは、再生元アプリが録音を許可した音声だけ取得できます。通話・DRM保護音声などは取得できません。また、他アプリがマイクを占有すると録音が停止することがあります。")
    put("microphoneDevices", org.json.JSONArray())
    put("systemDevices", org.json.JSONArray())
    put("sampleRate", 48_000)
    put("channels", 1)
    put("codec", "AAC-LC")
    put("bitrate", 64_000)
    put("maxDurationMs", 36_000_000L)
  }.toString()

  @JvmStatic fun start(@Suppress("UNUSED_PARAMETER") context: Context, config: String): String {
    synchronized(lock) {
      if (status.optString("phase") in setOf("starting", "recording", "finalizing")) {
        throw IllegalStateException("録音はすでに実行中です。")
      }
      status = JSONObject(defaultStatus()).apply {
        put("phase", "starting")
        put("microphone", JSONObject(config).optBoolean("microphone"))
        put("systemAudio", JSONObject(config).optBoolean("systemAudio"))
      }
    }
    val current = activity ?: throw IllegalStateException("録音を開始するにはMutsuna Echoを前面に表示してください。")
    current.requestRecording(config)
    return getStatus(context)
  }

  @JvmStatic fun stop(context: Context, cancel: Boolean): String {
    val intent = Intent(context, EchoRecordingService::class.java).setAction(if (cancel) EchoRecordingService.ACTION_CANCEL else EchoRecordingService.ACTION_STOP)
    context.startService(intent)
    return getStatus(context)
  }

  @JvmStatic fun getStatus(@Suppress("UNUSED_PARAMETER") context: Context): String = synchronized(lock) { status.toString() }

  @JvmStatic fun copyContentUri(context: Context, value: String): String {
    val uri = Uri.parse(value)
    require(uri.scheme == "content") { "Androidのcontent URIではありません。" }
    var displayName = "selected-audio"
    context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
      if (cursor.moveToFirst()) displayName = cursor.getString(0) ?: displayName
    }
    displayName = displayName.substringAfterLast('/').replace(Regex("[^A-Za-z0-9._-]"), "_")
    val directory = java.io.File(context.cacheDir, "imports").apply { mkdirs() }
    var destination = java.io.File(directory, displayName)
    var suffix = 2
    val extension = destination.extension.let { if (it.isEmpty()) "" else ".$it" }
    val stem = destination.name.removeSuffix(extension)
    while (destination.exists()) destination = java.io.File(directory, "${stem}_${suffix++}$extension")
    val temporary = java.io.File(directory, destination.name + ".partial")
    context.contentResolver.openInputStream(uri)?.use { input ->
      java.io.FileOutputStream(temporary).use { output -> input.copyTo(output); output.fd.sync() }
    } ?: throw IllegalStateException("選択した音声を開けませんでした。")
    if (!temporary.renameTo(destination)) throw IllegalStateException("選択した音声をアプリ領域へ確定できませんでした。")
    return destination.absolutePath
  }

  @JvmStatic fun recover(context: Context, sessionId: String): String {
    require(Regex("^[A-Za-z0-9-]+$").matches(sessionId)) { "録音セッションIDが不正です。" }
    val directory = java.io.File(context.filesDir, "recordings/in-progress/$sessionId")
    val primaryManifest = java.io.File(directory, "recording.json")
    val manifestFile = if (primaryManifest.exists()) primaryManifest else java.io.File(directory, "recording.json.backup")
    val manifest = JSONObject(manifestFile.readText())
    val source = java.io.File(manifest.getString("mixedFile"))
    require(source.isFile && source.length() > 0) { "復旧可能なM4Aフラグメントがありません。" }
    val outputDirectory = java.io.File(context.cacheDir, "recordings").apply { mkdirs() }
    val base = java.time.LocalDateTime.now().format(java.time.format.DateTimeFormatter.ofPattern("yyyy-MM-dd_HH-mm-ss"))
    var output = java.io.File(outputDirectory, "$base.m4a")
    var suffix = 2
    while (output.exists()) output = java.io.File(outputDirectory, "${base}_${suffix++}.m4a")
    val temporary = java.io.File(outputDirectory, ".${output.name}.partial")
    source.inputStream().use { input -> java.io.FileOutputStream(temporary).use { sink -> input.copyTo(sink); sink.fd.sync() } }
    check(temporary.renameTo(output)) { "復旧した録音を確定できませんでした。" }
    val values = ContentValues().apply {
      put(MediaStore.Audio.Media.DISPLAY_NAME, output.name); put(MediaStore.Audio.Media.MIME_TYPE, "audio/mp4")
      put(MediaStore.Audio.Media.RELATIVE_PATH, "Music/Mutsuna Echo"); put(MediaStore.Audio.Media.IS_PENDING, 1)
    }
    val uri = context.contentResolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values)
      ?: throw IllegalStateException("Musicへの保存先を作成できませんでした。")
    try {
      context.contentResolver.openOutputStream(uri)?.use { sink -> output.inputStream().use { it.copyTo(sink) } }
        ?: throw IllegalStateException("Musicへ録音を書き込めませんでした。")
      context.contentResolver.update(uri, ContentValues().apply { put(MediaStore.Audio.Media.IS_PENDING, 0) }, null, null)
    } catch (error: Throwable) { context.contentResolver.delete(uri, null, null); throw error }
    check(directory.deleteRecursively()) { "復旧後の一時データを削除できませんでした。" }
    return output.absolutePath
  }

  internal fun update(block: JSONObject.() -> Unit) = synchronized(lock) { status.apply(block) }

  internal fun failStart(message: String) = update {
    put("phase", "failed")
    put("error", message)
    put("stopReason", "captureError")
  }

  internal fun requiresSystemAudio(config: String): Boolean = JSONObject(config).optBoolean("systemAudio")

  private fun defaultStatus() = """{
    "phase":"idle","sessionId":null,"elapsedMs":0,"microphone":false,"systemAudio":false,
    "microphoneLevel":0.0,"systemLevel":0.0,"outputPath":null,"stopReason":null,"error":null
  }""".trimIndent()
}
