package jp.mutsuna.echo

import android.content.Context
import android.content.ContentUris
import android.content.ContentValues
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.provider.MediaStore
import android.provider.DocumentsContract
import org.json.JSONObject
import org.json.JSONArray

object RecordingBridge {
  private const val HISTORY_RELATIVE_PATH = "Music/Mutsuna Echo/"
  @Volatile internal var activity: MainActivity? = null
  private val lock = Any()
  private var status = JSONObject(defaultStatus())
  @Volatile private var microphoneMonitorRequested = false

  @JvmStatic fun capabilities(@Suppress("UNUSED_PARAMETER") context: Context): String = JSONObject().apply {
    put("platform", "android")
    put("supported", true)
    put("microphoneSupported", true)
    put("systemAudioSupported", true)
    put("systemAudioLimited", true)
    put("limitation", "Androidでは、再生元アプリが録音を許可した音声だけ取得できます。通話・DRM保護音声などは取得できません。また、他アプリがマイクを占有すると録音が停止することがあります。")
    put("microphoneDevices", org.json.JSONArray())
    put("systemDevices", org.json.JSONArray())
    put("sampleRate", 48_000)
    put("channels", 1)
    put("codec", "AAC-LC")
    put("bitrate", 64_000)
    put("maxDurationMs", 36_000_000L)
  }.toString()

  @JvmStatic fun start(@Suppress("UNUSED_PARAMETER") context: Context, config: String): String {
    EchoInputMonitor.stop()
    synchronized(lock) {
      if (status.optString("phase") in setOf("starting", "recording", "finalizing")) {
        throw IllegalStateException("録音はすでに実行中です。")
      }
      status = JSONObject(defaultStatus()).apply {
        put("phase", "starting")
        put("microphone", JSONObject(config).optBoolean("microphone"))
        put("systemAudio", JSONObject(config).optBoolean("systemAudio"))
      }
    }
    val current = activity ?: throw IllegalStateException("録音を開始するにはMutsuna Echoを前面に表示してください。")
    current.requestRecording(config)
    return getStatus(context)
  }

  @JvmStatic fun stop(context: Context, cancel: Boolean): String {
    val intent = Intent(context, EchoRecordingService::class.java).setAction(if (cancel) EchoRecordingService.ACTION_CANCEL else EchoRecordingService.ACTION_STOP)
    context.startService(intent)
    return getStatus(context)
  }

  @JvmStatic fun getStatus(@Suppress("UNUSED_PARAMETER") context: Context): String = synchronized(lock) { status.toString() }

  @JvmStatic fun clearCompletedStatus(@Suppress("UNUSED_PARAMETER") context: Context) = synchronized(lock) {
    if (status.optString("phase") !in setOf("starting", "recording", "finalizing")) {
      status = JSONObject(defaultStatus())
    }
  }

