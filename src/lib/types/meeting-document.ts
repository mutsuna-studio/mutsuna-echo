export type SourceActor = "ai" | "user" | "import" | "system";
export type SemanticBasis =
  | "explicit"
  | "normalized"
  | "inferred"
  | "user_supplied"
  | "imported"
  | "derived";

export type FieldState = {
  source: SourceActor;
  basis: SemanticBasis;
  locked: boolean;
  updatedAt: string;
  evidenceIds?: string[];
  generationRunId?: string;
};

export type RecordMeta = {
  origin: SourceActor;
  lifecycle: "active" | "deleted";
  lifecycleSource: SourceActor;
  createdAt: string;
  updatedAt: string;
  generationRunId?: string;
  fingerprint?: string;
  fieldStates: Record<string, FieldState>;
};

export type Evidence = {
  evidenceId: string;
  relation: "direct" | "contextual";
  spans: Array<{
    segmentId: string;
    startChar?: number;
    endChar?: number;
    startMs?: number;
    endMs?: number;
  }>;
  quote?: string;
};

export type TemporalExpression = {
  rawText: string;
  resolutionStatus: "normalized" | "ambiguous" | "unresolved";
  intervalStart?: string;
  intervalEnd?: string;
  precision: "minute" | "hour" | "day" | "week" | "month" | "quarter" | "year" | "unknown";
  basis: "explicit_exact" | "explicit_relative" | "inferred";
  resolvedAgainst?: string;
  timeZone?: string;
};

export type MeetingRecord = {
  recordMeta: RecordMeta;
  evidenceIds: string[];
};

export type MeetingDocument = {
  schemaVersion: "1.0.0";
  documentType: "meeting";
  documentId: string;
  revision: number;
  createdAt: string;
  updatedAt: string;
  sourceTranscript: { documentId: string; revision: number; contentHash: string };
  meeting: {
    title: string;
    meetingType: "internal" | "client" | "sales" | "interview" | "standup" | "retrospective" | "workshop" | "other" | "unknown";
    purpose?: string;
    startedAt?: string;
    endedAt?: string;
    timeZone: string;
    languageCodes: string[];
    organizerParticipantId?: string;
    externalRefs: Array<{ system: string; kind: string; externalId: string; url?: string }>;
  };
  participants: Array<{
    participantId: string;
    displayName: string;
    kind: "person" | "group" | "unknown";
    attendance: "present" | "remote" | "absent" | "unknown";
    role?: string;
    organization?: string;
    aliases: string[];
    externalRefs: Array<{ system: string; kind: string; externalId: string; url?: string }>;
    recordMeta: RecordMeta;
  }>;
  speakerMappings: Array<MeetingRecord & { mappingId: string; speakerId: string; participantId: string; status: "confirmed" | "inferred" }>;
  summary: { oneLine?: string; overview: string; keyPoints: Array<MeetingRecord & { keyPointId: string; text: string }> };
  topics: Array<MeetingRecord & { topicId: string; title: string; summary?: string; order: number; status: "discussed" | "open" | "deferred"; participantIds: string[] }>;
  decisions: Array<MeetingRecord & { decisionId: string; statement: string; rationale?: string; status: "active" | "tentative" | "superseded" | "revoked"; topicIds: string[]; ownerParticipantIds: string[]; supersedesDecisionIds: string[] }>;
  actionItems: Array<MeetingRecord & { actionItemId: string; title: string; description?: string; status: "open" | "in_progress" | "blocked" | "done" | "cancelled"; assigneeParticipantIds: string[]; due?: TemporalExpression; priority?: "low" | "medium" | "high" | "urgent"; topicIds: string[]; relatedDecisionIds: string[]; blockerIssueIds: string[] }>;
  openIssues: Array<MeetingRecord & { issueId: string; title: string; description?: string; status: "open" | "resolved" | "deferred" | "cancelled"; ownerParticipantIds: string[]; due?: TemporalExpression; topicIds: string[]; relatedDecisionIds: string[]; relatedActionItemIds: string[]; resolution?: { text: string; evidenceIds: string[] } }>;
  questions: Array<MeetingRecord & { questionId: string; text: string; status: "open" | "answered" | "deferred"; askedByParticipantId?: string; directedToParticipantIds: string[]; answer?: { text: string; answeredByParticipantIds: string[]; evidenceIds: string[] }; topicIds: string[]; relatedIssueIds: string[] }>;
  notes: Array<MeetingRecord & { noteId: string; title?: string; body: string; topicIds: string[] }>;
  evidence: Evidence[];
  generationRuns: Array<{ runId: string; mode: "initial" | "regenerate" | "partial"; createdAt: string; provider: string; model: string; promptId: string; promptVersion: string; sourceTranscriptRevision: number; sourceTranscriptHash: string; outputSchemaVersion: "1.0.0"; warnings?: string[] }>;
  latestGenerationRunId?: string;
  qualityChecks?: Array<{
    checkId: string;
    createdAt: string;
    provider: string;
    model: string;
    generationRunId?: string;
    status: "passed" | "warning" | "failed";
    findings: Array<{
      code: "contradiction" | "ambiguous" | "broken_relation" | "other";
      message: string;
      relatedRecordIds: string[];
    }>;
    title?: string;
    titleApplied: boolean;
    error?: string;
  }>;
  latestQualityCheckId?: string;
  editorial: { fieldStates: Record<string, FieldState> };
};
