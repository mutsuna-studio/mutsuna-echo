package jp.mutsuna.echo

import androidx.activity.result.IntentSenderRequest
import com.google.android.gms.auth.api.identity.AuthorizationRequest
import com.google.android.gms.auth.api.identity.AuthorizationResult
import com.google.android.gms.auth.api.identity.Identity
import com.google.android.gms.common.api.Scope
import java.lang.ref.WeakReference

// Native-only authorization. Tokens never enter Tauri's WebView or event payloads.
object GoogleDriveBridge {
  private var activity = WeakReference<MainActivity>(null)
  private var generation = 0L
  private var result: String? = null
  private var pending = false

  @Synchronized fun attach(value: MainActivity) { activity = WeakReference(value) }
  @Synchronized fun detach(value: MainActivity) {
    if (activity.get() === value) {
      activity.clear()
      if (pending) { generation++; pending = false; result = "error" }
    }
  }

  @JvmStatic @Synchronized fun begin(): String? {
    generation++
    val requestGeneration = generation
    pending = true
    result = null
    val current = activity.get()
    if (current == null) { pending = false; result = "error"; return null }
    current.runOnUiThread {
      val request = AuthorizationRequest.builder()
        .setRequestedScopes(listOf(Scope("https://www.googleapis.com/auth/drive.file")))
        .build()
      Identity.getAuthorizationClient(current).authorize(request)
        .addOnSuccessListener { authorization ->
          synchronized(this) {
            if (generation != requestGeneration || !pending) return@addOnSuccessListener
            if (authorization.hasResolution()) {
              val intent = authorization.pendingIntent
              if (intent == null) { finish(requestGeneration, null) }
              else {
                current.googleDriveRequestGeneration = requestGeneration
                try { current.googleDriveAuthorization.launch(IntentSenderRequest.Builder(intent.intentSender).build()) }
                catch (_: Exception) { finish(requestGeneration, null) }
              }
            } else finish(requestGeneration, authorization)
          }
        }
        .addOnFailureListener { finish(requestGeneration, null) }
    }
    return null
  }

  @Synchronized fun finish(requestGeneration: Long, authorization: AuthorizationResult?) {
    if (generation != requestGeneration || !pending) return
    val token = authorization?.accessToken?.trim()
    result = if (!token.isNullOrEmpty()) "ok:$token" else "error"
    pending = false
  }
  @JvmStatic @Synchronized fun takeResult(): String? { val value = result; result = null; return value }
  @JvmStatic @Synchronized fun cancel(): String? { generation++; pending = false; result = null; return null }
}
