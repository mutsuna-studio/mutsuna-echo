<script lang="ts">
  import { tick, untrack } from "svelte";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import Search from "@lucide/svelte/icons/search";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import X from "@lucide/svelte/icons/x";
  import { Button } from "@mutsuna/ui/button";
  import { scrollbarVisibility } from "@mutsuna/ui/scrollbar";
  import { formatTimestamp } from "../format";
  import type { EditableTranscript, TranscriptSegmentTextChange } from "../types/transcript";

  type SearchMatch = {
    segmentId: string;
    segmentIndex: number;
    start: number;
    end: number;
  };

  type Props = {
    transcript: EditableTranscript;
    transcriptionId: string | null;
    currentPositionMs: number;
    playing: boolean;
    playbackAvailable: boolean;
    followRequestId: number;
    scrollContainer: HTMLElement | null;
    onSeek: (positionMs: number) => void;
    onPlay: (positionMs: number) => void;
    onPause: () => void;
    editable: boolean;
    formatting: boolean;
    onEditSegment: (segmentId: string, text: string) => void;
    onEditSpeakerLabel: (speaker: string, label: string) => void;
    onReplaceSegments: (changes: TranscriptSegmentTextChange[]) => Promise<boolean>;
    onFormat: () => Promise<void>;
    canUndoReplacement: boolean;
    onUndoReplacement: () => Promise<void>;
    onBlur: () => Promise<void>;
  };

  let {
    transcript,
    transcriptionId,
    currentPositionMs,
    playing,
    playbackAvailable,
    followRequestId,
    scrollContainer,
    onSeek,
    onPlay,
    onPause,
    editable,
    formatting,
    onEditSegment,
    onEditSpeakerLabel,
    onReplaceSegments,
    onFormat,
    canUndoReplacement,
    onUndoReplacement,
    onBlur
  }: Props = $props();

  const SEGMENT_PAGE_SIZE = 100;
  let visibleCount = $state(SEGMENT_PAGE_SIZE);
  let transcriptElement = $state<HTMLElement | null>(null);
  let replaceOpen = $state(false);
  let searchText = $state("");
  let replacementText = $state("");
  let currentMatchIndex = $state(0);
  let replacing = $state(false);
  let pinnedPlaybackSegmentId = $state<string | null>(null);
  const composingSegments = new Set<string>();
  const composingSpeakers = new Set<string>();
  const pendingEditorResizes = new Set<HTMLTextAreaElement>();
  let editorResizeFrame = 0;
  const visibleSegments = $derived(transcript.segments.slice(0, visibleCount));
  const remainingSegments = $derived(Math.max(0, transcript.segments.length - visibleSegments.length));
  const playbackIndex = $derived(playbackAvailable ? segmentIndexAt(transcript, currentPositionMs) : -1);
  const pinnedPlaybackIndex = $derived(
    pinnedPlaybackSegmentId
      ? transcript.segments.findIndex((segment) => segment.segmentId === pinnedPlaybackSegmentId)
      : -1
  );
  const activeIndex = $derived(pinnedPlaybackIndex >= 0 ? pinnedPlaybackIndex : playbackIndex);
  const searchMatches = $derived.by(() => findSearchMatches(transcript, searchText));
  const currentMatch = $derived(searchMatches[currentMatchIndex] ?? null);

  $effect(() => {
    transcriptionId;
    visibleCount = SEGMENT_PAGE_SIZE;
    replaceOpen = false;
    searchText = "";
    replacementText = "";
    currentMatchIndex = 0;
    pinnedPlaybackSegmentId = null;
  });

  $effect(() => {
    if (!pinnedPlaybackSegmentId) return;
    const segment = transcript.segments.find((candidate) => candidate.segmentId === pinnedPlaybackSegmentId);
    if (!segment || currentPositionMs >= segment.startMs) pinnedPlaybackSegmentId = null;
  });

  $effect(() => {
    const root = transcriptElement;
    if (!root) return;

    let observedWidth = root.clientWidth;
    const resizeObserver = new ResizeObserver(([entry]) => {
      const width = entry?.contentRect.width ?? root.clientWidth;
      if (width === observedWidth) return;
      observedWidth = width;
      scheduleEditorResize(root.querySelectorAll<HTMLTextAreaElement>("textarea.segment-text"));
    });
    resizeObserver.observe(root);

    return () => {
      resizeObserver.disconnect();
    };
  });

  $effect(() => {
    searchText;
    searchMatches.length;
    if (currentMatchIndex >= searchMatches.length) currentMatchIndex = Math.max(0, searchMatches.length - 1);
  });

  $effect(() => {
    const index = activeIndex;
    followRequestId;
    // Clicking a segment seeks to its three-second preroll. Keep the clicked
    // segment selected without moving the list underneath the pointer.
    if (untrack(() => pinnedPlaybackSegmentId) !== null) return;
    const container = scrollContainer;
    const root = transcriptElement;
    if (index < 0 || !container || !root) return;
    let cancelled = false;
    let frame = 0;
    const currentVisibleCount = untrack(() => visibleCount);
    if (index >= currentVisibleCount) {
      visibleCount = Math.ceil((index + 1) / SEGMENT_PAGE_SIZE) * SEGMENT_PAGE_SIZE;
    }
    void tick().then(() => {
      if (cancelled) return;
      frame = requestAnimationFrame(() => {
        const element = root.querySelector<HTMLElement>(`[data-segment-index="${index}"]`);
        if (!element) return;
        scrollToSegment(container, element);
      });
    });
    return () => {
      cancelled = true;
      if (frame) cancelAnimationFrame(frame);
    };
  });

  function scrollToSegment(container: HTMLElement, element: HTMLElement) {
    const viewport = container.getBoundingClientRect();
    const segment = element.getBoundingClientRect();
    const centeredTop = container.scrollTop
      + segment.top
      - viewport.top
      - (container.clientHeight - segment.height) / 2;
    const maximum = Math.max(0, container.scrollHeight - container.clientHeight);
    container.scrollTop = Math.round(Math.min(Math.max(0, centeredTop), maximum));
  }

  function scheduleEditorResize(elements: Iterable<HTMLTextAreaElement>) {
    for (const element of elements) pendingEditorResizes.add(element);
    if (editorResizeFrame) return;
    editorResizeFrame = requestAnimationFrame(() => {
      editorResizeFrame = 0;
      const elements = Array.from(pendingEditorResizes).filter((element) => element.isConnected);
      pendingEditorResizes.clear();
      resizeTextAreas(elements);
    });
  }

  function resizeTextAreas(elements: HTMLTextAreaElement[]) {
    for (const element of elements) element.style.height = "auto";
    const heights = elements.map((element) => element.scrollHeight);
    elements.forEach((element, index) => {
      element.style.height = `${heights[index]}px`;
    });
  }

  function autoResizeTextArea(element: HTMLTextAreaElement, _text: string) {
    scheduleEditorResize([element]);
    return {
      update() {
        scheduleEditorResize([element]);
      },
      destroy() {
        pendingEditorResizes.delete(element);
      }
    };
  }

  function editSegment(event: Event, segmentId: string) {
    const element = event.currentTarget as HTMLTextAreaElement;
    scheduleEditorResize([element]);
    if (composingSegments.has(segmentId)) return;
    onEditSegment(segmentId, element.value);
  }

  function playSegment(segment: EditableTranscript["segments"][number]) {
    pinnedPlaybackSegmentId = segment.segmentId;
    onPlay(segment.startMs);
  }

  function finishComposition(event: CompositionEvent, segmentId: string) {
    composingSegments.delete(segmentId);
    editSegment(event, segmentId);
  }

  function showMoreSegments() {
    visibleCount += SEGMENT_PAGE_SIZE;
  }

  function editSpeaker(event: Event, speaker: string) {
    if (composingSpeakers.has(speaker)) return;
    onEditSpeakerLabel(speaker, (event.currentTarget as HTMLInputElement).value);
  }

  function finishSpeakerComposition(event: CompositionEvent, speaker: string) {
    composingSpeakers.delete(speaker);
    editSpeaker(event, speaker);
  }

  function speakerLabel(speaker: string): string {
    const label = transcript.speakerLabels.find((entry) => entry.speaker === speaker)?.label.trim();
    return label || speaker;
  }

  function toggleReplace() {
    replaceOpen = !replaceOpen;
    if (!replaceOpen) return;
    void tick().then(() => transcriptElement?.querySelector<HTMLInputElement>(".replace-search")?.focus());
  }

  function updateSearchText(event: Event) {
    searchText = (event.currentTarget as HTMLInputElement).value;
    currentMatchIndex = 0;
  }

  function updateReplacementText(event: Event) {
    replacementText = (event.currentTarget as HTMLInputElement).value;
  }

  function moveMatch(direction: number) {
    if (searchMatches.length === 0) return;
    currentMatchIndex = (currentMatchIndex + direction + searchMatches.length) % searchMatches.length;
    void focusCurrentMatch();
  }

  async function focusCurrentMatch() {
    await tick();
    const match = searchMatches[currentMatchIndex];
    if (!match || !transcriptElement) return;
    if (match.segmentIndex >= visibleCount) {
      visibleCount = Math.ceil((match.segmentIndex + 1) / SEGMENT_PAGE_SIZE) * SEGMENT_PAGE_SIZE;
      await tick();
    }
    const element = transcriptElement.querySelector<HTMLTextAreaElement>(
      `textarea[data-segment-id="${CSS.escape(match.segmentId)}"]`
    );
    if (!element) return;
    element.focus({ preventScroll: true });
    element.setSelectionRange(match.start, match.end);
    if (scrollContainer) scrollToSegment(scrollContainer, element.closest<HTMLElement>(".segment") ?? element);
  }

  async function replaceCurrentMatch() {
    const match = currentMatch;
    if (!match || replacing) return;
    const segment = transcript.segments[match.segmentIndex];
    if (!segment || segment.segmentId !== match.segmentId) return;
    const text = `${segment.text.slice(0, match.start)}${replacementText}${segment.text.slice(match.end)}`;
    replacing = true;
    try {
      if (await onReplaceSegments([{ segmentId: segment.segmentId, text }])) {
        currentMatchIndex = Math.min(currentMatchIndex, Math.max(0, searchMatches.length - 1));
        void focusCurrentMatch();
      }
    } finally {
      replacing = false;
    }
  }

  async function replaceAllMatches() {
    if (searchMatches.length === 0 || !searchText || replacing) return;
    const changes = transcript.segments.flatMap((segment) => {
      if (!segment.text.includes(searchText)) return [];
      return [{ segmentId: segment.segmentId, text: segment.text.split(searchText).join(replacementText) }];
    });
    replacing = true;
    try {
      await onReplaceSegments(changes);
    } finally {
      replacing = false;
    }
  }

  function findSearchMatches(value: EditableTranscript, query: string): SearchMatch[] {
    if (!query) return [];
    const matches: SearchMatch[] = [];
    value.segments.forEach((segment, segmentIndex) => {
      let start = 0;
      while (start <= segment.text.length - query.length) {
        const index = segment.text.indexOf(query, start);
        if (index < 0) break;
        matches.push({ segmentId: segment.segmentId, segmentIndex, start: index, end: index + query.length });
        start = index + query.length;
      }
    });
    return matches;
  }

  function segmentIndexAt(value: EditableTranscript, positionMs: number): number {
    const segments = value.segments;
    if (segments.length === 0) return -1;
    let low = 0;
    let high = segments.length - 1;
    let result = 0;
    while (low <= high) {
      const middle = Math.floor((low + high) / 2);
      if (segments[middle].startMs <= positionMs) {
        result = middle;
        low = middle + 1;
      } else {
        high = middle - 1;
      }
    }
    return result;
  }
