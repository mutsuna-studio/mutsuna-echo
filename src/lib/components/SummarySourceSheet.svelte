<script lang="ts">
  import FileText from "@lucide/svelte/icons/file-text";
  import Play from "@lucide/svelte/icons/play";
  import X from "@lucide/svelte/icons/x";
  import { Button } from "@mutsuna/ui/button";
  import { ScrollbarArea } from "@mutsuna/ui/scrollbar";
  import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@mutsuna/ui/sheet";
  import { innerWidth } from "svelte/reactivity/window";
  import { formatTimestamp } from "../format";
  import type { SummarySourceSelection } from "../types/summary";
  import type { EditableTranscript } from "../types/transcript";

  type Props = {
    selection: SummarySourceSelection | null;
    transcript: EditableTranscript | null;
    playbackAvailable: boolean;
    onClose: () => void;
    onPlay: (positionMs: number) => void;
    onOpenTranscript: (positionMs: number) => void;
  };

  let {
    selection,
    transcript,
    playbackAvailable,
    onClose,
    onPlay,
    onOpenTranscript
  }: Props = $props();

  const mobile = $derived((innerWidth.current ?? 781) <= 780);
  const sourceSegments = $derived.by(() => {
    if (!selection || !transcript) return [];
    const sourceIds = new Set(selection.sourceSegmentIds);
    return transcript.segments.filter((segment) => sourceIds.has(segment.segmentId));
  });
  const kindLabel = $derived(({ keyPoint: "要点", topic: "議題", decision: "決定事項", actionItem: "アクション項目", openIssue: "未解決事項", question: "質問", note: "ノート" } as const)[selection?.kind ?? "note"]);
  const firstPositionMs = $derived(sourceSegments[0]?.startMs ?? 0);

  function speakerLabel(speaker: string): string {
    return transcript?.speakerLabels.find((label) => label.speaker === speaker)?.label || speaker;
  }
</script>

