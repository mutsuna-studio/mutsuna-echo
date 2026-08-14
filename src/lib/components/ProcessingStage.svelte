<script lang="ts">
  import { onMount } from "svelte";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import SummaryProcessingAnimation from "./SummaryProcessingAnimation.svelte";
  import TranscriptionProcessingAnimation from "./TranscriptionProcessingAnimation.svelte";

  type Props = {
    kind: "transcription" | "summary";
    status: string;
    detail?: string | null;
    progressValue?: number | null;
    progressMax?: number | null;
    waveformPeaks?: readonly number[];
    summarySourceLines?: readonly string[];
  };

  let {
    kind,
    status,
    detail = null,
    progressValue = null,
    progressMax = null,
    waveformPeaks = [],
    summarySourceLines = []
  }: Props = $props();

  const transcriptionHumor = [
    "「えーっと」も、ちゃんと会議の一員です。",
    "小声のひと言まで、耳を澄ませています。",
    "相づちと決定事項を、慎重に仕分けています。",
    "話が脱線した場所にも、道しるべを置いています。"
  ];
  const summaryHumor = [
    "会議が長かった分だけ、ノートは短くしておきます。",
    "全員がうなずいた瞬間を、決定事項か検討中です。",
    "宿題の担当者が逃げないよう、名前を確認しています。",
    "余談は味わい、要点はきっちり残します。"
  ];
  const transcriptionPhases = [
    "音声の特徴を聴き取っています",
    "話し言葉のつながりを考えています",
    "読みやすい文章に整えています"
  ];
  const summaryPhases = [
    "発言の流れを読み解いています",
    "決定事項と宿題を探しています",
    "会議ノートの構成を組み立てています"
  ];

  let humorIndex = $state(0);
  let phaseIndex = $state(0);
  const title = $derived(kind === "transcription" ? "ただいま、ことばを拾っています" : "会議の要点を整えています");
  const humorLines = $derived(kind === "transcription" ? transcriptionHumor : summaryHumor);
  const workPhases = $derived(kind === "transcription" ? transcriptionPhases : summaryPhases);
  const completionNote = $derived(kind === "transcription"
    ? "完了すると新しい文字起こしに自動で切り替わります"
    : "完了すると新しい会議ノートに自動で切り替わります");
  const initialVisualProgress = 0.04;
  const determinate = $derived(progressValue != null && progressMax != null && progressMax > 0);
  const actualProgressRatio = $derived(determinate ? Math.min(1, Math.max(0, (progressValue ?? 0) / (progressMax ?? 1))) : 0);
  const processedRatio = $derived(actualProgressRatio >= 1 ? 1 : Math.max(initialVisualProgress, actualProgressRatio));
  const visualProgressValue = $derived(determinate ? processedRatio * (progressMax ?? 1) : null);

  onMount(() => {
    const humorTimer = window.setInterval(() => {
      humorIndex = (humorIndex + 1) % humorLines.length;
    }, 6_500);
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const phaseTimer = reduceMotion ? null : window.setInterval(() => {
      phaseIndex = (phaseIndex + 1) % workPhases.length;
    }, 1_400);
    return () => {
      window.clearInterval(humorTimer);
      if (phaseTimer !== null) window.clearInterval(phaseTimer);
    };
  });

</script>

