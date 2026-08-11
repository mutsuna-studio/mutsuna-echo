package jp.mutsuna.echo

import android.content.Context
import android.view.WindowManager
import java.lang.ref.WeakReference

object ScreenOnController {
  private const val PREFERENCES = "processing-power"
  private const val KEEP_DISPLAY_ON = "keep-display-on"
  private val lock = Any()
  private var activity: WeakReference<MainActivity>? = null
  private var activeOperations = 0

  fun attach(value: MainActivity) {
    synchronized(lock) {
      activity = WeakReference(value)
      applyLocked(value.applicationContext)
    }
  }

  fun detach(value: MainActivity) {
    synchronized(lock) {
      if (activity?.get() === value) activity = null
    }
  }

  fun begin(context: Context) {
    synchronized(lock) {
      activeOperations += 1
      applyLocked(context)
    }
  }

  fun end(context: Context) {
    synchronized(lock) {
      activeOperations = (activeOperations - 1).coerceAtLeast(0)
      applyLocked(context)
    }
  }

  fun setEnabled(context: Context, enabled: Boolean) {
    context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
      .edit()
      .putBoolean(KEEP_DISPLAY_ON, enabled)
      .apply()
    synchronized(lock) {
      applyLocked(context)
    }
  }

  private fun applyLocked(context: Context) {
    val current = activity?.get() ?: return
    val enabled = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
      .getBoolean(KEEP_DISPLAY_ON, false)
    val keepOn = enabled && activeOperations > 0
    current.runOnUiThread {
      if (keepOn) {
        current.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      } else {
        current.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      }
    }
  }
}
