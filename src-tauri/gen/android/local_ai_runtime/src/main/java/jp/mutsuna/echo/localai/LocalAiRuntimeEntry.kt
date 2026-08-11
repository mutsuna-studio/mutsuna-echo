package jp.mutsuna.echo.localai

import android.content.Context
import com.google.android.play.core.splitinstall.SplitInstallHelper

object LocalAiRuntimeEntry {
  const val PROTOCOL_VERSION = 1
  const val RUNTIME_VERSION = "1.13.4-1"

  @JvmStatic fun load(context: Context) {
    SplitInstallHelper.loadLibrary(context, "onnxruntime")
    SplitInstallHelper.loadLibrary(context, "sherpa-onnx-c-api")
  }
}
