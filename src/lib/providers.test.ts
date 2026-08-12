import { describe, expect, it } from "vitest";

import {
  getTranscriptionProvider,
  isTranscriptionProviderId,
  transcriptionProviderLabel,
  transcriptionProviderOptions,
  type TranscriptionProviderDefinition
} from "./providers";

const mutsunaCloud: TranscriptionProviderDefinition = {
  id: "mutsunaCloud",
  label: "Mutsuna Cloud",
  kind: "cloud",
  setup: "cloudAccount",
  availability: "ready",
  ready: true,
  configured: true,
  modelId: "mutsuna-stt-standard-v1",
  modelLabel: "Mutsuna Cloud 文字起こし",
  capabilitySummary: "APIキー不要・クレジット制",
  capabilities: {
    timingGranularity: "word",
    speakerLabels: false,
    confidenceScores: false,
    externalDiarization: true,
    contextText: true,
    contextTerms: true
  },
  statusMessage: "利用可能",
  pricingUsdPerHour: null,
  pricingVerifiedOn: null
};

describe("Mutsuna Cloud provider", () => {
  it("accepts and labels the provider id", () => {
    expect(isTranscriptionProviderId("mutsunaCloud")).toBe(true);
    expect(transcriptionProviderLabel("mutsunaCloud")).toBe("Mutsuna Cloud");
    expect(getTranscriptionProvider([mutsunaCloud], "mutsunaCloud")).toEqual(mutsunaCloud);
  });

  it("appears in transcription selection only while usable", () => {
    expect(transcriptionProviderOptions([mutsunaCloud])).toEqual([
      {
        value: "mutsunaCloud",
        label: "Mutsuna Cloud 文字起こし",
        description: "Mutsuna Cloud · クラウドで処理"
      }
    ]);
    expect(transcriptionProviderOptions([{ ...mutsunaCloud, ready: false, availability: "unavailable" }])).toEqual([]);
  });
});
