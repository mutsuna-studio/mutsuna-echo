<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import FileUp from "@lucide/svelte/icons/file-up";
  import Mic from "@lucide/svelte/icons/mic";
  import { Button } from "@mutsuna/ui/button";
  import { Select } from "@mutsuna/ui/select";
  import RecordingPanel from "./RecordingPanel.svelte";
  import { formatEstimatedCost, formatFileSize, formatTimestamp } from "../format";
  import {
    getTranscriptionProvider,
    isTranscriptionProviderId,
    type TranscriptionProviderDefinition,
    type TranscriptionProviderId
  } from "../providers";
  import type { SelectedAudioFile, TranscriptionProgress } from "../types/transcript";

  interface Props {
    selectedAudio: SelectedAudioFile | null;
    providers: readonly TranscriptionProviderDefinition[];
    provider: TranscriptionProviderId;
    selecting: boolean;
    transcribing: boolean;
    transcriptionProgress: TranscriptionProgress | null;
    recordingBusy: boolean;
    busy: boolean;
    recordingDisabled: boolean;
    canTranscribe: boolean;
    onSelect: () => void;
    onTranscribe: () => void;
    onOpenSettings: () => void;
    onProviderChange: (provider: TranscriptionProviderId) => void;
    onRecordedAudio: (audio: SelectedAudioFile) => void;
    onRecordingBusyChange: (busy: boolean) => void;
    onRecordingModeChange: (active: boolean) => void;
    onMessage: (message: string) => void;
    onError: (message: string) => void;
  }

  let {
    selectedAudio,
    providers,
    provider,
    selecting,
    transcribing,
    transcriptionProgress,
    recordingBusy,
    busy,
    recordingDisabled,
    canTranscribe,
    onSelect,
    onTranscribe,
    onOpenSettings,
    onProviderChange,
    onRecordedAudio,
    onRecordingBusyChange,
    onRecordingModeChange,
    onMessage,
    onError
  }: Props = $props();

  let inputMode = $state<"choose" | "record">("choose");
  let audioBeforeRecording = $state<string | null>(null);
  const providerDefinition = $derived(getTranscriptionProvider(providers, provider));
  const usableProviders = $derived(providers.filter((availableProvider) => availableProvider.ready));
  const providerOptions = $derived(usableProviders.map((availableProvider) => ({
    value: availableProvider.id,
    label: `${truncateModelLabel(availableProvider.modelLabel)} · ${formatHourlyPrice(availableProvider)}`,
    description: availableProvider.kind === "local"
      ? "ローカル · 音声を外部送信しません"
      : `クラウド · ${availableProvider.capabilitySummary}`
  })));
  const estimatedCostUsd = $derived(
    selectedAudio && providerDefinition?.pricingUsdPerHour != null
      ? selectedAudio.durationMs / 3_600_000 * providerDefinition.pricingUsdPerHour
      : null
  );
  const transcriptionStatus = $derived.by(() => {
    if (!transcribing) return "文字起こし開始";
    if (transcriptionProgress?.stage === "detectingSpeech") return "発話区間を検出中…";
    if (transcriptionProgress?.stage === "transcribing") {
      if (transcriptionProgress.totalChunks != null) {
        return `文字起こし中 ${transcriptionProgress.completedChunks} / ${transcriptionProgress.totalChunks}`;
      }
      return "文字起こし中…";
    }
    return "準備中…";
  });

  function formatHourlyPrice(provider: TranscriptionProviderDefinition): string {
    return provider.kind === "local"
      ? "無料"
      : provider.pricingUsdPerHour == null
        ? "料金を確認中"
        : `$${provider.pricingUsdPerHour.toFixed(2)}/時間`;
  }

  function truncateModelLabel(label: string, maxLength = 16): string {
    return label.length > maxLength ? `${label.slice(0, maxLength)}...` : label;
  }

  function selectProvider(value: string) {
    if (isTranscriptionProviderId(value)) onProviderChange(value);
  }

  function openRecorder() {
    audioBeforeRecording = selectedAudio?.meetingId ?? null;
    inputMode = "record";
    onRecordingModeChange(true);
  }

  function closeRecorder() {
    if (recordingBusy) return;
    inputMode = "choose";
    onRecordingModeChange(false);
  }

  $effect(() => {
    const meetingId = selectedAudio?.meetingId ?? null;
    if (inputMode === "record" && meetingId && meetingId !== audioBeforeRecording) {
      inputMode = "choose";
      onRecordingModeChange(false);
    }
  });

</script>

