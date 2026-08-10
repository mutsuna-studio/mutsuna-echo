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
    preview?: boolean;
    onChanged: () => Promise<void>;
    onMessage: (message: string) => void;
    onError: (message: string) => void;
  }

  let { disabled, preview = false, onChanged, onMessage, onError }: Props = $props();
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
    return "端末内の文字起こし機能を変更できませんでした。";
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
    if (preview) {
      models = [{ modelId: "reazonspeech-k2", displayName: "ReazonSpeech K2 int8-fp32", version: "preview", languageCodes: ["ja"], sizeBytes: 177_209_344, installed: true, downloading: false, runtimeSupported: true }];
      vadModel = { modelId: "silero-vad", displayName: "Silero VAD", version: "preview", sizeBytes: 2_306_867, installed: true, downloading: false, runtimeSupported: true };
      loading = false;
      return;
    }
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
      onMessage("端末だけで文字起こしできるようになりました。");
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
    if (!model || working || !window.confirm("端末だけで文字起こしする機能を削除しますか？")) return;
    working = true;
    try {
      await invoke("delete_local_stt_model", { modelId: model.modelId });
      await refresh();
      await onChanged();
      onMessage("端末だけで文字起こしする機能を削除しました。");
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
        ? "音声のない部分を見つける機能を追加しました。"
        : "音声のない部分を見つける機能を追加しました。次回の文字起こしから処理時間を短くします。");
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
      onMessage("音声の見つけ方を変更しました。次の文字起こしから反映されます。");
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

<div class="local-model-manager model-row" aria-busy={loading || working}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>{model?.displayName ?? "ReazonSpeech K2 int8-fp32"}</strong>
      <Badge variant={model?.installed ? "default" : "secondary"}>
        {model?.installed ? "利用可能" : "未追加"}
      </Badge>
    </div>
    <small>
      日本語向け · 約{model ? formatFileSize(model.sizeBytes) : "169 MB"} · 音声はこの端末だけで処理します
    </small>
    {#if model && !model.runtimeSupported}
      <small>この端末ではまだ使用できません。</small>
    {/if}
    {#if working && progress}
      <progress max="100" value={progressPercent} aria-label="端末内の文字起こし機能を追加しています"></progress>
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
      追加
    </Button>
  {/if}
</div>

<div class="local-model-manager model-row" aria-busy={loading || vadWorking}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>{vadModel?.displayName ?? "Silero VAD"}</strong>
      <Badge variant={vadModel?.installed ? "default" : "secondary"}>
        {vadModel?.installed ? "使用中" : "未追加"}
      </Badge>
    </div>
    <small>
      音声のある部分を自動で見つけます · 約{vadModel ? formatFileSize(vadModel.sizeBytes) : "2.2 MB"}
    </small>
    <small>音声のない部分を飛ばして、端末での文字起こしを速くします。録音が自動で止まることはありません。</small>
    {#if vadModel?.installed}
      <Select
        value={vadPreset}
        options={VAD_PRESET_OPTIONS}
        onValueChange={changePreset}
        disabled={disabled || vadWorking || presetWorking}
        ariaLabel="音声の見つけ方"
      />
    {/if}
    {#if vadModel && !vadModel.runtimeSupported}
      <small>この端末ではまだ使用できません。</small>
    {/if}
    {#if vadWorking && vadProgress}
      <progress max="100" value={vadProgressPercent} aria-label="音声のない部分を見つける機能を追加しています"></progress>
      <small>{formatFileSize(vadProgress.downloadedBytes)} / {formatFileSize(vadProgress.totalBytes)}</small>
    {/if}
  </div>
  {#if vadWorking && !vadModel?.installed}
    <Button variant="outline" type="button" onclick={cancelVadDownload}>キャンセル</Button>
  {:else if !vadModel?.installed}
    <Button type="button" onclick={() => downloadVad()} disabled={disabled || loading || !vadModel || !vadModel.runtimeSupported}>
      追加
    </Button>
  {/if}
</div>
