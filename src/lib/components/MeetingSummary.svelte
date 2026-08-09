<script lang="ts">
  import CheckCircle2 from "@lucide/svelte/icons/check-circle-2";
  import ListTodo from "@lucide/svelte/icons/list-todo";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import { Button } from "@mutsuna/ui/button";
  import { Select } from "@mutsuna/ui/select";
  import type { EditableTranscript } from "../types/transcript";
  import type { SummaryProviderDefinition, SummaryStatus } from "../types/summary";

  type Props = {
    transcript: EditableTranscript | null;
    status: SummaryStatus | null;
    providers: readonly SummaryProviderDefinition[];
    providerId: string;
    modelId: string;
    customModelId: string;
    generating: boolean;
    onProviderChange: (value: string) => void;
    onModelChange: (value: string) => void;
    onCustomModelChange: (value: string) => void;
    onGenerate: () => void;
    onSeekSource: (positionMs: number) => void;
  };

  let {
    transcript,
    status,
    providers,
    providerId,
    modelId,
    customModelId,
    generating,
    onProviderChange,
    onModelChange,
    onCustomModelChange,
    onGenerate,
    onSeekSource
  }: Props = $props();

  const provider = $derived(providers.find((candidate) => candidate.id === providerId) ?? providers[0]);
  const providerOptions = $derived(providers.map((candidate) => ({ value: candidate.id, label: candidate.label, disabled: !candidate.ready })));
  const modelOptions = $derived([
    ...(provider?.models.map((model) => ({ value: model.id, label: model.label })) ?? []),
    ...(provider?.allowCustomModel ? [{ value: "custom", label: "モデルIDを指定" }] : [])
  ]);
  const canGenerate = $derived(Boolean(transcript && provider?.ready && !generating && (modelId !== "custom" || customModelId.trim())));
  const summary = $derived(status?.summary ?? null);

  function seekToFirstSource(ids: readonly string[]) {
    const id = ids.find((candidate) => transcript?.segments.some((segment) => segment.segmentId === candidate));
    const segment = transcript?.segments.find((candidate) => candidate.segmentId === id);
    if (segment) onSeekSource(segment.startMs);
  }
</script>

