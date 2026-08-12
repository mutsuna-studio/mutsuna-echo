<script lang="ts">
  import { AdminShellFrame } from "@mutsuna/ui/admin-shell-frame";
  import { ThemeProvider, createTheme } from "@mutsuna/ui/theme";
  import AppSidebar from "../components/AppSidebar.svelte";
  import MeetingHome from "../components/MeetingHome.svelte";
  import type { RecentMeetingSummary } from "../types/recording";

  const echoTheme = createTheme("custom", "oklch(0.527 0.093 185.044)");
  const baselineTime = new Date("2026-08-12T10:00:00+09:00").getTime();
  const meetings: RecentMeetingSummary[] = [
    {
      meetingId: "baseline-product-review",
      title: "プロダクト定例ミーティング",
      fileName: "product-review.m4a",
      sizeBytes: 68_000_000,
      occurredAtUnixMs: baselineTime,
      updatedAtUnixMs: baselineTime,
      audioAvailable: true,
      source: "recording",
      transcriptProviders: ["local"]
    },
    {
      meetingId: "baseline-marketing",
      title: "マーケティング戦略ディスカッション",
      fileName: "marketing-strategy.m4a",
      sizeBytes: 92_000_000,
      occurredAtUnixMs: baselineTime - 18_000_000,
      updatedAtUnixMs: baselineTime - 18_000_000,
      audioAvailable: true,
      source: "recording",
      transcriptProviders: ["elevenlabs"]
    },
    {
      meetingId: "baseline-customer",
      title: "顧客ヒアリング：株式会社サンプル",
      fileName: "customer-interview.m4a",
      sizeBytes: 48_000_000,
      occurredAtUnixMs: baselineTime - 90_000_000,
      updatedAtUnixMs: baselineTime - 90_000_000,
      audioAvailable: true,
      source: "imported",
      transcriptProviders: ["soniox"]
    },
    {
      meetingId: "baseline-weekly",
      title: "週次チームミーティング",
      fileName: "weekly-team.m4a",
      sizeBytes: 53_000_000,
      occurredAtUnixMs: baselineTime - 176_400_000,
      updatedAtUnixMs: baselineTime - 176_400_000,
      audioAvailable: true,
      source: "recording",
      transcriptProviders: []
    },
    {
      meetingId: "baseline-feature-review",
      title: "新機能レビュー",
      fileName: "feature-review.m4a",
      sizeBytes: 77_000_000,
      occurredAtUnixMs: baselineTime - 259_200_000,
      updatedAtUnixMs: baselineTime - 259_200_000,
      audioAvailable: true,
      source: "recording",
      transcriptProviders: ["local"]
    }
  ];

  const noop = () => {};
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

    <div class="app-shell-content">
      <div class="app-content">
        <div class="mobile-home-container">
          <MeetingHome
            {meetings}
            loading={false}
            busy={false}
            recordingDisabled={false}
            recordingBusy={false}
            recordingPreview
            selecting={false}
            onSelectMeeting={noop}
            onSelectFile={noop}
            onAudioReady={noop}
            onRecordingBusyChange={noop}
            onMessage={noop}
            onError={noop}
          />
        </div>
      </div>
    </div>
  </AdminShellFrame>
</ThemeProvider>
