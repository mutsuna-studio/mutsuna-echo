<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let apiKey = $state("");
  let hasApiKey = $state(false);
  let loading = $state(true);
  let saving = $state(false);
  let deleting = $state(false);
  let message = $state("");
  let errorMessage = $state("");

  function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    return "予期しないエラーが発生しました。";
  }

  async function refreshStatus() {
    hasApiKey = await invoke<boolean>("has_api_key");
  }

  onMount(async () => {
    try {
      await refreshStatus();
    } catch (error) {
      errorMessage = errorText(error);
    } finally {
      loading = false;
    }
  });

  async function saveApiKey(event: SubmitEvent) {
    event.preventDefault();
    saving = true;
    message = "";
    errorMessage = "";

    try {
      const modelsAccessible = await invoke<boolean>("save_api_key", { apiKey });
      apiKey = "";
      hasApiKey = true;
      message = modelsAccessible
        ? "APIキーを確認し、安全に保存しました。"
        : "制限付きAPIキーとして保存しました。Speech to Text権限は文字起こし時に確認します。";
    } catch (error) {
      errorMessage = errorText(error);
    } finally {
      saving = false;
    }
  }

  async function deleteApiKey() {
    if (!window.confirm("保存済みのElevenLabs APIキーを削除しますか？")) return;

    deleting = true;
    message = "";
    errorMessage = "";

    try {
      await invoke("delete_api_key");
      hasApiKey = false;
      apiKey = "";
      message = "APIキーを削除しました。";
    } catch (error) {
      errorMessage = errorText(error);
    } finally {
      deleting = false;
    }
  }
</script>

<main class="shell">
  <header>
    <p class="eyebrow">Mutsuna Echo</p>
    <h1>ElevenLabsの設定</h1>
    <p class="lead">文字起こしに使用するAPIキーを登録してください。</p>
  </header>

  <section class="card" aria-busy={loading || saving || deleting}>
    <div class="status-row">
      <div>
        <h2>APIキー</h2>
        <p class="help">キーはOSの資格情報ストアに保存され、画面へ読み戻しません。</p>
      </div>
      {#if loading}
        <span class="badge neutral">確認中</span>
      {:else if hasApiKey}
        <span class="badge saved">設定済み</span>
      {:else}
        <span class="badge neutral">未設定</span>
      {/if}
    </div>

    <form onsubmit={saveApiKey}>
      <label for="api-key">ElevenLabs API key</label>
      <div class="input-row">
        <input
          id="api-key"
          type="password"
          placeholder={hasApiKey ? "新しいキーに置き換える" : "sk_..."}
          autocomplete="off"
          spellcheck="false"
          bind:value={apiKey}
          disabled={loading || saving || deleting}
        />
        <button class="primary" type="submit" disabled={loading || saving || deleting || !apiKey.trim()}>
          {saving ? "確認中…" : hasApiKey ? "更新" : "保存"}
        </button>
      </div>
    </form>

    {#if message}
      <p class="notice success" role="status">{message}</p>
    {/if}
    {#if errorMessage}
      <p class="notice error" role="alert">{errorMessage}</p>
    {/if}

    {#if hasApiKey}
      <div class="danger-zone">
        <button class="danger" type="button" onclick={deleteApiKey} disabled={saving || deleting}>
          {deleting ? "削除中…" : "保存済みキーを削除"}
        </button>
      </div>
    {/if}
  </section>
</main>

<style>
  :global(:root) {
    font-family: Inter, "Noto Sans JP", system-ui, sans-serif;
    color: #17211b;
    background: #f2f5f1;
    font-synthesis: none;
  }

  :global(body) {
    margin: 0;
    min-width: 320px;
    min-height: 100vh;
  }

  :global(button),
  :global(input) {
    font: inherit;
  }

  .shell {
    width: min(680px, calc(100% - 40px));
    margin: 0 auto;
    padding: 64px 0;
  }

  header {
    margin-bottom: 28px;
  }

  .eyebrow {
    margin: 0 0 8px;
    color: #23704a;
    font-size: 0.78rem;
    font-weight: 800;
    letter-spacing: 0.16em;
    text-transform: uppercase;
  }

  h1,
  h2,
  p {
    margin-top: 0;
  }

  h1 {
    margin-bottom: 10px;
    font-size: clamp(2rem, 6vw, 3rem);
    letter-spacing: -0.04em;
  }

  h2 {
    margin-bottom: 5px;
    font-size: 1.1rem;
  }

  .lead,
  .help {
    color: #647068;
  }

  .lead {
    margin-bottom: 0;
  }

  .help {
    margin-bottom: 0;
    font-size: 0.88rem;
  }

  .card {
    padding: 28px;
    border: 1px solid #d8dfda;
    border-radius: 18px;
    background: #fff;
    box-shadow: 0 16px 42px rgb(29 54 39 / 8%);
  }

  .status-row,
  .input-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .status-row {
    justify-content: space-between;
    margin-bottom: 28px;
  }

  .badge {
    flex: none;
    padding: 6px 10px;
    border-radius: 999px;
    font-size: 0.78rem;
    font-weight: 700;
  }

  .badge.saved {
    color: #176440;
    background: #e2f5e9;
  }

  .badge.neutral {
    color: #667068;
    background: #edf0ed;
  }

  label {
    display: block;
    margin-bottom: 8px;
    font-size: 0.86rem;
    font-weight: 700;
  }

  input {
    box-sizing: border-box;
    min-width: 0;
    flex: 1;
    height: 46px;
    padding: 0 14px;
    border: 1px solid #bdc8c0;
    border-radius: 10px;
    color: #17211b;
    background: #fbfcfb;
    outline: none;
  }

  input:focus {
    border-color: #2c8058;
    box-shadow: 0 0 0 3px rgb(44 128 88 / 14%);
  }

  button {
    height: 46px;
    padding: 0 18px;
    border-radius: 10px;
    cursor: pointer;
    font-weight: 750;
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .primary {
    border: 1px solid #246b49;
    color: #fff;
    background: #246b49;
  }

  .notice {
    margin: 18px 0 0;
    padding: 11px 13px;
    border-radius: 9px;
    font-size: 0.9rem;
  }

  .notice.success {
    color: #175f3e;
    background: #e7f5ec;
  }

  .notice.error {
    color: #9a3028;
    background: #fff0ee;
  }

  .danger-zone {
    margin-top: 24px;
    padding-top: 20px;
    border-top: 1px solid #e5e9e6;
  }

  .danger {
    height: auto;
    padding: 0;
    border: 0;
    color: #a33a31;
    background: transparent;
    font-size: 0.86rem;
  }

  @media (max-width: 560px) {
    .shell {
      width: min(100% - 28px, 680px);
      padding: 36px 0;
    }

    .card {
      padding: 20px;
    }

    .input-row {
      align-items: stretch;
      flex-direction: column;
    }
  }
</style>
