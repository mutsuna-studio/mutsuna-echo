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
    LocalAiRuntimeProgress,
    LocalAiRuntimeStatus,
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
  let runtime = $state.raw<LocalAiRuntimeStatus | null>(null);
  let bundleWorking = $state(false);
  let bundleProgress = $state<LocalAiRuntimeProgress | null>(null);
  let diarizationModel = $state.raw<LocalDiarizationModelStatus | null>(null);
  let diarizationWorking = $state(false);
  let diarizationProgress = $state<LocalSttModelDownloadProgress | null>(null);

  const model = $derived(models[0] ?? null);
  const bundleReady = $derived(runtime?.state === "ready" && !!model?.installed && !!vadModel?.installed);
  const bundleSize = $derived((runtime?.sizeBytes ?? 25 * 1024 * 1024) + (model?.sizeBytes ?? 169 * 1024 * 1024) + (vadModel?.sizeBytes ?? 2.3 * 1024 * 1024));
  const bundleStageLabel = $derived(bundleProgress?.stage === "runtime" ? "実行環境を追加中" : bundleProgress?.stage === "reazonSpeech" ? "日本語モデルを追加中" : bundleProgress?.stage === "sileroVad" ? "無音検出を追加中" : "仕上げ中");
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
    [models, vadModel, vadPreset, recognitionSettings, diarizationModel, runtime] = await Promise.all([
      invoke<LocalSttModelCatalogEntry[]>("list_local_stt_model_catalog"),
      invoke<LocalVadModelStatus>("get_local_vad_model_status"),
      invoke<VadPreset>("get_vad_preset"),
      invoke<LocalRecognitionSettings>("get_local_recognition_settings"),
      invoke<LocalDiarizationModelStatus>("get_local_diarization_model_status"),
      invoke<LocalAiRuntimeStatus>("get_local_ai_runtime_status")
    ]);
    recognitionMode = recognitionSettings.mode;
    working = models.some((entry) => entry.downloading);
    vadWorking = vadModel.downloading;
    diarizationWorking = diarizationModel.downloading;
  }

  $effect(() => {
    if (preview) {
      models = [{ modelId: "reazonspeech-k2", displayName: "ReazonSpeech K2 int8-fp32", version: "preview", languageCodes: ["ja"], sizeBytes: 177_209_344, installed: true, downloading: false, runtimeSupported: true }];
      runtime = { state: "ready", source: "githubRelease", protocolVersion: 1, requiredRuntimeVersion: "preview", installedRuntimeVersion: "preview", progress: null, error: null, sizeBytes: 25 * 1024 * 1024, canDelete: false };
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
        const unlistenRuntime = await listen<LocalAiRuntimeProgress>(
          "local-ai-runtime-progress",
          ({ payload }) => {
            if (!cancelled) {
              bundleWorking = payload.state === "downloading" || payload.state === "installing";
              bundleProgress = payload;
              if (payload.stage === "ready") {
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
          unlistenRuntime();
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

  async function installBundle() {
    if (bundleWorking || bundleReady) return;
    bundleWorking = true;
    bundleProgress = { state: "downloading", stage: "runtime", downloadedBytes: 0, totalBytes: runtime?.sizeBytes ?? 1, progress: 0 };
    try {
      await invoke("install_local_transcription_bundle");
      await refresh();
      await onChanged();
      onMessage("端末内文字起こしを利用できるようになりました。");
    } catch (error) {
      onError(errorText(error));
    } finally {
      bundleWorking = false;
      bundleProgress = null;
      try { await refresh(); } catch { /* 次回表示時に再取得する */ }
    }
  }

  async function cancelBundle() {
    try { await invoke("cancel_local_transcription_bundle_install"); }
    catch (error) { onError(errorText(error)); }
  }

  async function removeRuntime() {
    if (!runtime?.canDelete || !window.confirm("ローカルAIの実行環境を削除しますか？")) return;
    try {
      await invoke("delete_local_ai_runtime");
      await refresh();
      await onChanged();
      onMessage("ローカルAIの実行環境を削除しました。");
    } catch (error) { onError(errorText(error)); }
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
        ? "高精度モードを有効にしました。次回から複数候補と短い発話の補完認識を使います。"
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

  async function removeVad() {
    if (!vadModel?.installed || vadWorking || !window.confirm("無音検出モデルを削除しますか？")) return;
    vadWorking = true;
    try {
      await invoke("delete_local_vad_model");
      await refresh();
      await onChanged();
      onMessage("無音検出モデルを削除しました。");
    } catch (error) {
      onError(errorText(error));
    } finally {
      vadWorking = false;
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

<div class="local-model-manager model-row" aria-busy={loading || bundleWorking}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>端末内文字起こしのセットアップ</strong>
      <span class:ready={bundleReady} class="model-status">{#if bundleReady}<CircleCheck aria-hidden="true" />{/if}{bundleReady ? "利用可能" : "未完了"}</span>
    </div>
    <small>実行環境・日本語モデル・無音検出をまとめて追加 · 最大約{formatFileSize(bundleSize)}</small>
    {#if model?.installed && runtime?.state !== "ready"}
      <small>既存モデルの利用を続けるには、実行環境を追加してください。</small>
    {/if}
    {#if runtime?.state === "incompatible"}<small>実行環境の更新が必要です。</small>{/if}
    {#if runtime?.state === "removalPending"}<small>実行環境はGoogle Playによる削除待ちです。</small>{/if}
    {#if runtime?.error}<small>{runtime.error}</small>{/if}
    {#if bundleWorking && bundleProgress}
      <progress max="100" value={Math.min(100, bundleProgress.progress * 100)} aria-label={bundleStageLabel}></progress>
      <small>{bundleStageLabel}</small>
    {/if}
  </div>
  {#if bundleWorking}
    <Button variant="outline" type="button" onclick={cancelBundle}>キャンセル</Button>
  {:else if !bundleReady}
    <Button type="button" onclick={installBundle} disabled={disabled || loading}>不足分を追加</Button>
  {/if}
</div>

<div class="local-model-manager model-row" aria-busy={loading || working}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>日本語文字起こしモデル</strong>
      <span class:ready={model?.installed} class="model-status">{#if model?.installed}<CircleCheck aria-hidden="true" />{/if}{model?.installed ? "利用可能" : "未追加"}</span>
    </div>
    <small>
      日本語向け · 約{model ? formatFileSize(model.sizeBytes) : "169 MB"} · 端末内で処理
    </small>
    {#if model?.installed}
      <Select
        value={recognitionMode}
        options={LOCAL_RECOGNITION_MODE_OPTIONS}
        onValueChange={changeRecognitionMode}
        disabled={disabled || working || recognitionWorking}
        ariaLabel="ローカル文字起こしの精度"
      />
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
  {/if}
</div>

<div class="local-model-manager model-row" aria-busy={loading || diarizationWorking}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>話者分離</strong>
      <span class:ready={diarizationModel?.installed} class="model-status">{#if diarizationModel?.installed}<CircleCheck aria-hidden="true" />{/if}{diarizationModel?.installed ? "利用可能" : "未追加"}</span>
    </div>
    <small>話者ごとに発話を分けます · 約{diarizationModel ? formatFileSize(diarizationModel.sizeBytes) : "39 MB"} · 端末内で処理</small>
    <small>声から人物を特定する機能ではありません。</small>
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
    <Button type="button" onclick={downloadDiarization} disabled={disabled || loading || runtime?.state !== "ready" || !diarizationModel || !diarizationModel.runtimeSupported}>追加</Button>
  {/if}
</div>

<div class="local-model-manager model-row" aria-busy={loading || vadWorking}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>文字起こしの高速化</strong>
      <span class:ready={vadModel?.installed} class="model-status">{#if vadModel?.installed}<CircleCheck aria-hidden="true" />{/if}{vadModel?.installed ? "使用中" : "未追加"}</span>
    </div>
    <small>
      無音部分を除いて文字起こしを高速化します · 約{vadModel ? formatFileSize(vadModel.sizeBytes) : "2.2 MB"}
    </small>
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
  {:else if vadModel?.installed}
    <Button variant="outline" type="button" onclick={removeVad} disabled={disabled || vadWorking}>削除</Button>
  {/if}
</div>

<div class="local-model-manager model-row" aria-busy={loading}>
  <div class="local-model-copy">
    <div class="local-model-title">
      <strong>ローカルAI実行環境</strong>
      <span class:ready={runtime?.state === "ready"} class="model-status">{runtime?.state === "ready" ? `v${runtime.installedRuntimeVersion}` : "未追加"}</span>
    </div>
    <small>{runtime?.source === "googlePlay" ? "Google Playから追加" : "署名を確認してGitHub Releaseから追加"} · 約{formatFileSize(runtime?.sizeBytes ?? 25 * 1024 * 1024)}</small>
    {#if runtime?.state === "ready" && !runtime.canDelete}<small>モデルが残っている間は削除できません。</small>{/if}
  </div>
  {#if runtime?.state === "ready"}
    <Button variant="outline" type="button" onclick={removeRuntime} disabled={disabled || !runtime.canDelete}>削除</Button>
  {/if}
</div>
