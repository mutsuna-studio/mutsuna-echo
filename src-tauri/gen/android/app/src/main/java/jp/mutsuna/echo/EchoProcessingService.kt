package jp.mutsuna.echo

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat

class EchoProcessingService : Service() {
  private var wakeLock: PowerManager.WakeLock? = null
  private var screenOnRegistered = false

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    when (intent?.action) {
      ACTION_RELEASE -> stopProcessing()
      ACTION_ACQUIRE -> startProcessing(intent.getStringExtra(EXTRA_REASON) ?: "処理中")
    }
    return START_NOT_STICKY
  }

  override fun onDestroy() {
    unregisterScreenOn()
    releaseWakeLock()
    super.onDestroy()
  }

  private fun startProcessing(reason: String) {
    createChannel()
    val openIntent = PendingIntent.getActivity(
      this,
      0,
      Intent(this, MainActivity::class.java).addFlags(
        Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
      ),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    )
    val notification = NotificationCompat.Builder(this, CHANNEL)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setContentTitle("Mutsuna Echoで処理中")
      .setContentText(reason)
      .setOngoing(true)
      .setContentIntent(openIntent)
      .build()
    if (Build.VERSION.SDK_INT >= 29) {
      startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
    } else {
      startForeground(NOTIFICATION_ID, notification)
    }
    if (wakeLock?.isHeld != true) {
      wakeLock = getSystemService(PowerManager::class.java)
        .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "mutsuna-echo:processing")
        .apply { acquire() }
    }
    if (!screenOnRegistered) {
      ScreenOnController.begin(applicationContext)
      screenOnRegistered = true
    }
  }

  private fun stopProcessing() {
    unregisterScreenOn()
    releaseWakeLock()
    stopForeground(STOP_FOREGROUND_REMOVE)
    stopSelf()
  }

  private fun releaseWakeLock() {
    wakeLock?.let { if (it.isHeld) it.release() }
    wakeLock = null
  }

  private fun unregisterScreenOn() {
    if (!screenOnRegistered) return
    screenOnRegistered = false
    ScreenOnController.end(applicationContext)
  }

  private fun createChannel() {
    getSystemService(NotificationManager::class.java).createNotificationChannel(
      NotificationChannel(CHANNEL, "文字起こしと会議ノート", NotificationManager.IMPORTANCE_LOW)
    )
  }

  companion object {
    private const val ACTION_ACQUIRE = "jp.mutsuna.echo.ACQUIRE_PROCESSING_POWER"
    private const val ACTION_RELEASE = "jp.mutsuna.echo.RELEASE_PROCESSING_POWER"
    private const val EXTRA_REASON = "reason"
    private const val CHANNEL = "processing"
    private const val NOTIFICATION_ID = 42

    fun acquire(context: Context, reason: String) {
      val intent = Intent(context, EchoProcessingService::class.java)
        .setAction(ACTION_ACQUIRE)
        .putExtra(EXTRA_REASON, reason)
      ContextCompat.startForegroundService(context, intent)
    }

    fun release(context: Context) {
      context.startService(
        Intent(context, EchoProcessingService::class.java).setAction(ACTION_RELEASE)
      )
    }
  }
}

class ProcessingPowerBridge {
  companion object {
    @JvmStatic fun acquire(context: Context, reason: String) {
      EchoProcessingService.acquire(context, reason)
    }

    @JvmStatic fun release(context: Context) {
      EchoProcessingService.release(context)
    }

    @JvmStatic fun setDisplayRequired(context: Context, required: Boolean) {
      ScreenOnController.setEnabled(context, required)
    }
  }
}
