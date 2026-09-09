#[cfg(any(target_os = "android", test))]
#[cfg_attr(test, allow(dead_code))]
mod android;
mod audio;
pub(crate) mod auth;
mod document;
mod limits;
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{AppHandle, Emitter};

#[derive(Default)]
pub(crate) struct State {
    pub lock: tokio::sync::Mutex<()>,
}

pub(super) fn client() -> Result<Client, String> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|_| "Googleへの接続を準備できません。".into())
}

fn network_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "Googleへの接続がタイムアウトしました。再試行してください。"
    } else {
        "Googleへ接続できません。ネットワーク・DNS・TLSの設定を確認してください。"
    }
    .into()
}

async fn response(result: Result<Response, reqwest::Error>) -> Result<Response, String> {
    response_value(limits::check(result.map_err(network_error)?, &limits::LIMITS).await?).await
}

async fn response_value(response: Response) -> Result<Response, String> {
    if response.status().is_success() {
        return Ok(response);
    }
    Err(match response.status().as_u16() {
        401 => "Googleとの再連携が必要です。",
        403 => "Google Driveへの書き込み権限がありません。APIの有効化、アクセス許可、空き容量を確認してください。",
        404 => "保存先が削除されたか、アクセスできません。別版として保存してください。",
        409 | 412 => "保存先が変更されています。内容を確認して再試行してください。",
        429 => "Googleの利用制限に達しました。しばらく待って再試行してください。",
        500..=599 => "Google側で一時的なエラーが発生しました。再試行してください。",
        _ => "Googleへのリクエストが拒否されました。連携設定と保存内容を確認してください。",
    }.into())
}

pub(super) async fn json_response(
    result: Result<Response, reqwest::Error>,
) -> Result<Value, String> {
    response(result)
        .await?
        .json()
        .await
        .map_err(|_| "Googleからの応答を読み取れませんでした。".into())
}

struct Api {
    client: Client,
    token: String,
}
impl Api {
    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, String> {
        self.read_json(
            self.client
                .get(format!("https://www.googleapis.com/drive/v3/{path}"))
                .bearer_auth(&self.token)
                .query(query),
        )
        .await
    }
    async fn read_json(&self, request: reqwest::RequestBuilder) -> Result<Value, String> {
        response_value(limits::send(request).await?)
            .await?
            .json()
            .await
            .map_err(|_| "Googleからの応答を読み取れませんでした。".into())
    }
    async fn create(&self, body: Value) -> Result<Value, String> {
        limits::LIMITS.check_quota()?;
        json_response(
            self.client
                .post("https://www.googleapis.com/drive/v3/files")
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await,
        )
        .await
    }
    async fn docs(&self, id: &str) -> Result<Value, String> {
        self.read_json(
            self.client
                .get(format!("https://docs.googleapis.com/v1/documents/{id}"))
                .query(&[("includeTabsContent", "true")])
                .bearer_auth(&self.token),
        )
        .await
    }
    async fn batch(&self, id: &str, body: Value) -> Result<Value, String> {
        limits::LIMITS.check_quota()?;
        json_response(
            self.client
                .post(format!(
                    "https://docs.googleapis.com/v1/documents/{id}:batchUpdate"
                ))
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await,
        )
        .await
    }
    async fn ensure_file(
        &self,
        key: &str,
        name: &str,
        mime: &str,
        parent: Option<&str>,
    ) -> Result<String, String> {
        // Keys are native-generated UUIDs plus fixed ASCII suffixes. Searching
        // before creating also recovers an acknowledged-lost files.create.
        let q = format!(
            "trashed = false and appProperties has {{ key='echoExport' and value='{key}' }}"
        );
        let files = self
            .get(
                "files",
                &[("q", &q), ("fields", "files(id)"), ("pageSize", "100")],
            )
            .await?;
        if let Some(id) = files["files"][0]["id"].as_str() {
            return valid_id(id);
        }
        let mut body = json!({"name":name,"mimeType":mime,"appProperties":{"echoExport":key}});
        if let Some(parent) = parent {
            body["parents"] = json!([parent]);
        }
        let file = self.create(body).await?;
        valid_id(
            file["id"]
                .as_str()
                .ok_or("DriveのファイルIDを取得できません。")?,
        )
    }
}

