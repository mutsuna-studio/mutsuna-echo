<script lang="ts">
  import { onMount } from "svelte";
  import MoreVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import Play from "@lucide/svelte/icons/play";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import { AdminShellFrame } from "@mutsuna/ui/admin-shell-frame";
  import { ThemeProvider, createTheme } from "@mutsuna/ui/theme";
  import AppSidebar from "../components/AppSidebar.svelte";
  import ProcessingStage from "../components/ProcessingStage.svelte";

  const echoTheme = createTheme("custom", "oklch(0.527 0.093 185.044)");
  const noop = () => {};
  const previewWaveform = [0.12,0.24,0.18,0.42,0.72,0.38,0.56,0.88,0.44,0.28,0.62,0.94,0.48,0.34,0.76,0.52,0.22,0.46,0.82,0.58,0.36,0.68,0.9,0.42,0.26,0.54,0.78,0.48,0.32,0.64,0.86,0.4,0.2,0.5,0.74,0.36,0.18,0.44,0.68,0.3];
  let activeTab = $state<"summary" | "transcription" | "info">(
    new URLSearchParams(window.location.search).get("kind") === "summary" ? "summary" : "transcription"
  );
  let transcriptionPreviewProgress = $state(0);

  onMount(() => {
    const timer = window.setInterval(() => {
      transcriptionPreviewProgress = transcriptionPreviewProgress >= 12 ? 0 : transcriptionPreviewProgress + 1;
    }, 1_800);
    return () => window.clearInterval(timer);
  });
</script>

<ThemeProvider theme={echoTheme}>
  <AdminShellFrame
    pageTitle="録音と会議"
    contentGutter="auto"
    contentPadding="none"
    contentClass="app-shell-frame-content overflow-hidden"
    headerClass="app-main-header"
  >
    {#snippet sidebar()}
      <AppSidebar
        section="meetings"
        settingsPane="general"
        settingsPreview={false}
        recordingBusy={false}
        onNavigate={noop}
        onSelectSettingsPane={noop}
      />
    {/snippet}

    <main class="processing-preview">
      <header class="meeting-heading">
        <div>
          <h1>プロダクト定例ミーティング</h1>
          <p>2026年8月12日(水) 10:00 · 30:12</p>
        </div>
      </header>

      <div class="audio-preview" aria-label="音声プレイヤー">
        <button type="button" aria-label="再生"><Play aria-hidden="true" /></button>
        <RotateCcw aria-hidden="true" />
        <span>00:00</span>
        <div class="audio-track"><i></i></div>
        <span>30:12</span>
        <Volume2 aria-hidden="true" />
        <div class="volume-track"><i></i></div>
        <span>1×</span>
        <MoreVertical aria-hidden="true" />
      </div>

      <div class="detail-tabs" role="tablist" aria-label="会議の表示内容">
        <button class:active={activeTab === "summary"} type="button" role="tab" aria-selected={activeTab === "summary"} onclick={() => activeTab = "summary"}>会議ノート</button>
        <button class:active={activeTab === "transcription"} type="button" role="tab" aria-selected={activeTab === "transcription"} onclick={() => activeTab = "transcription"}>文字起こし</button>
        <button class:active={activeTab === "info"} type="button" role="tab" aria-selected={activeTab === "info"} onclick={() => activeTab = "info"}>会議情報</button>
      </div>

      {#if activeTab === "summary"}
        <ProcessingStage
          kind="summary"
          status="内容とタイトルを確認中"
          detail="経過 18秒 · 3/4"
          progressValue={3}
          progressMax={4}
          summarySourceLines={[
            "次回リリースでは録音一覧の検索性を優先して改善します",
            "デザイン案は木曜日までに共有し、金曜日にレビューします",
            "モバイル版の対応範囲は実装コストを確認して決定します"
          ]}
        />
      {:else if activeTab === "transcription"}
        <ProcessingStage
          kind="transcription"
          status={`文字起こし中 ${transcriptionPreviewProgress} / 12`}
          detail="聞き取ったことばを読みやすく整えています"
          progressValue={transcriptionPreviewProgress}
          progressMax={12}
          waveformPeaks={previewWaveform}
        />
      {:else}
        <section class="info-preview" aria-label="会議情報のプレビュー">
          <strong>会議情報</strong>
          <p>待機状態は「会議ノート」と「文字起こし」の各タブで確認できます。</p>
        </section>
      {/if}
    </main>
  </AdminShellFrame>
</ThemeProvider>

<style>
  .processing-preview { box-sizing: border-box; height: 100%; padding: 28px 34px 0; overflow: hidden; }
  .meeting-heading { display: flex; align-items: flex-start; justify-content: space-between; margin-bottom: 22px; }
  .meeting-heading h1 { margin: 0; font-size: 1.42rem; letter-spacing: 0.01em; }
  .meeting-heading p { margin: 7px 0 0; color: var(--muted-foreground); font-size: 0.72rem; }
  .audio-preview { display: grid; min-height: 76px; grid-template-columns: auto auto auto minmax(180px, 1fr) auto auto 120px auto auto; align-items: center; gap: 16px; color: var(--muted-foreground); }
  .audio-preview button { display: grid; width: 46px; height: 46px; place-items: center; border: 0; border-radius: 50%; color: var(--primary-foreground); background: var(--primary); }
  .audio-preview :global(svg) { width: 17px; height: 17px; }
  .audio-preview button :global(svg) { fill: currentColor; }
  .audio-track, .volume-track { height: 5px; overflow: hidden; border-radius: 99px; background: var(--muted); }
  .audio-track i, .volume-track i { display: block; width: 0; height: 100%; background: var(--primary); }
  .volume-track i { width: 68%; }
  .detail-tabs { display: grid; grid-template-columns: repeat(3, 1fr); border-top: 1px solid var(--border); border-bottom: 1px solid var(--border); }
  .detail-tabs button { position: relative; min-height: 50px; border: 0; color: var(--muted-foreground); background: transparent; font: inherit; font-size: 0.76rem; font-weight: 680; }
  .detail-tabs button.active { color: var(--primary); }
  .detail-tabs button.active::after { position: absolute; right: 14%; bottom: -1px; left: 14%; height: 2px; background: var(--primary); content: ""; }
  .info-preview { display: grid; min-height: 430px; place-content: center; gap: 8px; text-align: center; }
  .info-preview strong { font-size: 1rem; }
  .info-preview p { margin: 0; color: var(--muted-foreground); font-size: 0.75rem; }
</style>
