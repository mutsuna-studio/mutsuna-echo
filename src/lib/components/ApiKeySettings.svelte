<script lang="ts">
  import { Badge } from "@mutsuna/ui/badge";
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
  import { Card } from "@mutsuna/ui/card";
  import { Input } from "@mutsuna/ui/input";
  import { Label } from "@mutsuna/ui/label";
  import type { TranscriptionProviderDefinition } from "../providers";

  interface Props {
    provider: TranscriptionProviderDefinition;
    loading: boolean;
    saving: boolean;
    deleting: boolean;
    hasApiKey: boolean;
    busy: boolean;
    onSave: (apiKey: string) => Promise<void>;
    onDelete: () => void;
  }

  let { provider, loading, saving, deleting, hasApiKey, busy, onSave, onDelete }: Props = $props();
  let apiKey = $state("");
  let deleteDialogOpen = $state(false);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const submittedApiKey = apiKey.trim();
    apiKey = "";
    await onSave(submittedApiKey);
  }
</script>

<Card class="card settings-card" aria-busy={loading || saving || deleting}>
  <div class="credential-heading">
    <div>
      <h3>{provider.label} APIキー</h3>
      <p class="help">キーはOSの暗号化機能で保護し、画面へ読み戻しません。</p>
    </div>
    <Badge variant="secondary">{provider.modelLabel}</Badge>
    {#if loading}
      <Badge variant="secondary">確認中</Badge>
    {:else if hasApiKey}
      <Badge>設定済み</Badge>
    {:else}
      <Badge variant="secondary">未設定</Badge>
    {/if}
  </div>

  <form onsubmit={submit}>
    <Label for={`${provider.id}-api-key`}>API key</Label>
    <div class="input-row">
      <Input
        id={`${provider.id}-api-key`}
        type="password"
        placeholder={hasApiKey ? "新しいキーに置き換える" : "APIキーを入力"}
        autocomplete="off"
        spellcheck="false"
        bind:value={apiKey}
        disabled={busy}
      />
      <Button variant="secondary" size="lg" type="submit" disabled={busy || !apiKey.trim()} loading={saving}>
        {saving ? "確認中…" : hasApiKey ? "更新" : "保存"}
      </Button>
    </div>
  </form>

  {#if provider.id === "elevenlabs"}
    <p class="security-note">
      Speech to Text、User Read、Workspace Analytics Full Readだけを許可し、利用上限を設定してください。
    </p>
  {:else}
    <p class="security-note">
      Soniox Consoleで文字起こしに必要な範囲だけを許可し、利用上限を設定してください。
    </p>
  {/if}

  {#if hasApiKey}
    <Button class="danger" variant="link" type="button" onclick={() => deleteDialogOpen = true} disabled={busy} loading={deleting}>
      {deleting ? "削除中…" : "保存済みキーを削除"}
    </Button>
  {/if}
</Card>

<AlertDialog bind:open={deleteDialogOpen}>
  <AlertDialogContent>
    <AlertDialogHeader>
      <AlertDialogTitle>保存済みAPIキーを削除しますか？</AlertDialogTitle>
      <AlertDialogDescription>
        {provider.label}のAPIキーをこの端末から削除します。再度文字起こしを行うには、APIキーの入力が必要です。
      </AlertDialogDescription>
    </AlertDialogHeader>
    <AlertDialogFooter>
      <AlertDialogCancel>キャンセル</AlertDialogCancel>
      <AlertDialogAction variant="destructive" onclick={onDelete}>削除</AlertDialogAction>
    </AlertDialogFooter>
  </AlertDialogContent>
</AlertDialog>
