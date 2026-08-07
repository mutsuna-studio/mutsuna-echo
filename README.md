# Mutsuna Echo

Svelte 5 + TypeScript + Tauri 2 + Rustで作る、デスクトップ文字起こしアプリです。

## v0.1

- MP3 / M4A / WAV / FLACの選択
- 再生時間のローカル解析と送信前のコスト概算
- ElevenLabs Scribe v2による日本語文字起こし
- 話者分離とタイムスタンプ表示
- ElevenLabs APIキーの安全なローカル保存
  - Windows: ユーザー単位のDPAPI
  - macOS / Linux: OSの資格情報ストア
- ElevenLabsの契約枠とSpeech to Text使用量のScribe v2時間換算表示

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

## 確認コマンド

```powershell
pnpm check
pnpm build
cd src-tauri
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```
