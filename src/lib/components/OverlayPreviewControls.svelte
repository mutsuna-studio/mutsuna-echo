<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import PanelTopOpen from "@lucide/svelte/icons/panel-top-open";
  import ScanText from "@lucide/svelte/icons/scan-text";
  import { Button } from "@mutsuna/ui/button";
  import { Select } from "@mutsuna/ui/select";
  import {
    OVERLAY_PREVIEW_OPTIONS,
    type OverlayPreviewMode
  } from "../types/overlay-preview";

  let mode = $state<OverlayPreviewMode>("detection");
  let busy = $state(false);
  let processingMode = $state<"transcription" | "summary">("transcription");
  let processingBusy = $state(false);
  let processingPreviewShown = $state(false);
  let error = $state("");

  const processingPreviewOptions = [
    { value: "transcription", label: "文字起こし中" },
    { value: "summary", label: "会議ノート生成中" }
  ];

  async function showPreview() {
    if (busy) return;
    busy = true;
    error = "";
    try {
      await invoke("show_overlay_preview", { mode });
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      busy = false;
    }
  }

  async function changeMode(value: string) {
    mode = value as OverlayPreviewMode;
    try {
      const current = await invoke<OverlayPreviewMode | null>("get_overlay_preview_mode");
      if (current) await showPreview();
    } catch {
      // プレビューがまだ開かれていない場合は、表示ボタンで開始する。
    }
  }

  async function showProcessingPreview() {
    if (processingBusy) return;
    processingBusy = true;
    error = "";
    try {
      await invoke("show_processing_preview", { mode: processingMode });
      processingPreviewShown = true;
    } catch (cause) {
      processingPreviewShown = false;
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      processingBusy = false;
    }
  }

  async function changeProcessingMode(value: string) {
    if (value !== "transcription" && value !== "summary") return;
    processingMode = value;
    if (processingPreviewShown) await showProcessingPreview();
  }
</script>

<div class="overlay-preview-tools" aria-label="開発用オーバーレイプレビュー">
  <span class="dev-label">DEV</span>
  <Select
    value={mode}
    options={OVERLAY_PREVIEW_OPTIONS}
    onValueChange={changeMode}
    size="sm"
    class="w-24"
    ariaLabel="プレビュー状態"
  />
  <Button
    type="button"
    size="sm"
    variant="outline"
    icon={PanelTopOpen}
    loading={busy}
    onclick={showPreview}
  >
    オーバーレイを確認
  </Button>
  <span class="dev-preview-divider" aria-hidden="true"></span>
  <Select
    value={processingMode}
    options={processingPreviewOptions}
    onValueChange={changeProcessingMode}
    size="sm"
    class="w-32"
    ariaLabel="待機画面の状態"
  />
  <Button
    type="button"
    size="sm"
    variant="outline"
    icon={ScanText}
    loading={processingBusy}
    onclick={showProcessingPreview}
  >
    待機画面を確認
  </Button>
  {#if error}<span class="preview-error" role="alert">{error}</span>{/if}
</div>
