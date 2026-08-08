export type MeetingProvider = "zoom" | "googleMeet" | "microsoftTeams";

export type MeetingDetection = {
  provider: MeetingProvider;
  providerLabel: string;
  windowTitle: string;
  detectedAtUnixMs: number;
};
