<script lang="ts">
  import { tick } from "svelte";
  import { Button } from "@mutsuna/ui/button";
  import { formatTimestamp } from "../format";
  import type { Transcript } from "../types/transcript";

  type Props = {
    transcript: Transcript;
    currentPositionMs: number;
    followRequestId: number;
    onSeek: (positionMs: number) => void;
  };

  let { transcript, currentPositionMs, followRequestId, onSeek }: Props = $props();

  const SEGMENT_PAGE_SIZE = 300;
  let visibleCount = $state(SEGMENT_PAGE_SIZE);
  let segmentElements = $state.raw<Array<HTMLButtonElement | undefined>>([]);
  const visibleSegments = $derived(transcript.segments.slice(0, visibleCount));
  const remainingSegments = $derived(Math.max(0, transcript.segments.length - visibleSegments.length));
  const activeIndex = $derived(segmentIndexAt(transcript, currentPositionMs));

  $effect(() => {
    transcript;
    visibleCount = SEGMENT_PAGE_SIZE;
    segmentElements = [];
  });

  $effect(() => {
    const index = activeIndex;
    followRequestId;
    if (index < 0) return;
    if (index >= visibleCount) {
      visibleCount = Math.ceil((index + 1) / SEGMENT_PAGE_SIZE) * SEGMENT_PAGE_SIZE;
    }
    void tick().then(() => {
      const element = segmentElements[index];
      if (!element) return;
      scrollToSegment(element);
    });
  });

  function scrollToSegment(element: HTMLElement) {
    const candidate = element.closest(".detail-content");
    if (!(candidate instanceof HTMLElement)) {
      element.scrollIntoView({ behavior: "auto", block: "center" });
      return;
    }
    const viewport = candidate.getBoundingClientRect();
    const segment = element.getBoundingClientRect();
    const centeredTop = candidate.scrollTop
      + segment.top
      - viewport.top
      - (candidate.clientHeight - segment.height) / 2;
    const maximum = Math.max(0, candidate.scrollHeight - candidate.clientHeight);
    candidate.scrollTo({ top: Math.min(Math.max(0, centeredTop), maximum), behavior: "auto" });
  }

  function segmentIndexAt(value: Transcript, positionMs: number): number {
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

<section class="transcript-view" aria-label="文字起こし結果">
  {#if transcript.segments.length > 0}
    <div class="segments">
      {#each visibleSegments as segment, index (`${segment.speaker}-${segment.startMs}-${index}`)}
        <button
          class:active={index === activeIndex}
          class="segment"
          type="button"
          bind:this={segmentElements[index]}
          aria-current={index === activeIndex ? "true" : undefined}
          aria-label={`${formatTimestamp(segment.startMs)}、${segment.speaker}へ移動`}
          onclick={() => onSeek(segment.startMs)}
        >
          <span class="segment-meta">
            <time title={`${formatTimestamp(segment.startMs)} – ${formatTimestamp(segment.endMs)}`}>
              {formatTimestamp(segment.startMs)}
            </time>
            <strong>{segment.speaker}</strong>
          </span>
          <span class="segment-text">{segment.text}</span>
        </button>
      {/each}
    </div>
    {#if remainingSegments > 0}
      <div class="transcript-more">
        <Button variant="outline" type="button" onclick={() => visibleCount += SEGMENT_PAGE_SIZE}>
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
  .segments { display: grid; }
  .segment { display: grid; width: 100%; grid-template-columns: 84px minmax(0, 1fr); gap: 18px; padding: 17px 10px; border: 0; border-bottom: 1px solid var(--border); color: inherit; background: transparent; cursor: pointer; font: inherit; text-align: left; transition: background-color 120ms ease, box-shadow 120ms ease; }
  .segment:hover { background: color-mix(in oklch, var(--primary) 4%, transparent); }
  .segment.active { background: color-mix(in oklch, var(--primary) 8%, transparent); box-shadow: inset 3px 0 var(--primary); }
  .segment:focus-visible { position: relative; z-index: 1; outline: 2px solid var(--ring); outline-offset: -2px; }
  .segment-meta { display: contents; }
  .segment-meta time { grid-column: 1; grid-row: 1 / span 2; padding-top: 2px; color: var(--primary); font-size: 0.76rem; font-variant-numeric: tabular-nums; }
  .segment-meta strong { grid-column: 2; color: var(--foreground); font-size: 0.82rem; }
  .segment-text { grid-column: 2; margin-top: 7px; font-size: 0.9rem; line-height: 1.75; white-space: pre-wrap; }
  .transcript-more {
    display: flex;
    justify-content: center;
    margin-top: 18px;
  }
  .empty-result { margin: 28px 0 0; color: var(--muted-foreground); font-size: 0.84rem; }

  @media (max-width: 600px) {
    .segment { grid-template-columns: 64px minmax(0, 1fr); gap: 10px; }
  }
</style>
