# Code signing policy

Status: preparing a SignPath Foundation application. Approval and signing are
not yet enabled. Existing Windows releases are not signed by SignPath Foundation.

If accepted, the intended attribution is:
Free code signing provided by [SignPath.io](https://signpath.io), certificate by
[SignPath Foundation](https://signpath.org).

Project roles:
- Author, reviewer, and release signing approver: [mutsuna-jp](https://github.com/mutsuna-jp).
- Contributors' changes require maintainer review.
- Signing participants must enable MFA for GitHub and SignPath.

Signing will require an explicit approval for each release and verifiable
GitHub Actions builds from this repository. Only Mutsuna Echo binaries and its
installer are intended signing targets; third-party binaries are excluded from
our signing requests. Product identity and version must match the release.

See the [privacy policy](https://echo.mutsuna.jp/privacy) for local storage,
optional cloud processing, third-party providers, and Mutsuna Cloud retention.

Installation and removal: use the Windows installer, and uninstall through
Windows Settings > Apps > Installed apps.
