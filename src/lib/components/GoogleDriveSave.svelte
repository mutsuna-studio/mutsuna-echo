<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { audioSelectedByDefault, waitSeconds, LARGE_AUDIO_BYTES, type DriveLimits } from "../googleDriveSave";
  import { Button } from "@mutsuna/ui/button";
  import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@mutsuna/ui/dialog";
  let { meetingId, audioAvailable, documentAvailable, disabled = false }: {
    meetingId: string; audioAvailable: boolean; documentAvailable: boolean; disabled?: boolean;
  } = $props();
  type Saved = { exists: boolean; version: string | null; url: string | null; completedAt: string | null; audioSize: number | null; unchanged: { audio: boolean; document: boolean; backup: boolean }; limits: DriveLimits };
  let limitStatus = $state<DriveLimits>({ retryAt: 0, quotaPaused: false });
  let currentTime = $state(Date.now());
  const remaining = $derived(waitSeconds(limitStatus, currentTime));
  const waitMessage = $derived(remaining > 0 ? `${limitStatus.quotaPaused ? "Googleの利用制限により送信を一時停止中です。" : "次の保存まで"}あと${remaining}秒` : "");
  $effect(() => {
    let active = true;
    let polling = false;
    const refresh = async () => {
      if (polling) return;
      polling = true;
      try { const next = await invoke<DriveLimits>("google_drive_limits"); if (active) limitStatus = next; }
      catch { /* Native save still enforces the limit if polling is unavailable. */ }
      finally { polling = false; }
    };
    void refresh();
    const timer = setInterval(() => { currentTime = Date.now(); void refresh(); }, 1000);
    return () => { active = false; clearInterval(timer); };
  });
  let open = $state(false);
  let busy = $state(false);
  let checking = $state(false);
  let includeAudio = $state(true);
  let includeDocument = $state(true);
  let includeBackup = $state(true);
  let saved = $state<Saved | null>(null);
  let error = $state("");
  let message = $state("");
  let percent = $state(0);
  const empty = $derived(!includeAudio && !includeDocument && !includeBackup);
  $effect(() => {
    const id = meetingId;
    let disposed = false;
    const unlisten = listen<{ meetingId: string; stage: string; percent: number }>("google-drive-progress", ({ payload }) => {
      if (!disposed && payload.meetingId === id) { message = payload.stage; percent = payload.percent; }
    });
    return () => { disposed = true; void unlisten.then(fn => fn()); };
  });
  async function prepare() {
    checking = true; error = ""; message = ""; saved = null;
    includeAudio = false; includeDocument = documentAvailable;
    try {
      const connection = await invoke<{ connected: boolean }>("google_drive_connection");
      if (!connection.connected) { error = "設定 → 一般 → Google Driveでアカウントを連携してください。"; return; }
      saved = await invoke<Saved>("google_drive_saved_state", { meetingId });
      includeAudio = audioSelectedByDefault(audioAvailable, saved.audioSize, saved.unchanged.audio);
      includeDocument = documentAvailable && !saved.unchanged.document;
      includeBackup = !saved.unchanged.backup;
      limitStatus = saved.limits;
      open = true;
    } catch (e) { error = String(e); }
    finally { checking = false; }
  }
  async function save(mode: "create" | "overwrite" | "newVersion") {
    const id = meetingId;
    busy = true; error = ""; percent = 0;
    try {
      saved = await invoke<Saved>("google_drive_save", { request: { meetingId: id, mode, expectedVersion: saved?.version ?? null, includeAudio, selectedAudioSize: includeAudio ? saved?.audioSize ?? null : null, includeDocument, includeBackup } });
      limitStatus = saved.limits; currentTime = Date.now();
      message = "Google Driveに保存しました。"; open = false;
    } catch (e) {
      error = String(e);
      // Failed uploads may have created files; retry must use the saved version.
      try { saved = await invoke<Saved>("google_drive_saved_state", { meetingId: id }); } catch { /* Keep original error. */ }
    } finally {
      if (saved) limitStatus = saved.limits;
      try { limitStatus = await invoke<DriveLimits>("google_drive_limits"); } catch { /* Keep saved status. */ }
      currentTime = Date.now(); busy = false;
    }
  }
  async function openDrive() {
    try { await invoke("google_drive_open", { meetingId }); } catch (e) { error = String(e); }
  }
</script>

