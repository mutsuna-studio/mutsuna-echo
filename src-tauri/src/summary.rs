use std::{
    collections::HashSet,
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::transcript_store::SummaryTranscriptSnapshot;

const SCHEMA_VERSION: u8 = 1;
const MAX_SUMMARY_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PROMPT_BYTES: usize = 4 * 1024 * 1024;
const SUMMARY_CHUNK_DURATION_MS: u64 = 15 * 60 * 1_000;
const MAX_SUMMARY_CHUNK_BYTES: usize = 192 * 1024;
#[cfg(target_os = "android")]
const MAX_PARALLEL_SUMMARY_CHUNKS: usize = 2;
#[cfg(not(target_os = "android"))]
const MAX_PARALLEL_SUMMARY_CHUNKS: usize = 4;
const CODEX_TIMEOUT: Duration = Duration::from_secs(180);
const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);
const CLOUDFLARE_PROVIDER_ID: &str = "cloudflare";
const CLOUDFLARE_GRANITE_MODEL_ID: &str = "@cf/ibm-granite/granite-4.0-h-micro";
const CLOUDFLARE_GLM_MODEL_ID: &str = "@cf/zai-org/glm-4.7-flash";
const CLOUDFLARE_GEMMA_MODEL_ID: &str = "@cf/google/gemma-4-26b-a4b-it";
const NODE_VERSION: &str = "24.18.0";
const LEGACY_NODE_VERSIONS: &[&str] = &["22.23.2"];
const MAX_NODE_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy)]
struct AcpAgentDefinition {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    executable: &'static str,
    executable_env: &'static str,
    args: &'static [&'static str],
    install_hint: &'static str,
    package: &'static str,
    version: &'static str,
    binary: &'static str,
}

