# Mutsuna Echo

Svelte 5 + TypeScript + Tauri 2 + Rustで作る、マルチOS対応の録音・文字起こしアプリです。

## v0.1

- MP3 / M4A / WAV / FLACの選択
- 再生時間のローカル解析と送信前のコスト概算
- ElevenLabs Scribe v2による日本語文字起こし
- クラウド／ローカル共通の文字起こしProvider基盤
- Silero VADによる音声区間検出
- Sonora（WebRTC Audio ProcessingのPure Rust移植）によるNoise Suppression + AGC2
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

ローカルSTTはアプリ本体とモデルを分離し、Windows、macOS、AndroidではReazonSpeech K2 int8-fp32を任意にダウンロードして日本語文字起こしに使用できます。モデルは固定した配布元・サイズ・SHA-256を検証してから公開し、音声を外部へ送信しません。Androidでは発熱とメモリ使用量を抑えるため推論スレッド数を最大4に制限します。

将来ダウンロードされたモデルは、OSごとのアプリローカルデータ領域にある `local-stt/models/<model-id>/<version>/` へ配置します。各導入単位は次の情報を持つ `manifest.json` で管理します。

- schema version、Provider、model ID、version、推論engine
- 表示名、対応言語
- 各モデルファイルの相対パス、サイズ、SHA-256

manifestと実ファイルが揃い、ID・保存パス・サイズなどの検証を通ったモデルだけを導入済みとして認識します。ダウンローダーは一時領域へのダウンロード、SHA-256検証、ディレクトリのatomic切り替えを行ってからこの保存領域へ公開します。

Silero VADは独立したダウンロードモデルとして標準導入します。有効時はローカルSTTの前処理で音声区間だけを抽出し、最大30秒単位で推論します。各区間の前後300 msも認識へ渡し、元の発話区間に属するトークンだけを残すことで、語頭・語尾や30秒境界を欠落させず重複を防ぎます。無音区間を物理的に削除した音声は作らず、各区間の元音声上のオフセットをトークン時刻へ戻すため、Transcriptのタイムスタンプは録音全体の時間軸を維持します。録音中も同じ設定で発話状態だけを表示しますが、録音の自動停止やリアルタイム文字起こしには使用しません。

録音中はマイクだけをSonoraのNoise SuppressionとAGC2（Adaptive Digital Gain）へ10 ms単位で入力し、システム音声は未加工のまま保持します。完成したミックス音声に加えて、強調済みマイク／未加工システムの別トラックをMeetingへ保存します。文字起こし時はローカル・クラウド共通で各トラックを別々に認識し、元の時刻順へ統合します。マイクを「自分」、システム音声を「相手」として扱うため、1対1の録音では追加モデルなしで話者を分離できます。Sonoraへの依存はRust側の`AudioEnhancer`トレイトに閉じ込めています。

ローカル認識は、Greedy Searchを使う「高速」と、`modified_beam_search`で最大8候補を比較する「高精度」を選択できます。重要用語が設定されている場合はモデルの`tokens.txt`で安全にトークン化し、hotwordsとしてBeam Searchへ渡します。また、共通／会議別に「誤表記 ⇒ 正式表記」の表記補正を保存でき、端末内の機械整形でフィラー除去とともに適用します。発話欄で手動修正した短い表記差分も端末内の学習辞書へ自動保存され、次回以降の整形に利用されます。一括置換・AI整形・削除・文章全体の書き換えは誤学習を避けるため対象外です。認識原文は変更せず保存するため、整形結果は取り消せます。

## ローカル話者分離

時刻付きトークンを持つ保存済み文字起こしには、設定画面から任意導入するpyannote segmentation-3.0 INT8と3D-Speaker ERes2Net Baseを使って、端末内だけで話者分離を後処理できます。モデルは固定URL・サイズ・SHA-256を検証し、`local-diarization/models/<pack-id>/<version>/`へSTTモデルとは独立して保存します。通信するのはユーザーがモデルを導入するときだけで、音声や推論結果は送信しません。

推論はmono 16 kHzへストリーミング変換し、20分チャンクと30秒オーバーラップで処理します。各チャンクの話者を重複区間と話者埋め込みで全録音の共通IDへ統合するため、録音全体をメモリへ載せません。話者数は自動または1〜10人から指定できます。実行中は文字起こしと相互排他になり、キャンセル時や文字起こしrevisionが変化した場合は結果を保存しません。

再話者分離では自動割り当てだけを置き換え、明示的なトークン単位のユーザー修正と編集済み本文を保持します。既存の表示用話者名は解除され、結果は初出順の匿名ラベル`Speaker 1…N`へ戻ります。初期リリースは保存済み文字起こしに対する手動後処理であり、録音中のリアルタイム分離、iOS、登録済み人物の声紋識別は対象外です。

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

