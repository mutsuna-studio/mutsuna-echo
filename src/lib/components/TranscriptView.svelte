<script lang="ts">
  import { Badge } from "@mutsuna/ui/badge";
  import { Card } from "@mutsuna/ui/card";
  import { formatTimestamp } from "../format";
  import { transcriptionProviderLabel } from "../providers";
  import type { Transcript } from "../types/transcript";

  let { transcript }: { transcript: Transcript } = $props();
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
</Card>
