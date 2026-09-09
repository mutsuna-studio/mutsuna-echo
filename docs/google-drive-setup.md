# Google Drive連携の設定

Mutsuna Echoからユーザー自身のDriveへ直接保存する。自社サーバーは経由しない。
音声、会議ノートと文字起こしを2タブにまとめたGoogleドキュメント、構造化JSONを会議フォルダーへ書き出す。
双方向同期は行わない。再保存時は必ず上書き・別版保存を選ぶ。

## Reserveとの分離

推奨構成はMutsuna Echo専用のGoogle Cloudプロジェクト。Reserveの既存クライアントは使用しない。
同一プロジェクトに複数プラットフォームのOAuthクライアントを作成することは可能だが、ブランド名、同意画面、公開設定を共有する。
同一ブランドの製品群として運用する場合は共有も選択肢になる。EchoとReserveを別製品として表示する場合はプロジェクトを分離する。
開発用と公開用のプロジェクトも分離する。

## Google Cloud / Google Auth Platform

1. Echo用プロジェクトを作成する（例: 表示名 `Mutsuna Echo Development`）。
2. APIライブラリで **Google Drive API** と **Google Docs API** を有効化する。
3. Google Auth PlatformのBrandingにアプリ名 `Mutsuna Echo`、サポートメール、開発者連絡先を設定する。公開時はホームページとプライバシーポリシーも設定する。
4. Audienceを用途に合わせて設定する。一般利用者向けはExternal。開発時はTestingにし、テストアカウントを登録する。
5. Data Accessで `https://www.googleapis.com/auth/drive.file` を追加する。Drive全体の権限や独立した `documents` スコープは要求しない。
6. Clientsで以下のクライアントを作成する。

| クライアント | 種類 | 用途 |
| --- | --- | --- |
| Echo Desktop | Desktop app | Windows / macOS。必要ならOSごとに分けて管理できる |
| Echo Android Debug | Android | パッケージ `jp.mutsuna.echo` と開発用署名のSHA-1 |
| Echo Android Release | Android | パッケージ `jp.mutsuna.echo` と実際の配布署名のSHA-1 |

Google Play配布ではアップロード鍵ではなく **Play App Signingのアプリ署名証明書** のSHA-1を使う。
ローカル署名配布も行う場合は、その署名に対応するAndroidクライアントも登録する。
AndroidはGoogle Play servicesのAuthorizationClientを使用し、Webクライアントの秘密情報やサーバーは不要。

## デスクトップのビルド

### 本番設定の登録状況

- Google Cloudプロジェクト: `mutsuna-echo`。運営者により検証済み・本番環境への変更を確認済み。
- Desktop: `127865799185-r7fqndmsp4kn6lv4eb0gre63f2tc29eb.apps.googleusercontent.com`。GitHubの `release-windows` / `release-macos` EnvironmentへクライアントID（Variable）とclient secret（Secret）を登録済み。
- Android開発用: `127865799185-uqlji3e6scjl27ael3fr6o9jvqdros57.apps.googleusercontent.com`。
- Android RELEASE用として登録されたID: `127865799185-n2sl9vqub06d1onpmr1ule75aj1oqu2d.apps.googleusercontent.com`。Play Consoleと照合し、現在の従来型アプリ署名鍵と一致することを確認済み。
- AndroidのAuthorizationClientはパッケージ名と署名でクライアントが識別されるため、上記IDやDesktop用secretをAndroidコードへ埋め込まない。
- 利用規約 `/terms` とGoogle Driveの説明を含む `/privacy` は本番サイトに反映済み。


ビルドプロセスの環境変数に以下を設定する。値をGitへコミットしたり、チャットへ貼り付けたりしない。

- `MUTSUNA_GOOGLE_DESKTOP_CLIENT_ID`: Desktop appクライアントID
- `MUTSUNA_GOOGLE_DESKTOP_CLIENT_SECRET`: GoogleがDesktop app用に発行したclient secret（発行された場合）

現在はシェルまたはCIの環境変数から読み込む。`.env`への追記だけでは反映されない。
環境変数を設定した後、`pnpm tauri dev` または通常の配布ビルドを実行する。
CIではEcho用の値を設定し、ReserveのWebクライアントの値を流用しない。

Desktopクライアントは公開クライアントであり、配布バイナリに含まれるclient secretをサーバーの秘密鍵として扱うことはできない。
この実装は外部ブラウザー、ランダムstate、PKCE S256、127.0.0.1の動的ポートを使う。
ユーザーのアクセストークンとリフレッシュトークンはネイティブの資格情報ストアにのみ保存する。

