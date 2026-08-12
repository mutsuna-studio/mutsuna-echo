<script lang="ts">
  import CheckCircle2 from "@lucide/svelte/icons/check-circle-2";
  import ListTodo from "@lucide/svelte/icons/list-todo";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import { Button } from "@mutsuna/ui/button";
  import { Select, SelectContent, SelectItem, SelectTrigger } from "@mutsuna/ui/select";
  import type { EditableTranscript } from "../types/transcript";
  import type {
    SummaryProgress,
    SummaryProviderDefinition,
    SummarySourceSelection,
    SummaryStatus
  } from "../types/summary";

  type Props = {
    transcript: EditableTranscript | null;
    status: SummaryStatus | null;
    providers: readonly SummaryProviderDefinition[];
    providerId: string;
    modelId: string;
    modelsLoading: boolean;
    generating: boolean;
    progress: SummaryProgress | null;
    blocked: boolean;
    selectedSourceKey: string | null;
    onProviderChange: (value: string) => void;
    onModelChange: (value: string) => void;
    onGenerate: () => void;
    onShowSource: (selection: SummarySourceSelection, trigger: HTMLButtonElement) => void;
  };

  let {
    transcript,
    status,
    providers,
    providerId,
    modelId,
    modelsLoading,
    generating,
    progress,
    blocked,
    selectedSourceKey,
    onProviderChange,
    onModelChange,
    onGenerate,
    onShowSource
  }: Props = $props();

  const provider = $derived(providers.find((candidate) => candidate.id === providerId) ?? providers[0]);
  const providerOptions = $derived(providers.map((candidate) => ({ value: candidate.id, label: candidate.label, disabled: !candidate.ready })));
  const modelOptions = $derived(provider?.models.map((model) => ({ value: model.id, label: model.label })) ?? []);
  const canGenerate = $derived(Boolean(transcript && provider?.ready && !modelsLoading && !generating && !blocked && modelOptions.some((model) => model.value === modelId)));
  const summary = $derived(status?.summary ?? null);
  const selectedModelLabel = $derived(modelsLoading ? "モデルを取得中…" : (modelOptions.find((option) => option.value === modelId)?.label ?? "モデルを選択"));
  const progressLabel = $derived(progress?.stage === "merging" ? "会議ノートを統合中" : "会議ノートを作成中");

  function availableSourceIds(ids: readonly string[]): string[] {
    const availableIds = new Set(transcript?.segments.map((segment) => segment.segmentId) ?? []);
    return ids.filter((id) => availableIds.has(id));
  }

  function showSource(event: MouseEvent, selection: SummarySourceSelection) {
    if (!(event.currentTarget instanceof HTMLButtonElement)) return;
    onShowSource(selection, event.currentTarget);
  }
</script>

