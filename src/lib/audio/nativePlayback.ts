import { invoke } from "@tauri-apps/api/core";

export type AudioPlaybackBackend = "web" | "android-native";

export type NativePlaybackState = {
  loaded: boolean;
  playing: boolean;
  positionMs: number;
  durationMs: number;
  bufferedPositionMs: number;
  buffering: boolean;
  ended: boolean;
  error: string | null;
};

export function getAudioPlaybackBackend(): Promise<AudioPlaybackBackend> {
  return invoke<AudioPlaybackBackend>("get_audio_playback_backend");
}

export function loadNativeAudio(meetingId: string): Promise<NativePlaybackState> {
  return invoke<NativePlaybackState>("load_selected_audio_for_playback", { meetingId });
}

export function playNativeAudio(): Promise<NativePlaybackState> {
  return invoke<NativePlaybackState>("play_selected_audio");
}

export function pauseNativeAudio(): Promise<NativePlaybackState> {
  return invoke<NativePlaybackState>("pause_selected_audio");
}

export function seekNativeAudio(positionMs: number): Promise<NativePlaybackState> {
  return invoke<NativePlaybackState>("seek_selected_audio", { positionMs: Math.max(0, Math.round(positionMs)) });
}

export function getNativeAudioState(): Promise<NativePlaybackState> {
  return invoke<NativePlaybackState>("get_audio_playback_state");
}

export function setNativeAudioVolume(volume: number): Promise<NativePlaybackState> {
  return invoke<NativePlaybackState>("set_audio_playback_volume", { volume });
}

export function setNativeAudioRate(rate: number): Promise<NativePlaybackState> {
  return invoke<NativePlaybackState>("set_audio_playback_rate", { rate });
}

export function releaseNativeAudio(): Promise<NativePlaybackState> {
  return invoke<NativePlaybackState>("release_audio_playback");
}
