<script lang="ts">
  import Mic from "@lucide/svelte/icons/mic";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Settings from "@lucide/svelte/icons/settings";
  import AudioLines from "@lucide/svelte/icons/audio-lines";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ChartNoAxesColumn from "@lucide/svelte/icons/chart-no-axes-column";
  import Info from "@lucide/svelte/icons/info";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import * as Sidebar from "@mutsuna/ui/sidebar";
  import { formatFileSize } from "../format";
  import type { RecentMeetingSummary } from "../types/recording";

  type AppSection = "meetings" | "recording" | "settings";
  type SettingsPane = "general" | "transcription" | "summary" | "usage";
  type MeetingGroup = { label: string; meetings: RecentMeetingSummary[] };

  type Props = {
    section: AppSection;
    meetings: readonly RecentMeetingSummary[];
    selectedMeetingId: string | null;
    settingsPane: SettingsPane;
    settingsPreview?: boolean;
    loading: boolean;
    busy: boolean;
    allowMeetingNavigation?: boolean;
    recordingBusy: boolean;
    onNavigate: (section: AppSection) => void;
    onSelectSettingsPane: (pane: SettingsPane) => void;
    onSelectMeeting: (meeting: RecentMeetingSummary) => void;
    onRefreshMeetings: () => void;
  };

  let {
    section,
    meetings,
    selectedMeetingId,
    settingsPane,
    settingsPreview = false,
    loading,
    busy,
    allowMeetingNavigation = false,
    recordingBusy,
    onNavigate,
    onSelectSettingsPane,
    onSelectMeeting,
    onRefreshMeetings
  }: Props = $props();
  const sidebar = Sidebar.useSidebar();

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
      const items = buckets.get(label) ?? [];
      items.push(meeting);
      buckets.set(label, items);
    }
    return Array.from(buckets, ([label, items]) => ({ label, meetings: items }));
  });

  function navigateTo(nextSection: AppSection) {
    onNavigate(nextSection);
    sidebar.setOpenMobile(false);
  }

  function selectMeeting(meeting: RecentMeetingSummary) {
    onSelectMeeting(meeting);
    sidebar.setOpenMobile(false);
  }

  function selectSettingsPane(pane: SettingsPane) {
    onSelectSettingsPane(pane);
    sidebar.setOpenMobile(false);
  }

  function time(meeting: RecentMeetingSummary): string {
    return new Intl.DateTimeFormat("ja-JP", { hour: "2-digit", minute: "2-digit" }).format(meeting.occurredAtUnixMs);
  }
</script>