## 一般公開の手順（2026年9月9日確認）

利用規約ページを公開した後、Brandingの「アプリケーション利用規約」に `https://echo.mutsuna.jp/terms` を登録する。利用規約URLはGoogleの公開要件では任意だが、本アプリでは登録する。

1. 公開用プロジェクトでDrive API / Docs APIを有効化する。既存のEcho専用プロジェクトを本番にする場合、以後の開発検証は別プロジェクトへ分ける。クライアントを変更する場合は再連携・既存保存先への影響も確認する。
2. 公開サイトのプライバシーポリシーに、Googleデータの利用目的、端末からの直接保存、トークンの保管、解除とファイル削除の違いを掲載する。サイト側のソース変更だけでは公開済みとはならない。
3. Google Auth Platform → Brandingでアプリ名、サポートメール、開発者連絡先を設定する。ホームページは `https://echo.mutsuna.jp/`、プライバシーポリシーは `https://echo.mutsuna.jp/privacy`。承認済みドメインは `mutsuna.jp` とし、プロジェクトに関係するアカウントでSearch Consoleの所有権を確認する。
4. Data Accessは `https://www.googleapis.com/auth/drive.file` のみ。これは非機密スコープであり、Drive全体アクセス用の制限付きスコープの審査とは異なる。Brandingの確認・公開と、Audienceの公開状態は別々に確認する。
5. AudienceをExternalとし、公開ステータスをTestingからIn productionへ変更する。Brandingで要求される確認を完了し、承認されたブランド情報を公開する。画面の指示・審査状態に従う。
6. DesktopクライアントとAndroid配布署名のクライアントを確認する。Google Playの場合はアプリ署名証明書のSHA-1であり、アップロード鍵ではない。
7. GitHubのリポジトリまたはデスクトップ配布用Environmentで、Variable `MUTSUNA_GOOGLE_DESKTOP_CLIENT_ID` とSecret `MUTSUNA_GOOGLE_DESKTOP_CLIENT_SECRET` を設定する。公開用Desktop JSONから登録し、値をログへ表示しない。CIのWindows/macOSビルド（Store用MSIXを含む）に渡される。未設定時は配布ビルドを停止する。AndroidはこのDesktop情報を使わない。
8. テストユーザーに登録していないGoogleアカウントで、Windows/macOSの配布ビルドとGoogle Play署名のAndroidビルドから、認証、再起動後の保存、上書き、別版、解除を確認して配布する。OAuthの公開だけでは既存配布アプリは更新されない。

