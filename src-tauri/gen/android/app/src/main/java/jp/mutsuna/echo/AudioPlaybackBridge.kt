package jp.mutsuna.echo

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.AudioAttributes
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.PlaybackParameters
import androidx.media3.exoplayer.ExoPlayer
import org.json.JSONObject
import java.io.File
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit

/** Native playback engine for local meeting audio.
 *
 * WebView custom protocols on Android cannot reliably serve the byte-range
 * requests made by Chromium's media pipeline. Media3 reads the validated local
 * file directly and leaves the Svelte UI responsible only for presentation.
 */
object AudioPlaybackBridge {
  private val mainHandler = Handler(Looper.getMainLooper())
  private var player: ExoPlayer? = null
  private var currentPath: String? = null
  private var lastError: String? = null

  private val listener = object : Player.Listener {
    override fun onPlayerError(error: PlaybackException) {
      lastError = buildString {
        append("Androidの音声再生に失敗しました（")
        append(error.errorCodeName)
        append("）")
        error.cause?.message?.takeIf { it.isNotBlank() }?.let { append(": ").append(it) }
      }
    }
  }

  @JvmStatic fun load(context: Context, path: String): String = execute {
    val file = File(path)
    require(file.isFile && file.length() > 0L) { "再生する音声ファイルが見つかりません。" }
    val target = file.canonicalPath
    val instance = obtainPlayer(context.applicationContext)
    lastError = null
    if (currentPath != target) {
      currentPath = target
      instance.setMediaItem(MediaItem.fromUri(Uri.fromFile(file)))
      instance.prepare()
    }
  }

  @JvmStatic fun play(context: Context): String = execute {
    val instance = requirePlayer(context)
    require(currentPath != null) { "再生する音声が読み込まれていません。" }
    lastError = null
    if (instance.playbackState == Player.STATE_ENDED) instance.seekTo(0L)
    instance.play()
  }

  @JvmStatic fun pause(context: Context): String = execute {
    requirePlayer(context).pause()
  }

  @JvmStatic fun seekTo(context: Context, positionMs: Long): String = execute {
    requirePlayer(context).seekTo(positionMs.coerceAtLeast(0L))
  }

  @JvmStatic fun setVolume(context: Context, volume: Float): String = execute {
    requirePlayer(context).volume = volume.coerceIn(0f, 1f)
  }

  @JvmStatic fun setPlaybackRate(context: Context, rate: Float): String = execute {
    require(rate in 0.25f..4f) { "再生速度が対応範囲外です。" }
    requirePlayer(context).playbackParameters = PlaybackParameters(rate)
  }

  @JvmStatic fun getState(context: Context): String = execute {
    obtainPlayer(context.applicationContext)
  }

  @JvmStatic fun release(@Suppress("UNUSED_PARAMETER") context: Context): String = execute {
    player?.removeListener(listener)
    player?.release()
    player = null
    currentPath = null
    lastError = null
  }

  private fun obtainPlayer(context: Context): ExoPlayer = player ?: ExoPlayer.Builder(context).build().also {
    it.setAudioAttributes(
      AudioAttributes.Builder()
        .setUsage(C.USAGE_MEDIA)
        .setContentType(C.AUDIO_CONTENT_TYPE_SPEECH)
        .build(),
      true
    )
    it.setHandleAudioBecomingNoisy(true)
    it.addListener(listener)
    player = it
  }

  private fun requirePlayer(context: Context): ExoPlayer = obtainPlayer(context.applicationContext)

  private fun stateJson(): String {
    val instance = player
    val duration = instance?.duration?.takeUnless { it == C.TIME_UNSET }?.coerceAtLeast(0L) ?: 0L
    val playbackState = instance?.playbackState ?: Player.STATE_IDLE
    return JSONObject().apply {
      put("loaded", currentPath != null)
      put("playing", instance?.playWhenReady == true && playbackState != Player.STATE_ENDED)
      put("positionMs", instance?.currentPosition?.coerceAtLeast(0L) ?: 0L)
      put("durationMs", duration)
      put("bufferedPositionMs", instance?.bufferedPosition?.coerceAtLeast(0L) ?: 0L)
      put("buffering", playbackState == Player.STATE_BUFFERING)
      put("ended", playbackState == Player.STATE_ENDED)
      put("error", lastError ?: JSONObject.NULL)
    }.toString()
  }

  private fun execute(block: () -> Unit): String = try {
    onMainThread {
      block()
      stateJson()
    }
  } catch (error: Throwable) {
    val cause = generateSequence(error) { it.cause }.last()
    onMainThread {
      lastError = cause.message?.takeIf { it.isNotBlank() } ?: "Androidの音声再生処理に失敗しました。"
      stateJson()
    }
  }

  private fun <T> onMainThread(block: () -> T): T {
    if (Looper.myLooper() == Looper.getMainLooper()) return block()
    val task = FutureTask(block)
    check(mainHandler.post(task)) { "音声再生処理をメインスレッドへ送信できませんでした。" }
    return task.get(5, TimeUnit.SECONDS)
  }
}