<section class="processing-stage" aria-label={title} aria-busy="true">
  <div class="processing-copy"><h2>{title}</h2></div>

  {#if kind === "transcription"}
    <div class="transcription-workbench" aria-hidden="true">
      <TranscriptionProcessingAnimation peaks={waveformPeaks} progress={processedRatio} />
    </div>
  {:else}
    <div class="summary-workbench">
      <SummaryProcessingAnimation
        sourceLines={summarySourceLines}
        progress={processedRatio}
        {determinate}
      />
    </div>

    <p class="work-phase" aria-hidden="true">
      <span class="ai-label">AI</span>
      {#key phaseIndex}<span class="phase-text">{workPhases[phaseIndex]}</span>{/key}
    </p>
  {/if}

  <div class="progress-copy" role="status" aria-live="polite" aria-atomic="true">
    <strong>{status}</strong>
    {#if detail}<small>{detail}</small>{/if}
  </div>
  {#if kind === "transcription"}
    {#if determinate}
      <progress max={progressMax ?? 1} value={visualProgressValue ?? 0} aria-label={status}></progress>
    {:else}
      <progress aria-label={status}></progress>
    {/if}
  {/if}

  <p class="processing-humor" aria-live="polite"><LoaderCircle aria-hidden="true" /><span>{humorLines[humorIndex]}</span></p>
  <p class="completion-note">{completionNote}</p>
</section>

<style>
  .processing-stage {
    box-sizing: border-box;
    display: grid;
    width: min(100%, 880px);
    min-height: 430px;
    margin: 0 auto;
    place-items: center;
    align-content: center;
    gap: 22px;
    padding: 48px 28px 64px;
    text-align: center;
  }

  .processing-copy { display: grid; gap: 8px; }
  h2 { margin: 0; font-size: clamp(1.15rem, 2.1vw, 1.55rem); letter-spacing: 0.01em; }

  .transcription-workbench {
    position: relative;
    width: min(100%, 610px);
    height: 126px;
  }
  .summary-workbench { width: min(100%, 610px); height: 100px; }
  .work-phase {
    display: flex;
    min-height: 24px;
    align-items: center;
    justify-content: center;
    gap: 9px;
    margin: -13px 0 -5px;
    color: var(--muted-foreground);
    font-size: 0.72rem;
  }
  .ai-label {
    padding: 2px 7px;
    border: 1px solid color-mix(in oklch, var(--primary) 34%, var(--border));
    border-radius: 999px;
    color: var(--primary);
    font-size: 0.58rem;
    font-weight: 760;
    letter-spacing: 0.08em;
  }
  .phase-text { animation: phase-arrive 240ms ease-out; }

  .progress-copy { display: flex; min-height: 20px; align-items: baseline; justify-content: center; gap: 10px; }
  .progress-copy strong { font-size: 0.84rem; }
  .progress-copy small { color: var(--muted-foreground); font-size: 0.69rem; }
  progress {
    width: min(100%, 470px);
    height: 7px;
    margin-top: -12px;
    overflow: hidden;
    border: 0;
    border-radius: 999px;
    appearance: none;
    background: color-mix(in oklch, var(--muted) 78%, var(--background));
    accent-color: var(--primary);
  }
  progress::-webkit-progress-bar { border-radius: inherit; background: color-mix(in oklch, var(--muted) 78%, var(--background)); }
  progress::-webkit-progress-value { border-radius: inherit; background: var(--primary); }
  progress::-moz-progress-bar { border-radius: inherit; background: var(--primary); }
  .processing-humor { display: flex; min-height: 1.6em; align-items: center; gap: 9px; margin: 2px 0 0; color: var(--foreground); font-size: 0.78rem; line-height: 1.6; }
  .processing-humor :global(svg) { width: 17px; height: 17px; flex: none; color: var(--primary); animation: humor-spin 1.8s linear infinite; }
  .completion-note { margin: -13px 0 0; color: var(--muted-foreground); font-size: 0.69rem; }

  @keyframes phase-arrive {
    from { opacity: 0; transform: translateY(3px); }
    to { opacity: 1; transform: translateY(0); }
  }
  @keyframes humor-spin { to { transform: rotate(360deg); } }

  @media (max-width: 760px) {
    .processing-stage { min-height: 380px; gap: 18px; padding: 34px 18px 48px; }
    .summary-workbench { width: min(100%, 540px); }
    .progress-copy { flex-direction: column; align-items: center; gap: 3px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .phase-text, .processing-humor :global(svg) { animation: none; transition: none; }
  }
</style>
