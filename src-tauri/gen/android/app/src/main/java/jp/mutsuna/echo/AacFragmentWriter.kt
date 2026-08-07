package jp.mutsuna.echo

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.muxer.BufferInfo
import androidx.media3.muxer.FragmentedMp4Muxer
import java.io.File
import java.io.FileOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.channels.Channels

@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
internal class AacFragmentWriter(file: File, private val bitrate: Int) : AutoCloseable {
  private val codec = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_AUDIO_AAC)
  private val muxer = FragmentedMp4Muxer.Builder(Channels.newChannel(FileOutputStream(file)))
    .setFragmentDurationMs(2_000)
    .setSampleCopyingEnabled(true)
    .build()
  private var trackId = -1
  private var framesWritten = 0L
  private var closed = false

  init {
    val format = MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_AAC, SAMPLE_RATE, 1).apply {
      setInteger(MediaFormat.KEY_AAC_PROFILE, MediaCodecInfo.CodecProfileLevel.AACObjectLC)
      setInteger(MediaFormat.KEY_BIT_RATE, bitrate)
      setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, 8_192)
    }
    codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
    codec.start()
  }

  fun write(samples: ShortArray) {
    check(!closed)
    var offset = 0
    while (offset < samples.size) {
      val inputIndex = codec.dequeueInputBuffer(10_000)
      if (inputIndex < 0) { drain(false); continue }
      val input = codec.getInputBuffer(inputIndex) ?: error("AAC入力バッファを取得できません。")
      input.clear().order(ByteOrder.LITTLE_ENDIAN)
      val count = minOf(samples.size - offset, input.remaining() / 2)
      repeat(count) { input.putShort(samples[offset + it]) }
      val ptsUs = framesWritten * 1_000_000L / SAMPLE_RATE
      codec.queueInputBuffer(inputIndex, 0, count * 2, ptsUs, 0)
      framesWritten += count
      offset += count
      drain(false)
    }
  }

  private fun drain(end: Boolean) {
    val info = MediaCodec.BufferInfo()
    var emptyPolls = 0
    while (true) {
      val index = codec.dequeueOutputBuffer(info, if (end) 10_000 else 0)
      when {
        index == MediaCodec.INFO_TRY_AGAIN_LATER -> {
          if (!end || ++emptyPolls >= 100) return
        }
        index == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
          emptyPolls = 0
          val format = codec.outputFormat
          val csd = format.getByteBuffer("csd-0")?.let { buffer -> ByteArray(buffer.remaining()).also { buffer.get(it) } } ?: ByteArray(0)
          trackId = muxer.addTrack(Format.Builder()
            .setSampleMimeType(MimeTypes.AUDIO_AAC)
            .setSampleRate(SAMPLE_RATE)
            .setChannelCount(1)
            .setAverageBitrate(bitrate)
            .setInitializationData(listOf(csd))
            .build())
        }
        index >= 0 -> {
          emptyPolls = 0
          if (info.size > 0 && trackId >= 0 && info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG == 0) {
            val output = codec.getOutputBuffer(index) ?: error("AAC出力バッファを取得できません。")
            output.position(info.offset)
            output.limit(info.offset + info.size)
            val flags = if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) C.BUFFER_FLAG_END_OF_STREAM else 0
            muxer.writeSampleData(trackId, output.slice(), BufferInfo(info.presentationTimeUs, info.size, flags))
          }
          val eos = info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0
          codec.releaseOutputBuffer(index, false)
          if (eos) return
        }
      }
    }
  }

  override fun close() {
    if (closed) return
    closed = true
    var index = codec.dequeueInputBuffer(100_000)
    var attempts = 0
    while (index < 0 && attempts++ < 100) {
      drain(false)
      index = codec.dequeueInputBuffer(10_000)
    }
    if (index < 0) throw IllegalStateException("AACエンコーダーを正常終了できませんでした。")
    codec.queueInputBuffer(index, 0, 0, framesWritten * 1_000_000L / SAMPLE_RATE, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
    drain(true)
    codec.stop()
    codec.release()
    muxer.close()
  }

  companion object { const val SAMPLE_RATE = 48_000 }
}
