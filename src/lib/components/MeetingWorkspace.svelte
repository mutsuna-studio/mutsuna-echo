<script lang="ts">
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import Clock3 from "@lucide/svelte/icons/clock-3";
  import FileAudio from "@lucide/svelte/icons/file-audio";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import PanelLeftOpen from "@lucide/svelte/icons/panel-left-open";
  import { Button } from "@mutsuna/ui/button";
  import { Select } from "@mutsuna/ui/select";
  import { formatFileSize, formatTimestamp } from "../format";
  import {
    isTranscriptionProviderId,
    transcriptionProviderOptions,
    type TranscriptionProviderDefinition,
    type TranscriptionProviderId
  } from "../providers";
  import type { RecentMeetingSummary } from "../types/recording";
  import type { SelectedAudioFile, Transcript, TranscriptionProgress } from "../types/transcript";
  import AudioPlayer from "./AudioPlayer.svelte";
  import TranscriptView from "./TranscriptView.svelte";

  type Props = {
    selectedAudio: SelectedAudioFile | null;
    meeting: RecentMeetingSummary | null;
    transcript: Transcript | null;
    providers: readonly TranscriptionProviderDefinition[];
    provider: TranscriptionProviderId;
    providerLabel: string;
    providerStatus: string;
    transcribing: boolean;
    progress: TranscriptionProgress | null;
    canTranscribe: boolean;
    libraryOpen: boolean;
    onOpenLibrary: () => void;
    onTranscribe: () => void;
    onProviderChange: (provider: TranscriptionProviderId) => void;
    onReveal: (meeting: RecentMeetingSummary) => void;
    onCreate: () => void;
    onOpenSettings: () => void;
    onError: (message: string) => void;
  };

  let {
    selectedAudio,
    meeting,
    transcript,
    providers,
    provider,
    providerLabel,
    providerStatus,
    transcribing,
    progress,
    canTranscribe,
    libraryOpen,
    onOpenLibrary,
    onTranscribe,
    onProviderChange,
    onReveal,
    onCreate,
    onOpenSettings,
    onError
  }: Props = $props();

  let detailTab = $state<"transcript" | "info">("transcript");
  const providerOptions = $derived(transcriptionProviderOptions(providers));

  const title = $derived(selectedAudio?.name.replace(/\.[^.]+$/, "") ?? "会議を選択");
  const recordedAt = $derived(
    meeting
      ? new Intl.DateTimeFormat("ja-JP", { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }).format(
          meeting.source === "recording" ? meeting.occurredAtUnixMs : meeting.updatedAtUnixMs
        )
      : null
  );
  const transcriptionLabel = $derived.by(() => {
    if (!transcribing) return transcript ? "再文字起こし" : "文字起こし";
    if (progress?.stage === "detectingSpeech") return "発話を検出中…";
    if (progress?.stage === "transcribing" && progress.totalChunks != null) return `${progress.completedChunks} / ${progress.totalChunks}`;
    return "文字起こし中…";
  });

  function selectProvider(value: string) {
    if (isTranscriptionProviderId(value)) onProviderChange(value);
  }
</script>