<section class="meeting-summary" aria-label="会議ノート">
  <header class="summary-toolbar">
    <div class="summary-heading">
      <Sparkles aria-hidden="true" />
      <div><strong>Meeting Note</strong><span>修正版の文字起こしから生成</span></div>
    </div>
    <div class="summary-controls">
      <Select value={providerId} options={providerOptions} onValueChange={onProviderChange} disabled={generating || providers.length === 0} ariaLabel="要約プロバイダー" />
      <Select value={modelId} options={modelOptions} onValueChange={onModelChange} disabled={generating || !provider} ariaLabel="要約モデル" />
      {#if modelId === "custom"}
        <input value={customModelId} oninput={(event) => onCustomModelChange(event.currentTarget.value)} placeholder="モデルID" aria-label="要約モデルID" disabled={generating} />
      {/if}
      <Button size="sm" type="button" icon={summary ? RefreshCw : Sparkles} onclick={onGenerate} disabled={!canGenerate} loading={generating}>
        {summary ? "要約を更新" : "要約を作成"}
      </Button>
    </div>
  </header>

  {#if provider && !provider.ready}
    <p class="provider-warning" role="status">{provider.statusMessage}</p>
  {/if}

  {#if status?.stale}
    <div class="stale-notice" role="status">
      <span>文字起こしが変更されました。現在の要約は変更前の内容です。</span>
      <Button size="xs" type="button" variant="ghost" icon={RefreshCw} onclick={onGenerate} disabled={!canGenerate}>要約を更新</Button>
    </div>
  {/if}

  {#if summary}
    <article class="note">
      <section class="overview">
        <h2>概要</h2>
        <p>{summary.content.overview}</p>
      </section>

      <section>
        <h2><CheckCircle2 aria-hidden="true" />決定事項</h2>
        {#if summary.content.decisions.length > 0}
          <ul>
            {#each summary.content.decisions as decision, index (`${decision.text}-${index}`)}
              <li>
                <span>{decision.text}</span>
                {#if decision.sourceSegmentIds.length > 0}<button type="button" onclick={() => seekToFirstSource(decision.sourceSegmentIds)}>原文を確認</button>{/if}
              </li>
            {/each}
          </ul>
        {:else}<p class="empty-section">明確な決定事項はありません。</p>{/if}
      </section>

      <section>
        <h2><ListTodo aria-hidden="true" />アクション項目</h2>
        {#if summary.content.actionItems.length > 0}
          <ul>
            {#each summary.content.actionItems as item, index (`${item.text}-${index}`)}
              <li>
                <span>{item.assignee ? `${item.assignee}：` : ""}{item.text}{item.due ? `（${item.due}）` : ""}</span>
                {#if item.sourceSegmentIds.length > 0}<button type="button" onclick={() => seekToFirstSource(item.sourceSegmentIds)}>原文を確認</button>{/if}
              </li>
            {/each}
          </ul>
        {:else}<p class="empty-section">アクション項目はありません。</p>{/if}
      </section>
      <footer>{summary.provider} · {summary.model === "default" ? "既定モデル" : summary.model} · {new Date(summary.generatedAt).toLocaleString("ja-JP")}</footer>
    </article>
  {:else if transcript}
    <div class="empty-note"><Sparkles aria-hidden="true" /><h2>会議ノートを作成</h2><p>編集済みの話者名と文字起こしを使って、概要・決定事項・アクション項目を生成します。</p></div>
  {:else}
    <div class="empty-note"><Sparkles aria-hidden="true" /><h2>先に文字起こしを作成してください</h2><p>会議ノートは文字起こしを根拠として生成されます。</p></div>
  {/if}
</section>

<style>
  .meeting-summary { max-width: 880px; margin: 22px auto 0; }
  .summary-toolbar { display: flex; align-items: flex-end; justify-content: space-between; gap: 18px; padding-bottom: 16px; border-bottom: 1px solid var(--border); }
  .summary-heading { display: flex; align-items: center; gap: 10px; }
  .summary-heading > :global(svg) { width: 21px; color: var(--primary); }
  .summary-heading div { display: grid; gap: 2px; }
  .summary-heading strong { font-size: 0.92rem; }
  .summary-heading span { color: var(--muted-foreground); font-size: 0.69rem; }
  .summary-controls { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 7px; }
  .summary-controls :global([data-slot="select-trigger"]) { min-width: 150px; }
  .summary-controls input { width: 160px; height: 32px; padding: 0 9px; border: 1px solid var(--border); border-radius: 7px; color: var(--foreground); background: var(--background); font: inherit; font-size: 0.75rem; }
  .summary-controls input:focus { outline: 2px solid var(--ring); outline-offset: 1px; }
  .provider-warning, .stale-notice { margin: 14px 0 0; padding: 10px 12px; border-radius: 8px; font-size: 0.75rem; }
  .provider-warning { color: var(--destructive); background: color-mix(in oklch, var(--destructive) 8%, var(--background)); }
  .stale-notice { display: flex; align-items: center; justify-content: space-between; gap: 12px; color: color-mix(in oklch, var(--foreground) 80%, #9a6700); background: color-mix(in oklch, #d99b19 13%, var(--background)); }
  .note { display: grid; gap: 26px; padding: 26px 2px 40px; }
  .note section { display: grid; gap: 10px; }
  .note h2 { display: flex; align-items: center; gap: 7px; margin: 0; font-size: 0.82rem; letter-spacing: 0.015em; }
  .note h2 :global(svg) { width: 16px; color: var(--primary); }
  .overview p { margin: 0; color: var(--foreground); font-size: 0.9rem; line-height: 1.8; white-space: pre-wrap; }
  ul { display: grid; gap: 0; margin: 0; padding: 0; list-style: none; }
  li { display: flex; align-items: flex-start; justify-content: space-between; gap: 14px; padding: 10px 0; border-bottom: 1px solid color-mix(in oklch, var(--border) 70%, transparent); font-size: 0.84rem; line-height: 1.6; }
  li button { flex: none; padding: 2px 0; border: 0; color: var(--primary); background: transparent; cursor: pointer; font: inherit; font-size: 0.69rem; font-weight: 650; }
  .empty-section { margin: 0; color: var(--muted-foreground); font-size: 0.8rem; }
  footer { color: var(--muted-foreground); font-size: 0.65rem; }
  .empty-note { display: grid; min-height: 330px; place-items: center; align-content: center; text-align: center; }
  .empty-note > :global(svg) { width: 34px; height: 34px; color: var(--primary); stroke-width: 1.5; }
  .empty-note h2 { margin: 14px 0 6px; font-size: 1rem; }
  .empty-note p { max-width: 440px; margin: 0; color: var(--muted-foreground); font-size: 0.8rem; line-height: 1.6; }
  @media (max-width: 760px) { .summary-toolbar { align-items: stretch; flex-direction: column; } .summary-controls { justify-content: stretch; } .summary-controls > * { flex: 1; } }
</style>