  @JvmStatic fun copyContentUri(context: Context, value: String): String {
    val uri = Uri.parse(value)
    require(uri.scheme == "content") { "Androidのcontent URIではありません。" }
    var displayName = "selected-audio"
    context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
      if (cursor.moveToFirst()) displayName = cursor.getString(0) ?: displayName
    }
    displayName = displayName.substringAfterLast('/').replace(Regex("[^A-Za-z0-9._-]"), "_")
    val directory = java.io.File(context.cacheDir, "imports").apply { mkdirs() }
    var destination = java.io.File(directory, displayName)
    var suffix = 2
    val extension = destination.extension.let { if (it.isEmpty()) "" else ".$it" }
    val stem = destination.name.removeSuffix(extension)
    while (destination.exists()) destination = java.io.File(directory, "${stem}_${suffix++}$extension")
    val temporary = java.io.File(directory, destination.name + ".partial")
    context.contentResolver.openInputStream(uri)?.use { input ->
      java.io.FileOutputStream(temporary).use { output -> input.copyTo(output); output.fd.sync() }
    } ?: throw IllegalStateException("選択した音声を開けませんでした。")
    if (!temporary.renameTo(destination)) throw IllegalStateException("選択した音声をアプリ領域へ確定できませんでした。")
    return destination.absolutePath
  }

  @JvmStatic fun listCompletedRecordings(context: Context): String {
    val recordings = JSONArray()
    val projection = arrayOf(
      MediaStore.Audio.Media._ID,
      MediaStore.Audio.Media.DISPLAY_NAME,
      MediaStore.Audio.Media.SIZE,
      MediaStore.Audio.Media.DATE_MODIFIED
    )
    val selection = "${MediaStore.Audio.Media.RELATIVE_PATH}=? AND ${MediaStore.Audio.Media.MIME_TYPE}=?"
    context.contentResolver.query(
      MediaStore.Audio.Media.EXTERNAL_CONTENT_URI,
      projection,
      selection,
      arrayOf(HISTORY_RELATIVE_PATH, "audio/mp4"),
      "${MediaStore.Audio.Media.DATE_MODIFIED} DESC"
    )?.use { cursor ->
      val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media._ID)
      val nameColumn = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.DISPLAY_NAME)
      val sizeColumn = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.SIZE)
      val modifiedColumn = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.DATE_MODIFIED)
      while (cursor.moveToNext() && recordings.length() < 100) {
        val uri = ContentUris.withAppendedId(
          MediaStore.Audio.Media.EXTERNAL_CONTENT_URI,
          cursor.getLong(idColumn)
        )
        recordings.put(JSONObject().apply {
          put("id", uri.toString())
          put("meetingId", meetingIdForRecording(context, uri.toString()))
          put("fileName", cursor.getString(nameColumn))
          put("sizeBytes", cursor.getLong(sizeColumn))
          put("recordedAtUnixMs", cursor.getLong(modifiedColumn) * 1_000L)
        })
      }
    }
    return recordings.toString()
  }

  @JvmStatic fun openRecordingFolder(context: Context) {
    val folderUri = DocumentsContract.buildDocumentUri(
      "com.android.externalstorage.documents",
      "primary:Music/Mutsuna Echo"
    )
    val browseAudio = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
      addCategory(Intent.CATEGORY_OPENABLE)
      type = "audio/*"
      putExtra(DocumentsContract.EXTRA_INITIAL_URI, folderUri)
      addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    context.startActivity(browseAudio)
  }

  @JvmStatic fun renameCompletedRecording(context: Context, value: String, newFileName: String) {
    val uri = Uri.parse(value)
    require(uri.scheme == "content" && uri.authority == MediaStore.AUTHORITY) {
      "録音履歴のIDが不正です。"
    }
    require(Regex("^[^<>:\"/\\|?*\\p{Cntrl}]{1,128}\\.m4a$", RegexOption.IGNORE_CASE).matches(newFileName)) {
      "ファイル名には使用できない文字が含まれています。"
    }
    val projection = arrayOf(MediaStore.Audio.Media.RELATIVE_PATH, MediaStore.Audio.Media.MIME_TYPE)
    var ownedRecording = false
    context.contentResolver.query(uri, projection, null, null, null)?.use { cursor ->
      if (cursor.moveToFirst()) {
        ownedRecording = cursor.getString(cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.RELATIVE_PATH)) == HISTORY_RELATIVE_PATH &&
          cursor.getString(cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.MIME_TYPE)) == "audio/mp4"
      }
    }
    require(ownedRecording) { "Mutsuna Echoの録音ファイルではありません。" }
    val updated = context.contentResolver.update(
      uri,
      ContentValues().apply { put(MediaStore.Audio.Media.DISPLAY_NAME, newFileName) },
      null,
      null
    )
    check(updated == 1) { "録音ファイル名を変更できませんでした。" }
  }

  @JvmStatic fun deleteCompletedRecording(context: Context, value: String) {
    val uri = Uri.parse(value)
    require(uri.scheme == "content" && uri.authority == MediaStore.AUTHORITY) {
      "録音履歴のIDが不正です。"
    }
    val projection = arrayOf(MediaStore.Audio.Media.RELATIVE_PATH, MediaStore.Audio.Media.MIME_TYPE)
    var ownedRecording = false
    context.contentResolver.query(uri, projection, null, null, null)?.use { cursor ->
      if (cursor.moveToFirst()) {
        ownedRecording = cursor.getString(cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.RELATIVE_PATH)) == HISTORY_RELATIVE_PATH &&
          cursor.getString(cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.MIME_TYPE)) == "audio/mp4"
      }
    }
    require(ownedRecording) { "Mutsuna Echoの録音ファイルではありません。" }
    check(context.contentResolver.delete(uri, null, null) == 1) {
      "録音ファイルを削除できませんでした。"
    }
    check(context.getSharedPreferences("mutsuna_echo_meetings", Context.MODE_PRIVATE)
      .edit().remove("recording:$value").commit()) {
      "録音ファイルのMeeting情報を削除できませんでした。"
    }
  }

  @JvmStatic fun startMonitor(context: Context, config: String): String {
    val parsed = JSONObject(config)
    microphoneMonitorRequested = parsed.optBoolean("microphone")
    synchronized(lock) {
      if (status.optString("phase") in setOf("starting", "recording", "finalizing")) {
        return status.toString()
      }
      status.put("microphone", parsed.optBoolean("microphone"))
      status.put("systemAudio", parsed.optBoolean("systemAudio"))
      status.put("microphoneLevel", 0.0)
      status.put("systemLevel", 0.0)
      status.put("warning", JSONObject.NULL)
      status.put("error", JSONObject.NULL)
    }
    if (parsed.optBoolean("microphone") || parsed.optBoolean("systemAudio")) {
      val current = activity ?: throw IllegalStateException("入力を確認するにはMutsuna Echoを前面に表示してください。")
      current.requestInputMonitor(parsed.optBoolean("microphone"), parsed.optBoolean("systemAudio"))
    }
    return getStatus(context)
  }

  @JvmStatic fun stopMonitor(context: Context) {
    microphoneMonitorRequested = false
    EchoInputMonitor.stop()
    EchoRecordingService.stopMonitor(context)
    update { put("microphoneLevel", 0.0); put("systemLevel", 0.0) }
  }

  private fun meetingIdForRecording(context: Context, recordingId: String): String {
    val preferences = context.getSharedPreferences("mutsuna_echo_meetings", Context.MODE_PRIVATE)
    val key = "recording:$recordingId"
    preferences.getString(key, null)?.let { return it }
    val meetingId = newMeetingId()
    check(preferences.edit().putString(key, meetingId).commit()) {
      "Meeting IDを端末へ保存できませんでした。"
    }
    return meetingId
  }

  private fun newMeetingId(): String {
    val random = java.security.SecureRandom()
    val timestamp = System.currentTimeMillis() and 0x0000ffffffffffffL
    val mostSignificant = (timestamp shl 16) or 0x7000L or (random.nextLong() and 0x0fffL)
    val leastSignificant = (random.nextLong() and 0x3fffffffffffffffL) or Long.MIN_VALUE
    return java.util.UUID(mostSignificant, leastSignificant).toString()
  }

  @JvmStatic fun copyCompletedRecording(context: Context, value: String): String {
    val uri = Uri.parse(value)
    require(uri.scheme == "content" && uri.authority == MediaStore.AUTHORITY) {
      "録音履歴のIDが不正です。"
    }
    val projection = arrayOf(
      MediaStore.Audio.Media.DISPLAY_NAME,
      MediaStore.Audio.Media.RELATIVE_PATH,
      MediaStore.Audio.Media.MIME_TYPE
    )
    var displayName: String? = null
    context.contentResolver.query(uri, projection, null, null, null)?.use { cursor ->
      if (cursor.moveToFirst()) {
        val relativePath = cursor.getString(cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.RELATIVE_PATH))
        val mimeType = cursor.getString(cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.MIME_TYPE))
        require(relativePath == HISTORY_RELATIVE_PATH && mimeType == "audio/mp4") {
          "Mutsuna Echoの録音ファイルではありません。"
        }
        displayName = cursor.getString(cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.DISPLAY_NAME))
      }
    }
    val safeName = requireNotNull(displayName) { "選択した録音ファイルが見つかりません。" }
      .substringAfterLast('/').replace(Regex("[^A-Za-z0-9._-]"), "_")
    require(safeName.lowercase().endsWith(".m4a")) { "録音ファイルの形式が不正です。" }
    val recordingKey = requireNotNull(uri.lastPathSegment)
      .replace(Regex("[^A-Za-z0-9._-]"), "_")
    require(recordingKey.isNotBlank()) { "録音履歴のIDが不正です。" }
    val directory = java.io.File(context.cacheDir, "imports/recording-$recordingKey").apply { mkdirs() }
    val destination = java.io.File(directory, safeName)
    if (destination.isFile && destination.length() > 0) return destination.absolutePath
    val temporary = java.io.File(directory, ".${destination.name}.fragmented")
    context.contentResolver.openInputStream(uri)?.use { input ->
      java.io.FileOutputStream(temporary).use { output -> input.copyTo(output); output.fd.sync() }
    } ?: throw IllegalStateException("選択した録音を開けませんでした。")
    try {
      M4aFinalizer.finalize(temporary, destination)
    } finally {
      temporary.delete()
    }
    return destination.absolutePath
  }

  @JvmStatic fun recover(context: Context, sessionId: String): String {
    require(Regex("^[A-Za-z0-9-]+$").matches(sessionId)) { "録音セッションIDが不正です。" }
    val directory = java.io.File(context.filesDir, "recordings/in-progress/$sessionId")
    val primaryManifest = java.io.File(directory, "recording.json")
    val manifestFile = if (primaryManifest.exists()) primaryManifest else java.io.File(directory, "recording.json.backup")
    val manifest = JSONObject(manifestFile.readText())
    val source = java.io.File(manifest.getString("mixedFile"))
    require(source.isFile && source.length() > 0) { "復旧可能なM4Aフラグメントがありません。" }
    val outputDirectory = java.io.File(context.cacheDir, "recordings").apply { mkdirs() }
    val base = java.time.LocalDateTime.now().format(java.time.format.DateTimeFormatter.ofPattern("yyyy-MM-dd_HH-mm-ss"))
    var output = java.io.File(outputDirectory, "$base.m4a")
    var suffix = 2
    while (output.exists()) output = java.io.File(outputDirectory, "${base}_${suffix++}.m4a")
    M4aFinalizer.finalize(source, output)
    val values = ContentValues().apply {
      put(MediaStore.Audio.Media.DISPLAY_NAME, output.name); put(MediaStore.Audio.Media.MIME_TYPE, "audio/mp4")
      put(MediaStore.Audio.Media.RELATIVE_PATH, "Music/Mutsuna Echo"); put(MediaStore.Audio.Media.IS_PENDING, 1)
    }
    val uri = context.contentResolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values)
      ?: throw IllegalStateException("Musicへの保存先を作成できませんでした。")
    try {
      context.contentResolver.openOutputStream(uri)?.use { sink -> output.inputStream().use { it.copyTo(sink) } }
        ?: throw IllegalStateException("Musicへ録音を書き込めませんでした。")
      context.contentResolver.update(uri, ContentValues().apply { put(MediaStore.Audio.Media.IS_PENDING, 0) }, null, null)
    } catch (error: Throwable) { context.contentResolver.delete(uri, null, null); throw error }
    val trackDirectory = java.io.File(context.cacheDir, "recording-tracks/$sessionId").apply { mkdirs() }
    val microphoneSource = manifest.optString("microphoneFile")
      .takeIf { manifest.optBoolean("microphone") && it.isNotBlank() && it != "null" }
      ?.let { java.io.File(it) }
    val systemSource = manifest.optString("systemFile")
      .takeIf { manifest.optBoolean("systemAudio") && it.isNotBlank() && it != "null" }
      ?.let { java.io.File(it) }
    val microphoneTrack = microphoneSource?.takeIf { it.isFile && it.length() > 0 }?.let {
      java.io.File(trackDirectory, "microphone.m4a").also { output -> M4aFinalizer.finalize(it, output) }
    }
    val systemTrack = systemSource?.takeIf { it.isFile && it.length() > 0 }?.let {
      java.io.File(trackDirectory, "system.m4a").also { output -> M4aFinalizer.finalize(it, output) }
    }
    check(directory.deleteRecursively()) { "復旧後の一時データを削除できませんでした。" }
    return JSONObject().apply {
      put("path", output.absolutePath)
      put("microphoneTrackPath", microphoneTrack?.absolutePath ?: JSONObject.NULL)
      put("systemTrackPath", systemTrack?.absolutePath ?: JSONObject.NULL)
    }.toString()
  }

  internal fun update(block: JSONObject.() -> Unit) = synchronized(lock) { status.apply(block) }

  internal fun updateMonitorLevel(level: Double) = update {
    if (optString("phase") == "idle") put("microphoneLevel", level)
  }

  internal fun updateSystemMonitorLevel(level: Double) = update {
    if (optString("phase") == "idle") put("systemLevel", level)
  }

  internal fun failMonitor(message: String) = update {
    if (optString("phase") == "idle") put("warning", message)
  }

  internal fun pauseInputMonitor() = EchoInputMonitor.stop()

  internal fun resumeInputMonitor() {
    if (microphoneMonitorRequested) EchoInputMonitor.start()
  }

  internal fun failStart(message: String) = update {
    put("phase", "failed")
    put("error", message)
    put("stopReason", "captureError")
  }

  internal fun requiresSystemAudio(config: String): Boolean = JSONObject(config).optBoolean("systemAudio")

  private fun defaultStatus() = """{
    "phase":"idle","sessionId":null,"elapsedMs":0,"microphone":false,"systemAudio":false,
    "microphoneLevel":0.0,"systemLevel":0.0,"outputPath":null,"microphoneTrackPath":null,"systemTrackPath":null,"stopReason":null,"warning":null,"error":null
  }""".trimIndent()
}
