<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { LogicalSize, PhysicalPosition, type PhysicalSize } from "@tauri-apps/api/dpi";
  import { currentMonitor, monitorFromPoint, type Monitor } from "@tauri-apps/api/window";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import AudioWaveform from "@lucide/svelte/icons/audio-waveform";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Maximize2 from "@lucide/svelte/icons/maximize-2";
  import Mic from "@lucide/svelte/icons/mic";
  import Minimize2 from "@lucide/svelte/icons/minimize-2";
  import MonitorSpeaker from "@lucide/svelte/icons/monitor-speaker";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import { Button } from "@mutsuna/ui/button";
  import { ThemeProvider, createTheme } from "@mutsuna/ui/theme";
  import AudioLevelWaveform from "./AudioLevelWaveform.svelte";
  import type { MeetingDetection } from "../types/meeting";
  import type {
    OverlayPreviewMode,
    OverlayPreviewRuntime,
    OverlayPreviewSnapshot
  } from "../types/overlay-preview";
  import type { PendingAction } from "../types/pending-action";
  import type { RecordingStatus, StopRecordingResult } from "../types/recording";

  const echoTheme = createTheme("custom", "oklch(0.49 0.12 154)");
  const promptSize = { width: 320, height: 60 };
  const controllerSize = { width: 380, height: 64 };
  const minimizedControllerSize = { width: 88, height: 48 };
  const snapStorageKey = "meeting-overlay-snap-position";
  const snapMargin = 20;
  const snapPositions = ["top-left", "top-center", "top-right", "bottom-left", "bottom-center", "bottom-right"] as const;
  type OverlaySnapPosition = (typeof snapPositions)[number];

  function storedSnapPosition(): OverlaySnapPosition {
    try {
      const stored = window.localStorage.getItem(snapStorageKey);
      return snapPositions.find((position) => position === stored) ?? "bottom-right";
    } catch {
      return "bottom-right";
    }
  }

  let snapPosition = storedSnapPosition();
  let overlayDragInProgress = false;
  let detection = $state<MeetingDetection | null>(null);
  let status = $state.raw<RecordingStatus | null>(null);
  let previewRuntime = $state.raw<OverlayPreviewRuntime | null>(null);
  let previewSnapshot = $state.raw<OverlayPreviewSnapshot | null>(null);
  let loading = $state(true);
  let starting = $state(false);
  let stopping = $state(false);
  let controllerMode = $state(false);
  let controllerMinimized = $state(false);
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

  function snapPoint(position: OverlaySnapPosition, monitor: Monitor, windowSize: PhysicalSize): PhysicalPosition {
    const workArea = monitor.workArea;
    const margin = Math.round(snapMargin * monitor.scaleFactor);
    const left = workArea.position.x + margin;
    const center = workArea.position.x + Math.round((workArea.size.width - windowSize.width) / 2);
    const right = workArea.position.x + workArea.size.width - windowSize.width - margin;
    const top = workArea.position.y + margin;
    const bottom = workArea.position.y + workArea.size.height - windowSize.height - margin;
    const x = position.endsWith("left") ? left : position.endsWith("right") ? right : center;
    const y = position.startsWith("top") ? top : bottom;
    return new PhysicalPosition(x, y);
  }

  async function applySnap(position: OverlaySnapPosition, targetMonitor?: Monitor | null) {
    const monitor = targetMonitor ?? await currentMonitor();
    if (!monitor) return;
    const overlay = getCurrentWebviewWindow();
    const size = await overlay.outerSize();
    await overlay.setPosition(snapPoint(position, monitor, size));
  }

  function persistSnap(position: OverlaySnapPosition) {
    snapPosition = position;
    try {
      window.localStorage.setItem(snapStorageKey, position);
    } catch {
      // Snapping still works for this session when WebView storage is unavailable.
    }
  }

  async function snapToNearestPosition() {
    const overlay = getCurrentWebviewWindow();
    const [windowPosition, windowSize] = await Promise.all([overlay.outerPosition(), overlay.outerSize()]);
    const centerX = windowPosition.x + windowSize.width / 2;
    const centerY = windowPosition.y + windowSize.height / 2;
    const monitor = await monitorFromPoint(centerX, centerY) ?? await currentMonitor();
    if (!monitor) return;

    let nearest: OverlaySnapPosition = snapPositions[0];
    let nearestDistance = Number.POSITIVE_INFINITY;
    for (const candidate of snapPositions) {
      const point = snapPoint(candidate, monitor, windowSize);
      const distance = (point.x - windowPosition.x) ** 2 + (point.y - windowPosition.y) ** 2;
      if (distance < nearestDistance) {
        nearest = candidate;
        nearestDistance = distance;
      }
    }
    persistSnap(nearest);
    await applySnap(nearest, monitor);
  }

  function waitForBrowserPointerRelease(): Promise<void> {
    return new Promise((resolve) => {
      const finish = () => {
        window.removeEventListener("pointerup", finish, true);
        window.removeEventListener("pointercancel", finish, true);
        resolve();
      };
      window.addEventListener("pointerup", finish, { capture: true, once: true });
      window.addEventListener("pointercancel", finish, { capture: true, once: true });
    });
  }

  async function startOverlayDrag(event: PointerEvent) {
    if (event.button !== 0 || overlayDragInProgress) return;
    event.preventDefault();
    overlayDragInProgress = true;
    const platform = navigator.userAgent.toLowerCase();
    const isWindows = platform.includes("windows");
    const isMacOs = platform.includes("macintosh") || platform.includes("mac os");
    const browserRelease = isWindows || isMacOs ? null : waitForBrowserPointerRelease();
    const nativeRelease = isWindows
      ? invoke<void>("wait_for_overlay_pointer_release").then(
          () => ({ error: null }),
          (cause: unknown) => ({ error: cause })
        )
      : null;
    try {
      await getCurrentWebviewWindow().startDragging();
      if (nativeRelease) {
        const result = await nativeRelease;
        if (result.error !== null) throw result.error;
      }
      else if (browserRelease) await browserRelease;
      await snapToNearestPosition();
    } catch (cause) {
      error = errorText(cause);
    } finally {
      overlayDragInProgress = false;
    }
  }

  async function closeOverlay() {
    await getCurrentWebviewWindow().destroy();
  }

  async function resizeOverlay(compact: boolean, minimized = controllerMinimized) {
    const size = compact ? (minimized ? minimizedControllerSize : controllerSize) : promptSize;
    const overlay = getCurrentWebviewWindow();
    await overlay.setSize(new LogicalSize(size.width, size.height));
    await applySnap(snapPosition);
  }

  async function enterController(nextStatus: RecordingStatus) {
    status = nextStatus;
    if (!controllerMode) controllerMinimized = false;
    controllerMode = true;
    await resizeOverlay(true);
  }

  async function minimizeController() {
    controllerMinimized = true;
    await resizeOverlay(true, true);
  }

  async function restoreController() {
    controllerMinimized = false;
    await resizeOverlay(true, false);
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
    if (
      !snapshot.controllerMode ||
      snapshot.status?.phase === "completed" ||
      snapshot.status?.phase === "failed"
    ) {
      controllerMinimized = false;
    }
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
      controllerMinimized = false;
      await resizeOverlay(true, false);
      await handoffToMain();
    } else if (nextStatus.phase === "failed") {
      controllerMinimized = false;
      await resizeOverlay(true, false);
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
        else await applySnap(snapPosition);
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
    class:minimized={controllerMode && controllerMinimized}
    class:preview={isPreview}
    class="meeting-overlay"
    aria-busy={loading || starting || stopping}
  >
    <div class="glass-highlight" aria-hidden="true"></div>
    {#if controllerMode && controllerMinimized}
      <section class="minimized-controller" aria-label="最小化された録音コントローラー" aria-live="polite">
        <Button
          class="restore-controller-button"
          size="icon-sm"
          variant="ghost"
          type="button"
          icon={Maximize2}
          aria-label="通常サイズに戻す"
          title="通常サイズに戻す"
          onclick={restoreController}
        />
        <Button
          class="minimized-stop-button"
          size="icon-sm"
          variant="destructive"
          type="button"
          icon={Square}
          aria-label={status?.phase === "finalizing" ? "録音を保存中" : "録音を停止"}
          title={status?.phase === "finalizing" ? "録音を保存しています" : "録音を停止"}
          onclick={stopRecording}
          loading={stopping}
          disabled={!active || status?.phase === "finalizing"}
        />
      </section>
    {:else if controllerMode}
      <section class="recording-controller" aria-label="録音コントローラー" aria-live="polite">
        <button class="recording-summary overlay-drag-handle" type="button" aria-label="オーバーレイを移動" title="ドラッグして位置を移動" onpointerdown={startOverlayDrag}>
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
            {#if isPreview}<span class="preview-label">{previewRuntime?.badgeLabel}</span>{/if}
          </div>
          <time>{formatElapsed(status?.elapsedMs ?? 0)}</time>
        </button>

        {#if active}
          <div class="audio-sources">
            <div class="waveform-with-legend">
              <Mic
                class={`microphone-legend${status?.microphone ? " enabled" : ""}`}
                aria-label={status?.microphone ? "マイク入力: 緑" : "マイク入力なし"}
              />
              <AudioLevelWaveform
                microphoneLevel={status?.microphoneLevel ?? 0}
                systemLevel={status?.systemLevel ?? 0}
                microphoneEnabled={status?.microphone ?? false}
                systemEnabled={status?.systemAudio ?? false}
                elapsedMs={status?.elapsedMs ?? 0}
              />
              <MonitorSpeaker
                class={`system-legend${status?.systemAudio ? " enabled" : ""}`}
                aria-label={status?.systemAudio ? "システム音声: 青" : "システム音声なし"}
              />
            </div>
            <p class:speaking={status?.voiceActivity === "speechDetected"} class="overlay-vad" role="status">
              <AudioWaveform aria-hidden="true" /><span>{voiceActivityLabel}</span>
            </p>
          </div>
        {:else if completionMessage}
          <p class="recording-result controller-message" role="status">{completionMessage}</p>
        {:else if error}
          <p class="compact-error controller-message" role="alert">{error}</p>
        {/if}

        <div class="controller-action">
          {#if status?.phase === "completed"}
            <Button size="sm" type="button" onclick={handoffToMain} loading={handoffBusy} disabled={handoffBusy}>
              {isPreview ? "閉じる" : handoffBusy ? "準備中…" : "文字起こしへ"}
            </Button>
          {:else if status?.phase === "failed"}
            <Button size="sm" variant="outline" type="button" onclick={isPreview ? closePreview : closeOverlay}>閉じる</Button>
          {:else}
            <Button
              class="minimize-controller-button"
              size="icon-sm"
              variant="ghost"
              type="button"
              icon={Minimize2}
              aria-label="停止ボタンだけに最小化"
              title="停止ボタンだけに最小化"
              onclick={minimizeController}
              disabled={!active || status?.phase === "finalizing"}
            />
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
            <button class="detection-identity overlay-drag-handle" type="button" aria-label="オーバーレイを移動" title="ドラッグして位置を移動" onpointerdown={startOverlayDrag}>
              <strong>{shownDetection.providerLabel}</strong>
              {#if isPreview}<span class="preview-label">{previewRuntime?.badgeLabel}</span>{/if}
            </button>
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
    cursor: default;
    user-select: none;
    -webkit-user-drag: none;
    -webkit-user-select: none;
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

  .meeting-overlay:not(.compact) {
    display: grid;
    padding: 0 13px;
    align-items: center;
    color: rgb(247 250 248);
    background: rgb(31 35 33 / 88%);
    border: 1px solid rgb(255 255 255 / 8%);
    border-radius: 16px;
    box-shadow:
      0 8px 24px rgb(0 0 0 / 18%),
      inset 0 1px 0 rgb(255 255 255 / 7%);
    backdrop-filter: blur(18px) saturate(120%);
  }

  :global(html.transparent-overlay) .meeting-overlay:not(.compact) {
    background: rgb(31 35 33 / 88%);
    border-color: rgb(255 255 255 / 8%);
    box-shadow:
      0 8px 24px rgb(0 0 0 / 18%),
      inset 0 1px 0 rgb(255 255 255 / 7%);
  }

  .meeting-overlay.compact {
    display: grid;
    min-width: 0;
    padding: 0 12px;
    align-items: center;
    color: rgb(247 250 248);
    background: rgb(31 35 33 / 88%);
    border: 1px solid rgb(255 255 255 / 8%);
    border-radius: 16px;
    box-shadow:
      0 8px 24px rgb(0 0 0 / 18%),
      inset 0 1px 0 rgb(255 255 255 / 7%);
    backdrop-filter: blur(18px) saturate(120%);
  }

  .meeting-overlay.minimized {
    padding: 0 7px;
    border-radius: 14px;
  }

  :global(html.transparent-overlay) .meeting-overlay.compact {
    background: rgb(31 35 33 / 88%);
    border-color: rgb(255 255 255 / 8%);
    box-shadow:
      0 8px 24px rgb(0 0 0 / 18%),
      inset 0 1px 0 rgb(255 255 255 / 7%);
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

  .meeting-overlay:not(.compact) .glass-highlight,
  :global(html.transparent-overlay) .meeting-overlay:not(.compact) .glass-highlight,
  .meeting-overlay.compact .glass-highlight,
  :global(html.transparent-overlay) .meeting-overlay.compact .glass-highlight {
    background: rgb(255 255 255 / 3%);
  }

  .meeting-prompt,
  .recording-controller {
    position: relative;
    z-index: 1;
  }
  .minimized-controller {
    position: relative;
    z-index: 1;
    display: flex;
    width: 100%;
    align-items: center;
    justify-content: center;
    gap: 3px;
  }
  .minimized-controller :global(.restore-controller-button) { color: rgb(226 233 229 / 72%); }
  .minimized-controller :global(.restore-controller-button:hover) {
    color: rgb(255 255 255);
    background: rgb(255 255 255 / 10%);
  }

  .meeting-prompt { height: 100%; }
  .meeting-overlay:not(.compact) .meeting-prompt { width: 100%; height: auto; }

  .overlay-drag-handle {
    padding: 0;
    border: 0;
    color: inherit;
    background: transparent;
    cursor: grab;
    font: inherit;
    text-align: left;
  }
  .overlay-drag-handle:active { cursor: grabbing; }
  .overlay-drag-handle > * { pointer-events: none; }

  .prompt-row,
  .recording-summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .detection-identity,
  .detection-actions,
  .phase-state,
  .waveform-with-legend,
  .overlay-vad,
  .meeting-loading {
    display: flex;
    align-items: center;
  }

  .prompt-row {
    height: 34px;
    min-width: 0;
    gap: 7px;
  }

  .detection-identity { min-width: 0; flex: 1; gap: 7px; }
  .detection-identity strong {
    min-width: 0;
    overflow: hidden;
    color: rgb(247 250 248);
    font-size: 0.75rem;
    font-weight: 680;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preview-label {
    flex: none;
    padding-left: 7px;
    border-left: 1px solid rgb(255 255 255 / 14%);
    color: rgb(209 218 213 / 72%);
    font-size: 0.57rem;
    font-weight: 680;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .detection-actions { flex: none; gap: 3px; }

  .detection-actions :global(.overlay-record-button) {
    height: 34px;
    border-color: rgb(120 232 167 / 18%);
    color: rgb(246 255 249);
    background: rgb(29 148 86 / 92%);
    box-shadow:
      0 2px 8px rgb(0 0 0 / 14%),
      inset 0 1px 0 rgb(255 255 255 / 14%);
  }

  .detection-actions :global(.overlay-record-button:hover) {
    background: rgb(35 168 99 / 96%);
  }

  .detection-actions :global(.overlay-dismiss-button) {
    color: rgb(226 233 229 / 68%);
    background: transparent;
  }

  .detection-actions :global(.overlay-dismiss-button:hover) {
    color: rgb(255 255 255);
    background: rgb(255 255 255 / 10%);
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

  .recording-controller {
    display: flex;
    width: 100%;
    min-width: 0;
    align-items: center;
    gap: 9px;
  }
  .recording-summary { flex: none; justify-content: flex-start; gap: 8px; }
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
    font-size: 0.9rem;
    font-weight: 760;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.035em;
  }

  .audio-sources {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 6px;
  }

  .waveform-with-legend {
    min-width: 0;
    flex: none;
    gap: 4px;
  }

  .waveform-with-legend > :global(svg) { width: 12px; height: 12px; flex: none; opacity: 0.32; }
  .waveform-with-legend > :global(.microphone-legend.enabled) { color: rgb(102 222 156); opacity: 0.9; }
  .waveform-with-legend > :global(.system-legend.enabled) { color: rgb(101 201 243); opacity: 0.9; }

  .controller-message { flex: 1; }
  .controller-action { display: flex; flex: none; align-items: center; gap: 3px; }
  .controller-action :global(.minimize-controller-button) { color: rgb(226 233 229 / 66%); }
  .controller-action :global(.minimize-controller-button:hover) {
    color: rgb(255 255 255);
    background: rgb(255 255 255 / 10%);
  }

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

  .overlay-vad { gap: 4px; color: rgb(209 218 213 / 62%); }
  .overlay-vad.speaking { color: rgb(102 222 156); font-weight: 700; }
  .overlay-vad :global(svg) { width: 13px; height: 13px; flex: none; }
  .overlay-vad span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .recording-result { color: rgb(102 222 156); }
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

    .meeting-overlay:not(.compact),
    :global(html.transparent-overlay) .meeting-overlay:not(.compact) {
      color: rgb(247 250 248);
      background: rgb(31 35 33 / 88%);
      border-color: rgb(255 255 255 / 8%);
    }

    .meeting-overlay.compact,
    :global(html.transparent-overlay) .meeting-overlay.compact {
      color: rgb(247 250 248);
      background: rgb(31 35 33 / 88%);
      border-color: rgb(255 255 255 / 8%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .meeting-overlay,
    .rec-dot,
    :global(.spin) { animation: none; }
  }
</style>
