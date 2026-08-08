<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { tick } from "svelte";
  import {
    Toaster,
    showErrorToast,
    showSuccessToast,
    showWarningToast
  } from "@mutsuna/ui/sonner";
  import { ThemeProvider, createTheme } from "@mutsuna/ui/theme";
  import ApiKeySettings from "./lib/components/ApiKeySettings.svelte";
  import AudioInputPanel from "./lib/components/AudioInputPanel.svelte";
  import TranscriptView from "./lib/components/TranscriptView.svelte";
  import UsagePanel from "./lib/components/UsagePanel.svelte";
  import type { TranscriptionProviderId } from "./lib/providers";
  import type { PendingAction } from "./lib/types/pending-action";
  import type {
    SelectedAudioFile,
    Transcript,
    TranscriptionResult,
    TranscriptionSession,
    TranscriptionUsage
  } from "./lib/types/transcript";

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
  let transcriptionProvider = $state<TranscriptionProviderId>("elevenlabs");
  // Transcriptは読み取り専用の大きな値なので、深いProxy化を避ける。
  let transcript = $state.raw<Transcript | null>(null);
  let transcriptRevision = $state(0);
  let transcriptionUsage = $state<TranscriptionUsage | null>(null);
  let usageError = $state("");
  let lastErrorToast = $state("");
  let lastErrorToastAt = $state(0);
  let pendingActionPromise: Promise<void> | null = null;
  let pendingActionId = "";

  const busy = $derived(loading || saving || deleting || selecting || transcribing || recordingBusy);
  const recordingDisabled = $derived(loading || saving || deleting || selecting || transcribing);
  const providerConfigured = $derived(
    transcriptionProvider === "elevenlabs" && hasApiKey
  );
  const canTranscribe = $derived(providerConfigured && selectedAudio !== null && !busy);

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

  function receivePendingAction(action: PendingAction): Promise<void> {
    if (pendingActionPromise && pendingActionId === action.id) return pendingActionPromise;
    if (pendingActionPromise) {
      return pendingActionPromise.then(() => receivePendingAction(action));
    }
    pendingActionId = action.id;
    pendingActionPromise = applyPendingAction(action).finally(() => {
      pendingActionPromise = null;
      pendingActionId = "";
    });
    return pendingActionPromise;
  }

  async function applyPendingAction(action: PendingAction) {
    const audio = await invoke<SelectedAudioFile>("receive_pending_action", {
      actionId: action.id
    });
    selectedAudio = audio;
    await restoreSelectedTranscript();
    await focusTranscriptionAction();
    await invoke("acknowledge_pending_action", { actionId: action.id });
    showSuccessToast(
      "録音を保存しました。",
      "音声を選択しました。設定を確認して文字起こしを開始できます。"
    );
  }

  async function focusTranscriptionAction() {
    await tick();
    await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
    const section = document.querySelector<HTMLElement>("[data-transcription-action]");
    const button = section?.querySelector<HTMLButtonElement>("button");
    if (!section || !button) {
      throw new Error("文字起こし操作を画面に準備できませんでした。");
    }
    section.scrollIntoView({ behavior: "smooth", block: "center" });
    button.focus({ preventScroll: true });
  }

  $effect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    void (async () => {
      try {
        unlisten = await listen<PendingAction>("pending-action-available", ({ payload }) => {
          if (!cancelled) {
            void receivePendingAction(payload).catch((error) => showError(errorText(error)));
          }
        });
        const [nextHasApiKey, session, pendingAction] = await Promise.all([
          invoke<boolean>("has_api_key"),
          invoke<TranscriptionSession>("get_transcription_session"),
          invoke<PendingAction | null>("get_pending_action")
        ]);
        if (cancelled) return;
        hasApiKey = nextHasApiKey;
        selectedAudio = session.selectedAudio;
        transcribing = session.transcribing;
        if (pendingAction) {
          await receivePendingAction(pendingAction);
        } else if (selectedAudio) {
          await restoreSelectedTranscript();
        }
        if (hasApiKey) await refreshUsage();
      } catch (error) {
        showError(errorText(error));
      } finally {
        loading = false;
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  });

  // WebViewを閉じている間に進んだ文字起こしを、再生成後に再同期する。
  $effect(() => {
    if (!transcribing || loading) return;
    let cancelled = false;
    let polling = false;
    const poll = async () => {
      if (polling) return;
      polling = true;
      try {
        const session = await invoke<TranscriptionSession>("get_transcription_session");
        if (cancelled || session.transcribing) return;
        selectedAudio = session.selectedAudio;
        transcribing = false;
        if (selectedAudio) {
          await restoreSelectedTranscript();
          transcriptRevision += 1;
        }
        await refreshUsage();
      } catch (error) {
        if (!cancelled) showError(errorText(error));
      } finally {
        polling = false;
      }
    };
    const timer = window.setInterval(() => void poll(), 1_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  });

  async function saveApiKey(apiKey: string) {
    saving = true;

    try {
      const modelsAccessible = await invoke<boolean>("save_api_key", { apiKey });
      hasApiKey = true;
      if (modelsAccessible) {
        showSuccessToast("APIキーを確認し、安全に保存しました。");
      } else {
        showWarningToast("制限付きAPIキーとして保存しました。", "各権限は利用時に確認します。");
      }
      await refreshUsage();
    } catch (error) {
      showError(errorText(error));
    } finally {
      saving = false;
    }
  }

  async function deleteApiKey() {
    deleting = true;
    try {
      await invoke("delete_api_key");
      hasApiKey = false;
      transcriptionUsage = null;
      usageError = "";
      showSuccessToast("APIキーを削除しました。");
    } catch (error) {
      showError(errorText(error));
    } finally {
      deleting = false;
    }
  }

  async function selectAudioFile() {
    selecting = true;
    try {
      const selected = await invoke<SelectedAudioFile | null>("select_audio_file");
      if (selected) {
        selectedAudio = selected;
        await restoreSelectedTranscript();
      }
    } catch (error) {
      showError(errorText(error));
    } finally {
      selecting = false;
    }
  }

  async function transcribeAudio() {
    if (!canTranscribe) return;

    transcribing = true;
    transcript = null;
    try {
      const result = await invoke<TranscriptionResult>("transcribe_selected_audio", {
        request: { provider: transcriptionProvider }
      });
      transcript = result.transcript;
      if (transcript.segments.length > 0) {
        showSuccessToast("文字起こしが完了しました。");
      } else {
        showWarningToast("文字起こしは完了しました。", "発話を検出できませんでした。");
      }
      if (result.persistenceWarning) {
        showWarningToast("文字起こしを保存できませんでした。", result.persistenceWarning);
      } else {
        transcriptRevision += 1;
      }
      await refreshUsage();
    } catch (error) {
      showError(errorText(error));
    } finally {
      transcribing = false;
    }
  }

  async function handleRecordedAudio(audio: SelectedAudioFile) {
    selectedAudio = audio;
    await restoreSelectedTranscript();
  }

  async function restoreSelectedTranscript(
    provider: TranscriptionProviderId = transcriptionProvider
  ) {
    try {
      const restored = await invoke<Transcript | null>("get_selected_transcript", {
        request: { provider }
      });
      if (provider === transcriptionProvider) transcript = restored;
    } catch (error) {
      if (provider === transcriptionProvider) {
        transcript = null;
        showError(errorText(error));
      }
    }
  }

  async function changeTranscriptionProvider(provider: TranscriptionProviderId) {
    transcriptionProvider = provider;
    transcript = null;
    if (selectedAudio) await restoreSelectedTranscript(provider);
  }

  function showMessage(nextMessage: string) {
    if (nextMessage) showSuccessToast(nextMessage);
  }

  function showError(nextError: string) {
    if (!nextError) return;

    const now = Date.now();
    if (nextError === lastErrorToast && now - lastErrorToastAt < 3_000) return;

    lastErrorToast = nextError;
    lastErrorToastAt = now;
    showErrorToast("処理に失敗しました", nextError);
  }
</script>

<svelte:head>
  <title>Mutsuna Echo</title>
</svelte:head>

<ThemeProvider theme={echoTheme}>
  <Toaster position="top-right" closeButton />
  <main class="shell">
  <header class="hero">
    <p class="eyebrow">Mutsuna Echo</p>
    <h1>会話を、読み返せる形へ。</h1>
    <p class="lead">音声ファイルを選択して、話者とタイムスタンプ付きで文字起こしします。</p>
  </header>

  <AudioInputPanel
    {selectedAudio}
    provider={transcriptionProvider}
    {selecting}
    {transcribing}
    {recordingBusy}
    {transcriptRevision}
    {busy}
    {recordingDisabled}
    {providerConfigured}
    {canTranscribe}
    onSelect={selectAudioFile}
    onTranscribe={transcribeAudio}
    onProviderChange={changeTranscriptionProvider}
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
