# Mutsuna Echo

Svelte 5 + TypeScript + Tauri 2 + Rustで作る、デスクトップ文字起こしアプリです。

## v0.1

- MP3 / M4A / WAV / FLACの選択
- ElevenLabs Scribe v2による日本語文字起こし
- 話者分離とタイムスタンプ表示
- ElevenLabs APIキーの安全なローカル保存
  - Windows: ユーザー単位のDPAPI
  - macOS / Linux: OSの資格情報ストア

音声はWebViewへ読み込まず、RustからElevenLabsへストリーミング送信します。APIキーもRust側だけで読み出します。

## 開発

```powershell
pnpm install
pnpm tauri dev
```

アプリ内でElevenLabs APIキーを登録してください。制限付きキーではSpeech to Textだけを許可し、利用上限を設定することを推奨します。

## 確認コマンド

```powershell
pnpm check
pnpm build
cd src-tauri
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```
