<script lang="ts">
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { formatFileSize, formatRecordedAt } from "../format";
  import { transcriptionProviderLabel } from "../providers";
  import type { RecordedAudioSummary } from "../types/recording";

  interface Props {
    recordings: RecordedAudioSummary[];
    disabled: boolean;
    busy: boolean;
    onRefresh: () => void;
    onSelect: (recording: RecordedAudioSummary) => void;
    onReveal: (recording: RecordedAudioSummary) => void;
  }

  let { recordings, disabled, busy, onRefresh, onSelect, onReveal }: Props = $props();
</script>

<section class="history" aria-label="過去の録音">
  <div class="history-heading">
    <div>
      <strong>過去の録音</strong>
      <small>Music/Mutsuna Echoに保存された最新100件</small>
    </div>
    <Button variant="outline" size="sm" type="button" onclick={onRefresh} disabled={busy}>更新</Button>
  </div>
  {#if recordings.length > 0}
    <div class="history-list">
      {#each recordings as recording (recording.id)}
        <div class="history-item">
          <Button
            class="history-select"
            variant="ghost"
            type="button"
            onclick={() => onSelect(recording)}
            disabled={busy || disabled}
          >
            <span>
              <strong>{recording.fileName}</strong>
              <small>
                {formatRecordedAt(recording.recordedAtUnixMs)} · {formatFileSize(recording.sizeBytes)}
                {#each recording.transcriptProviders as provider}
                  <Badge variant="secondary">{transcriptionProviderLabel(provider)} 済み</Badge>
                {/each}
              </small>
            </span>
            <span class="history-action">選択</span>
          </Button>
          <Button
            class="history-reveal"
            variant="outline"
            size="sm"
            type="button"
            onclick={() => onReveal(recording)}
            disabled={busy}
            aria-label={`${recording.fileName}の保存場所を開く`}
          >場所を開く</Button>
        </div>
      {/each}
    </div>
  {:else}
    <p class="history-empty">保存済みの録音はまだありません。</p>
  {/if}
</section>

<style>
  .history { display: grid; gap: 10px; margin-top: 18px; }
  .history-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .history-heading > div { display: grid; gap: 3px; }
  .history-heading small,
  .history-empty { color: #68746c; font-size: 0.78rem; }
  .history-list { display: grid; overflow: hidden; border: 1px solid #dce3de; border-radius: 12px; }
  .history-item { display: flex; align-items: center; gap: 8px; padding: 6px; border-top: 1px solid var(--border); }
  .history-item:first-child { border-top: 0; }
  .history-item :global(.history-select) { min-width: 0; height: auto; flex: 1; justify-content: space-between; padding: 8px; text-align: left; }
  .history-item :global(.history-select > span:first-child) { display: grid; min-width: 0; gap: 3px; }
  .history-item :global(.history-select strong) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .history-item :global(.history-select small) { color: var(--muted-foreground); }
  .history-item :global(.history-reveal) { flex: none; }
  .history-action { flex: none; color: #23704a; font-size: 0.78rem; font-weight: 750; }
  .history-empty { margin: 0; padding: 14px; border-radius: 10px; background: #f5f7f5; }

  @media (max-width: 600px) {
    .history-action { display: none; }
  }
</style>
