<script lang="ts">
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Download from "@lucide/svelte/icons/download";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import { Alert, AlertDescription } from "@mutsuna/ui/alert";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { Card } from "@mutsuna/ui/card";
  import { getVersion } from "@tauri-apps/api/app";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
  import { onDestroy, onMount } from "svelte";

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
  const desktopSupported = !/Android|iPhone|iPad/i.test(navigator.userAgent);
  const progress = $derived(
    totalBytes && totalBytes > 0 ? Math.min(100, Math.round(downloadedBytes / totalBytes * 100)) : null
  );

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
    if (!desktopSupported || checking || installing) return;
    checking = true;
    error = "";
    status = "更新情報を確認しています…";
    try {
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

  onMount(() => {
    void getVersion().then((version) => currentVersion = version).catch(() => undefined);
    if (desktopSupported) void checkForUpdates();
  });

  onDestroy(() => {
    onBusyChange(false);
    const update = availableUpdate;
    availableUpdate = null;
    if (update) void update.close().catch(() => undefined);
  });
</script>

<Card class="update-card" aria-busy={checking || installing}>
  <div class="update-heading">
    <div class="update-icon" aria-hidden="true"><ShieldCheck /></div>
    <div>
      <div class="update-title">
        <h3>アプリの更新</h3>
        <Badge variant="secondary">v{currentVersion}</Badge>
      </div>
      <p>{desktopSupported ? status : "モバイル版の更新はアプリストアから行います。"}</p>
    </div>
  </div>

  {#if installing}
    <div class="download-progress" aria-live="polite">
      <progress max="100" value={progress ?? undefined} aria-label="更新のダウンロード進捗"></progress>
      <span>
        {progress !== null ? `${progress}%` : "ダウンロード中"}
        {totalBytes ? ` · ${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)}` : ""}
      </span>
    </div>
  {/if}

  {#if availableUpdate?.body}
    <p class="release-notes">{availableUpdate.body}</p>
  {/if}

  {#if error}
    <Alert variant="destructive" role="alert"><AlertDescription>{error}</AlertDescription></Alert>
  {/if}

  {#if desktopSupported}
    <div class="update-actions">
      {#if availableUpdate}
        <Button type="button" icon={Download} onclick={installUpdate} disabled={disabled || installing} loading={installing}>
          v{availableUpdate.version}へ更新
        </Button>
      {:else if !checking && !error}
        <span class="up-to-date"><CircleCheck aria-hidden="true" /> 最新です</span>
      {/if}
      <Button variant="outline" type="button" icon={RefreshCw} onclick={checkForUpdates} disabled={disabled || checking || installing} loading={checking}>
        更新を確認
      </Button>
    </div>
  {/if}
</Card>

<style>
  :global(.update-card) {
    display: grid;
    gap: 18px;
    margin-top: 12px;
    padding: 18px;
  }

  .update-heading,
  .update-title,
  .update-actions,
  .up-to-date {
    display: flex;
    align-items: center;
  }

  .update-heading { gap: 13px; }
  .update-heading > div:last-child { min-width: 0; }
  .update-title { flex-wrap: wrap; gap: 8px; }
  .update-title h3 { margin: 0; font-size: 0.94rem; }
  .update-heading p { margin: 4px 0 0; color: var(--muted-foreground); font-size: 0.8rem; }

  .update-icon {
    display: grid;
    width: 38px;
    height: 38px;
    flex: none;
    place-items: center;
    border-radius: 10px;
    color: var(--primary);
    background: color-mix(in oklch, var(--primary) 10%, var(--background));
  }

  .update-icon :global(svg) { width: 20px; height: 20px; }
  .update-actions { flex-wrap: wrap; justify-content: flex-end; gap: 10px; }
  .up-to-date { margin-right: auto; gap: 6px; color: var(--primary); font-size: 0.82rem; font-weight: 650; }
  .up-to-date :global(svg) { width: 16px; height: 16px; }

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
</style>