{#if selection}
  <Sheet open={true} onOpenChange={(open) => { if (!open) onClose(); }}>
    <SheetContent
      id="summary-source-sheet"
      side={mobile ? "bottom" : "right"}
      class="summary-source-sheet"
      showCloseButton={false}
    >
    <div class="sheet-handle" aria-hidden="true"></div>
    <SheetHeader class="source-header">
      <div class="source-heading-row">
        <span class="source-kind">{kindLabel}の根拠</span>
        <Button size="icon-sm" variant="ghost" type="button" icon={X} aria-label="根拠を閉じる" onclick={onClose} />
      </div>
      <SheetTitle>{selection?.text ?? "根拠"}</SheetTitle>
      <SheetDescription>
        会議ノートの生成時に参照した文字起こしです。
      </SheetDescription>
    </SheetHeader>

    <ScrollbarArea class="summary-source-scroll" gutter="both-edges">
      <div class="source-body-inner">
      {#if sourceSegments.length > 0}
        <div class="source-count">関連する発言 {sourceSegments.length}件</div>
        <ol>
          {#each sourceSegments as segment (segment.segmentId)}
            <li>
              <div class="segment-heading">
                {#if playbackAvailable}
                  <button
                    class="play-source"
                    type="button"
                    aria-label={`${formatTimestamp(segment.startMs)}から再生`}
                    onclick={() => onPlay(segment.startMs)}
                  >
                    <Play aria-hidden="true" />
                    {formatTimestamp(segment.startMs)}
                  </button>
                {:else}
                  <span class="source-time">{formatTimestamp(segment.startMs)}</span>
                {/if}
                <strong>{speakerLabel(segment.speaker)}</strong>
              </div>
              <p>{segment.text}</p>
            </li>
          {/each}
        </ol>
      {:else}
        <div class="source-empty" role="status">
          <FileText aria-hidden="true" />
          <strong>根拠の発言を見つけられませんでした</strong>
          <span>文字起こしが更新された可能性があります。会議ノートを再生成してください。</span>
        </div>
      {/if}
      </div>
    </ScrollbarArea>

    {#if sourceSegments.length > 0}
      <div class="source-footer">
        <Button variant="outline" type="button" onclick={() => onOpenTranscript(firstPositionMs)}>
          文字起こし全体で開く
        </Button>
      </div>
    {/if}
    </SheetContent>
  </Sheet>
{/if}

<style>
  :global(.summary-source-sheet) { width: min(520px, calc(100vw - 24px)); max-width: none !important; gap: 0; padding: 0; overflow: hidden; }
  .sheet-handle { display: none; }
  :global(.source-header) { gap: 8px; padding: 22px 24px 18px; border-bottom: 1px solid var(--border); }
  .source-heading-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .source-kind { color: var(--primary); font-size: 0.72rem; font-weight: 750; letter-spacing: 0.02em; }
  :global(.source-header [data-slot="sheet-title"]) { padding-right: 0; font-size: 1rem; line-height: 1.55; }
  :global(.source-header [data-slot="sheet-description"]) { color: var(--muted-foreground); font-size: 0.73rem; line-height: 1.5; }
  :global(.summary-source-scroll) { min-height: 0; flex: 1; overflow-y: auto; overscroll-behavior: contain; }
  .source-body-inner { padding: 18px 24px 24px; }
  .source-count { margin-bottom: 10px; color: var(--muted-foreground); font-size: 0.7rem; font-weight: 650; }
  ol { display: grid; gap: 10px; margin: 0; padding: 0; list-style: none; }
  li { padding: 14px; border: 1px solid color-mix(in oklch, var(--border) 85%, transparent); border-radius: 11px; background: color-mix(in oklch, var(--muted) 24%, var(--background)); }
  .segment-heading { display: flex; align-items: center; gap: 9px; }
  .segment-heading strong { min-width: 0; overflow: hidden; font-size: 0.75rem; text-overflow: ellipsis; white-space: nowrap; }
  .play-source { display: inline-flex; flex: none; align-items: center; gap: 4px; padding: 3px 7px; border: 0; border-radius: 6px; color: var(--primary); background: color-mix(in oklch, var(--primary) 10%, transparent); cursor: pointer; font: inherit; font-size: 0.68rem; font-weight: 700; font-variant-numeric: tabular-nums; }
  .play-source:hover { background: color-mix(in oklch, var(--primary) 16%, transparent); }
  .play-source:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
  .play-source :global(svg) { width: 12px; height: 12px; fill: currentColor; }
  .source-time { flex: none; color: var(--primary); font-size: 0.68rem; font-weight: 700; font-variant-numeric: tabular-nums; }
  li p { margin: 10px 0 0; font-size: 0.82rem; line-height: 1.75; white-space: pre-wrap; }
  .source-empty { display: grid; min-height: 240px; place-items: center; align-content: center; gap: 8px; padding: 30px; text-align: center; }
  .source-empty :global(svg) { width: 30px; height: 30px; color: var(--muted-foreground); stroke-width: 1.5; }
  .source-empty strong { font-size: 0.84rem; }
  .source-empty span { max-width: 330px; color: var(--muted-foreground); font-size: 0.74rem; line-height: 1.6; }
  .source-footer { padding: 14px 24px 20px; border-top: 1px solid var(--border); background: var(--background); }
  .source-footer :global(button) { width: 100%; }

  @media (max-width: 780px) {
    :global(.summary-source-sheet[data-side="bottom"]) { width: 100%; max-width: none !important; max-height: min(82dvh, 720px); border-radius: 16px 16px 0 0; }
    .sheet-handle { display: block; width: 42px; height: 4px; flex: none; margin: 8px auto 0; border-radius: 999px; background: color-mix(in oklch, var(--muted-foreground) 32%, transparent); }
    :global(.source-header) { padding: 12px 18px 15px; }
    .source-body-inner { padding: 14px 18px 20px; }
    .source-footer { padding: 12px 18px calc(14px + env(safe-area-inset-bottom)); }
    .source-footer :global(button) { min-height: 46px; }
  }
</style>
