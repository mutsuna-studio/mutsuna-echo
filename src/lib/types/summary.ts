export type SummaryModelDefinition = {
  id: string;
  label: string;
  description: string;
  isDefault: boolean;
};

export type SummaryProviderDefinition = {
  id: string;
  label: string;
  description: string;
  ready: boolean;
  statusMessage: string;
  models: SummaryModelDefinition[];
};

export type SummaryAgentInstallStatus = {
  id: string;
  label: string;
  version: string;
  installed: boolean;
  external: boolean;
  installable: boolean;
  statusMessage: string;
};

export type SummarySourceSelection = {
  key: string;
  kind: "keyPoint" | "topic" | "decision" | "actionItem" | "openIssue" | "question" | "note";
  text: string;
  sourceSegmentIds: string[];
};

export type SummaryProgress = {
  meetingId: string;
  completedSteps: number;
  totalSteps: number;
  stage: "summarizing" | "waiting" | "streaming" | "retrying" | "merging" | "mechanically-repairing" | "repairing" | "checking" | "complete";
  activeStep?: number;
  attempt?: number;
  maxAttempts?: number;
  retryDelaySeconds?: number;
  receivedBytes?: number;
  activityKind?: "thought" | "plan" | "tool";
  activityText?: string;
  activityStatus?: string;
};

export type GenerationAttemptSummary = {
  attemptId: string;
  transcriptionId: string;
  sourceRevision: number;
  provider: string;
  model: string;
  startedAt: string;
  status: "generating" | "failed" | "completed";
  stage: string;
  error: string | null;
  canRevalidate: boolean;
};