const ACP_AGENTS: [AcpAgentDefinition; 3] = [
    AcpAgentDefinition {
        id: "codex",
        label: "Codex",
        description: "ローカルでログイン済みのCodexをACP経由で使用します。",
        executable: "codex-acp",
        executable_env: "MUTSUNA_CODEX_ACP_PATH",
        args: &[],
        install_hint: "codex-acpが見つかりません。npm install -g @agentclientprotocol/codex-acp で追加してください。",
        package: "@agentclientprotocol/codex-acp",
        version: "1.1.14",
        binary: "codex-acp",
    },
    AcpAgentDefinition {
        id: "claude",
        label: "Claude Code",
        description: "ローカルのClaude Agent認証をACP経由で使用します。",
        executable: "claude-agent-acp",
        executable_env: "MUTSUNA_CLAUDE_ACP_PATH",
        args: &[],
        install_hint: "claude-agent-acpが見つかりません。npm install -g @agentclientprotocol/claude-agent-acp で追加してください。",
        package: "@agentclientprotocol/claude-agent-acp",
        version: "0.66.0",
        binary: "claude-agent-acp",
    },
    AcpAgentDefinition {
        id: "gemini",
        label: "Gemini CLI",
        description: "ローカルでログイン済みのGemini CLIをネイティブACPモードで使用します。",
        executable: "gemini",
        executable_env: "MUTSUNA_GEMINI_PATH",
        args: &["--acp"],
        install_hint: "Gemini CLIが見つかりません。Gemini CLIをインストールし、ログインしてください。",
        package: "@google/gemini-cli",
        version: "0.54.4",
        binary: "gemini",
    },
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummaryAgentInstallStatus {
    id: String,
    label: String,
    version: String,
    installed: bool,
    external: bool,
    installable: bool,
    status_message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummaryModelDefinition {
    id: String,
    label: String,
    description: String,
    is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummaryProviderDefinition {
    id: String,
    label: String,
    description: String,
    ready: bool,
    status_message: String,
    models: Vec<SummaryModelDefinition>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SummaryReference {
    text: String,
    #[serde(default)]
    source_segment_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SummaryActionItem {
    assignee: Option<String>,
    text: String,
    due: Option<String>,
    #[serde(default)]
    source_segment_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SummaryContent {
    overview: String,
    decisions: Vec<SummaryReference>,
    action_items: Vec<SummaryActionItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MeetingSummary {
    schema_version: u8,
    summary_id: String,
    meeting_id: String,
    transcription_id: String,
    source_revision: u64,
    provider: String,
    model: String,
    generated_at: String,
    content: SummaryContent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummaryStatus {
    summary: Option<MeetingSummary>,
    transcription_id: Option<String>,
    current_revision: Option<u64>,
    stale: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryProgress {
    meeting_id: String,
    completed_steps: u32,
    total_steps: u32,
    stage: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerateSummaryRequest {
    meeting_id: String,
    provider_id: String,
    model_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FormatTranscriptRequest {
    meeting_id: String,
    provider_id: Option<String>,
    model_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TranscriptFormattingChange {
    segment_id: String,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TranscriptFormattingContent {
    changes: Vec<TranscriptFormattingChange>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptFormattingResult {
    transcription_id: String,
    source_revision: u64,
    method: &'static str,
    provider: Option<String>,
    model: Option<String>,
    changes: Vec<TranscriptFormattingChange>,
    warning: Option<String>,
}

pub(crate) async fn providers(app: AppHandle) -> Vec<SummaryProviderDefinition> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut providers: Vec<_> = ACP_AGENTS
            .iter()
            .map(|agent| {
                let ready = resolve_agent_executable(&app, agent).is_some();
                SummaryProviderDefinition {
                    id: agent.id.into(),
                    label: agent.label.into(),
                    description: agent.description.into(),
                    ready,
                    status_message: if ready {
                        "ACP接続可能・ログイン状態は生成時に確認します。".into()
                    } else {
                        agent.install_hint.into()
                    },
                    models: vec![SummaryModelDefinition {
                        id: "default".into(),
                        label: format!("{}の既定モデル", agent.label),
                        description: "ACPエージェント側の既定モデルを使用します。".into(),
                        is_default: true,
                    }],
                }
            })
            .collect();
        let cloudflare_ready =
            crate::credentials::has(&app, crate::credentials::CredentialId::CloudflareApiToken)
                .unwrap_or(false)
                && crate::credentials::has(
                    &app,
                    crate::credentials::CredentialId::CloudflareAccountId,
                )
                .unwrap_or(false);
        providers.push(SummaryProviderDefinition {
            id: CLOUDFLARE_PROVIDER_ID.into(),
            label: "Cloudflare Workers AI".into(),
            description: "保存済みのCloudflare認証情報で会議ノートを生成します。".into(),
            ready: cloudflare_ready,
            status_message: if cloudflare_ready {
                "APIトークンとAccount IDを設定済みです。".into()
            } else {
                "文字起こし設定でCloudflare APIトークンとAccount IDを設定してください。".into()
            },
            models: cloudflare_summary_models(),
        });
        providers
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub(crate) async fn get_summary_providers(app: AppHandle) -> Vec<SummaryProviderDefinition> {
    providers(app).await
}

#[tauri::command]
pub(crate) async fn get_summary_models(
    app: AppHandle,
    provider_id: String,
) -> Result<Vec<SummaryModelDefinition>, String> {
    if provider_id == CLOUDFLARE_PROVIDER_ID {
        let ready =
            crate::credentials::has(&app, crate::credentials::CredentialId::CloudflareApiToken)
                .unwrap_or(false)
                && crate::credentials::has(
                    &app,
                    crate::credentials::CredentialId::CloudflareAccountId,
                )
                .unwrap_or(false);
        return ready
            .then(cloudflare_summary_models)
            .ok_or_else(|| "Cloudflare APIトークンとAccount IDを設定してください。".into());
    }
    let agent = ACP_AGENTS
        .iter()
        .find(|agent| agent.id == provider_id)
        .copied()
        .ok_or_else(|| "選択した要約プロバイダーには対応していません。".to_string())?;
    let executable =
        resolve_agent_executable(&app, &agent).ok_or_else(|| agent.install_hint.to_string())?;
    let node_bin = managed_node_bin_directory(&app);
    tauri::async_runtime::spawn_blocking(move || {
        discover_models_with_acp(agent, executable, node_bin)
    })
    .await
    .map_err(|_| "ACPエージェントのモデル取得を完了できませんでした。".to_string())?
}

#[tauri::command]
pub(crate) async fn list_summary_agent_install_status(
    app: AppHandle,
) -> Vec<SummaryAgentInstallStatus> {
    tauri::async_runtime::spawn_blocking(move || install_statuses(&app))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) async fn install_summary_agent(
    app: AppHandle,
    provider_id: String,
) -> Result<(), String> {
    ensure_managed_node(&app).await?;
    tauri::async_runtime::spawn_blocking(move || install_agent(&app, &provider_id))
        .await
        .map_err(|_| "要約エージェントのインストール処理を完了できませんでした。".to_string())?
}

#[tauri::command]
pub(crate) async fn delete_summary_agent(
    app: AppHandle,
    provider_id: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || delete_managed_agent(&app, &provider_id))
        .await
        .map_err(|_| "要約エージェントの削除処理を完了できませんでした。".to_string())?
}

#[tauri::command]
pub(crate) fn get_selected_summary(
    app: AppHandle,
    meeting_id: String,
) -> Result<SummaryStatus, String> {
    status(&app, &meeting_id)
}

#[tauri::command]
pub(crate) async fn generate_selected_summary(
    app: AppHandle,
    request: GenerateSummaryRequest,
) -> Result<SummaryStatus, String> {
    let _power_guard = crate::processing_power::acquire(&app, "会議ノートを生成中")?;
    generate(app, request).await
}

#[tauri::command]
pub(crate) async fn format_selected_transcript(
    app: AppHandle,
    request: FormatTranscriptRequest,
) -> Result<TranscriptFormattingResult, String> {
    let _power_guard = crate::processing_power::acquire(&app, "文字起こしを整形中")?;
    format_transcript(app, request).await
}

pub(crate) fn status(app: &AppHandle, meeting_id: &str) -> Result<SummaryStatus, String> {
    crate::meeting_store::validate_meeting_id(meeting_id)?;
    let snapshot = crate::transcript_store::selected_summary_snapshot(app, meeting_id)?;
    let Some(snapshot) = snapshot else {
        return Ok(SummaryStatus {
            summary: None,
            transcription_id: None,
            current_revision: None,
            stale: false,
        });
    };
    let summary = read_summary(app, meeting_id, &snapshot.transcription_id)?;
    let stale = summary
        .as_ref()
        .is_some_and(|summary| summary.source_revision != snapshot.revision);
    Ok(SummaryStatus {
        summary,
        transcription_id: Some(snapshot.transcription_id),
        current_revision: Some(snapshot.revision),
        stale,
    })
}

pub(crate) async fn generate(
    app: AppHandle,
    request: GenerateSummaryRequest,
) -> Result<SummaryStatus, String> {
    crate::meeting_store::validate_meeting_id(&request.meeting_id)?;
    validate_model_id(&request.model_id)?;
    let snapshot = crate::transcript_store::selected_summary_snapshot(&app, &request.meeting_id)?
        .ok_or_else(|| "先に文字起こしを作成してください。".to_string())?;
    if snapshot.segments.is_empty() {
        return Err("要約できる文字起こしがありません。".into());
    }
    let generated = if request.provider_id == CLOUDFLARE_PROVIDER_ID {
        generate_with_cloudflare(&app, &snapshot, &request.model_id).await?
    } else {
        let agent = ACP_AGENTS
            .iter()
            .find(|agent| agent.id == request.provider_id)
            .copied()
            .ok_or_else(|| "選択した要約プロバイダーには対応していません。".to_string())?;
        let model_id = request.model_id.clone();
        let executable =
            resolve_agent_executable(&app, &agent).ok_or_else(|| agent.install_hint.to_string())?;
        let node_bin = managed_node_bin_directory(&app);
        let generation_app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            generate_with_acp(
                &generation_app,
                &snapshot,
                agent,
                executable,
                node_bin,
                &model_id,
            )
        })
        .await
        .map_err(|_| "ACPエージェントの要約処理を完了できませんでした。".to_string())??
    };
    let current = crate::transcript_store::selected_summary_snapshot(&app, &request.meeting_id)?
        .ok_or_else(|| "要約中に文字起こしが削除されました。".to_string())?;
    if current.transcription_id != generated.transcription_id
        || current.revision != generated.source_revision
    {
        return Err(
            "要約中に文字起こしが変更されました。内容を確認して、もう一度要約してください。".into(),
        );
    }
    write_summary(&app, &generated)?;
    status(&app, &request.meeting_id)
}

async fn format_transcript(
    app: AppHandle,
    request: FormatTranscriptRequest,
) -> Result<TranscriptFormattingResult, String> {
    crate::meeting_store::validate_meeting_id(&request.meeting_id)?;
    let original = crate::transcript_store::selected_summary_snapshot(&app, &request.meeting_id)?
        .ok_or_else(|| "先に文字起こしを作成してください。".to_string())?;

    let mut formatted = original.clone();
    let corrections =
        crate::transcription::context::effective_corrections(&app, &request.meeting_id)?;
    for segment in &mut formatted.segments {
        segment.text = mechanically_format_transcript_text(&segment.text);
        segment.text = apply_text_corrections(&segment.text, &corrections);
    }

    let mut method = "mechanical";
    let mut provider = None;
    let mut model = None;
    let mut warning = None;

    if let (Some(provider_id), Some(model_id)) =
        (request.provider_id.as_deref(), request.model_id.as_deref())
    {
        match generate_transcript_formatting_with_acp(&app, &formatted, provider_id, model_id).await
        {
            Ok((llm_changes, resolved_model)) => {
                apply_formatting_changes(&mut formatted, &llm_changes)?;
                for segment in &mut formatted.segments {
                    segment.text = mechanically_format_transcript_text(&segment.text);
                    segment.text = apply_text_corrections(&segment.text, &corrections);
                }
                method = "mechanicalAndLlm";
                provider = Some(provider_id.to_string());
                model = Some(resolved_model);
            }
            Err(error) => {
                warning = Some(format!(
                    "LLMによる校正を適用できなかったため、機械的な整形のみを適用しました: {error}"
                ));
            }
        }
    }

    let current = crate::transcript_store::selected_summary_snapshot(&app, &request.meeting_id)?
        .ok_or_else(|| "整形中に文字起こしが削除されました。".to_string())?;
    if current.transcription_id != original.transcription_id
        || current.revision != original.revision
    {
        return Err(
            "整形中に文字起こしが変更されました。内容を確認して、もう一度整形してください。".into(),
        );
    }

    let changes = original
        .segments
        .iter()
        .zip(&formatted.segments)
        .filter(|(before, after)| before.text != after.text)
        .map(|(_, after)| TranscriptFormattingChange {
            segment_id: after.segment_id.clone(),
            text: after.text.clone(),
        })
        .collect();

    Ok(TranscriptFormattingResult {
        transcription_id: original.transcription_id,
        source_revision: original.revision,
        method,
        provider,
        model,
        changes,
        warning,
    })
}

async fn generate_transcript_formatting_with_acp(
    app: &AppHandle,
    snapshot: &SummaryTranscriptSnapshot,
    provider_id: &str,
    model_id: &str,
) -> Result<(Vec<TranscriptFormattingChange>, String), String> {
    if provider_id == CLOUDFLARE_PROVIDER_ID {
        validate_cloudflare_model_id(model_id)?;
        let prompt = build_transcript_formatting_prompt(snapshot)?;
        if prompt.len() > MAX_PROMPT_BYTES {
            return Err("文字起こしが大きすぎるため整形できません。".into());
        }
        let output = generate_cloudflare_text(app, model_id, &prompt).await?;
        let changes = parse_transcript_formatting_content(&output, snapshot)?;
        return Ok((changes, model_id.to_string()));
    }
    let agent = ACP_AGENTS
        .iter()
        .find(|agent| agent.id == provider_id)
        .copied()
        .ok_or_else(|| "選択した要約プロバイダーには対応していません。".to_string())?;
    validate_model_id(model_id)?;
    let executable =
        resolve_agent_executable(app, &agent).ok_or_else(|| agent.install_hint.to_string())?;
    let snapshot = snapshot.clone();
    let model_id = model_id.to_string();
    let node_bin = managed_node_bin_directory(app);
    tauri::async_runtime::spawn_blocking(move || {
        let prompt = build_transcript_formatting_prompt(&snapshot)?;
        if prompt.len() > MAX_PROMPT_BYTES {
            return Err("文字起こしが大きすぎるため整形できません。".into());
        }
        let work_dir = std::env::temp_dir().join(format!(
            "mutsuna-echo-format-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        fs::create_dir(&work_dir)
            .map_err(|error| format!("整形用の一時領域を作成できませんでした: {error}"))?;
        let result = run_acp_agent(agent, executable, node_bin, &work_dir, &model_id, &prompt)
            .and_then(|(output, model)| {
                parse_transcript_formatting_content(&output, &snapshot)
                    .map(|changes| (changes, model))
            });
        let _ = fs::remove_dir_all(&work_dir);
        result
    })
    .await
    .map_err(|_| "ACPエージェントの整形処理を完了できませんでした。".to_string())?
}

fn discover_models_with_acp(
    agent: AcpAgentDefinition,
    executable: PathBuf,
    node_bin: Option<PathBuf>,
) -> Result<Vec<SummaryModelDefinition>, String> {
    let work_dir = std::env::temp_dir().join(format!(
        "mutsuna-echo-summary-models-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7()
    ));
    fs::create_dir(&work_dir)
        .map_err(|error| format!("モデル取得用の一時領域を作成できませんでした: {error}"))?;
    let result = run_acp_model_discovery(agent, executable, node_bin, &work_dir);
    let _ = fs::remove_dir_all(&work_dir);
    result
}

fn run_acp_model_discovery(
    agent: AcpAgentDefinition,
    executable: PathBuf,
    node_bin: Option<PathBuf>,
    work_dir: &Path,
) -> Result<Vec<SummaryModelDefinition>, String> {
    let mut command = Command::new(executable);
    command.current_dir(work_dir).args(agent.args);
    configure_background_command(&mut command);
    if let Some(node_bin) = node_bin {
        let mut paths = vec![node_bin];
        if let Some(current) = env::var_os("PATH") {
            paths.extend(env::split_paths(&current));
        }
        if let Ok(path) = env::join_paths(paths) {
            command.env("PATH", path);
        }
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!(
                "{}のACPエージェントを起動できませんでした: {error}",
                agent.label
            )
        })?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "ACPエージェントへ接続できませんでした。".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ACPエージェントの応答を取得できませんでした。".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "ACPエージェントのエラー出力を取得できませんでした。".to_string())?;
    let (sender, receiver) = mpsc::channel::<String>();
    let stdout_reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_reader = thread::spawn(move || {
        let mut value = String::new();
        let _ = stderr.read_to_string(&mut value);
        value
    });

    let result = (|| {
        let deadline = Instant::now() + MODEL_DISCOVERY_TIMEOUT;
        send_rpc(
            &mut stdin,
            0,
            "initialize",
            serde_json::json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "mutsuna-echo", "title": "Mutsuna Echo", "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        let initialized = wait_for_response(&receiver, &mut stdin, 0, deadline, None)?;
        ensure_rpc_success(&initialized, agent.label)?;
        send_rpc(
            &mut stdin,
            1,
            "session/new",
            serde_json::json!({ "cwd": work_dir, "mcpServers": [] }),
        )?;
        let session_response = wait_for_response(&receiver, &mut stdin, 1, deadline, None)?;
        let session_result = ensure_rpc_success(&session_response, agent.label)?;
        Ok(model_definitions_from_session(session_result, agent.label))
    })();
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let _ = stdout_reader.join();
    let stderr = stderr_reader.join().unwrap_or_default();
    if let Err(error) = result {
        let detail = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        return Err(if detail.is_empty() {
            error
        } else {
            format!("{error} ({})", truncate(detail, 500))
        });
    }
    result
}

fn model_definitions_from_session(
    session_result: &serde_json::Value,
    agent_label: &str,
) -> Vec<SummaryModelDefinition> {
    let model_config = session_result
        .get("configOptions")
        .and_then(serde_json::Value::as_array)
        .and_then(|options| {
            options.iter().find(|option| {
                option.get("category").and_then(serde_json::Value::as_str) == Some("model")
            })
        });
    let current = model_config
        .and_then(|config| config.get("currentValue"))
        .and_then(serde_json::Value::as_str);
    let mut seen = HashSet::new();
    let mut models: Vec<SummaryModelDefinition> = model_config
        .and_then(|config| config.get("options"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let id = option
                .get("value")
                .and_then(serde_json::Value::as_str)?
                .trim();
            if id.is_empty() || !seen.insert(id.to_string()) {
                return None;
            }
            let label = option
                .get("name")
                .or_else(|| option.get("label"))
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(id);
            let description = option
                .get("description")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            Some(SummaryModelDefinition {
                id: id.into(),
                label: label.into(),
                description: description.into(),
                is_default: current == Some(id),
            })
        })
        .collect();
    if models.is_empty() || !models.iter().any(|model| model.is_default) {
        models.insert(
            0,
            SummaryModelDefinition {
                id: "default".into(),
                label: format!("{agent_label}の既定モデル"),
                description: "ACPエージェント側の既定モデルを使用します。".into(),
                is_default: true,
            },
        );
    }
    models
}

fn cloudflare_summary_models() -> Vec<SummaryModelDefinition> {
    vec![
        SummaryModelDefinition {
            id: CLOUDFLARE_GLM_MODEL_ID.into(),
            label: "GLM-4.7-Flash".into(),
            description: "高速な多言語モデル。日本語の会議ノートに適しています。".into(),
            is_default: true,
        },
        SummaryModelDefinition {
            id: CLOUDFLARE_GRANITE_MODEL_ID.into(),
            label: "Granite 4.0 H Micro".into(),
            description: "軽量で低コストな指示追従モデルです。".into(),
            is_default: false,
        },
        SummaryModelDefinition {
            id: CLOUDFLARE_GEMMA_MODEL_ID.into(),
            label: "Gemma 4 26B-A4B".into(),
            description: "長い文脈と推論に対応する高品質なMoEモデルです。".into(),
            is_default: false,
        },
    ]
}

fn validate_cloudflare_model_id(model_id: &str) -> Result<(), String> {
    [
        CLOUDFLARE_GRANITE_MODEL_ID,
        CLOUDFLARE_GLM_MODEL_ID,
        CLOUDFLARE_GEMMA_MODEL_ID,
    ]
    .contains(&model_id)
    .then_some(())
    .ok_or_else(|| "選択したCloudflare Workers AIモデルには対応していません。".into())
}

async fn generate_cloudflare_text(
    app: &AppHandle,
    model_id: &str,
    prompt: &str,
) -> Result<String, String> {
    validate_cloudflare_model_id(model_id)?;
    let api_token =
        crate::credentials::load(app, crate::credentials::CredentialId::CloudflareApiToken)?;
    let account_id =
        crate::credentials::load(app, crate::credentials::CredentialId::CloudflareAccountId)?;
    crate::transcription::cloudflare::generate_text(&account_id, &api_token, model_id, prompt).await
}

async fn generate_with_cloudflare(
    app: &AppHandle,
    snapshot: &SummaryTranscriptSnapshot,
    model_id: &str,
) -> Result<MeetingSummary, String> {
    let chunks = split_summary_snapshot(snapshot);
    let total_steps = summary_total_steps(chunks.len());
    emit_summary_progress(app, &snapshot.meeting_id, 0, total_steps, "summarizing");

    let content = if chunks.len() == 1 {
        let prompt = build_prompt(snapshot)?;
        ensure_prompt_size(&prompt)?;
        let output = generate_cloudflare_text(app, model_id, &prompt).await?;
        let content = parse_generated_content(&output, snapshot)?;
        emit_summary_progress(app, &snapshot.meeting_id, 1, total_steps, "complete");
        content
    } else {
        let requests = futures_util::stream::iter(chunks.into_iter().enumerate())
            .map(|(index, chunk)| async move {
                let prompt = build_prompt(&chunk)?;
                ensure_prompt_size(&prompt)?;
                let output = generate_cloudflare_text(app, model_id, &prompt).await?;
                let content = parse_generated_content(&output, &chunk)?;
                Ok::<_, String>((index, content))
            })
            .buffer_unordered(MAX_PARALLEL_SUMMARY_CHUNKS);
        futures_util::pin_mut!(requests);
        let mut completed = 0u32;
        let mut partials = Vec::new();
        while let Some(result) = requests.next().await {
            partials.push(result?);
            completed += 1;
            emit_summary_progress(
                app,
                &snapshot.meeting_id,
                completed,
                total_steps,
                "summarizing",
            );
        }
        partials.sort_by_key(|(index, _)| *index);
        let partials: Vec<_> = partials.into_iter().map(|(_, content)| content).collect();
        let merge_prompt = build_summary_merge_prompt(&partials)?;
        ensure_prompt_size(&merge_prompt)?;
        emit_summary_progress(app, &snapshot.meeting_id, completed, total_steps, "merging");
        let output = generate_cloudflare_text(app, model_id, &merge_prompt).await?;
        let content = parse_generated_content(&output, snapshot)?;
        emit_summary_progress(
            app,
            &snapshot.meeting_id,
            total_steps,
            total_steps,
            "complete",
        );
        content
    };
    Ok(MeetingSummary {
        schema_version: SCHEMA_VERSION,
        summary_id: uuid::Uuid::now_v7().to_string(),
        meeting_id: snapshot.meeting_id.clone(),
        transcription_id: snapshot.transcription_id.clone(),
        source_revision: snapshot.revision,
        provider: CLOUDFLARE_PROVIDER_ID.into(),
        model: model_id.into(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        content,
    })
}

fn generate_with_acp(
    app: &AppHandle,
    snapshot: &SummaryTranscriptSnapshot,
    agent: AcpAgentDefinition,
    executable: PathBuf,
    node_bin: Option<PathBuf>,
    model_id: &str,
) -> Result<MeetingSummary, String> {
    let work_dir = std::env::temp_dir().join(format!(
        "mutsuna-echo-summary-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7()
    ));
    fs::create_dir(&work_dir)
        .map_err(|error| format!("要約用の一時領域を作成できませんでした: {error}"))?;
    let chunks = split_summary_snapshot(snapshot);
    let total_steps = summary_total_steps(chunks.len());
    emit_summary_progress(app, &snapshot.meeting_id, 0, total_steps, "summarizing");
    let result: Result<(SummaryContent, String), String> = (|| {
        if chunks.len() == 1 {
            let prompt = build_prompt(snapshot)?;
            ensure_prompt_size(&prompt)?;
            let (output, model) = run_acp_agent(
                agent,
                executable.clone(),
                node_bin.clone(),
                &work_dir,
                model_id,
                &prompt,
            )?;
            let content = parse_generated_content(&output, snapshot)?;
            emit_summary_progress(app, &snapshot.meeting_id, 1, total_steps, "complete");
            return Ok((content, model));
        }

        let mut partials = Vec::with_capacity(chunks.len());
        for (index, chunk) in chunks.iter().enumerate() {
            let prompt = build_prompt(chunk)?;
            ensure_prompt_size(&prompt)?;
            let (output, _) = run_acp_agent(
                agent,
                executable.clone(),
                node_bin.clone(),
                &work_dir,
                model_id,
                &prompt,
            )?;
            partials.push(parse_generated_content(&output, chunk)?);
            emit_summary_progress(
                app,
                &snapshot.meeting_id,
                (index + 1) as u32,
                total_steps,
                "summarizing",
            );
        }
        let merge_prompt = build_summary_merge_prompt(&partials)?;
        ensure_prompt_size(&merge_prompt)?;
        emit_summary_progress(
            app,
            &snapshot.meeting_id,
            partials.len() as u32,
            total_steps,
            "merging",
        );
        let (output, model) = run_acp_agent(
            agent,
            executable,
            node_bin,
            &work_dir,
            model_id,
            &merge_prompt,
        )?;
        let content = parse_generated_content(&output, snapshot)?;
        emit_summary_progress(
            app,
            &snapshot.meeting_id,
            total_steps,
            total_steps,
            "complete",
        );
        Ok((content, model))
    })();
    let _ = fs::remove_dir_all(&work_dir);
    let (content, resolved_model) = result?;
    Ok(MeetingSummary {
        schema_version: SCHEMA_VERSION,
        summary_id: uuid::Uuid::now_v7().to_string(),
        meeting_id: snapshot.meeting_id.clone(),
        transcription_id: snapshot.transcription_id.clone(),
        source_revision: snapshot.revision,
        provider: agent.id.into(),
        model: resolved_model,
        generated_at: chrono::Utc::now().to_rfc3339(),
        content,
    })
}

fn run_acp_agent(
    agent: AcpAgentDefinition,
    executable: PathBuf,
    node_bin: Option<PathBuf>,
    work_dir: &Path,
    model_id: &str,
    prompt: &str,
) -> Result<(String, String), String> {
    let mut command = Command::new(executable);
    command.current_dir(work_dir).args(agent.args);
    configure_background_command(&mut command);
    if let Some(node_bin) = node_bin {
        let mut paths = vec![node_bin];
        if let Some(current) = env::var_os("PATH") {
            paths.extend(env::split_paths(&current));
        }
        if let Ok(path) = env::join_paths(paths) {
            command.env("PATH", path);
        }
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!(
                "{}のACPエージェントを起動できませんでした: {error}",
                agent.label
            )
        })?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "ACPエージェントへ接続できませんでした。".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ACPエージェントの応答を取得できませんでした。".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "ACPエージェントのエラー出力を取得できませんでした。".to_string())?;
    let (sender, receiver) = mpsc::channel::<String>();
    let stdout_reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_reader = thread::spawn(move || {
        let mut value = String::new();
        let _ = stderr.read_to_string(&mut value);
        value
    });

    let result = (|| {
        let deadline = Instant::now() + CODEX_TIMEOUT;
        send_rpc(
            &mut stdin,
            0,
            "initialize",
            serde_json::json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "mutsuna-echo", "title": "Mutsuna Echo", "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        let initialized = wait_for_response(&receiver, &mut stdin, 0, deadline, None)?;
        ensure_rpc_success(&initialized, agent.label)?;
        send_rpc(
            &mut stdin,
            1,
            "session/new",
            serde_json::json!({ "cwd": work_dir, "mcpServers": [] }),
        )?;
        let session_response = wait_for_response(&receiver, &mut stdin, 1, deadline, None)?;
        let session_result = ensure_rpc_success(&session_response, agent.label)?;
        let session_id = session_result
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "ACPエージェントがSession IDを返しませんでした。".to_string())?;
        let model_config = session_result
            .get("configOptions")
            .and_then(serde_json::Value::as_array)
            .and_then(|options| {
                options.iter().find(|option| {
                    option.get("category").and_then(serde_json::Value::as_str) == Some("model")
                })
            });
        let mut resolved_model = model_config
            .and_then(|config| config.get("currentValue"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("default")
            .to_string();
        if model_id != "default" {
            let model_config = model_config.ok_or_else(|| {
                format!(
                    "{}はACPでモデル選択を公開していません。既定モデルを選択してください。",
                    agent.label
                )
            })?;
            let config_id = model_config
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "ACPのモデル設定が不正です。".to_string())?;
            let available = model_config
                .get("options")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|options| {
                    options.iter().any(|option| {
                        option.get("value").and_then(serde_json::Value::as_str) == Some(model_id)
                    })
                });
            if !available {
                return Err(format!(
                    "モデル「{model_id}」は{}のACPセッションで利用できません。",
                    agent.label
                ));
            }
            send_rpc(
                &mut stdin,
                2,
                "session/set_config_option",
                serde_json::json!({ "sessionId": session_id, "configId": config_id, "value": model_id }),
            )?;
            let configured = wait_for_response(&receiver, &mut stdin, 2, deadline, None)?;
            ensure_rpc_success(&configured, agent.label)?;
            resolved_model = model_id.to_string();
        }
        send_rpc(
            &mut stdin,
            3,
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": prompt }]
            }),
        )?;
        let mut output = String::new();
        let completed = wait_for_response(&receiver, &mut stdin, 3, deadline, Some(&mut output))?;
        ensure_rpc_success(&completed, agent.label)?;
        if output.trim().is_empty() {
            return Err(format!(
                "{}から要約本文を受け取れませんでした。",
                agent.label
            ));
        }
        Ok((output, resolved_model))
    })();
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let _ = stdout_reader.join();
    let stderr = stderr_reader.join().unwrap_or_default();
    if let Err(error) = result {
        let detail = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        return Err(if detail.is_empty() {
            error
        } else {
            format!("{error} ({})", truncate(detail, 500))
        });
    }
    result
}

fn send_rpc(
    stdin: &mut impl Write,
    id: u64,
    method: &str,
    params: serde_json::Value,
) -> Result<(), String> {
    serde_json::to_writer(
        &mut *stdin,
        &serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
    )
    .map_err(|error| format!("ACPリクエストを作成できませんでした: {error}"))?;
    stdin
        .write_all(b"\n")
        .and_then(|()| stdin.flush())
        .map_err(|error| format!("ACPリクエストを送信できませんでした: {error}"))
}

fn wait_for_response(
    receiver: &mpsc::Receiver<String>,
    stdin: &mut impl Write,
    expected_id: u64,
    deadline: Instant,
    mut output: Option<&mut String>,
) -> Result<serde_json::Value, String> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("ACP処理が制限時間内に完了しませんでした。".into());
        }
        let line = receiver
            .recv_timeout(remaining)
            .map_err(|_| "ACPエージェントからの応答が途切れました。".to_string())?;
        let message: serde_json::Value = serde_json::from_str(&line)
            .map_err(|error| format!("ACPエージェントから不正な応答を受信しました: {error}"))?;
        if message.get("id").and_then(serde_json::Value::as_u64) == Some(expected_id) {
            return Ok(message);
        }
        if message.get("method").and_then(serde_json::Value::as_str) == Some("session/update") {
            if let Some(text) = message
                .pointer("/params/update/content/text")
                .and_then(serde_json::Value::as_str)
            {
                if message
                    .pointer("/params/update/sessionUpdate")
                    .and_then(serde_json::Value::as_str)
                    == Some("agent_message_chunk")
                {
                    if let Some(output) = output.as_deref_mut() {
                        if output.len().saturating_add(text.len()) > MAX_SUMMARY_BYTES as usize {
                            return Err("ACPエージェントの応答が大きすぎます。".into());
                        }
                        output.push_str(text);
                    }
                }
            }
            continue;
        }
        if let (Some(id), Some(method)) = (
            message.get("id").cloned(),
            message.get("method").and_then(serde_json::Value::as_str),
        ) {
            let denial = serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("Mutsuna Echo does not expose client method {method}") } });
            serde_json::to_writer(&mut *stdin, &denial)
                .map_err(|error| format!("ACP応答を作成できませんでした: {error}"))?;
            stdin
                .write_all(b"\n")
                .and_then(|()| stdin.flush())
                .map_err(|error| format!("ACP応答を送信できませんでした: {error}"))?;
        }
    }
}

fn ensure_rpc_success<'a>(
    response: &'a serde_json::Value,
    label: &str,
) -> Result<&'a serde_json::Value, String> {
    if let Some(error) = response.get("error") {
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("詳細不明");
        return Err(format!(
            "{label}のACP処理に失敗しました: {message}。ログイン状態も確認してください。"
        ));
    }
    response
        .get("result")
        .ok_or_else(|| format!("{label}からACP結果を受け取れませんでした。"))
}

fn build_prompt(snapshot: &SummaryTranscriptSnapshot) -> Result<String, String> {
    let transcript = serde_json::to_string(snapshot)
        .map_err(|error| format!("文字起こしを要約用に変換できませんでした: {error}"))?;
    Ok(format!(
        "あなたは会議記録の編集者です。次の修正版文字起こしだけを根拠に、日本語の会議ノートを作成してください。外部情報、ファイル、Web、ツールを使わず、推測で事実を補わないでください。JSON以外の文字、説明、Markdownコードフェンスは一切出力しないでください。形式は厳密に {{\"overview\":\"簡潔な概要\",\"decisions\":[{{\"text\":\"決定事項\",\"sourceSegmentIds\":[\"segment id\"]}}],\"actionItems\":[{{\"assignee\":nullまたは文字列,\"text\":\"作業\",\"due\":nullまたは文字列,\"sourceSegmentIds\":[\"segment id\"]}}]}} とします。決定事項とアクション項目には根拠となる実在のsourceSegmentIdsを付け、該当項目がなければ空配列にしてください。\n\n文字起こしJSON:\n{transcript}"
    ))
}

fn build_summary_merge_prompt(partials: &[SummaryContent]) -> Result<String, String> {
    let summaries = serde_json::to_string(partials)
        .map_err(|error| format!("部分要約を統合用に変換できませんでした: {error}"))?;
    Ok(format!(
        "あなたは会議記録の編集者です。時系列順の部分要約を、一つの日本語の会議ノートへ統合してください。部分要約だけを根拠にし、重複する内容をまとめ、決定事項とアクション項目を漏らさないでください。sourceSegmentIdsは入力に実在するIDだけをそのまま維持してください。JSON以外の文字、説明、Markdownコードフェンスは一切出力しないでください。形式は厳密に {{\"overview\":\"簡潔な概要\",\"decisions\":[{{\"text\":\"決定事項\",\"sourceSegmentIds\":[\"segment id\"]}}],\"actionItems\":[{{\"assignee\":nullまたは文字列,\"text\":\"作業\",\"due\":nullまたは文字列,\"sourceSegmentIds\":[\"segment id\"]}}]}} とします。\n\n部分要約JSON:\n{summaries}"
    ))
}

fn ensure_prompt_size(prompt: &str) -> Result<(), String> {
    if prompt.len() > MAX_PROMPT_BYTES {
        Err("会議ノート生成用の入力が大きすぎます。".into())
    } else {
        Ok(())
    }
}

fn summary_total_steps(chunk_count: usize) -> u32 {
    if chunk_count <= 1 {
        1
    } else {
        chunk_count.saturating_add(1).min(u32::MAX as usize) as u32
    }
}

fn split_summary_snapshot(snapshot: &SummaryTranscriptSnapshot) -> Vec<SummaryTranscriptSnapshot> {
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut current_bytes = 0usize;
    let mut chunk_start_ms = 0u64;

    for segment in &snapshot.segments {
        let segment_bytes = segment
            .text
            .len()
            .saturating_add(segment.speaker.len())
            .saturating_add(segment.segment_id.len())
            .saturating_add(64);
        let exceeds_duration = !current.is_empty()
            && segment.end_ms.saturating_sub(chunk_start_ms) > SUMMARY_CHUNK_DURATION_MS;
        let exceeds_size = !current.is_empty()
            && current_bytes.saturating_add(segment_bytes) > MAX_SUMMARY_CHUNK_BYTES;
        if exceeds_duration || exceeds_size {
            chunks.push(summary_chunk(snapshot, std::mem::take(&mut current)));
            current_bytes = 0;
        }
        if current.is_empty() {
            chunk_start_ms = segment.start_ms;
        }
        current_bytes = current_bytes.saturating_add(segment_bytes);
        current.push(segment.clone());
    }
    if !current.is_empty() {
        chunks.push(summary_chunk(snapshot, current));
    }
    chunks
}

fn summary_chunk(
    snapshot: &SummaryTranscriptSnapshot,
    segments: Vec<crate::transcript_store::SummaryTranscriptSegment>,
) -> SummaryTranscriptSnapshot {
    SummaryTranscriptSnapshot {
        meeting_id: snapshot.meeting_id.clone(),
        transcription_id: snapshot.transcription_id.clone(),
        revision: snapshot.revision,
        language: snapshot.language.clone(),
        segments,
    }
}

fn emit_summary_progress(
    app: &AppHandle,
    meeting_id: &str,
    completed_steps: u32,
    total_steps: u32,
    stage: &'static str,
) {
    let _ = app.emit(
        "summary-progress",
        SummaryProgress {
            meeting_id: meeting_id.to_string(),
            completed_steps,
            total_steps,
            stage,
        },
    );
}

fn mechanically_format_transcript_text(text: &str) -> String {
    const FILLERS: [&str; 6] = ["えーと", "えっと", "ええと", "あのー", "あのう", "そのー"];
    const BOUNDARIES: [char; 7] = ['。', '！', '？', '!', '?', '、', ','];
    const COMMAS: [char; 3] = ['、', '，', ','];
    const SENTENCE_ENDS: [char; 5] = ['。', '！', '？', '!', '?'];

    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    let mut changed = false;
    while index < text.len() {
        let at_boundary = output
            .chars()
            .rev()
            .find(|character| !character.is_whitespace())
            .is_none_or(|character| BOUNDARIES.contains(&character));
        let filler = at_boundary
            .then(|| {
                FILLERS
                    .iter()
                    .find(|filler| text[index..].starts_with(**filler))
            })
            .flatten();
        if let Some(filler) = filler {
            while output.ends_with(char::is_whitespace) {
                output.pop();
            }
            index += filler.len();
            changed = true;
            while let Some(character) = text[index..].chars().next() {
                if character.is_whitespace() || COMMAS.contains(&character) {
                    index += character.len_utf8();
                } else {
                    break;
                }
            }
            if let Some(next) = text[index..].chars().next() {
                if SENTENCE_ENDS.contains(&next) {
                    if output
                        .chars()
                        .last()
                        .is_some_and(|last| COMMAS.contains(&last))
                    {
                        output.pop();
                    } else if output.is_empty() {
                        index += next.len_utf8();
                    }
                }
            }
            continue;
        }

        let character = text[index..]
            .chars()
            .next()
            .expect("index remains on a character boundary");
        output.push(character);
        index += character.len_utf8();
    }

    if changed {
        output.trim().to_string()
    } else {
        text.to_string()
    }
}

fn apply_text_corrections(
    text: &str,
    corrections: &[crate::transcription::context::TextCorrection],
) -> String {
    corrections
        .iter()
        .fold(text.to_string(), |value, correction| {
            value.replace(&correction.from, &correction.to)
        })
}

fn build_transcript_formatting_prompt(
    snapshot: &SummaryTranscriptSnapshot,
) -> Result<String, String> {
    let transcript = serde_json::to_string(snapshot)
        .map_err(|error| format!("文字起こしを整形用に変換できませんでした: {error}"))?;
    Ok(format!(
        "あなたは会議文字起こしの校正者です。次の文字起こしだけを根拠に、明白な誤字脱字、音声認識の明白な誤り、句読点だけを保守的に修正してください。言い換え、要約、情報の追加、話者の意図の変更、固有名詞の推測修正は禁止します。変更が必要な発話だけを返してください。JSON以外の文字、説明、Markdownコードフェンスは一切出力しないでください。形式は厳密に {{\"changes\":[{{\"segmentId\":\"実在するsegment id\",\"text\":\"修正後の発話全文\"}}]}} とし、変更がなければchangesを空配列にしてください。segmentIdの重複や存在しないIDを含めないでください。\n\n文字起こしJSON:\n{transcript}"
    ))
}

fn parse_transcript_formatting_content(
    output: &str,
    snapshot: &SummaryTranscriptSnapshot,
) -> Result<Vec<TranscriptFormattingChange>, String> {
    let trimmed = output.trim();
    let without_prefix = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let json = without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim();
    let content: TranscriptFormattingContent = serde_json::from_str(json)
        .map_err(|error| format!("AIの整形結果をJSONとして解析できませんでした: {error}"))?;
    validate_formatting_changes(&content.changes, snapshot)?;
    Ok(content.changes)
}

fn validate_formatting_changes(
    changes: &[TranscriptFormattingChange],
    snapshot: &SummaryTranscriptSnapshot,
) -> Result<(), String> {
    if changes.len() > snapshot.segments.len() {
        return Err("整形結果の変更件数が不正です。".into());
    }
    let known_ids: HashSet<&str> = snapshot
        .segments
        .iter()
        .map(|segment| segment.segment_id.as_str())
        .collect();
    let mut seen = HashSet::new();
    let mut total_bytes = 0usize;
    for change in changes {
        if !known_ids.contains(change.segment_id.as_str()) {
            return Err("整形結果に存在しない文字起こし区間が含まれています。".into());
        }
        if !seen.insert(change.segment_id.as_str()) {
            return Err("整形結果に同じ文字起こし区間が重複しています。".into());
        }
        total_bytes = total_bytes.saturating_add(change.text.len());
        if change.text.len() > MAX_SUMMARY_BYTES as usize
            || total_bytes > MAX_SUMMARY_BYTES as usize
        {
            return Err("整形結果が大きすぎます。".into());
        }
    }
    Ok(())
}

fn apply_formatting_changes(
    snapshot: &mut SummaryTranscriptSnapshot,
    changes: &[TranscriptFormattingChange],
) -> Result<(), String> {
    validate_formatting_changes(changes, snapshot)?;
    for change in changes {
        let segment = snapshot
            .segments
            .iter_mut()
            .find(|segment| segment.segment_id == change.segment_id)
            .ok_or_else(|| "整形対象の文字起こし区間が見つかりません。".to_string())?;
        segment.text = change.text.clone();
    }
    Ok(())
}

fn parse_generated_content(
    output: &str,
    snapshot: &SummaryTranscriptSnapshot,
) -> Result<SummaryContent, String> {
    let trimmed = output.trim();
    let without_prefix = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let json = without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim();
    let content: SummaryContent = serde_json::from_str(json)
        .map_err(|error| format!("AIの要約結果をJSONとして解析できませんでした: {error}"))?;
    validate_content(&content, snapshot)?;
    Ok(content)
}

fn validate_content(
    content: &SummaryContent,
    snapshot: &SummaryTranscriptSnapshot,
) -> Result<(), String> {
    if content.overview.trim().is_empty() || content.overview.len() > 100_000 {
        return Err("要約本文が空か、長すぎます。".into());
    }
    if content.decisions.len() > 500 || content.action_items.len() > 500 {
        return Err("要約項目が多すぎます。".into());
    }
    let ids: HashSet<&str> = snapshot
        .segments
        .iter()
        .map(|segment| segment.segment_id.as_str())
        .collect();
    let references = content
        .decisions
        .iter()
        .flat_map(|item| item.source_segment_ids.iter())
        .chain(
            content
                .action_items
                .iter()
                .flat_map(|item| item.source_segment_ids.iter()),
        );
    if references.into_iter().any(|id| !ids.contains(id.as_str())) {
        return Err("要約に存在しない文字起こし区間への参照が含まれています。".into());
    }
    Ok(())
}

fn summaries_directory(app: &AppHandle, meeting_id: &str) -> Result<PathBuf, String> {
    Ok(crate::meeting_store::meeting_directory(app, meeting_id)?.join("summaries"))
}

fn summary_path(
    app: &AppHandle,
    meeting_id: &str,
    transcription_id: &str,
) -> Result<PathBuf, String> {
    validate_identifier(transcription_id)?;
    Ok(summaries_directory(app, meeting_id)?.join(format!("{transcription_id}.json")))
}

fn read_summary(
    app: &AppHandle,
    meeting_id: &str,
    transcription_id: &str,
) -> Result<Option<MeetingSummary>, String> {
    let path = summary_path(app, meeting_id, transcription_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("保存済みの要約を確認できませんでした: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_SUMMARY_BYTES {
        return Err("保存済みの要約ファイルが不正です。".into());
    }
    let summary: MeetingSummary = serde_json::from_slice(
        &fs::read(&path)
            .map_err(|error| format!("保存済みの要約を読み込めませんでした: {error}"))?,
    )
    .map_err(|error| format!("保存済みの要約が壊れています: {error}"))?;
    if summary.schema_version != SCHEMA_VERSION
        || summary.meeting_id != meeting_id
        || summary.transcription_id != transcription_id
    {
        return Err("保存済みの要約形式または識別子が一致しません。".into());
    }
    Ok(Some(summary))
}

fn write_summary(app: &AppHandle, summary: &MeetingSummary) -> Result<(), String> {
    let directory = summaries_directory(app, &summary.meeting_id)?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("要約の保存先を作成できませんでした: {error}"))?;
    let path = summary_path(app, &summary.meeting_id, &summary.transcription_id)?;
    let temporary = directory.join(format!(
        ".{}.{}.tmp",
        summary.transcription_id,
        uuid::Uuid::now_v7()
    ));
    let bytes = serde_json::to_vec_pretty(summary)
        .map_err(|error| format!("要約を保存形式へ変換できませんでした: {error}"))?;
    if bytes.len() as u64 > MAX_SUMMARY_BYTES {
        return Err("要約が大きすぎるため保存できません。".into());
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("要約を書き込めませんでした: {error}"))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("要約を安全に書き込めませんでした: {error}"))?;
    drop(file);
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|error| format!("以前の要約を更新できませんでした: {error}"))?;
    }
    fs::rename(&temporary, &path)
        .map_err(|error| format!("要約の保存を確定できませんでした: {error}"))
}

