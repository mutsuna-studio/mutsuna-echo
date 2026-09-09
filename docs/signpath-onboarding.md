# SignPath onboarding

Application: https://signpath.org/apply.html
Requirements: https://signpath.org/terms.html
Integration: https://docs.signpath.io/trusted-build-systems/github

## Prepared information

- Project: Mutsuna Echo
- Source/homepage: https://github.com/mutsuna-studio/mutsuna-echo
- Downloads: https://github.com/mutsuna-studio/mutsuna-echo/releases
- License: MPL-2.0; shared @mutsuna/ui component library is public and MIT licensed.
- Privacy: https://echo.mutsuna.jp/privacy
- Requested targets: Windows executable and NSIS installer.
- Project is new; do not claim widespread adoption or guaranteed eligibility.

## Before submission

Applicant details, signing roles, and GitHub MFA were confirmed by the maintainer
on 9 September 2026. Confirm all shipped dependencies are eligible OSS/system libraries;
external cloud services are not bundled server implementations. Review the
Foundation's terms and personal-data consent at submission time. Do not opt into
optional marketing.

## After acceptance

Obtain the actual organization ID, project slug, artifact configuration, signing
policy, and CI token from SignPath. Install/authorize their GitHub integration
only for this repository. Store the token in GitHub environment secrets.
Keep each release's manual SignPath approval.

Integrate Windows signing before public release publication:
1. Build from the release ref; upload an unsigned GitHub Actions artifact.
2. Submit the artifact ID using SignPath's official action, pinned to a reviewed
   commit. Enforce project metadata and restrict file selection to our binaries.
3. Download approved signed artifacts, verify Authenticode and expected identity.
4. Generate Tauri updater signatures from the final signed installer, then update
   the Windows entry in latest.json. Signing changes file bytes, so an older
   updater .sig must never be reused. Preserve macOS entries.
5. Publish only after these checks. A signing failure must stop publication when
   signing is enabled; never silently fall back to unsigned releases.

Do not retrofit published release files while their updater metadata still
references the old bytes. No live signing workflow is enabled by this preparation.
