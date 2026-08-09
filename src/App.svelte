<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { check as checkForAppUpdate } from "@tauri-apps/plugin-updater";
  import { tick, type Component } from "svelte";
  import {
    Toaster,
    showErrorToast,
    showSuccessToast,
    showWarningToast
  } from "@mutsuna/ui/sonner";
  import { ThemeProvider, createTheme } from "@mutsuna/ui/theme";
  import ApiKeySettings from "./lib/components/ApiKeySettings.svelte";
  import AppUpdateManager from "./lib/components/AppUpdateManager.svelte";
  import AppSidebar from "./lib/components/AppSidebar.svelte";
  import AudioInputPanel from "./lib/components/AudioInputPanel.svelte";
  import LocalModelManager from "./lib/components/LocalModelManager.svelte";
  import MeetingLibrary from "./lib/components/MeetingLibrary.svelte";
  import MeetingWorkspace from "./lib/components/MeetingWorkspace.svelte";
  import PendingActionNotice from "./lib/components/PendingActionNotice.svelte";
  import SummaryAgentManager from "./lib/components/SummaryAgentManager.svelte";
  import UsagePanel from "./lib/components/UsagePanel.svelte";
  import {
    getTranscriptionProvider,
    isTranscriptionProviderId,
    type TranscriptionProviderDefinition,
    type TranscriptionProviderId
  } from "./lib/providers";
  import type { PendingAction } from "./lib/types/pending-action";
  import type { RecentMeetingSummary } from "./lib/types/recording";
  import type { SummaryProviderDefinition, SummaryStatus } from "./lib/types/summary";
  import type {
    SelectedAudioFile,
    EditableTranscript,
    TranscriptionHistory,
    TranscriptionProgress,
    TranscriptionResult,
    TranscriptionRunDetail,
    TranscriptionRunSummary,
    TranscriptSegmentTextChange,
    TranscriptSaveState,
    TranscriptionSession,
    TranscriptionUsage
  } from "./lib/types/transcript";

  const echoTheme = createTheme("custom", "oklch(0.49 0.12 154)");
  const TRANSCRIPTION_PROVIDER_STORAGE_KEY = "mutsuna-echo.transcription-provider";
  const SUMMARY_PROVIDER_STORAGE_KEY = "mutsuna-echo.summary-provider";
  const SUMMARY_MODEL_STORAGE_KEY = "mutsuna-echo.summary-model";
  const SUMMARY_CUSTOM_MODEL_STORAGE_KEY = "mutsuna-echo.summary-custom-model";
  type AppSection = "meetings" | "new" | "settings";
  type TranscriptReplacementUndo = {
    transcriptionId: string;
    changes: TranscriptSegmentTextChange[];
  };

  function savedTranscriptionProvider(): TranscriptionProviderId {
    try {
      const saved = localStorage.getItem(TRANSCRIPTION_PROVIDER_STORAGE_KEY);
      return saved && isTranscriptionProviderId(saved) ? saved : "elevenlabs";
    } catch {
      return "elevenlabs";
    }
  }

  let loading = $state(true);
  let section = $state<AppSection>("meetings");
  let libraryOpen = $state(true);
  let meetings = $state.raw<RecentMeetingSummary[]>([]);
  let meetingsLoading = $state(false);
  let meetingBusy = $state(false);
  let saving = $state(false);
  let deleting = $state(false);
  let selecting = $state(false);
  let transcribing = $state(false);
  let transcriptionProgress = $state<TranscriptionProgress | null>(null);
  let usageLoading = $state(false);
  let recordingBusy = $state(false);
  let updating = $state(false);
  let selectedAudio = $state<SelectedAudioFile | null>(null);
  let transcriptionProvider = $state<TranscriptionProviderId>(savedTranscriptionProvider());
  let transcriptionProviders = $state.raw<TranscriptionProviderDefinition[]>([]);
  let summaryProviders = $state.raw<SummaryProviderDefinition[]>([]);
  let summaryProviderId = $state(localStorage.getItem(SUMMARY_PROVIDER_STORAGE_KEY) ?? "codex");
  let summaryModelId = $state(localStorage.getItem(SUMMARY_MODEL_STORAGE_KEY) ?? "default");
  let summaryCustomModelId = $state(localStorage.getItem(SUMMARY_CUSTOM_MODEL_STORAGE_KEY) ?? "");
  let summaryStatus = $state.raw<SummaryStatus | null>(null);
  let summaryGenerating = $state(false);
  // Transcriptは大きな値なので、編集時も必要なSegmentだけを置換する。
  let transcript = $state.raw<EditableTranscript | null>(null);
  let transcriptionRuns = $state.raw<TranscriptionRunSummary[]>([]);
  let selectedTranscriptionId = $state<string | null>(null);
  let selectedTranscriptionRun = $state.raw<TranscriptionRunDetail | null>(null);
  let transcriptSaveState = $state<TranscriptSaveState>("saved");
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
  let transcriptSaveTimer: number | null = null;
  let transcriptSavePromise: Promise<void> | null = null;
  let transcriptReplacementUndo = $state.raw<TranscriptReplacementUndo | null>(null);
  const pendingTranscriptChanges = new Map<string, string>();
  const pendingSpeakerLabelChanges = new Map<string, string>();

  const busy = $derived(loading || saving || deleting || selecting || transcribing || recordingBusy || updating);
  const recordingDisabled = $derived(loading || saving || deleting || selecting || transcribing || updating);
  const currentProvider = $derived(
    getTranscriptionProvider(transcriptionProviders, transcriptionProvider)
  );
  const hasApiKey = $derived(
    transcriptionProviders.find((provider) => provider.id === "elevenlabs")?.configured ?? false
  );
  const apiKeyProviders = $derived(
    transcriptionProviders.filter((provider) => provider.setup === "apiKey")
  );
  const providerConfigured = $derived(currentProvider?.ready ?? false);
  const canTranscribe = $derived(providerConfigured && selectedAudio !== null && !busy);
  const selectedMeeting = $derived(
    meetings.find((meeting) => meeting.meetingId === selectedAudio?.meetingId) ?? null
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

  async function refreshSummaryProviders() {
    summaryProviders = await invoke<SummaryProviderDefinition[]>("get_summary_providers");
  }

  async function refreshMeetings() {
    if (meetingsLoading) return;
    meetingsLoading = true;
    try {
      meetings = await invoke<RecentMeetingSummary[]>("list_recent_meetings");
    } catch (error) {
      showError(errorText(error));
    } finally {
      meetingsLoading = false;
    }
  }

  async function selectMeeting(meeting: RecentMeetingSummary) {
    if (meetingBusy) return;
    meetingBusy = true;
    try {
      await flushTranscriptEdits();
      if (transcriptSaveState === "error") return;
      selectedAudio = await invoke<SelectedAudioFile>("select_meeting_audio", {
        meetingId: meeting.meetingId
      });
      await restoreTranscriptionHistory();
      section = "meetings";
    } catch (error) {
      showError(errorText(error));
      await refreshMeetings();
    } finally {
      meetingBusy = false;
    }
  }

  async function revealMeeting(meeting: RecentMeetingSummary) {
    try {
      await invoke("reveal_meeting_audio", { meetingId: meeting.meetingId });
    } catch (error) {
      showError(errorText(error));
      await refreshMeetings();
    }
  }

  function navigate(nextSection: AppSection) {
    if (updating) {
      showWarningToast("更新処理中です。", "完了してアプリが再起動するまでお待ちください。");
      return;
    }
    section = nextSection;
    if (nextSection === "meetings") libraryOpen = true;
  }

  async function checkForAvailableUpdate() {
    if (import.meta.env.DEV || /Android|iPhone|iPad/i.test(navigator.userAgent)) return;
    try {
      const update = await checkForAppUpdate({ timeout: 15_000 });
      if (!update) return;
      const version = update.version;
      await update.close();
      showSuccessToast(
        `Mutsuna Echo ${version}を利用できます。`,
        "設定の「アプリの更新」からインストールできます。"
      );
    } catch {
      // 起動時の自動確認は通信不能でも作業を妨げない。手動確認では詳細を表示する。
    }
  }

  async function prepareForUpdate(): Promise<boolean> {
    await flushTranscriptEdits();
    if (transcriptSaveState !== "error") return true;
    showWarningToast("文字起こしを保存できていません。", "編集内容を保存してから更新してください。");
    return false;
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
    await flushTranscriptEdits();
    if (transcriptSaveState === "error") throw new Error("文字起こしの編集を保存してから再試行してください。");
    const audio = await invoke<SelectedAudioFile>("receive_pending_action", {
      actionId: action.id
    });
    selectedAudio = audio;
    await restoreTranscriptionHistory();
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
        const [nextProviders, nextSummaryProviders, session, pendingResult, nextMeetings] = await Promise.all([
          invoke<TranscriptionProviderDefinition[]>("get_transcription_providers"),
          invoke<SummaryProviderDefinition[]>("get_summary_providers"),
          invoke<TranscriptionSession>("get_transcription_session"),
          invoke<PendingAction | null>("get_pending_action")
            .then((action) => ({ action, error: "" }))
            .catch((error) => ({ action: null, error: errorText(error) })),
          invoke<RecentMeetingSummary[]>("list_recent_meetings").catch((error) => {
            showError(errorText(error));
            return [];
          })
        ]);
        if (cancelled) return;
        transcriptionProviders = nextProviders;
        summaryProviders = nextSummaryProviders;
        meetings = nextMeetings;
        selectedAudio = session.selectedAudio;
        transcribing = session.transcribing;
        transcriptionProgress = session.progress;
        if (pendingResult.error) {
          pendingActionProblem = { action: null, message: pendingResult.error };
          showError(pendingResult.error);
        } else if (pendingResult.action) {
          await handlePendingAction(pendingResult.action);
        } else if (selectedAudio) {
          await restoreTranscriptionHistory();
        }
        if (hasApiKey) await refreshUsage();
        void ensureStandardVad();
        void checkForAvailableUpdate();
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
          await restoreTranscriptionHistory();
          await refreshMeetings();
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

  $effect(() => {
    const flushBeforeLeaving = () => void flushTranscriptEdits();
    const flushWhenHidden = () => {
      if (document.visibilityState === "hidden") void flushTranscriptEdits();
    };
    window.addEventListener("beforeunload", flushBeforeLeaving);
    document.addEventListener("visibilitychange", flushWhenHidden);
    return () => {
      window.removeEventListener("beforeunload", flushBeforeLeaving);
      document.removeEventListener("visibilitychange", flushWhenHidden);
    };
  });

  async function saveApiKey(providerId: TranscriptionProviderId, apiKey: string) {
    saving = true;

    try {
      const modelsAccessible = await invoke<boolean>("save_provider_api_key", { providerId, apiKey });
      await refreshProviders();
      if (modelsAccessible) {
        showSuccessToast("APIキーを確認し、安全に保存しました。");
      } else {
        showWarningToast("制限付きAPIキーとして保存しました。", "各権限は利用時に確認します。");
      }
      if (providerId === "elevenlabs") await refreshUsage();
    } catch (error) {
      showError(errorText(error));
    } finally {
      saving = false;
    }
  }

  async function deleteApiKey(providerId: TranscriptionProviderId) {
    deleting = true;
    try {
      await invoke("delete_provider_api_key", { providerId });
      await refreshProviders();
      if (providerId === "elevenlabs") {
        transcriptionUsage = null;
        usageError = "";
      }
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
      await flushTranscriptEdits();
      if (transcriptSaveState === "error") return;
      const selected = await invoke<SelectedAudioFile | null>("select_audio_file");
      if (selected) {
        selectedAudio = selected;
        await restoreTranscriptionHistory();
      }
    } catch (error) {
      showError(errorText(error));
    } finally {
      selecting = false;
    }
  }

  async function transcribeAudio() {
    if (!canTranscribe) return;

    await flushTranscriptEdits();
    if (transcriptSaveState === "error") return;
    transcribing = true;
    transcriptionProgress = { stage: "preparing", completedChunks: 0, totalChunks: null };
    try {
      const result = await invoke<TranscriptionResult>("transcribe_selected_audio", {
        request: {
          provider: transcriptionProvider,
          modelId: currentProvider?.modelId ?? null
        }
      });
      if (result.run) {
        setSelectedTranscriptionRun(result.run);
        await refreshTranscriptionHistoryList();
      } else {
        selectedTranscriptionRun = null;
        selectedTranscriptionId = null;
        transcript = {
          ...result.transcript,
          speakerLabels: [...new Set(result.transcript.segments.map((segment) => segment.speaker))]
            .sort()
            .map((speaker) => ({ speaker, label: speaker, edited: false })),
          segments: result.transcript.segments.map((segment) => ({
            ...segment,
            segmentId: "",
            edited: false
          }))
        };
      }
      if (result.transcript.segments.length > 0) {
        showSuccessToast("文字起こしが完了しました。");
      } else {
        showWarningToast("文字起こしは完了しました。", "発話を検出できませんでした。");
      }
      if (result.persistenceWarning) {
        showWarningToast("文字起こしを保存できませんでした。", result.persistenceWarning);
      }
      await refreshMeetings();
      if (transcriptionProvider === "elevenlabs") await refreshUsage();
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
    await flushTranscriptEdits();
    if (transcriptSaveState === "error") return;
    selectedAudio = audio;
    await restoreTranscriptionHistory();
    await refreshMeetings();
  }

  function clearTranscriptEditingState() {
    if (transcriptSaveTimer != null) window.clearTimeout(transcriptSaveTimer);
    transcriptSaveTimer = null;
    pendingTranscriptChanges.clear();
    pendingSpeakerLabelChanges.clear();
    transcriptReplacementUndo = null;
    transcriptSaveState = "saved";
  }

  function setSelectedTranscriptionRun(run: TranscriptionRunDetail | null) {
    clearTranscriptEditingState();
    selectedTranscriptionRun = run;
    selectedTranscriptionId = run?.transcriptionId ?? null;
    transcript = run?.transcript ?? null;
    summaryStatus = null;
  }

  async function refreshSummaryStatus() {
    if (!selectedAudio || !selectedTranscriptionRun) {
      summaryStatus = null;
      return;
    }
    try {
      summaryStatus = await invoke<SummaryStatus>("get_selected_summary", {
        meetingId: selectedAudio.meetingId
      });
    } catch (error) {
      summaryStatus = null;
      showError(errorText(error));
    }
  }

  function changeSummaryProvider(value: string) {
    summaryProviderId = value;
    localStorage.setItem(SUMMARY_PROVIDER_STORAGE_KEY, value);
    const provider = summaryProviders.find((candidate) => candidate.id === value);
    summaryModelId = provider?.models.find((model) => model.isDefault)?.id ?? provider?.models[0]?.id ?? "default";
    localStorage.setItem(SUMMARY_MODEL_STORAGE_KEY, summaryModelId);
  }

  function changeSummaryModel(value: string) {
    summaryModelId = value;
    localStorage.setItem(SUMMARY_MODEL_STORAGE_KEY, value);
  }

  function changeSummaryCustomModel(value: string) {
    summaryCustomModelId = value;
    localStorage.setItem(SUMMARY_CUSTOM_MODEL_STORAGE_KEY, value);
  }

  async function generateSummary() {
    if (!selectedAudio || !selectedTranscriptionRun || summaryGenerating) return;
    await flushTranscriptEdits();
    if (transcriptSaveState === "error") return;
    summaryGenerating = true;
    try {
      summaryStatus = await invoke<SummaryStatus>("generate_selected_summary", {
        request: {
          meetingId: selectedAudio.meetingId,
          providerId: summaryProviderId,
          modelId: summaryModelId === "custom" ? summaryCustomModelId.trim() : summaryModelId
        }
      });
      showSuccessToast("会議ノートを生成しました。");
    } catch (error) {
      showError(errorText(error));
    } finally {
      summaryGenerating = false;
    }
  }

  async function refreshTranscriptionHistoryList() {
    if (!selectedAudio) {
      transcriptionRuns = [];
      selectedTranscriptionId = null;
      return;
    }
    const history = await invoke<TranscriptionHistory>("get_selected_transcription_history");
    transcriptionRuns = history.runs;
    selectedTranscriptionId = history.selectedTranscriptionId;
  }

  async function restoreTranscriptionHistory() {
    try {
      const history = await invoke<TranscriptionHistory>("get_selected_transcription_history");
      transcriptionRuns = history.runs;
      selectedTranscriptionId = history.selectedTranscriptionId;
      const run = history.selectedTranscriptionId
        ? await invoke<TranscriptionRunDetail | null>("get_selected_transcription_run")
        : null;
      setSelectedTranscriptionRun(run);
      await refreshSummaryStatus();
    } catch (error) {
      transcriptionRuns = [];
      setSelectedTranscriptionRun(null);
      showError(errorText(error));
    }
  }

  async function selectTranscriptionRun(transcriptionId: string) {
    if (transcriptionId === selectedTranscriptionId) return;
    await flushTranscriptEdits();
    if (transcriptSaveState === "error") return;
    try {
      const run = await invoke<TranscriptionRunDetail>("select_transcription_run", {
        request: { transcriptionId }
      });
      setSelectedTranscriptionRun(run);
      await refreshSummaryStatus();
    } catch (error) {
      showError(errorText(error));
    }
  }

  function editTranscriptSegment(segmentId: string, text: string) {
    if (!transcript || !selectedTranscriptionRun || !segmentId) return;
    transcriptReplacementUndo = null;
    transcript = {
      ...transcript,
      segments: transcript.segments.map((segment) =>
        segment.segmentId === segmentId ? { ...segment, text, edited: true } : segment
      )
    };
    pendingTranscriptChanges.set(segmentId, text);
    transcriptSaveState = "unsaved";
    if (transcriptSaveTimer != null) window.clearTimeout(transcriptSaveTimer);
    transcriptSaveTimer = window.setTimeout(() => void flushTranscriptEdits(), 500);
  }

  function queueTranscriptChanges(changes: readonly TranscriptSegmentTextChange[]) {
    if (!transcript || !selectedTranscriptionRun || changes.length === 0) return;
    const replacements = new Map(changes.map((change) => [change.segmentId, change.text]));
    transcript = {
      ...transcript,
      segments: transcript.segments.map((segment) => {
        const text = replacements.get(segment.segmentId);
        return text == null ? segment : { ...segment, text, edited: true };
      })
    };
    for (const change of changes) pendingTranscriptChanges.set(change.segmentId, change.text);
    transcriptSaveState = "unsaved";
    if (transcriptSaveTimer != null) window.clearTimeout(transcriptSaveTimer);
    transcriptSaveTimer = null;
  }

  function transcriptSaveFailed(): boolean {
    return transcriptSaveState === "error";
  }

  async function replaceTranscriptSegments(changes: TranscriptSegmentTextChange[]): Promise<boolean> {
    await flushTranscriptEdits();
    if (transcriptSaveState === "error" || !transcript || !selectedTranscriptionRun) return false;

    const currentSegments = new Map(transcript.segments.map((segment) => [segment.segmentId, segment.text]));
    const effectiveChanges = changes.filter((change) => {
      const current = currentSegments.get(change.segmentId);
      return current != null && current !== change.text;
    });
    if (effectiveChanges.length === 0) return false;

    transcriptReplacementUndo = {
      transcriptionId: selectedTranscriptionRun.transcriptionId,
      changes: effectiveChanges.map((change) => ({
        segmentId: change.segmentId,
        text: currentSegments.get(change.segmentId)!
      }))
    };
    queueTranscriptChanges(effectiveChanges);
    await flushTranscriptEdits();
    if (transcriptSaveFailed()) {
      transcriptReplacementUndo = null;
      return false;
    }
    showSuccessToast(effectiveChanges.length === 1 ? "文字起こしを置換しました。" : "文字起こしを一括置換しました。");
    return true;
  }

  async function undoTranscriptReplacement(): Promise<void> {
    const undo = transcriptReplacementUndo;
    if (!undo) return;
    await flushTranscriptEdits();
    if (transcriptSaveState === "error" || selectedTranscriptionRun?.transcriptionId !== undo.transcriptionId) return;
    transcriptReplacementUndo = null;
    queueTranscriptChanges(undo.changes);
    await flushTranscriptEdits();
    if (!transcriptSaveFailed()) showSuccessToast("一括置換を元に戻しました。");
  }

  function editSpeakerLabel(speaker: string, label: string) {
    if (!transcript || !selectedTranscriptionRun) return;
    transcript = {
      ...transcript,
      speakerLabels: transcript.speakerLabels.map((entry) =>
        entry.speaker === speaker
          ? { ...entry, label, edited: label.trim() !== "" && label.trim() !== speaker }
          : entry
      )
    };
    pendingSpeakerLabelChanges.set(speaker, label);
    transcriptSaveState = "unsaved";
    if (transcriptSaveTimer != null) window.clearTimeout(transcriptSaveTimer);
    transcriptSaveTimer = window.setTimeout(() => void flushTranscriptEdits(), 500);
  }

  async function flushTranscriptEdits(): Promise<void> {
    if (transcriptSaveTimer != null) window.clearTimeout(transcriptSaveTimer);
    transcriptSaveTimer = null;
    if (transcriptSavePromise) {
      await transcriptSavePromise;
      if ((pendingTranscriptChanges.size > 0 || pendingSpeakerLabelChanges.size > 0)
        && transcriptSaveState !== "error") {
        return flushTranscriptEdits();
      }
      return;
    }
    const run = selectedTranscriptionRun;
    if (!run || (pendingTranscriptChanges.size === 0 && pendingSpeakerLabelChanges.size === 0)) return;
    const snapshot = new Map(pendingTranscriptChanges);
    const speakerSnapshot = new Map(pendingSpeakerLabelChanges);
    pendingTranscriptChanges.clear();
    pendingSpeakerLabelChanges.clear();
    transcriptSaveState = "saving";
    transcriptSavePromise = (async () => {
      try {
        const saved = await invoke<TranscriptionRunDetail>("update_transcript_document", {
          request: {
            transcriptionId: run.transcriptionId,
            expectedRevision: run.revision,
            changes: [...snapshot].map(([segmentId, text]) => ({ segmentId, text })),
            speakerLabels: [...speakerSnapshot].map(([speaker, label]) => ({ speaker, label }))
          }
        });
        if (selectedTranscriptionRun?.transcriptionId === saved.transcriptionId) {
          const newerChanges = new Map(pendingTranscriptChanges);
          const newerSpeakerLabels = new Map(pendingSpeakerLabelChanges);
          selectedTranscriptionRun = saved;
          transcript = {
            ...saved.transcript,
            speakerLabels: saved.transcript.speakerLabels.map((entry) => {
              const newerLabel = newerSpeakerLabels.get(entry.speaker);
              return newerLabel == null
                ? entry
                : { ...entry, label: newerLabel, edited: newerLabel.trim() !== "" && newerLabel.trim() !== entry.speaker };
            }),
            segments: saved.transcript.segments.map((segment) => {
              const newerText = newerChanges.get(segment.segmentId);
              return newerText == null ? segment : { ...segment, text: newerText, edited: true };
            })
          };
          selectedTranscriptionId = saved.transcriptionId;
          transcriptSaveState = newerChanges.size > 0 || newerSpeakerLabels.size > 0 ? "unsaved" : "saved";
          if (summaryStatus?.summary) {
            summaryStatus = { ...summaryStatus, currentRevision: saved.revision, stale: summaryStatus.summary.sourceRevision !== saved.revision };
          }
        }
        await refreshTranscriptionHistoryList();
      } catch (error) {
        for (const [segmentId, text] of snapshot) {
          if (!pendingTranscriptChanges.has(segmentId)) pendingTranscriptChanges.set(segmentId, text);
        }
        for (const [speaker, label] of speakerSnapshot) {
          if (!pendingSpeakerLabelChanges.has(speaker)) pendingSpeakerLabelChanges.set(speaker, label);
        }
        transcriptSaveState = "error";
        showError(errorText(error));
      } finally {
        transcriptSavePromise = null;
        if ((pendingTranscriptChanges.size > 0 || pendingSpeakerLabelChanges.size > 0)
          && transcriptSaveState !== "error") {
          transcriptSaveTimer = window.setTimeout(() => void flushTranscriptEdits(), 500);
        }
      }
    })();
    return transcriptSavePromise;
  }

  async function resetTranscriptDocument() {
    const run = selectedTranscriptionRun;
    if (!run || !run.edited) return;
    await flushTranscriptEdits();
    if (transcriptSaveState === "error" || !selectedTranscriptionRun) return;
    if (!window.confirm("この文字起こしの編集内容を破棄し、モデルの出力へ戻しますか？")) return;
    try {
      const reset = await invoke<TranscriptionRunDetail>("reset_transcript_document", {
        request: {
          transcriptionId: selectedTranscriptionRun.transcriptionId,
          expectedRevision: selectedTranscriptionRun.revision
        }
      });
      setSelectedTranscriptionRun(reset);
      await refreshSummaryStatus();
      await refreshTranscriptionHistoryList();
      showSuccessToast("モデルの出力へ戻しました。");
    } catch (error) {
      showError(errorText(error));
    }
  }

  async function changeTranscriptionProvider(provider: TranscriptionProviderId) {
    transcriptionProvider = provider;
    try {
      localStorage.setItem(TRANSCRIPTION_PROVIDER_STORAGE_KEY, provider);
    } catch {
      // 選択はこのセッションでは維持できるため、ストレージ利用不可は無視する。
    }
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
        {meetings}
        selectedMeetingId={selectedAudio?.meetingId ?? null}
        loading={meetingsLoading}
        busy={meetingBusy || busy}
        onSelect={selectMeeting}
        onRefresh={refreshMeetings}
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
          meeting={selectedMeeting}
          {transcript}
          runs={transcriptionRuns}
          {selectedTranscriptionId}
          selectedRun={selectedTranscriptionRun}
          saveState={transcriptSaveState}
          {summaryStatus}
          {summaryProviders}
          {summaryProviderId}
          summaryModelId={summaryModelId}
          summaryCustomModelId={summaryCustomModelId}
          summaryGenerating={summaryGenerating}
          providers={transcriptionProviders}
          provider={transcriptionProvider}
          providerLabel={currentProvider?.label ?? "プロバイダーを確認中"}
          providerStatus={currentProvider?.statusMessage ?? "文字起こしプロバイダーを確認しています。"}
          {transcribing}
          progress={transcriptionProgress}
          {canTranscribe}
          {libraryOpen}
          onOpenLibrary={() => libraryOpen = true}
          onTranscribe={transcribeAudio}
          onProviderChange={changeTranscriptionProvider}
          onRunChange={selectTranscriptionRun}
          onEditSegment={editTranscriptSegment}
          onEditSpeakerLabel={editSpeakerLabel}
          onReplaceSegments={replaceTranscriptSegments}
          canUndoReplacement={transcriptReplacementUndo?.transcriptionId === selectedTranscriptionId}
          onUndoReplacement={undoTranscriptReplacement}
          onFlushEdits={flushTranscriptEdits}
          onResetTranscript={resetTranscriptDocument}
          onSummaryProviderChange={changeSummaryProvider}
          onSummaryModelChange={changeSummaryModel}
          onSummaryCustomModelChange={changeSummaryCustomModel}
          onGenerateSummary={generateSummary}
          onReveal={revealMeeting}
          onCreate={() => section = "new"}
          onOpenSettings={() => section = "settings"}
          onError={showError}
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
            <span>アプリ更新、文字起こし、要約エージェント、ローカルモデル、利用状況を管理します。</span>
          </header>
          <div class="settings-section">
            <div class="settings-section-heading">
              <h2>Mutsuna Echo</h2>
              <p>新しいバージョンを安全に確認し、アプリ内からインストールします。</p>
            </div>
            <AppUpdateManager
              disabled={busy && !updating}
              onBeforeInstall={prepareForUpdate}
              onBusyChange={(value) => updating = value}
            />
          </div>
          <div class="settings-section">
            <div class="settings-section-heading">
              <h2>ローカル文字起こし</h2>
              <p>端末内で使用するモデルを管理します。</p>
            </div>
            <LocalModelManager disabled={busy} onChanged={refreshProviders} onMessage={showMessage} onError={showError} />
          </div>
          <div class="settings-section">
            <div class="settings-section-heading">
              <h2>AI会議ノート</h2>
              <p>要約に使用するACPエージェントを任意で追加します。</p>
            </div>
            <SummaryAgentManager disabled={busy} onChanged={refreshSummaryProviders} onMessage={showMessage} onError={showError} />
          </div>
          {#if hasApiKey}
            <UsagePanel usage={transcriptionUsage} loading={usageLoading} error={usageError} onRefresh={refreshUsage} />
          {/if}
          <div class="settings-section">
            <div class="settings-section-heading">
              <h2>クラウド文字起こし</h2>
              <p>サービスごとに認証情報を管理します。</p>
            </div>
            {#each apiKeyProviders as provider (provider.id)}
              <ApiKeySettings
                {provider}
                {loading}
                {saving}
                {deleting}
                hasApiKey={provider.configured}
                {busy}
                onSave={(apiKey) => saveApiKey(provider.id, apiKey)}
                onDelete={() => deleteApiKey(provider.id)}
              />
            {/each}
          </div>
        </section>
      {/if}
    </div>
  </main>

  {#if OverlayPreviewControls}
    <div class="dev-preview-dock"><OverlayPreviewControls /></div>
  {/if}
</ThemeProvider>
