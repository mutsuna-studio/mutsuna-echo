<script lang="ts">
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

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const submittedApiKey = apiKey.trim();
    apiKey = "";
    await onSave(submittedApiKey);
  }
</script>

<section class="card settings-card" aria-busy={loading || saving || deleting}>
  <div class="section-heading">
    <div>
      <p class="step">Settings</p>
      <h2>ElevenLabs APIキー</h2>
      <p class="help">キーはOSの暗号化機能で保護し、画面へ読み戻しません。</p>
    </div>
    {#if loading}
      <span class="badge">確認中</span>
    {:else if hasApiKey}
      <span class="badge ready">設定済み</span>
    {:else}
      <span class="badge">未設定</span>
    {/if}
  </div>

  <form onsubmit={submit}>
    <label for="api-key">API key</label>
    <div class="input-row">
      <input
        id="api-key"
        type="password"
        placeholder={hasApiKey ? "新しいキーに置き換える" : "sk_..."}
        autocomplete="off"
        spellcheck="false"
        bind:value={apiKey}
        disabled={busy}
      />
      <button class="secondary" type="submit" disabled={busy || !apiKey.trim()}>
        {saving ? "確認中…" : hasApiKey ? "更新" : "保存"}
      </button>
    </div>
  </form>

  <p class="security-note">
    ElevenLabsではSpeech to Text、User Read、Workspace Analytics Full Readだけを許可し、利用上限を設定してください。
  </p>

  {#if hasApiKey}
    <button class="danger" type="button" onclick={onDelete} disabled={busy}>
      {deleting ? "削除中…" : "保存済みキーを削除"}
    </button>
  {/if}
</section>
