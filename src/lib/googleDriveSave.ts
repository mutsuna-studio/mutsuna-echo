export const LARGE_AUDIO_BYTES = 500_000_000;
export type DriveLimits = { retryAt: number; quotaPaused: boolean };
export function audioSelectedByDefault(available: boolean, size: number | null, unchanged = false): boolean {
  return available && !unchanged && size !== null && size < LARGE_AUDIO_BYTES;
}
export function waitSeconds(limits: DriveLimits, now: number): number {
  return Math.max(0, Math.ceil((limits.retryAt - now) / 1000));
}
