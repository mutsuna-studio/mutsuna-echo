import type { MeetingDetection } from "./meeting";
import type { RecordingStatus } from "./recording";

export type OverlayPreviewMode =
  | "detection"
  | "recording"
  | "finalizing"
  | "completed"
  | "error";

export const OVERLAY_PREVIEW_OPTIONS = [
  { value: "detection", label: "会議検出" },
  { value: "recording", label: "録音中" },
  { value: "finalizing", label: "保存中" },
  { value: "completed", label: "保存完了" },
  { value: "error", label: "エラー" }
] as const satisfies ReadonlyArray<{ value: OverlayPreviewMode; label: string }>;

export type OverlayPreviewSnapshot = {
  mode: OverlayPreviewMode;
  detection: MeetingDetection | null;
  status: RecordingStatus | null;
  controllerMode: boolean;
  completionMessage: string;
  error: string;
};

export type OverlayPreviewRuntime = {
  changedEvent: string;
  badgeLabel: string;
  snapshot: (mode: OverlayPreviewMode) => OverlayPreviewSnapshot;
  show: (mode: OverlayPreviewMode) => Promise<void>;
  get: () => Promise<OverlayPreviewMode | null>;
  close: () => Promise<void>;
};
