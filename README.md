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
