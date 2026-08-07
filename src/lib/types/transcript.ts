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

export type SelectedAudioFile = {
  name: string;
  sizeBytes: number;
  durationMs: number;
  estimatedCostUsd: number;
  pricingRateUsdPerHour: number;
  pricingVerifiedOn: string;
};
