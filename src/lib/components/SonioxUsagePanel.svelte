<script lang="ts">
  import { Alert, AlertDescription } from "@mutsuna/ui/alert";
  import { Button } from "@mutsuna/ui/button";
  import { Card } from "@mutsuna/ui/card";
  import { formatActualCost } from "../format";
  import type { SonioxUsage } from "../types/transcript";

  interface Props {
    usage: SonioxUsage | null;
    loading: boolean;
    error: string;
    onRefresh: () => void;
  }

  let { usage, loading, error, onRefresh }: Props = $props();

  function fetchedAt(value: string): string {
    const date = new Date(value);
    return Number.isNaN(date.getTime())
      ? value
      : new Intl.DateTimeFormat("ja-JP", {
          month: "numeric",
          day: "numeric",
          hour: "2-digit",
          minute: "2-digit"
        }).format(date);
  }
</script>

<Card class="card usage-card" aria-busy={loading}>
  <div class="section-heading">
    <div><h2>Sonioxの利用状況</h2></div>
    <Button variant="outline" size="sm" type="button" onclick={onRefresh} disabled={loading} loading={loading}>
      {loading ? "更新中…" : "更新"}
    </Button>
  </div>

  {#if loading && !usage}
    <p class="usage-placeholder" role="status">今月の使用料を確認しています…</p>
  {:else}
    <div class="usage-grid">
      <div>
        <span>今月の使用料</span>
        <strong>{usage ? formatActualCost(usage.monthlyCostUsd) : "未取得"}</strong>
      </div>
      <div>
        <span>チャージ残高</span>
        <strong class="console-value">Consoleで確認</strong>
      </div>
    </div>
  {/if}

  {#if usage}
    <p class="usage-meta">{fetchedAt(usage.fetchedAt)}に取得した値を保存しています。</p>
  {/if}
  {#if error}
    <Alert class="usage-warning" variant="destructive" role="alert"><AlertDescription>{error}</AlertDescription></Alert>
  {/if}
  <p class="usage-note">
    使用料はSonioxプロジェクト全体の当月分です。残高を取得する公開APIはないため、Soniox Consoleで確認してください。更新ボタンを押したときだけAPIへ接続します。
  </p>
</Card>

<style>
  .console-value { font-size: 0.95rem !important; }
</style>
