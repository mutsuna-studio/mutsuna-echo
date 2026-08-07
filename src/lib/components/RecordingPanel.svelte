<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { formatFileSize, formatRecordedAt } from "../format";
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
  let recordedAudio = $state<RecordedAudioSummary[]>([]);
  let microphone = $state(true);
  let systemAudio = $state(true);
  let microphoneDeviceId = $state("");
  let systemDeviceId = $state("");
  let loading = $state(true);
  let actionBusy = $state(false);
  let deliveredOutput = $state("");

  const active = $derived(
    status?.phase === "starting" || status?.phase === "recording" || status?.phase === "finalizing"
  );
  const canStart = $derived(
    Boolean(capabilities?.supported) && (microphone || systemAudio) && !disabled && !actionBusy && !active
  );

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
    status = nextStatus;
    await deliverCompletedRecording(nextStatus);
    if (nextStatus.phase === "failed" && nextStatus.error) onError(nextStatus.error);
  }

  $effect(() => {
    onBusyChange(active || actionBusy);
  });

  $effect(() => {
    let cancelled = false;
    let timer: number | undefined;
    void (async () => {
      try {
        const [nextCapabilities, nextStatus, nextRecoverable, nextRecordedAudio] = await Promise.all([
          invoke<RecordingCapabilities>("get_recording_capabilities"),
          invoke<RecordingStatus>("get_recording_status"),
          invoke<RecoverableRecording[]>("list_recoverable_recordings"),
          invoke<RecordedAudioSummary[]>("list_recorded_audio").catch((error) => {
            onError(errorText(error));
            return [];
          })
        ]);
        if (cancelled) return;
        capabilities = nextCapabilities;
        status = nextStatus;
        recoverable = nextRecoverable;
        recordedAudio = nextRecordedAudio;
        microphone = nextCapabilities.microphoneSupported;
        systemAudio = nextCapabilities.systemAudioSupported;
        microphoneDeviceId = nextCapabilities.microphoneDevices.find((device) => device.isDefault)?.id ?? "";
        systemDeviceId = nextCapabilities.systemDevices.find((device) => device.isDefault)?.id ?? "";
        await deliverCompletedRecording(nextStatus);
        timer = window.setInterval(() => {
          void refreshStatus().catch((error) => onError(errorText(error)));
        }, 500);
      } catch (error) {
        onError(errorText(error));
      } finally {
        loading = false;
      }
    })();
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearInterval(timer);
    };
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

  async function stop() {
    actionBusy = true;
    onMessage("");
    onError("");
    try {
      const result = await invoke<StopRecordingResult>("stop_recording");
      status = result.status;
      if (result.audio) {
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
    if (!window.confirm("録音中の音声を保存せず破棄しますか？")) return;
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
    if (!window.confirm("この未完了録音を削除しますか？")) return;
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

  async function selectRecordedAudio(recording: RecordedAudioSummary) {
    actionBusy = true;
    try {
      const audio = await invoke<SelectedAudioFile>("select_recorded_audio", {
        recordingId: recording.id
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
</script>

{#if loading}
  <p class="placeholder" role="status">録音デバイスを確認しています…</p>
{:else if capabilities}
  {#if capabilities.limitation}
    <p class="limitation" role="note">{capabilities.limitation}</p>
  {/if}

  {#if recoverable.length > 0 && !active}
    <div class="recovery" role="alert">
      <strong>中断された録音があります</strong>
      {#each recoverable as recording (recording.sessionId)}
        <div class="recovery-row">
          <span>{new Date(recording.startedAt).toLocaleString("ja-JP")} · {formatTimer(recording.durationMs)}</span>
          <div>
            <button type="button" onclick={() => recover(recording)} disabled={actionBusy}>復旧</button>
            <button class="text-danger" type="button" onclick={() => discard(recording)} disabled={actionBusy}>削除</button>
          </div>
        </div>
      {/each}
    </div>
  {/if}

  {#if !active}
    <section class="history" aria-label="過去の録音">
      <div class="history-heading">
        <div>
          <strong>過去の録音</strong>
          <small>Music/Mutsuna Echoに保存された最新100件</small>
        </div>
        <button type="button" onclick={refreshRecordedAudio} disabled={actionBusy}>更新</button>
      </div>
      {#if recordedAudio.length > 0}
        <div class="history-list">
          {#each recordedAudio as recording (recording.id)}
            <button
              class="history-item"
              type="button"
              onclick={() => selectRecordedAudio(recording)}
              disabled={actionBusy || disabled}
            >
              <span>
                <strong>{recording.fileName}</strong>
                <small>{formatRecordedAt(recording.recordedAtUnixMs)} · {formatFileSize(recording.sizeBytes)}</small>
              </span>
              <span class="history-action">選択</span>
            </button>
          {/each}
        </div>
      {:else}
        <p class="history-empty">保存済みの録音はまだありません。</p>
      {/if}
    </section>
  {/if}

  <div class="sources" aria-disabled={active || disabled}>
    <div class="source">
      <label class="source-toggle">
        <input type="checkbox" bind:checked={microphone} disabled={!capabilities.microphoneSupported || active || disabled} />
        <span>マイク</span>
      </label>
      {#if capabilities.microphoneDevices.length > 0}
        <select bind:value={microphoneDeviceId} disabled={!microphone || active || disabled} aria-label="マイクデバイス">
          <option value="">OSの既定マイク</option>
          {#each capabilities.microphoneDevices as device (device.id)}
            <option value={device.id}>{device.name}{device.isDefault ? "（既定）" : ""}</option>
          {/each}
        </select>
      {/if}
      <div class="meter" aria-label="マイク入力レベル"><span style:width={`${(status?.microphoneLevel ?? 0) * 100}%`}></span></div>
    </div>

    <div class="source">
      <label class="source-toggle">
        <input type="checkbox" bind:checked={systemAudio} disabled={!capabilities.systemAudioSupported || active || disabled} />
        <span>システム音声</span>
      </label>
      {#if capabilities.systemDevices.length > 0}
        <select bind:value={systemDeviceId} disabled={!systemAudio || active || disabled} aria-label="システム音声デバイス">
          <option value="">OSの既定出力</option>
          {#each capabilities.systemDevices as device (device.id)}
            <option value={device.id}>{device.name}{device.isDefault ? "（既定）" : ""}</option>
          {/each}
        </select>
      {/if}
      <div class="meter" aria-label="システム音声レベル"><span style:width={`${(status?.systemLevel ?? 0) * 100}%`}></span></div>
    </div>
  </div>

  <div class:active class="recorder">
    <span class="record-dot" aria-hidden="true"></span>
    <strong>{formatTimer(status?.elapsedMs ?? 0)}</strong>
    <small>48 kHz · mono · AAC-LC · 64 kbps</small>
  </div>

  {#if active && status?.warning}
    <p class="capture-warning" role="status">{status.warning}</p>
  {/if}

  <div class="record-actions">
    {#if active}
      <button class="stop" type="button" onclick={stop} disabled={actionBusy || status?.phase === "finalizing"}>
        {status?.phase === "finalizing" || actionBusy ? "M4Aを確定中…" : "録音を停止"}
      </button>
      <button class="cancel" type="button" onclick={cancel} disabled={actionBusy || status?.phase === "finalizing"}>破棄</button>
    {:else}
      <button class="start" type="button" onclick={start} disabled={!canStart}>録音を開始</button>
    {/if}
  </div>
{/if}

<style>
  .placeholder,
  .limitation { margin: 18px 0 0; color: #647068; font-size: 0.86rem; }
  .limitation { padding: 11px 12px; border-radius: 9px; color: #7a4c20; background: #fff4e8; }
  .recovery { display: grid; gap: 10px; margin-top: 18px; padding: 14px; border-radius: 12px; background: #fff7df; color: #624b17; font-size: 0.84rem; }
  .recovery-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .recovery-row div { display: flex; gap: 7px; }
  .recovery button { min-height: 32px; padding: 0 10px; border: 1px solid #d5c48f; border-radius: 8px; background: #fff; cursor: pointer; }
  .recovery .text-danger { color: #9a3028; }
  .history { display: grid; gap: 10px; margin-top: 18px; }
  .history-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .history-heading > div { display: grid; gap: 3px; }
  .history-heading small,
  .history-empty { color: #68746c; font-size: 0.78rem; }
  .history-heading button { min-height: 32px; padding: 0 10px; border: 1px solid #c7cfca; background: #fff; }
  .history-list { display: grid; overflow: hidden; border: 1px solid #dce3de; border-radius: 12px; }
  .history-item { display: flex; width: 100%; min-height: auto; align-items: center; justify-content: space-between; gap: 12px; padding: 12px 14px; border: 0; border-top: 1px solid #e4e9e6; border-radius: 0; color: #253b2e; background: #fff; text-align: left; }
  .history-item:first-child { border-top: 0; }
  .history-item:hover:not(:disabled) { background: #f5f8f6; }
  .history-item > span:first-child { display: grid; min-width: 0; gap: 3px; }
  .history-item strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .history-item small { color: #68746c; }
  .history-action { flex: none; color: #23704a; font-size: 0.78rem; font-weight: 750; }
  .history-empty { margin: 0; padding: 14px; border-radius: 10px; background: #f5f7f5; }
  .sources { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; margin-top: 20px; }
  .source { display: grid; gap: 10px; padding: 15px; border: 1px solid #dce3de; border-radius: 12px; background: #f8faf8; }
  .source-toggle { display: flex; align-items: center; gap: 8px; margin: 0; font-size: 0.9rem; font-weight: 750; }
  .source-toggle input { width: 17px; height: 17px; flex: none; }
  select { box-sizing: border-box; width: 100%; height: 38px; padding: 0 9px; border: 1px solid #bdc8c0; border-radius: 8px; color: #253b2e; background: #fff; }
  .meter { height: 5px; overflow: hidden; border-radius: 99px; background: #dfe6e1; }
  .meter span { display: block; height: 100%; border-radius: inherit; background: #2c8058; transition: width 100ms linear; }
  .recorder { display: flex; align-items: center; gap: 10px; margin-top: 14px; padding: 14px 16px; border-radius: 12px; background: #f3f6f4; }
  .recorder strong { font-size: 1.35rem; font-variant-numeric: tabular-nums; letter-spacing: 0.04em; }
  .recorder small { margin-left: auto; color: #68746c; }
  .record-dot { width: 10px; height: 10px; border-radius: 50%; background: #9ca7a0; }
  .recorder.active .record-dot { background: #dc4438; box-shadow: 0 0 0 5px rgb(220 68 56 / 12%); }
  .capture-warning { margin: 10px 0 0; padding: 10px 12px; border-radius: 9px; color: #7a4c20; background: #fff4e8; font-size: 0.84rem; }
  .record-actions { display: flex; justify-content: flex-end; gap: 9px; margin-top: 14px; }
  .record-actions button { min-height: 44px; padding: 0 18px; border-radius: 10px; cursor: pointer; font-weight: 750; }
  .record-actions button:disabled { cursor: not-allowed; opacity: 0.5; }
  .start { border: 1px solid #246b49; color: #fff; background: #246b49; }
  .stop { border: 1px solid #9f382f; color: #fff; background: #b84237; }
  .cancel { border: 1px solid #c7cfca; color: #6f3833; background: #fff; }
  @media (max-width: 600px) {
    .sources { grid-template-columns: 1fr; }
    .recorder { flex-wrap: wrap; }
    .recorder small { width: 100%; margin-left: 20px; }
    .record-actions button { flex: 1; }
    .recovery-row { align-items: flex-start; flex-direction: column; }
  }
</style>
