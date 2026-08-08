export type PendingAction = {
  schemaVersion: number;
  id: string;
  kind: "transcribeMeeting";
  meetingId: string;
  createdAt: string;
};