fn resolve_agent_executable(app: &AppHandle, agent: &AcpAgentDefinition) -> Option<PathBuf> {
    if let Some(path) = env::var_os(agent.executable_env).map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    if let Some(path) = managed_agent_executable(app, agent) {
        if path.is_file() {
            return Some(path);
        }
    }
    find_on_path(agent.executable)
}

fn managed_agents_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("summary-agents"))
        .map_err(|error| format!("要約エージェントの保存先を取得できませんでした: {error}"))
}

fn managed_agent_directory(app: &AppHandle, agent: &AcpAgentDefinition) -> Result<PathBuf, String> {
    Ok(managed_agents_root(app)?.join(agent.id))
}

fn managed_agent_executable(app: &AppHandle, agent: &AcpAgentDefinition) -> Option<PathBuf> {
    let binary = if cfg!(target_os = "windows") {
        format!("{}.cmd", agent.binary)
    } else {
        agent.binary.to_string()
    };
    managed_agent_directory(app, agent)
        .ok()
        .map(|directory| directory.join("node_modules").join(".bin").join(binary))
}

#[derive(Clone, Copy)]
struct NodeDistribution {
    archive: &'static str,
    sha256: &'static str,
}

fn node_distribution() -> Option<NodeDistribution> {
    match (env::consts::OS, env::consts::ARCH) {
        ("windows", "x86_64") => Some(NodeDistribution {
            archive: "node-v24.18.0-win-x64.zip",
            sha256: "0ae68406b42d7725661da979b1403ec9926da205c6770827f33aac9d8f26e821",
        }),
        ("windows", "aarch64") => Some(NodeDistribution {
            archive: "node-v24.18.0-win-arm64.zip",
            sha256: "f274669adb93b1fd0fbf8f21fd078609e9dcc84333d4f2718d2dde3f9a161a01",
        }),
        ("macos", "aarch64") => Some(NodeDistribution {
            archive: "node-v24.18.0-darwin-arm64.tar.gz",
            sha256: "e1a97e14c99c803e96c7339403282ea05a499c32f8d83defe9ef5ec66f979ed1",
        }),
        ("macos", "x86_64") => Some(NodeDistribution {
            archive: "node-v24.18.0-darwin-x64.tar.gz",
            sha256: "dfd0dbd3e721503434df7b7205e719f61b3a3a31b2bcf9729b8b91fea240f080",
        }),
        ("linux", "aarch64") => Some(NodeDistribution {
            archive: "node-v24.18.0-linux-arm64.tar.gz",
            sha256: "6b4484c2190274175df9aa8f28e2d758a819cb1c1fe6ab481e2f95b463ab8508",
        }),
        ("linux", "x86_64") => Some(NodeDistribution {
            archive: "node-v24.18.0-linux-x64.tar.gz",
            sha256: "783130984963db7ba9cbd01089eaf2c2efb055c7c1693c943174b967b3050cb8",
        }),
        _ => None,
    }
}

