<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import { Button } from "@mutsuna/ui/button";
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

<section class="recording-mode-view">
  <header>
    <Button size="sm" variant="ghost" type="button" icon={ArrowLeft} onclick={onBack} disabled={busy}>会議一覧へ</Button>
  </header>
  <RecordingPanel {disabled} {onAudioReady} {onBusyChange} {onMessage} {onError} />
</section>

<style>
  .recording-mode-view { box-sizing: border-box; width: min(820px, calc(100% - 48px)); height: 100%; margin: 0 auto; padding: 24px 0 56px; overflow-y: auto; }
  header { display: flex; justify-content: flex-start; margin-bottom: 14px; }

  @media (max-width: 600px) {
    .recording-mode-view { width: 100%; padding: 10px 16px calc(24px + env(safe-area-inset-bottom, 0px)); }
  }
</style>
