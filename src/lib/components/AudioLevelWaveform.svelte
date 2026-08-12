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
    hero?: boolean;
    microphoneSpectrum?: number[];
    systemSpectrum?: number[];
  };

  let {
    microphoneLevel,
    systemLevel,
    microphoneEnabled,
    systemEnabled,
    elapsedMs,
    hero = false,
    microphoneSpectrum = [],
    systemSpectrum = []
  }: Props = $props();

  const sampleCount = 24;
  const spectrumBandCount = 24;
  const silentThreshold = 0.015;
  const spectrumFrameIntervalMs = 1000 / 30;
  const spectrumInterpolationMs = 55;
  const spectrumSettleThreshold = 0.002;
  const spectrumVisualFloor = 0.06;
  const spectrumPeakExponent = 0.72;
  const spectrumHeightExponent = 1.5;
  let canvas: HTMLCanvasElement;
  let samples = $state.raw<WaveformSample[]>(
    Array.from({ length: sampleCount }, () => ({ level: 0, source: "silent" as const }))
  );
  let lastSampleSignature = "";
  const displayedMicrophoneSpectrum = Array<number>(spectrumBandCount).fill(0);
  const displayedSystemSpectrum = Array<number>(spectrumBandCount).fill(0);
  const targetMicrophoneSpectrum = Array<number>(spectrumBandCount).fill(0);
  const targetSystemSpectrum = Array<number>(spectrumBandCount).fill(0);
  let spectrumAnimationFrame: number | null = null;
  let lastSpectrumFrameTime = 0;
  let reduceSpectrumMotion = false;

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
    const visualization = hero ? "現在の周波数分布" : "入力音量の履歴";
    return `${visualization}。マイク ${microphone}%、システム音声 ${system}%、${dominant}`;
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

    const style = getComputedStyle(canvas);
    const colors: Record<Source, string> = {
      microphone: style.getPropertyValue("--audio-microphone").trim() || "#007c72",
      system: style.getPropertyValue("--audio-system").trim() || "#31b7d9",
      silent: style.getPropertyValue("--audio-silent").trim() || "#d4e2e2"
    };

    if (hero) {
      drawSpectrum(context, width, height, ratio, colors, {
        microphoneOuter: style.getPropertyValue("--audio-microphone-gradient-outer").trim()
          || "oklch(0.61 0.095 202)",
        systemOuter: style.getPropertyValue("--audio-system-gradient-outer").trim()
          || "oklch(0.62 0.15 247)"
      });
      return;
    }

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

  function drawSpectrum(
    context: CanvasRenderingContext2D,
    width: number,
    height: number,
    ratio: number,
    colors: Record<Source, string>,
    gradientColors: {
      microphoneOuter: string;
      systemOuter: string;
    }
  ) {
    const center = width / 2;
    const centerGap = 48 * ratio;
    const edgeInset = 4 * ratio;
    const gap = 2 * ratio;
    const availableHalf = Math.max(0, center - centerGap - edgeInset);
    const barWidth = Math.max(
      1.5 * ratio,
      (availableHalf - gap * (spectrumBandCount - 1)) / spectrumBandCount
    );
    const minimumHeight = 3 * ratio;
    const maximumHeight = Math.max(minimumHeight, height - 4 * ratio);
    const microphoneGradient = createVerticalGradient(
      colors.microphone,
      gradientColors.microphoneOuter
    );
    const systemGradient = createVerticalGradient(
      colors.system,
      gradientColors.systemOuter
    );
    let microphonePeak = 0;
    let systemPeak = 0;

    for (let band = 0; band < spectrumBandCount; band += 1) {
      microphonePeak = Math.max(
        microphonePeak,
        toVisibleSpectrumLevel(displayedMicrophoneSpectrum[band])
      );
      systemPeak = Math.max(
        systemPeak,
        toVisibleSpectrumLevel(displayedSystemSpectrum[band])
      );
    }

    for (let band = 0; band < spectrumBandCount; band += 1) {
      const microphoneLevel = displayedMicrophoneSpectrum[band];
      const systemLevel = displayedSystemSpectrum[band];
      const microphoneX = center - centerGap - barWidth - band * (barWidth + gap);
      const systemX = center + centerGap + band * (barWidth + gap);
      drawSpectrumBar(
        context,
        microphoneX,
        microphoneLevel,
        microphonePeak,
        microphoneGradient
      );
      drawSpectrumBar(context, systemX, systemLevel, systemPeak, systemGradient);
    }

    function toVisibleSpectrumLevel(level: number): number {
      const normalized = Math.max(0, Math.min(1, level));
      return Math.max(0, (normalized - spectrumVisualFloor) / (1 - spectrumVisualFloor));
    }

    function createVerticalGradient(
      centerColor: string,
      outerColor: string
    ): CanvasGradient {
      const gradient = context.createLinearGradient(0, 2 * ratio, 0, height - 2 * ratio);
      gradient.addColorStop(0, outerColor);
      gradient.addColorStop(0.5, centerColor);
      gradient.addColorStop(1, outerColor);
      return gradient;
    }

    function drawSpectrumBar(
      target: CanvasRenderingContext2D,
      x: number,
      level: number,
      peakLevel: number,
      activeGradient: CanvasGradient
    ) {
      const normalized = Math.max(0, Math.min(1, level));
      const visibleLevel = toVisibleSpectrumLevel(normalized);
      const relativeLevel = peakLevel > 0 ? visibleLevel / peakLevel : 0;
      const heightLevel = Math.pow(peakLevel, spectrumPeakExponent)
        * Math.pow(relativeLevel, spectrumHeightExponent);
      const barHeight = Math.max(
        minimumHeight,
        minimumHeight + heightLevel * (maximumHeight - minimumHeight)
      );
      target.fillStyle = normalized > silentThreshold ? activeGradient : colors.silent;
      target.beginPath();
      target.roundRect(x, (height - barHeight) / 2, barWidth, barHeight, barWidth / 2);
      target.fill();
    }
  }

  function normalizedSpectrum(values: number[]): number[] {
    return Array.from({ length: spectrumBandCount }, (_, index) => {
      const value = values[index] ?? 0;
      return Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
    });
  }

  function updateSpectrumTargets(): boolean {
    const nextMicrophone = normalizedSpectrum(microphoneEnabled ? microphoneSpectrum : []);
    const nextSystem = normalizedSpectrum(systemEnabled ? systemSpectrum : []);
    let changed = false;

    for (let band = 0; band < spectrumBandCount; band += 1) {
      if (
        nextMicrophone[band] !== targetMicrophoneSpectrum[band]
        || nextSystem[band] !== targetSystemSpectrum[band]
      ) {
        changed = true;
      }
      targetMicrophoneSpectrum[band] = nextMicrophone[band];
      targetSystemSpectrum[band] = nextSystem[band];
    }

    return changed;
  }

  function snapSpectrumToTargets() {
    for (let band = 0; band < spectrumBandCount; band += 1) {
      displayedMicrophoneSpectrum[band] = targetMicrophoneSpectrum[band];
      displayedSystemSpectrum[band] = targetSystemSpectrum[band];
    }
  }

  function spectrumNeedsAnimation(): boolean {
    for (let band = 0; band < spectrumBandCount; band += 1) {
      if (
        Math.abs(targetMicrophoneSpectrum[band] - displayedMicrophoneSpectrum[band])
          > spectrumSettleThreshold
        || Math.abs(targetSystemSpectrum[band] - displayedSystemSpectrum[band])
          > spectrumSettleThreshold
      ) {
        return true;
      }
    }
    return false;
  }

  function scheduleSpectrumAnimation() {
    if (spectrumAnimationFrame !== null || !spectrumNeedsAnimation()) return;
    spectrumAnimationFrame = requestAnimationFrame(animateSpectrum);
  }

  function animateSpectrum(timestamp: number) {
    spectrumAnimationFrame = null;

    if (
      lastSpectrumFrameTime !== 0
      && timestamp - lastSpectrumFrameTime < spectrumFrameIntervalMs
    ) {
      spectrumAnimationFrame = requestAnimationFrame(animateSpectrum);
      return;
    }

    const elapsed = lastSpectrumFrameTime === 0
      ? spectrumFrameIntervalMs
      : Math.min(50, timestamp - lastSpectrumFrameTime);
    lastSpectrumFrameTime = timestamp;
    const blend = 1 - Math.exp(-elapsed / spectrumInterpolationMs);

    for (let band = 0; band < spectrumBandCount; band += 1) {
      displayedMicrophoneSpectrum[band] += (
        targetMicrophoneSpectrum[band] - displayedMicrophoneSpectrum[band]
      ) * blend;
      displayedSystemSpectrum[band] += (
        targetSystemSpectrum[band] - displayedSystemSpectrum[band]
      ) * blend;
    }

    if (!spectrumNeedsAnimation()) {
      snapSpectrumToTargets();
      lastSpectrumFrameTime = 0;
    }

    draw();
    scheduleSpectrumAnimation();
  }

  function cancelSpectrumAnimation() {
    if (spectrumAnimationFrame !== null) {
      cancelAnimationFrame(spectrumAnimationFrame);
      spectrumAnimationFrame = null;
    }
    lastSpectrumFrameTime = 0;
  }

  $effect(() => {
    if (hero) return;
    const signature = `${elapsedMs}:${microphoneEnabled}:${systemEnabled}`;
    if (signature === lastSampleSignature) return;
    lastSampleSignature = signature;
    appendSample();
  });

  $effect(() => {
    samples;
    if (hero) return;
    draw();
  });

  $effect(() => {
    if (!hero || !updateSpectrumTargets()) return;

    if (reduceSpectrumMotion) {
      cancelSpectrumAnimation();
      snapSpectrumToTargets();
      draw();
      return;
    }

    scheduleSpectrumAnimation();
  });

  $effect(() => {
    if (!hero) return;
    const motionPreference = window.matchMedia("(prefers-reduced-motion: reduce)");

    const applyMotionPreference = () => {
      reduceSpectrumMotion = motionPreference.matches;
      if (reduceSpectrumMotion) {
        cancelSpectrumAnimation();
        snapSpectrumToTargets();
        draw();
      } else {
        scheduleSpectrumAnimation();
      }
    };

    applyMotionPreference();
    motionPreference.addEventListener("change", applyMotionPreference);

    return () => {
      motionPreference.removeEventListener("change", applyMotionPreference);
      cancelSpectrumAnimation();
    };
  });

  $effect(() => {
    const observer = new ResizeObserver(draw);
    observer.observe(canvas);
    return () => observer.disconnect();
  });
</script>

<div class:hero class="audio-waveform" role="img" aria-label={accessibleLabel} title={accessibleLabel}>
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
</div>

<style>
  .audio-waveform {
    display: block;
    width: 72px;
    height: 22px;
  }

  .audio-waveform.hero {
    width: 100%;
    height: 84px;
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
