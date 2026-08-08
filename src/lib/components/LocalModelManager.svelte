<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { formatFileSize } from "../format";
  import type {
    LocalSttModelCatalogEntry,
    LocalSttModelDownloadProgress,
    LocalVadModelStatus
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
  let vadModel = $state.raw<LocalVadModelStatus | null>(null);
  let vadWorking = $state(false);
  let vadProgress = $state<LocalSttModelDownloadProgress | null>(null);

  const model = $derived(models[0] ?? null);
  const progressPercent = $derived(
    progress && progress.totalBytes > 0
      ? Math.min(100, progress.downloadedBytes / progress.totalBytes * 100)
      : 0
  );
  const vadProgressPercent = $derived(
    vadProgress && vadProgress.totalBytes > 0
      ? Math.min(100, vadProgress.downloadedBytes / vadProgress.totalBytes * 100)
      : 0
  );

  function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    return "ローカルモデルの操作に失敗しました。";
  }

  async function refresh() {
    [models, vadModel] = await Promise.all([
      invoke<LocalSttModelCatalogEntry[]>("list_local_stt_model_catalog"),
      invoke<LocalVadModelStatus>("get_local_vad_model_status")
    ]);
    working = models.some((entry) => entry.downloading);
    vadWorking = vadModel.downloading;
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
        const unlistenVad = await listen<LocalSttModelDownloadProgress>(
          "local-vad-model-download-progress",
          ({ payload }) => {
            if (!cancelled) vadProgress = payload;
          }
        );
        const unlistenStt = unlisten;
        unlisten = () => {
          unlistenStt?.();
          unlistenVad();
        };
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

  async function downloadVad() {
    if (!vadModel || vadWorking) return;
    vadWorking = true;
    vadProgress = { modelId: vadModel.modelId, downloadedBytes: 0, totalBytes: vadModel.sizeBytes };
    try {
      await invoke("download_local_vad_model");
      await refresh();
      onMessage("Silero VADをインストールしました。次回のローカル文字起こしから無音区間を除外します。");
    } catch (error) {
      onError(errorText(error));
    } finally {
      vadWorking = false;
      vadProgress = null;
      try { await refresh(); } catch { /* 次回表示時に再取得する */ }
    }
  }

  async function cancelVadDownload() {
    try {
      await invoke("cancel_local_vad_model_download");
    } catch (error) {
      onError(errorText(error));
    }
  }

  async function removeVad() {
    if (!vadModel || vadWorking || !window.confirm("Silero VADを端末から削除しますか？")) return;
    vadWorking = true;
    try {
      await invoke("delete_local_vad_model");
      await refresh();
      onMessage("Silero VADを削除しました。ローカル文字起こしは従来の全音声処理に戻ります。");
    } catch (error) {
      onError(errorText(error));
    } finally {
      vadWorking = false;
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

<div class="local-model-manager" aria-busy={loading || vadWorking}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>{vadModel?.displayName ?? "Silero VAD"}</strong>
      <Badge variant={vadModel?.installed ? "default" : "secondary"}>
        {vadModel?.installed ? "有効" : "未インストール"}
      </Badge>
    </div>
    <small>
      音声区間検出 · 約{vadModel ? formatFileSize(vadModel.sizeBytes) : "2.2 MB"} · 元音声の時刻を保持
    </small>
    <small>ローカルSTTの無音区間を除外して処理時間を短縮します。録音の自動停止には使用しません。</small>
    {#if vadModel && !vadModel.runtimeSupported}
      <small>このOS向けのVAD推論エンジンは準備中です。</small>
    {/if}
    {#if vadWorking && vadProgress}
      <progress max="100" value={vadProgressPercent} aria-label="VADモデルのダウンロード進捗"></progress>
      <small>{formatFileSize(vadProgress.downloadedBytes)} / {formatFileSize(vadProgress.totalBytes)}</small>
    {/if}
  </div>
  {#if vadModel?.installed}
    <Button variant="outline" type="button" onclick={removeVad} disabled={disabled || vadWorking}>
      削除
    </Button>
  {:else if vadWorking}
    <Button variant="outline" type="button" onclick={cancelVadDownload}>キャンセル</Button>
  {:else}
    <Button type="button" onclick={downloadVad} disabled={disabled || loading || !vadModel || !vadModel.runtimeSupported}>
      ダウンロード
    </Button>
  {/if}
</div>