<section class="meeting-workspace">
  {#if selectedAudio}
    <header class="workspace-header">
      <div class="title-row">
        {#if !libraryOpen}
          <Button size="icon-sm" variant="ghost" type="button" icon={PanelLeftOpen} aria-label="最近の会議を開く" title="最近の会議を開く" onclick={onOpenLibrary} />
        {/if}
        <div class="meeting-title">
          <h1>{title}</h1>
          <div class="meeting-meta">
            {#if recordedAt}<span><CalendarDays aria-hidden="true" />{recordedAt}</span>{/if}
            <span><Clock3 aria-hidden="true" />{formatTimestamp(selectedAudio.durationMs)}</span>
          </div>
        </div>
        <div class="header-actions">
          {#if meeting?.audioAvailable}
            <Button size="sm" variant="ghost" type="button" icon={FolderOpen} onclick={() => onReveal(meeting)}>場所を開く</Button>
          {/if}
        </div>
      </div>
    </header>

    <div class="audio-player-wrap">
      <AudioPlayer audio={selectedAudio} source={meeting?.source} {onError} />
    </div>

    <section class="transcription-toolbar" aria-label="文字起こしモデルと実行">
      <div class="model-picker">
        <span>文字起こしモデル</span>
        <Select
          value={provider}
          options={providerOptions}
          onValueChange={selectProvider}
          searchable
          disabled={transcribing || providers.length === 0}
          ariaLabel="文字起こしモデル"
        />
      </div>
      <span class="transcription-action" data-transcription-action>
        <Button type="button" onclick={onTranscribe} disabled={!canTranscribe} loading={transcribing}>{transcriptionLabel}</Button>
      </span>
      <div class="provider-line">
        <span class:ready={canTranscribe || Boolean(transcript)} aria-hidden="true"></span>
        <strong>{providerLabel}</strong>
        <small>{providerStatus}</small>
        {#if !canTranscribe && !transcript}<button type="button" onclick={onOpenSettings}>設定を確認</button>{/if}
      </div>
    </section>

    <div class="detail-tabs" role="tablist" aria-label="会議の表示内容">
      <button class:active={detailTab === "transcript"} type="button" role="tab" aria-selected={detailTab === "transcript"} onclick={() => detailTab = "transcript"}>文字起こし</button>
      <button class:active={detailTab === "info"} type="button" role="tab" aria-selected={detailTab === "info"} onclick={() => detailTab = "info"}>会議情報</button>
    </div>

    <div class="detail-content">
      {#if detailTab === "transcript"}
        {#if transcript}
          <TranscriptView {transcript} />
        {:else}
          <div class="empty-transcript">
            <FileAudio aria-hidden="true" />
            <h2>文字起こしはまだありません</h2>
            <p>上の「文字起こしモデル」から使用するモデルを選び、文字起こしを開始できます。</p>
          </div>
        {/if}
      {:else}
        <dl class="meeting-info">
          <div><dt>ファイル名</dt><dd>{selectedAudio.name}</dd></div>
          <div><dt>{meeting?.source === "recording" ? "録音日時" : "更新日時"}</dt><dd>{recordedAt ?? "読み込んだ音声"}</dd></div>
          <div><dt>長さ</dt><dd>{formatTimestamp(selectedAudio.durationMs)}</dd></div>
          <div><dt>ファイルサイズ</dt><dd>{formatFileSize(selectedAudio.sizeBytes)}</dd></div>
          <div><dt>Meeting ID</dt><dd class="meeting-id">{selectedAudio.meetingId}</dd></div>
        </dl>
      {/if}
    </div>
  {:else}
    <div class="workspace-empty">
      {#if !libraryOpen}
        <Button class="open-library" size="sm" variant="ghost" type="button" icon={PanelLeftOpen} onclick={onOpenLibrary}>最近の会議</Button>
      {/if}
      <FileAudio aria-hidden="true" />
      <h1>会議を選択してください</h1>
      <p>最近の録音を開くか、新しい録音・音声ファイルから始められます。</p>
      <Button type="button" onclick={onCreate}>新しい録音</Button>
    </div>
  {/if}
</section>

<style>
  .meeting-workspace { display: grid; width: 100%; height: 100%; min-width: 0; min-height: 0; grid-template-rows: auto auto auto auto minmax(0, 1fr); overflow: hidden; background: var(--background); }
  .workspace-header { padding: 25px 30px 18px; }
  .title-row { display: flex; min-width: 0; align-items: flex-start; gap: 12px; }
  .meeting-title { min-width: 0; flex: 1; }
  h1 { margin: 0; overflow: hidden; font-size: clamp(1.3rem, 2.4vw, 1.85rem); line-height: 1.2; letter-spacing: -0.035em; text-overflow: ellipsis; white-space: nowrap; }
  .meeting-meta { display: flex; flex-wrap: wrap; gap: 14px; margin-top: 9px; color: var(--muted-foreground); font-size: 0.79rem; }
  .meeting-meta span { display: flex; align-items: center; gap: 5px; }
  .meeting-meta :global(svg) { width: 14px; height: 14px; }
  .header-actions { display: flex; flex: none; gap: 6px; }

  .transcription-toolbar { display: grid; grid-template-columns: minmax(210px, 280px) auto; align-items: end; gap: 8px 12px; margin: 0 30px 18px; padding: 14px; border: 1px solid var(--border); border-radius: 10px; background: color-mix(in oklch, var(--muted) 35%, var(--background)); }
  .model-picker { display: grid; min-width: 0; gap: 5px; }
  .model-picker > span { color: var(--muted-foreground); font-size: 0.7rem; font-weight: 650; }
  .transcription-action { align-self: end; }
  .provider-line { display: flex; min-width: 0; grid-column: 1 / -1; align-items: center; gap: 7px; color: var(--muted-foreground); font-size: 0.75rem; }
  .provider-line > span { width: 7px; height: 7px; flex: none; border-radius: 50%; background: var(--muted-foreground); }
  .provider-line > span.ready { background: var(--primary); }
  .provider-line strong { color: var(--foreground); font-weight: 650; }
  .provider-line small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .provider-line button { flex: none; padding: 0; border: 0; color: var(--primary); background: transparent; cursor: pointer; font: inherit; font-weight: 650; }

  .audio-player-wrap { min-width: 0; margin: 0 30px 18px; container: audio-player / inline-size; }

  .detail-tabs { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); margin: 0 30px; border-bottom: 1px solid var(--border); }
  .detail-tabs button { position: relative; height: 42px; border: 0; color: var(--muted-foreground); background: transparent; cursor: pointer; font: inherit; font-size: 0.82rem; font-weight: 650; }
  .detail-tabs button::after { position: absolute; right: 0; bottom: -1px; left: 0; height: 2px; background: transparent; content: ""; }
  .detail-tabs button.active { color: var(--primary); }
  .detail-tabs button.active::after { background: var(--primary); }
  .detail-tabs button:focus-visible { outline: 2px solid var(--ring); outline-offset: -3px; }
  .detail-content { min-height: 0; overflow-x: hidden; overflow-y: auto; padding: 0 30px 36px; overscroll-behavior: contain; scrollbar-gutter: stable; }

  .empty-transcript, .workspace-empty { display: grid; place-items: center; align-content: center; text-align: center; }
  .empty-transcript { min-height: 360px; }
  .empty-transcript > :global(svg), .workspace-empty > :global(svg) { width: 34px; height: 34px; color: var(--primary); stroke-width: 1.5; }
  .empty-transcript h2, .workspace-empty h1 { margin: 14px 0 7px; font-size: 1.05rem; }
  .empty-transcript p, .workspace-empty p { max-width: 420px; margin: 0 0 18px; color: var(--muted-foreground); font-size: 0.82rem; line-height: 1.6; }
  .workspace-empty { position: relative; grid-row: 1 / -1; min-height: 100%; padding: 40px; }
  :global(.open-library) { position: absolute; top: 18px; left: 18px; }

  .meeting-info { display: grid; max-width: 720px; margin: 24px 0 0; }
  .meeting-info div { display: grid; grid-template-columns: 150px minmax(0, 1fr); gap: 18px; padding: 14px 0; border-bottom: 1px solid var(--border); }
  .meeting-info dt { color: var(--muted-foreground); font-size: 0.79rem; }
  .meeting-info dd { margin: 0; font-size: 0.84rem; }
  .meeting-id { overflow-wrap: anywhere; font-family: ui-monospace, monospace; font-size: 0.75rem !important; }

  @media (max-width: 780px) {
    .workspace-header { padding: 20px 18px 14px; }
    .title-row { flex-wrap: wrap; }
    .meeting-title { flex-basis: calc(100% - 44px); }
    .header-actions { width: 100%; justify-content: flex-end; }
    .audio-player-wrap, .transcription-toolbar, .detail-tabs { margin-right: 18px; margin-left: 18px; }
    .detail-content { padding-right: 18px; padding-left: 18px; }
  }

  @media (max-width: 520px) {
    .transcription-toolbar { grid-template-columns: minmax(0, 1fr); }
    .transcription-action { justify-self: stretch; }
    .transcription-action :global(button) { width: 100%; }
  }
</style>
