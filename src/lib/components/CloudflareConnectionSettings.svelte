<script lang="ts">
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import { Button } from "@mutsuna/ui/button";
  import { Input } from "@mutsuna/ui/input";
  import type { CloudflareConnectionStatus } from "../providers";

  interface Props {
    status: CloudflareConnectionStatus | null;
    loading: boolean;
    connecting: boolean;
    saving: boolean;
    deleting: boolean;
    busy: boolean;
    onConnect: () => Promise<void>;
    onSelectAccount: (accountId: string) => Promise<void>;
    onDisconnectOAuth: () => Promise<void>;
    onSaveManual: (apiToken: string, accountId: string) => Promise<boolean>;
    onDeleteManual: () => Promise<void>;
  }

  let { status, loading, connecting, saving, deleting, busy, onConnect, onSelectAccount, onDisconnectOAuth, onSaveManual, onDeleteManual }: Props = $props();
  let apiToken = $state("");
  let accountId = $state("");

  async function saveManual(event: SubmitEvent) {
    event.preventDefault();
    if (await onSaveManual(apiToken.trim(), accountId.trim())) {
      apiToken = "";
      accountId = "";
    }
  }
</script>

<div class="cloudflare-settings" aria-busy={loading || connecting || saving || deleting}>
  <div class="heading">
    <div>
      <strong>Cloudflare Free</strong>
      <p>Cloudflare Workers AIの無料枠を利用できます。</p>
    </div>
    {#if status?.connected}
      <span class="connected"><CircleCheck aria-hidden="true" />接続済み</span>
    {:else}
      <span class="muted">未接続</span>
    {/if}
  </div>

  {#if status?.authMethod === "oauth"}
    <div class="connection-card">
      <div><small>認証方式</small><b>Cloudflare OAuth</b></div>
      {#if status.accountName}<div><small>アカウント</small><b>{status.accountName}</b></div>{/if}
      <Button variant="outline" type="button" disabled={busy} onclick={() => void onDisconnectOAuth()}>接続解除</Button>
    </div>
  {:else if status?.accountSelectionRequired}
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
      {#if status?.oauthConfigured === false}
        <small class="warning">このビルドにはCloudflare OAuth Client IDが設定されていません。</small>
      {:else}
        <small>APIキーの作成は不要です。音声はMutsunaのサーバーを経由しません。</small>
      {/if}
    </div>
  {/if}

  <ul>
    <li>文字起こし</li>
    <li>AI会議ノート</li>
    <li>Cloudflareの無料枠を利用</li>
  </ul>

  <details>
    <summary>詳細設定: APIトークンを使用</summary>
    <p>従来方式です。Workers AI Read権限を持つAPIトークンとAccount IDを安全な端末ストアへ保存します。</p>
    {#if status?.authMethod === "oauth" && status.legacyConfigured}
      <p class="warning">手動トークンも保存されています。OAuthを解除すると手動トークンが有効になります。</p>
    {/if}
    <form onsubmit={saveManual}>
      <Input aria-label="Cloudflare Account ID" placeholder="Account ID" autocomplete="off" spellcheck="false" bind:value={accountId} disabled={busy} />
      <Input aria-label="Cloudflare API token" type="password" placeholder="APIトークン" autocomplete="off" spellcheck="false" bind:value={apiToken} disabled={busy} />
      <Button variant="secondary" type="submit" disabled={busy || !accountId.trim() || !apiToken.trim()} loading={saving}>保存</Button>
      {#if status?.legacyConfigured}
        <Button variant="outline" type="button" disabled={busy} loading={deleting} onclick={() => void onDeleteManual()}>手動設定を削除</Button>
      {/if}
    </form>
  </details>
</div>

<style>
  .cloudflare-settings { display: grid; gap: 12px; padding: 16px; border-bottom: 1px solid var(--border); }
  .heading, .connection-card { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .heading strong { font-size: .9rem; }
  p, small, li { color: var(--muted-foreground); font-size: .72rem; line-height: 1.55; }
  p { margin: 3px 0; }
  .connected, .muted { display: inline-flex; align-items: center; gap: 5px; font-size: .7rem; font-weight: 600; }
  .connected { color: var(--primary); }
  .connected :global(svg) { width: 14px; }
  .muted { color: var(--muted-foreground); }
  .oauth-action, .account-selection { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .connection-card { padding: 12px; border: 1px solid var(--border); border-radius: 8px; }
  .connection-card div { display: grid; gap: 2px; }
  .connection-card b { font-size: .76rem; }
  ul { display: flex; flex-wrap: wrap; gap: 6px 24px; margin: 0; padding-left: 18px; }
  details { border-top: 1px solid var(--border); padding-top: 10px; }
  summary { cursor: pointer; font-size: .74rem; font-weight: 600; }
  form { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto auto; gap: 8px; margin-top: 10px; }
  .warning { color: var(--destructive); }
  @media (max-width: 680px) {
    .heading, .connection-card { align-items: stretch; flex-direction: column; }
    form { grid-template-columns: 1fr; }
  }
</style>
