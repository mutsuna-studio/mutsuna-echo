<script lang="ts">
  import { Button } from "@mutsuna/ui/button";
  import { formatTimestamp } from "../format";
  import type { Transcript } from "../types/transcript";

  let { transcript }: { transcript: Transcript } = $props();

  const SEGMENT_PAGE_SIZE = 300;
  let visibleCount = $state(SEGMENT_PAGE_SIZE);
  const visibleSegments = $derived(transcript.segments.slice(0, visibleCount));
  const remainingSegments = $derived(Math.max(0, transcript.segments.length - visibleSegments.length));

  $effect(() => {
    transcript;
    visibleCount = SEGMENT_PAGE_SIZE;
  });
</script>

<section class="transcript-view" aria-label="文字起こし結果">
  {#if transcript.segments.length > 0}
    <div class="segments">
      {#each visibleSegments as segment, index (`${segment.speaker}-${segment.startMs}-${index}`)}
        <article class="segment">
          <div class="segment-meta">
            <time title={`${formatTimestamp(segment.startMs)} – ${formatTimestamp(segment.endMs)}`}>
              {formatTimestamp(segment.startMs)}
            </time>
            <strong>{segment.speaker}</strong>
          </div>
          <p>{segment.text}</p>
        </article>
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
  .segment { display: grid; grid-template-columns: 84px minmax(0, 1fr); gap: 18px; padding: 17px 0; border-bottom: 1px solid var(--border); }
  .segment-meta { display: contents; }
  .segment-meta time { grid-column: 1; grid-row: 1 / span 2; padding-top: 2px; color: var(--primary); font-size: 0.76rem; font-variant-numeric: tabular-nums; }
  .segment-meta strong { grid-column: 2; color: var(--foreground); font-size: 0.82rem; }
  .segment p { grid-column: 2; margin: 7px 0 0; font-size: 0.9rem; line-height: 1.75; white-space: pre-wrap; }
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
