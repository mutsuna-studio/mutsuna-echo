<script lang="ts">
  import { onMount } from "svelte";
  import type { Attachment } from "svelte/attachments";

  type Props = {
    sourceLines?: readonly string[];
    progress?: number;
    determinate?: boolean;
  };

  type TextBitmap = {
    canvas: HTMLCanvasElement;
    width: number;
    height: number;
  };

  type SessionLayout = {
    widths: readonly number[];
    offsets: readonly number[];
    cycleWidth: number;
    scrollSpeed: number;
  };

  let { sourceLines = [], progress = 0, determinate = false }: Props = $props();
  let root: HTMLDivElement;
  let streamOverlay: HTMLCanvasElement;
  let overlay: HTMLCanvasElement;

  const captureRoot: Attachment<HTMLDivElement> = (node) => { root = node; };
  const captureStreamOverlay: Attachment<HTMLCanvasElement> = (node) => { streamOverlay = node; };
  const captureOverlay: Attachment<HTMLCanvasElement> = (node) => { overlay = node; };

  const scrollStepSeconds = 1.8;
  const extractionDurationSeconds = 3;
  const extractionIntervalSeconds = scrollStepSeconds * 2;
  const fallbackLines = [
    "次回リリースの対象範囲を確認しました",
    "担当者と確認期限を決めて進めます",
    "未確定の項目は次回までに調査します",
    "利用者への案内方法もあわせて検討します",
    "実装コストを確認して優先順位を決定します"
  ];
  const streamLines = $derived.by(() => {
    const normalized = sourceLines
      .map((line) => line.replace(/\s+/g, " ").trim())
      .filter(Boolean);
    return (normalized.length > 0 ? normalized : fallbackLines).slice(0, 16);
  });
  const displayedProgress = $derived(normalizedProgress(progress));

  onMount(() => {
    const streamContext = streamOverlay.getContext("2d", { alpha: true });
    const overlayContext = overlay.getContext("2d", { alpha: true });
    if (!streamContext || !overlayContext) return;

    let primary = "#007c72";
    let mutedForeground = "#829b99";
    let fontFamily = "sans-serif";
    let cssWidth = 0;
    let cssHeight = 0;
    let slotWidth = 0;
    let fittedLines: string[] = [];
    let sessionLayout: SessionLayout = { widths: [], offsets: [], cycleWidth: 1, scrollSpeed: 1 };
    let frame = 0;
    let lastExtractionCycle = -1;
    let landingRatio = clampedLandingProgress(progress);
    let textCache: Record<string, TextBitmap> = Object.create(null) as Record<string, TextBitmap>;
    let textCacheSize = 0;
    let isIntersecting = true;
    let isDocumentVisible = !document.hidden;
    const startedAt = performance.now();
    let pausedStartedAt: number | null = null;
    let accumulatedPausedMs = 0;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const clearTextCache = () => {
      textCache = Object.create(null) as Record<string, TextBitmap>;
      textCacheSize = 0;
    };

    const getTextBitmap = (text: string, color: string) => {
      const key = `${fontFamily}\u0000${color}\u0000${text}`;
      const cached = textCache[key];
      if (cached) return cached;
      if (textCacheSize >= 40) clearTextCache();

      const target = document.createElement("canvas");
      const targetContext = target.getContext("2d", { alpha: true });
      const fallback: TextBitmap = { canvas: target, width: 1, height: 22 };
      if (!targetContext) return fallback;
      const scale = 1.5;
      targetContext.font = `10px ${fontFamily}`;
      const width = Math.ceil(targetContext.measureText(text).width + 4);
      target.width = Math.ceil(width * scale);
      target.height = Math.ceil(22 * scale);
      targetContext.setTransform(scale, 0, 0, scale, 0, 0);
      targetContext.font = `10px ${fontFamily}`;
      targetContext.textAlign = "center";
      targetContext.textBaseline = "middle";
      targetContext.fillStyle = color;
      targetContext.fillText(text, width / 2, 11);
      const bitmap = { canvas: target, width, height: 22 };
      textCache[key] = bitmap;
      textCacheSize += 1;
      return bitmap;
    };

    const rebuildFittedLines = () => {
      if (slotWidth <= 0) return;
      streamContext.font = `10px ${fontFamily}`;
      fittedLines = streamLines.map((line) => fitTextToWidth(streamContext, line, slotWidth * 0.84));
      const widths = fittedLines.map((line) => Math.min(slotWidth, Math.max(56, streamContext.measureText(line).width + 28)));
      const rawCenters: number[] = [];
      let cursor = 0;
      for (const width of widths) {
        rawCenters.push(cursor + width / 2);
        cursor += width;
      }
      const origin = rawCenters[0] ?? 0;
      sessionLayout = {
        widths,
        offsets: rawCenters.map((center) => center - origin),
        cycleWidth: Math.max(1, cursor),
        scrollSpeed: slotWidth / scrollStepSeconds
      };
    };

    const resizeLayers = () => {
      cssWidth = root.clientWidth;
      cssHeight = root.clientHeight;
      if (cssWidth <= 0 || cssHeight <= 0) return;
      const span = cssWidth * 0.88;
      slotWidth = span * 0.35;

      const pixelRatio = Math.min(window.devicePixelRatio || 1, 1.5);
      streamOverlay.width = Math.max(1, Math.round(cssWidth * pixelRatio));
      streamOverlay.height = Math.max(1, Math.round(cssHeight * pixelRatio));
      streamContext.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
      overlay.width = Math.max(1, Math.round(cssWidth * pixelRatio));
      overlay.height = Math.max(1, Math.round(cssHeight * pixelRatio));
      overlayContext.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
      rebuildFittedLines();
    };

    const refreshTheme = () => {
      const style = getComputedStyle(root);
      primary = style.getPropertyValue("--primary").trim() || primary;
      mutedForeground = style.getPropertyValue("--muted-foreground").trim() || mutedForeground;
      fontFamily = style.fontFamily || fontFamily;
      clearTextCache();
      rebuildFittedLines();
    };

    const drawOverlay = (now: number) => {
      frame = 0;
      if (!isIntersecting || !isDocumentVisible) return;
      if (cssWidth <= 0 || cssHeight <= 0) {
        frame = window.requestAnimationFrame(drawOverlay);
        return;
      }

      const elapsed = reduceMotion ? 0 : (now - startedAt - accumulatedPausedMs) / 1000;
      const extractionCycle = reduceMotion ? -1 : Math.floor(elapsed / extractionIntervalSeconds);
      const selectedLogicalIndex = reduceMotion
        ? null
        : nearestLogicalIndex(extractionCycle * extractionIntervalSeconds * sessionLayout.scrollSpeed, sessionLayout);
      if (!reduceMotion && extractionCycle !== lastExtractionCycle) {
        landingRatio = clampedLandingProgress(progress);
        lastExtractionCycle = extractionCycle;
      }

      streamContext.clearRect(0, 0, cssWidth, cssHeight);
      overlayContext.clearRect(0, 0, cssWidth, cssHeight);
      const visibleLines = fittedLines.length > 0 ? fittedLines : streamLines;
      drawScrollingSessions(streamContext, cssWidth, elapsed, visibleLines, sessionLayout, selectedLogicalIndex, extractionCycle, mutedForeground, getTextBitmap);
      if (!reduceMotion) {
        drawCompressingSession(overlayContext, cssWidth, elapsed, visibleLines, sessionLayout, selectedLogicalIndex ?? 0, extractionCycle, primary, mutedForeground, getTextBitmap);
        drawToken(overlayContext, cssWidth, cssHeight, elapsed, landingRatio, primary);
      }
      if (!reduceMotion) frame = window.requestAnimationFrame(drawOverlay);
    };

    const startDrawing = () => {
      if (pausedStartedAt !== null) {
        accumulatedPausedMs += performance.now() - pausedStartedAt;
        pausedStartedAt = null;
      }
      if (frame === 0 && isIntersecting && isDocumentVisible && !reduceMotion) {
        frame = window.requestAnimationFrame(drawOverlay);
      }
    };
    const stopDrawing = () => {
      if (frame !== 0) window.cancelAnimationFrame(frame);
      frame = 0;
      if (pausedStartedAt === null) pausedStartedAt = performance.now();
    };

    resizeLayers();
    refreshTheme();
    if (reduceMotion) drawOverlay(performance.now());
    const resizeObserver = new ResizeObserver(() => {
      resizeLayers();
      if (reduceMotion) drawOverlay(performance.now());
    });
    resizeObserver.observe(root);
    const themeObserver = new MutationObserver(() => {
      refreshTheme();
      if (reduceMotion) drawOverlay(performance.now());
    });
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "style"] });
    const visibilityObserver = new IntersectionObserver(([entry]) => {
      isIntersecting = entry?.isIntersecting ?? false;
      if (isIntersecting) {
        startDrawing();
      } else {
        stopDrawing();
      }
    }, { threshold: 0.01 });
    const handleVisibilityChange = () => {
      isDocumentVisible = !document.hidden;
      if (isDocumentVisible) {
        startDrawing();
      } else {
        stopDrawing();
      }
    };
    visibilityObserver.observe(root);
    document.addEventListener("visibilitychange", handleVisibilityChange);
    startDrawing();

    return () => {
      stopDrawing();
      resizeObserver.disconnect();
      themeObserver.disconnect();
      visibilityObserver.disconnect();
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      clearTextCache();
    };
  });

  function fitTextToWidth(context: CanvasRenderingContext2D, value: string, maximumWidth: number) {
    if (context.measureText(value).width <= maximumWidth) return value;

    const characters = Array.from(value);
    const ellipsis = "…";
    let low = 0;
    let high = characters.length;
    while (low < high) {
      const middle = Math.ceil((low + high) / 2);
      const candidate = `${characters.slice(0, middle).join("")}${ellipsis}`;
      if (context.measureText(candidate).width <= maximumWidth) low = middle;
      else high = middle - 1;
    }
    return `${characters.slice(0, low).join("")}${ellipsis}`;
  }

  function logicalSessionPosition(logicalIndex: number, layout: SessionLayout) {
    const count = layout.widths.length;
    if (count === 0) return 0;
    const cycle = Math.floor(logicalIndex / count);
    return cycle * layout.cycleWidth + (layout.offsets[modulo(logicalIndex, count)] ?? 0);
  }

  function nearestLogicalIndex(scrollOffset: number, layout: SessionLayout) {
    const count = layout.widths.length;
    if (count === 0) return 0;
    const baseCycle = Math.floor(scrollOffset / layout.cycleWidth);
    let nearestIndex = baseCycle * count;
    let nearestDistance = Number.POSITIVE_INFINITY;
    for (let cycle = baseCycle - 1; cycle <= baseCycle + 1; cycle += 1) {
      for (let lineIndex = 0; lineIndex < count; lineIndex += 1) {
        const logicalIndex = cycle * count + lineIndex;
        const distance = Math.abs(logicalSessionPosition(logicalIndex, layout) - scrollOffset);
        if (distance < nearestDistance) {
          nearestIndex = logicalIndex;
          nearestDistance = distance;
        }
      }
    }
    return nearestIndex;
  }

  function drawScrollingSessions(
    context: CanvasRenderingContext2D,
    width: number,
    elapsed: number,
    lines: readonly string[],
    layout: SessionLayout,
    selectedLogicalIndex: number | null,
    extractionCycle: number,
    mutedForeground: string,
    getTextBitmap: (text: string, color: string) => TextBitmap
  ) {
    const center = width / 2;
    const left = width * 0.06;
    const right = width - left;
    const fadeWidth = Math.max(24, width * 0.0616);
    const scrollOffset = elapsed * layout.scrollSpeed;
    const extractionElapsed = elapsed - extractionCycle * extractionIntervalSeconds;
    const centerLogicalIndex = nearestLogicalIndex(scrollOffset, layout);
    const firstVisible = centerLogicalIndex - 9;
    const lastVisible = centerLogicalIndex + 9;

    for (let logicalIndex = firstVisible; logicalIndex <= lastVisible; logicalIndex += 1) {
      const lineIndex = modulo(logicalIndex, lines.length);
      const sessionWidth = layout.widths[lineIndex] ?? 56;
      const isSelected = selectedLogicalIndex !== null && logicalIndex === selectedLogicalIndex && extractionElapsed <= extractionDurationSeconds;
      if (isSelected) continue;
      const x = center + logicalSessionPosition(logicalIndex, layout) - scrollOffset;
      const opacity = edgeOpacity(x, left, right, fadeWidth);
      if (opacity <= 0 || x < -sessionWidth || x > width + sessionWidth) continue;

      const bitmap = getTextBitmap(lines[lineIndex], mutedForeground);
      context.save();
      context.beginPath();
      const clipLeft = Math.max(left, x - sessionWidth * 0.46);
      const clipRight = Math.min(right, x + sessionWidth * 0.46);
      if (clipRight <= clipLeft) {
        context.restore();
        continue;
      }
      context.rect(clipLeft, 4, clipRight - clipLeft, 22);
      context.clip();
      context.translate(x, 15);
      context.globalAlpha = opacity;
      context.drawImage(bitmap.canvas, -bitmap.width / 2, -bitmap.height / 2, bitmap.width, bitmap.height);
      context.restore();
    }
    context.globalAlpha = 1;
  }

  function drawCompressingSession(
    context: CanvasRenderingContext2D,
    width: number,
    elapsed: number,
    lines: readonly string[],
    layout: SessionLayout,
    selectedLogicalIndex: number,
    extractionCycle: number,
    primary: string,
    mutedForeground: string,
    getTextBitmap: (text: string, color: string) => TextBitmap
  ) {
    const extractionElapsed = elapsed - extractionCycle * extractionIntervalSeconds;
    if (extractionElapsed > extractionDurationSeconds) return;

    const lineIndex = modulo(selectedLogicalIndex, lines.length);
    const line = lines[lineIndex];
    const sessionWidth = layout.widths[lineIndex] ?? 56;
    const mutedBitmap = getTextBitmap(line, mutedForeground);
    const primaryBitmap = getTextBitmap(line, primary);
    const scan = easedRange(extractionElapsed, 0.08, 0.82);
    const compression = easedRange(extractionElapsed, 0.88, 1.28);
    const opacity = extractionElapsed >= 1.42 ? 0 : mix(1, 0.16, compression);
    if (opacity <= 0) return;

    const center = width / 2;
    const scrollOffset = elapsed * layout.scrollSpeed;
    const naturalX = center + logicalSessionPosition(selectedLogicalIndex, layout) - scrollOffset;
    const blockX = mix(naturalX, center, compression);
    const scaleX = mix(1, 0.12, compression);
    const scaleY = mix(1, 0.24, compression);
    const left = width * 0.06;
    const right = width - left;
    const clipLeft = Math.max(left, blockX - sessionWidth * 0.46);
    const clipRight = Math.min(right, blockX + sessionWidth * 0.46);
    if (clipRight <= clipLeft) return;
    context.save();
    context.beginPath();
    context.rect(clipLeft, 4, clipRight - clipLeft, 22);
    context.clip();
    context.translate(blockX, 15);
    context.scale(scaleX, scaleY);
    context.globalAlpha = opacity;
    context.drawImage(mutedBitmap.canvas, -mutedBitmap.width / 2, -mutedBitmap.height / 2, mutedBitmap.width, mutedBitmap.height);

    const scannedWidth = primaryBitmap.width * scan;
    context.save();
    context.beginPath();
    context.rect(-primaryBitmap.width / 2, -primaryBitmap.height / 2, scannedWidth, primaryBitmap.height);
    context.clip();
    context.drawImage(primaryBitmap.canvas, -primaryBitmap.width / 2, -primaryBitmap.height / 2, primaryBitmap.width, primaryBitmap.height);
    context.restore();

    if (scan > 0 && scan < 1 && compression < 0.2) {
      const scannerX = -primaryBitmap.width / 2 + scannedWidth;
      context.fillStyle = primary;
      context.globalAlpha = opacity * 0.1;
      context.fillRect(scannerX - 13, -10, 13, 20);
      context.globalAlpha = opacity * 0.88;
      context.fillRect(scannerX - 0.75, -10, 1.5, 20);
    }
    context.restore();
  }

  function drawToken(
    context: CanvasRenderingContext2D,
    width: number,
    height: number,
    elapsed: number,
    landingRatio: number,
    primary: string
  ) {
    const extractionElapsed = elapsed % extractionIntervalSeconds;
    if (extractionElapsed < 0.9 || extractionElapsed > extractionDurationSeconds) return;
    const progressWidth = Math.min(width, 470);
    const progressHeight = 7;
    const progressHeadX = (width - progressWidth) / 2 + progressWidth * landingRatio;
    const destinationX = progressHeadX - progressHeight / 2;
    const progressTop = height - 4 - progressHeight;
    const destinationY = progressTop + progressHeight / 2;
    const startX = width / 2;
    const startY = 15;
    const transferY = Math.min(49, destinationY - 14);
    let x = startX;
    let y = startY;

    if (extractionElapsed > 1.24 && extractionElapsed <= 1.76) {
      y = mix(startY, transferY, easeInOut((extractionElapsed - 1.24) / 0.52));
    } else if (extractionElapsed > 1.76) {
      const transfer = easeInOut(Math.min(1, (extractionElapsed - 1.76) / 0.76));
      x = mix(startX, destinationX, transfer);
      y = mix(transferY, destinationY, transfer);
    }

    const formation = easedRange(extractionElapsed, 0.9, 1.28);
    const fade = extractionElapsed > 2.64 ? Math.max(0, (3 - extractionElapsed) / 0.36) : 1;
    const tokenSize = progressHeight * formation;
    context.globalAlpha = formation * fade * 0.92;
    context.fillStyle = primary;
    roundRect(context, x - tokenSize / 2, y - tokenSize / 2, tokenSize, tokenSize, tokenSize * 0.36);
    context.fill();
    context.globalAlpha = 1;
  }

  function normalizedProgress(value: number) {
    return Math.min(1, Math.max(0, Number.isFinite(value) ? value : 0));
  }

  function clampedLandingProgress(value: number) {
    return Math.min(0.96, Math.max(0.04, normalizedProgress(value)));
  }

  function edgeOpacity(x: number, left: number, right: number, fadeWidth: number) {
    return Math.min(
      1,
      Math.max(0, (x - left) / fadeWidth),
      Math.max(0, (right - x) / fadeWidth)
    );
  }

  function easedRange(value: number, from: number, to: number) {
    return easeInOut(Math.min(1, Math.max(0, (value - from) / (to - from))));
  }

  function mix(from: number, to: number, amount: number) {
    return from + (to - from) * amount;
  }

  function easeInOut(value: number) {
    return value < 0.5 ? 2 * value * value : 1 - Math.pow(-2 * value + 2, 2) / 2;
  }

  function modulo(value: number, divisor: number) {
    return ((value % divisor) + divisor) % divisor;
  }

  function roundRect(context: CanvasRenderingContext2D, x: number, y: number, width: number, height: number, radius: number) {
    const safeWidth = Math.max(0, width);
    const safeHeight = Math.max(0, height);
    const safeRadius = Math.min(Math.max(0, radius), safeWidth / 2, safeHeight / 2);
    context.beginPath();
    if (typeof context.roundRect === "function") {
      context.roundRect(x, y, safeWidth, safeHeight, safeRadius);
    } else {
      context.rect(x, y, safeWidth, safeHeight);
    }
  }
