<script lang="ts">
  import FileUp from "@lucide/svelte/icons/file-up";
  import Mic from "@lucide/svelte/icons/mic";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { formatFileSize } from "../format";
  import type { RecentMeetingSummary } from "../types/recording";

  type MeetingGroup = { label: string; meetings: RecentMeetingSummary[] };

  type Props = {
    meetings: readonly RecentMeetingSummary[];
    loading: boolean;
    busy: boolean;
    selecting: boolean;
    onSelectMeeting: (meeting: RecentMeetingSummary) => void;
    onRefresh: () => void;
    onRecord: () => void;
    onSelectFile: () => void;
  };

  let { meetings, loading, busy, selecting, onSelectMeeting, onRefresh, onRecord, onSelectFile }: Props = $props();

  const groups = $derived.by<MeetingGroup[]>(() => {
    const today = new Date();
    const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
    const startOfYesterday = startOfToday - 86_400_000;
    const startOfWeek = startOfToday - 6 * 86_400_000;
    const buckets = new Map<string, RecentMeetingSummary[]>();
    for (const meeting of meetings) {
      const label = meeting.occurredAtUnixMs >= startOfToday
        ? "今日"
        : meeting.occurredAtUnixMs >= startOfYesterday
          ? "昨日"
          : meeting.occurredAtUnixMs >= startOfWeek
            ? "今週"
            : new Intl.DateTimeFormat("ja-JP", { year: "numeric", month: "long" }).format(meeting.occurredAtUnixMs);
      const items = buckets.get(label) ?? [];
      items.push(meeting);
      buckets.set(label, items);
    }
    return Array.from(buckets, ([label, items]) => ({ label, meetings: items }));
  });

  function meetingTime(meeting: RecentMeetingSummary): string {
    return new Intl.DateTimeFormat("ja-JP", { hour: "2-digit", minute: "2-digit" }).format(meeting.occurredAtUnixMs);
  }
</script>

