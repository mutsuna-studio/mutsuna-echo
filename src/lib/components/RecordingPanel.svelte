<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { Alert, AlertDescription, AlertTitle } from "@mutsuna/ui/alert";
  import {
    AlertDialog,
    AlertDialogAction,
    AlertDialogCancel,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle
  } from "@mutsuna/ui/alert-dialog";
  import { Button } from "@mutsuna/ui/button";
  import { Checkbox } from "@mutsuna/ui/checkbox";
  import { Popover, PopoverContent, PopoverTrigger } from "@mutsuna/ui/popover";
  import { Select, SelectContent, SelectItem, SelectTrigger } from "@mutsuna/ui/select";
  import Info from "@lucide/svelte/icons/info";
  import Mic from "@lucide/svelte/icons/mic";
  import MonitorSpeaker from "@lucide/svelte/icons/monitor-speaker";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Square from "@lucide/svelte/icons/square";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { VAD_PRESET_OPTIONS, type LocalVadModelStatus, type VadPreset } from "../providers";
  import type { SelectedAudioFile } from "../types/transcript";
  import type {
    RecoverableRecording,
    RecordingCapabilities,
    RecordingStatus,
    StopRecordingResult
  } from "../types/recording";
  import AudioLevelWaveform from "./AudioLevelWaveform.svelte";

  interface Props {
    disabled?: boolean;
    preview?: boolean;
    consumeMobileAction?: () => boolean;
    onAudioReady: (audio: SelectedAudioFile) => void;
    onBusyChange: (busy: boolean) => void;
    onMessage: (message: string) => void;
    onError: (message: string) => void;
  }

  let {
    disabled = false,
    preview = false,
    consumeMobileAction = () => false,
    onAudioReady,
    onBusyChange = () => {},
    onMessage,
    onError
  }: Props = $props();

  const previewCapabilities: RecordingCapabilities = {
    platform: "windows",
    supported: true,
    microphoneSupported: true,
    systemAudioSupported: true,
    systemAudioLimited: false,
    limitation: null,
    microphoneDevices: [{ id: "preview-microphone", name: "既定のマイク", isDefault: true }],
    systemDevices: [{ id: "preview-system", name: "既定の出力", isDefault: true }],
    sampleRate: 48_000,
    channels: 1,
    codec: "AAC-LC",
    bitrate: 128_000,
    maxDurationMs: 36_000_000
  };
  const previewStatus: RecordingStatus = {
    phase: "idle",
    sessionId: null,
    elapsedMs: 0,
    microphoneLevel: 0.18,
    systemLevel: 0,
    microphoneSpectrum: [],
    systemSpectrum: [],
    microphone: true,
    systemAudio: false,
    voiceActivity: "listening",
    outputPath: null,
    microphoneTrackPath: null,
    systemTrackPath: null,
    stopReason: null,
    warning: null,
    error: null
  };

  let capabilities = $state<RecordingCapabilities | null>(null);
  let status = $state<RecordingStatus | null>(null);
  let recoverable = $state<RecoverableRecording[]>([]);
  let microphone = $state(true);
  let systemAudio = $state(true);
  let microphoneDeviceId = $state("");
  let systemDeviceId = $state("");
  let loading = $state(true);
  let actionBusy = $state(false);
  let deliveredOutput = $state("");
  let cancelDialogOpen = $state(false);
  let discardDialogOpen = $state(false);
  let pendingDiscard = $state<RecoverableRecording | null>(null);
  let statusEventsAvailable = $state(false);
  let monitoring = $state(false);
  let monitorSuspended = $state(false);
  let monitorRevision = 0;
  let vadPreset = $state<VadPreset>("standard");
  let vadPresetBusy = $state(false);
  let vadModel = $state.raw<LocalVadModelStatus | null>(null);
  let previewRecordingStartedAt = 0;
  let previewMicrophoneLevel = $state(0.18);
  let settingsPopoverOpen = $state(false);

  $effect(() => {
    if (!preview) return;
    capabilities = previewCapabilities;
    status = previewStatus;
    microphone = true;
    systemAudio = false;
    microphoneDeviceId = "preview-microphone";
    systemDeviceId = "preview-system";
    vadModel = {
      modelId: "preview-vad",
      displayName: "Silero VAD",
      version: "preview",
      sizeBytes: 0,
      installed: true,
      downloading: false,
      runtimeSupported: true
    };
    loading = false;

    const startedAt = performance.now();
    const previewMeter = window.setInterval(() => {
      const phase = (performance.now() - startedAt) / 420;
      const current = status ?? previewStatus;
      const previewActive = current.phase === "recording";
      previewMicrophoneLevel = 0.08 + Math.abs(Math.sin(phase) * 0.2 + Math.sin(phase * 0.47) * 0.08);
      if (previewActive) {
        status = {
          ...current,
          elapsedMs: performance.now() - previewRecordingStartedAt,
          microphoneLevel: previewMicrophoneLevel
        };
      }
    }, 90);
    return () => window.clearInterval(previewMeter);
  });

  const active = $derived(
    status?.phase === "starting" || status?.phase === "recording" || status?.phase === "finalizing"
  );
  const canStart = $derived(
    Boolean(capabilities?.supported) && (microphone || systemAudio) && !disabled && !actionBusy && !active
  );
  const metering = $derived(active || monitoring);
  const microphoneInputLevel = $derived(preview ? previewMicrophoneLevel : (status?.microphoneLevel ?? 0));
  const microphoneSpectrum = $derived(
    preview ? createPreviewSpectrum(previewMicrophoneLevel, 0.35) : (status?.microphoneSpectrum ?? [])
  );
  const systemSpectrum = $derived(status?.systemSpectrum ?? []);
  const microphoneMeterPercent = $derived(toMeterPercent(microphoneInputLevel, (preview || metering) && microphone));
  const systemMeterPercent = $derived(toMeterPercent(status?.systemLevel ?? 0, metering && systemAudio));
  const vadAvailability = $derived.by<"preparing" | "available" | "unavailable">(() => {
    if (loading || !capabilities || !vadModel || vadModel.downloading) return "preparing";
    // AndroidのVADランタイムは文字起こし用で、録音中の無音検出にはまだ接続されていない。
    if (capabilities.platform === "android" || !vadModel.runtimeSupported || !vadModel.installed) {
      return "unavailable";
    }
    if (status?.phase === "starting") return "preparing";
    if (active && status?.voiceActivity === "unavailable") return "unavailable";
    return "available";
  });
  const microphoneOptions = $derived([
    { value: "", label: "OSの既定マイク" },
    ...((capabilities?.microphoneDevices ?? []).map((device) => ({
      value: device.id,
      label: `${device.name}${device.isDefault ? "（既定）" : ""}`
    })))
  ]);
  const systemOptions = $derived([
    { value: "", label: "OSの既定出力" },
    ...((capabilities?.systemDevices ?? []).map((device) => ({
      value: device.id,
      label: `${device.name}${device.isDefault ? "（既定）" : ""}`
    })))
  ]);
  const defaultDeviceSelectValue = "__os_default_device__";
  const selectedMicrophoneLabel = $derived(
    microphoneOptions.find((option) => option.value === microphoneDeviceId)?.label ?? "OSの既定マイク"
  );
  const selectedSystemLabel = $derived(
    systemOptions.find((option) => option.value === systemDeviceId)?.label ?? "OSの既定出力"
  );
  const selectedVadLabel = $derived(
    VAD_PRESET_OPTIONS.find((option) => option.value === vadPreset)?.label ?? "標準"
  );
  const inputSettingsSummary = $derived.by(() => {
    const sources = [
      microphone ? "マイク ON" : "マイク OFF",
      systemAudio ? "システム音声 ON" : "システム音声 OFF"
    ];
    return `${sources.join(" · ")} · 無音停止 ${selectedVadLabel}`;
  });

  $effect(() => {
    if (active) settingsPopoverOpen = false;
  });

  function toDeviceSelectValue(value: string): string {
    return value === "" ? defaultDeviceSelectValue : value;
  }

  function fromDeviceSelectValue(value: string): string {
    return value === defaultDeviceSelectValue ? "" : value;
  }

  function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    return "録音中に予期しないエラーが発生しました。";
  }

  function createPreviewSpectrum(level: number, phase: number): number[] {
    return Array.from({ length: 24 }, (_, index) => {
      const lowFrequencyBias = Math.exp(-index / 14);
      const contour = 0.56 + 0.28 * Math.sin(index * 0.72 + phase) + 0.16 * Math.sin(index * 1.83);
      return Math.max(0, Math.min(1, level * lowFrequencyBias * Math.max(0.12, contour) * 3.2));
    });
  }

  function formatTimer(milliseconds: number): string {
    const seconds = Math.floor(milliseconds / 1000);
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const rest = seconds % 60;
    return `${hours.toString().padStart(2, "0")}:${minutes.toString().padStart(2, "0")}:${rest.toString().padStart(2, "0")}`;
  }

  function stopMessage(reason: RecordingStatus["stopReason"]): string {
    if (reason === "durationLimit") return "10時間の上限に達したため録音を停止しました。";
    if (reason === "sourceDisconnected") return "音声デバイスが切断されたため録音を停止し、取得済みの音声を保存しました。";
    if (reason === "sourceStalled") return "選択した音声を取得できなくなったため録音を停止し、取得済みの音声を保存しました。";
    if (reason === "captureError") return "音声の取得エラーにより録音を停止しました。";
    return "録音を保存しました。内容を確認してから文字起こしを開始できます。";
  }

  async function deliverCompletedRecording(nextStatus: RecordingStatus, completedAudio?: SelectedAudioFile | null) {
    const outputPath = nextStatus.outputPath;
    if (nextStatus.phase !== "completed" || !outputPath || deliveredOutput === outputPath) return;

    // 状態イベントと停止操作の結果が並行して届くため、非同期処理の前に予約する。
    deliveredOutput = outputPath;
    try {
      const audio = completedAudio ?? await invoke<SelectedAudioFile | null>("get_recorded_audio");
      if (!audio) {
        deliveredOutput = "";
        return;
      }
      onAudioReady(audio);
      onMessage(stopMessage(nextStatus.stopReason));
    } catch (error) {
      deliveredOutput = "";
      throw error;
    }
  }

  function toMeterPercent(amplitude: number, enabled: boolean): number {
    if (!enabled || !Number.isFinite(amplitude) || amplitude <= 0.001) return 0;
    // PCM peakをdBFSへ変換し、小さな声も視認できる範囲へ正規化する。
    const decibels = 20 * Math.log10(Math.min(1, amplitude));
    return Math.round(Math.max(0, Math.min(1, (decibels + 60) / 60)) * 100);
  }

  async function refreshStatus() {
    const nextStatus = await invoke<RecordingStatus>("get_recording_status");
    await acceptStatus(nextStatus);
  }

  async function acceptStatus(nextStatus: RecordingStatus, deliverCompletion = true) {
    status = nextStatus;
    if (deliverCompletion) await deliverCompletedRecording(nextStatus);
    if (nextStatus.phase === "failed" && nextStatus.error) onError(nextStatus.error);
  }

  function currentRequest() {
    return {
      microphone,
      systemAudio,
      microphoneDeviceId: microphoneDeviceId || null,
      systemDeviceId: systemDeviceId || null
    };
  }

  $effect(() => {
    onBusyChange(active || actionBusy);
  });

  $effect(() => {
    if (preview) return;
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    void (async () => {
      try {
        try {
          unlisten = await listen<RecordingStatus>("recording-status", ({ payload }) => {
            if (!cancelled) void acceptStatus(payload).catch((error) => onError(errorText(error)));
          });
          statusEventsAvailable = true;
        } catch (error) {
          console.error("Could not subscribe to recording status events", error);
        }
        const [nextCapabilities, nextStatus, nextRecoverable, nextVadPreset, nextVadModel] = await Promise.all([
          invoke<RecordingCapabilities>("get_recording_capabilities"),
          invoke<RecordingStatus>("get_recording_status"),
          invoke<RecoverableRecording[]>("list_recoverable_recordings"),
          invoke<VadPreset>("get_vad_preset"),
          invoke<LocalVadModelStatus>("get_local_vad_model_status")
        ]);
        if (cancelled) return;
        capabilities = nextCapabilities;
        recoverable = nextRecoverable;
        microphone = nextCapabilities.microphoneSupported;
        // Androidでは画面共有の確認が録音開始時に必要になるため、初期値はオフにする。
        systemAudio = nextCapabilities.platform === "android" ? false : nextCapabilities.systemAudioSupported;
        microphoneDeviceId = nextCapabilities.microphoneDevices.find((device) => device.isDefault)?.id ?? "";
        systemDeviceId = nextCapabilities.systemDevices.find((device) => device.isDefault)?.id ?? "";
        vadPreset = nextVadPreset;
        vadModel = nextVadModel;
        // 画面の再表示時に残っている前回のcompleted状態は、新しい録音完了ではない。
        // 初回同期では表示状態だけを復元し、通知と音声の再引き渡しを行わない。
        if (nextStatus.phase === "completed" && nextStatus.outputPath) {
          deliveredOutput = nextStatus.outputPath;
        }
        await acceptStatus(nextStatus, false);
      } catch (error) {
        onError(errorText(error));
      } finally {
        loading = false;
      }
    })();
    return () => {
      cancelled = true;
      statusEventsAvailable = false;
      unlisten?.();
    };
  });

  $effect(() => {
    if (preview) return;
    const ready = !loading && Boolean(capabilities?.supported) && !active && !monitorSuspended && (microphone || systemAudio);
    const request = currentRequest();
    const revision = ++monitorRevision;
    if (!ready) {
      monitoring = false;
      if (!(monitorSuspended && capabilities?.platform === "android" && systemAudio)) {
        void invoke("stop_recording_monitor").catch(() => {});
      }
      return;
    }

    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          await invoke("stop_recording_monitor");
          if (revision !== monitorRevision) return;
          const nextStatus = await invoke<RecordingStatus>("start_recording_monitor", { request });
          if (revision !== monitorRevision) {
            await invoke("stop_recording_monitor");
            return;
          }
          monitoring = true;
          await acceptStatus(nextStatus);
        } catch (error) {
          if (revision === monitorRevision) {
            monitoring = false;
            status = status ? { ...status, warning: errorText(error) } : status;
          }
        }
      })();
    }, 180);

    return () => {
      window.clearTimeout(timer);
      if (revision === monitorRevision) monitorRevision += 1;
      monitoring = false;
      if (!(monitorSuspended && capabilities?.platform === "android" && systemAudio)) {
        void invoke("stop_recording_monitor").catch(() => {});
      }
    };
  });

  // Androidまたはイベント購読失敗時だけ、録音中に限定して状態を確認する。
  $effect(() => {
    if (preview) return;
    if ((!active && !monitoring) || (capabilities?.platform !== "android" && statusEventsAvailable)) return;
    const timer = window.setInterval(() => {
      void refreshStatus().catch((error) => onError(errorText(error)));
    }, 120);
    return () => window.clearInterval(timer);
  });

  async function start() {
    if (preview) {
      previewRecordingStartedAt = performance.now();
      status = { ...previewStatus, phase: "recording", sessionId: "preview-recording" };
      return;
    }
    actionBusy = true;
    monitorSuspended = true;
    monitorRevision += 1;
    monitoring = false;
    onMessage("");
    onError("");
    try {
      if (!(capabilities?.platform === "android" && systemAudio)) {
        await invoke("stop_recording_monitor");
      }
      status = await invoke<RecordingStatus>("start_recording", {
        request: currentRequest()
      });
      deliveredOutput = "";
    } catch (error) {
      monitorSuspended = false;
      onError(errorText(error));
    } finally {
      actionBusy = false;
    }
  }

  async function changeVadPreset(value: string) {
    if (!(VAD_PRESET_OPTIONS as readonly { value: string }[]).some((option) => option.value === value)) return;
    const previous = vadPreset;
    vadPreset = value as VadPreset;
    if (preview) return;
    vadPresetBusy = true;
    try {
      await invoke("set_vad_preset", { preset: vadPreset });
      onMessage("音声検出の感度を更新しました。");
    } catch (error) {
      vadPreset = previous;
      onError(errorText(error));
    } finally {
      vadPresetBusy = false;
    }
  }

  async function stop() {
    if (preview) {
      status = previewStatus;
      return;
    }
    actionBusy = true;
    onMessage("");
    onError("");
    try {
      const result = await invoke<StopRecordingResult>("stop_recording");
      status = result.status;
      await deliverCompletedRecording(result.status, result.audio);
    } catch (error) {
      onError(errorText(error));
    } finally {
      actionBusy = false;
    }
  }

  async function cancel() {
    if (preview) {
      status = previewStatus;
      cancelDialogOpen = false;
      return;
    }
    actionBusy = true;
    try {
      status = await invoke<RecordingStatus>("cancel_recording");
      cancelDialogOpen = false;
      onMessage("録音を破棄しました。");
    } catch (error) {
      onError(errorText(error));
    } finally {
      actionBusy = false;
    }
  }

  async function recover(recording: RecoverableRecording) {
    actionBusy = true;
    try {
      const audio = await invoke<SelectedAudioFile>("recover_recording", { sessionId: recording.sessionId });
      recoverable = recoverable.filter((item) => item.sessionId !== recording.sessionId);
      onAudioReady(audio);
      onMessage("中断された録音を復旧しました。");
    } catch (error) {
      onError(errorText(error));
    } finally {
      actionBusy = false;
    }
  }

  async function discard(recording: RecoverableRecording) {
    actionBusy = true;
    try {
      await invoke("discard_recording", { sessionId: recording.sessionId });
      recoverable = recoverable.filter((item) => item.sessionId !== recording.sessionId);
      discardDialogOpen = false;
      pendingDiscard = null;
    } catch (error) {
      onError(errorText(error));
    } finally {
      actionBusy = false;
    }
  }

  function confirmDiscard(recording: RecoverableRecording) {
    pendingDiscard = recording;
    discardDialogOpen = true;
  }

