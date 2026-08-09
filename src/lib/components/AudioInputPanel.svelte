<script lang="ts">
  import { Button } from "@mutsuna/ui/button";
  import { Select } from "@mutsuna/ui/select";
  import { Tabs, TabsContent, TabsList, TabsTrigger } from "@mutsuna/ui/tabs";
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
    onProviderChange: (provider: TranscriptionProviderId) => void;
    onRecordedAudio: (audio: SelectedAudioFile) => void;
    onRecordingBusyChange: (busy: boolean) => void;
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
    onProviderChange,
    onRecordedAudio,
    onRecordingBusyChange,
    onMessage,
    onError
  }: Props = $props();

  let inputMode = $state<"file" | "record">("file");
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
</script>

<section class="creation-panel" aria-busy={selecting || transcribing}>
  <div class="section-heading">
    <div>
      <h2>音声を用意</h2>
      <p class="section-description">ファイルを読み込むか、この端末で録音します。</p>
    </div>
  </div>

  <Tabs bind:value={inputMode}>
    <TabsList class="input-tabs" aria-label="音声の入力方法">
      <TabsTrigger value="file" disabled={recordingBusy}>ファイルを選択</TabsTrigger>
      <TabsTrigger value="record" disabled={recordingBusy}>このアプリで録音</TabsTrigger>
    </TabsList>

  <TabsContent value="file">
    <Button class={selectedAudio ? "file-picker file-picker-selected" : "file-picker"} variant="outline" size="lg" type="button" onclick={onSelect} disabled={busy}>
      <span class="file-icon" aria-hidden="true">♪</span>
      <span class="file-copy">
        <strong>{selecting ? "ファイルを確認中…" : selectedAudio?.name ?? "音声ファイルを選択"}</strong>
        <small>
          {selectedAudio
            ? `${formatTimestamp(selectedAudio.durationMs)} · ${formatFileSize(selectedAudio.sizeBytes)} · クリックして変更`
            : "MP3・M4A・WAV・FLAC"}
        </small>
      </span>
    </Button>
  </TabsContent>
  <TabsContent value="record">
    <RecordingPanel
      disabled={recordingDisabled}
      onAudioReady={onRecordedAudio}
      onBusyChange={onRecordingBusyChange}
      {onMessage}
      {onError}
    />
  </TabsContent>
  </Tabs>

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
        searchable
        class="w-full max-w-[420px]"
        disabled={busy || usableProviders.length === 0}
        ariaLabel="文字起こしモデル"
      />
      {#if usableProviders.length === 0}
        <p class="provider-unavailable">利用できるモデルがありません。設定画面でモデルを準備してください。</p>
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
  .section-description { margin: 7px 0 0; color: var(--muted-foreground); font-size: 0.82rem; }
  .transcription-settings-heading p { margin: 6px 0 0; color: var(--muted-foreground); font-size: 0.8rem; }
  .provider-picker { display: grid; gap: 10px; }
  .provider-unavailable { margin: 0; color: var(--muted-foreground); font-size: 0.78rem; }
  .selected-cost { margin: 12px 0 0; color: var(--muted-foreground); font-size: 0.8rem; }
  .selected-cost strong { color: var(--foreground); }

</style>