fn managed_runtime_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("summary-runtime"))
        .map_err(|error| format!("要約ランタイムの保存先を取得できませんでした: {error}"))
}

fn managed_node_directory(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(managed_runtime_root(app)?.join(format!("node-v{NODE_VERSION}")))
}

fn managed_node_commands(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let root = managed_node_directory(app)?;
    let (node, npm_cli) = if cfg!(target_os = "windows") {
        (
            root.join("node.exe"),
            root.join("node_modules")
                .join("npm")
                .join("bin")
                .join("npm-cli.js"),
        )
    } else {
        (
            root.join("bin").join("node"),
            root.join("lib")
                .join("node_modules")
                .join("npm")
                .join("bin")
                .join("npm-cli.js"),
        )
    };
    if !node.is_file() || !npm_cli.is_file() {
        return Err("Echo管理の要約ランタイムがインストールされていません。".into());
    }
    Ok((node, npm_cli))
}

fn managed_node_bin_directory(app: &AppHandle) -> Option<PathBuf> {
    let root = managed_node_directory(app).ok()?;
    let directory = if cfg!(target_os = "windows") {
        root
    } else {
        root.join("bin")
    };
    directory.is_dir().then_some(directory)
}

async fn ensure_managed_node(app: &AppHandle) -> Result<(), String> {
    if managed_node_commands(app).is_ok() {
        cleanup_legacy_node_runtimes(app)?;
        return Ok(());
    }
    let distribution = node_distribution()
        .ok_or_else(|| "このOS・CPU向けの要約ランタイムは準備中です。".to_string())?;
    let runtime_root = managed_runtime_root(app)?;
    fs::create_dir_all(&runtime_root)
        .map_err(|error| format!("要約ランタイムの保存先を作成できませんでした: {error}"))?;
    let staging = runtime_root.join(format!(".node-{}.installing", uuid::Uuid::now_v7()));
    fs::create_dir(&staging)
        .map_err(|error| format!("要約ランタイムの一時領域を作成できませんでした: {error}"))?;
    let archive_path = staging.join(distribution.archive);
    let result = download_node_archive(distribution, &archive_path).await;
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let archive = archive_path.clone();
    let extraction_root = staging.clone();
    tauri::async_runtime::spawn_blocking(move || {
        extract_node_archive(&archive, &extraction_root, distribution.archive)
    })
    .await
    .map_err(|_| "要約ランタイムの展開処理を完了できませんでした。".to_string())??;
    let folder_name = distribution
        .archive
        .strip_suffix(".zip")
        .or_else(|| distribution.archive.strip_suffix(".tar.gz"))
        .ok_or_else(|| "要約ランタイムの配布形式が不正です。".to_string())?;
    let extracted = staging.join(folder_name);
    let target = managed_node_directory(app)?;
    if target.exists() {
        fs::remove_dir_all(&target)
            .map_err(|error| format!("以前の要約ランタイムを更新できませんでした: {error}"))?;
    }
    fs::rename(&extracted, &target)
        .map_err(|error| format!("要約ランタイムのインストールを確定できませんでした: {error}"))?;
    let _ = fs::remove_dir_all(&staging);
    managed_node_commands(app)?;
    cleanup_legacy_node_runtimes(app)
}

