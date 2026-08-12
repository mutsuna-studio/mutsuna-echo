<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import FileUp from "@lucide/svelte/icons/file-up";
  import Mic from "@lucide/svelte/icons/mic";
  import { scrollbarVisibility } from "@mutsuna/ui/scrollbar";
  import { formatFileSize, formatTimestamp } from "../format";
  import type { RecentMeetingSummary } from "../types/recording";
  import type { SelectedAudioFile } from "../types/transcript";
  import RecordingPanel from "./RecordingPanel.svelte";

  type Props = {
    meetings: readonly RecentMeetingSummary[];
    loading: boolean;
    busy: boolean;
    recordingDisabled: boolean;
    recordingBusy: boolean;
    recordingPreview?: boolean;
    allowMeetingNavigation?: boolean;
    processingMeetingId?: string | null;
    processingStatus?: string | null;
    selecting: boolean;
    onSelectMeeting: (meeting: RecentMeetingSummary) => void;
    onSelectFile: () => void;
    onAudioReady: (audio: SelectedAudioFile) => void;
    onRecordingBusyChange: (busy: boolean) => void;
    onMessage: (message: string) => void;
    onError: (message: string) => void;
  };

  let {
    meetings,
    loading,
    busy,
    recordingDisabled,
    recordingBusy,
    recordingPreview = false,
    allowMeetingNavigation = false,
    processingMeetingId = null,
    processingStatus = null,
    selecting,
    onSelectMeeting,
    onSelectFile,
    onAudioReady,
    onRecordingBusyChange,
    onMessage,
    onError
  }: Props = $props();

  let meetingListElement = $state<HTMLDivElement | null>(null);
  let meetingHomeElement = $state<HTMLElement | null>(null);
  let recordButtonSwipeConsumed = false;
  let recordButtonSwipeResetTimer: number | null = null;
  const today = new Date();
  const todayIso = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
  const todayLabel = new Intl.DateTimeFormat("ja-JP", {
    year: "numeric",
    month: "long",
    day: "numeric",
    weekday: "short"
  }).format(today);
  let pullGesture = $state<{
    x: number;
    y: number;
    lastY: number;
    startedOnRecordButton: boolean;
  } | null>(null);

  function startPullGesture(event: TouchEvent) {
    const touch = event.touches[0];
    if (!touch) return;
    const target = event.target instanceof Element ? event.target : null;
    const startedOnRecordButton = Boolean(target?.closest(".mobile-record-toggle"));
    if (startedOnRecordButton) recordButtonSwipeConsumed = false;
    pullGesture = {
      x: touch.clientX,
      y: touch.clientY,
      lastY: touch.clientY,
      startedOnRecordButton
    };
  }

  function updatePullGesture(event: TouchEvent) {
    if (!pullGesture) return;
    const touch = event.touches[0];
    if (!touch) return;
    const horizontalDistance = Math.abs(touch.clientX - pullGesture.x);
    const verticalDistance = touch.clientY - pullGesture.y;
    const isVertical = Math.abs(verticalDistance) > horizontalDistance * 1.2;
    if (pullGesture.startedOnRecordButton && meetingListElement && isVertical && Math.abs(verticalDistance) >= 10) {
      event.preventDefault();
      meetingListElement.scrollTop -= touch.clientY - pullGesture.lastY;
      pullGesture.lastY = touch.clientY;
      recordButtonSwipeConsumed = true;
      if (recordButtonSwipeResetTimer != null) window.clearTimeout(recordButtonSwipeResetTimer);
      recordButtonSwipeResetTimer = window.setTimeout(() => {
        recordButtonSwipeConsumed = false;
        recordButtonSwipeResetTimer = null;
      }, 500);
      return;
    }
  }

  function endPullGesture() {
    pullGesture = null;
  }

  function consumeRecordButtonSwipe(): boolean {
    if (!recordButtonSwipeConsumed) return false;
    recordButtonSwipeConsumed = false;
    if (recordButtonSwipeResetTimer != null) window.clearTimeout(recordButtonSwipeResetTimer);
    recordButtonSwipeResetTimer = null;
    return true;
  }

  $effect(() => {
    const element = meetingHomeElement;
    if (!element) return;
    element.addEventListener("touchstart", startPullGesture, { passive: true });
    element.addEventListener("touchmove", updatePullGesture, { passive: false });
    element.addEventListener("touchend", endPullGesture, { passive: true });
    element.addEventListener("touchcancel", endPullGesture, { passive: true });
    return () => {
      element.removeEventListener("touchstart", startPullGesture);
      element.removeEventListener("touchmove", updatePullGesture);
      element.removeEventListener("touchend", endPullGesture);
      element.removeEventListener("touchcancel", endPullGesture);
      if (recordButtonSwipeResetTimer != null) window.clearTimeout(recordButtonSwipeResetTimer);
    };
  });

  function meetingDate(meeting: RecentMeetingSummary): string {
    return new Intl.DateTimeFormat("ja-JP", {
      month: "numeric",
      day: "numeric",
      weekday: "short",
      hour: "2-digit",
      minute: "2-digit"
    }).format(meeting.occurredAtUnixMs);
  }

  function meetingStatus(meeting: RecentMeetingSummary): {
    label: string;
    tone: "ready" | "complete" | "missing";
  } {
    if (!meeting.audioAvailable) return { label: "音声なし", tone: "missing" };
    if (meeting.transcriptProviders.length > 0) {
      return { label: "文字起こし済み", tone: "complete" };
    }
    return {
      label: meeting.source === "recording" ? "録音済み" : "取込済み",
      tone: "ready"
    };
  }
