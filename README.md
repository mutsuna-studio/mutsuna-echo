# Mutsuna Echo

Svelte 5 + TypeScript + Tauri 2 + Rustで作る、マルチOS対応の録音・文字起こしアプリです。

## v0.1

- MP3 / M4A / WAV / FLACの選択
- 再生時間のローカル解析と送信前のコスト概算
- ElevenLabs Scribe v2による日本語文字起こし
- クラウド／ローカル共通の文字起こしProvider基盤
- Silero VADによる音声区間検出
  - Windows／macOSでは初回起動時に約2.3 MBのモデルを任意モデル領域へ標準導入
  - 録音中は`Listening`／`Speech detected`だけを表示し、リアルタイムSTTは実行しない
  - 標準／小声優先／ノイズ抑制優先の検出プリセット
  - ローカルSTTでは発話区間検出後に区間ごとの進捗を表示
- 話者分離とタイムスタンプ表示
- ElevenLabs APIキーの安全なローカル保存
  - Windows: ユーザー単位のDPAPI
  - macOS / Linux: OSの資格情報ストア
- ElevenLabsの契約枠とSpeech to Text使用量のScribe v2時間換算表示
- アプリ内録音（リアルタイム文字起こしなし）
  - Windows 10 / 11: マイク + WASAPIシステム音声
  - macOS 14.4以降: マイク + Core Audio Tap
  - Android 10以降: マイク + AudioPlaybackCapture
  - 48 kHz / mono / AAC-LC / M4A
  - マイクとシステム音声を別々の復旧用トラックとして保持し、同一タイムラインでミックス
  - 録音中断に備えたfragmented MP4と起動時復旧
  - `Music/Mutsuna Echo`に保存した過去100件の録音を一覧から再選択

音声はWebViewへ読み込まず、RustからElevenLabsへストリーミング送信します。APIキーもRust側だけで読み出します。

## 開発

```powershell
pnpm install
pnpm tauri dev
```

開発ビルドではメイン画面上部の「オーバーレイを確認」から、会議検出・録音中・保存中・保存完了・エラーの各状態を実録音なしで確認できます。このプレビューUIとTauriコマンドはリリースビルドには含まれません。

アプリ内でElevenLabs APIキーを登録してください。制限付きキーでは次の権限だけを許可し、利用上限を設定することを推奨します。

- Speech to Text: アクセス
- User: 読み取り
- Workspace Analytics: Full Read

APIキーはSvelte/WebViewへ返さず、Rust側のElevenLabs通信だけで使用します。
AndroidではAndroid Keystoreで生成した端末内AES-GCM鍵を使用します。

## ローカルSTT基盤

ローカルSTTはアプリ本体とモデルを分離し、現在はReazonSpeech K2 int8-fp32を任意にダウンロードして日本語文字起こしに使用できます。モデルは固定した配布元・サイズ・SHA-256を検証してから公開し、音声を外部へ送信しません。

将来ダウンロードされたモデルは、OSごとのアプリローカルデータ領域にある `local-stt/models/<model-id>/<version>/` へ配置します。各導入単位は次の情報を持つ `manifest.json` で管理します。

- schema version、Provider、model ID、version、推論engine
- 表示名、対応言語
- 各モデルファイルの相対パス、サイズ、SHA-256

manifestと実ファイルが揃い、ID・保存パス・サイズなどの検証を通ったモデルだけを導入済みとして認識します。ダウンローダーは一時領域へのダウンロード、SHA-256検証、ディレクトリのatomic切り替えを行ってからこの保存領域へ公開します。

Silero VADは独立したダウンロードモデルとして標準導入します。有効時はローカルSTTの前処理で音声区間だけを抽出し、最大30秒単位で推論します。無音区間を物理的に削除した音声は作らず、各区間の元音声上のオフセットをトークン時刻へ戻すため、Transcriptのタイムスタンプは録音全体の時間軸を維持します。録音中も同じ設定で発話状態だけを表示しますが、録音の自動停止やリアルタイム文字起こしには使用しません。

## 録音について

正常終了した録音は `Music/Mutsuna Echo` にM4Aとして保存されます。録音中は2秒ごと（macOSは最初の1秒、その後約10秒）に内部フラグメントを確定し、正常終了後に1本のファイルとして扱います。録音と文字起こしは分離されているため、APIキーなしでも録音できます。

Androidのシステム音声は、再生元アプリがキャプチャを許可した音声に限られます。通話・DRM保護音声などは取得できません。開始時にAndroidの画面共有確認が表示され、録音中はフォアグラウンドサービスの通知を表示します。

## 確認コマンド

