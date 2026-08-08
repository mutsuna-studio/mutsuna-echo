export type TranscriptionProviderId = "elevenlabs" | "local";
export type TranscriptionProviderKind = "cloud" | "local";
export type TranscriptionProviderSetup = "apiKey" | "modelDownload";
export type TranscriptionProviderAvailability =
  | "ready"
  | "apiKeyRequired"
  | "modelRequired"
  | "engineUnavailable"
  | "unavailable";

export type TimingGranularity = "token" | "word";

export type TranscriptionCapabilities = {
  timingGranularity: TimingGranularity;
  speakerLabels: boolean;
  confidenceScores: boolean;
  externalDiarization: boolean;
};

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
  capabilities: TranscriptionCapabilities;
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

export type LocalSttModelCatalogEntry = {
  modelId: string;
  displayName: string;
  version: string;
  languageCodes: string[];
  sizeBytes: number;
  installed: boolean;
  downloading: boolean;
  runtimeSupported: boolean;
};

export type LocalSttModelDownloadProgress = {
  modelId: string;
  downloadedBytes: number;
  totalBytes: number;
};

export type LocalVadModelStatus = {
  modelId: string;
  displayName: string;
  version: string;
  sizeBytes: number;
  installed: boolean;
  downloading: boolean;
  runtimeSupported: boolean;
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
