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
  import { Select } from "@mutsuna/ui/select";
  import Info from "@lucide/svelte/icons/info";
  import Mic from "@lucide/svelte/icons/mic";
  import Square from "@lucide/svelte/icons/square";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { VAD_PRESET_OPTIONS, type VadPreset } from "../providers";
  import type { SelectedAudioFile } from "../types/transcript";
  import type {
    RecoverableRecording,
    RecordingCapabilities,
    RecordingStatus,
    StopRecordingResult
  } from "../types/recording";

  interface Props {
    disabled?: boolean;
    onAudioReady: (audio: SelectedAudioFile) => void;
    onBusyChange: (busy: boolean) => void;
    onMessage: (message: string) => void;
    onError: (message: string) => void;
  }

  let {
    disabled = false,
    onAudioReady,
    onBusyChange,
    onMessage,
    onError
  }: Props = $props();

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

  const active = $derived(
    status?.phase === "starting" || status?.phase === "recording" || status?.phase === "finalizing"
  );
  const canStart = $derived(
    Boolean(capabilities?.supported) && (microphone || systemAudio) && !disabled && !actionBusy && !active
  );
  const metering = $derived(active || monitoring);
  const microphoneMeterPercent = $derived(toMeterPercent(status?.microphoneLevel ?? 0, metering && microphone));
  const systemMeterPercent = $derived(toMeterPercent(status?.systemLevel ?? 0, metering && systemAudio));
  const voiceActivityLabel = $derived(
    status?.voiceActivity === "speechDetected"
      ? "Speech detected"
      : status?.voiceActivity === "listening"
        ? "Listening…"
        : "VAD unavailable"
  );
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

  function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    return "録音中に予期しないエラーが発生しました。";
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

  async function acceptStatus(nextStatus: RecordingStatus) {
    status = nextStatus;
    await deliverCompletedRecording(nextStatus);
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
        const [nextCapabilities, nextStatus, nextRecoverable, nextVadPreset] = await Promise.all([
          invoke<RecordingCapabilities>("get_recording_capabilities"),
          invoke<RecordingStatus>("get_recording_status"),
          invoke<RecoverableRecording[]>("list_recoverable_recordings"),
          invoke<VadPreset>("get_vad_preset")
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
        await acceptStatus(nextStatus);
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
    const ready = !loading && Boolean(capabilities?.supported) && !active && !monitorSuspended && (microphone || systemAudio);
    const request = currentRequest();
    const revision = ++monitorRevision;
    if (!ready) {
      monitoring = false;
      void invoke("stop_recording_monitor").catch(() => {});
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
      void invoke("stop_recording_monitor").catch(() => {});
    };
  });

  // Androidまたはイベント購読失敗時だけ、録音中に限定して状態を確認する。
  $effect(() => {
    if ((!active && !monitoring) || (capabilities?.platform !== "android" && statusEventsAvailable)) return;
    const timer = window.setInterval(() => {
      void refreshStatus().catch((error) => onError(errorText(error)));
    }, 120);
    return () => window.clearInterval(timer);
  });

  async function start() {
    actionBusy = true;
    monitorSuspended = true;
    monitorRevision += 1;
    monitoring = false;
    onMessage("");
    onError("");
    try {
      await invoke("stop_recording_monitor");
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

  <div class:active class="recorder mobile-recorder" role="status">
    <strong>{formatTimer(status?.elapsedMs ?? 0)}</strong>
    <span class="mobile-recording-state">
      <span class="record-dot" aria-hidden="true"></span>
      {active ? (status?.voiceActivity === "speechDetected" ? "音声を検出中" : "録音中") : "録音待機中"}
    </span>
  </div>

  <div class="sources" aria-disabled={active || disabled}>
    <div class="source">
      <label class="source-toggle">
        <Checkbox bind:checked={microphone} disabled={!capabilities.microphoneSupported || active || disabled} />
        <span>マイク</span>
      </label>
      {#if capabilities.microphoneDevices.length > 0}
        <Select bind:value={microphoneDeviceId} options={microphoneOptions} disabled={!microphone || active || disabled} ariaLabel="マイクデバイス" class="source-select" />
      {/if}
      <div
        class:live={metering && microphone}
        class="meter"
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
        <span>システム音声</span>
      </label>
      {#if capabilities.systemDevices.length > 0}
        <Select bind:value={systemDeviceId} options={systemOptions} disabled={!systemAudio || active || disabled} ariaLabel="システム音声デバイス" class="source-select" />
      {/if}
      <div
        class:live={metering && systemAudio}
        class="meter"
        role="meter"
        aria-label="システム音声レベル"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={systemMeterPercent}
      ><span style:width={`${systemMeterPercent}%`}></span></div>
      {#if capabilities.platform === "android" && systemAudio && !active}
        <small class="monitor-note">システム音声は録音開始後に確認できます</small>
      {/if}
    </div>
  </div>

  <div class="vad-setting">
    <span>音声検出</span>
    <Select
      value={vadPreset}
      options={VAD_PRESET_OPTIONS}
      onValueChange={changeVadPreset}
      disabled={active || disabled || vadPresetBusy}
      ariaLabel="録音中の音声検出感度"
    />
  </div>

  <div class:active class="recorder desktop-recorder">
    <span class="record-dot" aria-hidden="true"></span>
    <strong>{formatTimer(status?.elapsedMs ?? 0)}</strong>
    <small>48 kHz · mono · AAC-LC · 64 kbps</small>
  </div>
  {#if active}
    <p class:speaking={status?.voiceActivity === "speechDetected"} class="voice-activity" role="status">
      <span aria-hidden="true"></span>{voiceActivityLabel}
    </p>
  {/if}

  {#if active && status?.warning}
    <p class="capture-warning" role="status">{status.warning}</p>
  {/if}

  <div class="record-actions">
    {#if active}
      <Button variant="destructive" size="lg" type="button" onclick={stop} disabled={actionBusy || status?.phase === "finalizing"} loading={actionBusy || status?.phase === "finalizing"}>
        {status?.phase === "finalizing" || actionBusy ? "M4Aを確定中…" : "録音を停止"}
      </Button>
      <Button variant="outline" size="lg" type="button" onclick={() => cancelDialogOpen = true} disabled={actionBusy || status?.phase === "finalizing"}>破棄</Button>
    {:else}
      <span class="desktop-start"><Button size="lg" type="button" onclick={start} disabled={!canStart}>録音を開始</Button></span>
    {/if}
  </div>
  {#if active}
    <button
      class="mobile-discard-button"
      type="button"
      onclick={() => cancelDialogOpen = true}
      disabled={actionBusy || status?.phase === "finalizing"}
    >
      <Trash2 aria-hidden="true" /><span>録音を破棄</span>
    </button>
    <button
      class="mobile-stop-button"
      type="button"
      onclick={stop}
      disabled={actionBusy || status?.phase === "finalizing"}
      aria-label={status?.phase === "finalizing" || actionBusy ? "録音を保存中" : "録音を停止"}
    >
      <Square aria-hidden="true" />
    </button>
  {:else}
    <button class="mobile-record-button" type="button" onclick={start} disabled={!canStart} aria-label="録音を開始">
      <Mic aria-hidden="true" />
    </button>
  {/if}
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
  .limitation { margin: 18px 0 0; color: #647068; font-size: 0.86rem; }
  .limitation { display: flex; align-items: center; gap: 8px; padding: 11px 12px; border-radius: 9px; color: #7a4c20; background: #fff4e8; }
  .limitation :global(svg) { width: 17px; height: 17px; flex: 0 0 auto; }
  .recovery-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .recovery-row div { display: flex; gap: 7px; }
  .sources { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; margin-top: 20px; }
  .source { display: grid; gap: 10px; padding: 15px; border: 1px solid #dce3de; border-radius: 12px; background: #f8faf8; }
  .source-toggle { display: flex; align-items: center; gap: 8px; margin: 0; font-size: 0.9rem; font-weight: 750; }
  .meter { height: 5px; overflow: hidden; border-radius: 99px; background: #dfe6e1; }
  .meter.live { background: color-mix(in srgb, #2c8058 13%, #dfe6e1); }
  .meter span { display: block; height: 100%; border-radius: inherit; background: #2c8058; transition: width 90ms linear; }
  .monitor-note { color: #68746c; font-size: 0.7rem; }
  .recorder { display: flex; align-items: center; gap: 10px; margin-top: 14px; padding: 14px 16px; border-radius: 12px; background: #f3f6f4; }
  .vad-setting { display: grid; grid-template-columns: auto minmax(180px, 280px); align-items: center; justify-content: end; gap: 10px; margin-top: 12px; color: #68746c; font-size: 0.82rem; }
  .recorder strong { font-size: 1.35rem; font-variant-numeric: tabular-nums; letter-spacing: 0.04em; }
  .recorder small { margin-left: auto; color: #68746c; }
  .record-dot { width: 10px; height: 10px; border-radius: 50%; background: #9ca7a0; }
  .recorder.active .record-dot { background: #dc4438; box-shadow: 0 0 0 5px rgb(220 68 56 / 12%); }
  .capture-warning { margin: 10px 0 0; padding: 10px 12px; border-radius: 9px; color: #7a4c20; background: #fff4e8; font-size: 0.84rem; }
  .voice-activity { display: flex; align-items: center; gap: 7px; margin: 9px 2px 0; color: #68746c; font-size: 0.82rem; }
  .voice-activity span { width: 7px; height: 7px; border-radius: 50%; background: #9ca7a0; }
  .voice-activity.speaking { color: #256b4a; font-weight: 700; }
  .voice-activity.speaking span { background: #2c8058; box-shadow: 0 0 0 4px rgb(44 128 88 / 12%); }
  .record-actions { display: flex; justify-content: flex-end; gap: 9px; margin-top: 14px; }
  .mobile-recorder,
  .mobile-record-button,
  .mobile-stop-button,
  .mobile-discard-button { display: none; }
  @media (max-width: 600px) {
    .limitation { margin-top: 4px; padding: 8px 0; border-radius: 0; color: #68746c; background: transparent; font-size: 0.76rem; }
    .mobile-recorder { display: flex; min-height: 132px; flex-direction: column; justify-content: center; gap: 9px; margin: 0; padding: 4px 0 10px; background: transparent; }
    .mobile-recorder strong { font-size: clamp(2.55rem, 13vw, 3.35rem); font-weight: 760; line-height: 1; letter-spacing: 0.025em; }
    .mobile-recording-state { display: flex; align-items: center; gap: 7px; color: #4f5c54; font-size: 0.82rem; }
    .mobile-recording-state .record-dot { width: 8px; height: 8px; background: #2c8058; }
    .mobile-recorder.active .record-dot { background: #dc4438; }
    .desktop-recorder { display: none; }
    .sources { grid-template-columns: 1fr; gap: 0; margin-top: 0; border-top: 1px solid #e4e9e5; }
    .source { display: grid; grid-template-columns: 1fr; gap: 10px; min-height: 94px; padding: 16px 2px 14px; border: 0; border-bottom: 1px solid #e4e9e5; border-radius: 0; background: transparent; }
    .source-toggle { gap: 13px; font-size: 1.02rem; }
    .source-toggle :global(button) { width: 46px; height: 46px; border-radius: 10px; }
    .source-toggle :global(svg) { width: 27px; height: 27px; }
    :global(.source-select) { margin-left: 59px; }
    .meter { height: 7px; margin-left: 59px; background: repeating-linear-gradient(90deg, #e0e5e1 0 18px, transparent 18px 23px); }
    .meter.live { background: repeating-linear-gradient(90deg, #d8e7de 0 18px, transparent 18px 23px); }
    .meter span { background: repeating-linear-gradient(90deg, #2c8058 0 18px, transparent 18px 23px); }
    .monitor-note { margin-left: 59px; }
    .vad-setting { grid-template-columns: 1fr minmax(116px, 145px); justify-content: stretch; min-height: 68px; margin: 0; padding: 9px 2px; border-bottom: 1px solid #e4e9e5; color: #243129; font-size: 0.98rem; font-weight: 720; }
    .record-actions { display: none; }
    .desktop-start { display: none; }
    .mobile-record-button,
    .mobile-stop-button { display: flex; position: fixed; z-index: 30; left: 0; bottom: 0; width: 100vw; height: 50vw; align-items: center; justify-content: center; padding: 0 0 env(safe-area-inset-bottom, 0px); border: 0; border-radius: 50vw 50vw 0 0; color: white; box-shadow: 0 -8px 28px rgb(44 128 88 / 18%); cursor: pointer; -webkit-tap-highlight-color: transparent; }
    .mobile-record-button { background: #2c8058; }
    .mobile-record-button:active:not(:disabled) { background: #236a49; }
    .mobile-stop-button { background: #d64b43; box-shadow: 0 -8px 28px rgb(177 48 42 / 20%); }
    .mobile-stop-button:active:not(:disabled) { background: #b83b35; }
    .mobile-record-button:disabled,
    .mobile-stop-button:disabled { opacity: 0.58; }
    .mobile-record-button :global(svg),
    .mobile-stop-button :global(svg) { width: clamp(68px, 22vw, 92px); height: clamp(68px, 22vw, 92px); stroke-width: 2.2; }
    .mobile-stop-button :global(svg) { fill: currentColor; stroke-width: 1.4; }
    .mobile-discard-button { display: flex; position: fixed; z-index: 31; left: 50%; bottom: calc(50vw + 12px); min-height: 48px; align-items: center; justify-content: center; gap: 8px; padding: 0 18px; transform: translateX(-50%); border: 1px solid #d8a5a1; border-radius: 999px; color: #a93631; background: color-mix(in srgb, white 94%, #fff2f1); box-shadow: 0 5px 18px rgb(44 31 30 / 10%); font: inherit; font-size: 0.86rem; font-weight: 720; white-space: nowrap; }
    .mobile-discard-button:active:not(:disabled) { background: #fff0ef; }
    .mobile-discard-button:disabled { opacity: 0.48; }
    .mobile-discard-button :global(svg) { width: 18px; height: 18px; }
    .recovery-row { align-items: flex-start; flex-direction: column; }
  }
</style>
