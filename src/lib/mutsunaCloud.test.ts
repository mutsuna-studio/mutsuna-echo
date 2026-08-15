import { describe, expect, it } from "vitest";

import {
  MUTSUNA_CLOUD_COMMANDS,
  MUTSUNA_CLOUD_DEVICE_VERIFICATION_EVENT,
  MUTSUNA_CLOUD_PRICING_URL,
  describeMutsunaCloudStatus,
  mutsunaCloudAccountStatusLabel
} from "./mutsunaCloud";

describe("Mutsuna Cloud settings state", () => {
  it("uses the native command contract and public pricing page", () => {
    expect(MUTSUNA_CLOUD_COMMANDS).toEqual({
      getStatus: "get_mutsuna_cloud_status",
      connect: "connect_mutsuna_cloud",
      reopenVerification: "reopen_mutsuna_cloud_verification",
      cancelConnection: "cancel_mutsuna_cloud_connection",
      disconnect: "disconnect_mutsuna_cloud",
      purchaseCredits: "purchase_mutsuna_cloud_credits"
    });
    expect(MUTSUNA_CLOUD_PRICING_URL).toBe("https://mutsuna.jp/pricing");
    expect(MUTSUNA_CLOUD_DEVICE_VERIFICATION_EVENT).toBe("mutsuna-cloud-device-verification");
  });

  it("distinguishes disconnected, insufficient-credit, and usable accounts", () => {
    expect(describeMutsunaCloudStatus({ connected: false, canUse: false, availableCredits: null, accountStatus: null }, false).label).toBe("未接続");
    expect(describeMutsunaCloudStatus({ connected: true, canUse: false, availableCredits: "0", accountStatus: "active" }, false)).toMatchObject({ label: "残高不足", tone: "warning" });
    expect(describeMutsunaCloudStatus({ connected: true, canUse: true, availableCredits: "3600", accountStatus: "active" }, false)).toMatchObject({ label: "利用可能", tone: "ready" });
    expect(describeMutsunaCloudStatus({ connected: true, canUse: false, availableCredits: "3600", accountStatus: "suspended" }, false)).toMatchObject({ label: "利用できません", tone: "warning" });
  });

  it("renders account statuses as clear Japanese labels", () => {
    expect(mutsunaCloudAccountStatusLabel("active")).toBe("有効");
    expect(mutsunaCloudAccountStatusLabel("action_required")).toBe("確認が必要");
    expect(mutsunaCloudAccountStatusLabel("suspended")).toBe("一時停止中");
    expect(mutsunaCloudAccountStatusLabel(null)).toBeNull();
  });
});
