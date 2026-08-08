<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
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
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { Checkbox } from "@mutsuna/ui/checkbox";
  import { Select } from "@mutsuna/ui/select";
  import { formatFileSize, formatRecordedAt } from "../format";
  import { transcriptionProviderLabel } from "../providers";
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

  const active = $derived(
    status?.phase === "starting" || status?.phase === "recording" || status?.phase === "finalizing"
  );
  const canStart = $derived(
    Boolean(capabilities?.supported) && (microphone || systemAudio) && !disabled && !actionBusy && !active
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
    <section class="history" aria-label="過去の録音">
      <div class="history-heading">
        <div>
          <strong>過去の録音</strong>
          <small>Music/Mutsuna Echoに保存された最新100件</small>
        </div>
        <Button variant="outline" size="sm" type="button" onclick={refreshRecordedAudio} disabled={actionBusy}>更新</Button>
      </div>
      {#if recordedAudio.length > 0}
        <div class="history-list">
          {#each recordedAudio as recording (recording.id)}
            <div class="history-item">
              <Button
                class="history-select"
                variant="ghost"
                type="button"
                onclick={() => selectRecordedAudio(recording)}
                disabled={actionBusy || disabled}
              >
                <span>
                  <strong>{recording.fileName}</strong>
                  <small>
                    {formatRecordedAt(recording.recordedAtUnixMs)} · {formatFileSize(recording.sizeBytes)}
                    {#each recording.transcriptProviders as provider}
                      <Badge variant="secondary">{transcriptionProviderLabel(provider)} 済み</Badge>
                    {/each}
                  </small>
                </span>
                <span class="history-action">選択</span>
              </Button>
              <Button
                class="history-reveal"
                variant="outline"
                size="sm"
                type="button"
                onclick={() => revealRecordedAudio(recording)}
                disabled={actionBusy}
                aria-label={`${recording.fileName}の保存場所を開く`}
              >場所を開く</Button>
            </div>
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
  .history { display: grid; gap: 10px; margin-top: 18px; }
  .history-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .history-heading > div { display: grid; gap: 3px; }
  .history-heading small,
  .history-empty { color: #68746c; font-size: 0.78rem; }
  .history-list { display: grid; overflow: hidden; border: 1px solid #dce3de; border-radius: 12px; }
  .history-item { display: flex; align-items: center; gap: 8px; padding: 6px; border-top: 1px solid var(--border); }
  .history-item:first-child { border-top: 0; }
  .history-item :global(.history-select) { min-width: 0; height: auto; flex: 1; justify-content: space-between; padding: 8px; text-align: left; }
  .history-item :global(.history-select > span:first-child) { display: grid; min-width: 0; gap: 3px; }
  .history-item :global(.history-select strong) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .history-item :global(.history-select small) { color: var(--muted-foreground); }
  .history-item :global(.history-reveal) { flex: none; }
  .history-action { flex: none; color: #23704a; font-size: 0.78rem; font-weight: 750; }
  .history-empty { margin: 0; padding: 14px; border-radius: 10px; background: #f5f7f5; }
  .sources { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; margin-top: 20px; }
  .source { display: grid; gap: 10px; padding: 15px; border: 1px solid #dce3de; border-radius: 12px; background: #f8faf8; }
  .source-toggle { display: flex; align-items: center; gap: 8px; margin: 0; font-size: 0.9rem; font-weight: 750; }
  .meter { height: 5px; overflow: hidden; border-radius: 99px; background: #dfe6e1; }
  .meter span { display: block; height: 100%; border-radius: inherit; background: #2c8058; transition: width 100ms linear; }
  .recorder { display: flex; align-items: center; gap: 10px; margin-top: 14px; padding: 14px 16px; border-radius: 12px; background: #f3f6f4; }
  .recorder strong { font-size: 1.35rem; font-variant-numeric: tabular-nums; letter-spacing: 0.04em; }
  .recorder small { margin-left: auto; color: #68746c; }
  .record-dot { width: 10px; height: 10px; border-radius: 50%; background: #9ca7a0; }
  .recorder.active .record-dot { background: #dc4438; box-shadow: 0 0 0 5px rgb(220 68 56 / 12%); }
  .capture-warning { margin: 10px 0 0; padding: 10px 12px; border-radius: 9px; color: #7a4c20; background: #fff4e8; font-size: 0.84rem; }
  .record-actions { display: flex; justify-content: flex-end; gap: 9px; margin-top: 14px; }
  @media (max-width: 600px) {
    .sources { grid-template-columns: 1fr; }
    .recorder { flex-wrap: wrap; }
    .recorder small { width: 100%; margin-left: 20px; }
    .record-actions :global(button) { flex: 1; }
    .recovery-row { align-items: flex-start; flex-direction: column; }
    .history-action { display: none; }
  }
</style>
