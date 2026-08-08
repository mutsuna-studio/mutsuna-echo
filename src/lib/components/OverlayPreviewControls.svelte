<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import PanelTopOpen from "@lucide/svelte/icons/panel-top-open";
  import { Button } from "@mutsuna/ui/button";
  import { Select } from "@mutsuna/ui/select";
  import {
    OVERLAY_PREVIEW_OPTIONS,
    type OverlayPreviewMode
  } from "../types/overlay-preview";

  let mode = $state<OverlayPreviewMode>("detection");
  let busy = $state(false);
  let error = $state("");

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
</script>

<div class="overlay-preview-tools" aria-label="開発用オーバーレイプレビュー">
  <span class="dev-label">DEV</span>
  <Select
    value={mode}
    options={OVERLAY_PREVIEW_OPTIONS}
    onValueChange={changeMode}
    searchable
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
  {#if error}<span class="preview-error" role="alert">{error}</span>{/if}
</div>
