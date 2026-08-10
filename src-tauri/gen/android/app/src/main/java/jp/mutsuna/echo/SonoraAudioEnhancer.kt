package jp.mutsuna.echo

/** Thin JNI adapter over the shared Rust Sonora NS + AGC2 implementation. */
object SonoraAudioEnhancer {
  @JvmStatic private external fun create(sampleRate: Int): Long
  @JvmStatic private external fun process(handle: Long, samples: ShortArray): ShortArray
  @JvmStatic private external fun finish(handle: Long): ShortArray
  @JvmStatic private external fun destroy(handle: Long)

  class Session(sampleRate: Int) : AutoCloseable {
    private var handle = create(sampleRate)

    fun process(samples: ShortArray): ShortArray {
      check(handle != 0L) { "音声強調セッションは終了しています。" }
      return SonoraAudioEnhancer.process(handle, samples)
    }

    fun finish(): ShortArray {
      check(handle != 0L) { "音声強調セッションは終了しています。" }
      val current = handle
      handle = 0L
      return SonoraAudioEnhancer.finish(current)
    }

    override fun close() {
      if (handle != 0L) SonoraAudioEnhancer.destroy(handle)
      handle = 0L
    }
  }
}