</script>

<div class="summary-animation" {@attach captureRoot}>
  <canvas class="animation-overlay" {@attach captureStreamOverlay} aria-hidden="true"></canvas>
  <canvas class="animation-overlay effect-overlay" {@attach captureOverlay} aria-hidden="true"></canvas>

  <div
    class="summary-progress"
    role="progressbar"
    aria-label="会議ノート生成の進捗"
    aria-valuemin="0"
    aria-valuemax="1"
    aria-valuenow={determinate ? displayedProgress : undefined}
  >
    <span
      class:indeterminate={!determinate}
      class="summary-progress-value"
      style={determinate ? `transform: scaleX(${displayedProgress})` : undefined}
    ></span>
  </div>
</div>

<style>
  .summary-animation {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    contain: layout paint style;
  }

  .animation-overlay {
    position: absolute;
    inset: 0;
    display: block;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }

  .effect-overlay { z-index: 2; }

  .summary-progress {
    position: absolute;
    bottom: 4px;
    left: 50%;
    width: min(100%, 470px);
    height: 7px;
    overflow: hidden;
    z-index: 1;
    border-radius: 999px;
    background: color-mix(in oklch, var(--muted) 78%, var(--background));
    transform: translateX(-50%);
  }

  .summary-progress-value {
    display: block;
    width: 100%;
    height: 100%;
    border-radius: inherit;
    background: var(--primary);
    transform: scaleX(0);
    transform-origin: left;
    transition: transform 420ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  .summary-progress-value.indeterminate {
    width: 28%;
    animation: progress-glide 1.8s ease-in-out infinite;
  }

  @keyframes progress-glide {
    from { transform: translate3d(-110%, 0, 0); }
    to { transform: translate3d(460%, 0, 0); }
  }

  @media (prefers-reduced-motion: reduce) {
    .summary-progress-value.indeterminate { animation: none; }
    .summary-progress-value { transition: none; }
    .summary-progress-value.indeterminate { transform: translate3d(130%, 0, 0); }
  }
</style>