fn valid_id(id: &str) -> Result<String, String> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Driveの識別子が不正です。".into());
    }
    Ok(id.into())
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Export {
    version: String,
    folder: Option<String>,
    document: Option<String>,
    note_tab: Option<String>,
    transcript_tab: Option<String>,
    audio: Option<String>,
    backup: Option<String>,
    completed_at: Option<String>,
    audio_hash: Option<String>,
    document_hash: Option<String>,
    backup_hash: Option<String>,
}

fn state_path(app: &AppHandle, meeting: &str, account: &str) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    Ok(crate::meeting_store::meeting_directory(app, meeting)?
        .join(format!("drive-{:x}.json", Sha256::digest(account))))
}
fn read_export(path: &Path) -> Result<Option<Export>, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let value: Export =
                serde_json::from_slice(&bytes).map_err(|_| "Driveの保存履歴を読み取れません。")?;
            uuid::Uuid::parse_str(&value.version).map_err(|_| "Driveの版情報が不正です。")?;
            for id in [&value.folder, &value.document, &value.audio, &value.backup]
                .into_iter()
                .flatten()
            {
                valid_id(id)?;
            }
            Ok(Some(value))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Driveの保存履歴を読み取れません。".into()),
    }
}
fn write_export(path: &Path, value: &Export) -> Result<(), String> {
    use std::io::Write;
    let bytes = serde_json::to_vec(value).map_err(|_| "Driveの保存履歴を作成できません。")?;
    let temp = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&temp).map_err(|_| "Driveの保存履歴を書き込めません。")?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "Driveの保存履歴を書き込めません。")?;
    std::fs::rename(temp, path).map_err(|_| "Driveの保存履歴を確定できません。".into())
}

