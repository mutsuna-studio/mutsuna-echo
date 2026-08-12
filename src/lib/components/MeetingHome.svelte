<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import FileUp from "@lucide/svelte/icons/file-up";
  import Mic from "@lucide/svelte/icons/mic";
  import { scrollbarVisibility } from "@mutsuna/ui/scrollbar";
  import { formatFileSize } from "../format";
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
  let recordingSettingsExpanded = $state(false);
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
    canExpand: boolean;
    canCollapse: boolean;
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
      canExpand: recordingBusy || (meetingListElement?.scrollTop ?? 0) <= 1,
      canCollapse: recordingSettingsExpanded,
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
    if (pullGesture.startedOnRecordButton && pullGesture.canCollapse && isVertical && verticalDistance <= -10) {
      event.preventDefault();
      recordButtonSwipeConsumed = true;
      if (recordButtonSwipeResetTimer != null) window.clearTimeout(recordButtonSwipeResetTimer);
      recordButtonSwipeResetTimer = window.setTimeout(() => {
        recordButtonSwipeConsumed = false;
        recordButtonSwipeResetTimer = null;
      }, 500);
      if (verticalDistance <= -36) {
        recordingSettingsExpanded = false;
        pullGesture = null;
      }
      return;
    }
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
    if (isVertical && ((verticalDistance > 0 && pullGesture.canExpand) || (verticalDistance < 0 && pullGesture.canCollapse))) {
      event.preventDefault();
    }
    if (Math.abs(verticalDistance) < 36 || !isVertical) return;
    if (verticalDistance > 0 && pullGesture.canExpand) recordingSettingsExpanded = true;
    if (verticalDistance < 0 && pullGesture.canCollapse) recordingSettingsExpanded = false;
    pullGesture = null;
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
      mobileExpanded={recordingSettingsExpanded}
      consumeMobileAction={consumeRecordButtonSwipe}
      {onAudioReady}
      onBusyChange={onRecordingBusyChange}
      {onMessage}
      {onError}
    />
  </div>

  {#if !recordingBusy}
  <div class="meeting-list-shell">
    <div
      class="meeting-list mutsuna-scrollbar mutsuna-scrollbar--both-edges"
      bind:this={meetingListElement}
      use:scrollbarVisibility
    >
      <button class="file-row" type="button" onclick={onSelectFile} disabled={busy}>
        <span class="row-icon imported" aria-hidden="true"><FileUp /></span>
        <span class="row-copy">
          <strong>{selecting ? "音声ファイルを選択中…" : "音声ファイルを選択"}</strong>
          <small>録音済みの音声を文字起こし</small>
        </span>
        <ChevronRight aria-hidden="true" />
      </button>

      {#if loading && meetings.length === 0}
        <p class="library-message" role="status">会議を読み込んでいます…</p>
      {:else if meetings.length === 0}
        <p class="library-message">録音すると、ここに会議が追加されます。</p>
      {:else}
        {#each meetings as meeting (meeting.meetingId)}
          {@const activeProcessingStatus = meeting.meetingId === processingMeetingId ? processingStatus : null}
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
              <small>{meetingDate(meeting)} · {formatFileSize(meeting.sizeBytes)}</small>
            </span>
            <span class:processing={activeProcessingStatus != null} class="row-status">
              {#if activeProcessingStatus}
                <small class="processing-status" aria-live="polite"><span class="processing-indicator" aria-hidden="true"></span>{activeProcessingStatus}</small>
              {:else if meeting.transcriptProviders.length > 0}
                <small>文字起こし済み</small>
              {/if}
              {#if !meeting.audioAvailable}<small class="missing-audio">音声なし</small>{/if}
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
  .recording-area { position: relative; z-index: 2; border-bottom: 1px solid var(--border); }
  .recording-area.active { border-bottom: 0; }
  .meeting-list-shell { position: relative; min-height: 0; overflow: hidden; }
  .meeting-list-shell::after { position: absolute; z-index: 2; right: 0; bottom: 0; left: 0; height: 106px; pointer-events: none; background: linear-gradient(to bottom, transparent, color-mix(in srgb, var(--background) 68%, transparent) 42%, var(--background)); content: ""; }
  .meeting-list { height: 100%; overflow-x: hidden; overflow-y: auto; overscroll-behavior: contain; scrollbar-gutter: stable both-edges; }
  .file-row,
  .meeting-row { display: grid; width: 100%; min-width: 0; grid-template-columns: 44px minmax(0, 1fr) auto 20px; align-items: center; gap: 15px; min-height: 70px; padding: 10px 12px; border: 0; border-bottom: 1px solid var(--border); border-radius: 0; color: var(--foreground); background: transparent; cursor: pointer; font: inherit; text-align: left; }
  .file-row { grid-template-columns: 44px minmax(0, 1fr) 20px; }
  .file-row:hover,
  .meeting-row:hover { background: color-mix(in oklch, var(--muted) 55%, var(--background)); }
  .file-row:focus-visible,
  .meeting-row:focus-visible { position: relative; z-index: 1; outline: 2px solid var(--ring); outline-offset: -3px; }
  .file-row:disabled,
  .meeting-row:disabled { cursor: not-allowed; opacity: 0.56; }
  .row-icon { display: grid; width: 38px; height: 38px; place-items: center; color: var(--primary); }
  .row-icon.imported { color: var(--muted-foreground); }
  .row-icon :global(svg) { width: 21px; height: 21px; stroke-width: 1.8; }
  .row-copy { display: grid; min-width: 0; gap: 5px; }
  .row-copy strong { overflow: hidden; font-size: 0.86rem; font-weight: 680; text-overflow: ellipsis; white-space: nowrap; }
  .row-copy small { color: var(--muted-foreground); font-size: 0.7rem; }
  .row-status { display: flex; align-items: center; justify-content: flex-end; gap: 7px; }
  .row-status small { color: var(--primary); font-size: 0.68rem; font-weight: 650; white-space: nowrap; }
  .processing-status { display: inline-flex; align-items: center; gap: 6px; }
  .processing-indicator { box-sizing: border-box; width: 11px; height: 11px; border: 2px solid color-mix(in oklch, var(--primary) 24%, transparent); border-top-color: var(--primary); border-radius: 50%; animation: processing-spin 800ms linear infinite; }
  .row-status .missing-audio { color: var(--destructive); }
  .file-row > :global(svg),
  .meeting-row > :global(svg) { width: 17px; height: 17px; color: var(--muted-foreground); stroke-width: 1.8; }
  .library-message { margin: 0; padding: 28px 12px; color: var(--muted-foreground); font-size: 0.78rem; text-align: center; }

  @media (max-width: 780px) {
    .meeting-home { width: 100%; grid-template-rows: auto minmax(0, 1fr); }
    .home-intro { display: none; }
    .recording-area { padding: 0 14px; }
    .meeting-list { padding-bottom: calc(50vw + 28px + env(safe-area-inset-bottom, 0px)); }
    .meeting-list-shell::after { position: fixed; z-index: 1; bottom: 0; height: calc(50vw + 100px); background: linear-gradient(to bottom, transparent, color-mix(in srgb, var(--background) 72%, transparent) 58px, var(--background) 100px, var(--background)); }
    .file-row,
    .meeting-row { min-height: 68px; grid-template-columns: 38px minmax(0, 1fr) auto 18px; gap: 9px; padding: 9px 14px; }
    .file-row { grid-template-columns: 38px minmax(0, 1fr) 18px; }
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
