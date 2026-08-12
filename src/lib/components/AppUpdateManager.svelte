<script lang="ts">
  import Download from "@lucide/svelte/icons/download";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import { Alert, AlertDescription } from "@mutsuna/ui/alert";
  import { Button } from "@mutsuna/ui/button";
  import { scrollbarVisibility } from "@mutsuna/ui/scrollbar";
  import { getVersion } from "@tauri-apps/api/app";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
  import { onDestroy, onMount } from "svelte";
  import {
    checkAndroidUpdate,
    completeAndroidUpdate,
    getAndroidUpdateStatus,
    isAndroid,
    startAndroidUpdate,
    waitForAndroidUpdateCheck,
    type AndroidUpdateStatus
  } from "../androidUpdate";

  interface Props {
    disabled: boolean;
    onBeforeInstall: () => Promise<boolean>;
    onBusyChange: (busy: boolean) => void;
  }

  let { disabled, onBeforeInstall, onBusyChange }: Props = $props();
  let currentVersion = $state("—");
  let availableUpdate = $state.raw<Update | null>(null);
  let checking = $state(false);
  let installing = $state(false);
  let status = $state("更新情報を確認できます。");
  let error = $state("");
  let downloadedBytes = $state(0);
  let totalBytes = $state<number | null>(null);
  let androidUpdate = $state<AndroidUpdateStatus | null>(null);
  let androidPollTimer: number | null = null;
  const desktopSupported = !/Android|iPhone|iPad/i.test(navigator.userAgent);
  const progress = $derived(
    isAndroid && androidUpdate?.totalBytes
      ? Math.min(100, Math.round(androidUpdate.bytesDownloaded / androidUpdate.totalBytes * 100))
      : totalBytes && totalBytes > 0
        ? Math.min(100, Math.round(downloadedBytes / totalBytes * 100))
        : null
  );
  const androidBusy = $derived(androidUpdate?.phase === "starting" || androidUpdate?.phase === "installing");
  const displayError = $derived(isAndroid ? androidUpdate?.error ?? error : error);

  function androidStatusText(value: AndroidUpdateStatus | null): string {
    if (!value) return "Google Playで更新情報を確認します。";
    if (value.checking) return "更新情報を確認しています…";
    switch (value.phase) {
      case "available": return value.updatePriority >= 4 && value.immediateAllowed
        ? "重要な新しいバージョンがあります。"
        : "新しいバージョンがあります。";
      case "starting": return "Google Playの更新画面を開いています…";
      case "downloading": return "更新をダウンロードしています…";
      case "downloaded": return "更新の準備ができました。再起動すると適用されます。";
      case "installing": return "更新を適用しています…";
      case "latest": return "最新バージョンを使用しています。";
      case "failed": return "更新情報を確認できませんでした。";
      default: return "Google Playで更新情報を確認します。";
    }
  }

  function errorText(value: unknown): string {
    if (typeof value === "string") return value;
    if (value instanceof Error) return value.message;
    return "更新サーバーへ接続できませんでした。";
  }

  function formatBytes(value: number): string {
    if (value < 1024 * 1024) return `${Math.max(1, Math.round(value / 1024))} KB`;
    return `${(value / 1024 / 1024).toFixed(1)} MB`;
  }

  async function releaseUpdate() {
    const previous = availableUpdate;
    availableUpdate = null;
    if (previous) await previous.close().catch(() => undefined);
  }

  async function checkForUpdates() {
    if ((!desktopSupported && !isAndroid) || checking || installing) return;
    checking = true;
    error = "";
    status = "更新情報を確認しています…";
    try {
      if (isAndroid) {
        androidUpdate = await waitForAndroidUpdateCheck(await checkAndroidUpdate());
        return;
      }
      await releaseUpdate();
      const update = await check({ timeout: 15_000 });
      availableUpdate = update;
      status = update
        ? `バージョン ${update.version} を利用できます。`
        : "最新バージョンを使用しています。";
    } catch (cause) {
      error = errorText(cause);
      status = "更新情報を確認できませんでした。";
    } finally {
      checking = false;
    }
  }

  function trackDownload(event: DownloadEvent) {
    if (event.event === "Started") {
      downloadedBytes = 0;
      totalBytes = event.data.contentLength ?? null;
      status = "更新をダウンロードしています…";
    } else if (event.event === "Progress") {
      downloadedBytes += event.data.chunkLength;
    } else {
      status = "更新を適用しています…";
    }
  }

  async function installUpdate() {
    if (isAndroid) {
      if (installing || disabled || androidUpdate?.phase !== "available") return;
      if (!await onBeforeInstall()) return;
      installing = true;
      onBusyChange(true);
      try {
        androidUpdate = await startAndroidUpdate();
      } catch (cause) {
        error = errorText(cause);
      } finally {
        installing = false;
        onBusyChange(false);
      }
      return;
    }
    const update = availableUpdate;
    if (!update || installing || disabled) return;
    if (!await onBeforeInstall()) return;
    installing = true;
    onBusyChange(true);
    error = "";
    downloadedBytes = 0;
    totalBytes = null;
    try {
      await update.downloadAndInstall(trackDownload, { timeout: 600_000 });
      status = "更新が完了しました。アプリを再起動します…";
      await relaunch();
    } catch (cause) {
      error = errorText(cause);
      status = "更新を完了できませんでした。";
    } finally {
      installing = false;
      onBusyChange(false);
    }
  }

  async function completeMobileUpdate() {
    if (!isAndroid || disabled || androidUpdate?.phase !== "downloaded") return;
    if (!await onBeforeInstall()) return;
    installing = true;
    onBusyChange(true);
    try {
      androidUpdate = await completeAndroidUpdate();
    } catch (cause) {
      error = errorText(cause);
      installing = false;
      onBusyChange(false);
    }
  }

  async function refreshAndroidStatus() {
    try {
      androidUpdate = await getAndroidUpdateStatus();
      if (androidUpdate.phase !== "installing") {
        installing = false;
        onBusyChange(false);
      }
    } catch {
      // Play Store外の開発版では状態取得に失敗する場合がある。手動確認時だけエラーを表示する。
    }
  }

  onMount(() => {
    void getVersion().then((version) => currentVersion = version).catch(() => undefined);
    if (desktopSupported || isAndroid) void checkForUpdates();
    if (isAndroid) androidPollTimer = window.setInterval(() => void refreshAndroidStatus(), 1_000);
  });

  onDestroy(() => {
    onBusyChange(false);
    if (androidPollTimer !== null) window.clearInterval(androidPollTimer);
    const update = availableUpdate;
    availableUpdate = null;
    if (update) void update.close().catch(() => undefined);
  });
