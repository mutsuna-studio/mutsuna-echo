# Mutsuna Echo

Svelte 5 + TypeScript + Tauri 2 + Rustで作る、マルチOS対応の録音・文字起こしアプリです。

## v0.1

- MP3 / M4A / WAV / FLACの選択
- 再生時間のローカル解析と送信前のコスト概算
- ElevenLabs Scribe v2による日本語文字起こし
- クラウド／ローカル共通の文字起こしProvider基盤
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

ローカルSTTは、アプリ本体とモデルを分離する前提でProvider契約と保存領域だけを実装しています。現在の版には推論エンジン、モデルカタログ、モデルダウンロードUIは含まれていないため、ローカル文字起こしはまだ実行できません。

将来ダウンロードされたモデルは、OSごとのアプリローカルデータ領域にある `local-stt/models/<model-id>/<version>/` へ配置します。各導入単位は次の情報を持つ `manifest.json` で管理します。

- schema version、Provider、model ID、version、推論engine
- 表示名、対応言語
- 各モデルファイルの相対パス、サイズ、SHA-256

manifestと実ファイルが揃い、ID・保存パス・サイズなどの検証を通ったモデルだけを導入済みとして認識します。次段階のダウンローダーは、一時領域へのダウンロード、SHA-256検証、ディレクトリのatomic切り替えを行ってからこの保存領域へ公開します。

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
