<script lang="ts">
  import { formatTimestamp } from "../format";

  type Props = {
    peaks: number[];
    currentSeconds: number;
    durationSeconds: number;
    loading?: boolean;
    onseek: (seconds: number) => void;
  };

  let { peaks, currentSeconds, durationSeconds, loading = false, onseek }: Props = $props();
  let canvas = $state<HTMLCanvasElement | null>(null);
  let width = $state(0);
  let height = $state(0);
  let themeRevision = $state(0);
  const progress = $derived(durationSeconds > 0 ? Math.min(currentSeconds / durationSeconds, 1) : 0);

  $effect(() => {
    if (!canvas) return;
    const observer = new ResizeObserver(([entry]) => {
      width = entry.contentRect.width;
      height = entry.contentRect.height;
    });
    const themeObserver = new MutationObserver(() => themeRevision += 1);
    observer.observe(canvas);
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "style"] });
    return () => {
      observer.disconnect();
      themeObserver.disconnect();
    };
  });

  $effect(() => {
    themeRevision;
    drawWaveform(canvas, peaks, progress, width, height, loading);
  });

  function handleSeek(event: Event) {
    if (!(event.currentTarget instanceof HTMLInputElement)) return;
    const seconds = Number(event.currentTarget.value);
    if (Number.isFinite(seconds)) onseek(seconds);
  }

  function drawWaveform(
    target: HTMLCanvasElement | null,
    values: number[],
    played: number,
    cssWidth: number,
    cssHeight: number,
    isLoading: boolean,
  ) {
    if (!target || cssWidth <= 0 || cssHeight <= 0) return;
    const scale = window.devicePixelRatio || 1;
    target.width = Math.round(cssWidth * scale);
    target.height = Math.round(cssHeight * scale);
    const context = target.getContext("2d");
    if (!context) return;
    context.scale(scale, scale);
    context.clearRect(0, 0, cssWidth, cssHeight);

    const style = getComputedStyle(target);
    const primary = style.getPropertyValue("--primary").trim() || "#16854a";
    const muted = style.getPropertyValue("--waveform-muted").trim() || "#bdc3bf";
    const center = cssHeight / 2;
    if (isLoading || values.length === 0) {
      context.fillStyle = muted;
      context.fillRect(0, Math.round(center), cssWidth, 1);
      return;
    }

    const gap = 2;
    const barWidth = 1.5;
    const barCount = Math.max(1, Math.min(values.length, Math.floor(cssWidth / (barWidth + gap))));
    const step = cssWidth / barCount;
    const playedX = played * cssWidth;
    for (let index = 0; index < barCount; index += 1) {
      const from = Math.floor(index * values.length / barCount);
      const to = Math.max(from + 1, Math.floor((index + 1) * values.length / barCount));
      let peak = 0;
      for (let sample = from; sample < to; sample += 1) peak = Math.max(peak, values[sample] ?? 0);
      const barHeight = Math.max(3, peak * (cssHeight - 6));
      const x = index * step + (step - barWidth) / 2;
      context.fillStyle = x + barWidth / 2 <= playedX ? primary : muted;
      context.fillRect(x, center - barHeight / 2, barWidth, barHeight);
    }

    if (played > 0 && played < 1) {
      context.fillStyle = primary;
      context.fillRect(Math.max(0, playedX - 1), 2, 2, cssHeight - 4);
    }
  }
</script>

<div class:loading class="waveform-seek">
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
  <input
    type="range"
    min="0"
    max={Math.max(durationSeconds, 0.1)}
    step="0.1"
    value={currentSeconds}
    aria-label="再生位置"
    aria-valuetext={`${formatTimestamp(currentSeconds * 1_000)} / ${formatTimestamp(durationSeconds * 1_000)}`}
    oninput={handleSeek}
  />
</div>

<style>
  .waveform-seek {
    --waveform-muted: color-mix(in oklch, var(--muted-foreground) 38%, var(--background));
    position: relative;
    width: 100%;
    min-width: 60px;
    height: 34px;
    border-radius: 5px;
  }
  .waveform-seek:has(input:focus-visible) {
    outline: 2px solid color-mix(in oklch, var(--primary) 55%, transparent);
    outline-offset: 2px;
  }
  canvas { display: block; width: 100%; height: 100%; }
  input { position: absolute; inset: 0; width: 100%; height: 100%; margin: 0; opacity: 0; cursor: pointer; }
  .loading input { cursor: wait; }
</style>
