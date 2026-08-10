package jp.mutsuna.echo

import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMuxer
import java.io.File
import java.io.FileOutputStream
import java.nio.ByteBuffer

/**
 * Repackages a fragmented recording into a regular M4A without re-encoding AAC.
 *
 * Fragmented MP4 keeps an in-progress recording recoverable, but Android WebView
 * cannot seek reliably in that file. A normally completed recording therefore
 * passes through MediaMuxer before it is exposed to the rest of the app.
 */
internal object M4aFinalizer {
  fun finalize(source: File, destination: File) {
    require(source.isFile && source.length() > 0) { "確定する録音データがありません。" }
    destination.parentFile?.mkdirs()
    val temporary = File(destination.parentFile, ".${destination.name}.finalizing")
    temporary.delete()

    val extractor = MediaExtractor()
    var muxer: MediaMuxer? = null
    var completed = false
    try {
      extractor.setDataSource(source.absolutePath)
      val inputTrack = (0 until extractor.trackCount).firstOrNull { index ->
        extractor.getTrackFormat(index).getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true
      } ?: throw IllegalStateException("録音ファイルに音声トラックがありません。")
      val format = extractor.getTrackFormat(inputTrack)
      extractor.selectTrack(inputTrack)

      muxer = MediaMuxer(temporary.absolutePath, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
      val outputTrack = muxer.addTrack(format)
      muxer.start()

      val bufferSize = if (format.containsKey(MediaFormat.KEY_MAX_INPUT_SIZE)) {
        format.getInteger(MediaFormat.KEY_MAX_INPUT_SIZE).coerceAtLeast(DEFAULT_BUFFER_SIZE)
      } else {
        DEFAULT_BUFFER_SIZE
      }
      val buffer = ByteBuffer.allocateDirect(bufferSize)
      val info = MediaCodec.BufferInfo()
      var samplesWritten = 0
      while (true) {
        buffer.clear()
        val size = extractor.readSampleData(buffer, 0)
        if (size < 0) break
        info.set(0, size, extractor.sampleTime.coerceAtLeast(0), extractor.sampleFlags)
        muxer.writeSampleData(outputTrack, buffer, info)
        samplesWritten += 1
        extractor.advance()
      }
      check(samplesWritten > 0) { "録音ファイルに再生可能な音声がありません。" }

      muxer.stop()
      muxer.release()
      muxer = null
      FileOutputStream(temporary, true).use { it.fd.sync() }
      if (destination.exists() && !destination.delete()) {
        throw IllegalStateException("既存の録音ファイルを置き換えられません。")
      }
      check(temporary.renameTo(destination)) { "録音ファイルを確定できません。" }
      completed = true
    } finally {
      extractor.release()
      runCatching { muxer?.release() }
      if (!completed) temporary.delete()
    }
  }

  private const val DEFAULT_BUFFER_SIZE = 1024 * 1024
}
