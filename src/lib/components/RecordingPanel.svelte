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
  import RecordingHistory from "./RecordingHistory.svelte";
  import { VAD_PRESET_OPTIONS, type VadPreset } from "../providers";
  import type { SelectedAudioFile } from "../types/transcript";
  import type {
    RecoverableRecording,
    RecordedAudioSummary,
    RecordingCapabilities,
    RecordingStatus,
    StopRecordingResult
  } from "../types/recording";

  interface Props {
    disabled?: boolean;
    transcriptRevision?: number;
    onAudioReady: (audio: SelectedAudioFile) => void;
    onBusyChange: (busy: boolean) => void;
    onMessage: (message: string) => void;
    onError: (message: string) => void;
  }

  let {
    disabled = false,
    transcriptRevision = 0,
    onAudioReady,
    onBusyChange,
    onMessage,
    onError
  }: Props = $props();

  let capabilities = $state<RecordingCapabilities | null>(null);
  let status = $state<RecordingStatus | null>(null);
  let recoverable = $state<RecoverableRecording[]>([]);
  let recordedAudio = $state<RecordedAudioSummary[]>([]);
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
  let observedTranscriptRevision = $state(0);
  let statusEventsAvailable = $state(false);
  let vadPreset = $state<VadPreset>("standard");
  let vadPresetBusy = $state(false);

  const active = $derived(
    status?.phase === "starting" || status?.phase === "recording" || status?.phase === "finalizing"
  );
  const canStart = $derived(
    Boolean(capabilities?.supported) && (microphone || systemAudio) && !disabled && !actionBusy && !active
  );
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

  async function deliverCompletedRecording(nextStatus: RecordingStatus) {
    if (nextStatus.phase !== "completed" || !nextStatus.outputPath || deliveredOutput === nextStatus.outputPath) return;
    const audio = await invoke<SelectedAudioFile | null>("get_recorded_audio");
    if (audio) {
      deliveredOutput = nextStatus.outputPath;
      onAudioReady(audio);
      onMessage(stopMessage(nextStatus.stopReason));
      await refreshRecordedAudio();
    }
  }

  async function refreshRecordedAudio() {
    try {
      recordedAudio = await invoke<RecordedAudioSummary[]>("list_recorded_audio");
    } catch (error) {
      onError(errorText(error));
      recordedAudio = [];
    }
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

  $effect(() => {
    onBusyChange(active || actionBusy);
  });

  $effect(() => {
    if (loading || transcriptRevision === observedTranscriptRevision) return;
    observedTranscriptRevision = transcriptRevision;
    void refreshRecordedAudio();
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
        const [nextCapabilities, nextStatus, nextRecoverable, nextRecordedAudio, nextVadPreset] = await Promise.all([
          invoke<RecordingCapabilities>("get_recording_capabilities"),
          invoke<RecordingStatus>("get_recording_status"),
          invoke<RecoverableRecording[]>("list_recoverable_recordings"),
          invoke<RecordedAudioSummary[]>("list_recorded_audio").catch((error) => {
            onError(errorText(error));
            return [];
          }),
          invoke<VadPreset>("get_vad_preset")
        ]);
        if (cancelled) return;
        capabilities = nextCapabilities;
        recoverable = nextRecoverable;
        recordedAudio = nextRecordedAudio;
        microphone = nextCapabilities.microphoneSupported;
        systemAudio = nextCapabilities.systemAudioSupported;
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

  // Androidまたはイベント購読失敗時だけ、録音中に限定して状態を確認する。
  $effect(() => {
    if (!active || (capabilities?.platform !== "android" && statusEventsAvailable)) return;
    const timer = window.setInterval(() => {
      void refreshStatus().catch((error) => onError(errorText(error)));
    }, 500);
    return () => window.clearInterval(timer);
  });

  async function start() {
    actionBusy = true;
    onMessage("");
    onError("");
    try {
      status = await invoke<RecordingStatus>("start_recording", {
        request: {
          microphone,
          systemAudio,
          microphoneDeviceId: microphoneDeviceId || null,
          systemDeviceId: systemDeviceId || null
        }
      });
      deliveredOutput = "";
    } catch (error) {
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
      if (result.audio && deliveredOutput !== result.status.outputPath) {
        deliveredOutput = result.status.outputPath ?? "";
        onAudioReady(result.audio);
        await refreshRecordedAudio();
      }
      onMessage(stopMessage(result.status.stopReason));
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

  async function selectRecordedAudio(recording: RecordedAudioSummary) {
    actionBusy = true;
    try {
      const audio = await invoke<SelectedAudioFile>("select_recorded_audio", {
        recordingId: recording.id,
        meetingId: recording.meetingId
      });
      onAudioReady(audio);
      onMessage(`${recording.fileName}を文字起こし対象に選択しました。`);
    } catch (error) {
      onError(errorText(error));
      await refreshRecordedAudio();
    } finally {
      actionBusy = false;
    }
  }

  async function revealRecordedAudio(recording: RecordedAudioSummary) {
    try {
      await invoke("reveal_recorded_audio", { recordingId: recording.id });
    } catch (error) {
      onError(errorText(error));
      await refreshRecordedAudio();
    }
  }
</script>

{#if loading}
  <p class="placeholder" role="status">録音デバイスを確認しています…</p>
{:else if capabilities}
  {#if capabilities.limitation}
    <p class="limitation" role="note">{capabilities.limitation}</p>
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

  {#if !active}
    <RecordingHistory
      recordings={recordedAudio}
      {disabled}
      busy={actionBusy}
      onRefresh={refreshRecordedAudio}
      onSelect={selectRecordedAudio}
      onReveal={revealRecordedAudio}
    />
  {/if}

  <div class="sources" aria-disabled={active || disabled}>
    <div class="source">
      <label class="source-toggle">
        <Checkbox bind:checked={microphone} disabled={!capabilities.microphoneSupported || active || disabled} />
        <span>マイク</span>
      </label>
      {#if capabilities.microphoneDevices.length > 0}
        <Select bind:value={microphoneDeviceId} options={microphoneOptions} disabled={!microphone || active || disabled} ariaLabel="マイクデバイス" class="source-select" />
      {/if}
      <div class="meter" aria-label="マイク入力レベル"><span style:width={`${(status?.microphoneLevel ?? 0) * 100}%`}></span></div>
    </div>

    <div class="source">
      <label class="source-toggle">
        <Checkbox bind:checked={systemAudio} disabled={!capabilities.systemAudioSupported || active || disabled} />
        <span>システム音声</span>
      </label>
      {#if capabilities.systemDevices.length > 0}
        <Select bind:value={systemDeviceId} options={systemOptions} disabled={!systemAudio || active || disabled} ariaLabel="システム音声デバイス" class="source-select" />
      {/if}
      <div class="meter" aria-label="システム音声レベル"><span style:width={`${(status?.systemLevel ?? 0) * 100}%`}></span></div>
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

  <div class:active class="recorder">
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
      <Button size="lg" type="button" onclick={start} disabled={!canStart}>録音を開始</Button>
    {/if}
  </div>
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
        onclick={() => pendingDiscard && discard(pendingDiscard)}
      >削除</AlertDialogAction>
    </AlertDialogFooter>
  </AlertDialogContent>
</AlertDialog>

<style>
  .placeholder,
  .limitation { margin: 18px 0 0; color: #647068; font-size: 0.86rem; }
  .limitation { padding: 11px 12px; border-radius: 9px; color: #7a4c20; background: #fff4e8; }
  .recovery-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .recovery-row div { display: flex; gap: 7px; }
  .sources { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; margin-top: 20px; }
  .source { display: grid; gap: 10px; padding: 15px; border: 1px solid #dce3de; border-radius: 12px; background: #f8faf8; }
  .source-toggle { display: flex; align-items: center; gap: 8px; margin: 0; font-size: 0.9rem; font-weight: 750; }
  .meter { height: 5px; overflow: hidden; border-radius: 99px; background: #dfe6e1; }
  .meter span { display: block; height: 100%; border-radius: inherit; background: #2c8058; transition: width 100ms linear; }
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
  @media (max-width: 600px) {
    .sources { grid-template-columns: 1fr; }
    .vad-setting { grid-template-columns: 1fr; justify-content: stretch; }
    .recorder { flex-wrap: wrap; }
    .recorder small { width: 100%; margin-left: 20px; }
    .record-actions :global(button) { flex: 1; }
    .recovery-row { align-items: flex-start; flex-direction: column; }
  }
</style>
