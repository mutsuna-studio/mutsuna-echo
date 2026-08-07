<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Alert, AlertDescription } from "@mutsuna/ui/alert";
  import { ThemeProvider, createTheme } from "@mutsuna/ui/theme";
  import ApiKeySettings from "./lib/components/ApiKeySettings.svelte";
  import AudioInputPanel from "./lib/components/AudioInputPanel.svelte";
  import TranscriptView from "./lib/components/TranscriptView.svelte";
  import UsagePanel from "./lib/components/UsagePanel.svelte";
  import type { SelectedAudioFile, Transcript, TranscriptionUsage } from "./lib/types/transcript";

  const echoTheme = createTheme("custom", "oklch(0.49 0.12 154)");

  let hasApiKey = $state(false);
  let loading = $state(true);
  let saving = $state(false);
  let deleting = $state(false);
  let selecting = $state(false);
  let transcribing = $state(false);
  let usageLoading = $state(false);
  let recordingBusy = $state(false);
  let selectedAudio = $state<SelectedAudioFile | null>(null);
  let transcript = $state<Transcript | null>(null);
  let transcriptionUsage = $state<TranscriptionUsage | null>(null);
  let usageError = $state("");
  let message = $state("");
  let errorMessage = $state("");

  const busy = $derived(loading || saving || deleting || selecting || transcribing || recordingBusy);
  const recordingDisabled = $derived(loading || saving || deleting || selecting || transcribing);
  const canTranscribe = $derived(hasApiKey && selectedAudio !== null && !busy);

  function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    return "予期しないエラーが発生しました。";
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

  $effect(() => {
    void (async () => {
      try {
        hasApiKey = await invoke<boolean>("has_api_key");
        if (hasApiKey) await refreshUsage();
      } catch (error) {
        errorMessage = errorText(error);
      } finally {
        loading = false;
      }
    })();
  });

  async function saveApiKey(apiKey: string) {
    saving = true;
    message = "";
    errorMessage = "";

    try {
      const modelsAccessible = await invoke<boolean>("save_api_key", { apiKey });
      hasApiKey = true;
      message = modelsAccessible
        ? "APIキーを確認し、安全に保存しました。"
        : "制限付きAPIキーとして保存しました。各権限は利用時に確認します。";
      await refreshUsage();
    } catch (error) {
      errorMessage = errorText(error);
    } finally {
      saving = false;
    }
  }

  async function deleteApiKey() {
    deleting = true;
    message = "";
    errorMessage = "";
    try {
      await invoke("delete_api_key");
      hasApiKey = false;
      transcriptionUsage = null;
      usageError = "";
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

  function handleRecordedAudio(audio: SelectedAudioFile) {
    selectedAudio = audio;
    transcript = null;
  }

  function showMessage(nextMessage: string) {
    message = nextMessage;
    if (nextMessage) errorMessage = "";
  }

  function showError(nextError: string) {
    errorMessage = nextError;
    if (nextError) message = "";
  }
</script>

<svelte:head>
  <title>Mutsuna Echo</title>
</svelte:head>

<ThemeProvider theme={echoTheme}>
  <main class="shell">
  <header class="hero">
    <p class="eyebrow">Mutsuna Echo</p>
    <h1>会話を、読み返せる形へ。</h1>
    <p class="lead">音声ファイルを選択して、話者とタイムスタンプ付きで文字起こしします。</p>
  </header>

  {#if message}
    <Alert class="notice success" role="status"><AlertDescription>{message}</AlertDescription></Alert>
  {/if}
  {#if errorMessage}
    <Alert class="notice error" variant="destructive" role="alert"><AlertDescription>{errorMessage}</AlertDescription></Alert>
  {/if}

  <AudioInputPanel
    {selectedAudio}
    {selecting}
    {transcribing}
    {recordingBusy}
    {busy}
    {recordingDisabled}
    {hasApiKey}
    {canTranscribe}
    onSelect={selectAudioFile}
    onTranscribe={transcribeAudio}
    onRecordedAudio={handleRecordedAudio}
    onRecordingBusyChange={(value) => recordingBusy = value}
    onMessage={showMessage}
    onError={showError}
  />

  {#if hasApiKey}
    <UsagePanel
      usage={transcriptionUsage}
      loading={usageLoading}
      error={usageError}
      onRefresh={refreshUsage}
    />
  {/if}

  {#if transcript}
    <TranscriptView {transcript} />
  {/if}

  <ApiKeySettings
    {loading}
    {saving}
    {deleting}
    {hasApiKey}
    {busy}
    onSave={saveApiKey}
    onDelete={deleteApiKey}
  />
  </main>
</ThemeProvider>
