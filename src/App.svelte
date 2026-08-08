<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { tick, type Component } from "svelte";
  import {
    Toaster,
    showErrorToast,
    showSuccessToast,
    showWarningToast
  } from "@mutsuna/ui/sonner";
  import { ThemeProvider, createTheme } from "@mutsuna/ui/theme";
  import ApiKeySettings from "./lib/components/ApiKeySettings.svelte";
  import AppSidebar from "./lib/components/AppSidebar.svelte";
  import AudioInputPanel from "./lib/components/AudioInputPanel.svelte";
  import LocalModelManager from "./lib/components/LocalModelManager.svelte";
  import MeetingLibrary from "./lib/components/MeetingLibrary.svelte";
  import MeetingWorkspace from "./lib/components/MeetingWorkspace.svelte";
  import PendingActionNotice from "./lib/components/PendingActionNotice.svelte";
  import UsagePanel from "./lib/components/UsagePanel.svelte";
  import {
    getTranscriptionProvider,
    type TranscriptionProviderDefinition,
    type TranscriptionProviderId
  } from "./lib/providers";
  import type { PendingAction } from "./lib/types/pending-action";
  import type { RecordedAudioSummary } from "./lib/types/recording";
  import type {
    SelectedAudioFile,
    Transcript,
    TranscriptionProgress,
    TranscriptionResult,
    TranscriptionSession,
    TranscriptionUsage
  } from "./lib/types/transcript";

  const echoTheme = createTheme("custom", "oklch(0.49 0.12 154)");
  type AppSection = "meetings" | "new" | "settings";

  let loading = $state(true);
  let section = $state<AppSection>("meetings");
  let libraryOpen = $state(true);
  let recordings = $state.raw<RecordedAudioSummary[]>([]);
  let recordingsLoading = $state(false);
  let meetingBusy = $state(false);
  let saving = $state(false);
  let deleting = $state(false);
  let selecting = $state(false);
  let transcribing = $state(false);
  let transcriptionProgress = $state<TranscriptionProgress | null>(null);
  let usageLoading = $state(false);
  let recordingBusy = $state(false);
  let selectedAudio = $state<SelectedAudioFile | null>(null);
  let transcriptionProvider = $state<TranscriptionProviderId>("elevenlabs");
  let transcriptionProviders = $state.raw<TranscriptionProviderDefinition[]>([]);
  // Transcriptは読み取り専用の大きな値なので、深いProxy化を避ける。
  let transcript = $state.raw<Transcript | null>(null);
  let transcriptionUsage = $state<TranscriptionUsage | null>(null);
  let usageError = $state("");
  let lastErrorToast = $state("");
  let lastErrorToastAt = $state(0);
  let pendingActionPromise: Promise<void> | null = null;
  let pendingActionId = "";
  let lastAcknowledgedActionId = "";
  let pendingActionProblem = $state<{ action: PendingAction | null; message: string } | null>(null);
  let pendingActionBusy = $state(false);
  let OverlayPreviewControls = $state<Component | null>(null);

  const busy = $derived(loading || saving || deleting || selecting || transcribing || recordingBusy);
  const recordingDisabled = $derived(loading || saving || deleting || selecting || transcribing);
  const currentProvider = $derived(
    getTranscriptionProvider(transcriptionProviders, transcriptionProvider)
  );
  const hasApiKey = $derived(
    transcriptionProviders.find((provider) => provider.id === "elevenlabs")?.configured ?? false
  );
  const providerConfigured = $derived(currentProvider?.ready ?? false);
  const canTranscribe = $derived(providerConfigured && selectedAudio !== null && !busy);
  const selectedRecording = $derived(
    recordings.find((recording) => recording.meetingId === selectedAudio?.meetingId) ?? null
  );

  $effect(() => {
    if (!import.meta.env.DEV) return;
    let cancelled = false;
    void import("./lib/components/OverlayPreviewControls.svelte").then(({ default: component }) => {
      if (!cancelled) OverlayPreviewControls = component;
    });
    return () => {
      cancelled = true;
    };
  });

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

  async function refreshProviders() {
    transcriptionProviders = await invoke<TranscriptionProviderDefinition[]>(
      "get_transcription_providers"
    );
  }

  async function refreshRecordings() {
    if (recordingsLoading) return;
    recordingsLoading = true;
    try {
      recordings = await invoke<RecordedAudioSummary[]>("list_recorded_audio");
    } catch (error) {
      showError(errorText(error));
    } finally {
      recordingsLoading = false;
    }
  }

  async function selectRecording(recording: RecordedAudioSummary) {
    if (meetingBusy) return;
    meetingBusy = true;
    try {
      selectedAudio = await invoke<SelectedAudioFile>("select_recorded_audio", {
        recordingId: recording.id,
        meetingId: recording.meetingId
      });
      await restoreSelectedTranscript();
      section = "meetings";
    } catch (error) {
      showError(errorText(error));
      await refreshRecordings();
    } finally {
      meetingBusy = false;
    }
  }

  async function revealRecording(recording: RecordedAudioSummary) {
    try {
      await invoke("reveal_recorded_audio", { recordingId: recording.id });
    } catch (error) {
      showError(errorText(error));
      await refreshRecordings();
    }
  }

  function navigate(nextSection: AppSection) {
    section = nextSection;
    if (nextSection === "meetings") libraryOpen = true;
  }

  function receivePendingAction(action: PendingAction): Promise<void> {
    if (action.id === lastAcknowledgedActionId) return Promise.resolve();
    if (pendingActionPromise && pendingActionId === action.id) return pendingActionPromise;
    if (pendingActionPromise) {
      return pendingActionPromise
        .catch(() => undefined)
        .then(() => receivePendingAction(action));
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
    section = "new";
    await focusTranscriptionAction();
    await invoke("acknowledge_pending_action", { actionId: action.id });
    lastAcknowledgedActionId = action.id;
    pendingActionProblem = null;
    showSuccessToast(
      "録音を保存しました。",
      "音声を選択しました。設定を確認して文字起こしを開始できます。"
    );
  }

  async function handlePendingAction(action: PendingAction) {
    try {
      await receivePendingAction(action);
    } catch (error) {
      const message = errorText(error);
      pendingActionProblem = { action, message };
      showError(message);
    }
  }

  async function retryPendingAction() {
    if (pendingActionBusy) return;
    pendingActionBusy = true;
    try {
      const action = await invoke<PendingAction | null>("get_pending_action");
      if (!action) {
        pendingActionProblem = null;
        showWarningToast("文字起こし待ちの録音はありません。");
        return;
      }
      await handlePendingAction(action);
    } catch (error) {
      const message = errorText(error);
      pendingActionProblem = { action: null, message };
      showError(message);
    } finally {
      pendingActionBusy = false;
    }
  }

  async function discardPendingAction() {
    if (pendingActionBusy) return;
    pendingActionBusy = true;
    try {
      await invoke("discard_pending_action", {
        actionId: pendingActionProblem?.action?.id ?? null
      });
      pendingActionProblem = null;
      showSuccessToast("録音の引き渡し情報を解除しました。", "録音ファイルは削除されていません。");
    } catch (error) {
      showError(errorText(error));
    } finally {
      pendingActionBusy = false;
    }
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
    let unlistenPending: UnlistenFn | undefined;
    let unlistenProgress: UnlistenFn | undefined;
    void (async () => {
      try {
        [unlistenPending, unlistenProgress] = await Promise.all([
          listen<PendingAction>("pending-action-available", ({ payload }) => {
            if (!cancelled) void handlePendingAction(payload);
          }),
          listen<TranscriptionProgress>("transcription-progress", ({ payload }) => {
            if (!cancelled) transcriptionProgress = payload;
          })
        ]);
        const [nextProviders, session, pendingResult, nextRecordings] = await Promise.all([
          invoke<TranscriptionProviderDefinition[]>("get_transcription_providers"),
          invoke<TranscriptionSession>("get_transcription_session"),
          invoke<PendingAction | null>("get_pending_action")
            .then((action) => ({ action, error: "" }))
            .catch((error) => ({ action: null, error: errorText(error) })),
          invoke<RecordedAudioSummary[]>("list_recorded_audio").catch((error) => {
            showError(errorText(error));
            return [];
          })
        ]);
        if (cancelled) return;
        transcriptionProviders = nextProviders;
        recordings = nextRecordings;
        selectedAudio = session.selectedAudio;
        transcribing = session.transcribing;
        transcriptionProgress = session.progress;
        if (pendingResult.error) {
          pendingActionProblem = { action: null, message: pendingResult.error };
          showError(pendingResult.error);
        } else if (pendingResult.action) {
          await handlePendingAction(pendingResult.action);
        } else if (selectedAudio) {
          await restoreSelectedTranscript();
        }
        if (hasApiKey) await refreshUsage();
        void ensureStandardVad();
      } catch (error) {
        showError(errorText(error));
      } finally {
        loading = false;
      }
    })();
    return () => {
      cancelled = true;
      unlistenPending?.();
      unlistenProgress?.();
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
        if (cancelled) return;
        transcriptionProgress = session.progress;
        if (session.transcribing) return;
        selectedAudio = session.selectedAudio;
        transcribing = false;
        if (selectedAudio) {
          await restoreSelectedTranscript();
          await refreshRecordings();
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
      await refreshProviders();
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
      await refreshProviders();
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
    transcriptionProgress = { stage: "preparing", completedChunks: 0, totalChunks: null };
    transcript = null;
    try {
      const result = await invoke<TranscriptionResult>("transcribe_selected_audio", {
        request: {
          provider: transcriptionProvider,
          modelId: currentProvider?.modelId ?? null
        }
      });
      transcript = result.transcript;
      if (transcript.segments.length > 0) {
        showSuccessToast("文字起こしが完了しました。");
      } else {
        showWarningToast("文字起こしは完了しました。", "発話を検出できませんでした。");
      }
      if (result.persistenceWarning) {
        showWarningToast("文字起こしを保存できませんでした。", result.persistenceWarning);
      }
      await refreshRecordings();
      await refreshUsage();
      section = "meetings";
    } catch (error) {
      showError(errorText(error));
    } finally {
      transcribing = false;
      transcriptionProgress = null;
    }
  }

  async function ensureStandardVad() {
    try {
      const status = await invoke<{ installed: boolean; downloading: boolean; runtimeSupported: boolean }>(
        "get_local_vad_model_status"
      );
      if (!status.installed && !status.downloading && status.runtimeSupported) {
        await invoke("download_local_vad_model");
      }
    } catch (error) {
      console.warn("Could not install the standard VAD model", error);
    }
  }

  async function handleRecordedAudio(audio: SelectedAudioFile) {
    selectedAudio = audio;
    await restoreSelectedTranscript();
    await refreshRecordings();
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
  <main class:with-library={section === "meetings" && libraryOpen} class="app-shell">
    <AppSidebar {section} onNavigate={navigate} />

    {#if section === "meetings" && libraryOpen}
      <MeetingLibrary
        {recordings}
        selectedMeetingId={selectedAudio?.meetingId ?? null}
        loading={recordingsLoading}
        busy={meetingBusy || busy}
        onSelect={selectRecording}
        onRefresh={refreshRecordings}
        onClose={() => libraryOpen = false}
      />
    {/if}

    <div class="app-content">
      {#if pendingActionProblem}
        <PendingActionNotice
          action={pendingActionProblem.action}
          message={pendingActionProblem.message}
          busy={pendingActionBusy}
          onRetry={retryPendingAction}
          onDiscard={discardPendingAction}
        />
      {/if}

      {#if section === "meetings"}
        <MeetingWorkspace
          {selectedAudio}
          recording={selectedRecording}
          {transcript}
          providerLabel={currentProvider ? `${currentProvider.label} · ${currentProvider.modelLabel}` : "プロバイダーを確認中"}
          providerStatus={currentProvider?.statusMessage ?? "文字起こしプロバイダーを確認しています。"}
          {transcribing}
          progress={transcriptionProgress}
          {canTranscribe}
          {libraryOpen}
          onOpenLibrary={() => libraryOpen = true}
          onTranscribe={transcribeAudio}
          onReveal={revealRecording}
          onCreate={() => section = "new"}
          onOpenSettings={() => section = "settings"}
        />
      {:else if section === "new"}
        <section class="page-view new-meeting-view">
          <header class="page-header">
            <p>NEW MEETING</p>
            <h1>新しいMeeting</h1>
            <span>録音を開始するか、既存の音声ファイルを読み込みます。</span>
          </header>
          <AudioInputPanel
            {selectedAudio}
            providers={transcriptionProviders}
            provider={transcriptionProvider}
            {selecting}
            {transcribing}
            {transcriptionProgress}
            {recordingBusy}
            {busy}
            {recordingDisabled}
            {canTranscribe}
            onSelect={selectAudioFile}
            onTranscribe={transcribeAudio}
            onProviderChange={changeTranscriptionProvider}
            onProvidersChanged={refreshProviders}
            onRecordedAudio={handleRecordedAudio}
            onRecordingBusyChange={(value) => recordingBusy = value}
            onMessage={showMessage}
            onError={showError}
          />
        </section>
      {:else}
        <section class="page-view settings-view">
          <header class="page-header">
            <p>SETTINGS</p>
            <h1>設定</h1>
            <span>文字起こしプロバイダー、ローカルモデル、利用状況を管理します。</span>
          </header>
          <div class="settings-section">
            <div class="settings-section-heading">
              <h2>ローカル文字起こし</h2>
              <p>端末内で使用するモデルを管理します。</p>
            </div>
            <LocalModelManager disabled={busy} onChanged={refreshProviders} onMessage={showMessage} onError={showError} />
          </div>
          {#if hasApiKey}
            <UsagePanel usage={transcriptionUsage} loading={usageLoading} error={usageError} onRefresh={refreshUsage} />
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
        </section>
      {/if}
    </div>
  </main>

  {#if OverlayPreviewControls}
    <div class="dev-preview-dock"><OverlayPreviewControls /></div>
  {/if}
</ThemeProvider>
