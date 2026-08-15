<script lang="ts">
  import { onMount } from "svelte";
  import type { Attachment } from "svelte/attachments";

  type Props = {
    peaks?: readonly number[];
    progress?: number;
  };

  let { peaks = [], progress = 0 }: Props = $props();
  let canvas: HTMLCanvasElement;
  const captureCanvas: Attachment<HTMLCanvasElement> = (node) => { canvas = node; };
  let renderedRows = [0, 0, 0];
  let activeLineRow = 0;
  let lastArrivalKey: number | null = null;
  let currentLandingWidth = 0.09;
  let latestProgress = $derived(Math.min(1, Math.max(0, progress)));
  let flightProgressTarget = 0;
  let lastAnimationCycle = -1;
  let waveformProgress = 0;
  let waveformTransitionFrom = 0;
  let waveformTransitionTarget = 0;
  let waveformTransitionStartedAt = 0;

  const fallbackPeaks = [
    0.22, 0.38, 0.62, 0.31, 0.78, 0.48, 0.9, 0.42, 0.7, 0.28, 0.56, 0.82,
    0.36, 0.66, 0.45, 0.74, 0.32, 0.58, 0.86, 0.4, 0.68, 0.3, 0.52, 0.76,
    0.34, 0.64, 0.44, 0.8, 0.38, 0.6, 0.26, 0.7, 0.46, 0.84, 0.36, 0.56,
    0.72, 0.3, 0.62, 0.42, 0.78, 0.34, 0.54, 0.68, 0.28, 0.58, 0.4, 0.74
  ];
  let latestPeaks = $derived.by(() => enhancePeakContrast(samplePeaks(peaks.length > 0 ? peaks : fallbackPeaks, 64)));
  const cycleDuration = 2.4;
  const transferDuration = 0.24;
  const maximumFragmentWidth = 0.09;
  const rowCapacities = [0.76, 0.63, 0.45];
  const emissionPatterns = [
    [0.05, 0.38, 0.71],
    [0.09, 0.42, 0.75],
    [0.07, 0.4, 0.73]
  ];
  const originOffsetPatterns = [
    [-6, -3, 0],
    [-4, -1, -7],
    [-2, -5, 0]
  ];

  onMount(() => {
    const target = canvas;
    const context = target.getContext("2d");
    if (!context) return;
    let primary = "#007c72";
    let foreground = "#1b2928";
    let muted = "#9ab4b2";
    let bitmapWidth = 0;
    let bitmapHeight = 0;
    const refreshColors = () => {
      const style = getComputedStyle(target);
      primary = style.getPropertyValue("--primary").trim() || "#007c72";
      foreground = style.getPropertyValue("--foreground").trim() || "#1b2928";
      muted = style.getPropertyValue("--muted-foreground").trim() || "#9ab4b2";
    };
    refreshColors();
    const themeObserver = new MutationObserver(refreshColors);
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "style"] });
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let frame = 0;
    let animationElapsedSeconds = 0;
    let lastFrameAt = performance.now();

    const draw = (now: number) => {
      const cssWidth = target.clientWidth;
      const cssHeight = target.clientHeight;
      if (cssWidth <= 0 || cssHeight <= 0) {
        frame = window.requestAnimationFrame(draw);
        return;
      }
      const scale = window.devicePixelRatio || 1;
      const nextBitmapWidth = Math.round(cssWidth * scale);
      const nextBitmapHeight = Math.round(cssHeight * scale);
      if (nextBitmapWidth !== bitmapWidth || nextBitmapHeight !== bitmapHeight) {
        bitmapWidth = nextBitmapWidth;
        bitmapHeight = nextBitmapHeight;
        target.width = bitmapWidth;
        target.height = bitmapHeight;
        context.setTransform(scale, 0, 0, scale, 0, 0);
      }
      const values = latestPeaks;
      const actualProgress = latestProgress;
      const frameSeconds = Math.min(0.05, Math.max(0, (now - lastFrameAt) / 1000));
      lastFrameAt = now;
      if (!reduceMotion) animationElapsedSeconds += frameSeconds;
      const elapsedSeconds = reduceMotion ? 0.72 : animationElapsedSeconds;
      if (actualProgress < waveformTransitionTarget) {
        waveformProgress = actualProgress;
        waveformTransitionFrom = actualProgress;
        waveformTransitionTarget = actualProgress;
        waveformTransitionStartedAt = now;
      } else if (actualProgress !== waveformTransitionTarget) {
        waveformTransitionFrom = waveformProgress;
        waveformTransitionTarget = actualProgress;
        waveformTransitionStartedAt = now;
      }
      if (reduceMotion) {
        waveformProgress = actualProgress;
      } else {
        const transition = Math.min(1, Math.max(0, (now - waveformTransitionStartedAt) / 520));
        waveformProgress = mix(waveformTransitionFrom, waveformTransitionTarget, easeInOut(transition));
      }
      const animationCycle = Math.floor(elapsedSeconds / cycleDuration);
      if (animationCycle !== lastAnimationCycle) {
        flightProgressTarget = actualProgress;
        lastAnimationCycle = animationCycle;
      }
      if (reduceMotion) {
        renderedRows = rowsForProgress(actualProgress);
        activeLineRow = firstIncompleteRow(renderedRows);
      }
      context.clearRect(0, 0, cssWidth, cssHeight);
      drawScene(context, values, renderedRows, activeLineRow, waveformProgress, flightProgressTarget, elapsedSeconds, cssWidth, cssHeight, primary, foreground, muted);
      const arrivalKey = getArrivalKey(elapsedSeconds);
      if (lastArrivalKey === null) {
        lastArrivalKey = arrivalKey;
      } else if (arrivalKey > lastArrivalKey) {
        activeLineRow = advanceAfterLanding(renderedRows, activeLineRow, currentLandingWidth);
        lastArrivalKey = arrivalKey;
      }
      frame = window.requestAnimationFrame(draw);
    };
    frame = window.requestAnimationFrame(draw);
    return () => {
      window.cancelAnimationFrame(frame);
      themeObserver.disconnect();
    };
  });

  function drawScene(
    context: CanvasRenderingContext2D,
    values: readonly number[],
    completedRows: readonly number[],
    activeLineRow: number,
    actualProgress: number,
    activeProgressTarget: number,
    elapsedSeconds: number,
    cssWidth: number,
    cssHeight: number,
    primary: string,
    foreground: string,
    muted: string
  ) {
    const left = Math.max(18, cssWidth * 0.06);
    const right = cssWidth - left;
    const span = right - left;
    const waveformY = cssHeight * 0.28;
    const textRows = [cssHeight * 0.67, cssHeight * 0.82, cssHeight * 0.94];
    const barCount = Math.max(24, Math.min(64, Math.floor(span / 8)));
    const step = span / barCount;
    const completedX = left + span * actualProgress;
    const cycle = Math.floor(elapsedSeconds / cycleDuration);
    const cyclePhase = (elapsedSeconds % cycleDuration) / cycleDuration;
    const sweepIndex = Math.min(barCount - 1, Math.floor(cyclePhase * barCount));
    const emissionStarts = emissionPatterns[cycle % emissionPatterns.length];
    const originOffsets = originOffsetPatterns[cycle % originOffsetPatterns.length];
    const processedFront = Math.min(barCount - 1, Math.max(0, Math.floor(activeProgressTarget * barCount)));
    const activeOrigins = originOffsets.map((offset) => Math.min(barCount - 1, Math.max(0, processedFront + offset)));

    context.lineCap = "round";
    for (let index = 0; index < barCount; index += 1) {
      const from = Math.floor(index * values.length / barCount);
      const to = Math.max(from + 1, Math.floor((index + 1) * values.length / barCount));
      const peak = aggregatePeak(values, from, to);
      const x = left + (index + 0.5) * step;
      const height = Math.max(5, peak * cssHeight * 0.35);
      const sweepDistance = Math.abs(index - sweepIndex);
      const active = sweepDistance < 4;
      context.globalAlpha = active ? 1 - sweepDistance * 0.16 : x <= completedX ? 0.62 : 0.28;
      context.strokeStyle = x <= completedX || active ? primary : muted;
      context.lineWidth = sweepDistance < 4 ? 4 : 3;
      context.beginPath();
      context.moveTo(x, waveformY - height / 2);
      context.lineTo(x, waveformY + height / 2);
      context.stroke();
    }

    context.lineWidth = 5;
    for (let row = 0; row < rowCapacities.length; row += 1) {
      const completedInRow = Math.min(rowCapacities[row], Math.max(0, completedRows[row] ?? 0));
      if (completedInRow > 0) {
        context.globalAlpha = 0.68;
        context.strokeStyle = row === 0 ? primary : foreground;
        context.beginPath();
        context.moveTo(left, textRows[row]);
        context.lineTo(left + span * completedInRow, textRows[row]);
        context.stroke();
      }
    }

    const activeRow = Math.min(rowCapacities.length - 1, Math.max(0, activeLineRow));
    const activeStart = left + span * Math.min(rowCapacities[activeRow], Math.max(0, completedRows[activeRow] ?? 0));
    const availableOnRow = left + span * rowCapacities[activeRow] - activeStart;
    const workingWidth = Math.max(0, Math.min(span * maximumFragmentWidth, availableOnRow));
    for (let particle = 0; particle < activeOrigins.length; particle += 1) {
      const start = emissionStarts[particle];
      const transfer = Math.min(1, Math.max(0, (cyclePhase - start) / transferDuration));
      if (transfer <= 0 || workingWidth <= 0) continue;
      const easedTransfer = easeInOut(transfer);
      const barIndex = activeOrigins[particle];
      const from = Math.floor(barIndex * values.length / barCount);
      const to = Math.max(from + 1, Math.floor((barIndex + 1) * values.length / barCount));
      const peak = aggregatePeak(values, from, to);
      const sourceX = left + (barIndex + 0.5) * step;
      const sourceHeight = Math.max(5, peak * cssHeight * 0.35);
      const landingWidth = Math.min(sourceHeight, workingWidth);
      const destinationX = activeStart + landingWidth / 2;
      const destinationY = textRows[activeRow];
      const x = mix(sourceX, destinationX, easedTransfer);
      const arc = Math.sin(easedTransfer * Math.PI) * (12 + particle * 3);
      const y = mix(waveformY, destinationY, easedTransfer) - arc;
      const fadeAfterLanding = transfer > 0.86 ? Math.max(0, (1 - transfer) / 0.14) : 1;
      currentLandingWidth = landingWidth / span;

      context.globalAlpha = 0.9 * fadeAfterLanding;
      context.fillStyle = activeRow === 0 ? primary : foreground;
      context.save();
      context.translate(x, y);
      context.rotate(easedTransfer * Math.PI / 2);
      roundRect(context, -2, -sourceHeight / 2, 4, sourceHeight, 2);
      context.fill();
      context.restore();
    }
    context.globalAlpha = 1;
  }

  function mix(from: number, to: number, amount: number) {
    return from + (to - from) * amount;
  }

  function normalizedPeak(value: number | undefined) {
    return Number.isFinite(value) ? Math.min(1, Math.max(0, value ?? 0)) : 0;
  }

  function samplePeaks(values: readonly number[], maximumCount: number) {
    const count = Math.max(1, Math.min(maximumCount, values.length));
    return Array.from({ length: count }, (_, index) => {
      const from = Math.floor(index * values.length / count);
      const to = Math.max(from + 1, Math.floor((index + 1) * values.length / count));
      return aggregatePeak(values, from, to);
    });
  }

  function aggregatePeak(values: readonly number[], from: number, to: number) {
    let sumOfSquares = 0;
    let sampleCount = 0;
    for (let sample = from; sample < to && sample < values.length; sample += 1) {
      const value = normalizedPeak(values[sample]);
      sumOfSquares += value * value;
      sampleCount += 1;
    }
    return sampleCount > 0 ? Math.sqrt(sumOfSquares / sampleCount) : 0;
  }

  function enhancePeakContrast(values: readonly number[]) {
    if (values.length < 2) return [...values];
    const sorted = [...values].sort((left, right) => left - right);
    const quiet = sorted[Math.floor((sorted.length - 1) * 0.12)] ?? 0;
    const loud = sorted[Math.floor((sorted.length - 1) * 0.88)] ?? 1;
    const range = Math.max(0.025, loud - quiet);
    return values.map((value) => {
      if (value <= 0.01) return 0.08;
      const normalized = Math.min(1, Math.max(0, (value - quiet) / range));
      return 0.22 + Math.pow(normalized, 0.82) * 0.73;
    });
  }

  function getArrivalKey(elapsedSeconds: number) {
    const cycle = Math.floor(elapsedSeconds / cycleDuration);
    const cyclePhase = (elapsedSeconds % cycleDuration) / cycleDuration;
    const starts = emissionPatterns[cycle % emissionPatterns.length];
    const arrivals = starts.filter((start) => cyclePhase >= start + transferDuration).length;
    return cycle * starts.length + arrivals;
  }

  function advanceAfterLanding(rows: number[], activeRow: number, landingWidth: number) {
    const capacity = rowCapacities[activeRow];
    rows[activeRow] = Math.min(capacity, (rows[activeRow] ?? 0) + landingWidth);
    if (rows[activeRow] < capacity - 0.0001) return activeRow;

    const nextRow = (activeRow + 1) % rowCapacities.length;
    rows[nextRow] = 0;
    return nextRow;
  }

  function rowsForProgress(progress: number) {
    const totalCapacity = rowCapacities.reduce((total, capacity) => total + capacity, 0);
    let remaining = progress * totalCapacity;
    return rowCapacities.map((capacity) => {
      const completed = Math.min(capacity, Math.max(0, remaining));
      remaining -= completed;
      return completed;
    });
  }

  function firstIncompleteRow(rows: readonly number[]) {
    const incomplete = rowCapacities.findIndex((capacity, index) => (rows[index] ?? 0) < capacity - 0.0001);
    return incomplete >= 0 ? incomplete : rowCapacities.length - 1;
  }

  function easeInOut(value: number) {
    return value < 0.5 ? 2 * value * value : 1 - Math.pow(-2 * value + 2, 2) / 2;
  }

  function roundRect(context: CanvasRenderingContext2D, x: number, y: number, width: number, height: number, radius: number) {
    context.beginPath();
    if (typeof context.roundRect === "function") {
      context.roundRect(x, y, width, height, radius);
    } else {
      context.rect(x, y, width, height);
    }
  }
</script>

<canvas {@attach captureCanvas} aria-hidden="true"></canvas>

<style>
  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
