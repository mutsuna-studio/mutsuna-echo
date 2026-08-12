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
  import { AdminShellFrame } from "@mutsuna/ui/admin-shell-frame";
  import { ThemeProvider, createTheme } from "@mutsuna/ui/theme";
  import { scrollbarVisibility } from "@mutsuna/ui/scrollbar";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ChartNoAxesColumn from "@lucide/svelte/icons/chart-no-axes-column";
  import Settings from "@lucide/svelte/icons/settings";
  import ApiKeySettings from "./lib/components/ApiKeySettings.svelte";
  import AppUpdateManager from "./lib/components/AppUpdateManager.svelte";
  import ThirdPartyLicenses from "./lib/components/ThirdPartyLicenses.svelte";
  import { checkAndroidUpdate, isAndroid, waitForAndroidUpdateCheck } from "./lib/androidUpdate";
  import AppSidebar from "./lib/components/AppSidebar.svelte";
  import LocalModelManager from "./lib/components/LocalModelManager.svelte";
  import MeetingHome from "./lib/components/MeetingHome.svelte";
  import MeetingWorkspace from "./lib/components/MeetingWorkspace.svelte";
  import PendingActionNotice from "./lib/components/PendingActionNotice.svelte";
  import PowerSettings from "./lib/components/PowerSettings.svelte";
  import SummaryAgentManager from "./lib/components/SummaryAgentManager.svelte";
  import SummaryDefaultsSettings from "./lib/components/SummaryDefaultsSettings.svelte";
  import SonioxUsagePanel from "./lib/components/SonioxUsagePanel.svelte";
  import CloudflareUsagePanel from "./lib/components/CloudflareUsagePanel.svelte";
  import TranscriptionContextEditor from "./lib/components/TranscriptionContextEditor.svelte";
  import UsagePanel from "./lib/components/UsagePanel.svelte";
  import {
    getTranscriptionProvider,
    isTranscriptionProviderId,
    type TranscriptionProviderDefinition,
    type TranscriptionProviderId,
    type LocalDiarizationModelStatus
  } from "./lib/providers";
  import type { PendingAction } from "./lib/types/pending-action";
  import type { RecentMeetingSummary } from "./lib/types/recording";
  import type { SummaryModelDefinition, SummaryProgress, SummaryProviderDefinition, SummaryStatus } from "./lib/types/summary";
  import type {
    SelectedAudioFile,
    EditableTranscript,
    TranscriptionHistory,
    TranscriptionProgress,
    TranscriptionResult,
    TranscriptionRunDetail,
    TranscriptionRunSummary,
    TranscriptFormattingResult,
    TranscriptSegmentTextChange,
    TranscriptSaveState,
    TranscriptionSession,
    TranscriptionUsage,
    SonioxUsage,
    CloudflareUsage,
    ContextSaveState,
    GlobalTranscriptionContextSettings,
    MeetingTranscriptionContext,
    LocalDiarizationProgress
  } from "./lib/types/transcript";

  const echoTheme = createTheme("custom", "oklch(0.527 0.093 185.044)");
  const TRANSCRIPTION_PROVIDER_STORAGE_KEY = "mutsuna-echo.transcription-provider";
  const SUMMARY_PROVIDER_STORAGE_KEY = "mutsuna-echo.summary-provider";
  const SUMMARY_MODEL_STORAGE_KEY = "mutsuna-echo.summary-model";
  const SUMMARY_PROVIDER_MODEL_DEFAULTS_STORAGE_KEY = "mutsuna-echo.summary-provider-model-defaults";
  const SONIOX_USAGE_STORAGE_KEY = "mutsuna-echo.soniox-usage";
  const API_KEY_SAVE_TIMEOUT_MS = 30_000;
  const summarySettingsPreview = import.meta.env.DEV && new URLSearchParams(window.location.search).get("preview") === "summary-settings";
  const TRANSCRIPTION_SETTINGS_PREVIEW_PROVIDERS: TranscriptionProviderDefinition[] = [
    { id: "local", label: "ローカルSTT", kind: "local", setup: "modelDownload", availability: "ready", ready: true, configured: true, modelId: "reazonspeech-k2", modelLabel: "ReazonSpeech K2 int8-fp32", capabilitySummary: "日本語・話者分離・重要用語", capabilities: { timingGranularity: "word", speakerLabels: true, confidenceScores: false, externalDiarization: false, contextText: false, contextTerms: true }, statusMessage: "利用可能", pricingUsdPerHour: null, pricingVerifiedOn: null },
    { id: "elevenlabs", label: "ElevenLabs", kind: "cloud", setup: "apiKey", availability: "ready", ready: true, configured: true, modelId: "scribe-v2", modelLabel: "Scribe v2 Realtime Long Model Name", capabilitySummary: "多言語・話者分離", capabilities: { timingGranularity: "word", speakerLabels: true, confidenceScores: true, externalDiarization: true, contextText: false, contextTerms: true }, statusMessage: "利用可能", pricingUsdPerHour: null, pricingVerifiedOn: null },
    { id: "soniox", label: "Soniox", kind: "cloud", setup: "apiKey", availability: "apiKeyRequired", ready: false, configured: false, modelId: "stt-rt-v4", modelLabel: "Soniox Speech-to-Text Realtime v4", capabilitySummary: "多言語・話者分離", capabilities: { timingGranularity: "token", speakerLabels: true, confidenceScores: true, externalDiarization: true, contextText: true, contextTerms: true }, statusMessage: "APIキーが必要", pricingUsdPerHour: null, pricingVerifiedOn: null },
    { id: "cloudflare", label: "Cloudflare Workers AI", kind: "cloud", setup: "apiKey", availability: "apiKeyRequired", ready: false, configured: false, modelId: "@cf/openai/whisper-large-v3-turbo", modelLabel: "Whisper Large v3 Turbo", capabilitySummary: "多言語・単語タイムスタンプ", capabilities: { timingGranularity: "word", speakerLabels: false, confidenceScores: false, externalDiarization: true, contextText: true, contextTerms: true }, statusMessage: "APIトークンとAccount IDが必要", pricingUsdPerHour: 0.03, pricingVerifiedOn: "2026-08-11" }
  ];
  const SUMMARY_SETTINGS_PREVIEW_PROVIDERS: SummaryProviderDefinition[] = [
    {
      id: "codex",
      label: "Codex",
      description: "ローカルでログイン済みのCodexをACP経由で使用します。",
      ready: true,
      statusMessage: "ACP接続可能",
      models: [
        { id: "gpt-5.3-codex-spark", label: "GPT-5.3-Codex-Spark-Preview", description: "", isDefault: true },
        { id: "gpt-5.3-codex", label: "GPT-5.3 Codex", description: "", isDefault: false }
      ]
    },
    {
      id: "claude",
      label: "Claude Code",
      description: "ローカルのClaude Agent認証をACP経由で使用します。",
      ready: true,
      statusMessage: "ACP接続可能",
      models: [
        { id: "claude-sonnet", label: "Claude Sonnet 4.6 (1M context)", description: "", isDefault: true },
        { id: "claude-opus", label: "Claude Opus 4.6", description: "", isDefault: false }
      ]
    }
  ];
  type AppSection = "meetings" | "settings";
  type SettingsPane = "general" | "transcription" | "summary" | "usage";
  type TranscriptReplacementUndo = {
    transcriptionId: string;
    changes: TranscriptSegmentTextChange[];
  };
  type ContextDraft = { background: string; termsText: string; correctionsText: string };
  type MeetingContextDraft = ContextDraft & { useGlobal: boolean };

  function termsFromText(value: string): string[] {
    return [...new Set(value.split(/\r?\n/).map((term) => term.trim()).filter(Boolean))];
  }

  function correctionsFromText(value: string): { from: string; to: string }[] {
    const seen = new Set<string>();
    return value.split(/\r?\n/).flatMap((line) => {
      const parts = line.split(/\s*(?:=>|⇒)\s*/, 2);
      const from = parts[0]?.trim() ?? "";
      const to = parts[1]?.trim() ?? "";
      if (!from || !to || from === to || seen.has(from)) return [];
      seen.add(from);
      return [{ from, to }];
    });
  }

  function correctionsToText(corrections: { from: string; to: string }[]): string {
    return corrections.map(({ from, to }) => `${from} => ${to}`).join("\n");
  }

  function savedTranscriptionProvider(): TranscriptionProviderId {
    try {
      const saved = localStorage.getItem(TRANSCRIPTION_PROVIDER_STORAGE_KEY);
      return saved && isTranscriptionProviderId(saved) ? saved : "elevenlabs";
    } catch {
      return "elevenlabs";
    }
  }

  function savedSummaryProviderModelDefaults(): Record<string, string> {
    try {
      const parsed = JSON.parse(localStorage.getItem(SUMMARY_PROVIDER_MODEL_DEFAULTS_STORAGE_KEY) ?? "{}");
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
      return Object.fromEntries(
        Object.entries(parsed).filter((entry): entry is [string, string] => typeof entry[1] === "string")
      );
    } catch {
      return {};
    }
  }

  function savedSonioxUsage(): SonioxUsage | null {
    try {
      const value = JSON.parse(localStorage.getItem(SONIOX_USAGE_STORAGE_KEY) ?? "null");
      const usage = value
        && typeof value.monthlyCostUsd === "string"
        && typeof value.periodStart === "string"
        && typeof value.fetchedAt === "string"
        ? value as SonioxUsage
        : null;
      if (!usage) return null;
      const now = new Date();
      const periodStart = new Date(usage.periodStart);
      return periodStart.getUTCFullYear() === now.getUTCFullYear()
        && periodStart.getUTCMonth() === now.getUTCMonth()
        ? usage
        : null;
    } catch {
      return null;
    }
  }

  const savedSummaryProviderId = localStorage.getItem(SUMMARY_PROVIDER_STORAGE_KEY) ?? "codex";
  const savedSummaryModelId = localStorage.getItem(SUMMARY_MODEL_STORAGE_KEY) ?? "default";

  let loading = $state(!summarySettingsPreview);
  let toasterPosition = $state<"top-right" | "bottom-center">("top-right");
  let lastUpdateNotification = "";
  let section = $state<AppSection>(summarySettingsPreview ? "settings" : "meetings");
  let settingsPane = $state<SettingsPane>(summarySettingsPreview ? "summary" : "general");
  let meetings = $state.raw<RecentMeetingSummary[]>([]);
  let meetingsLoading = $state(false);
  let meetingBusy = $state(false);
  let savingProviderId = $state<TranscriptionProviderId | null>(null);
  let deleting = $state(false);
  let selecting = $state(false);
  let transcribing = $state(false);
  let transcriptionProgress = $state<TranscriptionProgress | null>(null);
  let transcriptionSessionSyncing = false;
  let diarizing = $state(false);
  let processingMeetingId = $state<string | null>(null);
  let diarizationProgress = $state<LocalDiarizationProgress | null>(null);
  let diarizationModelStatus = $state.raw<LocalDiarizationModelStatus | null>(null);
  let usageLoading = $state(false);
  let sonioxUsageLoading = $state(false);
  let cloudflareUsageLoading = $state(false);
  let recordingBusy = $state(false);
  let updating = $state(false);
  let selectedAudio = $state<SelectedAudioFile | null>(null);
  let selectedMeetingId = $state<string | null>(null);
  let transcriptionProvider = $state<TranscriptionProviderId>(savedTranscriptionProvider());
  let transcriptionProviders = $state.raw<TranscriptionProviderDefinition[]>(summarySettingsPreview ? TRANSCRIPTION_SETTINGS_PREVIEW_PROVIDERS : []);
  let globalContextSettings = $state.raw<GlobalTranscriptionContextSettings>({ contextEnabled: false, background: "", terms: [], corrections: [] });
  let globalContextDraft = $state.raw<ContextDraft>({ background: "", termsText: "", correctionsText: "" });
  let globalContextSaveState = $state<ContextSaveState>("saved");
  let globalContextLoading = $state(!summarySettingsPreview);
  let meetingContextDraft = $state.raw<MeetingContextDraft | null>(null);
  let meetingContextSaveState = $state<ContextSaveState>("saved");
  let meetingContextLoading = $state(false);
  let summaryProviders = $state.raw<SummaryProviderDefinition[]>(summarySettingsPreview ? SUMMARY_SETTINGS_PREVIEW_PROVIDERS : []);
  let summaryDefaultProviderId = $state(savedSummaryProviderId);
  let summaryDefaultModelId = $state(savedSummaryModelId);
  let summaryProviderModelDefaults = $state.raw<Record<string, string>>(savedSummaryProviderModelDefaults());
  let summaryProviderId = $state(savedSummaryProviderId);
  let summaryModelId = $state(savedSummaryModelId);
  let summaryModelsLoading = $state(false);
  let summaryStatus = $state.raw<SummaryStatus | null>(null);
  let summaryGenerating = $state(false);
  let summaryProgress = $state.raw<SummaryProgress | null>(null);
  let transcriptFormatting = $state(false);
  // Transcriptは大きな値なので、編集時も必要なSegmentだけを置換する。
  let transcript = $state.raw<EditableTranscript | null>(null);
  let transcriptionRuns = $state.raw<TranscriptionRunSummary[]>([]);
  let selectedTranscriptionId = $state<string | null>(null);
  let selectedTranscriptionRun = $state.raw<TranscriptionRunDetail | null>(null);
  let transcriptSaveState = $state<TranscriptSaveState>("saved");
  let transcriptionUsage = $state<TranscriptionUsage | null>(null);
  let sonioxUsage = $state.raw<SonioxUsage | null>(savedSonioxUsage());
  let cloudflareUsage = $state.raw<CloudflareUsage | null>(null);
  let usageError = $state("");
  let sonioxUsageError = $state("");
  let cloudflareUsageError = $state("");
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
  let globalContextSaveTimer: number | null = null;
  let globalContextSavePromise: Promise<boolean> | null = null;
  let globalContextRevision = 0;
  let meetingContextSaveTimer: number | null = null;
  let meetingContextSavePromise: Promise<boolean> | null = null;
  let meetingContextRevision = 0;
  let meetingContextRequestId = 0;
  let transcriptReplacementUndo = $state.raw<TranscriptReplacementUndo | null>(null);
  let summaryModelRequestId = 0;
  let lastSummaryMeetingId = "";
  let compactViewport = $state(false);
  let mobileMeetingDetail = $state(false);
  let settingsViewElement = $state<HTMLElement | null>(null);
  let settingsTabSwipe = $state<{ x: number; y: number } | null>(null);
  let hasAppHistoryEntry = false;
  const pendingTranscriptChanges = new Map<string, string>();
  const pendingLearnedCorrectionSegments = new Set<string>();
  const pendingSpeakerLabelChanges = new Map<string, string>();

  $effect(() => {
    const compactViewportQuery = window.matchMedia("(max-width: 600px)");
    const updateToasterPosition = () => {
      compactViewport = compactViewportQuery.matches;
      toasterPosition = compactViewport ? "bottom-center" : "top-right";
    };
    updateToasterPosition();
    compactViewportQuery.addEventListener("change", updateToasterPosition);
    return () => compactViewportQuery.removeEventListener("change", updateToasterPosition);
  });

  $effect(() => {
    const meetingId = selectedMeetingId ?? "";
    if (!meetingId || meetingId === lastSummaryMeetingId) return;
    lastSummaryMeetingId = meetingId;
    summaryProviderId = summaryDefaultProviderId;
    summaryModelId = summaryDefaultModelId;
    void refreshSummaryModels(summaryProviderId);
  });

  $effect(() => {
    const handlePopState = () => {
      hasAppHistoryEntry = false;
      if (recordingBusy) {
        section = "meetings";
        mobileMeetingDetail = false;
        pushAppHistoryEntry();
        showWarningToast("録音を続けています。", "録音と会議へ戻りました。");
        return;
      }
      section = "meetings";
      mobileMeetingDetail = false;
    };
    window.addEventListener("popstate", handlePopState);
    return () => window.removeEventListener("popstate", handlePopState);
  });

  // 詳細画面が表示される経路では、Androidの戻る操作を統合画面へ戻す履歴を必ず用意する。
  $effect(() => {
    if (mobileMeetingDetail) pushAppHistoryEntry();
  });

  function startSettingsTabSwipe(event: TouchEvent) {
    if (summarySettingsPreview || !window.matchMedia("(max-width: 780px)").matches) return;
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest("button, input, textarea, select, [role='slider'], [contenteditable='true'], [data-swipe-ignore]")) return;
    const touch = event.touches[0];
    if (touch) settingsTabSwipe = { x: touch.clientX, y: touch.clientY };
  }

  function updateSettingsTabSwipe(event: TouchEvent) {
    if (!settingsTabSwipe) return;
    const touch = event.touches[0];
    if (!touch) return;
    const horizontalDistance = touch.clientX - settingsTabSwipe.x;
    const verticalDistance = touch.clientY - settingsTabSwipe.y;
    if (Math.abs(horizontalDistance) < 56 || Math.abs(horizontalDistance) <= Math.abs(verticalDistance) * 1.25) return;
    event.preventDefault();
    const panes: SettingsPane[] = ["general", "transcription", "summary", "usage"];
    const currentIndex = panes.indexOf(settingsPane);
    const nextIndex = horizontalDistance < 0
      ? Math.min(panes.length - 1, currentIndex + 1)
      : Math.max(0, currentIndex - 1);
    if (nextIndex !== currentIndex) void selectSettingsPane(panes[nextIndex]);
    settingsTabSwipe = null;
  }

  function endSettingsTabSwipe() {
    settingsTabSwipe = null;
  }

  $effect(() => {
    const element = settingsViewElement;
    if (!element) return;
    element.addEventListener("touchstart", startSettingsTabSwipe, { passive: true });
    element.addEventListener("touchmove", updateSettingsTabSwipe, { passive: false });
    element.addEventListener("touchend", endSettingsTabSwipe, { passive: true });
    element.addEventListener("touchcancel", endSettingsTabSwipe, { passive: true });
    return () => {
      element.removeEventListener("touchstart", startSettingsTabSwipe);
      element.removeEventListener("touchmove", updateSettingsTabSwipe);
      element.removeEventListener("touchend", endSettingsTabSwipe);
      element.removeEventListener("touchcancel", endSettingsTabSwipe);
    };
  });

  const saving = $derived(savingProviderId !== null);
  const busy = $derived(loading || saving || deleting || selecting || transcribing || diarizing || transcriptFormatting || recordingBusy || updating);
  const processingMeetingStatus = $derived(
    summaryGenerating
      ? "要約中"
      : transcriptFormatting
        ? "整形中"
        : transcribing
          ? "文字起こし中"
          : diarizing
            ? "話者分離中"
            : null
  );
  const recordingDisabled = $derived(loading || saving || deleting || selecting || transcribing || diarizing || updating);
  const canDiarize = $derived(Boolean(
    selectedAudio
      && selectedTranscriptionRun
      && selectedTranscriptionRun.transcript.tokens.some((token) => token.startMs != null)
      && !transcribing
      && !transcriptFormatting
  ));
  const currentProvider = $derived(
    getTranscriptionProvider(transcriptionProviders, transcriptionProvider)
  );
  const effectiveContextTerms = $derived.by(() => {
    if (!globalContextSettings.contextEnabled) return [] as string[];
    const terms = meetingContextDraft?.useGlobal === false
      ? []
      : termsFromText(globalContextDraft.termsText);
    if (meetingContextDraft) terms.push(...termsFromText(meetingContextDraft.termsText));
    return [...new Set(terms)];
  });
  const contextSurchargeActive = $derived(
    transcriptionProvider === "elevenlabs" && currentProvider?.ready === true && effectiveContextTerms.length > 0
  );
  const hasApiKey = $derived(
    transcriptionProviders.find((provider) => provider.id === "elevenlabs")?.configured ?? false
  );
  const hasSonioxApiKey = $derived(
    transcriptionProviders.find((provider) => provider.id === "soniox")?.configured ?? false
  );
  const hasCloudflareApiKey = $derived(
    transcriptionProviders.find((provider) => provider.id === "cloudflare")?.configured ?? false
  );
  const pageTitle = $derived(
    section === "meetings"
      ? !mobileMeetingDetail
        ? "録音と会議"
        : meetings.find((meeting) => meeting.meetingId === selectedMeetingId)?.title ?? "会議"
      : "設定"
  );
  const apiKeyProviders = $derived(
    transcriptionProviders.filter((provider) => provider.setup === "apiKey")
  );
  const providerConfigured = $derived(currentProvider?.ready ?? false);
  const canTranscribe = $derived(
    providerConfigured
      && selectedAudio !== null
      && !busy
      && !globalContextLoading
      && !meetingContextLoading
      && meetingContextSaveState !== "error"
  );
  const selectedMeeting = $derived(
    meetings.find((meeting) => meeting.meetingId === selectedMeetingId) ?? null
  );

  $effect(() => {
    const availableProvider = transcriptionProviders.find((provider) => provider.ready);
    const selectedProviderIsAvailable = transcriptionProviders.some(
      (provider) => provider.id === transcriptionProvider && provider.ready
    );
    if (availableProvider && !selectedProviderIsAvailable) {
      void changeTranscriptionProvider(availableProvider.id);
    }
  });

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

  async function refreshSonioxUsage() {
    if (!hasSonioxApiKey || sonioxUsageLoading) return;

    sonioxUsageLoading = true;
    sonioxUsageError = "";
    try {
      sonioxUsage = await invoke<SonioxUsage>("get_soniox_usage");
      localStorage.setItem(SONIOX_USAGE_STORAGE_KEY, JSON.stringify(sonioxUsage));
    } catch (error) {
      sonioxUsageError = errorText(error);
    } finally {
      sonioxUsageLoading = false;
    }
  }

  function clearSonioxUsage() {
    sonioxUsage = null;
    sonioxUsageError = "";
    localStorage.removeItem(SONIOX_USAGE_STORAGE_KEY);
  }

  async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
    let timeoutId: number | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timeoutId = window.setTimeout(() => reject(new Error(message)), timeoutMs);
    });
    try {
      return await Promise.race([promise, timeout]);
    } finally {
      if (timeoutId !== undefined) window.clearTimeout(timeoutId);
    }
  }

  async function refreshProviders() {
    transcriptionProviders = await invoke<TranscriptionProviderDefinition[]>(
      "get_transcription_providers"
    );
  }

  async function refreshLocalModels() {
    const [nextProviders, nextDiarizationStatus] = await Promise.all([
      invoke<TranscriptionProviderDefinition[]>("get_transcription_providers"),
      invoke<LocalDiarizationModelStatus>("get_local_diarization_model_status")
    ]);
    transcriptionProviders = nextProviders;
    diarizationModelStatus = nextDiarizationStatus;
  }

  function normalizeSummaryProviderSelections() {
    const fallbackProvider = summaryProviders.find((provider) => provider.ready) ?? summaryProviders[0];
    if (!fallbackProvider) return;
    if (!summaryProviders.some((provider) => provider.id === summaryDefaultProviderId && provider.ready)) {
      summaryDefaultProviderId = fallbackProvider.id;
      summaryDefaultModelId = fallbackSummaryModel(fallbackProvider.models);
      localStorage.setItem(SUMMARY_PROVIDER_STORAGE_KEY, summaryDefaultProviderId);
      localStorage.setItem(SUMMARY_MODEL_STORAGE_KEY, summaryDefaultModelId);
    }
    if (!summaryProviders.some((provider) => provider.id === summaryProviderId && provider.ready)) {
      summaryProviderId = summaryDefaultProviderId;
      summaryModelId = summaryDefaultModelId;
    }
  }

  async function refreshSummaryProviders() {
    summaryProviders = await invoke<SummaryProviderDefinition[]>("get_summary_providers");
    normalizeSummaryProviderSelections();
    void refreshSummaryModels(summaryProviderId);
  }

  function fallbackSummaryModel(models: readonly SummaryModelDefinition[]): string {
    return models.find((model) => model.isDefault)?.id ?? models[0]?.id ?? "default";
  }

  function saveSummaryProviderModelDefaults(defaults: Record<string, string>) {
    summaryProviderModelDefaults = defaults;
    localStorage.setItem(SUMMARY_PROVIDER_MODEL_DEFAULTS_STORAGE_KEY, JSON.stringify(defaults));
  }

  function repairSummaryDefaults(providerId: string, models: readonly SummaryModelDefinition[]) {
    const fallback = fallbackSummaryModel(models);
    const providerDefault = summaryProviderModelDefaults[providerId];
    if (!providerDefault || !models.some((model) => model.id === providerDefault)) {
      saveSummaryProviderModelDefaults({ ...summaryProviderModelDefaults, [providerId]: fallback });
    }
    if (providerId === summaryDefaultProviderId && !models.some((model) => model.id === summaryDefaultModelId)) {
      summaryDefaultModelId = fallback;
      localStorage.setItem(SUMMARY_MODEL_STORAGE_KEY, fallback);
    }
  }

  async function refreshSummaryModels(providerId: string, reportError = false) {
    if (summarySettingsPreview) return;
    const provider = summaryProviders.find((candidate) => candidate.id === providerId);
    if (!provider?.ready) {
      if (providerId === summaryProviderId) {
        summaryModelsLoading = false;
        summaryModelId = provider?.models.find((model) => model.isDefault)?.id ?? provider?.models[0]?.id ?? "default";
      }
      return;
    }
    const requestId = ++summaryModelRequestId;
    if (providerId === summaryProviderId) summaryModelsLoading = true;
    try {
      const models = await invoke<SummaryModelDefinition[]>("get_summary_models", { providerId });
      if (models.length === 0) return;
      summaryProviders = summaryProviders.map((candidate) =>
        candidate.id === providerId ? { ...candidate, models } : candidate
      );
      repairSummaryDefaults(providerId, models);
      if (providerId === summaryProviderId) {
        const selectedStillAvailable = models.some((model) => model.id === summaryModelId);
        if (!selectedStillAvailable) {
          summaryModelId = summaryProviderModelDefaults[providerId] && models.some((model) => model.id === summaryProviderModelDefaults[providerId])
            ? summaryProviderModelDefaults[providerId]
            : fallbackSummaryModel(models);
        }
      }
    } catch (error) {
      if (reportError) showError(errorText(error));
    } finally {
      if (requestId === summaryModelRequestId) summaryModelsLoading = false;
    }
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
      const contextSaved = await flushMeetingContext();
      if (transcriptSaveState === "error" || !contextSaved) return;
      selectedAudio = await invoke<SelectedAudioFile | null>("select_meeting_audio", {
        meetingId: meeting.meetingId
      });
      selectedMeetingId = meeting.meetingId;
      await restoreTranscriptionHistory();
      pushAppHistoryEntry();
      section = "meetings";
      mobileMeetingDetail = true;
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

  async function renameMeeting(meeting: RecentMeetingSummary, newFileName: string) {
    const requested = newFileName.trim();
    if (!requested || requested === meeting.fileName) return;
    try {
      await invoke("rename_meeting_audio", {
        meetingId: meeting.meetingId,
        newFileName: requested
      });
      if (selectedAudio?.meetingId === meeting.meetingId) {
        selectedAudio = { ...selectedAudio, name: requested };
      }
      await refreshMeetings();
    } catch (error) {
      showError(errorText(error));
    }
  }

  async function deleteMeeting(meeting: RecentMeetingSummary, mode: "audioOnly" | "all") {
    if (meetingBusy) return;
    meetingBusy = true;
    try {
      await flushTranscriptEdits();
      const contextSaved = await flushMeetingContext();
      if (transcriptSaveState === "error" || !contextSaved) return;
      selectedAudio = null;
      await tick();
      await invoke("delete_meeting", { meetingId: meeting.meetingId, mode });
      if (mode === "audioOnly") {
        selectedMeetingId = meeting.meetingId;
        showSuccessToast("音声ファイルを削除しました。文字起こしと会議ノートは残っています。");
      } else {
        selectedMeetingId = null;
        setSelectedTranscriptionRun(null);
        transcriptionRuns = [];
        mobileMeetingDetail = false;
        showSuccessToast("会議と関連ファイルを削除しました。");
      }
      await refreshMeetings();
    } catch (error) {
      showError(errorText(error));
      await refreshMeetings();
      try {
        selectedAudio = await invoke<SelectedAudioFile | null>("select_meeting_audio", {
          meetingId: meeting.meetingId
        });
        selectedMeetingId = meeting.meetingId;
      } catch {
        selectedMeetingId = null;
      }
    } finally {
      meetingBusy = false;
    }
  }

  function navigate(nextSection: AppSection) {
    if (updating) {
      showWarningToast("更新処理中です。", "完了してアプリが再起動するまでお待ちください。");
      return;
    }
    if (nextSection === "meetings") {
      if (hasAppHistoryEntry) window.history.back();
      else mobileMeetingDetail = false;
      return;
    }
    pushAppHistoryEntry();
    section = nextSection;
  }

  async function selectSettingsPane(pane: SettingsPane) {
    settingsPane = pane;
    await tick();
    document.querySelector<HTMLElement>(".settings-detail-scroll")?.scrollTo({ top: 0 });
  }

  function startMobileRecording() {
    section = "meetings";
    mobileMeetingDetail = false;
  }

  async function selectHomeAudioFile() {
    const selected = await selectAudioFile();
    if (!selected) return;
    await refreshMeetings();
    pushAppHistoryEntry();
    mobileMeetingDetail = true;
    section = "meetings";
  }

  function createRecording() {
    startMobileRecording();
  }

  function pushAppHistoryEntry() {
    if (hasAppHistoryEntry) return;
    window.history.pushState({ mutsunaEchoView: true }, "");
    hasAppHistoryEntry = true;
  }

  async function checkForAvailableUpdate() {
    if (import.meta.env.DEV) return;
    if (isAndroid) {
      try {
        const update = await waitForAndroidUpdateCheck(await checkAndroidUpdate());
        if (update.phase === "available") {
          const notificationKey = `available:${update.availableVersionCode ?? "unknown"}`;
          if (lastUpdateNotification === notificationKey) return;
          lastUpdateNotification = notificationKey;
          showSuccessToast(
            "新しいバージョンがあります。",
            "設定の「アプリの更新」から更新できます。"
          );
        } else if (update.phase === "downloaded") {
          const notificationKey = `downloaded:${update.availableVersionCode ?? "unknown"}`;
          if (lastUpdateNotification === notificationKey) return;
          lastUpdateNotification = notificationKey;
          showSuccessToast(
            "更新の準備ができました。",
            "設定の「アプリの更新」から再起動して適用できます。"
          );
        }
      } catch {
        // Play Store外から入れた開発版など、更新対象外の環境では通知しない。
      }
      return;
    }
    if (/iPhone|iPad/i.test(navigator.userAgent)) return;
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
    selectedMeetingId = audio.meetingId;
    await restoreTranscriptionHistory();
    pushAppHistoryEntry();
    mobileMeetingDetail = true;
    section = "meetings";
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

  function setGlobalContextFromSettings(settings: GlobalTranscriptionContextSettings) {
    globalContextSettings = settings;
    globalContextDraft = { background: settings.background, termsText: settings.terms.join("\n"), correctionsText: correctionsToText(settings.corrections) };
    globalContextSaveState = "saved";
  }

  function updateGlobalContext(patch: Partial<ContextDraft> & { contextEnabled?: boolean }) {
    if (patch.contextEnabled !== undefined) {
      globalContextSettings = { ...globalContextSettings, contextEnabled: patch.contextEnabled };
    }
    globalContextDraft = {
      background: patch.background ?? globalContextDraft.background,
      termsText: patch.termsText ?? globalContextDraft.termsText,
      correctionsText: patch.correctionsText ?? globalContextDraft.correctionsText
    };
    globalContextRevision += 1;
    globalContextSaveState = "unsaved";
    if (globalContextSaveTimer != null) window.clearTimeout(globalContextSaveTimer);
    globalContextSaveTimer = window.setTimeout(() => void flushGlobalContext(), 600);
  }

  async function flushGlobalContext(): Promise<boolean> {
    if (globalContextSaveTimer != null) {
      window.clearTimeout(globalContextSaveTimer);
      globalContextSaveTimer = null;
    }
    if (globalContextSavePromise) await globalContextSavePromise;
    if (globalContextSaveState === "saved") return true;
    if (globalContextSaveState === "error") return false;
    const revision = globalContextRevision;
    const request: GlobalTranscriptionContextSettings = {
      contextEnabled: globalContextSettings.contextEnabled,
      background: globalContextDraft.background,
      terms: termsFromText(globalContextDraft.termsText),
      corrections: correctionsFromText(globalContextDraft.correctionsText)
    };
    globalContextSaveState = "saving";
    const saving = invoke<GlobalTranscriptionContextSettings>("set_global_transcription_context", { settings: request })
      .then((saved) => {
        if (revision === globalContextRevision) setGlobalContextFromSettings(saved);
        else globalContextSaveState = "unsaved";
        return true;
      })
      .catch((error) => {
        if (revision === globalContextRevision) globalContextSaveState = "error";
        showError(errorText(error));
        return false;
      });
    globalContextSavePromise = saving;
    const succeeded = await saving;
    globalContextSavePromise = null;
    return revision !== globalContextRevision ? flushGlobalContext() : succeeded;
  }

  async function loadMeetingContext(meetingId: string) {
    const requestId = ++meetingContextRequestId;
    meetingContextLoading = true;
    meetingContextSaveState = "saved";
    try {
      const context = await invoke<MeetingTranscriptionContext>("get_meeting_transcription_context", { meetingId });
      if (requestId !== meetingContextRequestId || meetingId !== selectedMeetingId) return;
      meetingContextDraft = {
        background: context.background,
        termsText: context.terms.join("\n"),
        correctionsText: correctionsToText(context.corrections),
        useGlobal: context.useGlobal
      };
    } catch (error) {
      if (requestId === meetingContextRequestId) {
        meetingContextDraft = null;
        meetingContextSaveState = "error";
        showError(errorText(error));
      }
    } finally {
      if (requestId === meetingContextRequestId) meetingContextLoading = false;
    }
  }

  function updateMeetingContext(patch: Partial<MeetingContextDraft>) {
    if (!meetingContextDraft) return;
    meetingContextDraft = { ...meetingContextDraft, ...patch };
    meetingContextRevision += 1;
    meetingContextSaveState = "unsaved";
    if (meetingContextSaveTimer != null) window.clearTimeout(meetingContextSaveTimer);
    meetingContextSaveTimer = window.setTimeout(() => void flushMeetingContext(), 600);
  }

  async function flushMeetingContext(): Promise<boolean> {
    if (meetingContextSaveTimer != null) {
      window.clearTimeout(meetingContextSaveTimer);
      meetingContextSaveTimer = null;
    }
    if (meetingContextSavePromise) await meetingContextSavePromise;
    if (!selectedMeetingId) return true;
    if (meetingContextSaveState === "error") return false;
    if (!meetingContextDraft || meetingContextSaveState === "saved") return true;
    const meetingId = selectedMeetingId;
    const revision = meetingContextRevision;
    const request: MeetingTranscriptionContext = {
      background: meetingContextDraft.background,
      terms: termsFromText(meetingContextDraft.termsText),
      corrections: correctionsFromText(meetingContextDraft.correctionsText),
      useGlobal: meetingContextDraft.useGlobal
    };
    meetingContextSaveState = "saving";
    const saving = invoke<MeetingTranscriptionContext>("set_meeting_transcription_context", { meetingId, context: request })
      .then((saved) => {
        if (revision === meetingContextRevision && meetingId === selectedMeetingId) {
          meetingContextDraft = {
            background: saved.background,
            termsText: saved.terms.join("\n"),
            correctionsText: correctionsToText(saved.corrections),
            useGlobal: saved.useGlobal
          };
          meetingContextSaveState = "saved";
        } else if (meetingId === selectedMeetingId) {
          meetingContextSaveState = "unsaved";
        }
        return true;
      })
      .catch((error) => {
        if (meetingId === selectedMeetingId && revision === meetingContextRevision) meetingContextSaveState = "error";
        showError(errorText(error));
        return false;
      });
    meetingContextSavePromise = saving;
    const succeeded = await saving;
    meetingContextSavePromise = null;
    return meetingId === selectedMeetingId && revision !== meetingContextRevision
      ? flushMeetingContext()
      : succeeded;
  }

  $effect(() => {
    const meetingId = selectedMeetingId;
    if (summarySettingsPreview) return;
    if (!meetingId) {
      meetingContextRequestId += 1;
      meetingContextDraft = null;
      meetingContextLoading = false;
      return;
    }
    void loadMeetingContext(meetingId);
  });

  $effect(() => {
    if (summarySettingsPreview) return;
    let cancelled = false;
    let unlistenPending: UnlistenFn | undefined;
    let unlistenProgress: UnlistenFn | undefined;
    let unlistenDiarizationProgress: UnlistenFn | undefined;
    let unlistenSummaryProgress: UnlistenFn | undefined;
    void (async () => {
      try {
        [unlistenPending, unlistenProgress, unlistenDiarizationProgress, unlistenSummaryProgress] = await Promise.all([
          listen<PendingAction>("pending-action-available", ({ payload }) => {
            if (!cancelled) void handlePendingAction(payload);
          }),
          listen<TranscriptionProgress>("transcription-progress", ({ payload }) => {
            if (!cancelled) transcriptionProgress = payload;
          }),
          listen<LocalDiarizationProgress>("local-diarization-progress", ({ payload }) => {
            if (!cancelled) diarizationProgress = payload;
          }),
          listen<SummaryProgress>("summary-progress", ({ payload }) => {
            if (!cancelled && payload.meetingId === selectedMeetingId) summaryProgress = payload;
          })
        ]);
        const [nextProviders, nextSummaryProviders, session, pendingResult, nextMeetings, nextGlobalContext] = await Promise.all([
          invoke<TranscriptionProviderDefinition[]>("get_transcription_providers"),
          invoke<SummaryProviderDefinition[]>("get_summary_providers"),
          invoke<TranscriptionSession>("get_transcription_session"),
          invoke<PendingAction | null>("get_pending_action")
            .then((action) => ({ action, error: "" }))
            .catch((error) => ({ action: null, error: errorText(error) })),
          invoke<RecentMeetingSummary[]>("list_recent_meetings").catch((error) => {
            showError(errorText(error));
            return [];
          }),
          invoke<GlobalTranscriptionContextSettings>("get_global_transcription_context").catch((error) => {
            showError(errorText(error));
            return { contextEnabled: false, background: "", terms: [], corrections: [] };
          })
        ]);
        if (cancelled) return;
        transcriptionProviders = nextProviders;
        setGlobalContextFromSettings(nextGlobalContext);
        globalContextLoading = false;
        summaryProviders = nextSummaryProviders;
        normalizeSummaryProviderSelections();
        void refreshSummaryModels(summaryProviderId);
        meetings = nextMeetings;
        selectedAudio = session.selectedAudio;
        selectedMeetingId = session.selectedAudio?.meetingId ?? null;
        transcribing = session.transcribing;
        diarizing = session.diarizing;
        processingMeetingId = session.transcribing || session.diarizing
          ? session.selectedAudio?.meetingId ?? null
          : null;
        transcriptionProgress = session.progress;
        diarizationModelStatus = await invoke<LocalDiarizationModelStatus>("get_local_diarization_model_status");
        if (pendingResult.error) {
          pendingActionProblem = { action: null, message: pendingResult.error };
          showError(pendingResult.error);
        } else if (pendingResult.action) {
          await handlePendingAction(pendingResult.action);
        } else if (selectedAudio) {
          await restoreTranscriptionHistory();
        }
        if (hasApiKey) await refreshUsage();
        if (hasCloudflareApiKey) await refreshCloudflareUsage();
        void checkForAvailableUpdate();
      } catch (error) {
        showError(errorText(error));
      } finally {
        globalContextLoading = false;
        loading = false;
      }
    })();
    return () => {
      cancelled = true;
      unlistenPending?.();
      unlistenProgress?.();
      unlistenDiarizationProgress?.();
      unlistenSummaryProgress?.();
    };
  });

  async function syncTranscriptionSession(reportError = false) {
    if (transcriptionSessionSyncing) return;
    transcriptionSessionSyncing = true;
    try {
      const session = await invoke<TranscriptionSession>("get_transcription_session");
      const wasTranscribing = transcribing;
      transcriptionProgress = session.progress;
      diarizing = session.diarizing;
      if (session.transcribing) {
        transcribing = true;
        processingMeetingId = session.selectedAudio?.meetingId ?? processingMeetingId;
        return;
      }
      selectedAudio = session.selectedAudio;
      selectedMeetingId = session.selectedAudio?.meetingId ?? selectedMeetingId;
      transcribing = false;
      diarizing = false;
      if (wasTranscribing && !summaryGenerating && !transcriptFormatting) processingMeetingId = null;
      transcriptionProgress = null;
      diarizationProgress = null;
      if (wasTranscribing && selectedAudio) {
        await restoreTranscriptionHistory();
        await refreshMeetings();
        await refreshUsage();
        if (hasCloudflareApiKey) await refreshCloudflareUsage();
      }
    } catch (error) {
      if (reportError) showError(errorText(error));
      else console.warn("Could not synchronize transcription state", error);
    } finally {
      transcriptionSessionSyncing = false;
    }
  }

  async function refreshCloudflareUsage() {
    if (!hasCloudflareApiKey || cloudflareUsageLoading) return;
    cloudflareUsageLoading = true;
    cloudflareUsageError = "";
    try {
      cloudflareUsage = await invoke<CloudflareUsage>("get_cloudflare_usage");
    } catch (error) {
      cloudflareUsageError = errorText(error);
    } finally {
      cloudflareUsageLoading = false;
    }
  }

  function clearCloudflareUsage() {
    cloudflareUsage = null;
    cloudflareUsageError = "";
  }

  // WebViewを閉じている間に進んだ文字起こしを、再生成後に再同期する。
  $effect(() => {
    if (!transcribing || loading) return;
    void syncTranscriptionSession();
    const timer = window.setInterval(() => void syncTranscriptionSession(), 1_000);
    return () => window.clearInterval(timer);
  });

  $effect(() => {
    const flushBeforeLeaving = () => {
      void flushTranscriptEdits();
      void flushGlobalContext();
      void flushMeetingContext();
    };
    const flushWhenHidden = () => {
      if (document.visibilityState === "hidden") flushBeforeLeaving();
      else {
        void syncTranscriptionSession(true);
        void checkForAvailableUpdate();
      }
    };
    const syncWhenFocused = () => void syncTranscriptionSession(true);
    window.addEventListener("beforeunload", flushBeforeLeaving);
    window.addEventListener("focus", syncWhenFocused);
    window.addEventListener("pageshow", syncWhenFocused);
    document.addEventListener("visibilitychange", flushWhenHidden);
    return () => {
      window.removeEventListener("beforeunload", flushBeforeLeaving);
      window.removeEventListener("focus", syncWhenFocused);
      window.removeEventListener("pageshow", syncWhenFocused);
      document.removeEventListener("visibilitychange", flushWhenHidden);
    };
  });

  async function saveApiKey(providerId: TranscriptionProviderId, apiKey: string, accountId?: string): Promise<boolean> {
    if (savingProviderId !== null) return false;
    savingProviderId = providerId;
    let saved = false;

    try {
      const modelsAccessible = await withTimeout(
        invoke<boolean>("save_provider_api_key", { providerId, apiKey, accountId }),
        API_KEY_SAVE_TIMEOUT_MS,
        "APIキーの確認に時間がかかっています。通信状態を確認して、もう一度お試しください。"
      );
      await withTimeout(
        refreshProviders(),
        10_000,
        "保存状態を更新できませんでした。設定画面を開き直して確認してください。"
      );
      saved = true;
      if (modelsAccessible) {
        showSuccessToast("APIキーを確認して保存しました。");
      } else {
        showWarningToast("APIキーを保存しました。", "必要な権限があるか、使用するときにもう一度確認します。");
      }
    } catch (error) {
      showError(errorText(error));
    } finally {
      savingProviderId = null;
    }
    if (saved && providerId === "elevenlabs") void refreshUsage();
    if (saved && providerId === "soniox") clearSonioxUsage();
    if (saved && providerId === "cloudflare") {
      void refreshCloudflareUsage();
      await refreshSummaryProviders();
    }
    return saved;
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
      if (providerId === "soniox") clearSonioxUsage();
      if (providerId === "cloudflare") {
        clearCloudflareUsage();
        await refreshSummaryProviders();
      }
      showSuccessToast("APIキーを削除しました。");
    } catch (error) {
      showError(errorText(error));
    } finally {
      deleting = false;
    }
  }

  async function selectAudioFile(): Promise<SelectedAudioFile | null> {
    selecting = true;
    try {
      await flushTranscriptEdits();
      const contextSaved = await flushMeetingContext();
      if (transcriptSaveState === "error" || !contextSaved) return null;
      const selected = await invoke<SelectedAudioFile | null>("select_audio_file");
      if (selected) {
        selectedAudio = selected;
        selectedMeetingId = selected.meetingId;
        await restoreTranscriptionHistory();
      }
      return selected;
    } catch (error) {
      showError(errorText(error));
      return null;
    } finally {
      selecting = false;
    }
  }

  async function transcribeAudio(diarizationSpeakerCount?: number | null) {
    if (!canTranscribe) return;
    const transcriptionMeetingId = selectedAudio?.meetingId ?? null;

    await flushTranscriptEdits();
    const [globalContextSaved, meetingContextSaved] = await Promise.all([
      flushGlobalContext(),
      flushMeetingContext()
    ]);
    if (transcriptSaveState === "error" || !globalContextSaved || !meetingContextSaved) {
      showWarningToast("コンテキストを保存できていません。", "入力内容を確認してから再試行してください。");
      return;
    }
    transcribing = true;
    processingMeetingId = transcriptionMeetingId;
    const diarizationEnabled = diarizationSpeakerCount !== undefined && transcriptionProvider === "local";
    diarizing = diarizationEnabled;
    transcriptionProgress = { stage: "preparing", completedChunks: 0, totalChunks: null };
    diarizationProgress = diarizationEnabled && selectedAudio
      ? { stage: "loadingModel", completedChunks: 0, totalChunks: null, processedMs: 0, totalMs: selectedAudio.durationMs }
      : null;
    try {
      const result = await invoke<TranscriptionResult>("transcribe_selected_audio", {
        request: {
          provider: transcriptionProvider,
          modelId: currentProvider?.modelId ?? null,
          diarization: {
            enabled: diarizationEnabled,
            speakerCount: diarizationSpeakerCount ?? null
          }
        }
      });
      if (selectedMeetingId === transcriptionMeetingId) {
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
      }
      if (result.transcript.segments.length > 0) {
        showSuccessToast("文字起こしが完了しました。");
      } else {
        showWarningToast("文字起こしは完了しました。", "発話を検出できませんでした。");
      }
      if (result.persistenceWarning) {
        showWarningToast("文字起こしを保存できませんでした。", result.persistenceWarning);
      }
      if (result.diarizationWarning) {
        showWarningToast("文字起こしは完了しました。", `話者分離のみ失敗しました: ${result.diarizationWarning}`);
      }
      await refreshMeetings();
      if (transcriptionProvider === "elevenlabs") await refreshUsage();
      if (transcriptionProvider === "cloudflare") await refreshCloudflareUsage();
      section = "meetings";
    } catch (error) {
      showError(errorText(error));
    } finally {
      transcribing = false;
      diarizing = false;
      if (processingMeetingId === transcriptionMeetingId) processingMeetingId = null;
      transcriptionProgress = null;
      diarizationProgress = null;
    }
  }

  async function handleRecordedAudio(audio: SelectedAudioFile) {
    await flushTranscriptEdits();
    const contextSaved = await flushMeetingContext();
    if (transcriptSaveState === "error" || !contextSaved) return;
    selectedAudio = audio;
    selectedMeetingId = audio.meetingId;
    await restoreTranscriptionHistory();
    await refreshMeetings();
    pushAppHistoryEntry();
    mobileMeetingDetail = true;
    section = "meetings";
  }

  function clearTranscriptEditingState() {
    if (transcriptSaveTimer != null) window.clearTimeout(transcriptSaveTimer);
    transcriptSaveTimer = null;
    pendingTranscriptChanges.clear();
    pendingLearnedCorrectionSegments.clear();
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
    if (!selectedMeetingId || !selectedTranscriptionRun) {
      summaryStatus = null;
      return;
    }
    try {
      summaryStatus = await invoke<SummaryStatus>("get_selected_summary", {
        meetingId: selectedMeetingId
      });
    } catch (error) {
      summaryStatus = null;
      showError(errorText(error));
    }
  }

  function changeSummaryProvider(value: string) {
    summaryProviderId = value;
    const provider = summaryProviders.find((candidate) => candidate.id === value);
    const configuredDefault = summaryProviderModelDefaults[value];
    summaryModelId = configuredDefault && provider?.models.some((model) => model.id === configuredDefault)
      ? configuredDefault
      : fallbackSummaryModel(provider?.models ?? []);
    void refreshSummaryModels(value, true);
  }

  function changeSummaryModel(value: string) {
    summaryModelId = value;
  }

  function changeSummaryDefaultProvider(value: string) {
    summaryDefaultProviderId = value;
    localStorage.setItem(SUMMARY_PROVIDER_STORAGE_KEY, value);
    const provider = summaryProviders.find((candidate) => candidate.id === value);
    const providerDefault = summaryProviderModelDefaults[value];
    summaryDefaultModelId = providerDefault && provider?.models.some((model) => model.id === providerDefault)
      ? providerDefault
      : fallbackSummaryModel(provider?.models ?? []);
    localStorage.setItem(SUMMARY_MODEL_STORAGE_KEY, summaryDefaultModelId);
    void refreshSummaryModels(value, true);
  }

  function changeSummaryDefaultModel(value: string) {
    summaryDefaultModelId = value;
    localStorage.setItem(SUMMARY_MODEL_STORAGE_KEY, value);
  }

  function changeSummaryProviderDefaultModel(providerId: string, modelId: string) {
    saveSummaryProviderModelDefaults({ ...summaryProviderModelDefaults, [providerId]: modelId });
  }

  async function generateSummary() {
    if (!selectedMeetingId || !selectedTranscriptionRun || summaryGenerating || transcriptFormatting) return;
    const operationMeetingId = selectedMeetingId;
    await flushTranscriptEdits();
    if (transcriptSaveState === "error") return;
    summaryGenerating = true;
    processingMeetingId = operationMeetingId;
    summaryProgress = null;
    try {
      summaryStatus = await invoke<SummaryStatus>("generate_selected_summary", {
        request: {
          meetingId: selectedMeetingId,
          providerId: summaryProviderId,
          modelId: summaryModelId
        }
      });
      showSuccessToast("会議ノートを生成しました。");
    } catch (error) {
      showError(errorText(error));
    } finally {
      summaryGenerating = false;
      if (processingMeetingId === operationMeetingId) processingMeetingId = null;
      summaryProgress = null;
    }
  }

  async function refreshTranscriptionHistoryList() {
    if (!selectedMeetingId) {
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

  function openDiarizationSettings() {
    section = "settings";
    settingsPane = "transcription";
  }

  async function diarizeTranscript(speakerCount: number | null) {
    const run = selectedTranscriptionRun;
    if (!run || !selectedAudio || diarizing) return;
    const operationMeetingId = selectedAudio.meetingId;
    if (!diarizationModelStatus?.installed) {
      openDiarizationSettings();
      showWarningToast("話者分離モデルを追加してください。", "設定の「モデルとサービス」から端末内モデルを追加できます。");
      return;
    }
    await flushTranscriptEdits();
    if (transcriptSaveState === "error" || !selectedTranscriptionRun) {
      showWarningToast("文字起こしの編集を保存してから再試行してください。");
      return;
    }
    diarizing = true;
    processingMeetingId = operationMeetingId;
    diarizationProgress = { stage: "loadingModel", completedChunks: 0, totalChunks: null, processedMs: 0, totalMs: selectedAudio.durationMs };
    try {
      const saved = await invoke<TranscriptionRunDetail>("diarize_selected_transcription", {
        request: {
          transcriptionId: selectedTranscriptionRun.transcriptionId,
          expectedRevision: selectedTranscriptionRun.revision,
          speakerCount
        }
      });
      setSelectedTranscriptionRun(saved);
      await refreshTranscriptionHistoryList();
      await refreshSummaryStatus();
      showSuccessToast("話者分離が完了しました。", "既存の話者名は解除し、本文の手修正は保持しました。");
    } catch (error) {
      const message = errorText(error);
      if (!message.includes("キャンセルしました")) showError(message);
    } finally {
      diarizing = false;
      if (processingMeetingId === operationMeetingId) processingMeetingId = null;
      diarizationProgress = null;
    }
  }

  async function cancelDiarization() {
    try {
      await invoke("cancel_selected_diarization");
    } catch (error) {
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
    pendingLearnedCorrectionSegments.add(segmentId);
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
    for (const change of changes) {
      pendingTranscriptChanges.set(change.segmentId, change.text);
      pendingLearnedCorrectionSegments.delete(change.segmentId);
    }
    transcriptSaveState = "unsaved";
    if (transcriptSaveTimer != null) window.clearTimeout(transcriptSaveTimer);
    transcriptSaveTimer = null;
  }

  function transcriptSaveFailed(): boolean {
    return transcriptSaveState === "error";
  }

  async function replaceTranscriptSegments(
    changes: TranscriptSegmentTextChange[],
    successMessage?: string
  ): Promise<boolean> {
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
    showSuccessToast(successMessage ?? (effectiveChanges.length === 1 ? "文字起こしを置換しました。" : "文字起こしを一括置換しました。"));
    return true;
  }

  async function formatTranscript(): Promise<void> {
    const run = selectedTranscriptionRun;
    if (!selectedMeetingId || !run || transcriptFormatting || summaryGenerating) return;
    const operationMeetingId = selectedMeetingId;
    await flushTranscriptEdits();
    if (transcriptSaveState === "error" || !selectedTranscriptionRun) return;

    const sourceTranscriptionId = selectedTranscriptionRun.transcriptionId;
    const sourceRevision = selectedTranscriptionRun.revision;
    const provider = summaryProviders.find((candidate) =>
      candidate.id === summaryProviderId && candidate.ready
    );
    const useLlm = !summaryModelsLoading
      && Boolean(provider?.models.some((model) => model.id === summaryModelId));

    transcriptFormatting = true;
    processingMeetingId = operationMeetingId;
    try {
      const result = await invoke<TranscriptFormattingResult>("format_selected_transcript", {
        request: {
          meetingId: selectedMeetingId,
          providerId: useLlm ? summaryProviderId : null,
          modelId: useLlm ? summaryModelId : null
        }
      });
      if (selectedTranscriptionRun?.transcriptionId !== sourceTranscriptionId
        || selectedTranscriptionRun.revision !== sourceRevision
        || result.transcriptionId !== sourceTranscriptionId
        || result.sourceRevision !== sourceRevision) {
        showError("整形中に文字起こしが変更されました。内容を確認して、もう一度整形してください。");
        return;
      }
      if (result.changes.length === 0) {
        showSuccessToast("整形する箇所はありませんでした。");
      } else {
        const method = result.method === "mechanicalAndLlm" ? "機械整形とLLM校正" : "機械整形";
        await replaceTranscriptSegments(
          result.changes,
          `${result.changes.length.toLocaleString("ja-JP")}件の発話を${method}しました。`
        );
      }
      if (result.warning) showWarningToast("LLM校正を省略しました。", result.warning);
    } catch (error) {
      showError(errorText(error));
    } finally {
      transcriptFormatting = false;
      if (processingMeetingId === operationMeetingId) processingMeetingId = null;
    }
  }

  async function undoTranscriptReplacement(): Promise<void> {
    const undo = transcriptReplacementUndo;
    if (!undo) return;
    await flushTranscriptEdits();
    if (transcriptSaveState === "error" || selectedTranscriptionRun?.transcriptionId !== undo.transcriptionId) return;
    transcriptReplacementUndo = null;
    queueTranscriptChanges(undo.changes);
    await flushTranscriptEdits();
    if (!transcriptSaveFailed()) showSuccessToast("一括編集を元に戻しました。");
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
    const learnedCorrectionSnapshot = new Set(
      [...pendingLearnedCorrectionSegments].filter((segmentId) => snapshot.has(segmentId))
    );
    const speakerSnapshot = new Map(pendingSpeakerLabelChanges);
    pendingTranscriptChanges.clear();
    pendingLearnedCorrectionSegments.clear();
    pendingSpeakerLabelChanges.clear();
    transcriptSaveState = "saving";
    transcriptSavePromise = (async () => {
      try {
        const saved = await invoke<TranscriptionRunDetail>("update_transcript_document", {
          request: {
            transcriptionId: run.transcriptionId,
            expectedRevision: run.revision,
            changes: [...snapshot].map(([segmentId, text]) => ({ segmentId, text })),
            speakerLabels: [...speakerSnapshot].map(([speaker, label]) => ({ speaker, label })),
            learnCorrectionSegmentIds: [...learnedCorrectionSnapshot]
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
          if (!pendingTranscriptChanges.has(segmentId)) {
            pendingTranscriptChanges.set(segmentId, text);
            if (learnedCorrectionSnapshot.has(segmentId)) {
              pendingLearnedCorrectionSegments.add(segmentId);
            }
          }
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
  <Toaster position={toasterPosition} closeButton />
  <AdminShellFrame
    {pageTitle}
    contentGutter="auto"
    contentPadding="none"
    contentClass={section === "settings"
      ? "app-shell-frame-content settings-shell-content overflow-hidden border-0 rounded-none"
      : "app-shell-frame-content overflow-hidden"}
    headerClass={section === "settings" ? "settings-shell-header app-main-header" : "app-main-header"}
  >
    {#snippet headerActions()}
      {#if section === "meetings"}
        <button class="mobile-header-settings" type="button" onclick={() => navigate("settings")} aria-label="設定を開く" title="設定">
          <Settings aria-hidden="true" />
        </button>
      {/if}
    {/snippet}

    {#snippet sidebar()}
      <AppSidebar
        {section}
        {settingsPane}
        settingsPreview={summarySettingsPreview}
        {recordingBusy}
        onNavigate={navigate}
        onSelectSettingsPane={selectSettingsPane}
      />
    {/snippet}

    <div class="app-shell-content">
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
        <div class:mobile-detail-open={mobileMeetingDetail} class="mobile-home-container">
          <MeetingHome
            {meetings}
            loading={meetingsLoading}
            busy={meetingBusy || busy}
            {recordingDisabled}
            {recordingBusy}
            allowMeetingNavigation={transcribing || diarizing || transcriptFormatting}
            {processingMeetingId}
            processingStatus={processingMeetingStatus}
            {selecting}
            onSelectMeeting={selectMeeting}
            onSelectFile={selectHomeAudioFile}
            onAudioReady={handleRecordedAudio}
            onRecordingBusyChange={(value) => recordingBusy = value}
            onMessage={showMessage}
            onError={showError}
          />
        </div>
        <div class:mobile-detail-open={mobileMeetingDetail} class="meeting-workspace-container">
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
          {summaryModelsLoading}
          summaryGenerating={summaryGenerating}
          {summaryProgress}
          {transcriptFormatting}
          providers={transcriptionProviders}
          provider={transcriptionProvider}
          {transcribing}
          progress={transcriptionProgress}
          {canTranscribe}
          {diarizing}
          {diarizationProgress}
          {canDiarize}
          diarizationModelReady={Boolean(diarizationModelStatus?.installed)}
          onDiarize={diarizeTranscript}
          onCancelDiarization={cancelDiarization}
          onOpenDiarizationSettings={openDiarizationSettings}
          contextEnabled={globalContextSettings.contextEnabled}
          contextSurchargeActive={contextSurchargeActive}
          contextTermCount={effectiveContextTerms.length}
          contextDraft={meetingContextDraft}
          contextSaveState={meetingContextSaveState}
          contextLoading={meetingContextLoading}
          onTranscribe={transcribeAudio}
          onContextBackgroundChange={(background) => updateMeetingContext({ background })}
          onContextTermsChange={(termsText) => updateMeetingContext({ termsText })}
          onContextCorrectionsChange={(correctionsText) => updateMeetingContext({ correctionsText })}
          onContextUseGlobalChange={(useGlobal) => updateMeetingContext({ useGlobal })}
          onProviderChange={changeTranscriptionProvider}
          onRunChange={selectTranscriptionRun}
          onEditSegment={editTranscriptSegment}
          onEditSpeakerLabel={editSpeakerLabel}
          onReplaceSegments={replaceTranscriptSegments}
          onFormatTranscript={formatTranscript}
          canUndoReplacement={transcriptReplacementUndo?.transcriptionId === selectedTranscriptionId}
          onUndoReplacement={undoTranscriptReplacement}
          onFlushEdits={flushTranscriptEdits}
          onResetTranscript={resetTranscriptDocument}
          onSummaryProviderChange={changeSummaryProvider}
          onSummaryModelChange={changeSummaryModel}
          onGenerateSummary={generateSummary}
          onReveal={revealMeeting}
          onRename={renameMeeting}
          onDelete={deleteMeeting}
          onCreate={createRecording}
          onError={showError}
        />
        </div>
      {:else}
        <section class="settings-view" bind:this={settingsViewElement}>
          {#if recordingBusy}
            <button class="mobile-recording-return" type="button" onclick={() => navigate("meetings")}>
              <ArrowLeft aria-hidden="true" /><span>録音へ戻る</span>
            </button>
          {/if}
          {#if !summarySettingsPreview}
            <nav class="mobile-settings-nav" aria-label="設定カテゴリ">
              <button class:active={settingsPane === "general"} type="button" aria-current={settingsPane === "general" ? "page" : undefined} onclick={() => selectSettingsPane("general")}>一般</button>
              <button class:active={settingsPane === "transcription"} type="button" aria-current={settingsPane === "transcription" ? "page" : undefined} onclick={() => selectSettingsPane("transcription")}>文字起こし</button>
              <button class:active={settingsPane === "summary"} type="button" aria-current={settingsPane === "summary" ? "page" : undefined} onclick={() => selectSettingsPane("summary")}>AI会議ノート</button>
              <button class:active={settingsPane === "usage"} type="button" aria-current={settingsPane === "usage" ? "page" : undefined} onclick={() => selectSettingsPane("usage")}>利用状況</button>
            </nav>
          {/if}
          <div
            class="settings-detail-scroll mutsuna-scrollbar mutsuna-scrollbar--both-edges"
            use:scrollbarVisibility
          >
            <div class="settings-detail">
              {#if settingsPane === "general" && !summarySettingsPreview}
                <header class="settings-detail-heading">
                  <h1>一般</h1>
                </header>
                <div class="settings-section native-settings-group">
                  <PowerSettings disabled={updating} onError={showError} />
                  <AppUpdateManager
                    disabled={busy && !updating}
                    onBeforeInstall={prepareForUpdate}
                    onBusyChange={(value) => updating = value}
                  />
                  <ThirdPartyLicenses />
                </div>
              {:else if settingsPane === "transcription" && !summarySettingsPreview}
                <header class="settings-detail-heading">
                  <h1>文字起こし</h1>
                </header>
                <section class="settings-section">
                  <div class="settings-section-heading">
                    <h2>文字起こし方法</h2>
                  </div>
                  <div class="transcription-model-manager">
                    <LocalModelManager disabled={busy} preview={summarySettingsPreview} onChanged={refreshLocalModels} onMessage={showMessage} onError={showError} />
                    {#each apiKeyProviders as provider (provider.id)}
                      <ApiKeySettings
                        {provider}
                        {loading}
                        saving={savingProviderId === provider.id}
                        {deleting}
                        hasApiKey={provider.configured}
                        {busy}
                        onSave={(apiKey, accountId) => saveApiKey(provider.id, apiKey, accountId)}
                        onDelete={() => deleteApiKey(provider.id)}
                      />
                    {/each}
                  </div>
                </section>
                <section class="settings-section">
                  <div class="settings-section-heading">
                    <h2>認識のヒント</h2>
                  </div>
                  <div class="context-settings-wrap">
                    <TranscriptionContextEditor
                      title="すべての会議で使うヒント"
                      description="会社名や製品名など、よく使う言葉を登録します。"
                      contextEnabled={globalContextSettings.contextEnabled}
                      showMasterToggle
                      background={globalContextDraft.background}
                      termsText={globalContextDraft.termsText}
                      correctionsText={globalContextDraft.correctionsText}
                      saveState={globalContextSaveState}
                      loading={globalContextLoading}
                      disabled={busy}
                      onContextEnabledChange={(contextEnabled) => updateGlobalContext({ contextEnabled })}
                      onBackgroundChange={(background) => updateGlobalContext({ background })}
                      onTermsChange={(termsText) => updateGlobalContext({ termsText })}
                      onCorrectionsChange={(correctionsText) => updateGlobalContext({ correctionsText })}
                    />
                  </div>
                </section>
              {:else if settingsPane === "summary"}
                <header class="settings-detail-heading">
                  <h1>AI会議ノート</h1>
                </header>
                <section class="settings-section">
                  <div class="settings-section-heading">
                    <h2>最初に使うAI</h2>
                  </div>
                  <SummaryDefaultsSettings
                    providers={summaryProviders}
                    defaultProviderId={summaryDefaultProviderId}
                    defaultModelId={summaryDefaultModelId}
                    disabled={busy}
                    onLoadModels={(providerId) => refreshSummaryModels(providerId)}
                    onDefaultProviderChange={changeSummaryDefaultProvider}
                    onDefaultModelChange={changeSummaryDefaultModel}
                  />
                </section>
                <section class="settings-section">
                  <div class="settings-section-heading">
                    <h2>AIを追加・管理</h2>
                  </div>
                  <div class="summary-agent-wrap">
                    <SummaryAgentManager
                      disabled={busy}
                      providers={summaryProviders}
                      providerModelDefaults={summaryProviderModelDefaults}
                      preview={summarySettingsPreview}
                      onChanged={refreshSummaryProviders}
                      onLoadModels={(providerId) => refreshSummaryModels(providerId)}
                      onProviderDefaultModelChange={changeSummaryProviderDefaultModel}
                      onMessage={showMessage}
                      onError={showError}
                    />
                  </div>
                </section>
              {:else if settingsPane === "usage" && !summarySettingsPreview}
                <header class="settings-detail-heading">
                  <h1>利用状況</h1>
                </header>
                <div class="usage-settings-stack">
                  {#if hasApiKey}
                    <UsagePanel usage={transcriptionUsage} loading={usageLoading} error={usageError} onRefresh={refreshUsage} />
                  {/if}
                  {#if hasSonioxApiKey}
                    <SonioxUsagePanel usage={sonioxUsage} loading={sonioxUsageLoading} error={sonioxUsageError} onRefresh={refreshSonioxUsage} />
                  {/if}
                  {#if hasCloudflareApiKey}
                    <CloudflareUsagePanel usage={cloudflareUsage} loading={cloudflareUsageLoading} error={cloudflareUsageError} onRefresh={refreshCloudflareUsage} />
                  {/if}
                  {#if !hasApiKey && !hasSonioxApiKey && !hasCloudflareApiKey}
                    <section class="settings-empty-state">
                      <ChartNoAxesColumn aria-hidden="true" />
                      <h2>利用状況はまだありません</h2>
                      <p>文字起こしサービスを接続すると表示されます。</p>
                      <button type="button" onclick={() => selectSettingsPane("transcription")}>サービスを接続</button>
                    </section>
                  {/if}
                </div>
              {/if}
            </div>
          </div>
        </section>
      {/if}
      </div>
    </div>
  </AdminShellFrame>

  {#if OverlayPreviewControls}
    <div class="dev-preview-dock"><OverlayPreviewControls /></div>
  {/if}
</ThemeProvider>