<section class="meeting-home" aria-label="会議一覧">
  <div class="mobile-home-heading">
    <div><h1>会議</h1><span>{meetings.length}件</span></div>
    <Button size="icon" variant="ghost" type="button" icon={RefreshCw} aria-label="会議一覧を更新" title="更新" onclick={onRefresh} loading={loading} disabled={busy || loading} />
  </div>

  <div class="mobile-meeting-list">
    {#if loading && meetings.length === 0}
      <p class="mobile-library-message" role="status">会議を読み込んでいます…</p>
    {:else if meetings.length === 0}
      <div class="mobile-library-empty"><strong>会議はまだありません</strong><p>録音を開始するか、音声ファイルを選択してください。</p></div>
    {:else}
      {#each groups as group (group.label)}
        <section class="mobile-meeting-group" aria-labelledby={`mobile-meeting-group-${group.label}`}>
          <h2 id={`mobile-meeting-group-${group.label}`}>{group.label}</h2>
          <div class="mobile-meeting-cards">
            {#each group.meetings as meeting (meeting.meetingId)}
              <button class="mobile-meeting-card" type="button" onclick={() => onSelectMeeting(meeting)} disabled={busy} title={meeting.audioAvailable ? meeting.fileName : `${meeting.title}（音声なし）`}>
                <span class:imported={meeting.source === "imported"} class="mobile-meeting-icon" aria-hidden="true">{#if meeting.source === "recording"}<Mic />{:else}<FileUp />{/if}</span>
                <span class="mobile-meeting-copy">
                  <strong>{meeting.title}</strong>
                  <small>{meetingTime(meeting)} · {formatFileSize(meeting.sizeBytes)}</small>
                  <span class="mobile-meeting-badges">
                    <Badge variant="secondary">{meeting.source === "recording" ? "録音" : "取込"}</Badge>
                    {#if meeting.transcriptProviders.length > 0}<small>文字起こし済み</small>{/if}
                    {#if !meeting.audioAvailable}<small class="missing-audio">音声なし</small>{/if}
                  </span>
                </span>
                <span class="mobile-meeting-chevron" aria-hidden="true">›</span>
              </button>
            {/each}
          </div>
        </section>
      {/each}
    {/if}
  </div>

  <footer class="mobile-home-actions">
    <Button class="mobile-home-action" size="lg" type="button" icon={Mic} onclick={onRecord} disabled={busy}>録音開始</Button>
    <Button class="mobile-home-action" size="lg" variant="outline" type="button" icon={FileUp} onclick={onSelectFile} loading={selecting} disabled={busy}>ファイル選択</Button>
  </footer>
</section>

<style>
  .meeting-home { display: grid; box-sizing: border-box; width: 100%; height: 100%; min-height: 0; grid-template-rows: auto minmax(0, 1fr) auto; background: var(--background); }
  .mobile-home-heading { display: flex; align-items: center; justify-content: space-between; padding: 18px 16px 8px; }
  .mobile-home-heading > div { display: flex; align-items: baseline; gap: 8px; }
  .mobile-home-heading h1 { margin: 0; font-size: 1.55rem; }
  .mobile-home-heading span { color: var(--muted-foreground); font-size: 0.75rem; }
  .mobile-meeting-list { min-height: 0; overflow-x: hidden; overflow-y: auto; padding: 0 16px 18px; overscroll-behavior: contain; }
  .mobile-meeting-group { padding-top: 14px; }
  .mobile-meeting-group h2 { margin: 0 2px 8px; color: var(--muted-foreground); font-size: 0.76rem; font-weight: 700; }
  .mobile-meeting-cards { display: grid; gap: 9px; }
  .mobile-meeting-card { display: grid; width: 100%; min-width: 0; grid-template-columns: 42px minmax(0, 1fr) auto; align-items: center; gap: 12px; padding: 13px; border: 1px solid var(--border); border-radius: 13px; color: var(--foreground); background: var(--background); box-shadow: 0 2px 8px rgb(0 0 0 / 4%); cursor: pointer; font: inherit; text-align: left; }
  .mobile-meeting-card:active { background: var(--muted); }
  .mobile-meeting-card:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
  .mobile-meeting-card:disabled { cursor: not-allowed; opacity: 0.58; }
  .mobile-meeting-icon { display: grid; width: 42px; height: 42px; place-items: center; border-radius: 11px; color: var(--primary); background: color-mix(in oklch, var(--primary) 10%, var(--background)); }
  .mobile-meeting-icon.imported { color: var(--muted-foreground); background: var(--muted); }
  .mobile-meeting-icon :global(svg) { width: 21px; height: 21px; }
  .mobile-meeting-copy { display: grid; min-width: 0; gap: 4px; }
  .mobile-meeting-copy > strong { overflow: hidden; font-size: 0.9rem; text-overflow: ellipsis; white-space: nowrap; }
  .mobile-meeting-copy > small { color: var(--muted-foreground); font-size: 0.7rem; }
  .mobile-meeting-badges { display: flex; min-width: 0; align-items: center; gap: 6px; overflow: hidden; }
  .mobile-meeting-badges :global([data-slot="badge"]) { height: 19px; flex: none; padding: 0 6px; font-size: 0.61rem; }
  .mobile-meeting-badges small { overflow: hidden; color: var(--primary); font-size: 0.64rem; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  .mobile-meeting-badges .missing-audio { color: var(--destructive); }
  .mobile-meeting-chevron { color: var(--muted-foreground); font-size: 1.6rem; line-height: 1; }
  .mobile-library-message, .mobile-library-empty { margin: 28px 0; color: var(--muted-foreground); text-align: center; }
  .mobile-library-empty { padding: 28px 18px; border: 1px dashed var(--border); border-radius: 13px; }
  .mobile-library-empty strong { color: var(--foreground); font-size: 0.92rem; }
  .mobile-library-empty p { margin: 7px 0 0; font-size: 0.78rem; line-height: 1.55; }
  .mobile-home-actions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; padding: 12px max(14px, env(safe-area-inset-right, 0px)) calc(12px + env(safe-area-inset-bottom, 0px)) max(14px, env(safe-area-inset-left, 0px)); border-top: 1px solid var(--border); background: color-mix(in oklch, var(--background) 94%, transparent); box-shadow: 0 -8px 24px rgb(0 0 0 / 7%); }
  :global(.mobile-home-action) { width: 100%; min-width: 0; min-height: 58px; justify-content: center; padding-inline: 10px; font-size: clamp(0.78rem, 3.7vw, 0.95rem); }
  :global(.mobile-home-action svg) { width: 21px; height: 21px; }

  @media (min-width: 601px) {
    .mobile-home-heading, .mobile-meeting-list { width: min(920px, calc(100% - 64px)); margin-right: auto; margin-left: auto; }
    .mobile-home-heading { box-sizing: border-box; padding-top: 30px; }
    .mobile-meeting-list { box-sizing: border-box; }
    .mobile-home-actions { grid-template-columns: repeat(2, minmax(220px, 340px)); justify-content: center; padding: 14px 32px 18px; }
  }
</style>
