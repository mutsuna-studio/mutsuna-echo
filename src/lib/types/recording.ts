import type { SelectedAudioFile } from "./transcript";

export type RecordingPhase = "idle" | "starting" | "recording" | "finalizing" | "completed" | "failed";
export type StopReason = "user" | "durationLimit" | "sourceDisconnected" | "sourceStalled" | "captureError";
export type VoiceActivityState = "unavailable" | "listening" | "speechDetected";

export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

export interface RecordingCapabilities {
  platform: "windows" | "macos" | "android" | "unsupported";
  supported: boolean;
  microphoneSupported: boolean;
  systemAudioSupported: boolean;
  systemAudioLimited: boolean;
  limitation: string | null;
  microphoneDevices: AudioDevice[];
  systemDevices: AudioDevice[];
  sampleRate: number;
  channels: number;
  codec: string;
  bitrate: number;
  maxDurationMs: number;
}

export interface RecordingStatus {
  phase: RecordingPhase;
  sessionId: string | null;
  elapsedMs: number;
  microphoneLevel: number;
  systemLevel: number;
  microphone: boolean;
  systemAudio: boolean;
  voiceActivity: VoiceActivityState;
  outputPath: string | null;
  stopReason: StopReason | null;
  warning: string | null;
  error: string | null;
}

export interface RecoverableRecording {
  sessionId: string;
  startedAt: string;
  durationMs: number;
  microphone: boolean;
  systemAudio: boolean;
}

export interface RecordedAudioSummary {
  id: string;
  meetingId: string;
  fileName: string;
  sizeBytes: number;
  recordedAtUnixMs: number;
  transcriptProviders: string[];
}

export interface RecentMeetingSummary {
  meetingId: string;
  title: string;
  fileName: string;
  sizeBytes: number;
  occurredAtUnixMs: number;
  updatedAtUnixMs: number;
  audioAvailable: boolean;
  source: "recording" | "imported";
  transcriptProviders: string[];
}

export interface StopRecordingResult {
  status: RecordingStatus;
  audio: SelectedAudioFile | null;
}