</script>

<div class="update-card" aria-busy={checking || installing || androidBusy}>
  <div class="update-main">
    <div class="update-heading">
      <div class="update-icon" aria-hidden="true"><ShieldCheck /></div>
      <div>
        <div class="update-title">
          <h3>アプリの更新</h3>
          <span>バージョン {currentVersion}</span>
        </div>
        <p>{isAndroid ? androidStatusText(androidUpdate) : desktopSupported ? status : "モバイル版の更新はアプリストアから行います。"}</p>
      </div>
    </div>
    {#if isAndroid}
      <div class="update-actions">
        {#if androidUpdate?.phase === "available"}
          <Button type="button" icon={Download} onclick={installUpdate} disabled={disabled || installing} loading={installing}>
            更新する
          </Button>
        {:else if androidUpdate?.phase === "downloaded"}
          <Button type="button" icon={RefreshCw} onclick={completeMobileUpdate} disabled={disabled || installing} loading={installing}>
            再起動して更新
          </Button>
        {/if}
        <Button variant="outline" type="button" icon={RefreshCw} onclick={checkForUpdates} disabled={disabled || checking || installing || androidBusy} loading={checking}>
          更新を確認
        </Button>
      </div>
    {:else if desktopSupported}
      <div class="update-actions">
        {#if availableUpdate}
          <Button type="button" icon={Download} onclick={installUpdate} disabled={disabled || installing} loading={installing}>
            v{availableUpdate.version}へ更新
          </Button>
        {/if}
        <Button variant="outline" type="button" icon={RefreshCw} onclick={checkForUpdates} disabled={disabled || checking || installing} loading={checking}>
          更新を確認
        </Button>
      </div>
    {/if}
  </div>

  {#if installing || androidUpdate?.phase === "downloading"}
    <div class="download-progress" aria-live="polite">
      <progress max="100" value={progress ?? undefined} aria-label="更新のダウンロード進捗"></progress>
      <span>
        {progress !== null ? `${progress}%` : "ダウンロード中"}
        {isAndroid && androidUpdate?.totalBytes
          ? ` · ${formatBytes(androidUpdate.bytesDownloaded)} / ${formatBytes(androidUpdate.totalBytes)}`
          : totalBytes ? ` · ${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)}` : ""}
      </span>
    </div>
  {/if}

  {#if availableUpdate?.body}
    <p
      class="release-notes mutsuna-scrollbar mutsuna-scrollbar--both-edges"
      use:scrollbarVisibility
    >{availableUpdate.body}</p>
  {/if}

  {#if displayError}
    <Alert variant="destructive" role="alert"><AlertDescription>{displayError}</AlertDescription></Alert>
  {/if}

</div>

<style>
  .update-card {
    display: grid;
    gap: 12px;
  }

  .update-main,
  .update-heading,
  .update-title,
  .update-actions {
    display: flex;
    align-items: center;
  }

  .update-main { min-height: 76px; justify-content: space-between; gap: 24px; padding: 14px 2px; }
  .update-heading { min-width: 0; gap: 12px; }
  .update-heading > div:last-child { min-width: 0; }
  .update-title { flex-wrap: wrap; gap: 8px; }
  .update-title h3 { margin: 0; font-size: 0.9rem; font-weight: 680; }
  .update-title span { color: var(--muted-foreground); font-size: 0.7rem; }
  .update-heading p { margin: 4px 0 0; color: var(--muted-foreground); font-size: 0.74rem; line-height: 1.55; }

  .update-icon {
    display: grid;
    width: 34px;
    height: 34px;
    flex: none;
    place-items: center;
    border-radius: 9px;
    color: var(--primary);
    background: color-mix(in oklch, var(--primary) 10%, var(--background));
  }

  .update-icon :global(svg) { width: 18px; height: 18px; stroke-width: 1.8; }
  .update-actions { flex-wrap: wrap; justify-content: flex-end; gap: 10px; }
  .update-actions :global(button) { min-height: 34px; font-size: 0.74rem; font-weight: 650; }

  .download-progress { display: grid; gap: 6px; }
  .download-progress progress { width: 100%; height: 7px; accent-color: var(--primary); }
  .download-progress span { color: var(--muted-foreground); font-size: 0.76rem; text-align: right; }

  .release-notes {
    max-height: 96px;
    margin: 0;
    padding: 10px 12px;
    overflow: auto;
    border-radius: 8px;
    color: var(--muted-foreground);
    background: var(--muted);
    font-size: 0.78rem;
    white-space: pre-wrap;
  }

  @media (max-width: 680px) {
    .update-main { align-items: stretch; flex-direction: column; gap: 12px; }
    .update-actions { justify-content: flex-start; }
  }
</style>
