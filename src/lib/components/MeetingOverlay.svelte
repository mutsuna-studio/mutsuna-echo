<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";
  import { currentMonitor } from "@tauri-apps/api/window";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import AudioWaveform from "@lucide/svelte/icons/audio-waveform";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Mic from "@lucide/svelte/icons/mic";
  import MonitorSpeaker from "@lucide/svelte/icons/monitor-speaker";
  import Square from "@lucide/svelte/icons/square";
  import Video from "@lucide/svelte/icons/video";
  import X from "@lucide/svelte/icons/x";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { ThemeProvider, createTheme } from "@mutsuna/ui/theme";
  import type { MeetingDetection } from "../types/meeting";
  import type {
    OverlayPreviewMode,
    OverlayPreviewRuntime,
    OverlayPreviewSnapshot
  } from "../types/overlay-preview";
  import type { PendingAction } from "../types/pending-action";
  import type { RecordingStatus, StopRecordingResult } from "../types/recording";

  const echoTheme = createTheme("custom", "oklch(0.49 0.12 154)");
  const promptSize = { width: 400, height: 60 };
  const controllerSize = { width: 310, height: 158 };
  let detection = $state<MeetingDetection | null>(null);
  let status = $state.raw<RecordingStatus | null>(null);
  let previewRuntime = $state.raw<OverlayPreviewRuntime | null>(null);
  let previewSnapshot = $state.raw<OverlayPreviewSnapshot | null>(null);
  let loading = $state(true);
  let starting = $state(false);
  let stopping = $state(false);
  let controllerMode = $state(false);
  let completionMessage = $state("");
  let error = $state("");
  let handoffBusy = $state(false);
  let handoffPromise: Promise<void> | null = null;
  let previewTransitionId: number | undefined;

  const isPreview = $derived(previewSnapshot !== null);
  const shownDetection = $derived(previewSnapshot?.detection ?? detection);
  const active = $derived(
    status?.phase === "starting" || status?.phase === "recording" || status?.phase === "finalizing"
  );
  const phaseLabel = $derived(
    status?.phase === "completed"
      ? "保存完了"
      : status?.phase === "failed"
        ? "録音エラー"
        : status?.phase === "finalizing"
          ? "保存中"
          : status?.phase === "starting"
            ? "準備中"
            : "録音中"
  );
  const voiceActivityLabel = $derived(
    status?.voiceActivity === "speechDetected"
      ? "音声を検出"
      : status?.voiceActivity === "listening"
        ? "音声を待機中"
        : "VADは利用できません"
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

  function levelStyle(level: number): string {
    return `--audio-level: ${Math.max(0, Math.min(1, level)) * 100}%`;
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
    await resizeOverlay(true);
  }

  async function applyPreviewMode(mode: OverlayPreviewMode) {
    if (!previewRuntime) return;
    if (previewTransitionId !== undefined) {
      window.clearTimeout(previewTransitionId);
      previewTransitionId = undefined;
    }
    const snapshot = previewRuntime.snapshot(mode);
    previewSnapshot = snapshot;
    status = snapshot.status;
    controllerMode = snapshot.controllerMode;
    completionMessage = snapshot.completionMessage;
    error = snapshot.error;
    await resizeOverlay(snapshot.controllerMode);
  }

  async function changePreviewMode(mode: OverlayPreviewMode) {
    if (!previewRuntime) return;
    await previewRuntime.show(mode);
    await applyPreviewMode(mode);
  }

  async function closePreview() {
    await previewRuntime?.close();
    await closeOverlay();
  }

  function handoffToMain(): Promise<void> {
    if (isPreview) return closePreview();
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
    let pollingId: number | undefined;
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<string>("pending-action-acknowledged", ({ payload }) => {
        acknowledged.add(payload);
        if (payload === actionId) resolveAcknowledgement?.();
      });
      const action = await invoke<PendingAction>("prepare_transcription_handoff");
      actionId = action.id;
      if (acknowledged.has(actionId)) resolveAcknowledgement?.();
      pollingId = window.setInterval(() => {
        void invoke<PendingAction | null>("get_pending_action")
          .then((pending) => {
            if (!pending || pending.id !== actionId) resolveAcknowledgement?.();
          })
          .catch(() => undefined);
      }, 250);
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
      if (pollingId !== undefined) window.clearInterval(pollingId);
      unlisten?.();
      handoffBusy = false;
    }
  }

  async function acceptStatus(nextStatus: RecordingStatus) {
    if (isPreview) return;
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
    if (isPreview) {
      await closePreview();
      return;
    }
    try {
      await invoke("dismiss_meeting_overlay");
    } finally {
      await closeOverlay();
    }
  }

  async function startRecording() {
    if (starting) return;
    if (isPreview) {
      await changePreviewMode("recording");
      return;
    }
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
    if (isPreview) {
      await changePreviewMode("finalizing");
      previewTransitionId = window.setTimeout(() => {
        void changePreviewMode("completed");
      }, 900);
      return;
    }
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
    let unlistenStatus: UnlistenFn | undefined;
    let unlistenPreview: UnlistenFn | undefined;
    void (async () => {
      try {
        unlistenStatus = await listen<RecordingStatus>("recording-status", ({ payload }) => {
          if (!cancelled) void acceptStatus(payload).catch((cause) => { error = errorText(cause); });
        });
        if (import.meta.env.DEV) {
          const { overlayPreviewRuntime } = await import("../overlay-preview-runtime");
          previewRuntime = overlayPreviewRuntime;
          unlistenPreview = await listen<OverlayPreviewMode>(overlayPreviewRuntime.changedEvent, ({ payload }) => {
            if (!cancelled) void applyPreviewMode(payload).catch((cause) => { error = errorText(cause); });
          });
          const initialPreview = await overlayPreviewRuntime.get();
          if (initialPreview) {
            await applyPreviewMode(initialPreview);
            return;
          }
        }
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
      unlistenStatus?.();
      unlistenPreview?.();
      if (previewTransitionId !== undefined) window.clearTimeout(previewTransitionId);
    };
  });

  $effect(() => {
    if (isPreview || !controllerMode || !active) return;
    const timer = window.setInterval(() => {
      void invoke<RecordingStatus>("get_recording_status")
        .then(acceptStatus)
        .catch((cause) => { error = errorText(cause); });
    }, 1_000);
    return () => window.clearInterval(timer);
  });
</script>

<ThemeProvider theme={echoTheme}>
  <main
    class:compact={controllerMode}
    class:preview={isPreview}
    class="meeting-overlay"
    aria-busy={loading || starting || stopping}
  >
    <div class="glass-highlight" aria-hidden="true"></div>
    {#if controllerMode}
      <section class="recording-controller" aria-label="録音コントローラー" aria-live="polite">
        <div class="recording-summary">
          <div class:error-state={status?.phase === "failed"} class:success-state={status?.phase === "completed"} class="phase-state">
            {#if status?.phase === "finalizing"}
              <LoaderCircle class="phase-icon spin" aria-hidden="true" />
            {:else if status?.phase === "completed"}
              <CircleCheck class="phase-icon" aria-hidden="true" />
            {:else if status?.phase === "failed"}
              <CircleAlert class="phase-icon" aria-hidden="true" />
            {:else}
              <span class="rec-dot" aria-hidden="true"></span>
            {/if}
            <strong>{phaseLabel}</strong>
            {#if isPreview}<Badge variant="secondary">{previewRuntime?.badgeLabel}</Badge>{/if}
          </div>
          <time>{formatElapsed(status?.elapsedMs ?? 0)}</time>
        </div>

        <div class="audio-sources">
          <div class:enabled={status?.microphone} class="audio-source">
            <Mic aria-hidden="true" />
            <span>マイク</span>
            <i
              class="level-track"
              role="meter"
              aria-label="マイク音量"
              aria-valuemin="0"
              aria-valuemax="100"
              aria-valuenow={Math.round((status?.microphoneLevel ?? 0) * 100)}
            >
              <b style={levelStyle(status?.microphoneLevel ?? 0)}></b>
            </i>
          </div>
          <div class:enabled={status?.systemAudio} class="audio-source">
            <MonitorSpeaker aria-hidden="true" />
            <span>システム</span>
            <i
              class="level-track"
              role="meter"
              aria-label="システム音量"
              aria-valuemin="0"
              aria-valuemax="100"
              aria-valuenow={Math.round((status?.systemLevel ?? 0) * 100)}
            >
              <b style={levelStyle(status?.systemLevel ?? 0)}></b>
            </i>
          </div>
        </div>

        <div class="controller-footer">
          {#if active}
            <p class:speaking={status?.voiceActivity === "speechDetected"} class="overlay-vad" role="status">
              <AudioWaveform aria-hidden="true" />{voiceActivityLabel}
            </p>
          {:else if completionMessage}
            <p class="recording-result" role="status">{completionMessage}</p>
          {:else if error}
            <p class="compact-error" role="alert">{error}</p>
          {/if}

          {#if status?.phase === "completed"}
            <Button size="sm" type="button" onclick={handoffToMain} loading={handoffBusy} disabled={handoffBusy}>
              {isPreview ? "閉じる" : handoffBusy ? "準備中…" : "文字起こしへ"}
            </Button>
          {:else if status?.phase === "failed"}
            <Button size="sm" variant="outline" type="button" onclick={isPreview ? closePreview : closeOverlay}>閉じる</Button>
          {:else}
            <Button
              size="sm"
              variant="destructive"
              type="button"
              icon={Square}
              onclick={stopRecording}
              loading={stopping}
              disabled={!active || status?.phase === "finalizing"}
            >
              {status?.phase === "finalizing" ? "保存中…" : "停止"}
            </Button>
          {/if}
        </div>
      </section>
    {:else if shownDetection}
      <section class="meeting-prompt" aria-label="会議検出">
        <div class="prompt-row">
          {#if error}
            <p class="detection-error" role="alert" title={error}>{error}</p>
          {:else}
            <div class="detection-context">
              <Video aria-hidden="true" />
              <strong>会議を検出</strong>
            </div>
            <div class="detection-badges">
              <Badge variant="outline" class="overlay-tool-badge">{shownDetection.providerLabel}</Badge>
              {#if isPreview}<Badge variant="outline" class="overlay-tool-badge">{previewRuntime?.badgeLabel}</Badge>{/if}
            </div>
          {/if}
          <div class="detection-actions">
            <Button
              size="sm"
              type="button"
              variant="ghost"
              class="overlay-record-button"
              icon={Mic}
              title="参加者の同意を確認してから、手動で録音を開始します。"
              aria-label="参加者の同意を確認して、録音を開始"
              onclick={startRecording}
              loading={starting}
              disabled={loading}
            >
              {starting ? "準備中…" : "録音を開始"}
            </Button>
            <Button class="overlay-dismiss-button" type="button" size="icon-sm" variant="ghost" icon={X} aria-label="今は録音しない" onclick={dismiss} />
          </div>
        </div>
      </section>
    {:else if loading}
      <p class="meeting-loading"><LoaderCircle class="spin" aria-hidden="true" />会議の状態を確認しています…</p>
    {/if}
  </main>
</ThemeProvider>

<style>
  .meeting-overlay {
    box-sizing: border-box;
    position: relative;
    min-height: 100vh;
    padding: 14px;
    overflow: hidden;
    /* Opaque platform windows keep their compositor-owned outline and corners. */
    color: var(--foreground);
    background:
      radial-gradient(circle at 8% 0%, color-mix(in oklch, var(--primary) 12%, transparent), transparent 42%),
      linear-gradient(145deg, color-mix(in oklch, var(--background) 96%, white), color-mix(in oklch, var(--background) 93%, var(--primary)));
    box-shadow:
      inset 0 1px 0 color-mix(in oklch, white 72%, transparent),
      inset 0 -1px 0 color-mix(in oklch, var(--foreground) 4%, transparent);
    animation: overlay-enter 180ms ease-out;
  }

  :global(html.transparent-overlay) .meeting-overlay {
    border: 1px solid color-mix(in oklch, var(--foreground) 12%, transparent);
    border-radius: 16px;
    background:
      radial-gradient(circle at 8% 0%, color-mix(in oklch, var(--primary) 15%, transparent), transparent 44%),
      linear-gradient(145deg, rgb(255 255 255 / 18%), rgb(255 255 255 / 10%)),
      linear-gradient(145deg, color-mix(in oklch, var(--background) 78%, transparent), color-mix(in oklch, var(--background) 70%, var(--primary) 8%, transparent));
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 18%),
      inset 0 -1px 0 color-mix(in oklch, var(--foreground) 3%, transparent);
  }

  .meeting-overlay.compact {
    display: grid;
    min-width: 0;
    padding: 15px 16px;
    align-items: center;
  }

  .glass-highlight {
    position: absolute;
    top: -48px;
    right: -28px;
    width: 150px;
    height: 90px;
    border-radius: 50%;
    background: color-mix(in oklch, white 34%, transparent);
    filter: blur(30px);
    pointer-events: none;
  }

  :global(html.transparent-overlay) .glass-highlight {
    inset: 0;
    width: auto;
    height: auto;
    border-radius: inherit;
    background: rgb(255 255 255 / 16%);
    filter: none;
  }

  .meeting-prompt,
  .recording-controller {
    position: relative;
    z-index: 1;
  }

  .meeting-prompt { height: 100%; }

  .prompt-row,
  .recording-summary,
  .controller-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .detection-context,
  .detection-badges,
  .detection-actions,
  .phase-state,
  .audio-source,
  .overlay-vad,
  .meeting-loading {
    display: flex;
    align-items: center;
  }

  .prompt-row {
    height: 100%;
    min-width: 0;
    gap: 7px;
  }

  .detection-context { flex: none; gap: 6px; }
  .detection-context > :global(svg) { width: 15px; height: 15px; color: color-mix(in oklch, var(--primary) 68%, var(--foreground)); }
  .detection-context strong { color: color-mix(in oklch, var(--foreground) 76%, transparent); font-size: 0.72rem; font-weight: 650; white-space: nowrap; }
  .detection-badges { min-width: 0; flex: 1; gap: 5px; }
  .detection-actions { flex: none; gap: 3px; }
  .detection-badges :global(.overlay-tool-badge) {
    border-color: color-mix(in oklch, var(--foreground) 10%, transparent);
    color: color-mix(in oklch, var(--foreground) 78%, transparent);
    background: rgb(255 255 255 / 10%);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 9%);
  }

  .detection-actions :global(.overlay-record-button) {
    border-color: color-mix(in oklch, var(--primary) 25%, transparent);
    color: color-mix(in oklch, var(--primary) 58%, var(--foreground));
    background: color-mix(in oklch, var(--primary) 18%, transparent);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 10%);
  }

  .detection-actions :global(.overlay-record-button:hover) {
    background: color-mix(in oklch, var(--primary) 25%, transparent);
  }

  .detection-actions :global(.overlay-dismiss-button) {
    color: color-mix(in oklch, var(--foreground) 62%, transparent);
    background: transparent;
  }

  .detection-actions :global(.overlay-dismiss-button:hover) {
    color: var(--foreground);
    background: rgb(255 255 255 / 9%);
  }

  .detection-error {
    min-width: 0;
    flex: 1;
    margin: 0;
    overflow: hidden;
    color: var(--destructive);
    font-size: 0.66rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .recording-controller { display: grid; min-width: 0; gap: 11px; }
  .phase-state { gap: 6px; color: var(--destructive); }
  .phase-state.success-state { color: var(--primary); }
  .phase-state.error-state { color: var(--destructive); }
  .phase-state strong { font-size: 0.72rem; letter-spacing: 0.04em; }
  .phase-state :global(.phase-icon) { width: 15px; height: 15px; }

  .rec-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--destructive);
    box-shadow: 0 0 0 4px color-mix(in oklch, var(--destructive) 13%, transparent);
    animation: recording-pulse 1.8s ease-in-out infinite;
  }

  time {
    font-size: 1.12rem;
    font-weight: 760;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.035em;
  }

  .audio-sources { display: grid; grid-template-columns: 1fr 1.25fr; gap: 7px; }

  .audio-source {
    min-width: 0;
    gap: 5px;
    padding: 7px 8px;
    border: 1px solid color-mix(in oklch, var(--border) 82%, transparent);
    border-radius: 10px;
    color: var(--muted-foreground);
    background: color-mix(in oklch, var(--background) 58%, transparent);
  }

  .audio-source.enabled { color: var(--foreground); }
  .audio-source > :global(svg) { width: 13px; height: 13px; flex: none; color: var(--primary); }
  .audio-source span { flex: none; font-size: 0.65rem; }

  .level-track {
    display: block;
    height: 4px;
    min-width: 18px;
    flex: 1;
    overflow: hidden;
    border-radius: 999px;
    background: color-mix(in oklch, var(--muted-foreground) 17%, transparent);
  }

  .level-track b {
    display: block;
    width: var(--audio-level);
    height: 100%;
    border-radius: inherit;
    background: linear-gradient(90deg, color-mix(in oklch, var(--primary) 70%, white), var(--primary));
    transition: width 110ms linear;
  }

  .controller-footer { min-width: 0; }

  .overlay-vad,
  .recording-result,
  .compact-error {
    min-width: 0;
    margin: 0;
    overflow: hidden;
    font-size: 0.66rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .overlay-vad { gap: 5px; color: var(--muted-foreground); }
  .overlay-vad.speaking { color: var(--primary); font-weight: 700; }
  .overlay-vad :global(svg) { width: 13px; height: 13px; flex: none; }
  .recording-result { color: var(--primary); }
  .compact-error { color: var(--destructive); }
  .meeting-loading { justify-content: center; gap: 8px; min-height: calc(100vh - 28px); color: var(--muted-foreground); font-size: 0.76rem; }
  .meeting-loading :global(svg) { width: 16px; height: 16px; }

  :global(.spin) { animation: spin 900ms linear infinite; }

  @keyframes overlay-enter {
    from { opacity: 0; transform: translateY(5px) scale(0.99); }
    to { opacity: 1; transform: translateY(0) scale(1); }
  }

  @keyframes recording-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.42; }
  }

  @keyframes spin { to { transform: rotate(360deg); } }

  @media (prefers-color-scheme: dark) {
    .meeting-overlay {
      background:
        radial-gradient(circle at 8% 0%, color-mix(in oklch, var(--primary) 13%, transparent), transparent 43%),
        linear-gradient(145deg, color-mix(in oklch, var(--background) 93%, white), color-mix(in oklch, var(--background) 90%, var(--primary)));
      box-shadow: inset 0 1px 0 color-mix(in oklch, white 12%, transparent);
    }

    :global(html.transparent-overlay) .meeting-overlay {
      background:
        radial-gradient(circle at 8% 0%, color-mix(in oklch, var(--primary) 16%, transparent), transparent 45%),
        linear-gradient(145deg, rgb(255 255 255 / 10%), rgb(255 255 255 / 5%)),
        linear-gradient(145deg, color-mix(in oklch, var(--background) 72%, transparent), color-mix(in oklch, var(--background) 64%, var(--primary) 8%, transparent));
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .meeting-overlay,
    .rec-dot,
    :global(.spin) { animation: none; }
    .level-track b { transition: none; }
  }
</style>
