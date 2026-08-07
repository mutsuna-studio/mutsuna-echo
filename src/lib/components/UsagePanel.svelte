<script lang="ts">
  import { formatOptionalDuration, formatResetDate } from "../format";
  import type { TranscriptionUsage } from "../types/transcript";

  interface Props {
    usage: TranscriptionUsage | null;
    loading: boolean;
    error: string;
    onRefresh: () => void;
  }

  let { usage, loading, error, onRefresh }: Props = $props();
</script>

<section class="card usage-card" aria-busy={loading}>
  <div class="section-heading">
    <div>
      <p class="step">Usage</p>
      <h2>ElevenLabs 利用状況</h2>
    </div>
    <button class="refresh" type="button" onclick={onRefresh} disabled={loading}>
      {loading ? "更新中…" : "更新"}
    </button>
  </div>

  {#if loading && !usage}
    <p class="usage-placeholder" role="status">契約枠と使用量を確認しています…</p>
  {:else}
    <div class="usage-grid">
      <div>
        <span>今月利用可能</span>
        <strong>{formatOptionalDuration(usage?.availableDurationMs)}</strong>
      </div>
      <div>
        <span>今月使用済み（Scribe換算）</span>
        <strong>{formatOptionalDuration(usage?.usedDurationMs)}</strong>
      </div>
    </div>
  {/if}

  {#if usage?.tier || usage?.resetsAtUnix}
    <p class="usage-meta">
      {usage.tier ? `${usage.tier}プラン` : ""}
      {usage.tier && usage.resetsAtUnix ? " · " : ""}
      {usage.resetsAtUnix ? `${formatResetDate(usage.resetsAtUnix)}にリセット` : ""}
    </p>
  {/if}
  {#if usage?.warning}
    <p class="usage-warning" role="alert">{usage.warning}</p>
  {/if}
  {#if error}
    <p class="usage-warning" role="alert">{error}</p>
  {/if}
  <p class="usage-note">
    時間は契約枠と製品別クレジット使用量をScribe v2の公開枠で換算した値です。他のElevenLabs機能や追加機能を使うと実際の音声時間と異なる場合があります。
  </p>
</section>