<div class="drive-save">
  <Button variant="outline" size="sm" disabled={disabled || busy || checking || remaining > 0} onclick={prepare}>{checking ? "確認中…" : busy ? "Driveへ保存中…" : "Driveへ保存"}</Button>
  {#if !open && waitMessage}<span role="status">{waitMessage}</span>{/if}
  {#if saved?.url}<Button variant="ghost" size="sm" onclick={openDrive}>Driveで開く</Button>{/if}
  {#if !open && message}<span role="status">{message}</span>{/if}
  {#if !open && error}<p role="alert">{error}</p>{/if}
</div>
<Dialog bind:open>
  <DialogContent onInteractOutside={(event) => { if (busy) event.preventDefault(); }} onEscapeKeydown={(event) => { if (busy) event.preventDefault(); }}>
    <DialogHeader>
      <DialogTitle>{saved?.exists ? "Google Driveへ再保存" : "Google Driveへ保存"}</DialogTitle>
      <DialogDescription>「Mutsuna Echo」フォルダー内に会議ごとのフォルダーを作成します。会議ノートと文字起こしは1つのGoogleドキュメントの別タブにまとめます。</DialogDescription>
    </DialogHeader>
    <div class="options">
      <label><input type="checkbox" bind:checked={includeAudio} disabled={busy || !audioAvailable} />録音した音声{#if saved?.unchanged.audio}<small>前回保存から変更なし</small>{/if}</label>
      {#if saved?.audioSize != null}
        <small>音声容量：{(saved.audioSize / 1_000_000).toLocaleString("ja-JP", { maximumFractionDigits: 1 })} MB{saved.audioSize >= LARGE_AUDIO_BYTES ? "。500MB以上のため、音声は初期選択していません。必要な場合はチェックしてください。" : "。保存済みの音声と内容が同じ場合は送信を省略します。"}</small>
      {/if}
      <label><input type="checkbox" bind:checked={includeDocument} disabled={busy || !documentAvailable} />会議ノート・文字起こし（1つのドキュメント）{#if saved?.unchanged.document}<small>前回保存から変更なし</small>{/if}</label>
      <label><input type="checkbox" bind:checked={includeBackup} disabled={busy} />復元用データ（JSON）{#if saved?.unchanged.backup}<small>前回保存から変更なし</small>{/if}</label>
      {#if saved?.exists}
        <p class="warning">上書きすると、選択したファイルとドキュメント内の「会議ノート」「文字起こし」タブの本文がアプリの保存済み内容に置き換わります。Drive上での編集はアプリに取り込まれません。編集内容を残す場合は「別版として保存」を選んでください。</p>
      {/if}
      <small>変更のない項目は初期選択していません。再保存や別版保存する場合は、必要な項目にチェックしてください。変更の判定はアプリ内の内容を対象とし、Drive上の編集・削除は確認しません。</small>
      <small>アプリに保存済みの内容を書き出します。保存対象に含めなかったファイルは削除されません。ドキュメントの2タブは「ページ分けなし」で保存します。</small>
      {#if waitMessage}<p role="status">{waitMessage}</p>{/if}
      {#if busy}<p role="status">{message}</p><progress max="100" value={percent} aria-label="Driveへの保存進捗"></progress>{/if}
      {#if error}<p role="alert">{error}</p>{/if}
    </div>
    <DialogFooter>
      <Button variant="ghost" disabled={busy} onclick={() => open = false}>キャンセル</Button>
      {#if saved?.exists}
        <Button variant="outline" disabled={busy || empty || remaining > 0} onclick={() => save("newVersion")}>別版として保存</Button>
        <Button disabled={busy || empty || remaining > 0} onclick={() => save("overwrite")}>上書き保存</Button>
      {:else}
        <Button disabled={busy || empty || remaining > 0} onclick={() => save("create")}>保存</Button>
      {/if}
    </DialogFooter>
  </DialogContent>
</Dialog>
<style>
  .drive-save { display: flex; flex-wrap: wrap; align-items: center; gap: 0.5rem; padding: 0.5rem 0; }
  .options { display: grid; gap: 1rem; padding: 1rem 0; }
  label { display: flex; align-items: center; gap: 0.5rem; }
  p { margin: 0; line-height: 1.7; }
  small, span { color: var(--muted-foreground); }
  .warning { padding: 0.75rem; border: 1px solid var(--border); border-radius: 0.5rem; }
  [role="alert"] { color: var(--destructive); }
  progress { width: 100%; }
</style>