参考：[Driveスコープ](https://developers.google.com/workspace/drive/api/guides/api-specific-auth)、[ブランド確認](https://developers.google.com/identity/protocols/oauth2/production-readiness/brand-verification)、[公開対象の管理](https://support.google.com/cloud/answer/15549945)。

## 保存の動作確認

1. 設定 → 一般 → Google Driveで連携する。Googleの許可画面にEchoのアプリ名が出ることを確認。
2. 音声・文字起こし・会議ノートを用意し、会議画面でDriveへ保存する。
3. `Mutsuna Echo/日時_会議名/` に音声、Googleドキュメント、JSONがあることを確認。
4. ドキュメント内の「会議ノート」「文字起こし」タブ、話者名、時刻、日本語、絵文字を確認。
5. Driveで本文を編集し、再保存 → キャンセルで変わらないこと、上書き保存でアプリの保存済み本文に戻ることを確認。
6. 別版保存で以前のフォルダーが残り、新しい会議フォルダーが作られることを確認。
7. 通信切断、権限取消し、容量不足、削除済みタブのケースを確認。
8. Windows、macOS、Androidの実機で認証・再起動後の利用・期限切れ後の更新を確認。

TestingのOAuth許可ではリフレッシュトークンの有効期間に制約があるため、再認証が必要になる場合がある。
本番公開時はGoogle Auth Platformの公開・ブランド確認要件を確認する。

## 保存動作と初期版の範囲

- アプリに保存済みの内容を読み出す。編集中で未保存の会議ノートは、先にアプリ内で保存する。
- 上書き対象は選択した音声、JSON、アプリの2タブの本文。他のタブは保持する。
- 2タブの本文更新はDocsの単一batchUpdateとrequiredRevisionIdで実施する。更新直前に競合した場合は失敗として返す。
- 音声、JSON、Googleドキュメントを横断する原子的更新はできない。部分的に保存された場合はエラーを表示する。再保存時は同じ版に再試行できる。
- 音声は8 MiBずつ送信し、同じ実行内の通信障害はアップロード済み位置を問い合わせて再開する。アプリ再起動後の送信再開・自動保存は未実装。再試行時は音声を先頭から送り直す。
- 保存先ファイルはGoogleアカウント・会議・版ごとに管理する。別版保存後は新しい版が次回の上書き先になる。
- アカウントを解除してもDriveのファイルは削除しない。Google側の許可も取り消す場合はGoogleアカウントの接続管理から行う。
- 初期版では専用フォルダーを自動作成する。任意フォルダー選択・共有ドライブ・復元インポートは対象外。

## 保存時の負荷抑制と表示

- 保存成功後は10秒間、次の保存を待機する。会議を切り替えても、同じアプリプロセス内のすべての保存に適用する。待機時間はネイティブ側で検証し、画面に残り秒数を表示する。
- 同じ保存先の音声とローカル音声のSHA-256・容量が一致し、Drive上で削除されていなければ、音声の送信を省略する。チェックサムが取得できない場合は省略せず送信する。別版保存は別のファイルを作るため音声も保存する。
- 音声が500MB（500,000,000 bytes）以上の場合、保存画面の音声チェックは初期状態でオフ。容量を表示し、ユーザーが選択した場合のみ保存する。追加の確認ダイアログは出さない。画面を開いてから容量が増えた場合は、ネイティブ側でも再選択を求める。
- 読み取り・再実行可能な更新は通信障害またはHTTP 5xx/408で最大3回、1秒・2秒・4秒と間隔を延ばして再試行する。音声送信は再開位置の問い合わせと進捗停滞を含め、1回の送信処理で最大3回の回復待機に制限する。ファイル作成・本文編集など結果が曖昧な操作は自動で重複実行せず、保存画面からの再試行と既存ファイルの再検索を使う。
- HTTP 429またはGoogleの利用制限を示す403では自動送信を止める。最低60秒、Retry-Afterがそれより長い場合は指定時間だけ待機する。権限不足の403は利用制限と区別する。期限後も自動では保存を再開せず、ユーザーが再保存する。待機状態は同じアプリプロセス内で共有し、アプリ再起動時にリセットする。
- Googleドキュメントの「会議ノート」「文字起こし」の両タブは、新規・上書きともに `PAGELESS`（ページ分けなし）を適用する。保存済みドキュメントには、次にドキュメントを上書き保存した際に反映される。

## 公式資料

- [OAuthプロジェクトとクライアントの構成](https://developers.googleblog.com/get-smart-about-preparing-your-app-for-oauth-verification/)
- [OAuthポリシー](https://developers.google.com/identity/protocols/oauth2/policies)
- [ネイティブアプリのOAuth](https://developers.google.com/identity/protocols/oauth2/native-app)
- [Android AuthorizationClient](https://developer.android.com/identity/authorization)
- [Driveのスコープ](https://developers.google.com/workspace/drive/api/guides/api-specific-auth)
- [Docsのタブ操作](https://developers.google.com/workspace/docs/api/how-tos/tabs)
- [再開可能アップロード](https://developers.google.com/workspace/drive/api/guides/manage-uploads)

保存画面では、項目ごとの前回成功時の内容とアプリ内の保存内容を比較し、変更のない音声・ドキュメント・JSONを初期選択から外します。必要なら手動で選択して上書き・別版保存できます。Drive側の編集や削除はこの判定に含みません。旧バージョンの保存履歴には比較情報がないため、各項目を一度保存すると判定が有効になります。音声を選択した場合も、Drive側のSHA-256と容量が一致すれば従来どおり送信を省略します。

### 量子対応のPlay署名

Play Consoleで3種類の署名を確認。運営者から追加2クライアントの登録完了報告あり。

- 現在の従来型: `9F:68:A2:63:5D:63:EF:13:BD:A7:A5:99:74:80:7C:D7:1E:4E:CF:F6`
- 以前のアプリ署名鍵: `76:8A:E1:1E:EE:69:C5:F5:5A:7B:4D:31:6F:C3:45:58:02:5C:D5:29`
- ポスト量子: `60:4F:03:13:9D:9B:1B:8D:9A:63:9E:D3:8D:B1:F1:1E:3F:3B:9C:59`

内部テスト用の `android-internal.yml` は専用ブランチへのpushで署名済みAABをビルドし、`internal` トラックだけに配布する。製品版・alphaの配布先変数は変更しない。
