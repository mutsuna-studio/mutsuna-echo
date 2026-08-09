<script lang="ts">
  import { tick } from "svelte";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Play from "@lucide/svelte/icons/play";
  import Search from "@lucide/svelte/icons/search";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import X from "@lucide/svelte/icons/x";
  import { Button } from "@mutsuna/ui/button";
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
    followRequestId: number;
    scrollContainer: HTMLElement | null;
    onSeek: (positionMs: number) => void;
    onPlay: (positionMs: number) => void;
    editable: boolean;
    onEditSegment: (segmentId: string, text: string) => void;
    onEditSpeakerLabel: (speaker: string, label: string) => void;
    onReplaceSegments: (changes: TranscriptSegmentTextChange[]) => Promise<boolean>;
    canUndoReplacement: boolean;
    onUndoReplacement: () => Promise<void>;
    onBlur: () => Promise<void>;
  };

  let {
    transcript,
    transcriptionId,
    currentPositionMs,
    followRequestId,
    scrollContainer,
    onSeek,
    onPlay,
    editable,
    onEditSegment,
    onEditSpeakerLabel,
    onReplaceSegments,
    canUndoReplacement,
    onUndoReplacement,
    onBlur
  }: Props = $props();

  const SEGMENT_PAGE_SIZE = 300;
  let visibleCount = $state(SEGMENT_PAGE_SIZE);
  let transcriptElement = $state<HTMLElement | null>(null);
  let replaceOpen = $state(false);
  let searchText = $state("");
  let replacementText = $state("");
  let currentMatchIndex = $state(0);
  let replacing = $state(false);
  const composingSegments = new Set<string>();
  const composingSpeakers = new Set<string>();
  const visibleSegments = $derived(transcript.segments.slice(0, visibleCount));
  const remainingSegments = $derived(Math.max(0, transcript.segments.length - visibleSegments.length));
  const activeIndex = $derived(segmentIndexAt(transcript, currentPositionMs));
  const searchMatches = $derived.by(() => findSearchMatches(transcript, searchText));
  const currentMatch = $derived(searchMatches[currentMatchIndex] ?? null);

  $effect(() => {
    transcriptionId;
    visibleCount = SEGMENT_PAGE_SIZE;
    replaceOpen = false;
    searchText = "";
    replacementText = "";
    currentMatchIndex = 0;
  });

  $effect(() => {
    transcript;
    void tick().then(() => {
      transcriptElement?.querySelectorAll<HTMLTextAreaElement>("textarea").forEach(resizeTextArea);
    });
  });

  $effect(() => {
    searchText;
    searchMatches.length;
    if (currentMatchIndex >= searchMatches.length) currentMatchIndex = Math.max(0, searchMatches.length - 1);
  });

  $effect(() => {
    const index = activeIndex;
    followRequestId;
    const container = scrollContainer;
    const root = transcriptElement;
    if (index < 0 || !container || !root) return;
    let cancelled = false;
    let frame = 0;
    if (index >= visibleCount) {
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

  function resizeTextArea(element: HTMLTextAreaElement) {
    element.style.height = "0";
    element.style.height = `${element.scrollHeight}px`;
  }

  function editSegment(event: Event, segmentId: string) {
    const element = event.currentTarget as HTMLTextAreaElement;
    resizeTextArea(element);
    if (composingSegments.has(segmentId)) return;
    onEditSegment(segmentId, element.value);
  }

  function finishComposition(event: CompositionEvent, segmentId: string) {
    composingSegments.delete(segmentId);
    editSegment(event, segmentId);
  }

  function showMoreSegments() {
    visibleCount += SEGMENT_PAGE_SIZE;
    void tick().then(() => {
      transcriptElement?.querySelectorAll<HTMLTextAreaElement>("textarea").forEach(resizeTextArea);
    });
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
    void focusCurrentMatch();
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
        <Button size="sm" variant={replaceOpen ? "secondary" : "ghost"} type="button" icon={Search} aria-expanded={replaceOpen} onclick={toggleReplace}>検索・置換</Button>
        {#if canUndoReplacement}
          <Button size="sm" variant="ghost" type="button" icon={Undo2} onclick={() => void onUndoReplacement()}>一括置換を元に戻す</Button>
        {/if}
      </div>
      {#if replaceOpen}
        <section class="replace-panel" aria-label="文字起こしを検索・置換">
          <div class="replace-fields">
            <label>
              <span>検索</span>
              <input class="replace-search" type="search" value={searchText} placeholder="修正したい文字" oninput={updateSearchText} />
            </label>
            <label>
              <span>置換後</span>
              <input type="text" value={replacementText} placeholder="正しい表記" oninput={updateReplacementText} />
            </label>
          </div>
          <div class="replace-summary" aria-live="polite">
            <span>{searchText ? `${searchMatches.length.toLocaleString("ja-JP")}件` : "検索語を入力してください"}</span>
            {#if searchText && searchMatches.length > 0}
              <small>「{searchText}」→「{replacementText || "（削除）"}」</small>
            {/if}
          </div>
          <div class="replace-actions">
            <span class="match-navigation">
              <Button size="icon-sm" variant="ghost" type="button" icon={ChevronLeft} aria-label="前の検索結果" title="前の検索結果" disabled={searchMatches.length === 0 || replacing} onclick={() => moveMatch(-1)} />
              <span>{searchMatches.length > 0 ? `${currentMatchIndex + 1} / ${searchMatches.length}` : "0 / 0"}</span>
              <Button size="icon-sm" variant="ghost" type="button" icon={ChevronRight} aria-label="次の検索結果" title="次の検索結果" disabled={searchMatches.length === 0 || replacing} onclick={() => moveMatch(1)} />
            </span>
            <Button size="sm" variant="outline" type="button" disabled={!currentMatch || replacing} onclick={() => void replaceCurrentMatch()}>1件置換</Button>
            <Button size="sm" type="button" disabled={searchMatches.length === 0 || replacing} loading={replacing} onclick={() => void replaceAllMatches()}>すべて置換{searchMatches.length > 0 ? `（${searchMatches.length}件）` : ""}</Button>
            <Button size="icon-sm" variant="ghost" type="button" icon={X} aria-label="検索・置換を閉じる" title="閉じる" onclick={toggleReplace} />
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
            <button
              class="timestamp"
              type="button"
              title={`${formatTimestamp(segment.startMs)} – ${formatTimestamp(segment.endMs)}へ移動`}
              aria-label={`${formatTimestamp(segment.startMs)}へ移動`}
              onclick={() => onSeek(segment.startMs)}
            >{formatTimestamp(segment.startMs)}</button>
            <span class="segment-heading">
              <strong title={segment.speaker}>{speakerLabel(segment.speaker)}</strong>
              <Button size="xs" variant="ghost" type="button" icon={Play} aria-label={`${formatTimestamp(segment.startMs)}の3秒前から再生`} title="3秒前から再生" onclick={() => onPlay(segment.startMs)}>再生</Button>
            </span>
          </span>
          {#if editable}
            <textarea
              class="segment-text editor"
              value={segment.text}
              data-segment-id={segment.segmentId}
              rows="1"
              aria-label={`${formatTimestamp(segment.startMs)}、${segment.speaker}の文字起こしを編集`}
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
  .replace-panel { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 10px 14px; margin: 6px 0 10px; padding: 12px; border: 1px solid var(--border); border-radius: 10px; background: color-mix(in oklch, var(--muted) 28%, var(--background)); }
  .replace-fields { display: grid; min-width: 0; grid-template-columns: repeat(2, minmax(120px, 1fr)); gap: 8px; }
  .replace-fields label { display: grid; min-width: 0; gap: 4px; }
  .replace-fields label > span { color: var(--muted-foreground); font-size: 0.67rem; font-weight: 650; }
  .replace-fields input { width: 100%; min-width: 0; height: 32px; padding: 0 9px; border: 1px solid var(--border); border-radius: 7px; color: var(--foreground); background: var(--background); font: inherit; font-size: 0.78rem; }
  .replace-fields input:focus { border-color: color-mix(in oklch, var(--primary) 55%, var(--border)); outline: 2px solid color-mix(in oklch, var(--primary) 18%, transparent); }
  .replace-summary { display: flex; min-width: 120px; flex-direction: column; justify-content: center; gap: 2px; color: var(--foreground); font-size: 0.74rem; font-weight: 650; }
  .replace-summary small { overflow: hidden; max-width: 240px; color: var(--muted-foreground); font-size: 0.65rem; font-weight: 500; text-overflow: ellipsis; white-space: nowrap; }
  .replace-actions { display: flex; grid-column: 1 / -1; align-items: center; justify-content: flex-end; gap: 5px; }
  .match-navigation { display: flex; margin-right: auto; align-items: center; gap: 3px; color: var(--muted-foreground); font-size: 0.68rem; font-variant-numeric: tabular-nums; }
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
  .segment { display: grid; width: 100%; grid-template-columns: 84px minmax(0, 1fr); gap: 18px; padding: 17px 10px; border: 0; border-bottom: 1px solid var(--border); color: inherit; background: transparent; font: inherit; text-align: left; transition: background-color 120ms ease, box-shadow 120ms ease; }
  .segment:hover { background: color-mix(in oklch, var(--primary) 4%, transparent); }
  .segment.active { background: color-mix(in oklch, var(--primary) 8%, transparent); box-shadow: inset 3px 0 var(--primary); }
  .segment.edited:not(.active) { box-shadow: inset 2px 0 color-mix(in oklch, var(--primary) 45%, transparent); }
  .segment-meta { display: contents; }
  .timestamp { align-self: start; justify-self: start; grid-column: 1; grid-row: 1 / span 2; padding: 2px 3px; border: 0; border-radius: 4px; color: var(--primary); background: transparent; cursor: pointer; font: inherit; font-size: 0.76rem; font-variant-numeric: tabular-nums; }
  .timestamp:hover { background: color-mix(in oklch, var(--primary) 10%, transparent); }
  .timestamp:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
  .segment-heading { display: flex; min-width: 0; grid-column: 2; align-items: center; justify-content: space-between; gap: 10px; }
  .segment-heading strong { overflow: hidden; color: var(--foreground); font-size: 0.82rem; text-overflow: ellipsis; white-space: nowrap; }
  .segment-text { grid-column: 2; margin-top: 7px; font-size: 0.9rem; line-height: 1.75; white-space: pre-wrap; }
  .editor { width: 100%; min-height: 1.75em; resize: none; overflow: hidden; padding: 3px 5px; border: 1px solid transparent; border-radius: 6px; color: var(--foreground); background: transparent; font: inherit; font-size: 0.9rem; line-height: 1.75; }
  .editor:hover { border-color: color-mix(in oklch, var(--border) 75%, transparent); background: color-mix(in oklch, var(--background) 70%, transparent); }
  .editor:focus { border-color: color-mix(in oklch, var(--primary) 55%, var(--border)); outline: 2px solid color-mix(in oklch, var(--primary) 18%, transparent); background: var(--background); }
  .transcript-more {
    display: flex;
    justify-content: center;
    margin-top: 18px;
  }
  .empty-result { margin: 28px 0 0; color: var(--muted-foreground); font-size: 0.84rem; }

  @media (max-width: 600px) {
    .replace-panel { grid-template-columns: minmax(0, 1fr); }
    .replace-fields { grid-template-columns: minmax(0, 1fr); }
    .replace-actions { flex-wrap: wrap; }
    .segment { grid-template-columns: 64px minmax(0, 1fr); gap: 10px; }
  }
</style>