fn cleanup_legacy_node_runtimes(app: &AppHandle) -> Result<(), String> {
    let runtime_root = managed_runtime_root(app)?;
    if !runtime_root.exists() {
        return Ok(());
    }
    let canonical_root = fs::canonicalize(&runtime_root)
        .map_err(|error| format!("要約ランタイムの保存先を確認できませんでした: {error}"))?;
    for version in LEGACY_NODE_VERSIONS {
        let legacy = runtime_root.join(format!("node-v{version}"));
        if !legacy.exists() {
            continue;
        }
        let canonical_legacy = fs::canonicalize(&legacy)
            .map_err(|error| format!("以前の要約ランタイムを確認できませんでした: {error}"))?;
        if canonical_legacy.parent() != Some(canonical_root.as_path()) {
            return Err("以前の要約ランタイムの削除先が安全な範囲外です。".into());
        }
        fs::remove_dir_all(canonical_legacy)
            .map_err(|error| format!("以前の要約ランタイムを削除できませんでした: {error}"))?;
    }
    Ok(())
}

async fn download_node_archive(distribution: NodeDistribution, path: &Path) -> Result<(), String> {
    let url = format!(
        "https://nodejs.org/dist/v{NODE_VERSION}/{}",
        distribution.archive
    );
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("要約ランタイムの通信を準備できませんでした: {error}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("要約ランタイムをダウンロードできませんでした: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "要約ランタイムをダウンロードできませんでした（HTTP {}）。",
            response.status()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_NODE_ARCHIVE_BYTES)
    {
        return Err("要約ランタイムの配布ファイルが大きすぎます。".into());
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("要約ランタイムを書き込めませんでした: {error}"))?;
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|error| format!("要約ランタイムの受信に失敗しました: {error}"))?;
        total = total.saturating_add(chunk.len() as u64);
        if total > MAX_NODE_ARCHIVE_BYTES {
            return Err("要約ランタイムの配布ファイルが大きすぎます。".into());
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .map_err(|error| format!("要約ランタイムを書き込めませんでした: {error}"))?;
    }
    file.sync_all()
        .map_err(|error| format!("要約ランタイムを安全に保存できませんでした: {error}"))?;
    let actual = format!("{:x}", hasher.finalize());
    if actual != distribution.sha256 {
        return Err(
            "要約ランタイムのSHA-256が公式配布値と一致しません。ファイルは使用しません。".into(),
        );
    }
    Ok(())
}