</script>

<section class="transcript-view" bind:this={transcriptElement} aria-label="文字起こし結果">
  {#if transcript.segments.length > 0}
    {#if editable}
      <div class="correction-toolbar">
        <Button size="sm" variant="outline" type="button" icon={Sparkles} disabled={formatting} loading={formatting} onclick={() => void onFormat()}>{formatting ? "整形中…" : "整形"}</Button>
        <Button size="sm" variant={replaceOpen ? "secondary" : "ghost"} type="button" icon={Search} aria-expanded={replaceOpen} disabled={formatting} onclick={toggleReplace}>検索・置換</Button>
        {#if canUndoReplacement}
          <Button size="sm" variant="ghost" type="button" icon={Undo2} disabled={formatting} onclick={() => void onUndoReplacement()}>一括編集を元に戻す</Button>
        {/if}
      </div>
      {#if replaceOpen}
        <section
          class="replace-panel mutsuna-scrollbar mutsuna-scrollbar--both-edges"
          aria-label="文字起こしを検索・置換"
          use:scrollbarVisibility
        >
          <div class="mobile-replace-heading">
            <strong>検索・置換</strong>
            <Button size="icon-sm" variant="ghost" type="button" icon={X} aria-label="検索・置換を閉じる" title="閉じる" disabled={formatting} onclick={toggleReplace} />
          </div>
          <div class="replace-fields">
            <label>
              <span>検索</span>
              <input class="replace-search" type="search" value={searchText} placeholder="修正したい文字" disabled={formatting} oninput={updateSearchText} />
            </label>
            <label>
              <span>置換後</span>
              <input type="text" value={replacementText} placeholder="正しい表記" disabled={formatting} oninput={updateReplacementText} />
            </label>
          </div>
          <div class="replace-actions">
            <span class="match-navigation">
              <Button size="icon-sm" variant="ghost" type="button" icon={ChevronLeft} aria-label="前の検索結果" title="前の検索結果" disabled={searchMatches.length === 0 || replacing || formatting} onclick={() => moveMatch(-1)} />
              <span class="match-count" aria-live="polite">{searchMatches.length > 0 ? `${currentMatchIndex + 1} / ${searchMatches.length}` : searchText ? "0件" : "—"}</span>
              <Button size="icon-sm" variant="ghost" type="button" icon={ChevronRight} aria-label="次の検索結果" title="次の検索結果" disabled={searchMatches.length === 0 || replacing || formatting} onclick={() => moveMatch(1)} />
            </span>
            <span class="replace-buttons">
              <Button size="sm" variant="outline" type="button" disabled={!currentMatch || replacing || formatting} onclick={() => void replaceCurrentMatch()}>置換</Button>
              <Button size="sm" type="button" disabled={searchMatches.length === 0 || replacing || formatting} loading={replacing} onclick={() => void replaceAllMatches()}>すべて置換</Button>
            </span>
            <span class="desktop-replace-close">
              <Button size="icon-sm" variant="ghost" type="button" icon={X} aria-label="検索・置換を閉じる" title="閉じる" disabled={formatting} onclick={toggleReplace} />
            </span>
          </div>
        </section>
      {/if}
    {/if}
    {#if editable && transcript.speakerLabels.length > 0}
      <section class="speaker-labels" aria-label="話者ラベル">
        <div class="speaker-labels-heading">
          <strong>話者ラベル</strong>
          <span>STTが分離した話者へ名前を設定します</span>
        </div>
        <div class="speaker-label-list">
          {#each transcript.speakerLabels as entry (entry.speaker)}
            <label class:edited={entry.edited}>
              <span>{entry.speaker}</span>
              <input
                type="text"
                value={entry.label}
                placeholder={entry.speaker}
                aria-label={`${entry.speaker}の表示名`}
                disabled={formatting}
                oncompositionstart={() => composingSpeakers.add(entry.speaker)}
                oncompositionend={(event) => finishSpeakerComposition(event, entry.speaker)}
                oninput={(event) => editSpeaker(event, entry.speaker)}
                onblur={() => void onBlur()}
              />
            </label>
          {/each}
        </div>
      </section>
    {/if}
    <div class="segments">
      {#each visibleSegments as segment, index (`${segment.segmentId}-${index}`)}
        <article
          class:active={index === activeIndex}
          class:edited={segment.edited}
          class="segment"
          data-segment-index={index}
          aria-current={index === activeIndex ? "true" : undefined}
        >
          <span class="segment-meta">
            <span class="segment-timing">
              {#if playbackAvailable}
                <button
                  class="timestamp"
                  type="button"
                  title={`${formatTimestamp(segment.startMs)} – ${formatTimestamp(segment.endMs)}へ移動`}
                  aria-label={`${formatTimestamp(segment.startMs)}へ移動`}
                  onclick={() => onSeek(segment.startMs)}
                >{formatTimestamp(segment.startMs)}</button>
                <Button
                  size="icon-xs"
                  variant="ghost"
                  type="button"
                  icon={playing && index === activeIndex ? Pause : Play}
                  aria-label={playing && index === activeIndex ? "一時停止" : `${formatTimestamp(segment.startMs)}の3秒前から再生`}
                  title={playing && index === activeIndex ? "一時停止" : "3秒前から再生"}
                  onclick={() => playing && index === activeIndex ? onPause() : playSegment(segment)}
                />
              {:else}
                <span class="timestamp">{formatTimestamp(segment.startMs)}</span>
              {/if}
            </span>
            <span class="segment-heading">
              <strong title={segment.speaker}>{speakerLabel(segment.speaker)}</strong>
            </span>
          </span>
          {#if editable}
            <textarea
              class="segment-text editor"
              value={segment.text}
              data-segment-id={segment.segmentId}
              rows="1"
              use:autoResizeTextArea={segment.text}
              aria-label={`${formatTimestamp(segment.startMs)}、${segment.speaker}の文字起こしを編集`}
              disabled={formatting}
              oncompositionstart={() => composingSegments.add(segment.segmentId)}
              oncompositionend={(event) => finishComposition(event, segment.segmentId)}
              oninput={(event) => editSegment(event, segment.segmentId)}
              onblur={() => void onBlur()}
            ></textarea>
          {:else}
            <span class="segment-text">{segment.text}</span>
          {/if}
        </article>
      {/each}
    </div>
    {#if remainingSegments > 0}
      <div class="transcript-more">
        <Button variant="outline" type="button" onclick={showMoreSegments}>
          続きを表示（残り{remainingSegments.toLocaleString("ja-JP")}件）
        </Button>
      </div>
    {/if}
  {:else}
    <p class="empty-result">発話は検出されませんでした。</p>
  {/if}
</section>

<style>
  .transcript-view { min-width: 0; }
  .correction-toolbar { display: flex; min-height: 42px; align-items: center; justify-content: flex-end; gap: 4px; padding: 7px 0 3px; }
  .replace-panel { display: flex; min-width: 0; flex-wrap: wrap; align-items: flex-end; gap: 8px; margin: 4px 0 8px; padding: 8px; border: 1px solid var(--border); border-radius: 9px; background: color-mix(in oklch, var(--muted) 28%, var(--background)); }
  .mobile-replace-heading { display: none; }
  .replace-fields { display: grid; min-width: 260px; flex: 1 1 360px; grid-template-columns: repeat(2, minmax(120px, 1fr)); gap: 6px; }
  .replace-fields label { display: grid; min-width: 0; gap: 3px; }
  .replace-fields label > span { color: var(--muted-foreground); font-size: 0.64rem; font-weight: 650; line-height: 1; }
  .replace-fields input { width: 100%; min-width: 0; height: 30px; padding: 0 8px; border: 1px solid var(--border); border-radius: 7px; color: var(--foreground); background: var(--background); font: inherit; font-size: 0.76rem; }
  .replace-fields input:focus { border-color: color-mix(in oklch, var(--primary) 55%, var(--border)); outline: 2px solid color-mix(in oklch, var(--primary) 18%, transparent); }
  .replace-actions { display: flex; flex: 0 0 auto; margin-left: auto; align-items: center; gap: 4px; }
  .match-navigation { display: flex; align-items: center; gap: 1px; color: var(--muted-foreground); font-size: 0.68rem; font-variant-numeric: tabular-nums; }
  .match-count { min-width: 38px; text-align: center; white-space: nowrap; }
  .replace-buttons { display: flex; align-items: center; gap: 4px; }
  .desktop-replace-close { display: flex; }
  .speaker-labels { margin: 14px 0 8px; padding: 13px 14px; border: 1px solid var(--border); border-radius: 10px; background: color-mix(in oklch, var(--muted) 28%, var(--background)); }
  .speaker-labels-heading { display: flex; align-items: baseline; gap: 9px; margin-bottom: 10px; }
  .speaker-labels-heading strong { font-size: 0.78rem; }
  .speaker-labels-heading span { color: var(--muted-foreground); font-size: 0.68rem; }
  .speaker-label-list { display: flex; flex-wrap: wrap; gap: 8px; }
  .speaker-label-list label { display: grid; min-width: 180px; grid-template-columns: auto minmax(90px, 1fr); align-items: center; gap: 7px; padding: 5px 6px 5px 9px; border: 1px solid var(--border); border-radius: 8px; background: var(--background); }
  .speaker-label-list label.edited { border-color: color-mix(in oklch, var(--primary) 45%, var(--border)); }
  .speaker-label-list span { color: var(--muted-foreground); font-size: 0.67rem; white-space: nowrap; }
  .speaker-label-list input { min-width: 0; width: 100%; padding: 5px 7px; border: 0; border-radius: 5px; color: var(--foreground); background: color-mix(in oklch, var(--muted) 40%, transparent); font: inherit; font-size: 0.76rem; }
  .speaker-label-list input:focus { outline: 2px solid color-mix(in oklch, var(--primary) 25%, transparent); background: var(--background); }
  .segments { display: grid; }
  .segment { display: grid; width: 100%; grid-template-columns: 84px minmax(0, 1fr); gap: 12px; padding: 10px 8px; border: 0; border-bottom: 1px solid var(--border); color: inherit; background: transparent; font: inherit; text-align: left; transition: background-color 120ms ease, box-shadow 120ms ease; }
  .segment:hover { background: color-mix(in oklch, var(--primary) 4%, transparent); }
  .segment.active { background: color-mix(in oklch, var(--primary) 8%, transparent); box-shadow: inset 3px 0 var(--primary); }
  .segment.edited:not(.active) { box-shadow: inset 2px 0 color-mix(in oklch, var(--primary) 45%, transparent); }
  .segment-meta { display: contents; }
  .segment-timing { display: flex; align-self: start; grid-column: 1; grid-row: 1 / span 2; align-items: center; gap: 2px; }
  .timestamp { padding: 2px 1px; border: 0; border-radius: 4px; color: var(--primary); background: transparent; cursor: pointer; font: inherit; font-size: 0.76rem; font-variant-numeric: tabular-nums; }
  .timestamp:hover { background: color-mix(in oklch, var(--primary) 10%, transparent); }
  .timestamp:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
  .segment-heading { display: flex; min-width: 0; grid-column: 2; align-items: center; justify-content: space-between; gap: 10px; }
  .segment-heading strong { overflow: hidden; color: var(--foreground); font-size: 0.78rem; text-overflow: ellipsis; white-space: nowrap; }
  .segment-text { min-width: 0; grid-column: 2; margin-top: 2px; overflow-wrap: anywhere; font-size: 0.88rem; line-height: 1.55; white-space: pre-wrap; }
  .editor { box-sizing: border-box; display: block; width: 100%; min-height: calc(1.55em + 4px); resize: none; overflow: hidden; padding: 1px 4px; border: 1px solid transparent; border-radius: 6px; color: var(--foreground); background: transparent; font: inherit; font-size: 0.88rem; line-height: 1.55; }
  .editor:hover { border-color: color-mix(in oklch, var(--border) 75%, transparent); background: color-mix(in oklch, var(--background) 70%, transparent); }
  .editor:focus { border-color: color-mix(in oklch, var(--primary) 55%, var(--border)); outline: 2px solid color-mix(in oklch, var(--primary) 18%, transparent); background: var(--background); }
  .transcript-more {
    display: flex;
    justify-content: center;
    margin-top: 18px;
  }
  .empty-result { margin: 28px 0 0; color: var(--muted-foreground); font-size: 0.84rem; }

  @media (max-width: 780px) {
    .replace-panel { position: fixed; z-index: 40; top: 0; right: 0; left: 0; max-height: 100dvh; align-items: stretch; margin: 0; padding: calc(12px + env(safe-area-inset-top, 0px)) calc(14px + env(safe-area-inset-right, 0px)) 14px calc(14px + env(safe-area-inset-left, 0px)); overflow-y: auto; border-width: 0 0 1px; border-radius: 0 0 16px 16px; background: var(--background); box-shadow: 0 14px 36px rgb(0 0 0 / 18%); animation: replace-panel-enter 180ms cubic-bezier(0.22, 1, 0.36, 1); }
    .mobile-replace-heading { display: flex; width: 100%; align-items: center; justify-content: space-between; }
    .mobile-replace-heading strong { font-size: 0.84rem; }
    .replace-fields { flex-basis: 100%; }
    .replace-actions { width: 100%; flex-wrap: wrap; justify-content: flex-end; }
    .match-navigation { margin-right: auto; }
    .desktop-replace-close { display: none; }
  }

  @media (max-width: 600px) {
    .replace-fields { grid-template-columns: minmax(0, 1fr); }
    .segment { grid-template-columns: 64px minmax(0, 1fr); gap: 8px; padding: 9px 4px; }
  }

  @keyframes replace-panel-enter {
    from { transform: translateY(-100%); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }

  @media (prefers-reduced-motion: reduce) {
    .replace-panel { animation: none; }
  }
</style>
