export type TranscriptionProviderId = "elevenlabs" | "soniox" | "local";
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

export type VadPreset = "softVoice" | "standard" | "noiseReduction";

export const VAD_PRESET_OPTIONS = [
  { value: "softVoice", label: "小声を優先", description: "小さな声を拾いやすくします" },
  { value: "standard", label: "標準", description: "会議音声向けの推奨設定" },
  { value: "noiseReduction", label: "ノイズ抑制を優先", description: "環境音の誤検出を抑えます" }
] as const;

export function isTranscriptionProviderId(value: string): value is TranscriptionProviderId {
  return value === "elevenlabs" || value === "soniox" || value === "local";
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
    label: provider.modelLabel,
    description: `${provider.label} · ${provider.kind === "local" ? "端末内で処理" : "クラウドで処理"}`
  }));
}

export function transcriptionProviderLabel(
  id: string,
  providers: readonly TranscriptionProviderDefinition[] = []
): string {
  return providers.find((provider) => provider.id === id)?.label
    ?? ({ elevenlabs: "ElevenLabs", soniox: "Soniox", local: "ローカルSTT" } as Record<string, string>)[id]
    ?? id;
}