</script>

{#if loading}
  <p class="placeholder" role="status">録音デバイスを確認しています…</p>
{:else if capabilities}
  {#if capabilities.limitation}
    <p class="limitation" role="note">
      <Info aria-hidden="true" />
      <span>{capabilities.platform === "android" ? "一部の通話・保護された音声は録音できません" : capabilities.limitation}</span>
    </p>
  {/if}

  {#if recoverable.length > 0 && !active}
    <Alert role="alert">
      <AlertTitle>中断された録音があります</AlertTitle>
      <AlertDescription>
      {#each recoverable as recording (recording.sessionId)}
        <div class="recovery-row">
          <span>{new Date(recording.startedAt).toLocaleString("ja-JP")} · {formatTimer(recording.durationMs)}</span>
          <div>
            <Button variant="outline" size="sm" type="button" onclick={() => recover(recording)} disabled={actionBusy}>復旧</Button>
            <Button variant="destructive" size="sm" type="button" onclick={() => confirmDiscard(recording)} disabled={actionBusy}>削除</Button>
          </div>
        </div>
      {/each}
      </AlertDescription>
    </Alert>
  {/if}

  <div class:active class="recording-console">
  <section class="desktop-recording-hero" aria-label="録音操作">
    <div class:active class="desktop-recording-status" role="status">
      {#if active}
        <span class="record-dot" aria-hidden="true"></span>
        <span>{status?.voiceActivity === "speechDetected" ? "音声を検出中" : "録音中"}</span>
        <strong>{formatTimer(status?.elapsedMs ?? 0)}</strong>
      {/if}
    </div>
    <div class="waveform-stage">
      <AudioLevelWaveform
        microphoneLevel={microphoneInputLevel}
        systemLevel={status?.systemLevel ?? 0}
        microphoneEnabled={microphone}
        systemEnabled={systemAudio}
        elapsedMs={status?.elapsedMs ?? 0}
        {microphoneSpectrum}
        {systemSpectrum}
        hero
      />
      <div class="desktop-record-controls">
        <button
          class:active
          class="desktop-start"
          type="button"
          onclick={active ? stop : start}
          disabled={active ? actionBusy || status?.phase === "finalizing" : !canStart}
          aria-label={active ? "録音を停止" : "録音を開始"}
        >
          {#if active}<Square aria-hidden="true" />{:else}<Mic aria-hidden="true" />{/if}
        </button>
        <span>{active ? (status?.phase === "finalizing" || actionBusy ? "保存中…" : "録音を停止") : "録音を開始"}</span>
        {#if active}
          <button class="desktop-discard" type="button" onclick={() => cancelDialogOpen = true} disabled={actionBusy || status?.phase === "finalizing"}>破棄</button>
        {/if}
      </div>
    </div>
  </section>
  {#if active}
  <div class:active class="recorder mobile-recorder" role="status">
    <strong>{formatTimer(status?.elapsedMs ?? 0)}</strong>
    <span class="mobile-recording-state">
      <span class="record-dot" aria-hidden="true"></span>
      {status?.voiceActivity === "speechDetected" ? "音声を検出中" : "録音中"}
    </span>
  </div>
  {/if}

  <Popover bind:open={settingsPopoverOpen}>
    <PopoverTrigger>
      {#snippet child({ props })}
        <button
          {...props}
          class="recording-settings-summary"
          type="button"
          disabled={active || disabled}
        >
          <SlidersHorizontal class="settings-summary-icon" aria-hidden="true" />
          <span class="settings-summary-copy">
            <strong>入力設定</strong>
            <small>{inputSettingsSummary}</small>
          </span>
          <span class="settings-summary-action">設定を変更</span>
        </button>
      {/snippet}
    </PopoverTrigger>
    <PopoverContent class="recording-settings-popover" align="center" sideOffset={8}>
      <div class="recording-input-settings" aria-label="入力設定">
  <div class="sources" aria-disabled={active || disabled}>
    <div class="source">
      <label class="source-toggle">
        <Checkbox bind:checked={microphone} disabled={!capabilities.microphoneSupported || active || disabled} />
        <Mic aria-hidden="true" />
        <span>マイク</span>
      </label>
      {#if capabilities.microphoneDevices.length > 0}
        <Select
          type="single"
          value={toDeviceSelectValue(microphoneDeviceId)}
          onValueChange={(value) => microphoneDeviceId = fromDeviceSelectValue(value)}
          disabled={!microphone || active || disabled}
        >
          <SelectTrigger aria-label="マイクデバイス" class="source-select">
            <span class="select-value" title={selectedMicrophoneLabel}>{selectedMicrophoneLabel}</span>
          </SelectTrigger>
          <SelectContent>
            {#each microphoneOptions as option (option.value)}
              <SelectItem value={toDeviceSelectValue(option.value)}>{option.label}</SelectItem>
            {/each}
          </SelectContent>
        </Select>
      {/if}
      <div
        class:live={metering && microphone}
        class="meter microphone-meter"
        role="meter"
        aria-label="マイク入力レベル"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={microphoneMeterPercent}
      ><span style:width={`${microphoneMeterPercent}%`}></span></div>
    </div>

    <div class="source">
      <label class="source-toggle">
        <Checkbox bind:checked={systemAudio} disabled={!capabilities.systemAudioSupported || active || disabled} />
        <MonitorSpeaker aria-hidden="true" />
        <span>システム音声</span>
      </label>
      {#if capabilities.systemDevices.length > 0}
        <Select
          type="single"
          value={toDeviceSelectValue(systemDeviceId)}
          onValueChange={(value) => systemDeviceId = fromDeviceSelectValue(value)}
          disabled={!systemAudio || active || disabled}
        >
          <SelectTrigger aria-label="システム音声デバイス" class="source-select">
            <span class="select-value" title={selectedSystemLabel}>{selectedSystemLabel}</span>
          </SelectTrigger>
          <SelectContent>
            {#each systemOptions as option (option.value)}
              <SelectItem value={toDeviceSelectValue(option.value)}>{option.label}</SelectItem>
            {/each}
          </SelectContent>
        </Select>
      {/if}
      <div
        class:live={metering && systemAudio}
        class="meter system-meter"
        role="meter"
        aria-label="システム音声レベル"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={systemMeterPercent}
      ><span style:width={`${systemMeterPercent}%`}></span></div>
      {#if capabilities.platform === "android" && systemAudio && !active && status?.warning}
        <small class="monitor-note">{status.warning}</small>
      {/if}
    </div>
  </div>

  <div class:unavailable={vadAvailability === "unavailable"} class="vad-setting">
    <span class="setting-title">無音自動停止</span>
    <Select
      type="single"
      value={vadPreset}
      onValueChange={changeVadPreset}
      disabled={vadAvailability !== "available" || active || disabled || vadPresetBusy}
    >
      <SelectTrigger aria-label="録音中の音声検出感度" class="source-select vad-select">
        <span class="select-value" title={selectedVadLabel}>{selectedVadLabel}</span>
      </SelectTrigger>
      <SelectContent>
        {#each VAD_PRESET_OPTIONS as option (option.value)}
          <SelectItem value={option.value}>{option.label}</SelectItem>
        {/each}
      </SelectContent>
    </Select>
    {#if vadAvailability === "preparing"}
      <small>音声検出を準備中</small>
    {:else if vadAvailability === "unavailable"}
      <small>この端末では無音自動停止を利用できません</small>
    {/if}
  </div>
      </div>
    </PopoverContent>
  </Popover>

  </div>

  {#if active && status?.warning}
    <p class="capture-warning" role="status">{status.warning}</p>
  {/if}
  {#if active}
    <button
      class="mobile-discard-button"
      type="button"
      onclick={() => cancelDialogOpen = true}
      disabled={actionBusy || status?.phase === "finalizing"}
    >
      <Trash2 aria-hidden="true" /><span>録音を破棄</span>
    </button>
  {/if}
  <button
    class:active
    class="mobile-record-toggle"
    type="button"
    onclick={() => {
      if (consumeMobileAction()) return;
      return active ? stop() : start();
    }}
    disabled={active ? actionBusy || status?.phase === "finalizing" : !canStart}
    aria-label={active ? (status?.phase === "finalizing" || actionBusy ? "録音を保存中" : "録音を停止") : "録音を開始"}
  >
    <span class:visible={!active} class="mobile-action-icon start-icon"><Mic aria-hidden="true" /></span>
    <span class:visible={active} class="mobile-action-icon stop-icon"><Square aria-hidden="true" /></span>
  </button>
{/if}

<AlertDialog bind:open={cancelDialogOpen}>
  <AlertDialogContent>
    <AlertDialogHeader>
      <AlertDialogTitle>録音を破棄しますか？</AlertDialogTitle>
      <AlertDialogDescription>録音中の音声は保存されず、復元できません。</AlertDialogDescription>
    </AlertDialogHeader>
    <AlertDialogFooter>
      <AlertDialogCancel>録音を続ける</AlertDialogCancel>
      <AlertDialogAction variant="destructive" onclick={cancel}>破棄</AlertDialogAction>
    </AlertDialogFooter>
  </AlertDialogContent>
</AlertDialog>

<AlertDialog bind:open={discardDialogOpen}>
  <AlertDialogContent>
    <AlertDialogHeader>
      <AlertDialogTitle>未完了の録音を削除しますか？</AlertDialogTitle>
      <AlertDialogDescription>取得済みの音声も削除され、この操作は取り消せません。</AlertDialogDescription>
    </AlertDialogHeader>
    <AlertDialogFooter>
      <AlertDialogCancel>キャンセル</AlertDialogCancel>
      <AlertDialogAction
        variant="destructive"
        disabled={actionBusy}
        onclick={() => pendingDiscard && discard(pendingDiscard)}
      >削除</AlertDialogAction>
    </AlertDialogFooter>
  </AlertDialogContent>
</AlertDialog>

<style>
  .placeholder,
  .limitation { margin: 18px 0 0; color: var(--muted-foreground); font-size: 0.86rem; }
  .limitation { display: flex; align-items: center; gap: 8px; padding: 11px 12px; border-radius: var(--radius-control); color: var(--warning-foreground); background: color-mix(in oklch, var(--warning) 16%, var(--background)); }
  .limitation :global(svg) { width: 17px; height: 17px; flex: 0 0 auto; }
  .recovery-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .recovery-row div { display: flex; gap: 7px; }
  .recording-console { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 0.72fr); align-items: stretch; }
  .desktop-recording-hero { grid-column: 1 / -1; min-width: 0; padding: 10px 24px 12px; }
  .recording-console.active .desktop-recording-hero { padding: 16px 24px 21px; }
  .desktop-recording-status { display: flex; min-height: 0; align-items: center; justify-content: center; gap: 9px; color: var(--muted-foreground); font-size: 0.8rem; font-weight: 680; }
  .desktop-recording-status.active { color: var(--foreground); }
  .desktop-recording-status.active { min-height: 24px; }
  .desktop-recording-status strong { margin-left: 3px; font-size: 0.83rem; font-variant-numeric: tabular-nums; letter-spacing: 0.04em; }
  .waveform-stage { position: relative; min-height: 94px; margin-top: 2px; padding: 8px 0 14px; }
  .recording-console.active .waveform-stage { min-height: 119px; padding: 13px 0 22px; }
  .desktop-record-controls { position: absolute; z-index: 1; top: 2px; left: 50%; display: flex; width: 142px; align-items: center; flex-direction: column; gap: 7px; transform: translateX(-50%); color: var(--foreground); font-size: 0.78rem; font-weight: 720; }
  .desktop-discard { padding: 0; border: 0; color: var(--muted-foreground); background: transparent; font: inherit; font-size: 0.72rem; cursor: pointer; }
  .desktop-discard:hover:not(:disabled) { color: var(--destructive); }
  .recording-settings-summary { display: grid; grid-column: 1 / -1; width: 100%; min-width: 0; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 11px; padding: 10px 14px; border: 0; border-radius: var(--radius-control); color: var(--foreground); background: color-mix(in oklch, var(--card) 82%, transparent); cursor: pointer; font: inherit; text-align: left; }
  .recording-settings-summary:hover { background: color-mix(in oklch, var(--muted) 48%, transparent); }
  .recording-settings-summary:disabled { cursor: not-allowed; opacity: 0.58; }
  .settings-summary-copy { display: flex; min-width: 0; align-items: baseline; gap: 14px; }
  .recording-settings-summary strong { flex: none; font-size: 0.8rem; font-weight: 730; }
  .recording-settings-summary small { overflow: hidden; color: var(--muted-foreground); font-size: 0.7rem; text-overflow: ellipsis; white-space: nowrap; }
  .recording-settings-summary :global(.settings-summary-icon) { width: 17px; height: 17px; color: var(--primary); }
  .settings-summary-action { padding: 4px 8px; border-radius: 999px; color: var(--primary); background: color-mix(in oklch, var(--primary) 9%, transparent); font-size: 0.68rem; font-weight: 700; white-space: nowrap; }
  .recording-input-settings { display: grid; grid-column: 1 / -1; grid-template-columns: repeat(2, minmax(0, 1fr)) 176px; }
  :global(.recording-settings-popover) { width: min(656px, calc(100vw - 32px)); max-height: min(560px, calc(100vh - 32px)); padding: 0; overflow-x: hidden; overflow-y: auto; }
  .sources { display: contents; }
  .source { display: grid; align-content: start; gap: 10px; min-width: 0; padding: 18px; border-right: 1px solid var(--border); background: transparent; }
  .source-toggle,
  .setting-title { display: flex; min-height: 32px; align-items: center; gap: 8px; margin: 0; font-size: 0.82rem; font-weight: 700; line-height: 1.3; }
  .source-toggle :global(> svg) { width: 17px; height: 17px; color: var(--muted-foreground); }
  :global(.source-select) { width: 100%; min-width: 0; max-width: 100%; overflow: hidden; }
  :global(.vad-select) { width: 140px; max-width: 100%; justify-self: start; }
  .select-value { display: block; min-width: 0; max-width: 100%; flex: 1 1 auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meter { --meter-color: var(--audio-microphone); --meter-gradient: linear-gradient(90deg, color-mix(in oklch, var(--meter-color) 42%, var(--card)) 0%, color-mix(in oklch, var(--meter-color) 76%, var(--card)) 52%, var(--meter-color) 100%); height: 5px; overflow: hidden; border-radius: 99px; background: var(--audio-silent); }
  .meter.system-meter { --meter-color: var(--audio-system); }
  .meter.live { background: color-mix(in oklch, var(--meter-color) 14%, var(--audio-silent)); }
  .meter span { display: block; height: 100%; border-radius: inherit; background: var(--meter-gradient); transition: width 90ms linear; }
  .monitor-note { color: var(--muted-foreground); font-size: 0.7rem; }
  .recorder { display: flex; align-items: center; gap: 9px; margin: 0; padding: 16px 20px 16px 4px; background: transparent; }
  .vad-setting { display: grid; box-sizing: border-box; grid-template-columns: 1fr; align-content: start; gap: 10px; min-width: 0; margin: 0; padding: 18px; color: var(--foreground); border: 0; }
  .vad-setting.unavailable { color: var(--muted-foreground); }
  .vad-setting small { color: var(--muted-foreground); font-size: 0.68rem; font-weight: 500; line-height: 1.4; }
  .recorder strong { font-size: 1.35rem; font-variant-numeric: tabular-nums; letter-spacing: 0.04em; }
  .record-dot { width: 8px; height: 8px; flex: none; border-radius: 50%; background: var(--audio-microphone); }
  .recorder.active .record-dot { background: var(--audio-recording); box-shadow: 0 0 0 5px color-mix(in oklch, var(--audio-recording) 14%, transparent); }
  .capture-warning { margin: 10px 0 0; padding: 10px 12px; border-radius: var(--radius-control); color: var(--warning-foreground); background: color-mix(in oklch, var(--warning) 16%, var(--background)); font-size: 0.84rem; }
  .desktop-start { display: grid; width: 72px; height: 72px; place-items: center; padding: 0; border: 7px solid var(--card); border-radius: 50%; color: var(--primary-foreground); background: var(--gradient-primary); box-shadow: 0 0 0 1px color-mix(in oklch, var(--primary) 22%, var(--border)), var(--shadow-active); cursor: pointer; }
  .recording-console.active .desktop-start { width: 82px; height: 82px; }
  .desktop-start.active { background: var(--audio-recording); box-shadow: 0 0 0 1px color-mix(in oklch, var(--audio-recording) 26%, var(--border)), var(--shadow-active); }
  .desktop-start:hover:not(:disabled) { background: var(--gradient-primary-hover); }
  .desktop-start.active:hover:not(:disabled) { background: color-mix(in oklch, var(--audio-recording) 88%, black); }
  .desktop-start:focus-visible { outline: 3px solid var(--focus-ring); outline-offset: 3px; }
  .desktop-start:disabled { cursor: not-allowed; opacity: 0.52; }
  .desktop-start :global(svg) { width: 31px; height: 31px; stroke-width: 2; }
  .desktop-start.active :global(svg) { width: 23px; height: 23px; fill: currentColor; }
  .mobile-recorder,
  .mobile-record-toggle,
  .mobile-discard-button { display: none; }
  @media (max-width: 600px) {
    .limitation { display: none; }
    .recording-console { --settings-motion: 300ms cubic-bezier(0.22, 1, 0.36, 1); display: block; }
    .desktop-recording-hero { display: none; }
    .recording-settings-summary { min-height: 48px; padding: 9px 14px; }
    .settings-summary-copy { display: grid; gap: 2px; }
    .recording-settings-summary strong { font-size: 0.82rem; }
    .recording-settings-summary small { font-size: 0.64rem; }
    .recording-input-settings { grid-template-columns: 1fr; }
    .mobile-recorder { display: flex; min-height: 52px; flex-direction: row; justify-content: flex-start; gap: 9px; margin: 0; padding: 8px 2px; background: transparent; }
    .mobile-recorder strong { font-size: clamp(2.55rem, 13vw, 3.35rem); font-weight: 760; line-height: 1; letter-spacing: 0.025em; }
    .mobile-recording-state { display: flex; align-items: center; gap: 7px; color: var(--muted-foreground); font-size: 0.82rem; }
    .mobile-recording-state .record-dot { width: 8px; height: 8px; background: var(--audio-microphone); }
    .mobile-recorder.active .record-dot { background: var(--audio-recording); }
    .sources { display: contents; }
    .source { display: grid; grid-template-columns: minmax(0, 1fr) minmax(92px, 42%); align-items: center; gap: 9px 12px; min-height: 58px; padding: 9px 2px; border: 0; border-bottom: 1px solid var(--border); background: transparent; transition: min-height var(--settings-motion), padding var(--settings-motion), gap var(--settings-motion); }
    .source:first-child,
    .source:last-child { grid-area: auto; }
    .source-toggle,
    .setting-title { min-height: 34px; gap: 10px; font-size: 0.88rem; font-weight: 720; transition: min-height var(--settings-motion), gap var(--settings-motion), font-size var(--settings-motion); }
    .source-toggle :global(button) { width: 34px; height: 34px; border-radius: 7px; transition: width var(--settings-motion), height var(--settings-motion), border-radius var(--settings-motion); }
    .source-toggle :global(svg) { width: 20px; height: 20px; transition: width var(--settings-motion), height var(--settings-motion); }
    .source-toggle :global(> svg) { display: none; }
    .source :global(.source-select) { width: calc(100% - 44px); grid-column: 1 / -1; margin-left: 44px; transition: width var(--settings-motion), margin-left var(--settings-motion), font-size var(--settings-motion); }
    .meter { height: 6px; margin-left: 0; background: repeating-linear-gradient(90deg, var(--audio-silent) 0 9px, transparent 9px 13px); transition: width var(--settings-motion), height var(--settings-motion); }
    .meter.live { background: repeating-linear-gradient(90deg, color-mix(in oklch, var(--meter-color) 18%, var(--audio-silent)) 0 9px, transparent 9px 13px); }
    .meter span { background: var(--meter-gradient); -webkit-mask: repeating-linear-gradient(90deg, #000 0 9px, transparent 9px 13px); mask: repeating-linear-gradient(90deg, #000 0 9px, transparent 9px 13px); }
    .monitor-note { grid-column: 1 / -1; margin-left: 44px; transition: margin-left var(--settings-motion), font-size var(--settings-motion); }
    .vad-setting { grid-template-columns: minmax(0, 1fr) 132px; align-items: center; justify-content: stretch; gap: 9px 12px; min-height: 58px; margin: 0; padding: 9px 2px; border: 0; color: var(--foreground); transition: min-height var(--settings-motion), padding var(--settings-motion), gap var(--settings-motion); }
    :global(.vad-select) { width: 132px; justify-self: end; }
    .vad-setting small { grid-column: 1 / -1; padding-bottom: 4px; transition: padding var(--settings-motion), font-size var(--settings-motion), line-height var(--settings-motion); }
    :global(.recording-settings-popover) { width: calc(100vw - 24px); max-height: min(70vh, 560px); }
    .desktop-start { display: none; }
    .mobile-record-toggle { display: flex; position: fixed; z-index: 30; left: 0; bottom: 0; width: 100vw; height: 50vw; align-items: center; justify-content: center; padding: 0 0 env(safe-area-inset-bottom, 0px); overflow: hidden; border: 0; border-radius: 50vw 50vw 0 0; color: var(--primary-foreground); background: var(--gradient-primary); box-shadow: 0 -12px 34px color-mix(in oklch, var(--primary) 24%, transparent); cursor: pointer; transition: background 360ms cubic-bezier(0.22, 1, 0.36, 1), box-shadow 360ms cubic-bezier(0.22, 1, 0.36, 1), opacity 180ms ease; -webkit-tap-highlight-color: transparent; }
    .mobile-record-toggle:active:not(:disabled) { background: var(--gradient-primary-hover); }
    .mobile-record-toggle.active { background: var(--audio-recording); box-shadow: 0 -8px 28px color-mix(in oklch, var(--audio-recording) 24%, transparent); }
    .mobile-record-toggle.active:active:not(:disabled) { background: color-mix(in oklch, var(--audio-recording) 84%, black); }
    .mobile-record-toggle:disabled { opacity: 0.58; }
    .mobile-action-icon { display: grid; position: absolute; inset: 0; place-items: center; padding-bottom: env(safe-area-inset-bottom, 0px); opacity: 0; transform: scale(0.72) rotate(-12deg); transition: opacity 220ms ease, transform 360ms cubic-bezier(0.22, 1, 0.36, 1); pointer-events: none; }
    .mobile-action-icon.visible { opacity: 1; transform: scale(1) rotate(0deg); }
    .mobile-action-icon :global(svg) { width: clamp(68px, 22vw, 92px); height: clamp(68px, 22vw, 92px); stroke-width: 2.2; }
    .mobile-action-icon.stop-icon :global(svg) { fill: currentColor; stroke-width: 1.4; }
    .mobile-discard-button { display: flex; position: fixed; z-index: 31; left: 50%; bottom: calc(50vw + 12px); min-height: 48px; align-items: center; justify-content: center; gap: 8px; padding: 0 18px; transform: translateX(-50%); border: 1px solid #d8a5a1; border-radius: 999px; color: #a93631; background: color-mix(in srgb, white 94%, #fff2f1); box-shadow: 0 5px 18px rgb(44 31 30 / 10%); font: inherit; font-size: 0.86rem; font-weight: 720; white-space: nowrap; }
    .mobile-discard-button:active:not(:disabled) { background: #fff0ef; }
    .mobile-discard-button:disabled { opacity: 0.48; }
    .mobile-discard-button :global(svg) { width: 18px; height: 18px; }
    .recovery-row { align-items: flex-start; flex-direction: column; }
  }

  @media (max-width: 600px) and (prefers-reduced-motion: reduce) {
    .recording-console { --settings-motion: 0ms linear; }
    .mobile-record-toggle,
    .mobile-action-icon { transition-duration: 0ms; }
  }
</style>
