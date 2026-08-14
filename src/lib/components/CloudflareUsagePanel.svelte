<script lang="ts">
  import { Button } from "@mutsuna/ui/button";
  import { Card } from "@mutsuna/ui/card";
  import { Alert, AlertDescription } from "@mutsuna/ui/alert";
  import { formatActualCost, formatOptionalDuration } from "../format";
  import type { CloudflareUsage } from "../types/transcript";

  interface Props {
    usage: CloudflareUsage | null;
    loading: boolean;
    error: string;
    onRefresh: () => void;
  }

  let { usage, loading, error, onRefresh }: Props = $props();

  function formatNeurons(value: number | undefined): string {
    return value === undefined ? "未取得" : Math.round(value).toLocaleString("ja-JP");
  }

  function formatReset(value: string | undefined): string {
    if (!value) return "未取得";
    return new Intl.DateTimeFormat("ja-JP", {
      month: "numeric",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    }).format(new Date(value));
  }
</script>

<Card class="card usage-card" aria-busy={loading}>
  <div class="section-heading">
    <div><h2>Cloudflare Workers AI</h2></div>
    <Button variant="outline" size="sm" type="button" onclick={onRefresh} disabled={loading} loading={loading}>
      {loading ? "更新中…" : "更新"}
    </Button>
  </div>

  {#if loading && !usage}
    <p class="usage-placeholder" role="status">利用状況を集計しています…</p>
  {:else}
    <h3 class="usage-subheading">本日の無償利用枠（UTC基準）</h3>
    <div class="usage-grid">
      <div>
        <span>使用量</span>
        <strong>{usage ? `${formatNeurons(usage.dailyEstimatedNeurons)} / ${formatNeurons(usage.dailyFreeAllocationNeurons)}` : "未取得"}</strong>
      </div>
      <div>
        <span>使用率</span>
        <strong>{usage ? `${usage.dailyUsagePercent.toFixed(1)}%` : "未取得"}</strong>
      </div>
      <div>
        <span>推定残量</span>
        <strong>{formatNeurons(usage?.dailyRemainingNeurons)}</strong>
      </div>
      <div>
        <span>次回リセット</span>
        <strong>{formatReset(usage?.dailyResetsAt)}</strong>
      </div>
    </div>
    {#if usage}
      <progress class="usage-progress" max="100" value={Math.min(usage.dailyUsagePercent, 100)} aria-label={`本日の無償利用枠を${usage.dailyUsagePercent.toFixed(1)}%使用`}></progress>
      <p class="usage-meta">本日：文字起こし {formatOptionalDuration(usage.dailyUsedDurationMs)}（{usage.dailyTranscriptionCount.toLocaleString("ja-JP")}回）・テキスト生成 {usage.dailyTextGenerationCount.toLocaleString("ja-JP")}回</p>
    {/if}

    <h3 class="usage-subheading monthly">今月の利用状況</h3>
    <div class="usage-grid">
      <div>
        <span>利用額（無償利用枠適用前）</span>
        <strong>{usage ? formatActualCost(usage.estimatedCostUsd) : "未取得"}</strong>
      </div>
      <div>
        <span>文字起こし時間</span>
        <strong>{formatOptionalDuration(usage?.usedDurationMs)}</strong>
      </div>
      <div>
        <span>推定Neurons</span>
        <strong>{formatNeurons(usage?.estimatedNeurons)}</strong>
      </div>
      <div>
        <span>実行回数</span>
        <strong>{usage ? `${(usage.transcriptionCount + usage.textGenerationCount).toLocaleString("ja-JP")}回` : "未取得"}</strong>
      </div>
    </div>
    {#if usage}
      <p class="usage-meta">文字起こし {usage.transcriptionCount.toLocaleString("ja-JP")}回・テキスト生成 {usage.textGenerationCount.toLocaleString("ja-JP")}回</p>
    {/if}
  {/if}

  {#if error}
    <Alert class="usage-warning" variant="destructive" role="alert"><AlertDescription>{error}</AlertDescription></Alert>
  {/if}
  <p class="usage-note">
    このアプリの文字起こし履歴とテキスト生成履歴を、利用モデルの公式単価で集計した推定値です。無償利用枠はCloudflareアカウント全体で1日10,000 Neuronsです。ほかのWorkerやアプリの利用分は含まれないため、実際の残量とは異なる場合があります。
  </p>
</Card>
