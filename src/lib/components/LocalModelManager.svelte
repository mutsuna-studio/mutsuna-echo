<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import { Button } from "@mutsuna/ui/button";
  import { Select } from "@mutsuna/ui/select";
  import { formatFileSize } from "../format";
  import { LOCAL_RECOGNITION_MODE_OPTIONS, VAD_PRESET_OPTIONS } from "../providers";
  import type {
    LocalSttModelCatalogEntry,
    LocalSttModelDownloadProgress,
    LocalRecognitionMode,
    LocalRecognitionSettings,
    LocalDiarizationModelStatus,
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
  let recognitionMode = $state<LocalRecognitionMode>("fast");
  let recognitionWorking = $state(false);
  let autoInstallAttempted = $state(false);
  let diarizationModel = $state.raw<LocalDiarizationModelStatus | null>(null);
  let diarizationWorking = $state(false);
  let diarizationProgress = $state<LocalSttModelDownloadProgress | null>(null);

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
  const diarizationProgressPercent = $derived(
    diarizationProgress && diarizationProgress.totalBytes > 0
      ? Math.min(100, diarizationProgress.downloadedBytes / diarizationProgress.totalBytes * 100)
      : 0
  );

  function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    return "端末内の文字起こし機能を変更できませんでした。";
  }

  async function refresh() {
    let recognitionSettings: LocalRecognitionSettings;
    [models, vadModel, vadPreset, recognitionSettings, diarizationModel] = await Promise.all([
      invoke<LocalSttModelCatalogEntry[]>("list_local_stt_model_catalog"),
      invoke<LocalVadModelStatus>("get_local_vad_model_status"),
      invoke<VadPreset>("get_vad_preset"),
      invoke<LocalRecognitionSettings>("get_local_recognition_settings"),
      invoke<LocalDiarizationModelStatus>("get_local_diarization_model_status")
    ]);
    recognitionMode = recognitionSettings.mode;
    working = models.some((entry) => entry.downloading);
    vadWorking = vadModel.downloading;
    diarizationWorking = diarizationModel.downloading;
  }

  $effect(() => {
    if (preview) {
      models = [{ modelId: "reazonspeech-k2", displayName: "ReazonSpeech K2 int8-fp32", version: "preview", languageCodes: ["ja"], sizeBytes: 177_209_344, installed: true, downloading: false, runtimeSupported: true }];
      vadModel = { modelId: "silero-vad", displayName: "Silero VAD", version: "preview", sizeBytes: 2_306_867, installed: true, downloading: false, runtimeSupported: true };
      diarizationModel = { modelId: "pyannote-3.0-int8-3dspeaker-eres2net-base", displayName: "pyannote 3.0 INT8 + 3D-Speaker ERes2Net Base", version: "preview", sizeBytes: 41_134_267, installed: true, downloading: false, runtimeSupported: true };
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
            if (!cancelled) {
              progress = payload;
              if (payload.totalBytes > 0 && payload.downloadedBytes >= payload.totalBytes) {
                window.setTimeout(() => {
                  if (!cancelled) void (async () => {
                    await refresh();
                    await onChanged();
                  })().catch((error) => onError(errorText(error)));
                }, 300);
              }
            }
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
        const unlistenDiarization = await listen<LocalSttModelDownloadProgress>(
          "local-diarization-model-download-progress",
          ({ payload }) => {
            if (!cancelled) {
              diarizationWorking = true;
              diarizationProgress = payload;
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
          unlistenDiarization();
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
      try {
        await refresh();
        await onChanged();
      } catch { /* 次回表示時に再取得する */ }
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
      await onChanged();
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

  async function changeRecognitionMode(value: string) {
    if (value !== "fast" && value !== "accurate") return;
    const previous = recognitionMode;
    recognitionMode = value;
    recognitionWorking = true;
    try {
      const saved = await invoke<LocalRecognitionSettings>("set_local_recognition_settings", {
        settings: { mode: recognitionMode }
      });
      recognitionMode = saved.mode;
      onMessage(recognitionMode === "accurate"
        ? "高精度モードを有効にしました。次回から複数候補と重要用語を使います。"
        : "高速モードを有効にしました。重要用語がある場合だけ複数候補を使います。");
    } catch (error) {
      recognitionMode = previous;
      onError(errorText(error));
    } finally {
      recognitionWorking = false;
    }
  }

  async function cancelVadDownload() {
    try {
      await invoke("cancel_local_vad_model_download");
    } catch (error) {
      onError(errorText(error));
    }
  }

  async function downloadDiarization() {
    if (!diarizationModel || diarizationWorking) return;
    diarizationWorking = true;
    diarizationProgress = { modelId: diarizationModel.modelId, downloadedBytes: 0, totalBytes: diarizationModel.sizeBytes };
    try {
      await invoke("download_local_diarization_models");
      await refresh();
      await onChanged();
      onMessage("端末だけで話者分離できるようになりました。");
    } catch (error) {
      onError(errorText(error));
    } finally {
      diarizationWorking = false;
      diarizationProgress = null;
      try { await refresh(); } catch { /* 次回表示時に再取得する */ }
    }
  }

  async function cancelDiarizationDownload() {
    try {
      await invoke("cancel_local_diarization_model_download");
    } catch (error) {
      onError(errorText(error));
    }
  }

  async function removeDiarization() {
    if (!diarizationModel || diarizationWorking || !window.confirm("端末内の話者分離モデルを削除しますか？")) return;
    diarizationWorking = true;
    try {
      await invoke("delete_local_diarization_models");
      await refresh();
      await onChanged();
      onMessage("端末内の話者分離モデルを削除しました。");
    } catch (error) {
      onError(errorText(error));
    } finally {
      diarizationWorking = false;
    }
  }

</script>

<div class="local-model-manager model-row" aria-busy={loading || working}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>{model?.displayName ?? "ReazonSpeech K2 int8-fp32"}</strong>
      <span class:ready={model?.installed} class="model-status">{#if model?.installed}<CircleCheck aria-hidden="true" />{/if}{model?.installed ? "利用可能" : "未追加"}</span>
    </div>
    <small>
      日本語向け · 約{model ? formatFileSize(model.sizeBytes) : "169 MB"} · 音声はこの端末だけで処理します
    </small>
    {#if model?.installed}
      <Select
        value={recognitionMode}
        options={LOCAL_RECOGNITION_MODE_OPTIONS}
        onValueChange={changeRecognitionMode}
        disabled={disabled || working || recognitionWorking}
        ariaLabel="ローカル文字起こしの精度"
      />
      <small>重要用語が設定されている場合は、高速モードでも用語を優先する探索を使用します。</small>
    {/if}
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

<div class="local-model-manager model-row" aria-busy={loading || diarizationWorking}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>{diarizationModel?.displayName ?? "pyannote 3.0 INT8 + 3D-Speaker ERes2Net Base"}</strong>
      <span class:ready={diarizationModel?.installed} class="model-status">{#if diarizationModel?.installed}<CircleCheck aria-hidden="true" />{/if}{diarizationModel?.installed ? "利用可能" : "未追加"}</span>
    </div>
    <small>長時間音声対応 · 約{diarizationModel ? formatFileSize(diarizationModel.sizeBytes) : "39 MB"} · 音声は端末外へ送信しません</small>
    <small>文字起こし後に話者を分けます。人物本人を識別する機能ではありません。</small>
    {#if diarizationModel && !diarizationModel.runtimeSupported}<small>この端末ではまだ使用できません。</small>{/if}
    {#if diarizationWorking && diarizationProgress}
      <progress max="100" value={diarizationProgressPercent} aria-label="話者分離モデルを追加しています"></progress>
      <small>{formatFileSize(diarizationProgress.downloadedBytes)} / {formatFileSize(diarizationProgress.totalBytes)}</small>
    {/if}
  </div>
  {#if diarizationModel?.installed}
    <Button variant="outline" type="button" onclick={removeDiarization} disabled={disabled || diarizationWorking}>削除</Button>
  {:else if diarizationWorking}
    <Button variant="outline" type="button" onclick={cancelDiarizationDownload}>キャンセル</Button>
  {:else}
    <Button type="button" onclick={downloadDiarization} disabled={disabled || loading || !diarizationModel || !diarizationModel.runtimeSupported}>追加</Button>
  {/if}
</div>

<div class="local-model-manager model-row" aria-busy={loading || vadWorking}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>{vadModel?.displayName ?? "Silero VAD"}</strong>
      <span class:ready={vadModel?.installed} class="model-status">{#if vadModel?.installed}<CircleCheck aria-hidden="true" />{/if}{vadModel?.installed ? "使用中" : "未追加"}</span>
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
