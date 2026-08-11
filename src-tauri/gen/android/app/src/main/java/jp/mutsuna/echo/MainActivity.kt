package jp.mutsuna.echo

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.media.projection.MediaProjectionManager
import android.os.Bundle
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.enableEdgeToEdge
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat

class MainActivity : TauriActivity() {
  private external fun initializeAndroidContext(context: android.content.Context)

  private var pendingConfig: String? = null
  private var pendingInputMonitor = false
  private val projectionConsent = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
    val config = pendingConfig.also { pendingConfig = null }
    if (config == null || result.resultCode != Activity.RESULT_OK || result.data == null) {
      RecordingBridge.failStart("画面共有が許可されなかったため、システム音声を録音できません。")
      return@registerForActivityResult
    }
    EchoRecordingService.start(this, config, result.resultCode, result.data!!)
  }

  private val microphonePermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
    if (pendingInputMonitor) {
      pendingInputMonitor = false
      if (granted) EchoInputMonitor.start()
      else RecordingBridge.failMonitor("マイクの権限がないため入力レベルを確認できません。")
    } else if (!granted) RecordingBridge.failStart("マイクの録音権限が許可されていません。")
    else beginPendingRecording()
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    initializeAndroidContext(applicationContext)
    enableEdgeToEdge()
    WindowCompat.getInsetsController(window, window.decorView).apply {
      isAppearanceLightStatusBars = true
      isAppearanceLightNavigationBars = true
    }
    RecordingBridge.activity = this
    AppUpdateBridge.attach(this)
    AppUpdateBridge.check(applicationContext)
  }

  override fun onDestroy() {
    if (RecordingBridge.activity === this) RecordingBridge.activity = null
    AppUpdateBridge.detach(this)
    EchoInputMonitor.stop()
    AudioPlaybackBridge.release(applicationContext)
    super.onDestroy()
  }

  override fun onStart() {
    super.onStart()
    RecordingBridge.resumeInputMonitor()
  }

  override fun onResume() {
    super.onResume()
    AppUpdateBridge.check(applicationContext)
  }

  override fun onStop() {
    RecordingBridge.pauseInputMonitor()
    super.onStop()
  }

  fun requestRecording(config: String) {
    runOnUiThread {
      pendingConfig = config
      if (ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
        microphonePermission.launch(Manifest.permission.RECORD_AUDIO)
      } else beginPendingRecording()
    }
  }

  fun requestInputMonitor() {
    runOnUiThread {
      if (ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
        EchoInputMonitor.start()
      } else {
        pendingInputMonitor = true
        microphonePermission.launch(Manifest.permission.RECORD_AUDIO)
      }
    }
  }

  private fun beginPendingRecording() {
    val config = pendingConfig ?: return
    if (RecordingBridge.requiresSystemAudio(config)) {
      val manager = getSystemService(MediaProjectionManager::class.java)
      projectionConsent.launch(manager.createScreenCaptureIntent())
    } else {
      pendingConfig = null
      EchoRecordingService.start(this, config, Activity.RESULT_CANCELED, Intent())
    }
  }
}
