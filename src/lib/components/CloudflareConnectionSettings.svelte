<script lang="ts">
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Unplug from "@lucide/svelte/icons/unplug";
  import { Button } from "@mutsuna/ui/button";
  import type { CloudflareConnectionStatus } from "../providers";

  interface Props {
    status: CloudflareConnectionStatus | null;
    loading: boolean;
    connecting: boolean;
    deleting: boolean;
    busy: boolean;
    onConnect: () => Promise<void>;
    onCancelConnect: () => Promise<void>;
    onSelectAccount: (accountId: string) => Promise<void>;
    onDisconnectOAuth: () => Promise<void>;
  }

  let { status, loading, connecting, deleting, busy, onConnect, onCancelConnect, onSelectAccount, onDisconnectOAuth }: Props = $props();
</script>

<div class="cloudflare-settings" aria-busy={loading || connecting || deleting}>
  {#if status?.connected}
    <div class="connected-row">
      <div class="connected-summary">
      <strong>Cloudflare Workers AI</strong>
        <span class="connected"><CircleCheck aria-hidden="true" />接続済み</span>
        {#if status.accountName}<span class="account-name">{status.accountName}</span>{/if}
      </div>
      <Button
        size="icon-sm"
        variant="ghost"
        type="button"
        icon={Unplug}
        aria-label="Cloudflareの接続を解除"
        title="接続解除"
        disabled={busy}
        loading={deleting}
        onclick={() => void onDisconnectOAuth()}
      />
    </div>
  {:else}
    <div class="heading">
      <div>
        <strong>Cloudflare Workers AI</strong>
        <p>CloudflareアカウントのWorkers AIを文字起こしと会議ノートで利用します。</p>
      </div>
      {#if status?.accountSelectionRequired}
        <span class="connected"><CircleCheck aria-hidden="true" />OAuth認証済み</span>
      {:else}
        <span class="muted">未接続</span>
      {/if}
    </div>
    {#if status?.accountSelectionRequired}
      <div class="account-selection">
        <p>Workers AIを使うアカウントを選択してください。</p>
        {#each status.accounts as account (account.id)}
          <Button variant="secondary" type="button" disabled={busy} onclick={() => void onSelectAccount(account.id)}>{account.name}</Button>
        {/each}
      </div>
    {:else}
      <div class="oauth-action">
        <Button type="button" disabled={busy || status?.oauthConfigured === false} loading={connecting} onclick={() => void onConnect()}>
          {connecting ? "Cloudflareで認証中…" : "Cloudflareに接続"}
        </Button>
        {#if connecting}
          <Button variant="secondary" type="button" onclick={() => void onCancelConnect()}>キャンセル</Button>
        {/if}
        {#if status?.oauthConfigured === false}
          <small class="warning">このビルドにはCloudflare OAuth Client IDが設定されていません。</small>
        {:else}
          <small>APIキーの作成は不要です。音声はMutsunaのサーバーを経由しません。</small>
        {/if}
      </div>
    {/if}
  {/if}

</div>

<style>
  .cloudflare-settings { display: grid; gap: 12px; padding: 16px; border-bottom: 1px solid var(--border); }
  .heading, .connected-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .heading strong, .connected-summary strong { font-size: .9rem; }
  .connected-summary { display: flex; min-width: 0; align-items: center; gap: 8px; }
  .account-name { min-width: 0; overflow: hidden; color: var(--muted-foreground); font-size: .72rem; text-overflow: ellipsis; white-space: nowrap; }
  p, small { color: var(--muted-foreground); font-size: .72rem; line-height: 1.55; }
  p { margin: 3px 0; }
  .connected, .muted { display: inline-flex; align-items: center; gap: 5px; font-size: .7rem; font-weight: 600; }
  .connected { color: var(--primary); }
  .connected :global(svg) { width: 14px; }
  .muted { color: var(--muted-foreground); }
  .oauth-action, .account-selection { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .warning { color: var(--destructive); }
  @media (max-width: 680px) {
    .heading { align-items: stretch; flex-direction: column; }
    .connected-summary { flex-wrap: wrap; }
    .account-name { flex-basis: 100%; }
  }
</style>
