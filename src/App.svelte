<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { SelectedAudioFile, Transcript, TranscriptionUsage } from "./lib/types/transcript";

  let apiKey = $state("");
  let hasApiKey = $state(false);
  let loading = $state(true);
  let saving = $state(false);
  let deleting = $state(false);
  let selecting = $state(false);
  let transcribing = $state(false);
  let usageLoading = $state(false);
  let selectedAudio = $state<SelectedAudioFile | null>(null);
  let transcript = $state<Transcript | null>(null);
  let transcriptionUsage = $state<TranscriptionUsage | null>(null);
  let usageError = $state("");
  let message = $state("");
  let errorMessage = $state("");

  const busy = $derived(loading || saving || deleting || selecting || transcribing);
  const canTranscribe = $derived(hasApiKey && selectedAudio !== null && !busy);

  function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    return "予期しないエラーが発生しました。";
  }

  function formatTimestamp(milliseconds: number): string {
    const totalSeconds = Math.floor(milliseconds / 1000);
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;

    return hours > 0
      ? `${hours.toString().padStart(2, "0")}:${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`
      : `${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`;
  }

  function formatFileSize(bytes: number): string {
    if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function formatEstimatedCost(costUsd: number): string {
    if (costUsd < 0.0001) return "$0.0001未満";
    return `約 $${costUsd < 0.01 ? costUsd.toFixed(4) : costUsd.toFixed(2)}`;
  }

  function formatDuration(milliseconds: number): string {
    const totalMinutes = Math.max(0, Math.round(milliseconds / 60_000));
    const hours = Math.floor(totalMinutes / 60);
    const minutes = totalMinutes % 60;
    if (hours === 0) return `${minutes}分`;
    return minutes === 0 ? `${hours}時間` : `${hours}時間 ${minutes}分`;
  }

  function formatResetDate(unixSeconds: number): string {
    return new Intl.DateTimeFormat("ja-JP", {
      year: "numeric",
      month: "long",
      day: "numeric"
    }).format(new Date(unixSeconds * 1_000));
  }

  async function refreshUsage() {
    if (!hasApiKey || usageLoading) return;

    usageLoading = true;
    usageError = "";
    try {
      transcriptionUsage = await invoke<TranscriptionUsage>("get_transcription_usage");
    } catch (error) {
      transcriptionUsage = null;
      usageError = errorText(error);
    } finally {
      usageLoading = false;
    }
  }

  async function refreshStatus() {
    hasApiKey = await invoke<boolean>("has_api_key");
  }

  $effect(() => {
    void (async () => {
      try {
        await refreshStatus();
        if (hasApiKey) await refreshUsage();
      } catch (error) {
        errorMessage = errorText(error);
      } finally {
        loading = false;
      }
    })();
  });

  async function saveApiKey(event: SubmitEvent) {
    event.preventDefault();
    let submittedApiKey = apiKey.trim();
    apiKey = "";
    saving = true;
    message = "";
    errorMessage = "";

    try {
      const modelsAccessible = await invoke<boolean>("save_api_key", { apiKey: submittedApiKey });
      hasApiKey = true;
      message = modelsAccessible
        ? "APIキーを確認し、安全に保存しました。"
        : "制限付きAPIキーとして保存しました。各権限は利用時に確認します。";
      await refreshUsage();
    } catch (error) {
      errorMessage = errorText(error);
    } finally {
      submittedApiKey = "";
      saving = false;
    }
  }

  async function deleteApiKey() {
    if (!window.confirm("保存済みのElevenLabs APIキーを削除しますか？")) return;

    deleting = true;
    message = "";
    errorMessage = "";

    try {
      await invoke("delete_api_key");
      hasApiKey = false;
      transcriptionUsage = null;
      usageError = "";
      apiKey = "";
      message = "APIキーを削除しました。";
    } catch (error) {
      errorMessage = errorText(error);
    } finally {
      deleting = false;
    }
  }

  async function selectAudioFile() {
    selecting = true;
    message = "";
    errorMessage = "";

    try {
      const selected = await invoke<SelectedAudioFile | null>("select_audio_file");
      if (selected) {
        selectedAudio = selected;
        transcript = null;
      }
    } catch (error) {
      errorMessage = errorText(error);
    } finally {
      selecting = false;
    }
  }

  async function transcribeAudio() {
    if (!canTranscribe) return;

    transcribing = true;
    transcript = null;
    message = "";
    errorMessage = "";

    try {
      transcript = await invoke<Transcript>("transcribe_selected_audio");
      message = transcript.segments.length > 0
        ? "文字起こしが完了しました。"
        : "文字起こしは完了しましたが、発話を検出できませんでした。";
      await refreshUsage();
    } catch (error) {
      errorMessage = errorText(error);
    } finally {
      transcribing = false;
    }
  }
</script>

<svelte:head>
  <title>Mutsuna Echo</title>
</svelte:head>

<main class="shell">
  <header class="hero">
    <p class="eyebrow">Mutsuna Echo</p>
    <h1>会話を、読み返せる形へ。</h1>
    <p class="lead">音声ファイルを選択して、話者とタイムスタンプ付きで文字起こしします。</p>
  </header>

  {#if message}
    <p class="notice success" role="status">{message}</p>
  {/if}
  {#if errorMessage}
    <p class="notice error" role="alert">{errorMessage}</p>
  {/if}

  <section class="card transcription-card" aria-busy={selecting || transcribing}>
    <div class="section-heading">
      <div>
        <p class="step">Step 1</p>
        <h2>音声ファイル</h2>
      </div>
      <span class:ready={selectedAudio} class="badge">
        {selectedAudio ? "選択済み" : "未選択"}
      </span>
    </div>

    <button class="file-picker" type="button" onclick={selectAudioFile} disabled={busy}>
      <span class="file-icon" aria-hidden="true">♪</span>
      <span class="file-copy">
        <strong>{selecting ? "ファイルを確認中…" : selectedAudio?.name ?? "音声ファイルを選択"}</strong>
        <small>
          {selectedAudio
            ? `${formatTimestamp(selectedAudio.durationMs)} · ${formatFileSize(selectedAudio.sizeBytes)} · クリックして変更`
            : "MP3・M4A・WAV・FLAC"}
        </small>
      </span>
    </button>

    {#if selectedAudio}
      <div class="cost-estimate">
        <div>
          <span>推定コスト</span>
          <strong>{formatEstimatedCost(selectedAudio.estimatedCostUsd)}</strong>
        </div>
        <small>
          公開単価 ${selectedAudio.pricingRateUsdPerHour.toFixed(2)}/時間
          （{selectedAudio.pricingVerifiedOn}確認）に基づく概算です。プラン内枠や請求時の丸めにより実際の請求額とは異なる場合があります。
        </small>
      </div>
    {/if}

    <div class="action-row">
      <div>
        <p class="step">Step 2</p>
        <p class="action-help">
          {hasApiKey ? "日本語・話者分離・単語タイムスタンプ" : "先にAPIキーを設定してください"}
        </p>
      </div>
      <button class="primary" type="button" onclick={transcribeAudio} disabled={!canTranscribe}>
        {transcribing ? "文字起こし中…" : "文字起こし開始"}
      </button>
    </div>
  </section>

  {#if hasApiKey}
    <section class="card usage-card" aria-busy={usageLoading}>
      <div class="section-heading">
        <div>
          <p class="step">Usage</p>
          <h2>ElevenLabs 利用状況</h2>
        </div>
        <button class="refresh" type="button" onclick={refreshUsage} disabled={usageLoading}>
          {usageLoading ? "更新中…" : "更新"}
        </button>
      </div>

      {#if usageLoading && !transcriptionUsage}
        <p class="usage-placeholder" role="status">契約枠と使用量を確認しています…</p>
      {:else}
        <div class="usage-grid">
          <div>
            <span>今月利用可能</span>
            <strong>
              {transcriptionUsage?.availableDurationMs !== null && transcriptionUsage?.availableDurationMs !== undefined
                ? formatDuration(transcriptionUsage.availableDurationMs)
                : "取得できません"}
            </strong>
          </div>
          <div>
            <span>今月使用済み（Scribe換算）</span>
            <strong>
              {transcriptionUsage?.usedDurationMs !== null && transcriptionUsage?.usedDurationMs !== undefined
                ? formatDuration(transcriptionUsage.usedDurationMs)
                : "取得できません"}
            </strong>
          </div>
        </div>
      {/if}

      {#if transcriptionUsage?.tier || transcriptionUsage?.resetsAtUnix}
        <p class="usage-meta">
          {transcriptionUsage.tier ? `${transcriptionUsage.tier}プラン` : ""}
          {transcriptionUsage.tier && transcriptionUsage.resetsAtUnix ? " · " : ""}
          {transcriptionUsage.resetsAtUnix ? `${formatResetDate(transcriptionUsage.resetsAtUnix)}にリセット` : ""}
        </p>
      {/if}
      {#if transcriptionUsage?.warning}
        <p class="usage-warning" role="alert">{transcriptionUsage.warning}</p>
      {/if}
      {#if usageError}
        <p class="usage-warning" role="alert">{usageError}</p>
      {/if}
      <p class="usage-note">
        時間は契約枠と製品別クレジット使用量をScribe v2の公開枠で換算した値です。他のElevenLabs機能や追加機能を使うと実際の音声時間と異なる場合があります。
      </p>
    </section>
  {/if}

  {#if transcript}
    <section class="card transcript-card" aria-label="文字起こし結果">
      <div class="section-heading transcript-heading">
        <div>
          <p class="step">Transcript</p>
          <h2>文字起こし結果</h2>
        </div>
        <span class="model">{transcript.model} · {transcript.language}</span>
      </div>

      {#if transcript.segments.length > 0}
        <div class="segments">
          {#each transcript.segments as segment, index (`${segment.speaker}-${segment.startMs}-${index}`)}
            <article class="segment">
              <div class="segment-meta">
                <strong>{segment.speaker}</strong>
                <time>{formatTimestamp(segment.startMs)}</time>
              </div>
              <p>{segment.text}</p>
            </article>
          {/each}
        </div>
      {:else}
        <p class="empty-result">発話は検出されませんでした。</p>
      {/if}
    </section>
  {/if}

  <section class="card settings-card" aria-busy={loading || saving || deleting}>
    <div class="section-heading">
      <div>
        <p class="step">Settings</p>
        <h2>ElevenLabs APIキー</h2>
        <p class="help">キーはOSの暗号化機能で保護し、画面へ読み戻しません。</p>
      </div>
      {#if loading}
        <span class="badge">確認中</span>
      {:else if hasApiKey}
        <span class="badge ready">設定済み</span>
      {:else}
        <span class="badge">未設定</span>
      {/if}
    </div>

    <form onsubmit={saveApiKey}>
      <label for="api-key">API key</label>
      <div class="input-row">
        <input
          id="api-key"
          type="password"
          placeholder={hasApiKey ? "新しいキーに置き換える" : "sk_..."}
          autocomplete="off"
          spellcheck="false"
          bind:value={apiKey}
          disabled={busy}
        />
        <button class="secondary" type="submit" disabled={busy || !apiKey.trim()}>
          {saving ? "確認中…" : hasApiKey ? "更新" : "保存"}
        </button>
      </div>
    </form>

    <p class="security-note">
      ElevenLabsではSpeech to Text、User Read、Workspace Analytics Full Readだけを許可し、利用上限を設定してください。
    </p>

    {#if hasApiKey}
      <button class="danger" type="button" onclick={deleteApiKey} disabled={busy}>
        {deleting ? "削除中…" : "保存済みキーを削除"}
      </button>
    {/if}
  </section>
</main>

<style>
  :global(:root) {
    font-family: Inter, "Noto Sans JP", system-ui, sans-serif;
    color: #17211b;
    background: #f2f5f1;
    font-synthesis: none;
  }

  :global(body) {
    margin: 0;
    min-width: 320px;
    min-height: 100vh;
  }

  :global(button),
  :global(input) {
    font: inherit;
  }

  .shell {
    width: min(760px, calc(100% - 40px));
    margin: 0 auto;
    padding: 56px 0 72px;
  }

  .hero {
    margin-bottom: 30px;
  }

  .eyebrow,
  .step {
    margin: 0 0 7px;
    color: #23704a;
    font-size: 0.74rem;
    font-weight: 800;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  h1,
  h2,
  p {
    margin-top: 0;
  }

  h1 {
    max-width: 650px;
    margin-bottom: 12px;
    font-size: clamp(2.1rem, 6vw, 3.4rem);
    line-height: 1.08;
    letter-spacing: -0.05em;
  }

  h2 {
    margin-bottom: 0;
    font-size: 1.14rem;
  }

  .lead,
  .help,
  .action-help,
  .security-note,
  .empty-result {
    color: #647068;
  }

  .lead {
    max-width: 610px;
    margin-bottom: 0;
  }

  .card {
    margin-top: 18px;
    padding: 26px;
    border: 1px solid #d8dfda;
    border-radius: 18px;
    background: #fff;
    box-shadow: 0 16px 42px rgb(29 54 39 / 7%);
  }

  .section-heading,
  .action-row,
  .input-row,
  .segment-meta {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .section-heading,
  .action-row {
    justify-content: space-between;
  }

  .cost-estimate {
    display: grid;
    gap: 5px;
    margin-top: 12px;
    padding: 13px 15px;
    border-radius: 10px;
    color: #315541;
    background: #edf7f1;
  }

  .cost-estimate div {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }

  .cost-estimate span,
  .cost-estimate small {
    font-size: 0.78rem;
  }

  .usage-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
    margin-top: 20px;
  }

  .usage-grid div {
    display: grid;
    gap: 6px;
    padding: 16px;
    border-radius: 12px;
    background: #f3f7f4;
  }

  .usage-grid span,
  .usage-meta,
  .usage-placeholder,
  .usage-note,
  .usage-warning {
    font-size: 0.82rem;
  }

  .usage-grid span,
  .usage-meta,
  .usage-placeholder,
  .usage-note {
    color: #647068;
  }

  .usage-grid strong {
    color: #235c3e;
    font-size: 1.15rem;
  }

  .usage-meta,
  .usage-placeholder,
  .usage-note,
  .usage-warning {
    margin: 12px 0 0;
    line-height: 1.55;
  }

  .usage-warning {
    padding: 11px 12px;
    border-radius: 9px;
    color: #8b4a19;
    background: #fff4e8;
  }

  .refresh {
    min-height: auto;
    padding: 7px 11px;
    border: 1px solid #c5cec8;
    color: #315541;
    background: #f8faf8;
    font-size: 0.78rem;
  }

  .cost-estimate strong {
    font-size: 1.02rem;
  }

  .cost-estimate small {
    color: #607269;
    line-height: 1.5;
  }

  .badge,
  .model {
    flex: none;
    padding: 6px 10px;
    border-radius: 999px;
    color: #667068;
    background: #edf0ed;
    font-size: 0.76rem;
    font-weight: 700;
  }

  .badge.ready {
    color: #176440;
    background: #e2f5e9;
  }

  .file-picker {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 14px;
    margin-top: 22px;
    padding: 16px;
    border: 1px dashed #9db2a4;
    border-radius: 12px;
    color: #17211b;
    background: #f8faf8;
    text-align: left;
  }

  .file-icon {
    display: grid;
    width: 40px;
    height: 40px;
    flex: none;
    place-items: center;
    border-radius: 10px;
    color: #fff;
    background: #2c8058;
    font-size: 1.2rem;
  }

  .file-copy {
    display: grid;
    min-width: 0;
    gap: 3px;
  }

  .file-copy strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file-copy small {
    color: #6a766e;
  }

  .action-row {
    margin-top: 22px;
    padding-top: 20px;
    border-top: 1px solid #e7ebe8;
  }

  .action-help,
  .help,
  .security-note {
    margin-bottom: 0;
    font-size: 0.86rem;
  }

  button {
    min-height: 44px;
    padding: 0 18px;
    border-radius: 10px;
    cursor: pointer;
    font-weight: 750;
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  .primary {
    border: 1px solid #246b49;
    color: #fff;
    background: #246b49;
  }

  .secondary {
    border: 1px solid #b7c4bb;
    color: #234c37;
    background: #f6f8f6;
  }

  .notice {
    margin: 14px 0 0;
    padding: 12px 14px;
    border-radius: 10px;
    font-size: 0.9rem;
  }

  .notice.success {
    color: #175f3e;
    background: #e7f5ec;
  }

  .notice.error {
    color: #9a3028;
    background: #fff0ee;
  }

  .transcript-heading {
    margin-bottom: 4px;
  }

  .segments {
    margin-top: 22px;
  }

  .segment {
    padding: 20px 0;
    border-top: 1px solid #e7ebe8;
  }

  .segment:first-child {
    border-top: 0;
  }

  .segment-meta {
    margin-bottom: 9px;
  }

  .segment-meta strong {
    color: #23704a;
    font-size: 0.86rem;
  }

  .segment-meta time {
    color: #78827b;
    font-size: 0.78rem;
    font-variant-numeric: tabular-nums;
  }

  .segment p {
    margin-bottom: 0;
    line-height: 1.8;
    white-space: pre-wrap;
  }

  .settings-card {
    margin-top: 30px;
  }

  form {
    margin-top: 22px;
  }

  label {
    display: block;
    margin-bottom: 8px;
    font-size: 0.84rem;
    font-weight: 700;
  }

  input {
    box-sizing: border-box;
    min-width: 0;
    height: 44px;
    flex: 1;
    padding: 0 13px;
    border: 1px solid #bdc8c0;
    border-radius: 10px;
    color: #17211b;
    background: #fbfcfb;
    outline: none;
  }

  input:focus {
    border-color: #2c8058;
    box-shadow: 0 0 0 3px rgb(44 128 88 / 14%);
  }

  .security-note {
    margin-top: 12px;
  }

  .danger {
    min-height: auto;
    margin-top: 18px;
    padding: 0;
    border: 0;
    color: #a33a31;
    background: transparent;
    font-size: 0.84rem;
  }

  @media (max-width: 600px) {
    .shell {
      width: min(100% - 28px, 760px);
      padding: 36px 0 52px;
    }

    .card {
      padding: 20px;
    }

    .action-row,
    .input-row {
      align-items: stretch;
      flex-direction: column;
    }

    .primary,
    .secondary {
      width: 100%;
    }

    .model {
      display: none;
    }

    .usage-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