fn content_hash(value: &impl Serialize) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(value).map_err(|_| "保存内容を確認できません。")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn matches_saved(saved: Option<&String>, current: Option<&String>) -> bool {
    saved.is_some() && saved == current
}
#[derive(Default, Serialize)]
struct Unchanged {
    audio: bool,
    document: bool,
    backup: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedState {
    exists: bool,
    version: Option<String>,
    url: Option<String>,
    completed_at: Option<String>,
    audio_size: Option<u64>,
    unchanged: Unchanged,
    limits: limits::Status,
}
#[tauri::command]
pub(crate) async fn google_drive_saved_state(
    app: AppHandle,
    meeting_id: String,
) -> Result<SavedState, String> {
    tauri::async_runtime::spawn_blocking(move || saved_state(&app, &meeting_id))
        .await
        .map_err(|_| "保存内容の確認に失敗しました。")?
}
fn saved_state(app: &AppHandle, meeting_id: &str) -> Result<SavedState, String> {
    let session = auth::load(app)?;
    let saved = read_export(&state_path(app, meeting_id, &session.account)?)?;
    let mut unchanged = Unchanged::default();
    if let Some(ref previous) = saved {
        let title = crate::meeting_store::meeting_title(app, meeting_id)?;
        let transcript = crate::transcript_store::selected_summary_snapshot(app, meeting_id)?;
        let note = crate::meeting_schema::selected(app, meeting_id)?;
        let doc_hash = content_hash(&(
            "pageless-v1",
            document::notes(&title, note.as_ref()),
            document::transcript(&title, transcript.as_ref()),
        ))?;
        let backup_hash = content_hash(
            &json!({"schemaVersion":1,"meetingId":meeting_id,"title":title,"transcript":transcript,"meetingDocument":note}),
        )?;
        unchanged.document = matches_saved(previous.document_hash.as_ref(), Some(&doc_hash));
        unchanged.backup = matches_saved(previous.backup_hash.as_ref(), Some(&backup_hash));
        if previous.audio_hash.is_some() {
            let fingerprint = crate::meeting_store::local_audio_path(app, meeting_id)
                .ok()
                .and_then(|p| audio::fingerprint(&p).ok());
            unchanged.audio = matches_saved(
                previous.audio_hash.as_ref(),
                fingerprint.as_ref().map(|f| &f.sha256),
            );
        }
    }
    Ok(SavedState {
        unchanged,
        audio_size: crate::meeting_store::local_audio_path(app, meeting_id)
            .ok()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len()),
        limits: limits::LIMITS.status(limits::now()),
        exists: saved.is_some(),
        version: saved.as_ref().map(|v| v.version.clone()),
        url: saved
            .as_ref()
            .and_then(|v| v.folder.as_ref())
            .map(|id| format!("https://drive.google.com/drive/folders/{id}")),
        completed_at: saved.and_then(|v| v.completed_at),
    })
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SaveMode {
    Create,
    Overwrite,
    NewVersion,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveRequest {
    meeting_id: String,
    mode: SaveMode,
    expected_version: Option<String>,
    include_audio: bool,
    #[serde(default)]
    selected_audio_size: Option<u64>,
    include_document: bool,
    include_backup: bool,
}

fn progress(app: &AppHandle, meeting: &str, stage: &str, percent: u64) {
    let _ = app.emit(
        "google-drive-progress",
        json!({"meetingId":meeting,"stage":stage,"percent":percent}),
    );
}

fn validate_save_choice(
    current: Option<&str>,
    expected: Option<&str>,
    mode: &SaveMode,
) -> Result<(), String> {
    if current != expected {
        return Err(
            "保存先のアカウントまたは版が変更されています。保存画面を開き直してください。".into(),
        );
    }
    match (current, mode) {
        (Some(_), SaveMode::Create) => {
            Err("保存履歴があります。上書きまたは別版保存を選択してください。".into())
        }
        (None, SaveMode::Overwrite) => Err("保存履歴がありません。新規保存してください。".into()),
        _ => Ok(()),
    }
}

#[tauri::command]
pub(crate) async fn google_drive_save(
    app: AppHandle,
    state: tauri::State<'_, State>,
    request: SaveRequest,
) -> Result<SavedState, String> {
    let _guard = state
        .lock
        .try_lock()
        .map_err(|_| "Google Driveの処理が進行中です。")?;
    limits::LIMITS.check_save()?;
    let meeting = &request.meeting_id;
    crate::meeting_store::validate_meeting_id(meeting)?;
    if crate::meeting_jobs::active_kind(meeting).is_some() {
        return Err("会議の処理が完了してから保存してください。".into());
    }
    if !request.include_audio && !request.include_document && !request.include_backup {
        return Err("保存する内容を選択してください。".into());
    }
    let title = crate::meeting_store::meeting_title(&app, meeting)?;
    let transcript = crate::transcript_store::selected_summary_snapshot(&app, meeting)?;
    let note = crate::meeting_schema::selected(&app, meeting)?;
    let audio = if request.include_audio {
        let path = crate::meeting_store::local_audio_path(&app, meeting)?;
        let size = std::fs::metadata(&path)
            .map_err(|_| "音声の容量を確認できません。")?
            .len();
        audio::validate_selection(size, request.selected_audio_size)?;
        Some(path)
    } else {
        None
    };
    if request.include_document && transcript.is_none() && note.is_none() {
        return Err("保存する文字起こし・会議ノートがありません。".into());
    }
    progress(&app, meeting, "Googleアカウントを確認中", 0);
    let session = auth::access(&app).await?;
    let path = state_path(&app, meeting, &session.account)?;
    let previous = read_export(&path)?;
    validate_save_choice(
        previous.as_ref().map(|v| v.version.as_str()),
        request.expected_version.as_deref(),
        &request.mode,
    )?;
    let mut export = if request.mode == SaveMode::Overwrite {
        previous.unwrap_or_default()
    } else {
        // Preserve the previous version's mapping before switching the active export.
        if let Some(ref old) = previous {
            write_export(&path.with_extension(format!("{}.json", old.version)), old)?;
        }
        Export {
            version: uuid::Uuid::now_v7().to_string(),
            ..Export::default()
        }
    };
    export.completed_at = None;
    write_export(&path, &export)?;
    let api = Api {
        client: client()?,
        token: session.access_token,
    };
    let root = api
        .ensure_file(
            "mutsuna-echo-root-v1",
            "Mutsuna Echo",
            "application/vnd.google-apps.folder",
            None,
        )
        .await?;
    let folder_name = format!(
        "{}_{}",
        chrono::Local::now().format("%Y-%m-%d_%H%M%S"),
        title
    );
    let folder = api
        .ensure_file(
            &format!("{}-folder", export.version),
            &folder_name,
            "application/vnd.google-apps.folder",
            Some(&root),
        )
        .await?;
    export.folder = Some(folder.clone());
    write_export(&path, &export)?;
    let result: Result<(), String> = async {
        if request.include_document {
            export.document_hash = None; write_export(&path, &export)?;
            progress(&app, meeting, "会議ノート・文字起こしを保存中", 10);
            let id = api.ensure_file(&format!("{}-doc", export.version), &title, "application/vnd.google-apps.document", Some(&folder)).await?;
            export.document = Some(id.clone()); write_export(&path, &export)?;
            let mut doc = api.docs(&id).await?;
            if export.note_tab.is_none() || export.transcript_tab.is_none() {
                // Recovery after a lost response finds tabs by title before adding any.
                let tabs = doc["tabs"].as_array().ok_or("ドキュメントのタブを確認できません。")?;
                let find = |title: &str| tabs.iter().find(|t| t["tabProperties"]["title"] == title)
                    .and_then(|t| t["tabProperties"]["tabId"].as_str()).map(str::to_owned);
                export.note_tab = find("会議ノート"); export.transcript_tab = find("文字起こし");
                let mut requests = vec![];
                if export.note_tab.is_none() {
                    let first = tabs.first().and_then(|t| t["tabProperties"]["tabId"].as_str()).ok_or("ドキュメントのタブがありません。")?;
                    requests.push(json!({"updateDocumentTabProperties":{"tabProperties":{"tabId":first,"title":"会議ノート"},"fields":"title"}}));
                }
                if export.transcript_tab.is_none() { requests.push(json!({"addDocumentTab":{"tabProperties":{"title":"文字起こし"}}})); }
                if !requests.is_empty() { api.batch(&id, json!({"requests":requests,"writeControl":{"requiredRevisionId":doc["revisionId"]}})).await?; }
                doc = api.docs(&id).await?;
                for tab in doc["tabs"].as_array().ok_or("タブの作成を確認できません。")? {
                    if tab["tabProperties"]["title"] == "会議ノート" { export.note_tab = tab["tabProperties"]["tabId"].as_str().map(str::to_owned); }
                    if tab["tabProperties"]["title"] == "文字起こし" { export.transcript_tab = tab["tabProperties"]["tabId"].as_str().map(str::to_owned); }
                }
                write_export(&path, &export)?;
            }
            let notes = document::notes(&title, note.as_ref());
            let text = document::transcript(&title, transcript.as_ref());
            let body = document::replace_tabs(&doc, export.note_tab.as_deref().ok_or("会議ノートのタブがありません。")?, &notes,
                export.transcript_tab.as_deref().ok_or("文字起こしのタブがありません。")?, &text)?;
            api.batch(&id, body).await?;
            export.document_hash = Some(content_hash(&("pageless-v1", &notes, &text))?);
            write_export(&path, &export)?;
        }
        if let Some(audio) = audio {
            export.audio_hash = None; write_export(&path, &export)?;
            progress(&app, meeting, "音声を保存中", 25);
            let name = audio.file_name().and_then(|s| s.to_str()).unwrap_or("recording.m4a");
            let mime = match audio.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase().as_str() {
                "wav" => "audio/wav", "mp3" => "audio/mpeg", "flac" => "audio/flac", "ogg" => "audio/ogg", _ => "audio/mp4"
            };
            let id = api.ensure_file(&format!("{}-audio", export.version), name, mime, Some(&folder)).await?;
            export.audio = Some(id.clone()); write_export(&path, &export)?;
            progress(&app, meeting, "音声の変更を確認中", 25);
            let audio_path = audio.clone();
            let fingerprint = tauri::async_runtime::spawn_blocking(move || audio::fingerprint(&audio_path)).await
                .map_err(|_| "音声の変更確認に失敗しました。")??;
            audio::validate_selection(fingerprint.size, request.selected_audio_size)?;
            let remote = api.get(&format!("files/{id}"), &[("fields", "id,size,sha256Checksum,trashed")]).await?;
            if audio::unchanged(&fingerprint, &remote) {
                progress(&app, meeting, "変更のない音声の送信を省略しました", 90);
            } else {
                upload(&api, &app, meeting, &id, &audio, mime, request.selected_audio_size).await?;
            }
            export.audio_hash = Some(fingerprint.sha256);
            write_export(&path, &export)?;
        }
        if request.include_backup {
            export.backup_hash = None; write_export(&path, &export)?;
            progress(&app, meeting, "復元用データを保存中", 95);
            let id = api.ensure_file(&format!("{}-backup", export.version), "meeting.json", "application/json", Some(&folder)).await?;
            export.backup = Some(id.clone()); write_export(&path, &export)?;
            let backup = json!({"schemaVersion":1,"meetingId":meeting,"title":title,"transcript":transcript,"meetingDocument":note});
            response_value(limits::send(api.client.patch(format!("https://www.googleapis.com/upload/drive/v3/files/{id}"))
                .query(&[("uploadType","media")]).bearer_auth(&api.token).header("Content-Type","application/json")
                .body(serde_json::to_vec_pretty(&backup).map_err(|_| "復元用データを作成できません。")?)).await?).await?;
            export.backup_hash = Some(content_hash(&backup)?);
            write_export(&path, &export)?;
        }
        Ok(())
    }.await;
    result.map_err(|e| {
        format!("{e} 一部のファイルは保存済みの可能性があります。再保存で続行できます。")
    })?;
    export.completed_at = Some(chrono::Utc::now().to_rfc3339());
    write_export(&path, &export)?;
    limits::LIMITS.saved(limits::now());
    progress(&app, meeting, "保存しました", 100);
    google_drive_saved_state(app, request.meeting_id).await
}

async fn upload(
    api: &Api,
    app: &AppHandle,
    meeting: &str,
    id: &str,
    path: &Path,
    mime: &str,
    selected_audio_size: Option<u64>,
) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).map_err(|_| "音声を読み込めません。")?;
    let size = file
        .metadata()
        .map_err(|_| "音声のサイズを確認できません。")?
        .len();
    audio::validate_selection(size, selected_audio_size)?;
    if size == 0 {
        return Err("音声ファイルが空です。".into());
    }
    let start = response_value(
        limits::send(
            api.client
                .patch(format!(
                    "https://www.googleapis.com/upload/drive/v3/files/{id}"
                ))
                .query(&[("uploadType", "resumable")])
                .bearer_auth(&api.token)
                .header("X-Upload-Content-Type", mime)
                .header("X-Upload-Content-Length", size)
                .json(&json!({})),
        )
        .await?,
    )
    .await?;
    let location = start
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or("音声の送信先を取得できません。")?;
    let url = url::Url::parse(location).map_err(|_| "音声の送信先が不正です。")?;
    if url.scheme() != "https"
        || url.host_str() != Some("www.googleapis.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("音声の送信先がGoogle APIではありません。".into());
    }
    let mut offset = 0_u64;
    let mut retries = limits::Retries::default();
    while offset < size {
        limits::LIMITS.check_quota()?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| "音声の読み込み位置を設定できません。")?;
        let length = (size - offset).min(8 * 1024 * 1024) as usize;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)
            .map_err(|_| "音声ファイルが変更されたか、読み込めません。")?;
        let result = api
            .client
            .put(url.clone())
            .bearer_auth(&api.token)
            .header("Content-Type", mime)
            .header(
                "Content-Range",
                format!("bytes {offset}-{}/{size}", offset + length as u64 - 1),
            )
            .body(bytes)
            .send()
            .await;
        let reply = if limits::transient(&result) {
            // A failed chunk may have been received. Probe before resending;
            // probes and repeated no-progress responses share the retry budget.
            loop {
                retries.wait().await?;
                limits::LIMITS.check_quota()?;
                let probe = api
                    .client
                    .put(url.clone())
                    .bearer_auth(&api.token)
                    .header("Content-Length", 0)
                    .header("Content-Range", format!("bytes */{size}"))
                    .send()
                    .await;
                if limits::transient(&probe) {
                    continue;
                }
                break limits::check(probe.map_err(network_error)?, &limits::LIMITS).await?;
            }
        } else {
            limits::check(result.map_err(network_error)?, &limits::LIMITS).await?
        };
        if reply.status().as_u16() != 308 && !reply.status().is_success() {
            response_value(reply).await?;
            return Err("音声を送信できません。".into());
        }
        if reply.status().is_success() {
            return Ok(());
        }
        let next = upload_offset(
            reply.headers().get("range").and_then(|v| v.to_str().ok()),
            size,
        )?;
        if next <= offset {
            retries.wait().await?;
        }
        offset = next;
        progress(app, meeting, "音声を保存中", 25 + 65 * offset / size);
    }
    Err("音声の保存完了を確認できません。再試行してください。".into())
}
fn upload_offset(range: Option<&str>, size: u64) -> Result<u64, String> {
    let Some(range) = range else { return Ok(0) };
    let end = range
        .strip_prefix("bytes=0-")
        .and_then(|s| s.parse::<u64>().ok())
        .and_then(|n| n.checked_add(1))
        .filter(|n| *n <= size)
        .ok_or("音声の再開位置が不正です。")?;
    Ok(end)
}

