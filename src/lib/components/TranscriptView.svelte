<script lang="ts">
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { Card } from "@mutsuna/ui/card";
  import { formatTimestamp } from "../format";
  import { transcriptionProviderLabel } from "../providers";
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

<Card class="card transcript-card" aria-label="文字起こし結果">
  <div class="section-heading transcript-heading">
    <div>
      <p class="step">Transcript</p>
      <h2>文字起こし結果</h2>
    </div>
    <div class="transcript-badges">
      <Badge>{transcriptionProviderLabel(transcript.provider)}</Badge>
      <Badge class="model" variant="secondary">{transcript.model}</Badge>
      <Badge variant="outline">{transcript.language}</Badge>
    </div>
  </div>

  {#if transcript.segments.length > 0}
    <div class="segments">
      {#each visibleSegments as segment, index (`${segment.speaker}-${segment.startMs}-${index}`)}
        <article class="segment">
          <div class="segment-meta">
            <strong>{segment.speaker}</strong>
            <time>
              {formatTimestamp(segment.startMs)} – {formatTimestamp(segment.endMs)}
            </time>
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
</Card>

<style>
  .transcript-more {
    display: flex;
    justify-content: center;
    margin-top: 18px;
  }
</style>
