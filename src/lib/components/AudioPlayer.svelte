<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import FastForward from "@lucide/svelte/icons/fast-forward";
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import Rewind from "@lucide/svelte/icons/rewind";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import { Button } from "@mutsuna/ui/button";
  import {
    getAudioPlaybackBackend,
    getNativeAudioState,
    loadNativeAudio,
    pauseNativeAudio,
    playNativeAudio,
    releaseNativeAudio,
    seekNativeAudio,
    setNativeAudioRate,
    setNativeAudioVolume,
    type AudioPlaybackBackend,
    type NativePlaybackState,
  } from "../audio/nativePlayback";
  import { formatTimestamp } from "../format";
  import type { AudioSeekRequest, AudioWaveform as AudioWaveformData, AudioWaveformProgress, SelectedAudioFile } from "../types/transcript";
  import AudioWaveform from "./AudioWaveform.svelte";

  type Props = {
    audio: SelectedAudioFile;
    seekRequest: AudioSeekRequest | null;
    onPositionChange: (positionMs: number, followTimeline: boolean) => void;
    onPlayingChange?: (playing: boolean) => void;
    onWaveformChange?: (peaks: readonly number[]) => void;
    onError: (message: string) => void;
  };

  let { audio, seekRequest, onPositionChange, onPlayingChange = () => {}, onWaveformChange = () => {}, onError }: Props = $props();
  let backend = $state<AudioPlaybackBackend | null>(null);
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
  let processedSeekRequestId = $state(0);
  let playbackRequested = $state(false);
  let recoveryAttempted = $state(false);
  let recovering = $state(false);
  let nativeLoadedMeetingId = $state<string | null>(null);
  let lastNativeError = $state<string | null>(null);
  let nativeActionSequence = 0;
  const pendingSeekRequestIds = new Set<number>();
  const playbackRates = [1, 1.25, 1.5, 2] as const;
  const playbackRate = $derived(playbackRates[playbackRateIndex]);

  $effect(() => {
    onPlayingChange(playing);
  });

  $effect(() => {
    let active = true;
    void getAudioPlaybackBackend()
      .then((value) => {
        if (active) backend = value;
      })
      .catch((error) => {
        if (!active) return;
        backend = "web";
        onError(`音声再生環境を確認できませんでした: ${String(error)}`);
      });
    return () => { active = false; };
  });

  $effect(() => {
    if (backend !== "android-native") return;
    return () => { void releaseNativeAudio(); };
  });

  $effect(() => {
    const meetingId = audio.meetingId;
    const selectedBackend = backend;
    let active = true;
    durationSeconds = audio.durationMs / 1_000;
    currentSeconds = 0;
    processedSeekRequestId = 0;
    playing = false;
    playbackRequested = false;
    recoveryAttempted = false;
    recovering = false;
    nativeLoadedMeetingId = null;
    lastNativeError = null;
    if (selectedBackend === "android-native") {
      void loadNativeAudio(meetingId)
        .then((state) => {
          if (!active || audio.meetingId !== meetingId) return;
          applyNativeState(state);
          if (state.loaded && !state.error) {
            nativeLoadedMeetingId = meetingId;
            void applySeekRequest();
          }
        })
        .catch((error) => {
          if (active) onError(`音声を読み込めませんでした: ${String(error)}`);
        });
    } else if (selectedBackend === "web" && element) {
      element.pause();
      element.load();
    }
    return () => { active = false; };
  });

  $effect(() => {
    seekRequest;
    audio.meetingId;
    backend;
    nativeLoadedMeetingId;
    element;
    void applySeekRequest();
  });

  $effect(() => {
    if (backend !== "android-native" || !playing) return;
    let active = true;
    let pending = false;
    const poll = async () => {
      if (!active || pending) return;
      pending = true;
      try {
        const actionSequence = nativeActionSequence;
        const state = await getNativeAudioState();
        if (active && nativeActionSequence === actionSequence) applyNativeState(state);
      } catch (error) {
        if (active) reportNativeError(`再生状態を取得できませんでした: ${String(error)}`);
      } finally {
        pending = false;
      }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 200);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  });

  $effect(() => {
    const meetingId = audio.meetingId;
    let active = true;
    let stopListening: (() => void) | undefined;
    publishWaveform([]);
    waveformCompletedPoints = 0;
    waveformLoading = true;
    (async () => {
      try {
        stopListening = await listen<AudioWaveformProgress>("audio-waveform-progress", ({ payload }) => {
          if (!active || payload.meetingId !== meetingId) return;
          publishWaveform(payload.peaks);
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
          publishWaveform(waveform.peaks);
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
    if (backend === "android-native") {
      try {
        await runNativeAction(playing ? pauseNativeAudio : playNativeAudio);
      } catch (error) {
        reportNativeError(`音声を再生できませんでした: ${String(error)}`);
      }
      return;
    }
    if (!element) return;
    if (!element.paused) {
      playbackRequested = false;
      element.pause();
      return;
    }
    await startPlayback();
  }

  function publishWaveform(peaks: number[]) {
    waveformPeaks = peaks;
    onWaveformChange(peaks);
  }

  async function pausePlayback() {
    if (backend === "android-native") {
      try {
        await runNativeAction(pauseNativeAudio);
      } catch (error) {
        reportNativeError(`音声を一時停止できませんでした: ${String(error)}`);
      }
      return;
    }
    playbackRequested = false;
    element?.pause();
  }

  async function startPlayback() {
    if (backend === "android-native") {
      try {
        await runNativeAction(playNativeAudio);
      } catch (error) {
        reportNativeError(`音声を再生できませんでした: ${String(error)}`);
      }
      return;
    }
    if (!element) return;
    playbackRequested = true;
    recoveryAttempted = false;
    try {
      if (element.error) {
        recoveryAttempted = true;
        recovering = true;
        await reloadMedia(element.currentTime);
        recovering = false;
      }
      await element.play();
    } catch {
      recovering = false;
      playbackRequested = false;
      onError("音声を再生できませんでした。元の音声ファイルと対応形式を確認してください。");
    }
  }

  async function handleMediaError() {
    if (!element || recovering) return;
    if (playbackRequested && !recoveryAttempted) {
      recoveryAttempted = true;
      recovering = true;
      const position = element.currentTime;
      try {
        await reloadMedia(position);
        recovering = false;
        await element.play();
        return;
      } catch {
        recovering = false;
      }
    }
    playbackRequested = false;
    onError("音声を読み込めませんでした。ファイルが移動・変更されていないか確認してください。");
  }

  async function reloadMedia(position: number) {
    if (!element) throw new Error("音声プレイヤーを初期化できませんでした。");
    const target = element;
    await new Promise<void>((resolve, reject) => {
      const timeout = window.setTimeout(() => finish(() => reject(new Error("音声の再読み込みがタイムアウトしました。"))), 8_000);
      const handleReady = () => finish(resolve);
      const handleError = () => finish(() => reject(new Error("音声を再読み込みできませんでした。")));
      const finish = (complete: () => void) => {
        window.clearTimeout(timeout);
        target.removeEventListener("canplay", handleReady);
        target.removeEventListener("error", handleError);
        complete();
      };
      target.addEventListener("canplay", handleReady, { once: true });
      target.addEventListener("error", handleError, { once: true });
      target.load();
      if (target.readyState >= HTMLMediaElement.HAVE_FUTURE_DATA) finish(resolve);
    });
    target.currentTime = Math.max(0, Math.min(durationSeconds, position));
  }

  async function seekBy(seconds: number) {
    await seekTo(currentSeconds + seconds);
  }

  async function seekTo(seconds: number): Promise<boolean> {
    const targetSeconds = Math.max(0, Math.min(durationSeconds, seconds));
    if (backend === "android-native") {
      if (nativeLoadedMeetingId !== audio.meetingId) return false;
      updatePosition(targetSeconds, true);
      try {
        await runNativeAction(() => seekNativeAudio(targetSeconds * 1_000), true);
        return true;
      } catch (error) {
        reportNativeError(`再生位置を変更できませんでした: ${String(error)}`);
        return false;
      }
    }
    if (!element) return false;
    try {
      element.currentTime = targetSeconds;
    } catch {
      return false;
    }
    updatePosition(element.currentTime, true);
    return true;
  }

  async function applySeekRequest() {
    const request = seekRequest;
    if (!request || request.meetingId !== audio.meetingId || request.requestId === processedSeekRequestId || pendingSeekRequestIds.has(request.requestId)) return;
    if (backend === "android-native" && nativeLoadedMeetingId !== audio.meetingId) return;
    if (backend === "web" && !element) return;
    pendingSeekRequestIds.add(request.requestId);
    try {
      if (request.pause) {
        await pausePlayback();
        processedSeekRequestId = request.requestId;
        return;
      }
      if (!await seekTo(request.positionMs / 1_000)) return;
      if (seekRequest?.requestId !== request.requestId || audio.meetingId !== request.meetingId) return;
      processedSeekRequestId = request.requestId;
      if (request.autoplay) await startPlayback();
    } finally {
      pendingSeekRequestIds.delete(request.requestId);
    }
  }

  function updatePosition(seconds: number, followTimeline = false) {
    currentSeconds = seconds;
    onPositionChange(Math.round(seconds * 1_000), followTimeline);
  }

  function handleTimeUpdate() {
    updatePosition(element?.currentTime ?? 0);
  }

  function handleLoadedMetadata() {
    durationSeconds = Number.isFinite(element?.duration) ? element!.duration : audio.durationMs / 1_000;
    void applySeekRequest();
  }

  function changeVolume(event: Event) {
    if (!(event.currentTarget instanceof HTMLInputElement)) return;
    volume = Number(event.currentTarget.value);
    if (backend === "android-native") {
      void runNativeAction(() => setNativeAudioVolume(volume)).catch((error) => {
        reportNativeError(`音量を変更できませんでした: ${String(error)}`);
      });
    } else if (element) {
      element.volume = volume;
    }
    if (volume > 0) previousVolume = volume;
  }

  function toggleMute() {
    volume = volume > 0 ? 0 : Math.max(previousVolume, 0.5);
    if (backend === "android-native") {
      void runNativeAction(() => setNativeAudioVolume(volume)).catch((error) => {
        reportNativeError(`音量を変更できませんでした: ${String(error)}`);
      });
    } else if (element) {
      element.volume = volume;
    }
  }

  function cyclePlaybackRate() {
    const nextIndex = (playbackRateIndex + 1) % playbackRates.length;
    playbackRateIndex = nextIndex;
    if (backend === "android-native") {
      void runNativeAction(() => setNativeAudioRate(playbackRates[nextIndex])).catch((error) => {
        reportNativeError(`再生速度を変更できませんでした: ${String(error)}`);
      });
    } else if (element) {
      element.playbackRate = playbackRates[nextIndex];
    }
  }

  function applyNativeState(state: NativePlaybackState, followTimeline = false) {
    playing = state.playing;
    if (state.durationMs > 0) durationSeconds = state.durationMs / 1_000;
    updatePosition(state.positionMs / 1_000, followTimeline);
    if (state.error) reportNativeError(state.error);
  }

  async function runNativeAction(action: () => Promise<NativePlaybackState>, followTimeline = false) {
    const actionSequence = ++nativeActionSequence;
    const state = await action();
    if (actionSequence === nativeActionSequence) applyNativeState(state, followTimeline);
    return state;
  }

  function reportNativeError(message: string) {
    if (message === lastNativeError) return;
    lastNativeError = message;
    onError(message);
  }
</script>

<section class="audio-player" aria-label="会議音声プレイヤー">
  {#if backend === "web"}
    <audio
      bind:this={element}
      src={audio.playbackUrl}
      preload="metadata"
      onloadedmetadata={handleLoadedMetadata}
      ondurationchange={() => durationSeconds = Number.isFinite(element?.duration) ? element!.duration : durationSeconds}
      ontimeupdate={handleTimeUpdate}
      onplay={() => playing = true}
      onpause={() => playing = false}
      onended={() => { playing = false; playbackRequested = false; }}
      onerror={() => void handleMediaError()}
    ></audio>
  {/if}

  <div class="player-controls">
    <div class="transport-controls">
      <Button size="icon-sm" variant="ghost" type="button" icon={Rewind} aria-label="10秒戻る" title="10秒戻る" disabled={!backend} onclick={() => void seekBy(-10)} />
      <Button size="icon" type="button" icon={playing ? Pause : Play} aria-label={playing ? "一時停止" : "再生"} title={playing ? "一時停止" : "再生"} disabled={!backend} onclick={() => void togglePlayback()} />
      <Button size="icon-sm" variant="ghost" type="button" icon={FastForward} aria-label="10秒進む" title="10秒進む" disabled={!backend} onclick={() => void seekBy(10)} />
    </div>
    <div class="timeline-controls">
      <time>{formatTimestamp(currentSeconds * 1_000)}</time>
      <AudioWaveform peaks={waveformPeaks} completedPoints={waveformCompletedPoints} {currentSeconds} {durationSeconds} loading={waveformLoading} onseek={(seconds) => void seekTo(seconds)} />
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
  .audio-player { display: grid; padding: 9px 12px; border: 1px solid var(--border); border-radius: 10px; background: var(--background); }
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
