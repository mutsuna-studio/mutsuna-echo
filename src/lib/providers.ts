export type TranscriptionProviderId = "elevenlabs" | "soniox" | "cloudflare" | "local";
export type TranscriptionProviderKind = "cloud" | "local";
export type TranscriptionProviderSetup = "apiKey" | "oauthOrApiKey" | "modelDownload";
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
  contextText: boolean;
  contextTerms: boolean;
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

export type CloudflareAccountOption = { id: string; name: string };

export type CloudflareConnectionStatus = {
  connected: boolean;
  authMethod: "oauth" | "apiToken" | null;
  accountName: string | null;
  needsReauthentication: boolean;
  accountSelectionRequired: boolean;
  accounts: CloudflareAccountOption[];
  oauthConfigured: boolean;
  legacyConfigured: boolean;
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

export type LocalAiRuntimeState =
  | "notInstalled"
  | "downloading"
  | "installing"
  | "ready"
  | "incompatible"
  | "removalPending"
  | "failed";

export type LocalAiRuntimeStatus = {
  state: LocalAiRuntimeState;
  source: "googlePlay" | "githubRelease";
  protocolVersion: number;
  requiredRuntimeVersion: string;
  installedRuntimeVersion: string | null;
  progress: number | null;
  error: string | null;
  sizeBytes: number;
  canDelete: boolean;
};

export type LocalAiRuntimeProgress = {
  state: LocalAiRuntimeState;
  stage: "runtime" | "reazonSpeech" | "sileroVad" | "ready";
  downloadedBytes: number;
  totalBytes: number;
  progress: number;
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

export type LocalDiarizationModelStatus = {
  modelId: string;
  displayName: string;
  version: string;
  sizeBytes: number;
  installed: boolean;
  downloading: boolean;
  runtimeSupported: boolean;
};

export type VadPreset = "softVoice" | "standard" | "noiseReduction";
export type LocalRecognitionMode = "fast" | "accurate";
export type LocalRecognitionSettings = { mode: LocalRecognitionMode };

export const LOCAL_RECOGNITION_MODE_OPTIONS = [
  { value: "fast", label: "高速", description: "Greedy Searchで処理時間を優先します" },
  { value: "accurate", label: "高精度", description: "Beam Searchと短い発話の補完認識を使います" }
] as const;

export const VAD_PRESET_OPTIONS = [
  { value: "softVoice", label: "小声を優先", description: "小さな声を拾いやすくします" },
  { value: "standard", label: "標準", description: "会議音声向けの推奨設定" },
  { value: "noiseReduction", label: "ノイズ抑制を優先", description: "環境音の誤検出を抑えます" }
] as const;

export function isTranscriptionProviderId(value: string): value is TranscriptionProviderId {
  return value === "elevenlabs" || value === "soniox" || value === "cloudflare" || value === "local";
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
  return providers.filter((provider) => provider.ready).map((provider) => ({
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
    ?? ({ elevenlabs: "ElevenLabs", soniox: "Soniox", cloudflare: "Cloudflare Workers AI", local: "ローカルSTT" } as Record<string, string>)[id]
    ?? id;
}
