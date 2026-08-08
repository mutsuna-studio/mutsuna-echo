export type TranscriptSegment = {
  speaker: string;
  startMs: number;
  endMs: number;
  text: string;
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
};

export type Transcript = {
  provider: string;
  model: string;
  language: string;
  tokens: TranscriptToken[];
  segments: TranscriptSegment[];
};

export type TranscriptionResult = {
  transcript: Transcript;
  persistenceWarning: string | null;
};

export type SelectedAudioFile = {
  meetingId: string;
  name: string;
  sizeBytes: number;
  durationMs: number;
};

export type TranscriptionSession = {
  selectedAudio: SelectedAudioFile | null;
  transcribing: boolean;
  progress: TranscriptionProgress | null;
};

export type TranscriptionStage = "preparing" | "detectingSpeech" | "transcribing";

export type TranscriptionProgress = {
  stage: TranscriptionStage;
  completedChunks: number;
  totalChunks: number | null;
};

export type TranscriptionUsage = {
  availableDurationMs: number | null;
  usedDurationMs: number | null;
  tier: string | null;
  resetsAtUnix: number | null;
  warning: string | null;
};
