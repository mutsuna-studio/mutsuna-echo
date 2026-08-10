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
      <h2>ElevenLabsで使える時間</h2>
    </div>
    <Button variant="outline" size="sm" type="button" onclick={onRefresh} disabled={loading} loading={loading}>
      {loading ? "更新中…" : "更新"}
    </Button>
  </div>

  {#if loading && !usage}
    <p class="usage-placeholder" role="status">今月使える時間を確認しています…</p>
  {:else}
    <div class="usage-grid">
      <div>
        <span>今月あと使える時間</span>
        <strong>{formatOptionalDuration(usage?.availableDurationMs)}</strong>
      </div>
      <div>
        <span>今月使った時間の目安</span>
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
    表示時間はElevenLabsの利用状況から計算した目安です。ほかのElevenLabs機能も使っている場合は、実際に文字起こしできる時間と異なることがあります。
  </p>
</Card>
