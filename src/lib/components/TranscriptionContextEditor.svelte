<script lang="ts">
  import BookOpenText from "@lucide/svelte/icons/book-open-text";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Info from "@lucide/svelte/icons/info";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { Button } from "@mutsuna/ui/button";
  import { Input } from "@mutsuna/ui/input";
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
  type Correction = { from: string; to: string };

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

  let correctionFromDraft = $state("");
  let correctionToDraft = $state("");

  function parseCorrections(value: string): Correction[] {
    const seen = new Set<string>();
    return value.split(/\r?\n/).flatMap((line) => {
      const parts = line.split(/\s*(?:=>|⇒)\s*/, 2);
      const from = parts[0]?.trim() ?? "";
      const to = parts[1]?.trim() ?? "";
      if (!from || !to || from === to || seen.has(from)) return [];
      seen.add(from);
      return [{ from, to }];
    });
  }

  function serializeCorrections(corrections: Correction[]): string {
    return corrections.map(({ from, to }) => `${from} => ${to}`).join("\n");
  }

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
  const backgroundActive = $derived(
    contextEnabled && (provider?.capabilities.contextText ?? true)
  );
  const termsActive = $derived(
    contextEnabled && (provider?.capabilities.contextTerms ?? true)
  );
  const corrections = $derived(parseCorrections(correctionsText));
  const correctionFrom = $derived(correctionFromDraft.trim());
  const correctionTo = $derived(correctionToDraft.trim());
  const correctionDraftMessage = $derived.by(() => {
    if (!correctionFrom && !correctionTo) return "";
    if (!correctionFrom) return "置換前の文字を入力してください。";
    if (!correctionTo) return "置換後の文字を入力してください。";
    if (correctionFrom === correctionTo) return "置換前と置換後には異なる文字を入力してください。";
    if (corrections.some((correction) => correction.from === correctionFrom)) return "同じ置換前の文字がすでに登録されています。";
    return "";
  });
  const canAddCorrection = $derived(
    !disabled && Boolean(correctionFrom) && Boolean(correctionTo) && !correctionDraftMessage
  );
  const providerMessage = $derived.by(() => {
    if (!contextEnabled) {
      return "カスタム指示と辞書はオフです。入力内容は保存されますが、文字起こしには使われません。";
    }
    if (!provider) return "選択したモデルが対応している情報を、文字起こしの認識に使います。";
    if (!provider.capabilities.contextText && !provider.capabilities.contextTerms) {
      return `${provider.label}はカスタム指示と辞書に対応していません。置換のみ端末内で適用します。`;
    }
    if (!provider.capabilities.contextText) {
      return `${provider.label}では辞書を認識に使います。カスタム指示は保存され、対応モデルへ切り替えたときに利用できます。`;
    }
    return `${provider.label}ではカスタム指示と辞書を認識に使います。`;
  });

  function changeEnabled(value: boolean) {
    onContextEnabledChange?.(value);
  }

  function changeUseGlobal(value: boolean) {
    onUseGlobalChange?.(value);
  }

  function addCorrection(event: SubmitEvent) {
    event.preventDefault();
    if (!canAddCorrection) return;
    onCorrectionsChange(serializeCorrections([...corrections, { from: correctionFrom, to: correctionTo }]));
    correctionFromDraft = "";
    correctionToDraft = "";
  }

  function removeCorrection(index: number) {
    onCorrectionsChange(serializeCorrections(corrections.filter((_, correctionIndex) => correctionIndex !== index)));
  }
</script>

