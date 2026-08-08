<script lang="ts">
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import X from "@lucide/svelte/icons/x";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { formatFileSize } from "../format";
  import type { RecentMeetingSummary } from "../types/recording";

  type MeetingGroup = { label: string; meetings: RecentMeetingSummary[] };

  type Props = {
    meetings: readonly RecentMeetingSummary[];
    selectedMeetingId: string | null;
    loading: boolean;
    busy: boolean;
    onSelect: (meeting: RecentMeetingSummary) => void;
    onRefresh: () => void;
    onClose: () => void;
  };

  let { meetings, selectedMeetingId, loading, busy, onSelect, onRefresh, onClose }: Props = $props();

  const groups = $derived.by<MeetingGroup[]>(() => {
    const today = new Date();
    const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
    const startOfYesterday = startOfToday - 86_400_000;
    const startOfWeek = startOfToday - 6 * 86_400_000;
    const buckets = new Map<string, RecentMeetingSummary[]>();
    for (const meeting of meetings) {
      const time = meeting.occurredAtUnixMs;
      const label = time >= startOfToday
        ? "今日"
        : time >= startOfYesterday
          ? "昨日"
          : time >= startOfWeek
            ? "今週"
            : new Intl.DateTimeFormat("ja-JP", { year: "numeric", month: "long" }).format(time);
      const bucket = buckets.get(label) ?? [];
      bucket.push(meeting);
      buckets.set(label, bucket);
    }
    return Array.from(buckets, ([label, items]) => ({ label, meetings: items }));
  });

  function time(meeting: RecentMeetingSummary): string {
    return new Intl.DateTimeFormat("ja-JP", { hour: "2-digit", minute: "2-digit" }).format(meeting.occurredAtUnixMs);
  }
</script>

<aside class="meeting-library" aria-label="最近の会議">
  <header>
    <div>
      <strong>最近の会議</strong>
      <span>{meetings.length}件</span>
    </div>
    <div class="library-actions">
      <Button size="icon-sm" variant="ghost" type="button" icon={RefreshCw} aria-label="会議一覧を更新" title="更新" onclick={onRefresh} loading={loading} disabled={busy || loading} />
      <Button size="icon-sm" variant="ghost" type="button" icon={X} aria-label="最近の会議を閉じる" title="閉じる" onclick={onClose} />
    </div>
  </header>

  <div class="meeting-list">
    {#if loading && meetings.length === 0}
      <p class="library-message" role="status">会議を読み込んでいます…</p>
    {:else if meetings.length === 0}
      <p class="library-message">録音または文字起こし済みの会議はまだありません。</p>
    {:else}
      {#each groups as group (group.label)}
        <section class="meeting-group" aria-labelledby={`meeting-group-${group.label}`}>
          <h2 id={`meeting-group-${group.label}`}>{group.label}</h2>
          {#each group.meetings as meeting (meeting.meetingId)}
            <button
              class:selected={meeting.meetingId === selectedMeetingId}
              class="meeting-row"
              type="button"
              onclick={() => onSelect(meeting)}
              disabled={busy || !meeting.audioAvailable}
              title={meeting.audioAvailable ? meeting.fileName : "元の音声ファイルが見つかりません"}
            >
              <strong>{meeting.title}</strong>
              <span>{time(meeting)} · {formatFileSize(meeting.sizeBytes)}</span>
              <span class="meeting-badges">
                <Badge variant="secondary">{meeting.source === "recording" ? "録音" : "取込"}</Badge>
                {#if meeting.transcriptProviders.length > 0}<small>文字起こし済み</small>{/if}
                {#if !meeting.audioAvailable}<small class="missing-audio">音声なし</small>{/if}
              </span>
            </button>
          {/each}
        </section>
      {/each}
    {/if}
  </div>
</aside>

<style>
  .meeting-library {
    display: grid;
    min-width: 0;
    grid-template-rows: auto minmax(0, 1fr);
    border-right: 1px solid var(--border);
    background: var(--background);
  }

  header {
    display: flex;
    min-height: 64px;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 0 14px 0 18px;
    border-bottom: 1px solid var(--border);
  }

  header > div:first-child { display: flex; align-items: baseline; gap: 7px; }
  header strong { font-size: 0.86rem; }
  header span { color: var(--muted-foreground); font-size: 0.72rem; }
  .library-actions { display: flex; gap: 2px; }
  .meeting-list { min-height: 0; overflow-y: auto; }
  .meeting-group { padding: 12px 8px 4px; }
  .meeting-group h2 { margin: 0 10px 7px; color: var(--muted-foreground); font-size: 0.72rem; font-weight: 650; }

  .meeting-row {
    display: grid;
    width: 100%;
    min-width: 0;
    gap: 4px;
    padding: 10px 11px;
    border: 0;
    border-radius: 8px;
    color: var(--foreground);
    background: transparent;
    cursor: pointer;
    font: inherit;
    text-align: left;
  }
  .meeting-row:hover { background: var(--muted); }
  .meeting-row.selected { color: color-mix(in oklch, var(--primary) 86%, var(--foreground)); background: color-mix(in oklch, var(--primary) 9%, var(--background)); }
  .meeting-row:focus-visible { outline: 2px solid var(--ring); outline-offset: -2px; }
  .meeting-row:disabled { cursor: not-allowed; opacity: 0.62; }
  .meeting-row strong { overflow: hidden; font-size: 0.84rem; text-overflow: ellipsis; white-space: nowrap; }
  .meeting-row span { color: var(--muted-foreground); font-size: 0.72rem; }
  .meeting-row small { color: var(--primary); font-size: 0.68rem; font-weight: 650; }
  .meeting-badges { display: flex; align-items: center; gap: 6px; }
  .meeting-badges :global([data-slot="badge"]) { height: 18px; padding: 0 6px; font-size: 0.62rem; }
  .meeting-row .missing-audio { color: var(--destructive); }
  .library-message { margin: 0; padding: 24px 18px; color: var(--muted-foreground); font-size: 0.82rem; line-height: 1.6; }

  @media (max-width: 1040px) {
    .meeting-library { position: absolute; z-index: 10; top: var(--library-top); bottom: 0; left: var(--library-left); width: min(320px, calc(100vw - var(--library-left) - 16px)); box-shadow: 16px 0 34px rgb(0 0 0 / 12%); }
  }
</style>
