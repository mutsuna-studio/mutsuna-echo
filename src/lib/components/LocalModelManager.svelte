<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { Select } from "@mutsuna/ui/select";
  import { formatFileSize } from "../format";
  import { VAD_PRESET_OPTIONS } from "../providers";
  import type {
    LocalSttModelCatalogEntry,
    LocalSttModelDownloadProgress,
    LocalVadModelStatus,
    VadPreset
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
  let vadPreset = $state<VadPreset>("standard");
  let presetWorking = $state(false);
  let autoInstallAttempted = $state(false);

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
    [models, vadModel, vadPreset] = await Promise.all([
      invoke<LocalSttModelCatalogEntry[]>("list_local_stt_model_catalog"),
      invoke<LocalVadModelStatus>("get_local_vad_model_status"),
      invoke<VadPreset>("get_vad_preset")
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
            if (!cancelled) {
              vadWorking = true;
              vadProgress = payload;
              if (payload.totalBytes > 0 && payload.downloadedBytes >= payload.totalBytes) {
                window.setTimeout(() => {
                  if (!cancelled) void refresh().catch((error) => onError(errorText(error)));
                }, 300);
              }
            }
          }
        );
        const unlistenStt = unlisten;
        unlisten = () => {
          unlistenStt?.();
          unlistenVad();
        };
        if (!cancelled) await refresh();
        if (!cancelled && !autoInstallAttempted && models[0]?.installed && vadModel && !vadModel.installed && !vadModel.downloading && vadModel.runtimeSupported) {
          autoInstallAttempted = true;
          await downloadVad(true);
        }
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
      onMessage("ReazonSpeech K2とSilero VADをインストールしました。");
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

  async function downloadVad(automatic = false) {
    if (!vadModel || vadWorking) return;
    vadWorking = true;
    vadProgress = { modelId: vadModel.modelId, downloadedBytes: 0, totalBytes: vadModel.sizeBytes };
    try {
      await invoke("download_local_vad_model");
      await refresh();
      onMessage(automatic
        ? "Silero VADを標準機能として追加しました。"
        : "Silero VADをインストールしました。次回のローカル文字起こしから無音区間を除外します。");
    } catch (error) {
      onError(errorText(error));
    } finally {
      vadWorking = false;
      vadProgress = null;
      try { await refresh(); } catch { /* 次回表示時に再取得する */ }
    }
  }

  async function changePreset(value: string) {
    if (!(["softVoice", "standard", "noiseReduction"] as string[]).includes(value)) return;
    const previous = vadPreset;
    vadPreset = value as VadPreset;
    presetWorking = true;
    try {
      await invoke("set_vad_preset", { preset: vadPreset });
      onMessage("VADの検出感度を更新しました。次の録音・文字起こしから反映します。");
    } catch (error) {
      vadPreset = previous;
      onError(errorText(error));
    } finally {
      presetWorking = false;
    }
  }

  async function cancelVadDownload() {
    try {
      await invoke("cancel_local_vad_model_download");
    } catch (error) {
      onError(errorText(error));
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
    {#if vadModel?.installed}
      <Select
        value={vadPreset}
        options={VAD_PRESET_OPTIONS}
        onValueChange={changePreset}
        searchable
        disabled={disabled || vadWorking || presetWorking}
        ariaLabel="VADの検出感度"
      />
    {/if}
    {#if vadModel && !vadModel.runtimeSupported}
      <small>このOS向けのVAD推論エンジンは準備中です。</small>
    {/if}
    {#if vadWorking && vadProgress}
      <progress max="100" value={vadProgressPercent} aria-label="VADモデルのダウンロード進捗"></progress>
      <small>{formatFileSize(vadProgress.downloadedBytes)} / {formatFileSize(vadProgress.totalBytes)}</small>
    {/if}
  </div>
  {#if vadWorking && !vadModel?.installed}
    <Button variant="outline" type="button" onclick={cancelVadDownload}>キャンセル</Button>
  {:else if !vadModel?.installed}
    <Button type="button" onclick={() => downloadVad()} disabled={disabled || loading || !vadModel || !vadModel.runtimeSupported}>
      ダウンロード
    </Button>
  {/if}
</div>