同じタグでAndroid 10以降のARM64端末向け署名済みAABも生成し、`android-aarch64-aab`というGitHub Actions Artifactへ保存したうえで、Google Playの`ANDROID_RELEASE_TRACK`へ`completed`として送信します。個人用デベロッパーアカウントで製品版アクセスが未開放の間は`alpha`を使用し、12人以上・14日間以上のクローズドテストとGoogleの承認後に`production`へ切り替えます。Play ConsoleのManaged publishingを無効にしておけば、production送信後はGoogleの審査承認を経て自動公開されます。タグ、`package.json`、`Cargo.toml`、`tauri.conf.json`のバージョンが一致しない場合は、ビルド前に停止します。

デスクトップビルドでは、Tauriがバンドル対象を検証する前に`scripts/prepare-desktop-runtime.ps1`が`sherpa-onnx-sys`だけを先行ビルドします。これにより、クリーンなRunnerでもSherpa ONNXとONNX RuntimeのDLL／dylibが確実に生成され、アプリ本体を二重にコンパイルせずに配布物へ含められます。

Androidビルドでは、共有のセットアップActionが`scripts/prepare-android-runtime.sh`を実行し、Rust依存と同じSherpa ONNX v1.13.4の公式Android C APIランタイムを検証してARM64 AABへ同梱します。ローカルでAndroid版をビルドする場合も、同じスクリプトを実行し、表示された`SHERPA_ONNX_LIB_DIR`をビルド環境へ設定してください。

リリースJobはGitHub Environmentsの承認後にだけ署名用Secretへアクセスします。次のEnvironmentを作成し、Required reviewersと`v*`タグのみ許可を設定してください。承認可能なMaintainerが2人以上になったら自己承認も禁止してください（現在の1人構成で禁止するとリリース不能になります）。

| Environment | 登録するGitHub Actions Secrets |
| --- | --- |
| `release-windows` | `TAURI_SIGNING_PRIVATE_KEY`、必要な場合は`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |
| `release-macos` | Tauri署名鍵一式。Developer IDへ移行する場合はApple署名・Notarization用Secret一式も追加 |
| `release-android` | AndroidアップロードKeystore用Secret一式、`GOOGLE_PLAY_SERVICE_ACCOUNT_JSON` |
| `release-production` | GitHub Releaseを下書きから公開へ切り替える承認ゲート。Required reviewersを設定 |

初回リリース前に、該当Environmentへ次のSecretを登録してください。Repository Secretへまとめて登録しないでください。

- `TAURI_SIGNING_PRIVATE_KEY`: `.tauri/mutsuna-echo.key`の内容
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: パスワード付きの更新署名鍵を使う場合のみ登録します
- macOS配布用のApple署名・Notarization Secrets: `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、`APPLE_API_ISSUER`、`APPLE_API_KEY`
- `APPLE_API_PRIVATE_KEY`: App Store Connectの`.p8`秘密鍵をbase64化した値
- `ANDROID_KEYSTORE_BASE64`: Androidアップロード用`.jks`をbase64化した値
- `ANDROID_KEYSTORE_PASSWORD`: Keystoreのパスワード
- `ANDROID_KEY_ALIAS`: アップロードキーのalias
- `ANDROID_KEY_PASSWORD`: アップロードキーのパスワード
- `GOOGLE_PLAY_SERVICE_ACCOUNT_JSON`: Google Play Developer APIへの公開権限を持つサービスアカウント鍵JSONの内容全体

Google Playへの自動公開には、Google CloudでGoogle Play Developer APIを有効化してサービスアカウントを作成し、Play Consoleの「ユーザーと権限」で`jp.mutsuna.echo`をproductionへ公開できる権限を付与します。JSON鍵の内容全体をGitHub Environment `release-android`の`GOOGLE_PLAY_SERVICE_ACCOUNT_JSON`へ登録してください。完全自動公開にする場合は、Play ConsoleのManaged publishingを無効にします。

GitHub Environment `release-android`のVariable `ANDROID_RELEASE_TRACK`には、製品版アクセスの承認前は`alpha`、承認後は`production`を設定します。失敗したAndroid公開だけを再実行する場合は、`v0.1.2-android-retry.1`のような再公開専用タグを作成します。この形式ではデスクトップJobをスキップし、元の`v0.1.2`をリリース名としてAndroid Jobだけを実行します。

リリース時は3か所のバージョンとストア向け更新文`distribution/whatsnew/whatsnew-ja-JP`を更新してから、同じバージョンのタグをpushします。タグのpushでWindows/macOS成果物をGitHub Releaseの下書きへ追加し、Androidを指定トラックへ配信します。すべて成功後、`release-production`の承認によって下書きが公開されます。Androidの`versionCode`はTauriがSemVerから生成し、`0.1.6`は`1006`になります。

```powershell
node scripts/verify-release-version.mjs v0.1.6
git tag v0.1.6
git push origin v0.1.6
```

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
