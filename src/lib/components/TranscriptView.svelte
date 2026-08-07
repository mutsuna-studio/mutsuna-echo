<script lang="ts">
  import { formatTimestamp } from "../format";
  import type { Transcript } from "../types/transcript";

  let { transcript }: { transcript: Transcript } = $props();
</script>

<section class="card transcript-card" aria-label="文字起こし結果">
  <div class="section-heading transcript-heading">
    <div>
      <p class="step">Transcript</p>
      <h2>文字起こし結果</h2>
    </div>
    <span class="model">{transcript.model} · {transcript.language}</span>
  </div>

  {#if transcript.segments.length > 0}
    <div class="segments">
      {#each transcript.segments as segment, index (`${segment.speaker}-${segment.startMs}-${index}`)}
        <article class="segment">
          <div class="segment-meta">
            <strong>{segment.speaker}</strong>
            <time>{formatTimestamp(segment.startMs)}</time>
          </div>
          <p>{segment.text}</p>
        </article>
      {/each}
    </div>
  {:else}
    <p class="empty-result">発話は検出されませんでした。</p>
  {/if}
</section>