fn extract_node_archive(
    archive_path: &Path,
    destination: &Path,
    archive_name: &str,
) -> Result<(), String> {
    if archive_name.ends_with(".zip") {
        let file = fs::File::open(archive_path)
            .map_err(|error| format!("要約ランタイムを開けませんでした: {error}"))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|error| format!("要約ランタイムのZIPが不正です: {error}"))?;
        archive
            .extract(destination)
            .map_err(|error| format!("要約ランタイムを展開できませんでした: {error}"))?;
    } else if archive_name.ends_with(".tar.gz") {
        let file = fs::File::open(archive_path)
            .map_err(|error| format!("要約ランタイムを開けませんでした: {error}"))?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        archive
            .unpack(destination)
            .map_err(|error| format!("要約ランタイムを展開できませんでした: {error}"))?;
    } else {
        return Err("要約ランタイムの配布形式に対応していません。".into());
    }
    Ok(())
}

fn install_statuses(app: &AppHandle) -> Vec<SummaryAgentInstallStatus> {
    let runtime_supported = node_distribution().is_some();
    ACP_AGENTS
        .iter()
        .map(|agent| {
            let managed = managed_agent_executable(app, agent).is_some_and(|path| path.is_file());
            let external = !managed
                && (env::var_os(agent.executable_env)
                    .map(PathBuf::from)
                    .is_some_and(|path| path.is_file())
                    || find_on_path(agent.executable).is_some());
            SummaryAgentInstallStatus {
                id: agent.id.into(),
                label: agent.label.into(),
                version: agent.version.into(),
                installed: managed || external,
                external,
                installable: runtime_supported,
                status_message: if managed {
                    format!("Echo管理版 v{}", agent.version)
                } else if external {
                    "システムにインストール済み".into()
                } else if runtime_supported {
                    "未インストール".into()
                } else {
                    "このOS・CPU向けの要約ランタイムは準備中です。".into()
                },
            }
        })
        .collect()
}