#[tauri::command]
pub(crate) fn google_drive_limits() -> limits::Status {
    limits::LIMITS.status(limits::now())
}

#[tauri::command]
pub(crate) fn google_drive_open(app: AppHandle, meeting_id: String) -> Result<(), String> {
    let session = auth::load(&app)?;
    let saved = read_export(&state_path(&app, &meeting_id, &session.account)?)?;
    let folder = saved
        .and_then(|v| v.folder)
        .ok_or("Driveの保存先がありません。")?;
    let url = format!("https://drive.google.com/drive/folders/{folder}");
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|_| "Driveを開けません。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_content_is_per_item_and_legacy_history_is_unknown() {
        let mut export: Export = serde_json::from_value(json!({"version":"legacy"})).unwrap();
        let original = content_hash(&json!({"text":"before"})).unwrap();
        let changed = content_hash(&json!({"text":"after"})).unwrap();
        assert!(!matches_saved(
            export.document_hash.as_ref(),
            Some(&original)
        ));
        export.document_hash = Some(original.clone());
        let mut restored: Export =
            serde_json::from_slice(&serde_json::to_vec(&export).unwrap()).unwrap();
        assert!(matches_saved(
            restored.document_hash.as_ref(),
            Some(&original)
        ));
        assert!(!matches_saved(
            restored.document_hash.as_ref(),
            Some(&changed)
        ));
        assert!(!matches_saved(
            restored.audio_hash.as_ref(),
            Some(&original)
        ));
        // A failed replacement must not retain the previous success fingerprint.
        restored.document_hash = None;
        assert!(!matches_saved(
            restored.document_hash.as_ref(),
            Some(&original)
        ));
        assert!(!matches_saved(None, None));
    }
    #[test]
    fn overwrite_requires_confirmed_version_and_new_version_is_explicit() {
        assert!(validate_save_choice(None, None, &SaveMode::Create).is_ok());
        assert!(validate_save_choice(Some("v1"), Some("v1"), &SaveMode::Overwrite).is_ok());
        assert!(validate_save_choice(Some("v1"), Some("v1"), &SaveMode::NewVersion).is_ok());
        assert!(validate_save_choice(Some("v1"), Some("v1"), &SaveMode::Create).is_err());
        assert!(validate_save_choice(Some("v2"), Some("v1"), &SaveMode::Overwrite).is_err());
        assert!(validate_save_choice(None, None, &SaveMode::Overwrite).is_err());
    }
    #[test]
    fn resumable_offsets_are_checked() {
        assert_eq!(
            upload_offset(Some("bytes=0-262143"), 524288).unwrap(),
            262144
        );
        assert_eq!(upload_offset(None, 10).unwrap(), 0);
        assert!(upload_offset(Some("bytes=0-10"), 10).is_err());
        assert!(upload_offset(Some("bytes=4-9"), 10).is_err());
    }
    #[test]
    fn rejects_url_in_file_id() {
        assert!(valid_id("../../other?secret").is_err());
        assert!(valid_id("google_ID-123").is_ok());
    }
}
