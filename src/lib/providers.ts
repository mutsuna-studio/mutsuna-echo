export type TranscriptionProviderId = "elevenlabs" | "local";
export type TranscriptionProviderKind = "cloud" | "local";
export type TranscriptionProviderSetup = "apiKey" | "modelDownload";
export type TranscriptionProviderAvailability =
  | "ready"
  | "apiKeyRequired"
  | "modelRequired"
  | "engineUnavailable"
  | "unavailable";

export type TranscriptionProviderDefinition = {
  id: TranscriptionProviderId;
  label: string;
  kind: TranscriptionProviderKind;
  setup: TranscriptionProviderSetup;
  availability: TranscriptionProviderAvailability;
  ready: boolean;
  configured: boolean;
  modelId: string | null;
  modelLabel: string;
  capabilitySummary: string;
  statusMessage: string;
  pricingUsdPerHour: number | null;
  pricingVerifiedOn: string | null;
};

export type InstalledLocalSttModel = {
  modelId: string;
  version: string;
  engine: string;
  displayName: string;
  languageCodes: string[];
  sizeBytes: number;
};

export function isTranscriptionProviderId(value: string): value is TranscriptionProviderId {
  return value === "elevenlabs" || value === "local";
}

export function getTranscriptionProvider(
  providers: readonly TranscriptionProviderDefinition[],
  id: TranscriptionProviderId
): TranscriptionProviderDefinition | null {
  return providers.find((provider) => provider.id === id) ?? providers[0] ?? null;
}

export function transcriptionProviderOptions(
  providers: readonly TranscriptionProviderDefinition[]
) {
  return providers.map((provider) => ({
    value: provider.id,
    label: provider.label,
    description: provider.modelLabel
  }));
}

export function transcriptionProviderLabel(
  id: string,
  providers: readonly TranscriptionProviderDefinition[] = []
): string {
  return providers.find((provider) => provider.id === id)?.label
    ?? ({ elevenlabs: "ElevenLabs", local: "ローカルSTT" } as Record<string, string>)[id]
    ?? id;
}
