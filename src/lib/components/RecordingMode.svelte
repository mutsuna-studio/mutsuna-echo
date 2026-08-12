<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import { Button } from "@mutsuna/ui/button";
  import { scrollbarVisibility } from "@mutsuna/ui/scrollbar";
  import type { SelectedAudioFile } from "../types/transcript";
  import RecordingPanel from "./RecordingPanel.svelte";

  type Props = {
    disabled: boolean;
    busy: boolean;
    onBack: () => void;
    onAudioReady: (audio: SelectedAudioFile) => void;
    onBusyChange: (busy: boolean) => void;
    onMessage: (message: string) => void;
    onError: (message: string) => void;
  };

  let { disabled, busy, onBack, onAudioReady, onBusyChange, onMessage, onError }: Props = $props();
</script>

<section
  class="recording-mode-view mutsuna-scrollbar mutsuna-scrollbar--both-edges"
  use:scrollbarVisibility
>
  <header>
    <span class="desktop-back"><Button size="sm" variant="ghost" type="button" icon={ArrowLeft} onclick={onBack} disabled={busy}>会議一覧へ</Button></span>
    <button class="mobile-back" type="button" onclick={onBack} disabled={busy} aria-label="会議一覧へ戻る"><ArrowLeft aria-hidden="true" /></button>
  </header>
  <RecordingPanel {disabled} {onAudioReady} {onBusyChange} {onMessage} {onError} />
</section>

<style>
  .recording-mode-view { box-sizing: border-box; width: min(820px, calc(100% - 48px)); height: 100%; margin: 0 auto; padding: 24px 0 56px; overflow-y: auto; }
  header { display: flex; justify-content: flex-start; margin-bottom: 14px; }
  .mobile-back { display: none; }

  @media (max-width: 600px) {
    .recording-mode-view { width: 100%; padding: 8px 20px calc(50vw + 18px); }
    header { margin-bottom: 0; }
    .desktop-back { display: none; }
    .mobile-back { display: grid; width: 44px; height: 44px; place-items: center; padding: 0; border: 0; border-radius: 50%; color: var(--foreground); background: transparent; }
    .mobile-back:active:not(:disabled) { background: var(--accent); }
    .mobile-back:disabled { opacity: 0.45; }
    .mobile-back :global(svg) { width: 25px; height: 25px; }
  }
</style>