fn install_agent(app: &AppHandle, provider_id: &str) -> Result<(), String> {
    let agent = ACP_AGENTS
        .iter()
        .find(|agent| agent.id == provider_id)
        .ok_or_else(|| "要約エージェントIDが不正です。".to_string())?;
    let (node, npm_cli) = managed_node_commands(app)?;
    let root = managed_agents_root(app)?;
    fs::create_dir_all(&root)
        .map_err(|error| format!("要約エージェントの保存先を作成できませんでした: {error}"))?;
    let target = managed_agent_directory(app, agent)?;
    if managed_agent_executable(app, agent).is_some_and(|path| path.is_file()) {
        return Ok(());
    }
    let staging = root.join(format!(".{}-{}.installing", agent.id, uuid::Uuid::now_v7()));
    let package = format!("{}@{}", agent.package, agent.version);
    let result = run_npm_install(&node, &npm_cli, &staging, &package);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let binary = if cfg!(target_os = "windows") {
        staging
            .join("node_modules")
            .join(".bin")
            .join(format!("{}.cmd", agent.binary))
    } else {
        staging.join("node_modules").join(".bin").join(agent.binary)
    };
    if !binary.is_file() {
        let _ = fs::remove_dir_all(&staging);
        return Err("インストール済みパッケージにACP実行ファイルが含まれていません。".into());
    }
    if target.exists() {
        fs::remove_dir_all(&target)
            .map_err(|error| format!("以前の要約エージェントを更新できませんでした: {error}"))?;
    }
    fs::rename(&staging, &target)
        .map_err(|error| format!("要約エージェントのインストールを確定できませんでした: {error}"))
}

fn run_npm_install(
    node: &Path,
    npm_cli: &Path,
    prefix: &Path,
    package: &str,
) -> Result<(), String> {
    let mut command = Command::new(node);
    command.arg(npm_cli);
    configure_background_command(&mut command);
    if let Some(node_bin) = node.parent() {
        let mut paths = vec![node_bin.to_path_buf()];
        if let Some(current) = env::var_os("PATH") {
            paths.extend(env::split_paths(&current));
        }
        if let Ok(path) = env::join_paths(paths) {
            command.env("PATH", path);
        }
    }
    let mut child = command
        .args([
            "install",
            "--omit=dev",
            "--no-audit",
            "--no-fund",
            "--save-exact",
            "--prefix",
        ])
        .arg(prefix)
        .arg(package)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("npmを起動できませんでした: {error}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "npmの出力を取得できませんでした。".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "npmのエラー出力を取得できませんでした。".to_string())?;
    let stdout_reader = thread::spawn(move || {
        let mut value = String::new();
        let _ = stdout.read_to_string(&mut value);
        value
    });
    let stderr_reader = thread::spawn(move || {
        let mut value = String::new();
        let _ = stderr.read_to_string(&mut value);
        value
    });
    let deadline = Instant::now() + Duration::from_secs(300);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err("要約エージェントのインストールが5分以内に完了しませんでした。".into());
            }
            Err(error) => return Err(format!("npmの実行状態を確認できませんでした: {error}")),
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    if status.success() {
        return Ok(());
    }
    let detail = stderr
        .lines()
        .rev()
        .chain(stdout.lines().rev())
        .find(|line| !line.trim().is_empty())
        .unwrap_or("詳細不明");
    Err(format!(
        "要約エージェントをインストールできませんでした: {}",
        truncate(detail, 500)
    ))
}

fn configure_background_command(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        command.creation_flags(CREATE_NO_WINDOW);
    }

    #[cfg(not(windows))]
    let _ = command;
}

