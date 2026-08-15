import type { MutsunaCloudStatus } from "./providers";

export const MUTSUNA_CLOUD_PRICING_URL = "https://mutsuna.jp/pricing";
export const MUTSUNA_CLOUD_DEVICE_VERIFICATION_EVENT = "mutsuna-cloud-device-verification";

export type MutsunaCloudDeviceVerification = {
  readonly userCode: string;
};

export const MUTSUNA_CLOUD_COMMANDS = Object.freeze({
  getStatus: "get_mutsuna_cloud_status",
  connect: "connect_mutsuna_cloud",
  reopenVerification: "reopen_mutsuna_cloud_verification",
  cancelConnection: "cancel_mutsuna_cloud_connection",
  disconnect: "disconnect_mutsuna_cloud",
  purchaseCredits: "purchase_mutsuna_cloud_credits"
});

export type MutsunaCloudAvailability = {
  readonly label: string;
  readonly detail: string;
  readonly tone: "muted" | "warning" | "ready";
};

export function describeMutsunaCloudStatus(
  status: MutsunaCloudStatus | null,
  loading: boolean
): MutsunaCloudAvailability {
  if (loading || status === null) {
    return { label: "接続状態を確認中", detail: "Mutsuna Cloudの利用状況を確認しています。", tone: "muted" };
  }
  if (!status.connected) {
    return { label: "未接続", detail: "ブラウザで認証すると、APIキーなしでクラウドAIを利用できます。", tone: "muted" };
  }
  if (!status.canUse) {
    if (status.accountStatus !== null && status.accountStatus !== "active") {
      return { label: "利用できません", detail: "Mutsuna Cloudのアカウント状態を確認してください。", tone: "warning" };
    }
    return { label: "残高不足", detail: "クレジットを追加すると、すぐに文字起こしを再開できます。", tone: "warning" };
  }
  return { label: "利用可能", detail: "Mutsuna Cloudで文字起こしできます。", tone: "ready" };
}

export function mutsunaCloudAccountStatusLabel(status: string | null): string | null {
  if (status === null) return null;
  return ({
    active: "有効",
    action_required: "確認が必要",
    suspended: "一時停止中",
    closed: "終了"
  } as Record<string, string>)[status] ?? status;
}
