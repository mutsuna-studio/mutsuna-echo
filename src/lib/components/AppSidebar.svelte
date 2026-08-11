<script lang="ts">
  import Mic from "@lucide/svelte/icons/mic";
  import Settings from "@lucide/svelte/icons/settings";
  import AudioLines from "@lucide/svelte/icons/audio-lines";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ChartNoAxesColumn from "@lucide/svelte/icons/chart-no-axes-column";
  import Info from "@lucide/svelte/icons/info";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import { Button } from "@mutsuna/ui/button";
  import * as Sidebar from "@mutsuna/ui/sidebar";

  type AppSection = "meetings" | "settings";
  type SettingsPane = "general" | "transcription" | "summary" | "usage";

  type Props = {
    section: AppSection;
    settingsPane: SettingsPane;
    settingsPreview?: boolean;
    recordingBusy: boolean;
    onNavigate: (section: AppSection) => void;
    onSelectSettingsPane: (pane: SettingsPane) => void;
  };

  let {
    section,
    settingsPane,
    settingsPreview = false,
    recordingBusy,
    onNavigate,
    onSelectSettingsPane
  }: Props = $props();
  const sidebar = Sidebar.useSidebar();

  function navigateTo(nextSection: AppSection) {
    onNavigate(nextSection);
    sidebar.setOpenMobile(false);
  }

  function selectSettingsPane(pane: SettingsPane) {
    onSelectSettingsPane(pane);
    sidebar.setOpenMobile(false);
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
        <button class="settings-back" type="button" onclick={() => navigateTo("meetings")}>
          <ArrowLeft aria-hidden="true" /><span>{recordingBusy ? "録音へ戻る" : "録音と会議へ戻る"}</span>
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
      <Button class="new-meeting" size="lg" type="button" icon={Mic} onclick={() => navigateTo("meetings")}>
        新しい録音
      </Button>
      <button class="home-link" type="button" aria-current="page" onclick={() => navigateTo("meetings")}>
        <AudioLines aria-hidden="true" /><span>録音と会議</span>
      </button>

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
  .home-link { display: grid; width: 100%; min-height: 42px; grid-template-columns: 28px minmax(0, 1fr); align-items: center; gap: 9px; padding: 7px 10px; border: 0; border-radius: 7px; color: color-mix(in oklch, var(--primary) 88%, var(--foreground)); background: color-mix(in oklch, var(--primary) 9%, var(--background)); cursor: pointer; font: inherit; font-size: 0.82rem; font-weight: 680; text-align: left; }
  .home-link:hover { background: color-mix(in oklch, var(--primary) 13%, var(--background)); }
  .home-link:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
  .home-link :global(svg) { width: 18px; height: 18px; justify-self: center; color: var(--primary); stroke-width: 1.8; }

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

  footer { margin-top: auto; padding-top: 8px; border-top: 1px solid var(--border); }
  footer button { display: flex; width: 100%; height: 42px; align-items: center; gap: 11px; padding: 0 12px; border: 0; border-radius: 9px; color: var(--muted-foreground); background: transparent; cursor: pointer; font: inherit; font-size: 0.88rem; font-weight: 650; text-align: left; }
  footer button:hover { color: var(--foreground); background: var(--muted); }
  footer button:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
  footer button :global(svg) { width: 18px; height: 18px; flex: none; stroke-width: 1.8; }

  @media (max-width: 780px) { .app-sidebar-content { padding: calc(20px + env(safe-area-inset-top, 0px)) calc(14px + env(safe-area-inset-right, 0px)) calc(18px + env(safe-area-inset-bottom, 0px)) calc(14px + env(safe-area-inset-left, 0px)); } }
</style>
