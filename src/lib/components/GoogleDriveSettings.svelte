<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Button } from "@mutsuna/ui/button";
  let connection = $state<{ connected: boolean; configured: boolean; email: string | null } | null>(null);
  let busy = $state(false);
  let error = $state("");
  async function refresh() {
    try { connection = await invoke("google_drive_connection"); }
    catch (e) { error = String(e); }
  }
  $effect(() => { void refresh(); });
  async function connect() {
    busy = true; error = "";
    try { connection = await invoke("google_drive_connect"); }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }
  async function disconnect() {
    busy = true; error = "";
    try { await invoke("google_drive_disconnect"); await refresh(); }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }
</script>

<section class="drive-settings" aria-label="Google Drive連携">
  <div><h3>Google Drive</h3><p>録音と、会議ノート・文字起こしをまとめたドキュメントを保存します。</p></div>
  {#if connection?.connected}
    <p>{connection.email} と連携済み</p>
    <Button variant="outline" disabled={busy} onclick={disconnect}>連携を解除</Button>
    <small>解除後もDriveに保存したファイルは残ります。Google側のアクセス許可はGoogleアカウント設定で削除できます。</small>
  {:else}
    <Button variant="outline" disabled={busy || !connection?.configured} onclick={connect}>{busy ? "Googleの認証を待っています…" : "Googleアカウントと連携"}</Button>
    {#if connection && !connection.configured}<p>このビルドではGoogle連携が未設定です。</p>{/if}
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>
<style>
  .drive-settings { display: grid; gap: 0.75rem; padding: 1rem 0; }
  h3, p { margin: 0; }
  p, small { color: var(--muted-foreground); line-height: 1.6; }
  [role="alert"] { color: var(--destructive); }
</style>
