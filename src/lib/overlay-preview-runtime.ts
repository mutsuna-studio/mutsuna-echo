import { invoke } from "@tauri-apps/api/core";
import type { MeetingDetection } from "./types/meeting";
import type {
  OverlayPreviewMode,
  OverlayPreviewRuntime,
  OverlayPreviewSnapshot
} from "./types/overlay-preview";
import type { RecordingStatus } from "./types/recording";

const detection: MeetingDetection = {
  provider: "googleMeet",
  providerLabel: "Google Meet",
  windowTitle: "Weekly Product Sync — Google Meet",
  detectedAtUnixMs: Date.now()
};

function status(mode: Exclude<OverlayPreviewMode, "detection">): RecordingStatus {
  return {
    phase: mode === "error" ? "failed" : mode,
    sessionId: "preview-session",
    elapsedMs: mode === "finalizing" ? 2_538_000 : 2_537_000,
    microphoneLevel: mode === "recording" ? 0.72 : 0.18,
    systemLevel: mode === "recording" ? 0.48 : 0.12,
    microphone: true,
    systemAudio: true,
    voiceActivity: mode === "recording" ? "speechDetected" : "listening",
    outputPath: mode === "completed" ? "preview.m4a" : null,
    microphoneTrackPath: null,
    systemTrackPath: null,
    stopReason: mode === "completed" ? "user" : null,
    warning: null,
    error: mode === "error" ? "システム音声を取得できませんでした。音声出力先を確認してください。" : null
  };
}

function snapshot(mode: OverlayPreviewMode): OverlayPreviewSnapshot {
  if (mode === "detection") {
    return {
      mode,
      detection,
      status: null,
      controllerMode: false,
      completionMessage: "",
      error: ""
    };
  }
  const nextStatus = status(mode);
  return {
    mode,
    detection: null,
    status: nextStatus,
    controllerMode: true,
    completionMessage: mode === "completed" ? "録音を安全に保存しました。" : "",
    error: nextStatus.error ?? ""
  };
}

export const overlayPreviewRuntime: OverlayPreviewRuntime = {
  changedEvent: "overlay-preview-mode-changed",
  badgeLabel: "PREVIEW",
  snapshot,
  show: (mode) => invoke("show_overlay_preview", { mode }),
  get: () => invoke("get_overlay_preview_mode"),
  close: () => invoke("close_overlay_preview")
};
