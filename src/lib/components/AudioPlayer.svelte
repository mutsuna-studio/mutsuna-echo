<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import FastForward from "@lucide/svelte/icons/fast-forward";
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import Rewind from "@lucide/svelte/icons/rewind";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { formatFileSize, formatTimestamp } from "../format";
  import type { AudioWaveform as AudioWaveformData, AudioWaveformProgress, SelectedAudioFile } from "../types/transcript";
  import AudioWaveform from "./AudioWaveform.svelte";

  type Props = {
    audio: SelectedAudioFile;
    source?: "recording" | "imported";
    onError: (message: string) => void;
  };

  let { audio, source, onError }: Props = $props();
  let element = $state<HTMLAudioElement | null>(null);
  let playing = $state(false);
  let currentSeconds = $state(0);
  let durationSeconds = $state(0);
  let volume = $state(1);
  let previousVolume = $state(1);
  let playbackRateIndex = $state(0);
  let waveformPeaks = $state.raw<number[]>([]);
  let waveformLoading = $state(false);
  let waveformCompletedPoints = $state(0);
  const playbackRates = [1, 1.25, 1.5, 2] as const;
  const playbackRate = $derived(playbackRates[playbackRateIndex]);

  $effect(() => {
    audio.playbackUrl;
    durationSeconds = audio.durationMs / 1_000;
    currentSeconds = 0;
    playing = false;
    if (!element) return;
    element.pause();
    element.load();
  });

  $effect(() => {
    const meetingId = audio.meetingId;
    let active = true;
    let stopListening: (() => void) | undefined;
    waveformPeaks = [];
    waveformCompletedPoints = 0;
    waveformLoading = true;
    (async () => {
      try {
        stopListening = await listen<AudioWaveformProgress>("audio-waveform-progress", ({ payload }) => {
          if (!active || payload.meetingId !== meetingId) return;
          waveformPeaks = payload.peaks;
          waveformCompletedPoints = payload.completedPoints;
        });
        if (!active) {
          stopListening();
          return;
        }
      } catch {
        // The final waveform command still works if progressive events are unavailable.
      }
      if (!active) return;
      try {
        const waveform = await invoke<AudioWaveformData>("get_selected_audio_waveform", { meetingId, points: 320 });
        if (active && waveform.meetingId === meetingId) {
          waveformPeaks = waveform.peaks;
          waveformCompletedPoints = waveform.points;
        }
      } catch (error) {
        if (active) onError(`音声波形を生成できませんでした: ${String(error)}`);
      } finally {
        if (active) waveformLoading = false;
      }
    })();
    return () => {
      active = false;
      stopListening?.();
    };
  });

  async function togglePlayback() {
    if (!element) return;
    if (!element.paused) {
      element.pause();
      return;
    }
    try {
      await element.play();
    } catch {
      onError("音声を再生できませんでした。元の音声ファイルと対応形式を確認してください。");
    }
  }

  function seekBy(seconds: number) {
    if (!element) return;
    element.currentTime = Math.max(0, Math.min(durationSeconds, element.currentTime + seconds));
    currentSeconds = element.currentTime;
  }

  function seekTo(seconds: number) {
    if (!element) return;
    element.currentTime = Math.max(0, Math.min(durationSeconds, seconds));
    currentSeconds = element.currentTime;
  }

  function changeVolume(event: Event) {
    if (!element || !(event.currentTarget instanceof HTMLInputElement)) return;
    volume = Number(event.currentTarget.value);
    element.volume = volume;
    if (volume > 0) previousVolume = volume;
  }

  function toggleMute() {
    if (!element) return;
    volume = volume > 0 ? 0 : Math.max(previousVolume, 0.5);
    element.volume = volume;
  }

  function cyclePlaybackRate() {
    const nextIndex = (playbackRateIndex + 1) % playbackRates.length;
    playbackRateIndex = nextIndex;
    if (element) element.playbackRate = playbackRates[nextIndex];
  }
</script>

