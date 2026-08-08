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

  interface Props {
    loading: boolean;
    saving: boolean;
    deleting: boolean;
    hasApiKey: boolean;
    busy: boolean;
    onSave: (apiKey: string) => Promise<void>;
    onDelete: () => void;
  }

  let { loading, saving, deleting, hasApiKey, busy, onSave, onDelete }: Props = $props();
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
  <div class="section-heading">
    <div>
      <p class="step">Settings</p>
      <h2>プロバイダー設定</h2>
      <p class="help">文字起こしサービスごとに認証情報を管理します。</p>
    </div>
    <Badge variant="secondary">ElevenLabs</Badge>
  </div>

  <div class="credential-heading">
    <div>
      <h3>ElevenLabs APIキー</h3>
      <p class="help">キーはOSの暗号化機能で保護し、画面へ読み戻しません。</p>
    </div>
    {#if loading}
      <Badge variant="secondary">確認中</Badge>
    {:else if hasApiKey}
      <Badge>設定済み</Badge>
    {:else}
      <Badge variant="secondary">未設定</Badge>
    {/if}
  </div>

  <form onsubmit={submit}>
    <Label for="api-key">API key</Label>
    <div class="input-row">
      <Input
        id="api-key"
        type="password"
        placeholder={hasApiKey ? "新しいキーに置き換える" : "sk_..."}
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

  <p class="security-note">
    ElevenLabsではSpeech to Text、User Read、Workspace Analytics Full Readだけを許可し、利用上限を設定してください。
  </p>

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
        ElevenLabsのAPIキーをこの端末から削除します。再度文字起こしを行うには、APIキーの入力が必要です。
      </AlertDialogDescription>
    </AlertDialogHeader>
    <AlertDialogFooter>
      <AlertDialogCancel>キャンセル</AlertDialogCancel>
      <AlertDialogAction variant="destructive" onclick={onDelete}>削除</AlertDialogAction>
    </AlertDialogFooter>
  </AlertDialogContent>
</AlertDialog>
