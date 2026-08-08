<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";
  import { currentMonitor } from "@tauri-apps/api/window";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { ThemeProvider, createTheme } from "@mutsuna/ui/theme";
  import type { MeetingDetection } from "../types/meeting";
  import type { PendingAction } from "../types/pending-action";
  import type { RecordingStatus, StopRecordingResult } from "../types/recording";

  const echoTheme = createTheme("custom", "oklch(0.49 0.12 154)");
  const promptSize = { width: 390, height: 230 };
  const controllerSize = { width: 310, height: 136 };

  let detection = $state<MeetingDetection | null>(null);
  let status = $state.raw<RecordingStatus | null>(null);
  let loading = $state(true);
  let starting = $state(false);
  let stopping = $state(false);
  let controllerMode = $state(false);
  let compactApplied = $state(false);
  let completionMessage = $state("");
  let error = $state("");
  let handoffBusy = $state(false);
  let handoffPromise: Promise<void> | null = null;

  const active = $derived(
    status?.phase === "starting" || status?.phase === "recording" || status?.phase === "finalizing"
  );

  function errorText(value: unknown): string {
    if (typeof value === "string") return value;
    if (value instanceof Error) return value.message;
    return "録音操作に失敗しました。";
  }

  function formatElapsed(milliseconds: number): string {
    const seconds = Math.floor(milliseconds / 1_000);
    const hours = Math.floor(seconds / 3_600);
    const minutes = Math.floor((seconds % 3_600) / 60);
    const rest = seconds % 60;
    return `${hours.toString().padStart(2, "0")}:${minutes.toString().padStart(2, "0")}:${rest.toString().padStart(2, "0")}`;
  }

  function levelStyle(level: number, enabled: boolean): string {
    const normalized = Math.max(0, Math.min(1, level));
    const opacity = enabled ? 0.45 + normalized * 0.55 : 0.24;
    const scale = enabled ? 0.85 + normalized * 0.35 : 0.8;
    return `opacity: ${opacity}; transform: scale(${scale})`;
  }

  async function closeOverlay() {
    await getCurrentWebviewWindow().destroy();
  }

  async function resizeOverlay(compact: boolean) {
    const size = compact ? controllerSize : promptSize;
    const overlay = getCurrentWebviewWindow();
    await overlay.setSize(new LogicalSize(size.width, size.height));
    const monitor = await currentMonitor();
    if (!monitor) return;
    const workPosition = monitor.workArea.position.toLogical(monitor.scaleFactor);
    const workSize = monitor.workArea.size.toLogical(monitor.scaleFactor);
    await overlay.setPosition(new LogicalPosition(
      workPosition.x + workSize.width - size.width - 24,
      workPosition.y + workSize.height - size.height - 24
    ));
  }

  async function enterController(nextStatus: RecordingStatus) {
    status = nextStatus;
    controllerMode = true;
    if (!compactApplied) {
      compactApplied = true;
      await resizeOverlay(true);
    }
  }

  function handoffToMain(): Promise<void> {
    if (handoffPromise) return handoffPromise;
    handoffPromise = performHandoff().finally(() => {
      handoffPromise = null;
    });
    return handoffPromise;
  }

  async function performHandoff() {
    handoffBusy = true;
    error = "";
    completionMessage = "録音を保存しました。メイン画面を準備しています…";
    let actionId = "";
    const acknowledged = new Set<string>();
    let resolveAcknowledgement: (() => void) | undefined;
    const acknowledgement = new Promise<void>((resolve) => {
      resolveAcknowledgement = resolve;
    });
    let timeoutId: number | undefined;
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<string>("pending-action-acknowledged", ({ payload }) => {
        acknowledged.add(payload);
        if (payload === actionId) resolveAcknowledgement?.();
      });
      const action = await invoke<PendingAction>("prepare_transcription_handoff");
      actionId = action.id;
      if (acknowledged.has(actionId)) resolveAcknowledgement?.();
      const timeout = new Promise<never>((_, reject) => {
        timeoutId = window.setTimeout(
          () => reject(new Error("メイン画面の準備確認がタイムアウトしました。")),
          10_000
        );
      });
      await Promise.race([acknowledgement, timeout]);
      await closeOverlay();
    } catch (cause) {
      completionMessage = "";
      error = `録音は保存されています。${errorText(cause)}`;
    } finally {
      if (timeoutId !== undefined) window.clearTimeout(timeoutId);
      unlisten?.();
      handoffBusy = false;
    }
  }

  async function acceptStatus(nextStatus: RecordingStatus) {
    status = nextStatus;
    if (
      nextStatus.phase === "starting" ||
      nextStatus.phase === "recording" ||
      nextStatus.phase === "finalizing"
    ) {
      await enterController(nextStatus);
      return;
    }
    if (!controllerMode) return;
    if (nextStatus.phase === "completed") {
      await handoffToMain();
    } else if (nextStatus.phase === "failed") {
      error = nextStatus.error ?? "録音を完了できませんでした。";
    }
  }

  async function dismiss() {
    try {
      await invoke("dismiss_meeting_overlay");
    } finally {
      await closeOverlay();
    }
  }

  async function startRecording() {
    if (starting) return;
    starting = true;
    error = "";
    try {
      const nextStatus = await invoke<RecordingStatus>("start_recording", {
        request: {
          microphone: true,
          systemAudio: true,
          microphoneDeviceId: null,
          systemDeviceId: null
        }
      });
      await invoke("dismiss_meeting_overlay");
      await enterController(nextStatus);
    } catch (cause) {
      error = errorText(cause);
    } finally {
      starting = false;
    }
  }

  async function stopRecording() {
    if (stopping || !active) return;
    stopping = true;
    error = "";
    try {
      const result = await invoke<StopRecordingResult>("stop_recording");
      await acceptStatus(result.status);
    } catch (cause) {
      error = errorText(cause);
    } finally {
      stopping = false;
    }
  }

  $effect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    void (async () => {
      try {
        unlisten = await listen<RecordingStatus>("recording-status", ({ payload }) => {
          if (!cancelled) void acceptStatus(payload).catch((cause) => { error = errorText(cause); });
        });
        const [nextDetection, nextStatus] = await Promise.all([
          invoke<MeetingDetection | null>("get_meeting_detection"),
          invoke<RecordingStatus>("get_recording_status")
        ]);
        if (cancelled) return;
        detection = nextDetection;
        await acceptStatus(nextStatus);
        if (!detection && !active && !controllerMode) await closeOverlay();
      } catch (cause) {
        error = errorText(cause);
      } finally {
        loading = false;
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  });

  // イベントが間引かれた場合も、経過時間と終了状態を1秒以内に同期する。
  $effect(() => {
    if (!controllerMode || !active) return;
    const timer = window.setInterval(() => {
      void invoke<RecordingStatus>("get_recording_status")
        .then(acceptStatus)
        .catch((cause) => { error = errorText(cause); });
    }, 1_000);
    return () => window.clearInterval(timer);
  });
</script>

<ThemeProvider theme={echoTheme}>
  <main class:compact={controllerMode} class="meeting-overlay" aria-busy={loading || starting || stopping}>
    {#if controllerMode}
      <section class="recording-controller" aria-label="録音コントローラー">
        <div class="recording-summary">
          <strong class:finalizing={status?.phase === "finalizing"} class="rec-state">
            <span aria-hidden="true"></span>
            {status?.phase === "completed" ? "SAVED" : status?.phase === "finalizing" ? "SAVING" : status?.phase === "starting" ? "READY" : "REC"}
          </strong>
          <time>{formatElapsed(status?.elapsedMs ?? 0)}</time>
        </div>
        <div class="recording-sources">
          <span>
            Mic
            <i class:enabled={status?.microphone} style={levelStyle(status?.microphoneLevel ?? 0, status?.microphone ?? false)}></i>
          </span>
          <span>
            System
            <i class:enabled={status?.systemAudio} style={levelStyle(status?.systemLevel ?? 0, status?.systemAudio ?? false)}></i>
          </span>
          {#if status?.phase === "completed"}
            <Button size="sm" type="button" onclick={handoffToMain} loading={handoffBusy} disabled={handoffBusy}>
              {handoffBusy ? "準備中…" : "メイン画面を再表示"}
            </Button>
          {:else}
            <Button size="sm" type="button" onclick={stopRecording} loading={stopping} disabled={!active || status?.phase === "finalizing"}>
              {status?.phase === "finalizing" ? "保存中…" : "■ 停止"}
            </Button>
          {/if}
        </div>
        {#if completionMessage}<p class="recording-result" role="status">{completionMessage}</p>{/if}
        {#if error}<p class="meeting-error compact-error" role="alert">{error}</p>{/if}
      </section>
    {:else if detection}
      <header>
        <div class="meeting-mark" aria-hidden="true">●</div>
        <div>
          <Badge variant="secondary">{detection.providerLabel}</Badge>
          <h1>会議を開始しましたか？</h1>
        </div>
      </header>
      <p class="meeting-title" title={detection.windowTitle}>{detection.windowTitle}</p>
      <p class="meeting-consent">録音はまだ開始されていません。参加者の同意を確認してから開始してください。</p>
      {#if error}<p class="meeting-error" role="alert">{error}</p>{/if}
      <div class="meeting-actions">
        <Button variant="ghost" type="button" onclick={dismiss} disabled={starting}>今は録音しない</Button>
        <Button type="button" onclick={startRecording} loading={starting} disabled={loading}>
          {starting ? "録音を準備中…" : "録音を開始"}
        </Button>
      </div>
    {:else if loading}
      <p class="meeting-loading">会議の状態を確認しています…</p>
    {/if}
  </main>
</ThemeProvider>
