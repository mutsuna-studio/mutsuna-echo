export type TranscriptSegment = {
  speaker: string;
  startMs: number;
  endMs: number;
  text: string;
};

export type EditableTranscriptSegment = TranscriptSegment & {
  segmentId: string;
  originalText: string;
  edited: boolean;
};

export type TranscriptSpeakerLabel = {
  speaker: string;
  label: string;
  edited: boolean;
};

export type TokenSpeakerSource = "provider" | "diarization" | "channel" | "user";
export type TokenTimeSource = "provider" | "alignment" | "inferred" | "user";

export type TranscriptToken = {
  text: string;
  startMs: number | null;
  endMs: number | null;
  startTimeSource: TokenTimeSource | null;
  endTimeSource: TokenTimeSource | null;
  speaker: string | null;
  speakerSource: TokenSpeakerSource | null;
  confidence: number | null;
  utteranceId?: number | null;
};

export type Transcript = {
  provider: string;
  model: string;
  language: string;
  tokens: TranscriptToken[];
  segments: TranscriptSegment[];
};

export type EditableTranscript = Omit<Transcript, "segments"> & {
  speakerLabels: TranscriptSpeakerLabel[];
  segments: EditableTranscriptSegment[];
};

export type TranscriptionRunSummary = {
  transcriptionId: string;
  sequence: number;
  createdAt: string;
  updatedAt: string;
  provider: string;
  model: string;
  language: string;
  edited: boolean;
  costUsd: string | null;
};

export type TranscriptionRunDetail = {
  transcriptionId: string;
  sequence: number;
  createdAt: string;
  updatedAt: string;
  revision: number;
  edited: boolean;
  costUsd: string | null;
  transcript: EditableTranscript;
};

export type TranscriptionHistory = {
  runs: TranscriptionRunSummary[];
  selectedTranscriptionId: string | null;
};

export type TranscriptSaveState = "saved" | "unsaved" | "saving" | "notSaved" | "error";

export type TranscriptionResult = {
  transcript: Transcript;
  run: TranscriptionRunDetail | null;
  persistenceWarning: string | null;
  diarizationWarning: string | null;
};

export type SelectedAudioFile = {
  meetingId: string;
  name: string;
  sizeBytes: number;
  durationMs: number;
  playbackUrl: string;
};

export type AudioWaveform = {
  meetingId: string;
  points: number;
  peaks: number[];
};

export type AudioWaveformProgress = {
  meetingId: string;
  peaks: number[];
  completedPoints: number;
};

export type AudioSeekRequest = {
  meetingId: string;
  requestId: number;
  positionMs: number;
  autoplay?: boolean;
  pause?: boolean;
};

export type TranscriptSegmentTextChange = {
  segmentId: string;
  text: string;
};

export type TranscriptFormattingResult = {
  transcriptionId: string;
  sourceRevision: number;
  method: "mechanical" | "mechanicalAndLlm";
  provider: string | null;
  model: string | null;
  changes: TranscriptSegmentTextChange[];
  warning: string | null;
};

export type TranscriptionSession = {
  selectedAudio: SelectedAudioFile | null;
  transcribing: boolean;
  diarizing: boolean;
  progress: TranscriptionProgress | null;
  backgroundError: string | null;
};

export type TranscriptionStage = "preparing" | "detectingSpeech" | "transcribing" | "recoveringSpeech" | "finalizing" | "complete";

export type TranscriptionProgress = {
  stage: TranscriptionStage;
  completedChunks: number;
  totalChunks: number | null;
  overallProgress?: number | null;
};

export type LocalDiarizationStage =
  | "loadingModel"
  | "decodingAudio"
  | "diarizingChunks"
  | "stitchingSpeakers"
  | "finalizing";

export type LocalDiarizationProgress = {
  stage: LocalDiarizationStage;
  completedChunks: number;
  totalChunks: number | null;
  processedMs: number;
  totalMs: number | null;
};

export type TranscriptionUsage = {
  availableDurationMs: number | null;
  usedDurationMs: number | null;
  tier: string | null;
  resetsAtUnix: number | null;
  warning: string | null;
};

export type SonioxUsage = {
  monthlyCostUsd: string;
  periodStart: string;
  fetchedAt: string;
};

export type CloudflareUsage = {
  estimatedCostUsd: string;
  usedDurationMs: number;
  estimatedNeurons: number;
  transcriptionCount: number;
  textGenerationCount: number;
  periodStart: string;
  dailyUsedDurationMs: number;
  dailyEstimatedNeurons: number;
  dailyTranscriptionCount: number;
  dailyTextGenerationCount: number;
  dailyFreeAllocationNeurons: number;
  dailyRemainingNeurons: number;
  dailyUsagePercent: number;
  dailyPeriodStart: string;
  dailyResetsAt: string;
  fetchedAt: string;
};

export type TranscriptionContext = {
  background: string;
  terms: string[];
  corrections: TextCorrection[];
};

export type TextCorrection = { from: string; to: string };

export type GlobalTranscriptionContextSettings = TranscriptionContext & {
  contextEnabled: boolean;
};

export type MeetingTranscriptionContext = TranscriptionContext & {
  useGlobal: boolean;
};

export type ContextSaveState = "saved" | "unsaved" | "saving" | "error";
