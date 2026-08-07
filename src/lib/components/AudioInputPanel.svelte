<script lang="ts">
  import RecordingPanel from "./RecordingPanel.svelte";
  import { formatEstimatedCost, formatFileSize, formatTimestamp } from "../format";
  import type { SelectedAudioFile } from "../types/transcript";

  interface Props {
    selectedAudio: SelectedAudioFile | null;
    selecting: boolean;
    transcribing: boolean;
    recordingBusy: boolean;
    busy: boolean;
    recordingDisabled: boolean;
    hasApiKey: boolean;
    canTranscribe: boolean;
    onSelect: () => void;
    onTranscribe: () => void;
    onRecordedAudio: (audio: SelectedAudioFile) => void;
    onRecordingBusyChange: (busy: boolean) => void;
    onMessage: (message: string) => void;
    onError: (message: string) => void;
  }

  let {
    selectedAudio,
    selecting,
    transcribing,
    recordingBusy,
    busy,
    recordingDisabled,
    hasApiKey,
    canTranscribe,
    onSelect,
    onTranscribe,
    onRecordedAudio,
    onRecordingBusyChange,
    onMessage,
    onError
  }: Props = $props();

  let inputMode = $state<"file" | "record">("file");
</script>

<section class="card transcription-card" aria-busy={selecting || transcribing}>
  <div class="section-heading">
    <div>
      <p class="step">Step 1</p>
      <h2>音声を用意</h2>
    </div>
    <span class:ready={Boolean(selectedAudio)} class="badge">
      {recordingBusy ? "録音中" : selectedAudio ? "準備済み" : "未選択"}
    </span>
  </div>

  <div class="input-tabs" role="tablist" aria-label="音声の入力方法">
    <button class:active={inputMode === "file"} type="button" role="tab" aria-selected={inputMode === "file"} onclick={() => inputMode = "file"} disabled={recordingBusy}>ファイルを選択</button>
    <button class:active={inputMode === "record"} type="button" role="tab" aria-selected={inputMode === "record"} onclick={() => inputMode = "record"} disabled={recordingBusy}>このアプリで録音</button>
  </div>

  {#if inputMode === "file"}
    <button class="file-picker" type="button" onclick={onSelect} disabled={busy}>
      <span class="file-icon" aria-hidden="true">♪</span>
      <span class="file-copy">
        <strong>{selecting ? "ファイルを確認中…" : selectedAudio?.name ?? "音声ファイルを選択"}</strong>
        <small>
          {selectedAudio
            ? `${formatTimestamp(selectedAudio.durationMs)} · ${formatFileSize(selectedAudio.sizeBytes)} · クリックして変更`
            : "MP3・M4A・WAV・FLAC"}
        </small>
      </span>
    </button>
  {:else}
    <RecordingPanel
      disabled={recordingDisabled}
      onAudioReady={onRecordedAudio}
      onBusyChange={onRecordingBusyChange}
      {onMessage}
      {onError}
    />
  {/if}

  {#if selectedAudio}
    <div class="cost-estimate">
      <div>
        <span>推定コスト</span>
        <strong>{formatEstimatedCost(selectedAudio.estimatedCostUsd)}</strong>
      </div>
      <small>
        公開単価 ${selectedAudio.pricingRateUsdPerHour.toFixed(2)}/時間
        （{selectedAudio.pricingVerifiedOn}確認）に基づく概算です。プラン内枠や請求時の丸めにより実際の請求額とは異なる場合があります。
      </small>
    </div>
  {/if}

  <div class="action-row">
    <div>
      <p class="step">Step 2</p>
      <p class="action-help">
        {hasApiKey ? "日本語・話者分離・単語タイムスタンプ" : "先にAPIキーを設定してください"}
      </p>
    </div>
    <button class="primary" type="button" onclick={onTranscribe} disabled={!canTranscribe}>
      {transcribing ? "文字起こし中…" : "文字起こし開始"}
    </button>
  </div>
</section>
