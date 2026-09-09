import { describe, it, expect } from "vitest";
import { audioSelectedByDefault, waitSeconds } from "./googleDriveSave";
describe("Drive save safeguards", () => {
  it("defaults large, missing and unknown audio to unchecked", () => {
    expect(audioSelectedByDefault(true, 499_999_999)).toBe(true);
    expect(audioSelectedByDefault(true, 100, true)).toBe(false);
    expect(audioSelectedByDefault(true, 100, false)).toBe(true);
    expect(audioSelectedByDefault(true, 500_000_000)).toBe(false);
    expect(audioSelectedByDefault(true, 700_000_000)).toBe(false);
    expect(audioSelectedByDefault(true, null)).toBe(false);
    expect(audioSelectedByDefault(false, 100)).toBe(false);
  });
  it("keeps saving disabled until the native deadline expires", () => {
    const limits = { retryAt: 11_000, quotaPaused: false };
    expect(waitSeconds(limits, 1000)).toBe(10);
    expect(waitSeconds(limits, 10_999)).toBe(1);
    expect(waitSeconds(limits, 11_000)).toBe(0);
    expect(waitSeconds(limits, 12_000)).toBe(0);
  });
});
