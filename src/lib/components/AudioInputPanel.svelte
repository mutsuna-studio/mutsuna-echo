<script lang="ts">
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { Label } from "@mutsuna/ui/label";
  import { Select } from "@mutsuna/ui/select";
  import { Tabs, TabsContent, TabsList, TabsTrigger } from "@mutsuna/ui/tabs";
  import RecordingPanel from "./RecordingPanel.svelte";
  import LocalModelManager from "./LocalModelManager.svelte";
  import { formatEstimatedCost, formatFileSize, formatTimestamp } from "../format";
  import {
    getTranscriptionProvider,
    isTranscriptionProviderId,
    transcriptionProviderOptions,
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
    onProvidersChanged: () => Promise<void>;
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
    onProvidersChanged,
    onRecordedAudio,
    onRecordingBusyChange,
    onMessage,
    onError
  }: Props = $props();

  let inputMode = $state<"file" | "record">("file");
  const providerDefinition = $derived(getTranscriptionProvider(providers, provider));
  const providerOptions = $derived(transcriptionProviderOptions(providers));
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
    <Badge variant={selectedAudio ? "default" : "secondary"}>
      {recordingBusy ? "録音中" : selectedAudio ? "準備済み" : "未選択"}
    </Badge>
  </div>

  <Tabs bind:value={inputMode}>
    <TabsList class="input-tabs" aria-label="音声の入力方法">
      <TabsTrigger value="file" disabled={recordingBusy}>ファイルを選択</TabsTrigger>
      <TabsTrigger value="record" disabled={recordingBusy}>このアプリで録音</TabsTrigger>
    </TabsList>

  <TabsContent value="file">
    <Button class="file-picker" variant="outline" size="lg" type="button" onclick={onSelect} disabled={busy}>
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

  {#if selectedAudio && providerDefinition?.pricingUsdPerHour != null && estimatedCostUsd != null}
    <div class="cost-estimate">
      <div>
        <span>{providerDefinition.label} {providerDefinition.modelLabel} の推定コスト</span>
        <strong>{formatEstimatedCost(estimatedCostUsd)}</strong>
      </div>
      <small>
        公開時間単価 ${providerDefinition.pricingUsdPerHour.toFixed(2)}/時間
        （{providerDefinition.pricingVerifiedOn}確認）に基づく概算です。プラン内枠や請求時の丸めにより実際の請求額とは異なる場合があります。
      </small>
    </div>
  {:else if selectedAudio && providerDefinition?.kind === "local"}
    <div class="cost-estimate local-processing-note">
      <div>
        <span>ローカル処理</span>
        <strong>クラウドAPI利用料なし</strong>
      </div>
      <small>音声は端末内で処理され、文字起こしAPIへ送信されません。</small>
    </div>
  {/if}

  <div class="transcription-settings">
    <div class="transcription-settings-heading">
      <h2>文字起こし設定</h2>
    </div>
    <div class="provider-grid">
      <div class="provider-field">
        <Label for="transcription-provider">文字起こしモデル</Label>
        <Select
          id="transcription-provider"
          value={provider}
          options={providerOptions}
          onValueChange={selectProvider}
          searchable
          disabled={busy}
          ariaLabel="文字起こしモデル"
        />
      </div>
      <div class="provider-summary">
        <span>モデル</span>
        <strong>{providerDefinition?.modelLabel ?? "確認中…"}</strong>
        <small>{providerDefinition?.capabilitySummary ?? "プロバイダー情報を読み込んでいます"}</small>
      </div>
    </div>
    {#if providerDefinition?.kind === "local"}
      <LocalModelManager
        disabled={busy}
        onChanged={onProvidersChanged}
        {onMessage}
        {onError}
      />
    {/if}
    <div class="action-row provider-action" data-transcription-action>
      <p class="action-help">
        {providerDefinition?.statusMessage ?? "プロバイダー情報を確認しています。"}
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
</style>
