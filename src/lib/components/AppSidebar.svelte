<script lang="ts">
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import Mic from "@lucide/svelte/icons/mic";
  import Settings from "@lucide/svelte/icons/settings";
  import AudioLines from "@lucide/svelte/icons/audio-lines";
  import { Button } from "@mutsuna/ui/button";
  import * as Sidebar from "@mutsuna/ui/sidebar";

  type AppSection = "meetings" | "new" | "settings";

  type Props = {
    section: AppSection;
    onNavigate: (section: AppSection) => void;
  };

  let { section, onNavigate }: Props = $props();
  const sidebar = Sidebar.useSidebar();

  function navigateTo(nextSection: AppSection) {
    onNavigate(nextSection);
    sidebar.setOpenMobile(false);
  }
</script>

<Sidebar.Root class="app-sidebar" collapsible="offcanvas">
  <div class="app-sidebar-content" aria-label="メインナビゲーション">
    <div class="brand" aria-label="Mutsuna Echo">
      <span class="brand-mark"><AudioLines aria-hidden="true" /></span>
      <strong>Mutsuna Echo</strong>
    </div>

    <Button class="new-meeting" size="lg" type="button" icon={Mic} onclick={() => navigateTo("new")}>
      新しい録音
    </Button>

    <nav>
      <button class:active={section === "meetings"} type="button" aria-current={section === "meetings" ? "page" : undefined} onclick={() => navigateTo("meetings")}>
        <CalendarDays aria-hidden="true" /><span>会議</span>
      </button>
      <button class:active={section === "settings"} type="button" aria-current={section === "settings" ? "page" : undefined} onclick={() => navigateTo("settings")}>
        <Settings aria-hidden="true" /><span>設定</span>
      </button>
    </nav>
  </div>
</Sidebar.Root>

<style>
  .app-sidebar-content {
    display: flex;
    height: 100%;
    min-width: 0;
    flex-direction: column;
    gap: 26px;
    padding: 24px 14px 18px;
    border-right: 1px solid var(--border);
    background: color-mix(in oklch, var(--primary) 3%, var(--background));
  }

  .brand {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 10px;
    padding: 0 8px;
  }

  .brand strong {
    overflow: hidden;
    font-size: 0.98rem;
    letter-spacing: -0.02em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .brand-mark {
    display: grid;
    width: 30px;
    height: 30px;
    flex: none;
    place-items: center;
    color: var(--primary);
  }

  .brand-mark :global(svg) { width: 28px; height: 28px; stroke-width: 1.8; }
  :global(.new-meeting) { width: 100%; justify-content: center; }

  nav { display: grid; gap: 5px; }

  nav button {
    display: flex;
    width: 100%;
    height: 44px;
    align-items: center;
    gap: 11px;
    padding: 0 12px;
    border: 0;
    border-radius: 9px;
    color: var(--muted-foreground);
    background: transparent;
    cursor: pointer;
    font: inherit;
    font-size: 0.88rem;
    font-weight: 650;
    text-align: left;
  }

  nav button:hover { color: var(--foreground); background: var(--muted); }
  nav button.active { color: var(--primary); background: color-mix(in oklch, var(--primary) 9%, var(--background)); }
  nav button:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
  nav button :global(svg) { width: 18px; height: 18px; flex: none; stroke-width: 1.8; }

  @media (max-width: 780px) {
    .app-sidebar-content { padding: 20px 14px 18px; }
  }
</style>
