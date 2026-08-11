import { invoke } from "@tauri-apps/api/core";

export type AndroidUpdatePhase =
  | "idle"
  | "latest"
  | "available"
  | "starting"
  | "downloading"
  | "downloaded"
  | "installing"
  | "failed";

export interface AndroidUpdateStatus {
  phase: AndroidUpdatePhase;
  checking: boolean;
  availableVersionCode: number | null;
  updatePriority: number;
  flexibleAllowed: boolean;
  immediateAllowed: boolean;
  bytesDownloaded: number;
  totalBytes: number;
  error: string | null;
}

export const isAndroid = /Android/i.test(navigator.userAgent);

export function getAndroidUpdateStatus(): Promise<AndroidUpdateStatus> {
  return invoke("get_android_update_status");
}

export function checkAndroidUpdate(): Promise<AndroidUpdateStatus> {
  return invoke("check_android_update");
}

export function startAndroidUpdate(): Promise<AndroidUpdateStatus> {
  return invoke("start_android_update");
}

export function completeAndroidUpdate(): Promise<AndroidUpdateStatus> {
  return invoke("complete_android_update");
}

export async function waitForAndroidUpdateCheck(
  initial: AndroidUpdateStatus,
  timeoutMs = 12_000
): Promise<AndroidUpdateStatus> {
  let status = initial;
  const deadline = Date.now() + timeoutMs;
  while (status.checking && Date.now() < deadline) {
    await new Promise((resolve) => window.setTimeout(resolve, 350));
    status = await getAndroidUpdateStatus();
  }
  return status;
}