<section class="creation-panel" aria-busy={selecting || transcribing}>
  <section class="audio-source" aria-label="音声の入力方法">
    {#if inputMode === "record"}
      <div class="source-panel-heading">
        <Button size="sm" variant="ghost" type="button" icon={ArrowLeft} onclick={closeRecorder} disabled={recordingBusy}>戻る</Button>
      </div>
      <RecordingPanel
        disabled={recordingDisabled}
        onAudioReady={onRecordedAudio}
        onBusyChange={onRecordingBusyChange}
        {onMessage}
        {onError}
      />
    {:else}
      <div class="source-actions">
        <Button class="source-action source-action-primary" size="lg" type="button" onclick={openRecorder} disabled={busy || recordingDisabled}>
          <span class="source-action-icon"><Mic aria-hidden="true" /></span>
          <span class="source-action-copy">
            <strong>録音を開始</strong>
            <small>マイクとシステム音声を録音</small>
          </span>
        </Button>
        <Button class={selectedAudio ? "source-action selected-audio" : "source-action"} variant="outline" size="lg" type="button" onclick={onSelect} disabled={busy}>
          <span class="source-action-icon"><FileUp aria-hidden="true" /></span>
          <span class="source-action-copy">
            <strong>{selecting ? "ファイルを確認中…" : selectedAudio?.name ?? "音声ファイルを読み込む"}</strong>
            <small>
              {selectedAudio
                ? `${formatTimestamp(selectedAudio.durationMs)} · ${formatFileSize(selectedAudio.sizeBytes)}`
                : "MP3・M4A・WAV・FLAC"}
            </small>
          </span>
        </Button>
      </div>
    {/if}
  </section>

  <div class="transcription-settings">
    <div class="transcription-settings-heading">
      <h2>文字起こし設定</h2>
      <p>利用可能なモデルから選択してください。料金は音声1時間あたりの目安です。</p>
    </div>
    <div class="provider-picker">
      <Select
        value={provider}
        options={providerOptions}
        onValueChange={selectProvider}
        class="w-full max-w-[420px]"
        disabled={busy || usableProviders.length === 0}
        ariaLabel="文字起こしモデル"
      />
      {#if usableProviders.length === 0}
        <div class="provider-unavailable">
          <p>利用できるモデルがありません。設定画面でモデルを準備してください。</p>
          <Button size="sm" variant="outline" type="button" onclick={onOpenSettings}>設定を開く</Button>
        </div>
      {/if}
    </div>
    {#if selectedAudio && providerDefinition?.pricingUsdPerHour != null && estimatedCostUsd != null}
      <p class="selected-cost">この音声の推定料金: <strong>{formatEstimatedCost(estimatedCostUsd)}</strong></p>
    {/if}
    <div class="action-row provider-action" data-transcription-action>
      <p class="action-help">
        {selectedAudio ? "選択したモデルで文字起こしを開始します。" : "音声を選択すると文字起こしを開始できます。"}
      </p>
      <Button size="lg" type="button" onclick={onTranscribe} disabled={!canTranscribe} loading={transcribing}>
        {transcriptionStatus}
      </Button>
    </div>
  </div>
</section>

<style>
  .creation-panel { padding: 0 0 36px; }
  .source-actions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  :global(.source-action) { min-height: 96px; justify-content: flex-start; gap: 14px; padding: 17px 18px; text-align: left; white-space: normal; }
  :global(.source-action-primary) { box-shadow: 0 8px 22px color-mix(in oklch, var(--primary) 22%, transparent); }
  .source-action-icon { display: grid; width: 40px; height: 40px; flex: none; place-items: center; border-radius: 11px; background: color-mix(in oklch, currentColor 13%, transparent); }
  .source-action-icon :global(svg) { width: 21px; height: 21px; }
  .source-action-copy { display: grid; min-width: 0; gap: 4px; }
  .source-action-copy strong { overflow: hidden; font-size: 0.91rem; text-overflow: ellipsis; white-space: nowrap; }
  .source-action-copy small { opacity: 0.72; font-size: 0.72rem; font-weight: 500; }
  :global(.source-action.selected-audio) { border-color: color-mix(in oklch, var(--primary) 35%, var(--border)); background: color-mix(in oklch, var(--primary) 4%, var(--background)); }
  .source-panel-heading { display: flex; align-items: center; justify-content: flex-end; gap: 14px; margin-bottom: 14px; }
  .transcription-settings-heading p { margin: 6px 0 0; color: var(--muted-foreground); font-size: 0.8rem; }
  .provider-picker { display: grid; gap: 10px; }
  .provider-unavailable { display: flex; align-items: center; justify-content: space-between; gap: 12px; color: var(--muted-foreground); font-size: 0.78rem; }
  .provider-unavailable p { margin: 0; }
  .selected-cost { margin: 12px 0 0; color: var(--muted-foreground); font-size: 0.8rem; }
  .selected-cost strong { color: var(--foreground); }

  @media (max-width: 600px) {
    .source-actions { grid-template-columns: 1fr; }
    :global(.source-action) { min-height: 82px; }
  }

</style>
