import { render } from "svelte/server";
import { describe, expect, it } from "vitest";

import MutsunaCloudSettings from "./MutsunaCloudSettings.svelte";

const noop = async () => {};

function renderSettings(
  status: {
    connected: boolean;
    canUse: boolean;
    availableCredits: string | null;
    accountStatus: string | null;
  } | null,
  options: { connecting?: boolean; purchasing?: boolean; verificationCode?: string | null } = {}
) {
  return render(MutsunaCloudSettings, {
    props: {
      status,
      loading: false,
      connecting: options.connecting ?? false,
      verificationCode: options.verificationCode ?? null,
      cancelling: false,
      disconnecting: false,
      purchasing: options.purchasing ?? false,
      busy: options.connecting === true || options.purchasing === true,
      onConnect: noop,
      onReopenVerification: noop,
      onCancelConnection: noop,
      onDisconnect: noop,
      onPurchase: noop
    }
  }).body;
}

describe("Mutsuna Cloud connection settings", () => {
  it("shows the API-key-free disconnected flow", () => {
    const body = renderSettings({ connected: false, canUse: false, availableCredits: null, accountStatus: null });

    expect(body).toContain("APIキー不要・クレジット制");
    expect(body).toContain("未接続");
    expect(body).toContain("Mutsuna Cloudに接続");
    expect(body).toContain("60分パック（3,600クレジット）を購入");
    expect(body).toContain("クレジットを購入するには、先にMutsuna Cloudへ接続してください");
    expect(body).toContain('href="https://mutsuna.jp/pricing"');
    expect(body).not.toMatch(/device.?code|token/i);
  });

  it("shows the short-lived verification code while browser authentication is in progress", () => {
    const body = renderSettings(
      { connected: false, canUse: false, availableCredits: null, accountStatus: null },
      { connecting: true, verificationCode: "TRNR-MQSL" }
    );

    expect(body).toContain("ブラウザで認証中…");
    expect(body).toContain("TRNR-MQSL");
    expect(body).toContain("一致する場合だけ");
    expect(body).toContain("ブラウザをもう一度開く");
    expect(body).toContain("接続をキャンセル");
  });

  it("shows connected credit states and the 60-minute pack purchase action", () => {
    const insufficient = renderSettings({ connected: true, canUse: false, availableCredits: "0", accountStatus: "active" });
    const usable = renderSettings({ connected: true, canUse: true, availableCredits: "3600", accountStatus: "active" });
    const purchasing = renderSettings(
      { connected: true, canUse: true, availableCredits: "3600", accountStatus: "active" },
      { purchasing: true }
    );

    expect(insufficient).toContain("残高不足");
    expect(insufficient).toContain("0 クレジット");
    expect(insufficient).toContain("60分パック（3,600クレジット）を購入");
    expect(usable).toContain("接続済み");
    expect(usable).toContain("利用可能");
    expect(usable).toContain("3600 クレジット");
    expect(usable).toContain("切断");
    expect(purchasing).toContain("購入画面を開いています…");
  });
});
