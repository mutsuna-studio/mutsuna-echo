<script lang="ts">
  import Sun from "@lucide/svelte/icons/sun";
  import { Switch } from "@mutsuna/ui/switch";
  import { invoke } from "@tauri-apps/api/core";

  type ProcessingPowerSettings = {
    keepDisplayOn: boolean;
  };

  type Props = {
    disabled?: boolean;
    onError: (message: string) => void;
  };

  let { disabled = false, onError }: Props = $props();
  let keepDisplayOn = $state(false);
  let loading = $state(true);
  let saving = $state(false);

  $effect(() => {
    let cancelled = false;
    void invoke<ProcessingPowerSettings>("get_processing_power_settings")
      .then((settings) => {
        if (!cancelled) keepDisplayOn = settings.keepDisplayOn;
      })
      .catch((error) => {
        if (!cancelled) onError(typeof error === "string" ? error : "画面点灯設定を読み込めませんでした。");
      })
      .finally(() => {
        if (!cancelled) loading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  async function changeKeepDisplayOn(value: boolean) {
    if (loading || saving || disabled) return;
    const previous = keepDisplayOn;
    keepDisplayOn = value;
    saving = true;
    try {
      const settings = await invoke<ProcessingPowerSettings>("set_processing_power_settings", {
        settings: { keepDisplayOn: value }
      });
      keepDisplayOn = settings.keepDisplayOn;
    } catch (error) {
      keepDisplayOn = previous;
      onError(typeof error === "string" ? error : "画面点灯設定を変更できませんでした。");
    } finally {
      saving = false;
    }
  }
</script>

<section class="power-settings" aria-labelledby="power-settings-title" aria-busy={loading || saving}>
  <div class="power-heading">
    <span class="power-icon" aria-hidden="true"><Sun /></span>
    <span>
      <strong id="power-settings-title">処理中は画面を点灯したままにする</strong>
      <small>録音、文字起こし、会議ノート生成が完了するまで自動消灯を防ぎます。オフでも処理は継続します。</small>
    </span>
  </div>
  <Switch
    checked={keepDisplayOn}
    onCheckedChange={changeKeepDisplayOn}
    disabled={disabled || loading || saving}
    aria-label="処理中は画面を点灯したままにする"
  />
</section>

<style>
  .power-settings {
    display: flex;
    min-height: 76px;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 14px 2px;
    border-bottom: 1px solid var(--border);
  }

  .power-heading { display: flex; min-width: 0; align-items: flex-start; gap: 12px; }
  .power-heading > span:last-child { display: grid; min-width: 0; gap: 4px; }
  .power-heading strong { font-size: 0.9rem; font-weight: 680; line-height: 1.35; }
  .power-heading small { max-width: 620px; color: var(--muted-foreground); font-size: 0.72rem; line-height: 1.55; }
  .power-icon { display: grid; width: 34px; height: 34px; flex: none; place-items: center; border-radius: 9px; color: var(--primary); background: color-mix(in oklch, var(--primary) 10%, var(--background)); }
  .power-icon :global(svg) { width: 17px; height: 17px; stroke-width: 1.8; }

  @media (max-width: 520px) {
    .power-settings { min-height: 0; align-items: flex-start; gap: 16px; padding: 15px 2px; }
    .power-heading { gap: 11px; }
  }
</style>
