<script lang="ts">
  import { Alert, AlertDescription } from "@mutsuna/ui/alert";
  import { Button } from "@mutsuna/ui/button";
  import { Card } from "@mutsuna/ui/card";
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

<Card class="card usage-card" aria-busy={loading}>
  <div class="section-heading">
    <div>
      <p class="step">Usage</p>
      <h2>ElevenLabs 利用状況</h2>
    </div>
    <Button variant="outline" size="sm" type="button" onclick={onRefresh} disabled={loading} loading={loading}>
      {loading ? "更新中…" : "更新"}
    </Button>
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
    <Alert class="usage-warning" role="alert"><AlertDescription>{usage.warning}</AlertDescription></Alert>
  {/if}
  {#if error}
    <Alert class="usage-warning" variant="destructive" role="alert"><AlertDescription>{error}</AlertDescription></Alert>
  {/if}
  <p class="usage-note">
    時間は契約枠と製品別クレジット使用量をScribe v2の公開枠で換算した値です。他のElevenLabs機能や追加機能を使うと実際の音声時間と異なる場合があります。
  </p>
</Card>
