# Contributing to Mutsuna Echo

Issue reports and pull requests are welcome. Before making a large change, open an issue so the intended behavior and platform impact can be agreed on first.

## Development

Use Node.js 24, pnpm 11.20.0, the stable Rust MSVC toolchain on Windows, and the prerequisites documented by Tauri 2.

```powershell
pnpm install --frozen-lockfile
pnpm check
pnpm build
cd src-tauri
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

When changing a dependency, local AI runtime, or model version, run `pnpm licenses:generate` from the repository root and include the generated license catalog in the same pull request. Use `pnpm licenses:check` to verify that the generated files are current.

Do not commit API keys, signing keys, keystores, recordings, transcripts, or other private meeting data. Use synthetic data in tests.

## Pull requests

- Keep each pull request focused on one concern.
- Add or update tests for behavior changes.
- Explain platform-specific behavior for Windows, macOS, and Android.
- Allow the required CI checks and Code Owner review to complete.
- Do not add a workflow using `pull_request_target` to execute pull-request code.

By contributing, you agree that your contribution is licensed under the repository's Mozilla Public License 2.0 (MPL-2.0).
