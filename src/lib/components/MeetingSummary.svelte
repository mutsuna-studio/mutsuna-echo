<script lang="ts">
  import { untrack } from "svelte";
  import CheckCircle2 from "@lucide/svelte/icons/check-circle-2";
  import Check from "@lucide/svelte/icons/check";
  import Clipboard from "@lucide/svelte/icons/clipboard";
  import AlertTriangle from "@lucide/svelte/icons/triangle-alert";
  import ListTodo from "@lucide/svelte/icons/list-todo";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import { Button } from "@mutsuna/ui/button";
  import { Checkbox } from "@mutsuna/ui/checkbox";
  import { Popover, PopoverContent, PopoverTrigger } from "@mutsuna/ui/popover";
  import { Select, SelectContent, SelectItem, SelectTrigger } from "@mutsuna/ui/select";
  import type { EditableTranscript } from "../types/transcript";
  import type { MeetingDocument } from "../types/meeting-document";
  import type {
    GenerationAttemptSummary,
    SummaryProgress,
    SummaryProviderDefinition,
    SummarySourceSelection
  } from "../types/summary";
  import ProcessingStage from "./ProcessingStage.svelte";

  type Props = {
    transcript: EditableTranscript | null;
    status: MeetingDocument | null;
    stale: boolean;
    attempt: GenerationAttemptSummary | null;
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
    onRevalidate: () => void;
    onSave: (document: MeetingDocument) => Promise<MeetingDocument | null>;
    onShowSource: (selection: SummarySourceSelection, trigger: HTMLButtonElement) => void;
  };

  let {
    transcript,
    status,
    stale,
    attempt,
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
    onRevalidate,
    onSave,
    onShowSource
  }: Props = $props();

  const provider = $derived(providers.find((candidate) => candidate.id === providerId) ?? providers[0]);
  const providerOptions = $derived(providers.map((candidate) => ({ value: candidate.id, label: candidate.label, disabled: !candidate.ready })));
  const modelOptions = $derived(provider?.models.map((model) => ({ value: model.id, label: model.label })) ?? []);
  const canGenerate = $derived(Boolean(transcript && provider?.ready && !modelsLoading && !generating && !blocked && modelOptions.some((model) => model.value === modelId)));
  const summary = $derived(status);
  const selectedModelLabel = $derived(modelsLoading ? "モデルを取得中…" : (modelOptions.find((option) => option.value === modelId)?.label ?? "モデルを選択"));
  const progressDetail = $derived.by(() => {
    const details: string[] = [];
    details.push(`経過 ${formatElapsed(elapsedSeconds)}`);
    if (!progress) return details[0];
    if (progress.totalSteps > 1 && progress.activeStep) details.push(`${progress.activeStep}/${progress.totalSteps}`);
    if (progress.attempt && progress.maxAttempts) details.push(`試行 ${progress.attempt}/${progress.maxAttempts}`);
    if (progress.stage === "retrying" && progress.retryDelaySeconds) details.push(`${progress.retryDelaySeconds}秒後`);
    return details.length > 0 ? details.join(" · ") : null;
  });
  const compactProgressLabel = $derived.by(() => {
    if (progress?.stage === "retrying") return "再試行中";
    if (progress?.stage === "merging") return "統合中";
    if (progress?.stage === "mechanically-repairing") return "生成結果を機械補正中";
    if (progress?.stage === "repairing") return "生成結果を補正中";
    if (progress?.stage === "checking") return "内容とタイトルを確認中";
    if (progress?.stage === "streaming") return "生成結果を受信中";
    if (progress?.stage === "waiting") return "AIの応答を待っています";
    return "会議ノートを準備中";
  });
  const visibleProgressStep = $derived(progress ? granularSummaryProgress(progress) : null);
  let saving = $state(false);
  let editableDocument = $state<MeetingDocument | null>(null);
  let dirty = $state(false);
  let editVersion = $state(0);
  let copied = $state(false);
  let generationStartedAt = $state<number | null>(null);
  let elapsedSeconds = $state(0);

  function formatElapsed(seconds: number): string {
    const minutes = Math.floor(seconds / 60);
    const remainder = seconds % 60;
    return minutes > 0 ? `${minutes}分${remainder.toString().padStart(2, "0")}秒` : `${remainder}秒`;
  }

  function granularSummaryProgress(value: SummaryProgress): number {
    const total = Math.max(1, value.totalSteps);
    const completed = Math.min(total, Math.max(0, value.completedSteps));
    if (value.stage === "complete" || completed >= total) return total;

    let fraction = 0.08;
    if (value.stage === "waiting") fraction = 0.18;
    else if (value.stage === "retrying") fraction = 0.22;
    else if (value.stage === "streaming") {
      const receivedBytes = Math.max(0, value.receivedBytes ?? 0);
      fraction = receivedBytes > 0
        ? 0.38 + 0.54 * (1 - Math.exp(-receivedBytes / 8_000))
        : 0.38;
    } else if (value.stage === "merging") fraction = 0.72;
    else if (value.stage === "checking") fraction = 0.86;

    return Math.min(total - 0.02, completed + fraction);
  }

  $effect(() => {
    if (!generating) {
      generationStartedAt = null;
      elapsedSeconds = 0;
      return;
    }
    const startedAt = untrack(() => generationStartedAt) ?? Date.now();
    generationStartedAt = startedAt;
    elapsedSeconds = Math.floor((Date.now() - startedAt) / 1_000);
    const timer = window.setInterval(() => {
      elapsedSeconds = Math.floor((Date.now() - startedAt) / 1_000);
    }, 1_000);
    return () => window.clearInterval(timer);
  });

  $effect(() => {
    const document = status;
    if (!document || dirty || saving) return;
    if (editableDocument?.revision === document.revision) return;
    editableDocument = structuredClone(document);
  });

  const generation = $derived(summary?.generationRuns.find((run) => run.runId === summary.latestGenerationRunId));

  function availableSourceIds(ids: readonly string[]): string[] {
    const availableIds = new Set(transcript?.segments.map((segment) => segment.segmentId) ?? []);
    return ids.filter((id) => availableIds.has(id));
  }

  function sourceIds(evidenceIds: readonly string[]): string[] {
    return availableSourceIds(summary?.evidence
      .filter((item) => evidenceIds.includes(item.evidenceId))
      .flatMap((item) => item.spans.map((span) => span.segmentId.replace(/^seg_/, ""))) ?? []);
  }

  function participantNames(ids: readonly string[]): string {
    return ids.map((id) => summary?.participants.find((item) => item.participantId === id)?.displayName).filter(Boolean).join("、");
  }

  function showSource(event: MouseEvent, selection: SummarySourceSelection) {
    if (!(event.currentTarget instanceof HTMLButtonElement)) return;
    onShowSource(selection, event.currentTarget);
  }

  function markDirty() {
    dirty = true;
    editVersion += 1;
  }

  function autoResizeTextArea(element: HTMLTextAreaElement, _text: string | null | undefined) {
    const resize = () => {
      element.style.height = "auto";
      element.style.height = `${element.scrollHeight}px`;
    };
    resize();
    return {
      update() {
        requestAnimationFrame(resize);
      }
    };
  }

  async function flushEdits() {
    if (!editableDocument || !dirty || saving) return;
    saving = true;
    const savingVersion = editVersion;
    const saved = await onSave(structuredClone($state.snapshot(editableDocument)));
    if (saved) {
      if (editVersion === savingVersion) {
        editableDocument = structuredClone(saved);
        dirty = false;
      } else if (editableDocument) {
        editableDocument.revision = saved.revision;
        editableDocument.updatedAt = saved.updatedAt;
      }
    } else {
      if (status) editableDocument = structuredClone(status);
      dirty = false;
    }
    saving = false;
    if (dirty) void flushEdits();
  }

  async function toggleAction(actionItemId: string, checked: boolean) {
    if (!editableDocument || saving) return;
    const item = editableDocument.actionItems.find((candidate) => candidate.actionItemId === actionItemId);
    if (!item) return;
    item.status = checked ? "done" : "open";
    markDirty();
    await flushEdits();
  }

  function markdown(document: MeetingDocument): string {
    const lines = [`# ${document.meeting.title}`, "", "## 概要", "", document.summary.overview];
    if (document.summary.keyPoints.length) lines.push("", "## 要点", "", ...document.summary.keyPoints.map((item) => `- ${item.text}`));
    if (document.topics.length) lines.push("", "## 議題", "", ...document.topics.map((item) => `- **${item.title}**${item.summary ? ` — ${item.summary}` : ""}`));
    if (document.decisions.length) lines.push("", "## 決定事項", "", ...document.decisions.map((item) => `- ${item.statement}`));
    if (document.actionItems.length) lines.push("", "## アクション項目", "", ...document.actionItems.map((item) => {
      const assignees = participantNames(item.assigneeParticipantIds);
      const due = item.due ? `（${item.due.rawText}）` : "";
      return `- [${item.status === "done" ? "x" : " "}] ${assignees ? `${assignees}：` : ""}${item.title}${due}`;
    }));
    if (document.openIssues.length) lines.push("", "## 未解決事項", "", ...document.openIssues.map((item) => `- **${item.title}**${item.description ? ` — ${item.description}` : ""}`));
    if (document.questions.length) lines.push("", "## 質問", "", ...document.questions.map((item) => `- ${item.text}${item.answer ? `\n  - 回答: ${item.answer.text}` : ""}`));
    if (document.notes.length) lines.push("", "## ノート", "", ...document.notes.map((item) => `- ${item.title ? `**${item.title}** — ` : ""}${item.body}`));
    return `${lines.join("\n").trim()}\n`;
  }

  async function copyMarkdown() {
    const document = editableDocument ?? summary;
    if (!document) return;
    try {
      await navigator.clipboard.writeText(markdown(document));
      copied = true;
    } catch {
      copied = false;
    }
    window.setTimeout(() => copied = false, 1800);
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
    <ProcessingStage
      kind="summary"
      status={compactProgressLabel}
      detail={progressDetail}
      progressValue={visibleProgressStep}
      progressMax={progress?.totalSteps ?? null}
      summarySourceLines={transcript?.segments.map((segment) => segment.text) ?? []}
    />
  {:else}
  {#if attempt?.status === "failed"}
    <div class="generation-failure" role="alert">
      <AlertTriangle aria-hidden="true" />
      <div>
        <strong>生成結果の検証に失敗しました</strong>
        <p>{attempt.error ?? "生成結果を会議ノートとして確定できませんでした。"}</p>
        <small>試行ID: {attempt.attemptId}</small>
      </div>
      {#if attempt.canRevalidate}
        <Button size="sm" variant="outline" type="button" icon={RefreshCw} onclick={onRevalidate} disabled={generating || blocked}>保存結果を再検証</Button>
      {/if}
    </div>
  {/if}
  {#if summary}
    {@const noteDocument = editableDocument ?? summary}
    <header class="summary-toolbar">
      <div class="summary-heading">
        <div class="summary-title-row">
          <strong>会議ノート</strong>
          {#if stale}
            <Popover>
              <PopoverTrigger>
                {#snippet child({ props })}
                  <Button {...props} class="stale-note-trigger" size="icon-xs" variant="ghost" type="button" icon={AlertTriangle} aria-label="文字起こし変更前の会議ノートについて" title="文字起こし変更前の会議ノート" />
                {/snippet}
              </PopoverTrigger>
              <PopoverContent class="stale-note-popover" align="start" sideOffset={7}>
                <strong>文字起こし変更前の会議ノートです</strong>
                <p>内容は編集できます。現在の文字起こしを反映して作り直す場合は「要約を更新」を押してください。</p>
              </PopoverContent>
            </Popover>
          {/if}
        </div>
        <span>モデルを変更して再生成できます</span>
      </div>
      <div class="summary-controls">
        <Button size="icon-sm" variant="ghost" type="button" icon={copied ? Check : Clipboard} onclick={copyMarkdown} aria-label="会議ノートをコピー" title={copied ? "コピーしました" : "コピー"} />
        {@render providerSelect("summary-provider-toolbar")}
        {@render modelSelect("summary-model-toolbar")}
        <Button size="sm" variant="outline" type="button" icon={RefreshCw} onclick={onGenerate} disabled={!canGenerate} loading={generating}>要約を更新</Button>
      </div>
    </header>

    {#if provider && !provider.ready}
      <p class="provider-warning" role="status">{provider.statusMessage}</p>
    {/if}

    <article class="note">
      <section class="overview">
        <h2>概要</h2>
        <textarea class="note-editor overview-editor" rows="1" use:autoResizeTextArea={noteDocument.summary.overview} bind:value={noteDocument.summary.overview} aria-label="概要" oninput={markDirty} onblur={flushEdits}></textarea>
      </section>

      {#if noteDocument.summary.keyPoints.length > 0}
        <section><h2>要点</h2><ul>{#each noteDocument.summary.keyPoints as point (point.keyPointId)}{@const ids = sourceIds(point.evidenceIds)}<li><textarea class="note-editor" rows="1" use:autoResizeTextArea={point.text} bind:value={point.text} aria-label="要点" oninput={markDirty} onblur={flushEdits}></textarea>{#if ids.length}<button type="button" onclick={(event) => showSource(event, { key: point.keyPointId, kind: "keyPoint", text: point.text, sourceSegmentIds: ids })}>根拠を見る</button>{/if}</li>{/each}</ul></section>
      {/if}

      {#if noteDocument.topics.length > 0}
        <section><h2>議題</h2><ul>{#each noteDocument.topics as topic (topic.topicId)}{@const ids = sourceIds(topic.evidenceIds)}<li><div class="edit-fields"><input class="note-editor title-editor" bind:value={topic.title} aria-label="議題名" oninput={markDirty} onblur={flushEdits} /><textarea class="note-editor" rows="1" use:autoResizeTextArea={topic.summary} bind:value={topic.summary} aria-label="議題の概要" placeholder="概要を追加" oninput={markDirty} onblur={flushEdits}></textarea></div>{#if ids.length}<button type="button" onclick={(event) => showSource(event, { key: topic.topicId, kind: "topic", text: topic.title, sourceSegmentIds: ids })}>根拠を見る</button>{/if}</li>{/each}</ul></section>
      {/if}

      <section>
        <h2><CheckCircle2 aria-hidden="true" />決定事項</h2>
        {#if noteDocument.decisions.length > 0}
          <ul>
            {#each noteDocument.decisions as decision (decision.decisionId)}
              {@const evidenceSourceIds = sourceIds(decision.evidenceIds)}
              {@const sourceKey = decision.decisionId}
              <li class:source-selected={selectedSourceKey === sourceKey}>
                <div class="edit-fields"><textarea class="note-editor" rows="1" use:autoResizeTextArea={decision.statement} bind:value={decision.statement} aria-label="決定事項" oninput={markDirty} onblur={flushEdits}></textarea><textarea class="note-editor" rows="1" use:autoResizeTextArea={decision.rationale} bind:value={decision.rationale} aria-label="決定理由" placeholder="理由を追加" oninput={markDirty} onblur={flushEdits}></textarea></div>
                {#if evidenceSourceIds.length > 0}
                  <button
                    type="button"
                    aria-expanded={selectedSourceKey === sourceKey}
                    aria-controls="summary-source-sheet"
                    aria-label={`根拠を見る: ${decision.statement}`}
                    onclick={(event) => showSource(event, { key: sourceKey, kind: "decision", text: decision.statement, sourceSegmentIds: evidenceSourceIds })}
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
        {#if noteDocument.actionItems.length > 0}
          <ul>
            {#each noteDocument.actionItems as item (item.actionItemId)}
              {@const evidenceSourceIds = sourceIds(item.evidenceIds)}
              {@const sourceKey = item.actionItemId}
              {@const assignees = participantNames(item.assigneeParticipantIds)}
              {@const itemText = `${assignees ? `${assignees}：` : ""}${item.title}${item.due ? `（${item.due.rawText}）` : ""}`}
              <li class:source-selected={selectedSourceKey === sourceKey}>
                <div class="checklist-item">
                  <Checkbox checked={item.status === "done"} onCheckedChange={(checked) => void toggleAction(item.actionItemId, checked)} disabled={saving} aria-label={`${item.title}を完了としてマーク`} />
                  <div class="edit-fields"><input class="note-editor title-editor" class:completed={item.status === "done"} bind:value={item.title} aria-label="アクション項目" oninput={markDirty} onblur={flushEdits} /><textarea class="note-editor" rows="1" use:autoResizeTextArea={item.description} bind:value={item.description} aria-label="アクションの詳細" placeholder="詳細を追加" oninput={markDirty} onblur={flushEdits}></textarea></div>
                </div>
                {#if evidenceSourceIds.length > 0}
                  <button
                    type="button"
                    aria-expanded={selectedSourceKey === sourceKey}
                    aria-controls="summary-source-sheet"
                    aria-label={`根拠を見る: ${itemText}`}
                    onclick={(event) => showSource(event, { key: sourceKey, kind: "actionItem", text: itemText, sourceSegmentIds: evidenceSourceIds })}
                  >
                    {selectedSourceKey === sourceKey ? "根拠を表示中" : "根拠を見る"}
                  </button>
                {/if}
              </li>
            {/each}
          </ul>
        {:else}<p class="empty-section">アクション項目はありません。</p>{/if}
      </section>
      {#if noteDocument.openIssues.length > 0}<section><h2>未解決事項</h2><ul>{#each noteDocument.openIssues as issue (issue.issueId)}{@const ids=sourceIds(issue.evidenceIds)}<li><div class="edit-fields"><input class="note-editor title-editor" bind:value={issue.title} aria-label="未解決事項" oninput={markDirty} onblur={flushEdits} /><textarea class="note-editor" rows="1" use:autoResizeTextArea={issue.description} bind:value={issue.description} aria-label="未解決事項の詳細" placeholder="詳細を追加" oninput={markDirty} onblur={flushEdits}></textarea></div>{#if ids.length}<button type="button" onclick={(event)=>showSource(event,{key:issue.issueId,kind:"openIssue",text:issue.title,sourceSegmentIds:ids})}>根拠を見る</button>{/if}</li>{/each}</ul></section>{/if}
      {#if noteDocument.questions.length > 0}<section><h2>質問</h2><ul>{#each noteDocument.questions as question (question.questionId)}{@const ids=sourceIds(question.evidenceIds)}<li><div class="edit-fields"><textarea class="note-editor" rows="1" use:autoResizeTextArea={question.text} bind:value={question.text} aria-label="質問" oninput={markDirty} onblur={flushEdits}></textarea>{#if question.answer}<textarea class="note-editor" rows="1" use:autoResizeTextArea={question.answer.text} bind:value={question.answer.text} aria-label="回答" oninput={markDirty} onblur={flushEdits}></textarea>{/if}</div>{#if ids.length}<button type="button" onclick={(event)=>showSource(event,{key:question.questionId,kind:"question",text:question.text,sourceSegmentIds:ids})}>根拠を見る</button>{/if}</li>{/each}</ul></section>{/if}
      {#if noteDocument.notes.length > 0}<section><h2>ノート</h2><ul>{#each noteDocument.notes as note (note.noteId)}{@const ids=sourceIds(note.evidenceIds)}<li><div class="edit-fields"><input class="note-editor title-editor" bind:value={note.title} aria-label="ノートのタイトル" placeholder="タイトルを追加" oninput={markDirty} onblur={flushEdits} /><textarea class="note-editor" rows="1" use:autoResizeTextArea={note.body} bind:value={note.body} aria-label="ノート本文" oninput={markDirty} onblur={flushEdits}></textarea></div>{#if ids.length}<button type="button" onclick={(event)=>showSource(event,{key:note.noteId,kind:"note",text:note.title ?? note.body,sourceSegmentIds:ids})}>根拠を見る</button>{/if}</li>{/each}</ul></section>{/if}
      <footer>revision {noteDocument.revision}{saving ? " · 保存中…" : ""}{#if generation} · {generation.provider} · {generation.model} · {new Date(generation.createdAt).toLocaleString("ja-JP")}{/if}</footer>
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
  {/if}
</section>

<style>
  .meeting-summary { max-width: 880px; margin: 22px auto 0; }
  .generation-failure { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: start; gap: 12px; margin-bottom: 18px; padding: 14px; border: 1px solid color-mix(in oklch, var(--destructive) 28%, var(--border)); border-radius: 10px; background: color-mix(in oklch, var(--destructive) 6%, var(--background)); }
  .generation-failure > :global(svg) { width: 18px; height: 18px; margin-top: 1px; color: var(--destructive); }
  .generation-failure div { min-width: 0; }
  .generation-failure strong { font-size: 0.8rem; }
  .generation-failure p { margin: 4px 0; color: var(--muted-foreground); font-size: 0.74rem; line-height: 1.5; white-space: pre-wrap; }
  .generation-failure small { color: var(--muted-foreground); font-size: 0.62rem; overflow-wrap: anywhere; }
  .summary-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 20px; padding-bottom: 18px; border-bottom: 1px solid var(--border); }
  .summary-heading { display: grid; min-width: 0; gap: 3px; }
  .summary-title-row { display: flex; align-items: center; gap: 3px; }
  .summary-heading strong { font-size: 0.92rem; }
  .summary-heading span { color: var(--muted-foreground); font-size: 0.72rem; }
  .summary-title-row :global(.stale-note-trigger) { color: color-mix(in oklch, var(--brand-amber) 75%, var(--foreground)); }
  :global(.stale-note-popover) { display: grid; width: min(340px, calc(100vw - 32px)); gap: 5px; padding: 12px 14px; }
  :global(.stale-note-popover strong) { font-size: 0.78rem; }
  :global(.stale-note-popover p) { margin: 0; color: var(--muted-foreground); font-size: 0.71rem; line-height: 1.55; }
  .summary-controls { display: flex; min-width: 0; flex-wrap: wrap; justify-content: flex-end; gap: 8px; }
  .summary-controls :global([data-slot="select-trigger"]) { width: 170px; min-width: 0; max-width: 100%; }
  :global(.summary-select) { min-width: 0; max-width: 100%; }
  .select-value { display: block; min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .provider-warning { margin: 14px 0 0; padding: 10px 12px; border-radius: 8px; font-size: 0.75rem; }
  .provider-warning { color: var(--destructive); background: color-mix(in oklch, var(--destructive) 8%, var(--background)); }
  .note { display: grid; min-width: 0; gap: 26px; margin: 0; padding: 26px 2px 40px; border: 0; }
  .note section { display: grid; gap: 10px; }
  .note h2 { display: flex; align-items: center; gap: 7px; margin: 0; font-size: 0.82rem; letter-spacing: 0.015em; }
  .note h2 :global(svg) { width: 16px; color: var(--primary); }
  .note-editor { box-sizing: border-box; display: block; width: 100%; min-width: 0; padding: 2px 5px; border: 1px solid transparent; border-radius: 6px; color: var(--foreground); background: transparent; font: inherit; line-height: 1.6; }
  .note-editor:hover { border-color: color-mix(in oklch, var(--border) 75%, transparent); background: color-mix(in oklch, var(--background) 70%, transparent); }
  .note-editor:focus { border-color: color-mix(in oklch, var(--primary) 55%, var(--border)); outline: 2px solid color-mix(in oklch, var(--primary) 18%, transparent); background: var(--background); }
  textarea.note-editor { min-height: calc(1.6em + 6px); overflow: hidden; resize: none; }
  .overview-editor { font-size: 0.9rem; line-height: 1.8; }
  .title-editor { font-weight: 650; }
  .edit-fields { display: grid; width: 100%; gap: 2px; }
  ul { display: grid; gap: 0; margin: 0; padding: 0; list-style: none; }
  li { display: flex; align-items: flex-start; justify-content: space-between; gap: 14px; padding: 10px 0; border-bottom: 1px solid color-mix(in oklch, var(--border) 70%, transparent); font-size: 0.84rem; line-height: 1.6; }
  li.source-selected { margin: 0 -10px; padding-right: 10px; padding-left: 10px; border-radius: 8px; background: color-mix(in oklch, var(--primary) 8%, transparent); }
  li button { flex: none; padding: 2px 0; border: 0; color: var(--primary); background: transparent; cursor: pointer; font: inherit; font-size: 0.69rem; font-weight: 650; }
  li button:focus-visible { border-radius: 3px; outline: 2px solid var(--ring); outline-offset: 3px; }
  .checklist-item { display: flex; min-width: 0; flex: 1; align-items: flex-start; gap: 10px; cursor: pointer; }
  .checklist-item :global([data-slot="checkbox"]) { width: 17px; height: 17px; flex: none; margin-top: 4px; }
  .checklist-item .completed { color: var(--muted-foreground); text-decoration: line-through; }
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
    .generation-failure { grid-template-columns: auto minmax(0, 1fr); }
    .generation-failure :global(button) { grid-column: 1 / -1; width: 100%; }
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
