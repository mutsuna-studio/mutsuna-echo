package jp.mutsuna.echo

import android.content.Context
import com.google.android.play.core.splitinstall.SplitInstallManager
import com.google.android.play.core.splitinstall.SplitInstallManagerFactory
import com.google.android.play.core.splitinstall.SplitInstallRequest
import com.google.android.play.core.splitinstall.SplitInstallStateUpdatedListener
import com.google.android.play.core.splitinstall.model.SplitInstallSessionStatus
import org.json.JSONObject

object LocalAiFeatureBridge {
  private const val MODULE = "local_ai_runtime"
  @Volatile private var manager: SplitInstallManager? = null
  @Volatile private var sessionId = 0
  @Volatile private var phase = "notInstalled"
  @Volatile private var downloaded = 0L
  @Volatile private var total = 0L
  @Volatile private var error: String? = null
  @Volatile private var removalPending = false

  private val listener = SplitInstallStateUpdatedListener { state ->
    if (sessionId != 0 && state.sessionId() != sessionId) return@SplitInstallStateUpdatedListener
    sessionId = state.sessionId()
    downloaded = state.bytesDownloaded()
    total = state.totalBytesToDownload()
    phase = when (state.status()) {
      SplitInstallSessionStatus.PENDING,
      SplitInstallSessionStatus.REQUIRES_USER_CONFIRMATION -> "downloading"
      SplitInstallSessionStatus.DOWNLOADING -> "downloading"
      SplitInstallSessionStatus.DOWNLOADED,
      SplitInstallSessionStatus.INSTALLING -> "installing"
      SplitInstallSessionStatus.INSTALLED -> "ready"
      SplitInstallSessionStatus.CANCELING,
      SplitInstallSessionStatus.CANCELED -> "notInstalled"
      SplitInstallSessionStatus.FAILED -> {
        error = "Google Playから実行環境を取得できませんでした（${state.errorCode()}）。"
        "failed"
      }
      else -> phase
    }
  }

  @JvmStatic fun getStatus(context: Context): String {
    val current = ensureManager(context)
    removalPending = preferences(context).getBoolean("removalPending", false)
    if (removalPending && !current.installedModules.contains(MODULE)) {
      removalPending = false
      preferences(context).edit().putBoolean("removalPending", false).apply()
    }
    if (current.installedModules.contains(MODULE) && !removalPending) phase = "ready"
    return json(current)
  }

  @JvmStatic fun install(context: Context): String {
    val current = ensureManager(context)
    removalPending = false
    preferences(context).edit().putBoolean("removalPending", false).apply()
    if (current.installedModules.contains(MODULE)) {
      phase = "ready"
      removalPending = false
      return json(current)
    }
    phase = "downloading"
    error = null
    val request = SplitInstallRequest.newBuilder().addModule(MODULE).build()
    current.startInstall(request)
      .addOnSuccessListener { sessionId = it }
      .addOnFailureListener {
        phase = "failed"
        error = it.localizedMessage ?: "Google Playから実行環境を取得できませんでした。"
      }
    return json(current)
  }

  @JvmStatic fun cancel(context: Context): String {
    val current = ensureManager(context)
    if (sessionId != 0) current.cancelInstall(sessionId)
    return json(current)
  }

  @JvmStatic fun delete(context: Context): String {
    val current = ensureManager(context)
    removalPending = true
    preferences(context).edit().putBoolean("removalPending", true).apply()
    phase = "removalPending"
    current.deferredUninstall(listOf(MODULE)).addOnFailureListener {
      removalPending = false
      preferences(context).edit().putBoolean("removalPending", false).apply()
      phase = "failed"
      error = it.localizedMessage ?: "実行環境の削除を予約できませんでした。"
    }
    return json(current)
  }

  @JvmStatic fun load(context: Context): String {
    val current = ensureManager(context)
    if (!current.installedModules.contains(MODULE) || removalPending) {
      phase = if (removalPending) "removalPending" else "notInstalled"
      return json(current)
    }
    return try {
      val entry = Class.forName("jp.mutsuna.echo.localai.LocalAiRuntimeEntry")
      entry.getMethod("load", Context::class.java).invoke(null, context)
      phase = "ready"
      json(current)
    } catch (cause: Throwable) {
      phase = "failed"
      error = cause.cause?.localizedMessage ?: cause.localizedMessage ?: "実行環境を読み込めませんでした。"
      json(current)
    }
  }

  private fun ensureManager(context: Context): SplitInstallManager {
    manager?.let { return it }
    return synchronized(this) {
      manager ?: SplitInstallManagerFactory.create(context.applicationContext).also {
        it.registerListener(listener)
        manager = it
      }
    }
  }

  private fun preferences(context: Context) =
    context.applicationContext.getSharedPreferences("local_ai_runtime", Context.MODE_PRIVATE)

  private fun json(current: SplitInstallManager) = JSONObject()
    .put("state", if (removalPending) "removalPending" else phase)
    .put("installed", current.installedModules.contains(MODULE) && !removalPending)
    .put("downloadedBytes", downloaded)
    .put("totalBytes", total)
    .put("error", error ?: JSONObject.NULL)
    .toString()
}
