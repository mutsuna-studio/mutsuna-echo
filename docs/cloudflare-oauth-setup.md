# Cloudflare OAuth setup

Mutsuna Echo は Cloudflare の Public OAuth Client として、Authorization Code Flow + PKCE (`S256`) でユーザー所有アカウントへ接続します。Client Secret や Mutsuna 側の中継サーバーは使用しません。

根拠となる公式資料:

- [Create your OAuth client](https://developers.cloudflare.com/fundamentals/oauth/create-an-oauth-client/)
- [Integrate your OAuth client with Cloudflare](https://developers.cloudflare.com/fundamentals/oauth/integrate-with-cloudflare/)
- [Authorizing an application](https://developers.cloudflare.com/fundamentals/oauth/authorizing-an-application/)
- [Workers AI: Execute AI model](https://developers.cloudflare.com/api/resources/ai/methods/run/)

## Cloudflare Dashboard で作成する Client

Cloudflare Dashboard の対象アカウントで **Manage Account > OAuth clients > Create client** を開き、次を設定します。

| 項目 | 値 |
| --- | --- |
| Client name | `Mutsuna Echo` |
| Client URL | Mutsuna Echo の公式 HTTPS URL |
| Response type | `code` |
| Grant types | `authorization_code`, `refresh_token` |
| Token endpoint authentication method | `none` |
| PKCE | Required, `S256` |
| Redirect URL | `http://127.0.0.1:8976/oauth/cloudflare/callback` |
| Dashboard scopes | **AI & Machine Learning** → **Workers AI** → **Read** |
| Authorization request scopes | `ai.read offline_access` |
| Client Secret | 使用しない |

ダッシュボードの権限一覧では `account:read`、`ai:read`、`offline_access` という文字列を探しません。`Workers AI` の `Read` だけを選択します。`Edit` は不要です。

アプリの認可リクエストでは、選択した権限の scope ID である `ai.read` と、refresh token を要求する標準スコープ `offline_access` を送信します。`offline_access` はダッシュボードの権限項目ではありません。Workers AI の実行 API は公式 API reference 上 `Workers AI Read` または `Workers AI Write` を受け付けるため、Mutsuna Echo は最小権限の read scope だけを要求します。

OAuth の consent 画面自体が許可対象アカウントを選択します。認証後、Mutsuna Echo は `GET /client/v4/accounts` で許可済みアカウントだけを列挙します。1件なら自動選択し、複数ならアカウント名から選択させます。OAuth 利用者に Account ID の手入力は求めません。

## Public Client と publisher verification

作成直後の Client は private です。公式配布で任意の Cloudflare ユーザーに利用させるには、Client name、logo、Client URL、scopes を設定し、Client URL のドメイン所有確認を完了してから visibility を public に変更します。

Cloudflare が表示する `cloudflare_oauth_client_publisher=...` を含む TXT record を、指示された名前で公式ドメインへ追加します。Public への変更は元へ戻せないため、redirect URL と公開情報を確認してから実行してください。

## Client ID のビルド設定

Client ID は公開 identifier であり secret ではありません。ビルド時に次の環境変数を設定します。

```powershell
$env:MUTSUNA_CLOUDFLARE_OAUTH_CLIENT_ID = "Cloudflare Dashboard の Client ID"
pnpm tauri build
```

Rust の `option_env!` で compile-time public configuration として取り込まれます。未設定ビルドは panic せず、設定画面に「このビルドにはCloudflare OAuth Client IDが設定されていません」と表示します。fork は独自の Public OAuth Client を作成し、同じ環境変数へ独自 Client ID を設定してください。Client Secret は追加しないでください。

### GitHub Actions の本番リリース

リポジトリの **Settings > Secrets and variables > Actions > Variables** で、次の Repository variable を登録します。

| Name | Value |
| --- | --- |
| `MUTSUNA_CLOUDFLARE_OAUTH_CLIENT_ID` | Cloudflare Dashboard で発行された本番用 Client ID |

Client ID は公開 identifier のため Variable として管理し、Secret や Client Secret は使用しません。リリースワークフローはこの値をWindows、macOS、Androidの全ビルドへ渡します。値が未設定または空白だけの場合は、OAuthを利用できない成果物を公開しないよう、バージョン検証の段階でリリース全体を失敗させます。通常CIのリリースキャッシュ生成も同じ値を使用し、未設定時は失敗します。

## Callback と platform 設定

Windows、macOS、Android はすべて固定 loopback callback を使います。

```text
http://127.0.0.1:8976/oauth/cloudflare/callback
```

これは Cloudflare 公式 Wrangler が採用する localhost callback と同じ構成で、Cloudflare Dashboard へ exact redirect URL として登録します。認可開始前に Rust/native layer が loopback listener を bind し、システムブラウザを開きます。認可コード、state、PKCE verifier は Svelte/WebView を通りません。callback の state を検証した後、同じ redirect URI と verifier で token endpoint へ交換します。

- Windows: 追加の URI scheme 登録は不要です。
- macOS: bundle の URL type や universal link は不要です。
- Android: Manifest の `singleTask` activity を変更せず、intent filter の追加も不要です。ブラウザの `127.0.0.1` は同じ端末上の native listener を指します。
- Development: release と同じ callback を使います。port 8976 を Wrangler 等が使用中なら、それを停止してから接続します。

loopback callback のため、認可コードを受け取る hosted backend、custom URI scheme、universal/app link、`tauri-plugin-deep-link` は不要です。システムブラウザには完了ページを返し、ユーザーは Mutsuna Echo へ戻ります。

## Token endpoint と保存

Cloudflare 公式 endpoint:

- Authorization: `https://dash.cloudflare.com/oauth2/auth`
- Token: `https://dash.cloudflare.com/oauth2/token`
- Revoke: `https://dash.cloudflare.com/oauth2/revoke`
- Accounts API: `https://api.cloudflare.com/client/v4/accounts`

access token、refresh token、expiry、OAuth 用 Account ID・表示名・候補一覧は既存 native credential store に保存します。Windows は DPAPI、Android は Android Keystore、macOS 等は OS keyring です。localStorage や通常設定ファイルには保存しません。

Cloudflare は token response の `expires_in` を返すため、その値から expiry を計算します。API 実行直前に expiry を確認し、60秒以内に期限切れとなる場合は refresh します。refresh は process 内の async mutex で single-flight 化します。refresh response が refresh token をローテーションした場合は新しい値を、refresh token が省略された場合は既存値を維持し、access token・refresh token・expiry を read-back 検証付き transaction で保存します。

access token lifetime や refresh token lifetime は固定値として実装へ埋め込みません。Cloudflare の token response と失効結果を正とします。
