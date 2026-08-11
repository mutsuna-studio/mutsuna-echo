<script lang="ts">
  import { tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import Check from "@lucide/svelte/icons/check";
  import FileAudio from "@lucide/svelte/icons/file-audio";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import History from "@lucide/svelte/icons/history";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import {
    AlertDialog,
    AlertDialogAction,
    AlertDialogCancel,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle
  } from "@mutsuna/ui/alert-dialog";
  import { Button } from "@mutsuna/ui/button";
  import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator,
    DropdownMenuTrigger
  } from "@mutsuna/ui/dropdown-menu";
  import { InputGroup, InputGroupAddon, InputGroupInput } from "@mutsuna/ui/input-group";
  import { Select, SelectContent, SelectItem, SelectTrigger } from "@mutsuna/ui/select";
  import { formatActualCost, formatEstimatedCost, formatFileSize, formatTimestamp } from "../format";
  import {
    isTranscriptionProviderId,
    LOCAL_RECOGNITION_MODE_OPTIONS,
    transcriptionProviderOptions,
    type LocalRecognitionMode,
    type LocalRecognitionSettings,
    type TranscriptionProviderDefinition,
    type TranscriptionProviderId
  } from "../providers";
  import type { RecentMeetingSummary } from "../types/recording";
  import type {
    AudioSeekRequest,
    EditableTranscript,
    SelectedAudioFile,
    TranscriptionProgress,
    TranscriptionRunDetail,
    TranscriptionRunSummary,
    TranscriptSegmentTextChange,
    TranscriptSaveState,
    ContextSaveState,
    LocalDiarizationProgress
  } from "../types/transcript";
  import type { SummaryProviderDefinition, SummaryStatus } from "../types/summary";
  import AudioPlayer from "./AudioPlayer.svelte";
  import MeetingSummary from "./MeetingSummary.svelte";
  import TranscriptView from "./TranscriptView.svelte";
  import TranscriptionContextEditor from "./TranscriptionContextEditor.svelte";

  type MeetingContextDraft = { background: string; termsText: string; correctionsText: string; useGlobal: boolean };

  type Props = {
    selectedAudio: SelectedAudioFile | null;
    meeting: RecentMeetingSummary | null;
    transcript: EditableTranscript | null;
    runs: readonly TranscriptionRunSummary[];
    selectedTranscriptionId: string | null;
    selectedRun: TranscriptionRunDetail | null;
    saveState: TranscriptSaveState;
    summaryStatus: SummaryStatus | null;
    summaryProviders: readonly SummaryProviderDefinition[];
    summaryProviderId: string;
    summaryModelId: string;
    summaryModelsLoading: boolean;
    summaryGenerating: boolean;
    transcriptFormatting: boolean;
    providers: readonly TranscriptionProviderDefinition[];
    provider: TranscriptionProviderId;
    transcribing: boolean;
    progress: TranscriptionProgress | null;
    canTranscribe: boolean;
    diarizing: boolean;
    diarizationProgress: LocalDiarizationProgress | null;
    canDiarize: boolean;
    diarizationModelReady: boolean;
    contextEnabled: boolean;
    contextSurchargeActive: boolean;
    contextTermCount: number;
    contextDraft: MeetingContextDraft | null;
    contextSaveState: ContextSaveState;
    contextLoading: boolean;
    onTranscribe: (diarizationSpeakerCount?: number | null) => void;
    onDiarize: (speakerCount: number | null) => void;
    onCancelDiarization: () => void;
    onOpenDiarizationSettings: () => void;
    onContextBackgroundChange: (background: string) => void;
    onContextTermsChange: (termsText: string) => void;
    onContextCorrectionsChange: (correctionsText: string) => void;
    onContextUseGlobalChange: (useGlobal: boolean) => void;
    onProviderChange: (provider: TranscriptionProviderId) => void;
    onRunChange: (transcriptionId: string) => void;
    onEditSegment: (segmentId: string, text: string) => void;
    onEditSpeakerLabel: (speaker: string, label: string) => void;
    onReplaceSegments: (changes: TranscriptSegmentTextChange[]) => Promise<boolean>;
    onFormatTranscript: () => Promise<void>;
    canUndoReplacement: boolean;
    onUndoReplacement: () => Promise<void>;
    onFlushEdits: () => Promise<void>;
    onResetTranscript: () => void;
    onSummaryProviderChange: (value: string) => void;
    onSummaryModelChange: (value: string) => void;
    onGenerateSummary: () => void;
    onReveal: (meeting: RecentMeetingSummary) => void;
    onRename: (meeting: RecentMeetingSummary, newFileName: string) => void;
    onDelete: (meeting: RecentMeetingSummary, mode: "audioOnly" | "all") => Promise<void>;
    onCreate: () => void;
    onError: (message: string) => void;
  };

  let {
    selectedAudio,
    meeting,
    transcript,
    runs,
    selectedTranscriptionId,
    selectedRun,
    saveState,
    summaryStatus,
    summaryProviders,
    summaryProviderId,
    summaryModelId,
    summaryModelsLoading,
    summaryGenerating,
    transcriptFormatting,
    providers,
    provider,
    transcribing,
    progress,
    canTranscribe,
    diarizing,
    diarizationProgress,
    canDiarize,
    diarizationModelReady,
    contextEnabled,
    contextSurchargeActive,
    contextTermCount,
    contextDraft,
    contextSaveState,
    contextLoading,
    onTranscribe,
    onDiarize,
    onCancelDiarization,
    onOpenDiarizationSettings,
    onContextBackgroundChange,
    onContextTermsChange,
    onContextCorrectionsChange,
    onContextUseGlobalChange,
    onProviderChange,
    onRunChange,
    onEditSegment,
    onEditSpeakerLabel,
    onReplaceSegments,
    onFormatTranscript,
    canUndoReplacement,
    onUndoReplacement,
    onFlushEdits,
    onResetTranscript,
    onSummaryProviderChange,
    onSummaryModelChange,
    onGenerateSummary,
    onReveal,
    onRename,
    onDelete,
    onCreate,
    onError
  }: Props = $props();

  let detailTab = $state<"summary" | "transcript" | "info">("transcript");
  let playbackPositionMs = $state(0);
  let playbackPlaying = $state(false);
  let timelineFollowRequestId = $state(0);
  let detailContentElement = $state<HTMLElement | null>(null);
  let seekRequest = $state.raw<AudioSeekRequest | null>(null);
  let editingFileName = $state(false);
  let fileNameDraft = $state("");
  let fileNameInput = $state<HTMLInputElement | null>(null);
  let deleteDialogOpen = $state(false);
  let deleteDialogMeetingId = $state<string | null>(null);
  let diarizationSpeakerCount = $state("off");
  let localRecognitionMode = $state<LocalRecognitionMode>("fast");
  let localRecognitionModeWorking = $state(false);
  let seekRequestId = 0;
  const providerOptions = $derived(transcriptionProviderOptions(providers));
  const selectedProviderOption = $derived(providerOptions.find((option) => option.value === provider));
  const selectedProvider = $derived(
    selectedProviderOption
      ? providers.find((candidate) => candidate.id === selectedProviderOption.value) ?? null
      : null
  );
  const localRecognitionModeLabel = $derived(
    LOCAL_RECOGNITION_MODE_OPTIONS.find((option) => option.value === localRecognitionMode)?.label
      ?? "高速"
  );

  $effect(() => {
    const currentMeetingId = meeting?.meetingId ?? null;
    if (deleteDialogMeetingId === currentMeetingId) return;
    deleteDialogMeetingId = currentMeetingId;
    deleteDialogOpen = false;
  });

  async function deleteSelectedMeeting(mode: "audioOnly" | "all") {
    const selectedMeeting = meeting;
    if (!selectedMeeting) return;
    deleteDialogOpen = false;
    await onDelete(selectedMeeting, mode);
  }

  $effect(() => {
    let cancelled = false;
    void invoke<LocalRecognitionSettings>("get_local_recognition_settings")
      .then((settings) => {
        if (!cancelled) localRecognitionMode = settings.mode;
      })
      .catch((error) => {
        if (!cancelled) onError(typeof error === "string" ? error : "ローカル文字起こし設定を取得できませんでした。");
      });
    return () => { cancelled = true; };
  });

  async function changeLocalRecognitionMode(value: string | undefined) {
    if (value !== "fast" && value !== "accurate") return;
    const previous = localRecognitionMode;
    localRecognitionMode = value;
    localRecognitionModeWorking = true;
    try {
      const saved = await invoke<LocalRecognitionSettings>("set_local_recognition_settings", {
        settings: { mode: localRecognitionMode }
      });
      localRecognitionMode = saved.mode;
    } catch (error) {
      localRecognitionMode = previous;
      onError(typeof error === "string" ? error : "ローカル文字起こし設定を変更できませんでした。");
    } finally {
      localRecognitionModeWorking = false;
    }
  }
  const selectedRunSummary = $derived(runs.find((run) => run.transcriptionId === selectedTranscriptionId) ?? runs[0] ?? null);
  const saveStatus = $derived.by(() => {
    if (!selectedRun) return "";
    if (saveState === "saving") return "保存中…";
    if (saveState === "unsaved") return "変更あり";
    if (saveState === "error") return "保存できませんでした";
    return selectedRun.edited ? "編集済み・保存済み" : "保存済み";
  });
  const diarizationProgressLabel = $derived.by(() => {
    if (!diarizationProgress) return "";
    if (diarizationProgress.stage === "loadingModel") return "モデルを読み込み中…";
    if (diarizationProgress.stage === "decodingAudio") return "音声を準備中…";
    if (diarizationProgress.stage === "stitchingSpeakers") return "長時間音声の話者を統合中…";
    if (diarizationProgress.stage === "finalizing") return "文字起こしへ反映中…";
    return diarizationProgress.totalChunks != null
      ? `${diarizationProgress.completedChunks} / ${diarizationProgress.totalChunks} チャンク`
      : `${diarizationProgress.completedChunks} チャンク完了`;
  });

  $effect(() => {
    if (provider !== "local" || !diarizationModelReady) diarizationSpeakerCount = "off";
  });

  function startTranscription() {
    if (provider !== "local" || diarizationSpeakerCount === "off") {
      onTranscribe(undefined);
      return;
    }
    const count = diarizationSpeakerCount === "auto" ? null : Number(diarizationSpeakerCount);
    onTranscribe(count);
  }

  const recordedAt = $derived(
    meeting
      ? new Intl.DateTimeFormat("ja-JP", { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }).format(
          meeting.source === "recording" ? meeting.occurredAtUnixMs : meeting.updatedAtUnixMs
        )
      : null
  );
  const selectedFileExtension = $derived(selectedAudio?.name.match(/\.[^.]+$/)?.[0] ?? "");
  const selectedFileStem = $derived(
    selectedAudio ? selectedAudio.name.slice(0, selectedAudio.name.length - selectedFileExtension.length) : ""
  );
  const transcriptionLabel = $derived.by(() => {
    if (!transcribing) return transcript ? "再文字起こし" : "文字起こし";
    if (progress?.stage === "detectingSpeech") {
      if (progress.totalChunks != null) return `${progress.completedChunks} / ${progress.totalChunks}`;
      return "発話を検出中…";
    }
    if (progress?.stage === "transcribing" && progress.totalChunks != null) return `${progress.completedChunks} / ${progress.totalChunks}`;
    return "文字起こし中…";
  });
  const contextStatus = $derived.by(() => {
    if (!selectedProvider) return "モデルを選択してください";
    if (!contextEnabled) return "コンテキスト: オフ";
    if (!selectedProvider?.capabilities.contextText && !selectedProvider?.capabilities.contextTerms) return "このモデルはコンテキスト非対応";
    if (!selectedProvider.capabilities.contextText && contextTermCount === 0) return "このモデルへ送信できる重要用語はありません";
    if (contextSurchargeActive) {
      const estimated = selectedAudio && selectedProvider?.pricingUsdPerHour != null
        ? formatEstimatedCost(selectedAudio.durationMs / 3_600_000 * selectedProvider.pricingUsdPerHour * 1.2)
        : null;
      return `重要用語 ${contextTermCount}件を使用・料金 +20%${estimated ? `・推定 ${estimated}` : ""}`;
    }
    if (contextTermCount > 0 || contextDraft?.background.trim()) return "コンテキストを使用";
    return "コンテキストは空です";
  });

  $effect(() => {
    selectedAudio?.meetingId;
    playbackPositionMs = 0;
    playbackPlaying = false;
    timelineFollowRequestId = 0;
    seekRequest = null;
    seekRequestId = 0;
    editingFileName = false;
  });

  async function startFileNameEditing() {
    fileNameDraft = selectedFileStem;
    editingFileName = true;
    await tick();
    fileNameInput?.focus();
    fileNameInput?.select();
  }

  function commitFileName() {
    const value = fileNameDraft.trim();
    editingFileName = false;
    if (!meeting || !value || value === selectedFileStem) return;
    onRename(meeting, `${value}${selectedFileExtension}`);
  }

  function cancelFileNameEditing() {
    editingFileName = false;
    fileNameDraft = selectedFileStem;
  }

  function handleFileNameKeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      commitFileName();
    } else if (event.key === "Escape") {
      event.preventDefault();
      cancelFileNameEditing();
    }
  }

  function selectProvider(value: string) {
    if (isTranscriptionProviderId(value)) onProviderChange(value);
  }

  function seekFromTranscript(positionMs: number) {
    if (!selectedAudio) return;
    playbackPositionMs = positionMs;
    seekRequest = { meetingId: selectedAudio.meetingId, requestId: ++seekRequestId, positionMs };
  }

  function playFromTranscript(positionMs: number) {
    if (!selectedAudio) return;
    const contextPositionMs = Math.max(0, positionMs - 3_000);
    playbackPositionMs = contextPositionMs;
    seekRequest = {
      meetingId: selectedAudio.meetingId,
      requestId: ++seekRequestId,
      positionMs: contextPositionMs,
      autoplay: true
    };
  }

  function pauseFromTranscript() {
    if (!selectedAudio) return;
    seekRequest = {
      meetingId: selectedAudio.meetingId,
      requestId: ++seekRequestId,
      positionMs: playbackPositionMs,
      pause: true
    };
  }

  function seekFromSummary(positionMs: number) {
    detailTab = "transcript";
    seekFromTranscript(positionMs);
    timelineFollowRequestId += 1;
  }

  function handlePlaybackPosition(positionMs: number, followTimeline: boolean) {
    playbackPositionMs = positionMs;
    if (followTimeline) timelineFollowRequestId += 1;
  }

  function runModelLabel(run: TranscriptionRunSummary): string {
    return providers.find((provider) => provider.modelId === run.model)?.modelLabel
      ?? ({ scribe_v2: "Scribe v2", "reazonspeech-k2-int8-fp32": "ReazonSpeech K2" } as Record<string, string>)[run.model]
      ?? run.model;
  }

  function runDate(value: string): string {
    const date = new Date(value);
    return Number.isNaN(date.getTime())
      ? value
      : new Intl.DateTimeFormat("ja-JP", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(date);
  }
</script>

<section class="meeting-workspace">
  {#if meeting || selectedAudio}
    {#if selectedAudio}
      <div class="audio-player-wrap">
        <AudioPlayer
          audio={selectedAudio}
          {seekRequest}
          onPositionChange={handlePlaybackPosition}
          onPlayingChange={(playing) => playbackPlaying = playing}
          {onError}
        />
      </div>
    {:else}
      <div class="audio-unavailable" role="status">
        <FileAudio aria-hidden="true" />
        <span><strong>音声ファイルは削除されています</strong><small>文字起こしと会議ノートは引き続き確認・編集できます。</small></span>
      </div>
    {/if}

    <div class="detail-tabs" role="tablist" aria-label="会議の表示内容">
      <button class:active={detailTab === "summary"} type="button" role="tab" aria-selected={detailTab === "summary"} onclick={() => detailTab = "summary"}>会議ノート</button>
      <button class:active={detailTab === "transcript"} type="button" role="tab" aria-selected={detailTab === "transcript"} onclick={() => detailTab = "transcript"}>文字起こし</button>
      <button class:active={detailTab === "info"} type="button" role="tab" aria-selected={detailTab === "info"} onclick={() => detailTab = "info"}>会議情報</button>
    </div>

    <div class="detail-content" bind:this={detailContentElement}>
      {#if detailTab === "summary"}
        <MeetingSummary
          {transcript}
          status={summaryStatus}
          providers={summaryProviders}
          providerId={summaryProviderId}
          modelId={summaryModelId}
          modelsLoading={summaryModelsLoading}
          generating={summaryGenerating}
          blocked={transcriptFormatting}
          playbackAvailable={Boolean(selectedAudio)}
          onProviderChange={onSummaryProviderChange}
          onModelChange={onSummaryModelChange}
          onGenerate={onGenerateSummary}
          onSeekSource={seekFromSummary}
        />
      {:else if detailTab === "transcript"}
        {#if transcript}
          <section class="transcription-toolbar" aria-label="文字起こしモデルと再実行">
            <div class="transcription-heading">
              <div class="transcription-title-row">
                <strong>文字起こし</strong>
                {#if runs.length > 0}
                  <DropdownMenu>
                    <DropdownMenuTrigger>
                      {#snippet child({ props })}
                        <Button {...props} size="xs" variant="ghost" type="button" icon={History} aria-label={`文字起こし履歴 ${runs.length}件`} disabled={transcriptFormatting}>
                          履歴 {runs.length}
                        </Button>
                      {/snippet}
                    </DropdownMenuTrigger>
                    <DropdownMenuContent class="transcription-history-menu" align="start">
                      {#each runs as run (run.transcriptionId)}
                        <DropdownMenuItem class="transcription-history-item" onclick={() => onRunChange(run.transcriptionId)}>
                          <span class="history-item-copy">
                            <strong>{run.sequence}回目・{runModelLabel(run)}</strong>
                            <small>
                              {runDate(run.createdAt)}{run.edited ? "・編集済み" : ""}{run.costUsd != null ? `・実コスト ${formatActualCost(run.costUsd)}` : ""}
                            </small>
                          </span>
                          {#if run.transcriptionId === selectedTranscriptionId}<Check aria-hidden="true" />{/if}
                        </DropdownMenuItem>
                      {/each}
                      {#if selectedRun?.edited && saveState === "saved"}
                        <DropdownMenuSeparator />
                        <DropdownMenuItem onclick={onResetTranscript}>
                          <RotateCcw aria-hidden="true" />
                          原文に戻す
                        </DropdownMenuItem>
                      {/if}
                    </DropdownMenuContent>
                  </DropdownMenu>
                {/if}
              </div>
              <span class:error={saveState === "error"} class:pending={saveState === "saving" || saveState === "unsaved"} aria-live="polite" aria-atomic="true">
                {#if selectedRunSummary}{selectedRunSummary.sequence}回目・{runModelLabel(selectedRunSummary)}{/if}{#if selectedRunSummary && saveStatus}・{/if}{saveStatus || "モデルを変更して再実行できます"}
              </span>
            </div>
            <div class="transcription-controls">
              <Select
                type="single"
                value={provider}
                onValueChange={selectProvider}
                disabled={transcribing || diarizing || transcriptFormatting || providerOptions.length === 0}
              >
                <SelectTrigger aria-label="文字起こしモデル" class="transcription-model-select">
                  <span>{selectedProviderOption?.label ?? "モデルを選択"}</span>
                </SelectTrigger>
                <SelectContent>
                  {#each providerOptions as option (option.value)}
                    <SelectItem value={option.value}>{option.label}</SelectItem>
                  {/each}
                </SelectContent>
              </Select>
              {#if provider === "local"}
                <Select
                  type="single"
                  value={localRecognitionMode}
                  onValueChange={changeLocalRecognitionMode}
                  disabled={transcribing || diarizing || transcriptFormatting || localRecognitionModeWorking}
                >
                  <SelectTrigger aria-label="ローカル文字起こしの精度" class="transcription-model-select">
                    <span>精度: {localRecognitionModeLabel}</span>
                  </SelectTrigger>
                  <SelectContent>
                    {#each LOCAL_RECOGNITION_MODE_OPTIONS as option (option.value)}
                      <SelectItem value={option.value}>{option.label} — {option.description}</SelectItem>
                    {/each}
                  </SelectContent>
                </Select>
              {/if}
              <span class="transcription-action" data-transcription-action>
                <Button size="sm" variant="outline" type="button" onclick={startTranscription} disabled={!canTranscribe} loading={transcribing}>{transcriptionLabel}</Button>
              </span>
              {#if provider === "local"}
                <Select
                  type="single"
                  value={diarizationSpeakerCount}
                  onValueChange={(value) => { if (value) diarizationSpeakerCount = value; }}
                  disabled={diarizing || transcribing || transcriptFormatting || !diarizationModelReady}
                >
                  <SelectTrigger aria-label="話者分離" class="speaker-count-select">
                    <span>{diarizationSpeakerCount === "off" ? "話者分離: オフ" : diarizationSpeakerCount === "auto" ? "話者分離: 自動" : `話者分離: ${diarizationSpeakerCount}人`}</span>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="off">話者分離: オフ</SelectItem>
                    <SelectItem value="auto">話者分離: 人数自動</SelectItem>
                    {#each Array.from({ length: 9 }, (_, index) => index + 2) as count}
                      <SelectItem value={String(count)}>話者分離: {count}人</SelectItem>
                    {/each}
                  </SelectContent>
                </Select>
                {#if !diarizationModelReady}
                  <Button size="sm" variant="outline" type="button" onclick={onOpenDiarizationSettings}>話者分離モデルを追加</Button>
                {/if}
              {/if}
            </div>
            {#if transcribing && diarizing && diarizationProgressLabel}
              <div class="diarization-controls"><small aria-live="polite">話者分離: {diarizationProgressLabel}</small></div>
            {/if}
          </section>
          <p class:surcharge={contextSurchargeActive} class="context-status">{contextStatus}</p>
          <TranscriptView
            {transcript}
            transcriptionId={selectedTranscriptionId}
            currentPositionMs={playbackPositionMs}
            playing={playbackPlaying}
            playbackAvailable={Boolean(selectedAudio)}
            followRequestId={timelineFollowRequestId}
            scrollContainer={detailContentElement}
            onSeek={seekFromTranscript}
            onPlay={playFromTranscript}
            onPause={pauseFromTranscript}
            editable={Boolean(selectedRun) && !diarizing}
            formatting={transcriptFormatting || diarizing}
            {onEditSegment}
            {onEditSpeakerLabel}
            {onReplaceSegments}
            onFormat={onFormatTranscript}
            {canUndoReplacement}
            {onUndoReplacement}
            onBlur={onFlushEdits}
          />
        {:else}
          <div class="empty-transcript">
            <FileAudio aria-hidden="true" />
            <h2>文字起こしはまだありません</h2>
            <p>{selectedAudio ? "使用するモデルを選んで、この音声の文字起こしを始めます。" : "音声ファイルがないため、新しく文字起こしを作成することはできません。"}</p>
            {#if selectedAudio}
              <div class="transcription-start-controls">
                <label for="transcription-model">文字起こしモデル</label>
                <Select
                  type="single"
                  value={provider}
                  onValueChange={selectProvider}
                  disabled={transcribing || providerOptions.length === 0}
                >
                  <SelectTrigger id="transcription-model" aria-label="文字起こしモデル" class="transcription-model-select">
                    <span>{selectedProviderOption?.label ?? "モデルを選択"}</span>
                  </SelectTrigger>
                  <SelectContent>
                    {#each providerOptions as option (option.value)}
                      <SelectItem value={option.value}>{option.label}</SelectItem>
                    {/each}
                  </SelectContent>
                </Select>
                {#if provider === "local"}
                  <Select
                    type="single"
                    value={localRecognitionMode}
                    onValueChange={changeLocalRecognitionMode}
                    disabled={transcribing || localRecognitionModeWorking}
                  >
                    <SelectTrigger aria-label="ローカル文字起こしの精度" class="transcription-model-select">
                      <span>精度: {localRecognitionModeLabel}</span>
                    </SelectTrigger>
                    <SelectContent>
                      {#each LOCAL_RECOGNITION_MODE_OPTIONS as option (option.value)}
                        <SelectItem value={option.value}>{option.label} — {option.description}</SelectItem>
                      {/each}
                    </SelectContent>
                  </Select>
                  <Select
                    type="single"
                    value={diarizationSpeakerCount}
                    onValueChange={(value) => { if (value) diarizationSpeakerCount = value; }}
                    disabled={transcribing || localRecognitionModeWorking || !diarizationModelReady}
                  >
                    <SelectTrigger aria-label="話者分離" class="speaker-count-select">
                      <span>{diarizationSpeakerCount === "off" ? "話者分離: オフ" : diarizationSpeakerCount === "auto" ? "話者分離: 自動" : `話者分離: ${diarizationSpeakerCount}人`}</span>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="off">話者分離: オフ</SelectItem>
                      <SelectItem value="auto">話者分離: 人数自動</SelectItem>
                      {#each Array.from({ length: 9 }, (_, index) => index + 2) as count}
                        <SelectItem value={String(count)}>話者分離: {count}人</SelectItem>
                      {/each}
                    </SelectContent>
                  </Select>
                  {#if !diarizationModelReady}
                    <Button size="sm" variant="outline" type="button" onclick={onOpenDiarizationSettings}>話者分離モデルを追加</Button>
                  {/if}
                {/if}
                <span class="transcription-action" data-transcription-action>
                  <Button size="lg" type="button" onclick={startTranscription} disabled={!canTranscribe} loading={transcribing}>{transcriptionLabel}</Button>
                </span>
                {#if transcribing && diarizing && diarizationProgressLabel}<small aria-live="polite">話者分離: {diarizationProgressLabel}</small>{/if}
                <small class:surcharge={contextSurchargeActive} class="context-start-status">{contextStatus}</small>
              </div>
            {/if}
          </div>
        {/if}
      {:else if meeting}
        <dl class="meeting-info">
          <div><dt>ファイル名</dt><dd>{#if meeting.source === "recording" && selectedAudio}{#if editingFileName}<InputGroup><InputGroupInput bind:ref={fileNameInput} bind:value={fileNameDraft} maxlength={123} aria-label="ファイル名を編集" onkeydown={handleFileNameKeydown} onblur={commitFileName} /><InputGroupAddon align="inline-end">{selectedFileExtension}</InputGroupAddon></InputGroup>{:else}<button class="file-name-display" type="button" aria-label="ファイル名を編集" onclick={startFileNameEditing}>{selectedAudio.name}</button>{/if}{:else}{meeting.fileName}{/if}</dd></div>
          <div><dt>{meeting.source === "recording" ? "録音日時" : "更新日時"}</dt><dd>{recordedAt ?? "読み込んだ音声"}</dd></div>
          <div><dt>長さ</dt><dd>{selectedAudio ? formatTimestamp(selectedAudio.durationMs) : "音声ファイルは削除済み"}</dd></div>
          <div><dt>ファイルサイズ</dt><dd>{formatFileSize(meeting.sizeBytes)}</dd></div>
          {#if meeting.audioAvailable}
            <div><dt>保存場所</dt><dd><Button size="sm" variant="outline" type="button" icon={FolderOpen} onclick={() => onReveal(meeting)}>場所を開く</Button></dd></div>
          {/if}
          <div><dt>Meeting ID</dt><dd class="meeting-id">{meeting.meetingId}</dd></div>
        </dl>
        {#if contextDraft}
          <div class="meeting-context-wrap">
            <TranscriptionContextEditor
              title="この会議のコンテキスト"
              description="この会議だけで使う背景情報と重要用語を設定します。"
              {contextEnabled}
              background={contextDraft.background}
              termsText={contextDraft.termsText}
              correctionsText={contextDraft.correctionsText}
              useGlobal={contextDraft.useGlobal}
              provider={selectedProvider}
              saveState={contextSaveState}
              loading={contextLoading}
              disabled={transcribing}
              onBackgroundChange={onContextBackgroundChange}
              onTermsChange={onContextTermsChange}
              onCorrectionsChange={onContextCorrectionsChange}
              onUseGlobalChange={onContextUseGlobalChange}
            />
          </div>
        {:else if contextLoading}
          <p class="meeting-info-loading" role="status">文字起こしコンテキストを読み込んでいます…</p>
        {:else if contextSaveState === "error"}
          <p class="meeting-info-loading context-error" role="alert">文字起こしコンテキストを読み込めませんでした。会議を開き直してください。</p>
        {/if}
        <section class="danger-zone" aria-labelledby="meeting-delete-heading">
          <div>
            <strong id="meeting-delete-heading">会議を削除</strong>
            <span>音声だけを削除するか、文字起こしや会議ノートを含めて削除できます。</span>
          </div>
          <Button size="sm" variant="outline" type="button" icon={Trash2} onclick={() => deleteDialogOpen = true}>削除方法を選ぶ</Button>
        </section>
      {:else}
        <p class="meeting-info-loading" role="status">会議情報を読み込んでいます…</p>
      {/if}
    </div>
  {:else}
    <div class="workspace-empty">
      <FileAudio aria-hidden="true" />
      <h1>会議を選択してください</h1>
      <p>最近の録音を開くか、新しい録音・音声ファイルから始められます。</p>
      <Button type="button" onclick={onCreate}>新しい録音</Button>
    </div>
  {/if}
</section>

{#if meeting}
  <AlertDialog bind:open={deleteDialogOpen}>
    <AlertDialogContent>
      <AlertDialogHeader>
        <AlertDialogTitle>何を削除しますか？</AlertDialogTitle>
        <AlertDialogDescription>
          どちらの操作も取り消せません。「音声だけ」を選ぶと、文字起こしと会議ノートは残ります。
        </AlertDialogDescription>
      </AlertDialogHeader>
      <div class="delete-options">
        <AlertDialogAction variant="outline" onclick={() => deleteSelectedMeeting("audioOnly")}>
          <span><strong>音声ファイルだけ削除</strong><small>文字起こしと会議ノートは残す</small></span>
        </AlertDialogAction>
        <AlertDialogAction variant="destructive" onclick={() => deleteSelectedMeeting("all")}>
          <span><strong>会議をすべて削除</strong><small>音声・文字起こし・会議ノートを削除</small></span>
        </AlertDialogAction>
      </div>
      <AlertDialogFooter><AlertDialogCancel>キャンセル</AlertDialogCancel></AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
{/if}

<style>
  .meeting-workspace { display: grid; width: 100%; height: 100%; min-width: 0; min-height: 0; grid-template-rows: auto auto auto auto minmax(0, 1fr); overflow: hidden; background: var(--background); }

  .transcription-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 20px; margin: 22px 0 20px; padding-bottom: 18px; border-bottom: 1px solid var(--border); }
  .transcription-heading { display: grid; min-width: 0; gap: 3px; }
  .transcription-title-row { display: flex; min-width: 0; align-items: center; gap: 6px; }
  .transcription-heading strong { font-size: 0.88rem; }
  .transcription-heading span { color: var(--muted-foreground); font-size: 0.72rem; }
  .transcription-heading span.error { color: var(--destructive); }
  .transcription-heading span.pending { color: var(--foreground); }
  .transcription-controls { display: flex; min-width: 0; align-items: center; gap: 8px; }
  .diarization-controls { display: flex; min-width: 0; align-items: center; gap: 8px; flex-wrap: wrap; }
  .diarization-controls small { color: var(--muted-foreground); }
  .diarization-controls :global(.speaker-count-select) { min-width: 132px; }
  .context-status { margin: -10px 0 18px; color: var(--muted-foreground); font-size: 0.72rem; }
  .context-status.surcharge { color: var(--foreground); font-weight: 650; }
  .transcription-toolbar :global([data-slot="select-trigger"]) { width: 240px; }
  :global(.transcription-history-menu) { width: min(320px, calc(100vw - 32px)); }
  :global(.transcription-history-item) { justify-content: space-between; gap: 12px; padding: 8px 9px; }
  :global(.transcription-history-item > svg) { color: var(--primary); }
  .history-item-copy { display: grid; min-width: 0; flex: 1; gap: 2px; }
  .history-item-copy strong, .history-item-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .history-item-copy strong { color: var(--foreground); font-size: 0.78rem; }
  .history-item-copy small { color: var(--muted-foreground); font-size: 0.68rem; }

  .audio-player-wrap { min-width: 0; margin: 12px 30px; container: audio-player / inline-size; }
  .audio-unavailable { display: flex; min-width: 0; align-items: center; gap: 12px; margin: 16px 30px 12px; padding: 14px 16px; border: 1px solid var(--border); border-radius: 12px; color: var(--muted-foreground); background: var(--muted); }
  .audio-unavailable > :global(svg) { width: 24px; height: 24px; flex: none; }
  .audio-unavailable span { display: grid; min-width: 0; gap: 2px; }
  .audio-unavailable strong { color: var(--foreground); font-size: 0.82rem; }
  .audio-unavailable small { font-size: 0.72rem; line-height: 1.5; }

  .detail-tabs { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); margin: 0 30px; border-bottom: 1px solid var(--border); }
  .detail-tabs button { position: relative; height: 42px; border: 0; color: var(--muted-foreground); background: transparent; cursor: pointer; font: inherit; font-size: 0.82rem; font-weight: 650; }
  .detail-tabs button::after { position: absolute; right: 0; bottom: -1px; left: 0; height: 2px; background: transparent; content: ""; }
  .detail-tabs button.active { color: var(--primary); }
  .detail-tabs button.active::after { background: var(--primary); }
  .detail-tabs button:focus-visible { outline: 2px solid var(--ring); outline-offset: -3px; }
  .detail-content { min-height: 0; overflow-x: hidden; overflow-y: auto; padding: 0 30px 36px; overscroll-behavior: contain; scrollbar-gutter: stable; }

  .empty-transcript, .workspace-empty { display: grid; place-items: center; align-content: center; text-align: center; }
  .empty-transcript { min-height: 420px; margin-top: 24px; padding: 32px; border: 1px solid var(--border); border-radius: 14px; background: color-mix(in oklch, var(--muted) 28%, var(--background)); }
  .empty-transcript > :global(svg), .workspace-empty > :global(svg) { width: 34px; height: 34px; color: var(--primary); stroke-width: 1.5; }
  .empty-transcript h2, .workspace-empty h1 { margin: 14px 0 7px; font-size: 1.05rem; }
  .empty-transcript p, .workspace-empty p { max-width: 420px; margin: 0 0 18px; color: var(--muted-foreground); font-size: 0.82rem; line-height: 1.6; }
  .transcription-start-controls { display: grid; width: min(100%, 360px); gap: 10px; text-align: left; }
  .transcription-start-controls label { color: var(--muted-foreground); font-size: 0.72rem; font-weight: 650; }
  .context-start-status { color: var(--muted-foreground); font-size: 0.7rem; line-height: 1.5; text-align: center; }
  .context-start-status.surcharge { color: var(--foreground); font-weight: 650; }
  .transcription-start-controls :global([data-slot="select-trigger"]) { width: 100%; }
  .transcription-start-controls .transcription-action, .transcription-start-controls :global(button[data-slot="button"]) { width: 100%; }
  .workspace-empty { position: relative; grid-row: 1 / -1; min-height: 100%; padding: 40px; }

  .meeting-info { display: grid; max-width: 720px; margin: 24px 0 0; }
  .meeting-info-loading { margin: 28px 0; color: var(--muted-foreground); font-size: 0.82rem; }
  .meeting-info-loading.context-error { color: var(--destructive); }
  .meeting-info div { display: grid; grid-template-columns: 150px minmax(0, 1fr); gap: 18px; padding: 14px 0; border-bottom: 1px solid var(--border); }
  .meeting-info dt { color: var(--muted-foreground); font-size: 0.79rem; }
  .meeting-info dd { margin: 0; font-size: 0.84rem; }
  .file-name-display { max-width: 100%; padding: 0; border: 0; color: inherit; background: transparent; font: inherit; text-align: left; overflow-wrap: anywhere; cursor: text; }
  .file-name-display:hover { text-decoration: underline; text-decoration-color: color-mix(in oklch, var(--foreground) 30%, transparent); text-underline-offset: 3px; }
  .meeting-id { overflow-wrap: anywhere; font-family: ui-monospace, monospace; font-size: 0.75rem !important; }
  .meeting-context-wrap { margin-top: 24px; }
  .danger-zone { display: flex; max-width: 720px; align-items: center; justify-content: space-between; gap: 20px; margin-top: 32px; padding: 18px; border: 1px solid color-mix(in oklch, var(--destructive) 35%, var(--border)); border-radius: 12px; }
  .danger-zone > div { display: grid; min-width: 0; gap: 4px; }
  .danger-zone strong { font-size: 0.84rem; }
  .danger-zone span { color: var(--muted-foreground); font-size: 0.74rem; line-height: 1.5; }
  .delete-options { display: grid; gap: 10px; }
  .delete-options :global(button) { height: auto; min-height: 64px; justify-content: flex-start; padding: 12px 16px; text-align: left; }
  .delete-options span { display: grid; gap: 3px; }
  .delete-options small { font-weight: 400; opacity: 0.8; }

  @media (max-width: 780px) {
    .audio-player-wrap, .audio-unavailable, .detail-tabs { margin-right: 18px; margin-left: 18px; }
    .detail-content { padding-right: 18px; padding-left: 18px; }
  }

  @media (max-width: 520px) {
    .transcription-toolbar { flex-direction: column; align-items: stretch; gap: 14px; margin-top: 20px; padding-bottom: 20px; }
    .transcription-controls { flex-direction: column; align-items: stretch; gap: 10px; }
    .diarization-controls { align-items: stretch; }
    .transcription-toolbar :global([data-slot="select-trigger"]) { width: 100%; min-height: 44px; }
    .transcription-action { width: 100%; }
    .transcription-action :global(button) { width: 100%; min-height: 48px; padding-right: 18px; padding-left: 18px; font-size: 0.95rem; }
    .empty-transcript { min-height: 380px; margin-top: 18px; padding: 28px 18px; }
    .transcription-start-controls :global([data-slot="select-trigger"]) { min-height: 48px; }
    .danger-zone { align-items: stretch; flex-direction: column; }
    .danger-zone :global(button) { width: 100%; min-height: 44px; justify-content: center; }
  }
</style>
