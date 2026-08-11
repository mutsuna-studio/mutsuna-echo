# Third-party license sources

The files in this directory are verbatim license and notice documents used when packaging external runtime binaries and downloaded AI models.

- `onnxruntime/`: ONNX Runtime v1.27.0 `LICENSE` and `ThirdPartyNotices.txt` from the corresponding upstream tag.
- `models/silero-vad-LICENSE.txt`: Silero VAD v5.0 license.
- `models/pyannote-segmentation-LICENSE.txt`: pyannote segmentation 3.0 license.
- `rust/`: upstream license texts for crates that omit the license file from their published archive.

The application-wide catalog is generated from these files, installed npm packages, Cargo metadata, and the vendored Sherpa ONNX license:

```powershell
pnpm licenses:generate
pnpm licenses:check
```

Regenerate and review `static/third-party-licenses.json` and `static/THIRD-PARTY-NOTICES.txt` whenever dependencies or pinned model/runtime versions change.
