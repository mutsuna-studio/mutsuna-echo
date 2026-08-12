<script lang="ts">
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import {
    AlertDialog,
    AlertDialogAction,
    AlertDialogCancel,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle
  } from "@mutsuna/ui/alert-dialog";
  import { Button } from "@mutsuna/ui/button";
  import { Input } from "@mutsuna/ui/input";
  import type { TranscriptionProviderDefinition } from "../providers";

  interface Props {
    provider: TranscriptionProviderDefinition;
    loading: boolean;
    saving: boolean;
    deleting: boolean;
    hasApiKey: boolean;
    busy: boolean;
    onSave: (apiKey: string, accountId?: string) => Promise<boolean>;
    onDelete: () => void;
  }

  let { provider, loading, saving, deleting, hasApiKey, busy, onSave, onDelete }: Props = $props();
  let apiKey = $state("");
  let deleteDialogOpen = $state(false);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const submittedApiKey = apiKey.trim();
    if (await onSave(submittedApiKey)) {
      apiKey = "";
    }
  }
</script>

<div class="cloud-model-row" aria-busy={loading || saving || deleting}>
  <div class="cloud-model-copy">
    <div class="cloud-model-title">
      <strong>{provider.label}</strong>
      {#if loading}
        <span class="cloud-status">確認中</span>
      {:else if hasApiKey}
        <span class="cloud-status ready"><CircleCheck aria-hidden="true" />接続済み</span>
      {:else}
        <span class="cloud-status">未接続</span>
      {/if}
    </div>
    <small>音声を{provider.label}へ送って文字起こしします。</small>
    {#if provider.id === "elevenlabs"}
      <small>APIキーには文字起こし・ユーザー情報・利用状況の権限が必要です。</small>
    {:else}
      <small>APIキーには文字起こし権限が必要です。</small>
    {/if}
  </div>

  <div class="cloud-model-actions">
    <form onsubmit={submit}>
      <Input
        id={`${provider.id}-api-key`}
        aria-label={`${provider.label} API key`}
        type="password"
        placeholder={hasApiKey ? "新しいAPIキーに変更" : "APIキーを入力"}
        autocomplete="off"
        spellcheck="false"
        bind:value={apiKey}
        disabled={busy}
      />
      <Button variant="secondary" type="submit" disabled={busy || !apiKey.trim()} loading={saving}>
        {saving ? "保存中…" : hasApiKey ? "更新" : "保存"}
      </Button>
    </form>
    {#if hasApiKey}
      <Button variant="outline" type="button" onclick={() => deleteDialogOpen = true} disabled={busy} loading={deleting}>
        {deleting ? "削除中…" : "削除"}
      </Button>
    {/if}
  </div>
</div>

<AlertDialog bind:open={deleteDialogOpen}>
  <AlertDialogContent>
    <AlertDialogHeader>
      <AlertDialogTitle>{provider.label}の接続を解除しますか？</AlertDialogTitle>
      <AlertDialogDescription>
        APIキーをこの端末から削除します。これまでの文字起こしは残ります。
      </AlertDialogDescription>
    </AlertDialogHeader>
    <AlertDialogFooter>
      <AlertDialogCancel>キャンセル</AlertDialogCancel>
      <AlertDialogAction variant="destructive" onclick={onDelete}>削除</AlertDialogAction>
    </AlertDialogFooter>
  </AlertDialogContent>
</AlertDialog>

<style>
  .cloud-model-row { display: flex; min-height: 64px; align-items: center; justify-content: space-between; gap: 24px; padding: 10px 14px; border-bottom: 1px solid var(--border); }
  .cloud-model-copy { display: grid; min-width: 0; gap: 5px; }
  .cloud-model-title { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
  .cloud-model-title strong { font-size: 0.8rem; }
  .cloud-status { display: inline-flex; align-items: center; gap: 4px; color: var(--muted-foreground); font-size: 0.66rem; font-weight: 600; }
  .cloud-status.ready { color: var(--primary); }
  .cloud-status :global(svg) { width: 13px; height: 13px; }
  small { color: var(--muted-foreground); font-size: 0.7rem; line-height: 1.5; }
  .cloud-model-actions { display: flex; min-width: 0; flex: none; align-items: center; gap: 8px; }
  form { display: grid; min-width: 0; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 8px; }
  form :global([data-slot="input"]) { width: 230px; min-width: 0; }
  @media (max-width: 680px) {
    .cloud-model-row { align-items: stretch; flex-direction: column; }
    .cloud-model-actions, form { width: 100%; }
    form { flex: 1; grid-template-columns: 1fr; }
    form :global([data-slot="input"]) { width: 100%; flex: 1; }
    .cloud-model-actions :global([data-slot="button"]), form :global([data-slot="input"]) { min-height: 44px; }
  }
  @media (max-width: 440px) {
    .cloud-model-actions { align-items: stretch; flex-direction: column; }
    form { grid-template-columns: 1fr; }
    form :global([data-slot="button"]) { width: 100%; }
    .cloud-model-actions :global([data-slot="button"]) { width: 100%; }
  }
</style>
