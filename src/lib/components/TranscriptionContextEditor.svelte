<script lang="ts">
  import { Switch } from "@mutsuna/ui/switch";
  import { Textarea } from "@mutsuna/ui/textarea";
  import type { TranscriptionProviderDefinition } from "../providers";
  import type { ContextSaveState } from "../types/transcript";

  type Props = {
    title: string;
    description: string;
    contextEnabled: boolean;
    showMasterToggle?: boolean;
    background: string;
    termsText: string;
    correctionsText: string;
    useGlobal?: boolean;
    provider?: TranscriptionProviderDefinition | null;
    saveState: ContextSaveState;
    disabled?: boolean;
    loading?: boolean;
    onContextEnabledChange?: (enabled: boolean) => void;
    onBackgroundChange: (background: string) => void;
    onTermsChange: (termsText: string) => void;
    onCorrectionsChange: (correctionsText: string) => void;
    onUseGlobalChange?: (useGlobal: boolean) => void;
  };

  let {
    title,
    description,
    contextEnabled,
    showMasterToggle = false,
    background,
    termsText,
    correctionsText,
    useGlobal,
    provider = null,
    saveState,
    disabled = false,
    loading = false,
    onContextEnabledChange,
    onBackgroundChange,
    onTermsChange,
    onCorrectionsChange,
    onUseGlobalChange
  }: Props = $props();

  const termCount = $derived(
    new Set(termsText.split(/\r?\n/).map((term) => term.trim()).filter(Boolean)).size
  );
  const saveLabel = $derived.by(() => {
    if (loading) return "読み込み中…";
    if (saveState === "saving") return "保存中…";
    if (saveState === "unsaved") return "変更を保存します…";
    if (saveState === "error") return "保存できませんでした";
    return "保存済み";
  });
  const providerMessage = $derived.by(() => {
    if (!provider) return "Sonioxでは背景情報と重要用語、ElevenLabsとローカルSTTでは重要用語だけを使用します。表記補正はクラウドへ送信せず、端末内の整形時だけ適用します。";
    if (!contextEnabled) return "コンテキストは全体設定でオフになっているため、この内容は送信されません。";
    if (!provider.capabilities.contextText && !provider.capabilities.contextTerms) return `${provider.modelLabel}はコンテキストに対応していません。入力内容は保存されます。`;
    if (!provider.capabilities.contextText) return `${provider.modelLabel}では重要用語だけを使用します。背景情報は保存されますが送信されません。`;
    return `${provider.modelLabel}では背景情報と重要用語を使用します。`;
  });

  function changeEnabled(value: boolean) {
    onContextEnabledChange?.(value);
  }

  function changeUseGlobal(value: boolean) {
    onUseGlobalChange?.(value);
  }
</script>

<section class="context-editor" aria-busy={loading || saveState === "saving"}>
  <div class="context-heading">
    <div>
      <h3>{title}</h3>
      <p>{description}</p>
    </div>
    <span class:error={saveState === "error"} aria-live="polite">{saveLabel}</span>
  </div>

  {#if showMasterToggle}
    <label class="toggle-row">
      <span><strong>文字起こしでコンテキストを使用</strong><small>オフにしても入力内容は削除されません。</small></span>
      <Switch checked={contextEnabled} onCheckedChange={changeEnabled} {disabled} aria-label="文字起こしでコンテキストを使用" />
    </label>
  {/if}

  {#if useGlobal !== undefined}
    <label class="toggle-row">
      <span><strong>全会議共通の内容を含める</strong><small>オフにすると、この会議固有の内容だけを使用します。</small></span>
      <Switch checked={useGlobal} onCheckedChange={changeUseGlobal} {disabled} aria-label="全会議共通のコンテキストを含める" />
    </label>
  {/if}

  <p class:inactive={!contextEnabled} class="provider-message">{providerMessage}</p>

  <div class="context-fields">
    <label>
      <span>背景情報 <small>{background.length.toLocaleString()} / 10,000文字</small></span>
      <Textarea
        value={background}
        oninput={(event) => onBackgroundChange(event.currentTarget.value)}
        maxlength={10000}
        rows={5}
        placeholder="会議の目的、参加者、扱う製品や議題など"
        aria-label={`${title}の背景情報`}
        {disabled}
      />
    </label>
    <label>
      <span>表記補正 <small>1行に「誤表記 ⇒ 正式表記」</small></span>
      <Textarea
        value={correctionsText}
        oninput={(event) => onCorrectionsChange(event.currentTarget.value)}
        rows={6}
        placeholder={'むつなエコー => Mutsuna Echo\n10パーセント => 10％'}
        aria-label={`${title}の表記補正`}
        {disabled}
      />
      <small>端末内の機械整形時だけ適用します。発話欄で直した短い誤表記も端末内へ自動学習されます。認識原文は保持され、取り消せます。</small>
    </label>
    <label>
      <span>重要用語 <small>{termCount.toLocaleString()} / 1,000件・1行に1用語</small></span>
      <Textarea
        value={termsText}
        oninput={(event) => onTermsChange(event.currentTarget.value)}
        rows={6}
        placeholder={'Mutsuna Echo\nScribe v2\n製品固有の専門用語'}
        aria-label={`${title}の重要用語`}
        {disabled}
      />
    </label>
  </div>
</section>

<style>
  .context-editor { display: grid; max-width: none; }
  .context-heading { display: flex; min-height: 58px; align-items: center; justify-content: space-between; gap: 20px; padding: 9px 14px; border-bottom: 1px solid var(--border); }
  .context-heading > div { display: grid; gap: 4px; }
  .context-heading h3 { margin: 0; font-size: 0.8rem; }
  .context-heading p, .provider-message { margin: 0; color: var(--muted-foreground); font-size: 0.72rem; line-height: 1.55; }
  .context-heading > span { flex: none; color: var(--muted-foreground); font-size: 0.68rem; }
  .context-heading > span.error { color: var(--destructive); }
  .toggle-row { display: flex; min-height: 58px; align-items: center; justify-content: space-between; gap: 20px; padding: 9px 14px; border-bottom: 1px solid var(--border); }
  .toggle-row > span { display: grid; gap: 2px; }
  .toggle-row strong { color: var(--foreground); font-size: 0.78rem; }
  .toggle-row small { color: var(--muted-foreground); font-size: 0.69rem; font-weight: 400; }
  .provider-message { padding: 9px 14px; border-bottom: 1px solid var(--border); background: color-mix(in oklch, var(--primary) 5%, var(--background)); }
  .provider-message.inactive { background: color-mix(in oklch, var(--muted) 55%, var(--background)); }
  .context-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; padding: 14px; }
  .context-fields label { display: grid; min-width: 0; align-content: start; gap: 6px; color: var(--muted-foreground); font-size: 0.72rem; font-weight: 650; }
  .context-fields label > span { display: flex; justify-content: space-between; gap: 12px; }
  .context-fields label small { font-weight: 400; }
  .context-fields :global(textarea) { min-height: 132px; resize: vertical; line-height: 1.55; }

  @media (max-width: 640px) {
    .context-fields { grid-template-columns: minmax(0, 1fr); }
    .context-heading { align-items: stretch; flex-direction: column; gap: 8px; }
    .toggle-row { align-items: flex-start; }
  }
</style>
