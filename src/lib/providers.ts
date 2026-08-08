export type TranscriptionProviderId = "elevenlabs";

export type TranscriptionProviderDefinition = {
  id: TranscriptionProviderId;
  label: string;
  modelId: string;
  modelLabel: string;
  capabilitySummary: string;
  pricingLabel: string;
};

export const transcriptionProviders: readonly TranscriptionProviderDefinition[] = [
  {
    id: "elevenlabs",
    label: "ElevenLabs",
    modelId: "scribe_v2",
    modelLabel: "Scribe v2",
    capabilitySummary: "日本語・話者分離・単語タイムスタンプ",
    pricingLabel: "公開時間単価"
  }
];

export const transcriptionProviderOptions = transcriptionProviders.map((provider) => ({
  value: provider.id,
  label: provider.label,
  description: provider.modelLabel
}));

export function isTranscriptionProviderId(value: string): value is TranscriptionProviderId {
  return transcriptionProviders.some((provider) => provider.id === value);
}

export function getTranscriptionProvider(id: TranscriptionProviderId): TranscriptionProviderDefinition {
  return transcriptionProviders.find((provider) => provider.id === id) ?? transcriptionProviders[0];
}

export function transcriptionProviderLabel(id: string): string {
  return transcriptionProviders.find((provider) => provider.id === id)?.label ?? id;
}
