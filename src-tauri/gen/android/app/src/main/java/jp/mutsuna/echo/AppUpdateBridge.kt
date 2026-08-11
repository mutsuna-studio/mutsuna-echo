package jp.mutsuna.echo

import android.app.Activity
import android.content.Context
import com.google.android.play.core.appupdate.AppUpdateInfo
import com.google.android.play.core.appupdate.AppUpdateManager
import com.google.android.play.core.appupdate.AppUpdateManagerFactory
import com.google.android.play.core.appupdate.AppUpdateOptions
import com.google.android.play.core.install.InstallStateUpdatedListener
import com.google.android.play.core.install.model.AppUpdateType
import com.google.android.play.core.install.model.InstallStatus
import com.google.android.play.core.install.model.UpdateAvailability
import org.json.JSONObject

object AppUpdateBridge {
  private const val IMMEDIATE_PRIORITY_THRESHOLD = 4

  @Volatile private var activity: MainActivity? = null
  @Volatile private var manager: AppUpdateManager? = null
  @Volatile private var phase = "idle"
  @Volatile private var availableVersionCode: Int? = null
  @Volatile private var updatePriority = 0
  @Volatile private var flexibleAllowed = false
  @Volatile private var immediateAllowed = false
  @Volatile private var bytesDownloaded = 0L
  @Volatile private var totalBytes = 0L
  @Volatile private var error: String? = null
  @Volatile private var checking = false

  private val installListener = InstallStateUpdatedListener { state ->
    bytesDownloaded = state.bytesDownloaded()
    totalBytes = state.totalBytesToDownload()
    error = null
    phase = when (state.installStatus()) {
      InstallStatus.PENDING -> "starting"
      InstallStatus.DOWNLOADING -> "downloading"
      InstallStatus.DOWNLOADED -> "downloaded"
      InstallStatus.INSTALLING -> "installing"
      InstallStatus.INSTALLED -> "latest"
      InstallStatus.CANCELED -> "available"
      InstallStatus.FAILED -> {
        error = "更新のダウンロードに失敗しました（${state.installErrorCode()}）。"
        "failed"
      }
      else -> phase
    }
  }

  @JvmStatic
  fun attach(current: MainActivity) {
    activity = current
    ensureManager(current.applicationContext)
  }

  @JvmStatic
  fun detach(current: MainActivity) {
    if (activity === current) activity = null
  }

  @JvmStatic
  fun check(context: Context): String {
    val updateManager = ensureManager(context)
    if (checking || phase == "starting" || phase == "installing") return getStatus(context)
    checking = true
    error = null
    updateManager.appUpdateInfo
      .addOnSuccessListener { info ->
        checking = false
        applyInfo(info)
        if (info.updateAvailability() == UpdateAvailability.DEVELOPER_TRIGGERED_UPDATE_IN_PROGRESS &&
          info.isUpdateTypeAllowed(AppUpdateType.IMMEDIATE)) {
          activity?.let { startFlow(updateManager, info, it, AppUpdateType.IMMEDIATE) }
        }
      }
      .addOnFailureListener { cause ->
        checking = false
        phase = "failed"
        error = cause.localizedMessage ?: "Google Playで更新情報を確認できませんでした。"
      }
    return getStatus(context)
  }

  @JvmStatic
  fun start(context: Context): String {
    val current = activity ?: return fail("更新画面を開けません。アプリを表示してから再試行してください。", context)
    val updateManager = ensureManager(context)
    phase = "starting"
    error = null
    updateManager.appUpdateInfo
      .addOnSuccessListener { info ->
        applyInfo(info)
        val type = if (info.updatePriority() >= IMMEDIATE_PRIORITY_THRESHOLD &&
          info.isUpdateTypeAllowed(AppUpdateType.IMMEDIATE)) AppUpdateType.IMMEDIATE else AppUpdateType.FLEXIBLE
        if (!info.isUpdateTypeAllowed(type)) {
          fail("この更新方法はGoogle Playで利用できません。", context)
        } else {
          startFlow(updateManager, info, current, type)
        }
      }
      .addOnFailureListener { cause -> fail(cause.localizedMessage ?: "更新を開始できませんでした。", context) }
    return getStatus(context)
  }

  @JvmStatic
  fun complete(context: Context): String {
    val updateManager = ensureManager(context)
    phase = "installing"
    error = null
    updateManager.completeUpdate().addOnFailureListener { cause ->
      fail(cause.localizedMessage ?: "更新を完了できませんでした。", context)
    }
    return getStatus(context)
  }

  @JvmStatic
  fun getStatus(context: Context): String {
    ensureManager(context)
    return synchronized(this) {
      JSONObject()
        .put("phase", phase)
        .put("checking", checking)
        .put("availableVersionCode", availableVersionCode ?: JSONObject.NULL)
        .put("updatePriority", updatePriority)
        .put("flexibleAllowed", flexibleAllowed)
        .put("immediateAllowed", immediateAllowed)
        .put("bytesDownloaded", bytesDownloaded)
        .put("totalBytes", totalBytes)
        .put("error", error ?: JSONObject.NULL)
        .toString()
    }
  }

  private fun ensureManager(context: Context): AppUpdateManager {
    manager?.let { return it }
    return synchronized(this) {
      manager ?: AppUpdateManagerFactory.create(context.applicationContext).also {
        it.registerListener(installListener)
        manager = it
      }
    }
  }

  private fun applyInfo(info: AppUpdateInfo) {
    availableVersionCode = if (info.availableVersionCode() > 0) info.availableVersionCode() else null
    updatePriority = info.updatePriority()
    flexibleAllowed = info.isUpdateTypeAllowed(AppUpdateType.FLEXIBLE)
    immediateAllowed = info.isUpdateTypeAllowed(AppUpdateType.IMMEDIATE)
    bytesDownloaded = info.bytesDownloaded()
    totalBytes = info.totalBytesToDownload()
    error = null
    phase = when {
      info.installStatus() == InstallStatus.DOWNLOADED -> "downloaded"
      info.installStatus() == InstallStatus.DOWNLOADING -> "downloading"
      info.updateAvailability() == UpdateAvailability.UPDATE_AVAILABLE -> "available"
      info.updateAvailability() == UpdateAvailability.DEVELOPER_TRIGGERED_UPDATE_IN_PROGRESS -> "starting"
      else -> "latest"
    }
  }

  private fun startFlow(manager: AppUpdateManager, info: AppUpdateInfo, current: Activity, type: Int) {
    phase = "starting"
    manager.startUpdateFlow(info, current, AppUpdateOptions.defaultOptions(type))
      .addOnSuccessListener { result ->
        if (result != Activity.RESULT_OK && phase == "starting") phase = "available"
      }
      .addOnFailureListener { cause ->
        fail(cause.localizedMessage ?: "Google Playの更新画面を開けませんでした。", current)
      }
  }

  private fun fail(message: String, context: Context): String {
    phase = "failed"
    error = message
    return getStatus(context)
  }
}
