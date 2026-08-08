<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { formatFileSize } from "../format";
  import type {
    LocalSttModelCatalogEntry,
    LocalSttModelDownloadProgress
  } from "../providers";

  interface Props {
    disabled: boolean;
    onChanged: () => Promise<void>;
    onMessage: (message: string) => void;
    onError: (message: string) => void;
  }

  let { disabled, onChanged, onMessage, onError }: Props = $props();
  let models = $state.raw<LocalSttModelCatalogEntry[]>([]);
  let loading = $state(true);
  let working = $state(false);
  let progress = $state<LocalSttModelDownloadProgress | null>(null);

  const model = $derived(models[0] ?? null);
  const progressPercent = $derived(
    progress && progress.totalBytes > 0
      ? Math.min(100, progress.downloadedBytes / progress.totalBytes * 100)
      : 0
  );

  function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    return "ローカルモデルの操作に失敗しました。";
  }

  async function refresh() {
    models = await invoke<LocalSttModelCatalogEntry[]>("list_local_stt_model_catalog");
    working = models.some((entry) => entry.downloading);
  }

  $effect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      try {
        unlisten = await listen<LocalSttModelDownloadProgress>(
          "local-stt-model-download-progress",
          ({ payload }) => {
            if (!cancelled) progress = payload;
          }
        );
        if (!cancelled) await refresh();
      } catch (error) {
        if (!cancelled) onError(errorText(error));
      } finally {
        if (!cancelled) loading = false;
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  });

  async function download() {
    if (!model || working) return;
    working = true;
    progress = { modelId: model.modelId, downloadedBytes: 0, totalBytes: model.sizeBytes };
    try {
      await invoke("download_local_stt_model", { modelId: model.modelId });
      await refresh();
      await onChanged();
      onMessage("ReazonSpeech K2をインストールしました。");
    } catch (error) {
      onError(errorText(error));
    } finally {
      working = false;
      progress = null;
      try { await refresh(); } catch { /* 次回表示時に再取得する */ }
    }
  }

  async function cancelDownload() {
    try {
      await invoke("cancel_local_stt_model_download");
    } catch (error) {
      onError(errorText(error));
    }
  }

  async function removeModel() {
    if (!model || working || !window.confirm("ReazonSpeech K2を端末から削除しますか？")) return;
    working = true;
    try {
      await invoke("delete_local_stt_model", { modelId: model.modelId });
      await refresh();
      await onChanged();
      onMessage("ReazonSpeech K2を削除しました。");
    } catch (error) {
      onError(errorText(error));
    } finally {
      working = false;
    }
  }
</script>

<div class="local-model-manager" aria-busy={loading || working}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>{model?.displayName ?? "ReazonSpeech K2 int8-fp32"}</strong>
      <Badge variant={model?.installed ? "default" : "secondary"}>
        {model?.installed ? "インストール済み" : "未インストール"}
      </Badge>
    </div>
    <small>
      日本語専用 · 約{model ? formatFileSize(model.sizeBytes) : "169 MB"} · 音声を外部送信しません
    </small>
    {#if model && !model.runtimeSupported}
      <small>このOS向けの推論エンジンは準備中です。</small>
    {/if}
    {#if working && progress}
      <progress max="100" value={progressPercent} aria-label="モデルのダウンロード進捗"></progress>
      <small>{formatFileSize(progress.downloadedBytes)} / {formatFileSize(progress.totalBytes)}</small>
    {/if}
  </div>
  {#if model?.installed}
    <Button variant="outline" type="button" onclick={removeModel} disabled={disabled || working}>
      削除
    </Button>
  {:else if working}
    <Button variant="outline" type="button" onclick={cancelDownload}>キャンセル</Button>
  {:else}
    <Button type="button" onclick={download} disabled={disabled || loading || !model || !model.runtimeSupported}>
      ダウンロード
    </Button>
  {/if}
</div>