</script>

<section
  class:recording-active={recordingBusy}
  class="meeting-home"
  bind:this={meetingHomeElement}
  aria-label="録音と会議"
>
  <header class="home-intro">
    <div>
      <h1>録音と会議</h1>
      <p>会議やインタビュー、アイデアのメモを録音して文字起こしします。</p>
    </div>
    <time datetime={todayIso}><CalendarDays aria-hidden="true" />{todayLabel}</time>
  </header>

  <div class:active={recordingBusy} class="recording-area">
    <RecordingPanel
      disabled={recordingDisabled}
      preview={recordingPreview}
      consumeMobileAction={consumeRecordButtonSwipe}
      {onAudioReady}
      onBusyChange={onRecordingBusyChange}
      {onMessage}
      {onError}
    />
  </div>

  {#if !recordingBusy}
  <div class="meeting-list-shell">
    <div class="meeting-library-header">
      <h2>最近の会議</h2>
      <button class="import-audio-button" type="button" onclick={onSelectFile} disabled={busy}>
        <FileUp aria-hidden="true" />
        <span>{selecting ? "選択中…" : "音声ファイルを選択"}</span>
      </button>
    </div>
    <div
      class="meeting-list mutsuna-scrollbar mutsuna-scrollbar--both-edges"
      bind:this={meetingListElement}
      use:scrollbarVisibility
    >
      {#if loading && meetings.length === 0}
        <p class="library-message" role="status">会議を読み込んでいます…</p>
      {:else if meetings.length === 0}
        <p class="library-message">録音すると、ここに会議が追加されます。</p>
      {:else}
        {#each meetings as meeting (meeting.meetingId)}
          {@const activeProcessingStatus = meeting.meetingId === processingMeetingId ? processingStatus : null}
          {@const settledStatus = meetingStatus(meeting)}
          <button
            class="meeting-row"
            type="button"
            onclick={() => onSelectMeeting(meeting)}
            disabled={busy && !allowMeetingNavigation}
            title={meeting.audioAvailable ? meeting.fileName : `${meeting.title}（音声なし）`}
          >
            <span class:imported={meeting.source === "imported"} class="row-icon" aria-hidden="true">
              {#if meeting.source === "recording"}<Mic />{:else}<FileUp />{/if}
            </span>
            <span class="row-copy">
              <strong>{meeting.title}</strong>
              <small class="row-meta">
                {meetingDate(meeting)}{#if meeting.durationMs != null} · {formatTimestamp(meeting.durationMs)}{/if} · {formatFileSize(meeting.sizeBytes)}
              </small>
            </span>
            <span class:processing={activeProcessingStatus != null} class="row-status">
              {#if activeProcessingStatus}
                <small class="processing-status" aria-live="polite"><span class="processing-indicator" aria-hidden="true"></span>{activeProcessingStatus}</small>
              {:else}
                <small
                  class:complete={settledStatus.tone === "complete"}
                  class:missing={settledStatus.tone === "missing"}
                  class="settled-status"
                ><span class="status-dot" aria-hidden="true"></span>{settledStatus.label}</small>
              {/if}
            </span>
            <ChevronRight aria-hidden="true" />
          </button>
        {/each}
      {/if}
    </div>
  </div>
  {/if}
</section>

<style>
  .meeting-home { display: grid; box-sizing: border-box; width: min(1120px, calc(100% - 72px)); height: 100%; min-height: 0; margin: 0 auto; grid-template-rows: auto auto minmax(0, 1fr); background: transparent; }
  .home-intro { display: flex; min-width: 0; align-items: flex-start; justify-content: space-between; gap: 32px; padding: 34px 0 26px; }
  .home-intro > div { min-width: 0; }
  .home-intro h1 { margin: 0 0 7px; font-size: clamp(1.7rem, 2.25vw, 2.1rem); font-weight: 720; line-height: 1.2; letter-spacing: -0.045em; }
  .home-intro p { margin: 0; color: var(--muted-foreground); font-size: 0.82rem; line-height: 1.6; }
  .home-intro time { display: inline-flex; flex: none; align-items: center; gap: 10px; padding-top: 8px; color: color-mix(in oklch, var(--foreground) 78%, var(--muted-foreground)); font-size: 0.78rem; font-weight: 560; white-space: nowrap; }
  .home-intro time :global(svg) { width: 18px; height: 18px; color: var(--muted-foreground); stroke-width: 1.8; }
  .recording-area { position: relative; z-index: 2; }
  .recording-area.active { border-bottom: 0; }
  .meeting-list-shell { position: relative; display: grid; min-height: 0; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; }
  .meeting-library-header { display: flex; min-width: 0; align-items: center; justify-content: space-between; gap: 20px; padding: 23px 12px 12px; }
  .meeting-library-header h2 { margin: 0; font-size: 1rem; font-weight: 740; letter-spacing: -0.02em; }
  .import-audio-button { display: inline-flex; flex: none; align-items: center; gap: 7px; padding: 7px 10px; border: 1px solid color-mix(in oklch, var(--primary) 34%, var(--border)); border-radius: var(--radius-control); color: var(--primary); background: color-mix(in oklch, var(--primary) 3%, transparent); cursor: pointer; font: inherit; font-size: 0.72rem; font-weight: 680; }
  .import-audio-button:hover:not(:disabled) { background: color-mix(in oklch, var(--primary) 8%, transparent); }
  .import-audio-button:disabled { cursor: not-allowed; opacity: 0.52; }
  .import-audio-button :global(svg) { width: 16px; height: 16px; stroke-width: 1.8; }
  .meeting-row { grid-template-columns: 34px minmax(0, 1fr) auto 18px; }
  .meeting-list { box-sizing: border-box; height: 100%; padding-bottom: 18px; overflow-x: hidden; overflow-y: auto; overscroll-behavior: contain; scrollbar-gutter: stable both-edges; }
  .meeting-row { width: 100%; min-width: 0; align-items: center; gap: 12px; min-height: 64px; padding: 9px 12px; border: 0; border-bottom: 1px solid var(--border); border-radius: 0; color: var(--foreground); background: transparent; cursor: pointer; font: inherit; text-align: left; }
  .meeting-row { display: grid; }
  .meeting-row:hover { background: color-mix(in oklch, var(--muted) 55%, var(--background)); }
  .meeting-row:focus-visible { position: relative; z-index: 1; outline: 2px solid var(--ring); outline-offset: -3px; }
  .meeting-row:disabled { cursor: not-allowed; opacity: 0.56; }
  .row-icon { display: grid; width: 30px; height: 30px; place-items: center; color: var(--primary); }
  .row-icon.imported { color: var(--muted-foreground); }
  .row-icon :global(svg) { width: 18px; height: 18px; stroke-width: 1.8; }
  .row-copy { display: grid; min-width: 0; gap: 4px; }
  .row-copy strong { overflow: hidden; font-size: 0.88rem; font-weight: 690; text-overflow: ellipsis; white-space: nowrap; }
  .row-copy small { color: var(--muted-foreground); font-size: 0.7rem; }
  .row-meta { display: block; font-variant-numeric: tabular-nums; }
  .row-status { display: flex; align-items: center; justify-content: flex-end; gap: 7px; }
  .row-status small { padding: 4px 8px; border-radius: 999px; color: var(--primary); background: color-mix(in oklch, var(--primary) 9%, transparent); font-size: 0.68rem; font-weight: 680; white-space: nowrap; }
  .settled-status { display: inline-flex; align-items: center; gap: 7px; }
  .status-dot { width: 7px; height: 7px; flex: none; border-radius: 50%; background: var(--audio-microphone); }
  .settled-status.complete { color: var(--success); background: color-mix(in oklch, var(--success) 10%, transparent); }
  .settled-status.complete .status-dot { background: var(--success); }
  .settled-status.missing { color: var(--destructive); background: color-mix(in oklch, var(--destructive) 9%, transparent); }
  .settled-status.missing .status-dot { background: var(--destructive); }
  .processing-status { display: inline-flex; align-items: center; gap: 6px; }
  .processing-indicator { box-sizing: border-box; width: 11px; height: 11px; border: 2px solid color-mix(in oklch, var(--primary) 24%, transparent); border-top-color: var(--primary); border-radius: 50%; animation: processing-spin 800ms linear infinite; }
  .row-status .missing-audio { color: var(--destructive); }
  .meeting-row > :global(svg) { width: 17px; height: 17px; color: var(--muted-foreground); stroke-width: 1.8; }
  .library-message { margin: 0; padding: 28px 12px; color: var(--muted-foreground); font-size: 0.78rem; text-align: center; }

  @media (max-width: 780px) {
    .meeting-home { width: 100%; grid-template-rows: auto minmax(0, 1fr); }
    .home-intro { display: none; }
    .recording-area { padding: 0 14px; }
    .meeting-list-shell { display: grid; grid-template-rows: auto minmax(0, 1fr); }
    .meeting-library-header { display: flex; gap: 12px; padding: 14px 14px 7px; }
    .meeting-library-header h2 { font-size: 0.86rem; }
    .import-audio-button { padding: 6px 7px; font-size: 0.68rem; }
    .meeting-list { padding-bottom: calc(50vw + 100px + env(safe-area-inset-bottom, 0px)); }
    .meeting-list-shell::after { position: fixed; z-index: 1; right: 0; bottom: 0; left: 0; height: calc(50vw + 100px); pointer-events: none; background: linear-gradient(to bottom, transparent, color-mix(in srgb, var(--background) 72%, transparent) 58px, var(--background) 100px, var(--background)); content: ""; }
    .meeting-row { min-height: 68px; grid-template-columns: 38px minmax(0, 1fr) auto 18px; gap: 9px; padding: 9px 14px; }
    .row-icon { width: 34px; height: 34px; }
    .row-icon :global(svg) { width: 20px; height: 20px; }
    .row-copy strong { font-size: 0.8rem; }
    .row-copy small { font-size: 0.65rem; }
    .row-status small { font-size: 0.62rem; }
  }

  @media (max-width: 430px) {
    .row-status { display: none; }
    .row-status.processing { display: flex; }
  }

  @keyframes processing-spin {
    to { transform: rotate(360deg); }
  }

  @media (prefers-reduced-motion: reduce) {
    .processing-indicator { animation: none; }
  }
</style>
