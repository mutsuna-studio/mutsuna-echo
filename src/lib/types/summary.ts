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

export type SummaryReference = {
  text: string;
  sourceSegmentIds: string[];
};

export type SummaryActionItem = SummaryReference & {
  assignee: string | null;
  due: string | null;
};

export type SummarySourceSelection = {
  key: string;
  kind: "decision" | "actionItem";
  text: string;
  sourceSegmentIds: string[];
};

export type MeetingSummary = {
  schemaVersion: number;
  summaryId: string;
  meetingId: string;
  transcriptionId: string;
  sourceRevision: number;
  provider: string;
  model: string;
  generatedAt: string;
  content: {
    overview: string;
    decisions: SummaryReference[];
    actionItems: SummaryActionItem[];
  };
};

export type SummaryStatus = {
  summary: MeetingSummary | null;
  transcriptionId: string | null;
  currentRevision: number | null;
  stale: boolean;
};

export type SummaryProgress = {
  meetingId: string;
  completedSteps: number;
  totalSteps: number;
  stage: "summarizing" | "merging" | "complete";
};