<section class="context-editor" aria-busy={loading || saveState === "saving"}>
  <div class="context-heading">
    <div class="context-title-icon" aria-hidden="true"><BookOpenText /></div>
    <div class="context-title-copy">
      <span class="context-eyebrow">文字起こしの認識精度を調整</span>
      <h3>{title}</h3>
      <p>{description} 入力しなくても文字起こしできます。</p>
    </div>
    <span
      class:error={saveState === "error"}
      class:pending={loading || saveState === "saving" || saveState === "unsaved"}
      class="save-status"
      aria-live="polite"
    ><i aria-hidden="true"></i>{saveLabel}</span>
  </div>

  {#if showMasterToggle || useGlobal !== undefined}
    <div class="context-options" aria-label="文字起こし補助設定の利用範囲">
      {#if showMasterToggle}
        <label class="toggle-row">
          <span>
            <strong>カスタム指示・辞書を認識に使う</strong>
            <small>オフにしても入力内容と置換は残ります。</small>
          </span>
          <Switch checked={contextEnabled} onCheckedChange={changeEnabled} {disabled} aria-label="カスタム指示と辞書を認識に使う" />
        </label>
      {/if}

      {#if useGlobal !== undefined}
        <label class="toggle-row">
          <span>
            <strong>全会議共通の設定も使う</strong>
            <small>共通のカスタム指示・辞書・置換に、この会議の内容を追加します。</small>
          </span>
          <Switch checked={useGlobal} onCheckedChange={changeUseGlobal} {disabled} aria-label="全会議共通の設定も使う" />
        </label>
      {/if}
    </div>
  {/if}

  <div class:inactive={!contextEnabled} class="context-provider-summary">
    <Info aria-hidden="true" />
    <div class="provider-copy">
      <strong>{provider?.label ?? "文字起こしモデルへの適用"}</strong>
      <p>{providerMessage}</p>
    </div>
    <div class="capability-list" aria-label="入力内容の適用状況">
      <span class:active={backgroundActive}>カスタム指示・{backgroundActive ? "使用" : "対象外"}</span>
      <span class:active={termsActive}>辞書・{termsActive ? "使用" : "対象外"}</span>
      <span class="active">置換・端末内</span>
    </div>
  </div>

  <div class="context-fields">
    <label class="context-field background-field">
      <span class="field-heading">
        <span><strong>カスタム指示</strong><small>誰が、何について話すかを文章で伝える</small></span>
        <small>{background.length.toLocaleString()} / 10,000文字</small>
      </span>
      <Textarea
        value={background}
        oninput={(event) => onBackgroundChange(event.currentTarget.value)}
        maxlength={10000}
        rows={5}
        placeholder="例：新製品Mutsuna Echoの定例会議。参加者は開発・営業チーム。議題は次期リリースの優先順位。"
        aria-label={`${title}のカスタム指示`}
        {disabled}
      />
      <small class="field-note">文章で入力できます。目的、参加者、製品名、議題などが効果的です。</small>
    </label>
    <label class="context-field">
      <span class="field-heading">
        <span><strong>辞書</strong><small>固有名詞や専門用語を認識しやすくする</small></span>
        <small>{termCount.toLocaleString()} / 1,000件</small>
      </span>
      <Textarea
        value={termsText}
        oninput={(event) => onTermsChange(event.currentTarget.value)}
        rows={6}
        placeholder={'Mutsuna Echo\nScribe v2\n製品固有の専門用語'}
        aria-label={`${title}の辞書`}
        {disabled}
      />
      <small class="field-note">1行に1用語を入力します。</small>
    </label>
    <section class="context-field correction-field" aria-labelledby="correction-field-title">
      <span class="field-heading">
        <span><strong id="correction-field-title">置換</strong><small>文字起こし後の表記を自動で置き換える</small></span>
        <small>{corrections.length.toLocaleString()}件・端末内で適用</small>
      </span>
      <form class="correction-builder" onsubmit={addCorrection}>
        <label>
          <span>置換前</span>
          <Input
            value={correctionFromDraft}
            oninput={(event) => correctionFromDraft = event.currentTarget.value}
            maxlength={100}
            placeholder="むつなエコー"
            aria-label={`${title}の置換前`}
            {disabled}
          />
        </label>
        <ArrowRight class="correction-arrow" aria-hidden="true" />
        <label>
          <span>置換後</span>
          <Input
            value={correctionToDraft}
            oninput={(event) => correctionToDraft = event.currentTarget.value}
            maxlength={100}
            placeholder="Mutsuna Echo"
            aria-label={`${title}の置換後`}
            {disabled}
          />
        </label>
        <Button type="submit" size="sm" icon={Plus} disabled={!canAddCorrection}>追加</Button>
      </form>
      <small class:error={Boolean(correctionDraftMessage)} class="correction-guidance" aria-live="polite">
        {correctionDraftMessage || "記号の入力は不要です。置換前と置換後を入力して「追加」を押します。"}
      </small>
      {#if corrections.length > 0}
        <div class="correction-list" aria-label="登録済みの置換">
          {#each corrections as correction, index (`${correction.from}\u0000${correction.to}`)}
            <div class="correction-row">
              <span title={correction.from}>{correction.from}</span>
              <ArrowRight aria-hidden="true" />
              <strong title={correction.to}>{correction.to}</strong>
              <Button
                type="button"
                size="icon-sm"
                variant="ghost"
                icon={Trash2}
                aria-label={`「${correction.from}」から「${correction.to}」への置換を削除`}
                onclick={() => removeCorrection(index)}
                {disabled}
              />
            </div>
          {/each}
        </div>
      {:else}
        <small class="correction-empty">登録済みの置換はありません。</small>
      {/if}
    </section>
  </div>
</section>

<style>
  .context-editor { display: grid; max-width: none; overflow: hidden; border: 1px solid var(--border); border-radius: 14px; background: var(--background); }
  .context-heading { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 12px; padding: 18px; }
  .context-title-icon { display: grid; width: 38px; height: 38px; place-items: center; border-radius: 10px; color: var(--primary); background: color-mix(in oklch, var(--primary) 10%, var(--background)); }
  .context-title-icon :global(svg) { width: 19px; height: 19px; }
  .context-title-copy { display: grid; min-width: 0; gap: 3px; }
  .context-eyebrow { color: var(--primary); font-size: 0.66rem; font-weight: 700; letter-spacing: 0.04em; }
  .context-heading h3 { margin: 0; font-size: 0.9rem; }
  .context-heading p { margin: 0; color: var(--muted-foreground); font-size: 0.72rem; line-height: 1.55; }
  .save-status { display: inline-flex; flex: none; align-items: center; gap: 6px; color: var(--muted-foreground); font-size: 0.68rem; white-space: nowrap; }
  .save-status i { width: 6px; height: 6px; border-radius: 999px; background: var(--primary); }
  .save-status.pending i { background: var(--muted-foreground); }
  .save-status.error { color: var(--destructive); }
  .save-status.error i { background: var(--destructive); }
  .context-options { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; padding: 0 18px 14px; }
  .context-options .toggle-row:only-child { grid-column: 1 / -1; }
  .toggle-row { display: flex; min-height: 64px; align-items: center; justify-content: space-between; gap: 18px; padding: 12px 14px; border: 1px solid var(--border); border-radius: 10px; background: color-mix(in oklch, var(--muted) 25%, var(--background)); }
  .toggle-row > span { display: grid; min-width: 0; gap: 3px; }
  .toggle-row strong { color: var(--foreground); font-size: 0.78rem; }
  .toggle-row small { color: var(--muted-foreground); font-size: 0.69rem; font-weight: 400; line-height: 1.45; }
  .context-provider-summary { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 10px; margin: 0 18px; padding: 11px 12px; border-radius: 10px; color: var(--foreground); background: color-mix(in oklch, var(--primary) 7%, var(--background)); }
  .context-provider-summary > :global(svg) { width: 17px; height: 17px; color: var(--primary); }
  .context-provider-summary.inactive { background: color-mix(in oklch, var(--muted) 55%, var(--background)); }
  .context-provider-summary.inactive > :global(svg) { color: var(--muted-foreground); }
  .provider-copy { display: grid; min-width: 0; gap: 2px; }
  .provider-copy strong { font-size: 0.74rem; }
  .provider-copy p { margin: 0; color: var(--muted-foreground); font-size: 0.69rem; line-height: 1.5; }
  .capability-list { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 5px; }
  .capability-list span { padding: 3px 7px; border: 1px solid var(--border); border-radius: 999px; color: var(--muted-foreground); background: var(--background); font-size: 0.64rem; font-weight: 650; }
  .capability-list span.active { border-color: color-mix(in oklch, var(--primary) 24%, var(--border)); color: var(--primary); background: color-mix(in oklch, var(--primary) 8%, var(--background)); }
  .context-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; padding: 18px; }
  .context-field { display: grid; min-width: 0; align-content: start; gap: 8px; padding: 14px; border: 1px solid var(--border); border-radius: 10px; background: color-mix(in oklch, var(--muted) 16%, var(--background)); }
  .background-field { grid-column: 1 / -1; }
  .field-heading { display: flex; align-items: start; justify-content: space-between; gap: 14px; }
  .field-heading > span { display: grid; gap: 2px; }
  .field-heading strong { color: var(--foreground); font-size: 0.78rem; }
  .field-heading small, .field-note { color: var(--muted-foreground); font-size: 0.66rem; font-weight: 400; line-height: 1.45; }
  .field-heading > small { flex: none; white-space: nowrap; }
  .context-fields :global(textarea) { min-height: 126px; resize: vertical; background: var(--background); line-height: 1.55; }
  .background-field :global(textarea) { min-height: 104px; }
  .correction-builder { display: grid; grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr) auto; align-items: end; gap: 8px; }
  .correction-builder label { display: grid; min-width: 0; gap: 5px; }
  .correction-builder label > span { color: var(--muted-foreground); font-size: 0.66rem; font-weight: 650; }
  .correction-builder :global([data-slot="input"]) { width: 100%; min-width: 0; background: var(--background); }
  .correction-builder :global(button) { margin-bottom: 1px; }
  .correction-arrow { width: 15px; height: 15px; margin-bottom: 8px; color: var(--muted-foreground); }
  .correction-guidance, .correction-empty { color: var(--muted-foreground); font-size: 0.66rem; font-weight: 400; line-height: 1.45; }
  .correction-guidance.error { color: var(--destructive); }
  .correction-list { display: grid; gap: 6px; margin-top: 2px; }
  .correction-row { display: grid; grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr) auto; align-items: center; gap: 8px; min-height: 36px; padding: 4px 5px 4px 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--background); font-size: 0.72rem; }
  .correction-row > span, .correction-row > strong { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .correction-row > span { color: var(--muted-foreground); font-weight: 500; }
  .correction-row > strong { color: var(--foreground); font-weight: 650; }
  .correction-row > :global(svg) { width: 14px; height: 14px; color: var(--muted-foreground); }

  @media (max-width: 640px) {
    .context-heading { grid-template-columns: auto minmax(0, 1fr); padding: 15px; }
    .save-status { grid-column: 2; }
    .context-options { grid-template-columns: minmax(0, 1fr); padding: 0 15px 12px; }
    .context-options .toggle-row:only-child { grid-column: auto; }
    .toggle-row { align-items: center; }
    .context-provider-summary { grid-template-columns: auto minmax(0, 1fr); margin: 0 15px; }
    .capability-list { grid-column: 2; justify-content: flex-start; }
    .context-fields { grid-template-columns: minmax(0, 1fr); padding: 15px; }
    .background-field { grid-column: auto; }
    .field-heading { align-items: start; }
    .correction-builder { grid-template-columns: minmax(0, 1fr) auto; }
    .correction-builder label { grid-column: 1 / -1; }
    .correction-builder .correction-arrow { display: none; }
    .correction-builder :global(button) { grid-column: 2; }
  }
</style>