fn delete_managed_agent(app: &AppHandle, provider_id: &str) -> Result<(), String> {
    let agent = ACP_AGENTS
        .iter()
        .find(|agent| agent.id == provider_id)
        .ok_or_else(|| "要約エージェントIDが不正です。".to_string())?;
    let target = managed_agent_directory(app, agent)?;
    if !target.exists() {
        return Ok(());
    }
    let root = managed_agents_root(app)?;
    let canonical_root = fs::canonicalize(&root)
        .map_err(|error| format!("削除先を確認できませんでした: {error}"))?;
    let canonical_target = fs::canonicalize(&target)
        .map_err(|error| format!("削除対象を確認できませんでした: {error}"))?;
    if canonical_target.parent() != Some(canonical_root.as_path()) {
        return Err("要約エージェントの削除先が安全な範囲外です。".into());
    }
    fs::remove_dir_all(canonical_target)
        .map_err(|error| format!("要約エージェントを削除できませんでした: {error}"))?;
    if ACP_AGENTS
        .iter()
        .all(|candidate| !managed_agent_directory(app, candidate).is_ok_and(|path| path.exists()))
    {
        let runtime = managed_node_directory(app)?;
        if runtime.exists() {
            let runtime_root = managed_runtime_root(app)?;
            let canonical_runtime_root = fs::canonicalize(&runtime_root).map_err(|error| {
                format!("要約ランタイムの削除先を確認できませんでした: {error}")
            })?;
            let canonical_runtime = fs::canonicalize(&runtime)
                .map_err(|error| format!("要約ランタイムを確認できませんでした: {error}"))?;
            if canonical_runtime.parent() != Some(canonical_runtime_root.as_path()) {
                return Err("要約ランタイムの削除先が安全な範囲外です。".into());
            }
            fs::remove_dir_all(canonical_runtime)
                .map_err(|error| format!("共有要約ランタイムを削除できませんでした: {error}"))?;
        }
        cleanup_legacy_node_runtimes(app)?;
    }
    Ok(())
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    #[cfg(target_os = "windows")]
    let extensions: Vec<String> = env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .map(str::to_ascii_lowercase)
        .collect();
    for directory in env::split_paths(&path) {
        let direct = directory.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        #[cfg(target_os = "windows")]
        for extension in &extensions {
            let candidate = directory.join(format!("{name}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn validate_model_id(model_id: &str) -> Result<(), String> {
    if model_id.is_empty()
        || model_id.len() > 128
        || model_id.chars().any(|character| character.is_control())
    {
        return Err("要約モデルIDが不正です。".into());
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| "要約の識別子が不正です。".into())
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript_store::SummaryTranscriptSegment;

    fn snapshot() -> SummaryTranscriptSnapshot {
        SummaryTranscriptSnapshot {
            meeting_id: uuid::Uuid::now_v7().to_string(),
            transcription_id: uuid::Uuid::now_v7().to_string(),
            revision: 3,
            language: "ja".into(),
            segments: vec![SummaryTranscriptSegment {
                segment_id: "segment-1".into(),
                speaker: "岡本".into(),
                start_ms: 1000,
                end_ms: 2000,
                text: "修正版です".into(),
            }],
        }
    }

    #[test]
    fn prompt_uses_corrected_transcript_and_labels() {
        let prompt = build_prompt(&snapshot()).expect("prompt");
        assert!(prompt.contains("修正版です"));
        assert!(prompt.contains("岡本"));
        assert!(prompt.contains("segment-1"));
    }

    #[test]
    fn splits_two_hour_transcript_into_fifteen_minute_chunks() {
        let mut transcript = snapshot();
        transcript.segments = (0..120)
            .map(|minute| SummaryTranscriptSegment {
                segment_id: format!("segment-{minute}"),
                speaker: "話者".into(),
                start_ms: minute * 60_000,
                end_ms: (minute + 1) * 60_000,
                text: format!("{minute}分の発話です。"),
            })
            .collect();

        let chunks = split_summary_snapshot(&transcript);

        assert_eq!(chunks.len(), 8);
        assert_eq!(summary_total_steps(chunks.len()), 9);
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.segments.len())
                .sum::<usize>(),
            120
        );
        assert_eq!(chunks[0].segments[0].segment_id, "segment-0");
        assert_eq!(
            chunks[7].segments.last().expect("last").segment_id,
            "segment-119"
        );
    }

    #[test]
    fn merge_prompt_preserves_source_segment_ids() {
        let partials = vec![SummaryContent {
            overview: "概要".into(),
            decisions: vec![SummaryReference {
                text: "決定".into(),
                source_segment_ids: vec!["segment-1".into()],
            }],
            action_items: Vec::new(),
        }];

        let prompt = build_summary_merge_prompt(&partials).expect("merge prompt");

        assert!(prompt.contains("segment-1"));
        assert!(prompt.contains("重複する内容をまとめ"));
    }

    #[test]
    fn mechanically_removes_fillers_and_adjacent_delimiters() {
        assert_eq!(
            mechanically_format_transcript_text(" えーと、今日は会議です。"),
            "今日は会議です。"
        );
        assert_eq!(
            mechanically_format_transcript_text("はい、 えっと、次へ進みます。"),
            "はい、次へ進みます。"
        );
        assert_eq!(
            mechanically_format_transcript_text("確認します。 ええと 次の項目です。"),
            "確認します。次の項目です。"
        );
    }

    #[test]
    fn local_dictionary_correction_is_deterministic() {
        let corrections = vec![crate::transcription::context::TextCorrection {
            from: "むつなエコー".into(),
            to: "Mutsuna Echo".into(),
        }];
        assert_eq!(
            apply_text_corrections("むつなエコーを使います。", &corrections),
            "Mutsuna Echoを使います。"
        );
    }

    #[test]
    fn mechanical_formatting_preserves_meaningful_and_embedded_words() {
        for text in [
            "あの人に確認します。",
            "その方法で進めます。",
            "これはえっと違います。",
            "English text",
        ] {
            assert_eq!(mechanically_format_transcript_text(text), text);
        }
    }

    #[test]
    fn mechanically_handles_repeated_and_filler_only_segments() {
        assert_eq!(
            mechanically_format_transcript_text("えーと、あのー、そのー。"),
            ""
        );
        assert_eq!(mechanically_format_transcript_text(""), "");
    }

    #[test]
    fn parses_valid_transcript_formatting_changes() {
        let changes = parse_transcript_formatting_content(
            r#"{"changes":[{"segmentId":"segment-1","text":"修正後です"}]}"#,
            &snapshot(),
        )
        .expect("formatting changes");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].text, "修正後です");
    }

    #[test]
    fn rejects_unknown_or_duplicate_formatting_segments() {
        let unknown = r#"{"changes":[{"segmentId":"unknown","text":"修正"}]}"#;
        assert!(parse_transcript_formatting_content(unknown, &snapshot()).is_err());

        let duplicate = r#"{"changes":[{"segmentId":"segment-1","text":"修正1"},{"segmentId":"segment-1","text":"修正2"}]}"#;
        assert!(parse_transcript_formatting_content(duplicate, &snapshot()).is_err());
    }

    #[test]
    fn formatting_prompt_forbids_invented_content() {
        let prompt = build_transcript_formatting_prompt(&snapshot()).expect("formatting prompt");
        assert!(prompt.contains("情報の追加"));
        assert!(prompt.contains("固有名詞の推測修正"));
        assert!(prompt.contains("修正版です"));
    }

    #[test]
    fn rejects_unknown_segment_reference() {
        let content = SummaryContent {
            overview: "要約".into(),
            decisions: vec![SummaryReference {
                text: "決定".into(),
                source_segment_ids: vec!["unknown".into()],
            }],
            action_items: vec![],
        };
        assert!(validate_content(&content, &snapshot()).is_err());
    }

    #[test]
    fn accepts_known_segment_reference() {
        let content = SummaryContent {
            overview: "要約".into(),
            decisions: vec![SummaryReference {
                text: "決定".into(),
                source_segment_ids: vec!["segment-1".into()],
            }],
            action_items: vec![],
        };
        assert!(validate_content(&content, &snapshot()).is_ok());
    }

    #[test]
    fn reads_model_options_from_acp_session() {
        let models = model_definitions_from_session(
            &serde_json::json!({
                "configOptions": [{
                    "id": "model",
                    "category": "model",
                    "currentValue": "gpt-5.4",
                    "options": [
                        { "value": "gpt-5.4", "name": "GPT-5.4", "description": "推奨" },
                        { "value": "gpt-5.3-codex", "name": "GPT-5.3 Codex" }
                    ]
                }]
            }),
            "Codex",
        );
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-5.4");
        assert_eq!(models[0].label, "GPT-5.4");
        assert!(models[0].is_default);
        assert!(!models[1].is_default);
    }

    #[test]
    fn falls_back_to_agent_default_when_acp_has_no_model_config() {
        let models = model_definitions_from_session(
            &serde_json::json!({ "sessionId": "session-1" }),
            "Claude Code",
        );
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "default");
        assert_eq!(models[0].label, "Claude Codeの既定モデル");
        assert!(models[0].is_default);
    }

    #[test]
    fn exposes_supported_cloudflare_summary_models() {
        let models = cloudflare_summary_models();
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].id, CLOUDFLARE_GLM_MODEL_ID);
        assert!(models[0].is_default);
        assert!(models
            .iter()
            .all(|model| validate_cloudflare_model_id(&model.id).is_ok()));
        assert!(validate_cloudflare_model_id("@cf/unknown/model").is_err());
    }
}
