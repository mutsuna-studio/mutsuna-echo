<script lang="ts">
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import { Button } from "@mutsuna/ui/button";
  import {
    MUTSUNA_CLOUD_PRICING_URL,
    describeMutsunaCloudStatus,
    mutsunaCloudAccountStatusLabel
  } from "../mutsunaCloud";
  import type { MutsunaCloudStatus } from "../providers";

  interface Props {
    status: MutsunaCloudStatus | null;
    loading: boolean;
    connecting: boolean;
    verificationCode: string | null;
    cancelling: boolean;
    disconnecting: boolean;
    purchasing: boolean;
    busy: boolean;
    onConnect: () => Promise<void>;
    onReopenVerification: () => Promise<void>;
    onCancelConnection: () => Promise<void>;
    onDisconnect: () => Promise<void>;
    onPurchase: () => Promise<void>;
  }

  let {
    status,
    loading,
    connecting,
    verificationCode,
    cancelling,
    disconnecting,
    purchasing,
    busy,
    onConnect,
    onReopenVerification,
    onCancelConnection,
    onDisconnect,
    onPurchase
  }: Props = $props();

  const availability = $derived(describeMutsunaCloudStatus(status, loading));

  const accountStatusLabel = $derived.by(() => {
    if (!status?.connected || status.accountStatus === null) return null;
    return mutsunaCloudAccountStatusLabel(status.accountStatus);
  });
</script>

<div class="cloud-settings" aria-busy={loading || connecting || disconnecting || purchasing}>
  <div class="heading">
    <div class="heading-copy">
      <strong>Mutsuna Cloud</strong>
      <p>APIキー不要・クレジット制のクラウドAI文字起こしです。</p>
    </div>
    <span class:ready={availability.tone === "ready"} class:warning={availability.tone === "warning"} class="status">
      {#if availability.tone === "ready"}
        <CircleCheck aria-hidden="true" />
      {:else if availability.tone === "warning"}
        <CircleAlert aria-hidden="true" />
      {/if}
      {availability.label}
    </span>
  </div>

  <p class="detail">{availability.detail}</p>

  {#if status?.connected}
    <div class="account-card">
      <div>
        <small>接続状態</small>
        <b>接続済み</b>
      </div>
      <div>
        <small>クレジット残高</small>
        <b>{status.availableCredits ?? "確認できません"}{status.availableCredits !== null ? " クレジット" : ""}</b>
      </div>
      {#if accountStatusLabel}
        <div>
          <small>アカウント状態</small>
          <b>{accountStatusLabel}</b>
        </div>
      {/if}
      <div class="account-actions">
        <Button variant="outline" type="button" disabled={busy} loading={disconnecting} onclick={() => void onDisconnect()}>
          {disconnecting ? "切断中…" : "切断"}
        </Button>
      </div>
    </div>
  {:else}
    <div class="connect-action">
      <Button type="button" disabled={busy} loading={connecting} onclick={() => void onConnect()}>
        {connecting ? "ブラウザで認証中…" : "Mutsuna Cloudに接続"}
      </Button>
      {#if connecting}
        <div class="verification" role="status" aria-live="polite">
          {#if verificationCode}
            <small>ブラウザに表示されたコードと一致することを確認してください</small>
            <strong aria-label={`照合コード ${verificationCode}`}>{verificationCode}</strong>
            <small>一致する場合だけ、ブラウザでログインして「この端末を承認」を押します。</small>
            <div class="verification-actions">
              <Button variant="outline" type="button" disabled={cancelling} onclick={() => void onReopenVerification()}>
                ブラウザをもう一度開く
              </Button>
              <Button variant="outline" type="button" disabled={cancelling} loading={cancelling} onclick={() => void onCancelConnection()}>
                {cancelling ? "キャンセル中…" : "接続をキャンセル"}
              </Button>
            </div>
          {:else}
            <small>安全な照合コードを準備しています…</small>
          {/if}
        </div>
      {:else}
        <small>接続すると、利用可能なクレジット残高をここで確認できます。</small>
      {/if}
    </div>
  {/if}

  <div class="purchase-action">
    <Button type="button" disabled={!status?.connected || busy} loading={purchasing} onclick={() => void onPurchase()}>
      {purchasing ? "購入画面を開いています…" : "60分パック（3,600クレジット）を購入"}
    </Button>
    {#if !status?.connected}
      <small>クレジットを購入するには、先にMutsuna Cloudへ接続してください。</small>
    {/if}
  </div>

  <a class="pricing-link" href={MUTSUNA_CLOUD_PRICING_URL} target="_blank" rel="noreferrer">
    料金・クレジットを確認 <ExternalLink aria-hidden="true" />
  </a>
</div>

<style>
  .cloud-settings { display: grid; gap: 12px; padding: 16px; border-bottom: 1px solid var(--border); }
  .heading, .account-card { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .heading-copy { display: grid; min-width: 0; gap: 3px; }
  .heading strong { font-size: .9rem; }
  p, small { color: var(--muted-foreground); font-size: .72rem; line-height: 1.55; }
  p { margin: 0; }
  .status { display: inline-flex; flex: none; align-items: center; gap: 5px; color: var(--muted-foreground); font-size: .7rem; font-weight: 600; }
  .status.ready { color: var(--primary); }
  .status.warning { color: var(--destructive); }
  .status :global(svg) { width: 14px; height: 14px; }
  .detail { margin-top: -6px; }
  .account-card { padding: 12px; border: 1px solid var(--border); border-radius: 8px; background: color-mix(in oklch, var(--muted) 24%, transparent); }
  .account-card div { display: grid; gap: 2px; }
  .account-card b { font-size: .78rem; }
  .account-card .account-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; }
  .connect-action { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; }
  .verification { display: grid; width: 100%; gap: 6px; padding: 12px; border: 1px solid var(--border); border-radius: 8px; background: color-mix(in oklch, var(--muted) 24%, transparent); }
  .verification strong { font: 700 1.35rem/1 ui-monospace, monospace; letter-spacing: .12em; }
  .verification-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 4px; }
  .purchase-action { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; }
  .pricing-link { display: inline-flex; width: fit-content; align-items: center; gap: 5px; color: var(--primary); font-size: .72rem; font-weight: 600; text-decoration: none; }
  .pricing-link:hover { text-decoration: underline; }
  .pricing-link :global(svg) { width: 13px; height: 13px; }
  @media (max-width: 680px) {
    .heading, .account-card { align-items: stretch; flex-direction: column; }
    .status { align-self: flex-start; }
    .connect-action, .purchase-action { align-items: stretch; flex-direction: column; }
    .account-card .account-actions { width: 100%; justify-content: stretch; }
    .connect-action :global([data-slot="button"]), .purchase-action :global([data-slot="button"]), .account-actions :global([data-slot="button"]) { min-height: 44px; flex: 1; }
  }
</style>
