# Mutsuna Echo agent instructions

These instructions apply to the entire repository.

## Cloud API credentials

Changes involving a cloud provider, API key, API token, account identifier, endpoint, or credential storage must satisfy all of the following requirements.

- Treat Windows, macOS, and Android as supported credential platforms. Do not consider a desktop-only check sufficient.
- Keep secrets in the Rust/native credential layer. Never return a stored secret to Svelte/WebView, print it, include it in an error, or commit it in a fixture.
- Trim user-entered credentials before validation and persistence. Reject empty values after trimming.
- Validate against the provider endpoint before persistence. Restricted-but-valid credentials may be stored only when the provider explicitly documents that behavior.
- Keep provider regions explicit. Soniox must use the Japan regional API at `https://api.jp.soniox.com/v1`; do not replace it with the global or US endpoint.
- Disable redirects for credential-validation requests so authorization headers cannot be forwarded to another origin.
- Bound connection and total request time. Return a user-facing error for timeouts, DNS/TLS failures, HTTP 401/403, missing permissions, and unsupported models.
- After saving, read the credential back through the platform store and verify an exact round trip. If verification fails, delete the newly written value and report failure.
- Multi-value credentials are a transaction. Cloudflare API token and Account ID must either both be verified and stored or both be rolled back.
- Android credential identifiers must be declared in `CredentialNames.kt` and covered by both JVM contract tests and Android Keystore instrumentation tests.

## Required verification for credential changes

Run the checks relevant to every touched layer before handing off a change:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml commands::api_key::tests
cargo test --manifest-path src-tauri/Cargo.toml transcription::soniox::tests
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm check
cd src-tauri/gen/android
./gradlew :app:testUniversalDebugUnitTest
```

When Android Keystore, JNI context initialization, or `SecureCredentialBridge` changes, also run `connectedUniversalDebugAndroidTest` on an API 29+ emulator or connected device. Tests must use synthetic secrets.

Every credential behavior change must add a regression test for its success path and its failure/rollback path. A constant assertion alone is not a sufficient endpoint or authentication-flow test.