{#snippet providerSelect(id: string)}
  <Select type="single" value={providerId} onValueChange={onProviderChange} disabled={generating || blocked || providers.length === 0}>
    <SelectTrigger {id} aria-label="要約プロバイダー" class="summary-select">
      <span class="select-value" title={provider?.label}>{provider?.label ?? "サービスを選択"}</span>
    </SelectTrigger>
    <SelectContent>
      {#each providerOptions as option (option.value)}
        <SelectItem value={option.value} disabled={option.disabled}>{option.label}</SelectItem>
      {/each}
    </SelectContent>
  </Select>
{/snippet}

{#snippet modelSelect(id: string)}
  <Select type="single" value={modelId} onValueChange={onModelChange} disabled={generating || blocked || modelsLoading || !provider || modelOptions.length === 0}>
    <SelectTrigger {id} aria-label="要約モデル" class="summary-select">
      <span class="select-value" title={selectedModelLabel}>{selectedModelLabel}</span>
    </SelectTrigger>
    <SelectContent>
      {#each modelOptions as option (option.value)}
        <SelectItem value={option.value}>{option.label}</SelectItem>
      {/each}
    </SelectContent>
  </Select>
{/snippet}

<section class="meeting-summary" aria-label="会議ノート">
  {#if generating}
    <div class="summary-progress" role="status" aria-live="polite">
      <span>{progressLabel}</span>
      {#if progress}<strong>{progress.completedSteps} / {progress.totalSteps}</strong>{/if}
      <progress max={progress?.totalSteps ?? 1} value={progress?.completedSteps ?? 0} aria-label={progressLabel}></progress>
    </div>
  {/if}
  {#if summary}
    <header class="summary-toolbar">
      <div class="summary-heading">
        <strong>会議ノート</strong>
        <span>モデルを変更して再生成できます</span>
      </div>
      <div class="summary-controls">
        {@render providerSelect("summary-provider-toolbar")}
        {@render modelSelect("summary-model-toolbar")}
        <Button size="sm" variant="outline" type="button" icon={RefreshCw} onclick={onGenerate} disabled={!canGenerate} loading={generating}>要約を更新</Button>
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
              {@const sourceIds = availableSourceIds(decision.sourceSegmentIds)}
              {@const sourceKey = `decision-${index}`}
              <li class:source-selected={selectedSourceKey === sourceKey}>
                <span>{decision.text}</span>
                {#if sourceIds.length > 0}
                  <button
                    type="button"
                    aria-expanded={selectedSourceKey === sourceKey}
                    aria-controls="summary-source-sheet"
                    aria-label={`根拠を見る: ${decision.text}`}
                    onclick={(event) => showSource(event, { key: sourceKey, kind: "decision", text: decision.text, sourceSegmentIds: sourceIds })}
                  >
                    {selectedSourceKey === sourceKey ? "根拠を表示中" : "根拠を見る"}
                  </button>
                {/if}
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
              {@const sourceIds = availableSourceIds(item.sourceSegmentIds)}
              {@const sourceKey = `action-item-${index}`}
              {@const itemText = `${item.assignee ? `${item.assignee}：` : ""}${item.text}${item.due ? `（${item.due}）` : ""}`}
              <li class:source-selected={selectedSourceKey === sourceKey}>
                <span>{itemText}</span>
                {#if sourceIds.length > 0}
                  <button
                    type="button"
                    aria-expanded={selectedSourceKey === sourceKey}
                    aria-controls="summary-source-sheet"
                    aria-label={`根拠を見る: ${itemText}`}
                    onclick={(event) => showSource(event, { key: sourceKey, kind: "actionItem", text: itemText, sourceSegmentIds: sourceIds })}
                  >
                    {selectedSourceKey === sourceKey ? "根拠を表示中" : "根拠を見る"}
                  </button>
                {/if}
              </li>
            {/each}
          </ul>
        {:else}<p class="empty-section">アクション項目はありません。</p>{/if}
      </section>
      <footer>{summary.provider} · {summary.model === "default" ? "既定モデル" : summary.model} · {new Date(summary.generatedAt).toLocaleString("ja-JP")}</footer>
    </article>
  {:else if transcript}
    <div class="empty-note actionable">
      <Sparkles aria-hidden="true" />
      <h2>会議ノートはまだありません</h2>
      <p>文字起こしをもとに、概要・決定事項・アクション項目を生成します。</p>
      <div class="summary-start-controls">
        <div class="summary-field">
          <label for="summary-provider-start">生成サービス</label>
          {@render providerSelect("summary-provider-start")}
        </div>
        <div class="summary-field">
          <label for="summary-model-start">生成モデル</label>
          {@render modelSelect("summary-model-start")}
        </div>
        <span class="summary-action">
          <Button size="lg" type="button" icon={Sparkles} onclick={onGenerate} disabled={!canGenerate} loading={generating}>会議ノートを作成</Button>
        </span>
      </div>
      {#if provider && !provider.ready}
        <p class="provider-warning" role="status">{provider.statusMessage}</p>
      {/if}
    </div>
  {:else}
    <div class="empty-note blocked"><Sparkles aria-hidden="true" /><h2>先に文字起こしを作成してください</h2><p>会議ノートは文字起こしをもとに生成されます。「文字起こし」タブから作成してください。</p></div>
  {/if}
</section>

<style>
  .meeting-summary { max-width: 880px; margin: 22px auto 0; }
  .summary-progress { display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 7px 12px; padding: 2px 2px 18px; color: var(--muted-foreground); font-size: 0.76rem; }
  .summary-progress strong { color: var(--foreground); font-variant-numeric: tabular-nums; }
  .summary-progress progress { width: 100%; height: 7px; grid-column: 1 / -1; accent-color: var(--primary); }
  .summary-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 20px; padding-bottom: 18px; border-bottom: 1px solid var(--border); }
  .summary-heading { display: grid; min-width: 0; gap: 3px; }
  .summary-heading strong { font-size: 0.92rem; }
  .summary-heading span { color: var(--muted-foreground); font-size: 0.72rem; }
  .summary-controls { display: flex; min-width: 0; flex-wrap: wrap; justify-content: flex-end; gap: 8px; }
  .summary-controls :global([data-slot="select-trigger"]) { width: 170px; min-width: 0; max-width: 100%; }
  :global(.summary-select) { min-width: 0; max-width: 100%; }
  .select-value { display: block; min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
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
  li.source-selected { margin: 0 -10px; padding-right: 10px; padding-left: 10px; border-radius: 8px; background: color-mix(in oklch, var(--primary) 8%, transparent); }
  li button { flex: none; padding: 2px 0; border: 0; color: var(--primary); background: transparent; cursor: pointer; font: inherit; font-size: 0.69rem; font-weight: 650; }
  li button:focus-visible { border-radius: 3px; outline: 2px solid var(--ring); outline-offset: 3px; }
  .empty-section { margin: 0; color: var(--muted-foreground); font-size: 0.8rem; }
  footer { color: var(--muted-foreground); font-size: 0.65rem; }
  .empty-note { display: grid; min-height: 420px; place-items: center; align-content: center; padding: 32px; border: 1px solid var(--border); border-radius: 14px; background: color-mix(in oklch, var(--muted) 28%, var(--background)); text-align: center; }
  .empty-note.blocked { background: color-mix(in oklch, var(--muted) 18%, var(--background)); }
  .empty-note > :global(svg) { width: 34px; height: 34px; color: var(--primary); stroke-width: 1.5; }
  .empty-note h2 { margin: 14px 0 7px; font-size: 1.05rem; }
  .empty-note > p { max-width: 440px; margin: 0 0 18px; color: var(--muted-foreground); font-size: 0.82rem; line-height: 1.6; }
  .empty-note.blocked > p { margin-bottom: 0; }
  .summary-start-controls { display: grid; width: min(100%, 520px); grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; text-align: left; }
  .summary-field { display: grid; min-width: 0; gap: 6px; }
  .summary-field label { color: var(--muted-foreground); font-size: 0.72rem; font-weight: 650; }
  .summary-field :global([data-slot="select-trigger"]) { width: 100%; }
  .summary-action { grid-column: 1 / -1; }
  .summary-action, .summary-action :global(button) { width: 100%; }
  .empty-note .provider-warning { width: min(100%, 520px); margin-bottom: 0; text-align: left; }

  @media (max-width: 760px) {
    .summary-toolbar { align-items: stretch; flex-direction: column; gap: 14px; }
    .summary-controls { flex-direction: column; align-items: stretch; justify-content: stretch; gap: 10px; }
    .summary-controls :global([data-slot="select-trigger"]) { width: 100%; min-height: 44px; }
    .summary-controls :global(button[data-slot="button"]) { width: 100%; min-height: 48px; font-size: 0.95rem; }
    .empty-note { min-height: 380px; padding: 28px 18px; }
    .summary-start-controls { grid-template-columns: minmax(0, 1fr); }
    .summary-action { grid-column: auto; }
    .summary-field :global([data-slot="select-trigger"]) { min-height: 48px; }
    .summary-action :global(button) { min-height: 48px; padding-right: 18px; padding-left: 18px; font-size: 0.95rem; }
  }
</style>
