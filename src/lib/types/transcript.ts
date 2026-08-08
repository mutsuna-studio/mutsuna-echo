export type TranscriptSegment = {
  speaker: string;
  startMs: number;
  endMs: number;
  text: string;
};

export type Transcript = {
  provider: string;
  model: string;
  language: string;
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
};

export type TranscriptionUsage = {
  availableDurationMs: number | null;
  usedDurationMs: number | null;
  tier: string | null;
  resetsAtUnix: number | null;
  warning: string | null;
};
