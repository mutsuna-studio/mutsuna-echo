<script lang="ts">
  type Source = "microphone" | "system" | "silent";

  type WaveformSample = {
    level: number;
    source: Source;
  };

  type Props = {
    microphoneLevel: number;
    systemLevel: number;
    microphoneEnabled: boolean;
    systemEnabled: boolean;
    elapsedMs: number;
  };

  let {
    microphoneLevel,
    systemLevel,
    microphoneEnabled,
    systemEnabled,
    elapsedMs
  }: Props = $props();

  const sampleCount = 24;
  const silentThreshold = 0.015;
  const colors: Record<Source, string> = {
    microphone: "#66de9c",
    system: "#65c9f3",
    silent: "rgb(209 218 213 / 24%)"
  };

  let canvas: HTMLCanvasElement;
  let samples = $state.raw<WaveformSample[]>(
    Array.from({ length: sampleCount }, () => ({ level: 0, source: "silent" as const }))
  );
  let lastElapsedMs = -1;

  const currentSource = $derived.by<Source>(() => {
    const microphone = microphoneEnabled ? microphoneLevel : 0;
    const system = systemEnabled ? systemLevel : 0;
    if (Math.max(microphone, system) < silentThreshold) return "silent";
    return microphone >= system ? "microphone" : "system";
  });

  const accessibleLabel = $derived.by(() => {
    const microphone = Math.round(Math.max(0, Math.min(1, microphoneLevel)) * 100);
    const system = Math.round(Math.max(0, Math.min(1, systemLevel)) * 100);
    const dominant = currentSource === "microphone"
      ? "マイク優勢"
      : currentSource === "system"
        ? "システム音声優勢"
        : "音声なし";
    return `入力音量の履歴。マイク ${microphone}%、システム音声 ${system}%、${dominant}`;
  });

  function appendSample() {
    const microphone = microphoneEnabled ? Math.max(0, Math.min(1, microphoneLevel)) : 0;
    const system = systemEnabled ? Math.max(0, Math.min(1, systemLevel)) : 0;
    samples = [
      ...samples.slice(-(sampleCount - 1)),
      { level: Math.max(microphone, system), source: currentSource }
    ];
  }

  function draw() {
    if (!canvas) return;
    const bounds = canvas.getBoundingClientRect();
    if (bounds.width === 0 || bounds.height === 0) return;

    const ratio = window.devicePixelRatio || 1;
    const width = Math.round(bounds.width * ratio);
    const height = Math.round(bounds.height * ratio);
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }

    const context = canvas.getContext("2d");
    if (!context) return;
    context.clearRect(0, 0, width, height);

    const gap = 2 * ratio;
    const barWidth = Math.max(1.5 * ratio, (width - gap * (sampleCount - 1)) / sampleCount);
    const center = height / 2;
    const minimumHeight = 3 * ratio;
    const maximumHeight = Math.max(minimumHeight, height - 2 * ratio);

    samples.forEach((sample, index) => {
      const scaledLevel = Math.sqrt(sample.level);
      const barHeight = Math.max(minimumHeight, scaledLevel * maximumHeight);
      const x = index * (barWidth + gap);
      const y = center - barHeight / 2;
      context.fillStyle = colors[sample.source];
      context.beginPath();
      context.roundRect(x, y, barWidth, barHeight, barWidth / 2);
      context.fill();
    });
  }

  $effect(() => {
    if (elapsedMs === lastElapsedMs) return;
    lastElapsedMs = elapsedMs;
    appendSample();
  });

  $effect(() => {
    samples;
    draw();
  });

  $effect(() => {
    const observer = new ResizeObserver(draw);
    observer.observe(canvas);
    return () => observer.disconnect();
  });
</script>

<div class="audio-waveform" role="img" aria-label={accessibleLabel} title={accessibleLabel}>
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
</div>

<style>
  .audio-waveform {
    display: block;
    width: 72px;
    height: 22px;
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
