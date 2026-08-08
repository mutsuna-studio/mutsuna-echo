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

class MainActivity : TauriActivity() {
  private var pendingConfig: String? = null
  private val projectionConsent = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
    val config = pendingConfig.also { pendingConfig = null }
    if (config == null || result.resultCode != Activity.RESULT_OK || result.data == null) {
      RecordingBridge.failStart("画面共有が許可されなかったため、システム音声を録音できません。")
      return@registerForActivityResult
    }
    EchoRecordingService.start(this, config, result.resultCode, result.data!!)
  }

  private val microphonePermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
    if (!granted) RecordingBridge.failStart("マイクの録音権限が許可されていません。")
    else beginPendingRecording()
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    RecordingBridge.activity = this
  }

  override fun onDestroy() {
    if (RecordingBridge.activity === this) RecordingBridge.activity = null
    super.onDestroy()
  }

  override fun onStop() {
    super.onStop()
    if (!isChangingConfigurations && pendingConfig == null && RecordingBridge.isActive()) {
      // 録音はForeground Serviceが所有するため、非表示のWebViewを保持しない。
      finishAndRemoveTask()
    }
  }

  fun requestRecording(config: String) {
    runOnUiThread {
      pendingConfig = config
      if (ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
        microphonePermission.launch(Manifest.permission.RECORD_AUDIO)
      } else beginPendingRecording()
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