```powershell
pnpm check
pnpm build
cd src-tauri
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

## リリースとアプリ内更新

`v0.1.0`のようなバージョンタグをpushすると、GitHub ActionsがWindowsとApple Silicon版macOSのデスクトップ版をビルドし、下書きのGitHub Releaseを作成します。Intel Macは配布対象外です。Windowsの配布物はNSISセットアップ（`.exe`）だけです。Releaseを公開すると、アプリは署名済みの`latest.json`を使って更新を検出します。

同じタグでAndroid 10以降のARM64端末向け署名済みAABも生成し、`android-aarch64-aab`というGitHub Actions Artifactへ保存します。Google Playへの自動アップロードは行わず、初回はPlay Consoleから手動で登録します。

リリースJobはGitHub Environmentsの承認後にだけ署名用Secretへアクセスします。次のEnvironmentを作成し、Required reviewersと`v*`タグのみ許可を設定してください。承認可能なMaintainerが2人以上になったら自己承認も禁止してください（現在の1人構成で禁止するとリリース不能になります）。

| Environment | 登録するGitHub Actions Secrets |
| --- | --- |
| `release-windows` | `TAURI_SIGNING_PRIVATE_KEY`、必要な場合は`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |
| `release-macos` | Tauri署名鍵一式。Developer IDへ移行する場合はApple署名・Notarization用Secret一式も追加 |
| `release-android` | AndroidアップロードKeystore用Secret一式 |

初回リリース前に、該当Environmentへ次のSecretを登録してください。Repository Secretへまとめて登録しないでください。

- `TAURI_SIGNING_PRIVATE_KEY`: `.tauri/mutsuna-echo.key`の内容
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: パスワード付きの更新署名鍵を使う場合のみ登録します
- macOS配布用のApple署名・Notarization Secrets: `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、`APPLE_API_ISSUER`、`APPLE_API_KEY`
- `APPLE_API_PRIVATE_KEY`: App Store Connectの`.p8`秘密鍵をbase64化した値
- `ANDROID_KEYSTORE_BASE64`: Androidアップロード用`.jks`をbase64化した値
- `ANDROID_KEYSTORE_PASSWORD`: Keystoreのパスワード
- `ANDROID_KEY_ALIAS`: アップロードキーのalias
- `ANDROID_KEY_PASSWORD`: アップロードキーのパスワード

### macOS署名モード

Repository Variable `MACOS_SIGNING_MODE`でmacOSの署名方法を切り替えます。

- `adhoc`: Apple Developer Programなしでアドホック署名する現在の設定です。Notarizationされないため、利用者は初回起動時にmacOSの「プライバシーとセキュリティ」から手動で許可する必要があります。
- `developer-id`: Developer ID署名とNotarizationを行います。`release-macos`へ上記6つのApple Secretを登録してから切り替えます。不足しているSecretがある場合は、署名処理前に分かりやすいエラーで停止します。

Apple Developer Program加入後の切り替えではWorkflowの変更は不要です。`release-macos`へApple Secretを登録し、Repository Variableを`developer-id`へ変更してください。

`.tauri/`はgitignore済みです。更新署名鍵を失うと既存ユーザーへ更新を配信できなくなるため、`.tauri/mutsuna-echo.key`はパスワード管理された保管先へバックアップしてください。公開鍵だけが`tauri.conf.json`へ含まれます。

AndroidのアップロードKeystoreも失うと同じアプリとして更新を公開できなくなるため、GitHub Secretsとは別の安全な場所へバックアップしてください。`keystore.properties`とKeystore本体はリポジトリへコミットしません。

アップロードKeystoreは初回だけ手元で作成します。

```powershell
keytool -genkeypair -v -keystore mutsuna-echo-upload.jks -keyalg RSA -keysize 2048 -validity 10000 -alias upload
[Convert]::ToBase64String([IO.File]::ReadAllBytes("mutsuna-echo-upload.jks"))
```

2行目の出力を`ANDROID_KEYSTORE_BASE64`へ登録します。AAB生成後はCI内で`jarsigner`による署名検証を通過した成果物だけをArtifactとして保存します。

## OSSリポジトリのCIと権限

Pull Requestと`main`へのpushでは、`.github/workflows/ci.yml`がSecretを使わずにフロントエンド検査・ビルドとRustテスト・Clippyを実行します。Workflowの既定権限は`contents: read`で、リリース作成Jobだけが`contents: write`を持ちます。Fork由来のコードを署名用Secretへ触れさせないため、`pull_request_target`でPRのコードを実行しないでください。

Public化時にはGitHub側でも次を設定してください。

- `main`はPR、CI成功、Code Owner reviewを必須にし、force pushを禁止する
- `v*`タグはリリース管理者だけが作成・削除できるRulesetで保護する
- ActionsのFork pull request workflowにはwrite tokenを渡さない
- `.github/CODEOWNERS`でWorkflowと署名設定の変更に`@mutsuna-jp`のレビューを必須にする

外部Actionは完全なコミットSHAへ固定し、`.github/dependabot.yml`で週次更新します。
