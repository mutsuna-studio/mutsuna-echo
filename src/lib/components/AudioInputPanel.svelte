<script lang="ts">
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { Card } from "@mutsuna/ui/card";
  import { Label } from "@mutsuna/ui/label";
  import { Select } from "@mutsuna/ui/select";
  import { Tabs, TabsContent, TabsList, TabsTrigger } from "@mutsuna/ui/tabs";
  import RecordingPanel from "./RecordingPanel.svelte";
  import { formatEstimatedCost, formatFileSize, formatTimestamp } from "../format";
  import {
    getTranscriptionProvider,
    isTranscriptionProviderId,
    transcriptionProviderOptions,
    type TranscriptionProviderId
  } from "../providers";
  import type { SelectedAudioFile } from "../types/transcript";

  interface Props {
    selectedAudio: SelectedAudioFile | null;
    provider: TranscriptionProviderId;
    selecting: boolean;
    transcribing: boolean;
    recordingBusy: boolean;
    transcriptRevision: number;
    busy: boolean;
    recordingDisabled: boolean;
    providerConfigured: boolean;
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
    provider,
    selecting,
    transcribing,
    recordingBusy,
    transcriptRevision,
    busy,
    recordingDisabled,
    providerConfigured,
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
  const providerDefinition = $derived(getTranscriptionProvider(provider));

  function selectProvider(value: string) {
    if (isTranscriptionProviderId(value)) onProviderChange(value);
  }
</script>

<Card class="card transcription-card" aria-busy={selecting || transcribing}>
  <div class="section-heading">
    <div>
      <p class="step">Step 1</p>
      <h2>音声を用意</h2>
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
      {transcriptRevision}
      onAudioReady={onRecordedAudio}
      onBusyChange={onRecordingBusyChange}
      {onMessage}
      {onError}
    />
  </TabsContent>
  </Tabs>

  {#if selectedAudio}
    <div class="cost-estimate">
      <div>
        <span>{providerDefinition.label} {providerDefinition.modelLabel} の推定コスト</span>
        <strong>{formatEstimatedCost(selectedAudio.estimatedCostUsd)}</strong>
      </div>
      <small>
        {providerDefinition.pricingLabel} ${selectedAudio.pricingRateUsdPerHour.toFixed(2)}/時間
        （{selectedAudio.pricingVerifiedOn}確認）に基づく概算です。プラン内枠や請求時の丸めにより実際の請求額とは異なる場合があります。
      </small>
    </div>
  {/if}

  <div class="transcription-settings">
    <div class="transcription-settings-heading">
      <p class="step">Step 2</p>
      <h2>文字起こし設定</h2>
    </div>
    <div class="provider-grid">
      <div class="provider-field">
        <Label for="transcription-provider">プロバイダー</Label>
        <Select
          id="transcription-provider"
          value={provider}
          options={transcriptionProviderOptions}
          onValueChange={selectProvider}
          disabled={busy}
          ariaLabel="文字起こしプロバイダー"
        />
      </div>
      <div class="provider-summary">
        <span>モデル</span>
        <strong>{providerDefinition.modelLabel}</strong>
        <small>{providerDefinition.capabilitySummary}</small>
      </div>
    </div>
    <div class="action-row provider-action" data-transcription-action>
      <p class="action-help">
        {providerConfigured ? `${providerDefinition.label}で文字起こしします` : `${providerDefinition.label}のAPIキーを設定してください`}
      </p>
      <Button size="lg" type="button" onclick={onTranscribe} disabled={!canTranscribe} loading={transcribing}>
        {transcribing ? "文字起こし中…" : "文字起こし開始"}
      </Button>
    </div>
  </div>
</Card>
