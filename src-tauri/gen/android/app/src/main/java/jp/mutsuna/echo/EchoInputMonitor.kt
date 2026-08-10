package jp.mutsuna.echo

import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread
import kotlin.math.abs
import kotlin.math.max

object EchoInputMonitor {
  private val stop = AtomicBoolean(true)
  @Volatile private var worker: Thread? = null

  @Synchronized fun start() {
    if (worker?.isAlive == true) return
    stop.set(false)
    worker = thread(name = "mutsuna-input-monitor") {
      var recorder: AudioRecord? = null
      try {
        val format = AudioFormat.Builder()
          .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
          .setSampleRate(SAMPLE_RATE)
          .setChannelMask(AudioFormat.CHANNEL_IN_MONO)
          .build()
        val bufferSize = max(
          FRAMES_PER_CHUNK * 4,
          AudioRecord.getMinBufferSize(SAMPLE_RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        )
        recorder = AudioRecord.Builder()
          .setAudioSource(MediaRecorder.AudioSource.VOICE_RECOGNITION)
          .setAudioFormat(format)
          .setBufferSizeInBytes(bufferSize)
          .build()
          .apply { startRecording() }
        val buffer = ShortArray(FRAMES_PER_CHUNK)
        while (!stop.get()) {
          val count = recorder.read(buffer, 0, buffer.size, AudioRecord.READ_BLOCKING)
          if (count < 0) error("マイク入力を確認できませんでした。")
          val level = if (count > 0) {
            (0 until count).maxOfOrNull { abs(buffer[it].toInt()) }?.div(32768.0) ?: 0.0
          } else 0.0
          RecordingBridge.updateMonitorLevel(level)
        }
      } catch (error: Throwable) {
        if (!stop.get()) RecordingBridge.failMonitor(error.message ?: "マイク入力を確認できませんでした。")
      } finally {
        try { recorder?.stop() } catch (_: Throwable) {}
        try { recorder?.release() } catch (_: Throwable) {}
        RecordingBridge.updateMonitorLevel(0.0)
        worker = null
      }
    }
  }

  @Synchronized fun stop() {
    stop.set(true)
    val active = worker
    if (active != null && active !== Thread.currentThread()) active.join(1_000)
    worker = null
    RecordingBridge.updateMonitorLevel(0.0)
  }

  private const val SAMPLE_RATE = 48_000
  private const val FRAMES_PER_CHUNK = 960
}