<Sidebar.Root class="app-sidebar" collapsible="offcanvas">
  <div class="app-sidebar-content" aria-label="メインナビゲーション">
    <div class="brand" aria-label="Mutsuna Echo">
      <span class="brand-mark"><AudioLines aria-hidden="true" /></span>
      <strong>Mutsuna Echo</strong>
    </div>

    {#if section === "settings"}
      {#if !settingsPreview}
        <button class="settings-back" type="button" onclick={() => navigateTo(recordingBusy ? "recording" : "meetings")}>
          <ArrowLeft aria-hidden="true" /><span>{recordingBusy ? "録音画面へ戻る" : "会議へ戻る"}</span>
        </button>
      {/if}

      <section class="settings-menu" aria-labelledby="settings-menu-heading">
        <h2 id="settings-menu-heading">設定</h2>
        <nav aria-label="設定カテゴリ">
          {#if !settingsPreview}
            <button class:active={settingsPane === "general"} type="button" aria-current={settingsPane === "general" ? "page" : undefined} onclick={() => selectSettingsPane("general")}>
              <Info aria-hidden="true" />
              <span><strong>一般</strong></span>
            </button>
            <button class:active={settingsPane === "transcription"} type="button" aria-current={settingsPane === "transcription" ? "page" : undefined} onclick={() => selectSettingsPane("transcription")}>
              <AudioLines aria-hidden="true" />
              <span><strong>文字起こし</strong></span>
            </button>
          {/if}
          <button class:active={settingsPane === "summary"} type="button" aria-current={settingsPane === "summary" ? "page" : undefined} onclick={() => selectSettingsPane("summary")}>
            <Sparkles aria-hidden="true" />
            <span><strong>AI会議ノート</strong></span>
          </button>
          {#if !settingsPreview}
            <button class:active={settingsPane === "usage"} type="button" aria-current={settingsPane === "usage" ? "page" : undefined} onclick={() => selectSettingsPane("usage")}>
              <ChartNoAxesColumn aria-hidden="true" />
              <span><strong>利用状況</strong></span>
            </button>
          {/if}
        </nav>
      </section>
    {:else}
      <Button class="new-meeting" size="lg" type="button" icon={Mic} onclick={() => navigateTo("recording")}>
        新しい録音
      </Button>

      <section class="recent-meetings" aria-labelledby="recent-meetings-heading">
        <header>
          <div>
            <strong id="recent-meetings-heading">最近の会議</strong>
            <span>{meetings.length}件</span>
          </div>
          <Button size="icon-sm" variant="ghost" type="button" icon={RefreshCw} aria-label="会議一覧を更新" title="更新" onclick={onRefreshMeetings} loading={loading} disabled={busy || loading} />
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
                    onclick={() => selectMeeting(meeting)}
                    disabled={busy && !allowMeetingNavigation}
                    title={meeting.audioAvailable ? meeting.fileName : `${meeting.title}（音声なし）`}
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
      </section>

      <footer>
        <button type="button" onclick={() => navigateTo("settings")}>
          <Settings aria-hidden="true" /><span>設定</span>
        </button>
      </footer>
    {/if}
  </div>
</Sidebar.Root>

<style>
  .app-sidebar-content { display: flex; height: 100%; min-width: 0; flex-direction: column; gap: 16px; padding: calc(20px + env(safe-area-inset-top, 0px)) calc(12px + env(safe-area-inset-right, 0px)) calc(16px + env(safe-area-inset-bottom, 0px)) calc(12px + env(safe-area-inset-left, 0px)); border-right: 1px solid var(--border); background: color-mix(in oklch, var(--muted) 44%, var(--background)); }
  .brand { display: flex; min-width: 0; align-items: center; gap: 10px; padding: 0 8px; }
  .brand strong { overflow: hidden; font-size: 0.98rem; letter-spacing: -0.02em; text-overflow: ellipsis; white-space: nowrap; }
  .brand-mark { display: grid; width: 30px; height: 30px; flex: none; place-items: center; color: var(--primary); }
  .brand-mark :global(svg) { width: 28px; height: 28px; stroke-width: 1.8; }
  :global(.new-meeting) { width: 100%; justify-content: center; }

  .settings-back { display: flex; width: 100%; height: 38px; align-items: center; gap: 9px; padding: 0 10px; border: 0; border-radius: 8px; color: var(--muted-foreground); background: transparent; cursor: pointer; font: inherit; font-size: 0.76rem; font-weight: 620; text-align: left; }
  .settings-back:hover { color: var(--foreground); background: var(--muted); }
  .settings-back:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
  .settings-back :global(svg) { width: 16px; height: 16px; stroke-width: 1.8; }

  .settings-menu { min-height: 0; flex: 1; padding-top: 4px; }
  .settings-menu > h2 { margin: 0 10px 9px; color: var(--muted-foreground); font-size: 0.67rem; font-weight: 750; letter-spacing: 0.08em; }
  .settings-menu nav { display: grid; gap: 4px; }
  .settings-menu nav button { display: grid; width: 100%; min-width: 0; min-height: 40px; grid-template-columns: 28px minmax(0, 1fr); align-items: center; gap: 9px; padding: 7px 10px; border: 0; border-radius: 7px; color: var(--foreground); background: transparent; cursor: pointer; font: inherit; text-align: left; }
  .settings-menu nav button:hover { background: var(--muted); }
  .settings-menu nav button.active { color: color-mix(in oklch, var(--primary) 88%, var(--foreground)); background: color-mix(in oklch, var(--primary) 11%, var(--background)); }
  .settings-menu nav button:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
  .settings-menu nav button > :global(svg) { width: 17px; height: 17px; justify-self: center; color: var(--muted-foreground); stroke-width: 1.8; }
  .settings-menu nav button.active > :global(svg) { color: var(--primary); }
  .settings-menu nav button span { display: grid; min-width: 0; gap: 2px; }
  .settings-menu nav button strong { overflow: hidden; font-size: 0.8rem; font-weight: 680; text-overflow: ellipsis; white-space: nowrap; }

  .recent-meetings { display: grid; min-height: 0; flex: 1; grid-template-rows: auto minmax(0, 1fr); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 0 7px 8px; }
  header > div { display: flex; min-width: 0; align-items: baseline; gap: 6px; }
  header strong { font-size: 0.8rem; }
  header span { color: var(--muted-foreground); font-size: 0.68rem; }
  .meeting-list { min-height: 0; overflow-y: auto; }
  .meeting-group { padding: 8px 0 3px; }
  .meeting-group h2 { margin: 0 8px 5px; color: var(--muted-foreground); font-size: 0.68rem; font-weight: 650; }
  .meeting-row { display: grid; width: 100%; min-width: 0; gap: 3px; padding: 9px 10px; border: 0; border-radius: 8px; color: var(--foreground); background: transparent; cursor: pointer; font: inherit; text-align: left; }
  .meeting-row:hover { background: var(--muted); }
  .meeting-row.selected { color: color-mix(in oklch, var(--primary) 86%, var(--foreground)); background: color-mix(in oklch, var(--primary) 9%, var(--background)); }
  .meeting-row:focus-visible { outline: 2px solid var(--ring); outline-offset: -2px; }
  .meeting-row:disabled { cursor: not-allowed; opacity: 0.62; }
  .meeting-row strong { overflow: hidden; font-size: 0.79rem; text-overflow: ellipsis; white-space: nowrap; }
  .meeting-row > span { color: var(--muted-foreground); font-size: 0.67rem; }
  .meeting-row small { color: var(--primary); font-size: 0.63rem; font-weight: 650; }
  .meeting-badges { display: flex; align-items: center; gap: 5px; }
  .meeting-badges :global([data-slot="badge"]) { height: 17px; padding: 0 5px; font-size: 0.58rem; }
  .meeting-row .missing-audio { color: var(--destructive); }
  .library-message { margin: 0; padding: 16px 8px; color: var(--muted-foreground); font-size: 0.75rem; line-height: 1.55; }

  footer { padding-top: 8px; border-top: 1px solid var(--border); }
  footer button { display: flex; width: 100%; height: 42px; align-items: center; gap: 11px; padding: 0 12px; border: 0; border-radius: 9px; color: var(--muted-foreground); background: transparent; cursor: pointer; font: inherit; font-size: 0.88rem; font-weight: 650; text-align: left; }
  footer button:hover { color: var(--foreground); background: var(--muted); }
  footer button:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
  footer button :global(svg) { width: 18px; height: 18px; flex: none; stroke-width: 1.8; }

  @media (max-width: 780px) { .app-sidebar-content { padding: calc(20px + env(safe-area-inset-top, 0px)) calc(14px + env(safe-area-inset-right, 0px)) calc(18px + env(safe-area-inset-bottom, 0px)) calc(14px + env(safe-area-inset-left, 0px)); } }
</style>