<section class="audio-player" aria-label="会議音声プレイヤー">
  <audio
    bind:this={element}
    src={audio.playbackUrl}
    preload="metadata"
    onloadedmetadata={() => durationSeconds = Number.isFinite(element?.duration) ? element!.duration : audio.durationMs / 1_000}
    ondurationchange={() => durationSeconds = Number.isFinite(element?.duration) ? element!.duration : durationSeconds}
    ontimeupdate={() => currentSeconds = element?.currentTime ?? 0}
    onplay={() => playing = true}
    onpause={() => playing = false}
    onended={() => playing = false}
    onerror={() => onError("音声を読み込めませんでした。ファイルが移動・変更されていないか確認してください。")}
  ></audio>

  <div class="audio-heading">
    <div>
      <strong>{audio.name}</strong>
      <small>{formatFileSize(audio.sizeBytes)}</small>
    </div>
    {#if source}<Badge variant="secondary">{source === "recording" ? "録音" : "取込"}</Badge>{/if}
  </div>

  <div class="player-controls">
    <div class="transport-controls">
      <Button size="icon-sm" variant="ghost" type="button" icon={Rewind} aria-label="10秒戻る" title="10秒戻る" onclick={() => seekBy(-10)} />
      <Button size="icon" type="button" icon={playing ? Pause : Play} aria-label={playing ? "一時停止" : "再生"} title={playing ? "一時停止" : "再生"} onclick={togglePlayback} />
      <Button size="icon-sm" variant="ghost" type="button" icon={FastForward} aria-label="10秒進む" title="10秒進む" onclick={() => seekBy(10)} />
    </div>
    <div class="timeline-controls">
      <time>{formatTimestamp(currentSeconds * 1_000)}</time>
      <AudioWaveform peaks={waveformPeaks} completedPoints={waveformCompletedPoints} {currentSeconds} {durationSeconds} loading={waveformLoading} onseek={seekTo} />
      <time>{formatTimestamp(durationSeconds * 1_000)}</time>
    </div>
    <div class="playback-options">
      <Button class="rate-button" size="sm" variant="ghost" type="button" aria-label="再生速度を変更" title="再生速度を変更" onclick={cyclePlaybackRate}>{playbackRate}×</Button>
      <Button size="icon-sm" variant="ghost" type="button" icon={volume > 0 ? Volume2 : VolumeX} aria-label={volume > 0 ? "ミュート" : "ミュート解除"} title={volume > 0 ? "ミュート" : "ミュート解除"} onclick={toggleMute} />
      <input class="volume-bar" type="range" min="0" max="1" step="0.05" value={volume} aria-label="音量" oninput={changeVolume} />
    </div>
  </div>
</section>

<style>
  audio { display: none; }
  .audio-player { display: grid; gap: 11px; padding: 12px 14px; border: 1px solid var(--border); border-radius: 10px; background: var(--background); }
  .audio-heading { display: flex; min-width: 0; align-items: center; gap: 10px; }
  .audio-heading > div { display: flex; min-width: 0; flex: 1; align-items: baseline; gap: 8px; }
  .audio-heading strong { overflow: hidden; font-size: 0.8rem; text-overflow: ellipsis; white-space: nowrap; }
  .audio-heading small { flex: none; color: var(--muted-foreground); font-size: 0.68rem; }
  .player-controls { display: grid; grid-template-columns: auto minmax(120px, 1fr) auto; align-items: center; gap: 9px; }
  .transport-controls, .playback-options { display: flex; align-items: center; gap: 3px; }
  .timeline-controls { display: grid; min-width: 0; grid-template-columns: auto minmax(60px, 1fr) auto; align-items: center; gap: 7px; }
  .timeline-controls time { color: var(--muted-foreground); font-size: 0.7rem; font-variant-numeric: tabular-nums; }
  .volume-bar { min-width: 0; accent-color: var(--primary); cursor: pointer; }
  .volume-bar { width: 68px; }
  :global(.rate-button) { min-width: 46px; font-variant-numeric: tabular-nums; }

  @container audio-player (max-width: 520px) {
    .player-controls { grid-template-columns: minmax(0, 1fr) auto; }
    .timeline-controls { grid-column: 1 / -1; grid-row: 1; }
    .transport-controls { grid-column: 1; grid-row: 2; }
    .playback-options { grid-column: 2; grid-row: 2; }
    .volume-bar { display: none; }
  }
</style>
