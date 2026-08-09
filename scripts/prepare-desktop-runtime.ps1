param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("windows", "macos")]
    [string]$Platform,

    [string]$Target = ""
)

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$tauriDirectory = Join-Path $repositoryRoot "src-tauri"
$manifestPath = Join-Path $tauriDirectory "Cargo.toml"

if ($Platform -eq "macos" -and [string]::IsNullOrWhiteSpace($Target)) {
    throw "-Target is required when preparing the macOS runtime."
}

# tauri-build validates bundle resources from the application build.rs before
# Cargo is guaranteed to run dependency build scripts. Build only the native
# dependency first; sherpa-onnx-sys then copies its shared runtime into Cargo's
# release directory without compiling the application twice.
$cargoArguments = @(
    "build",
    "--release",
    "--manifest-path", $manifestPath,
    "--package", "sherpa-onnx-sys"
)
if (-not [string]::IsNullOrWhiteSpace($Target)) {
    $cargoArguments += @("--target", $Target)
}

& cargo @cargoArguments
if ($LASTEXITCODE -ne 0) {
    throw "The native runtime preparation build failed with exit code $LASTEXITCODE."
}

$profileDirectory = if ([string]::IsNullOrWhiteSpace($Target)) {
    Join-Path $tauriDirectory "target/release"
} else {
    Join-Path $tauriDirectory "target/$Target/release"
}

if ($Platform -eq "windows") {
    $requiredFiles = @(
        "onnxruntime.dll",
        "onnxruntime_providers_shared.dll",
        "sherpa-onnx-c-api.dll"
    )
    $bundleSourceDirectory = $profileDirectory
} else {
    $requiredFiles = @(
        "libsherpa-onnx-c-api.dylib",
        "libonnxruntime.dylib"
    )

    # tauri.macos.conf.json also supports native Apple Silicon builds where
    # Cargo writes directly to target/release. Keep that stable bundle path and
    # copy cross-target output into it on GitHub-hosted runners.
    $bundleSourceDirectory = Join-Path $tauriDirectory "target/release"
    New-Item -ItemType Directory -Path $bundleSourceDirectory -Force | Out-Null
}

foreach ($fileName in $requiredFiles) {
    $sourcePath = Join-Path $profileDirectory $fileName
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
        throw "Required $Platform runtime library was not generated: $sourcePath"
    }

    if ($Platform -eq "macos") {
        Copy-Item -LiteralPath $sourcePath -Destination (Join-Path $bundleSourceDirectory $fileName) -Force
    }
}

foreach ($fileName in $requiredFiles) {
    $bundlePath = Join-Path $bundleSourceDirectory $fileName
    if (-not (Test-Path -LiteralPath $bundlePath -PathType Leaf)) {
        throw "Required $Platform bundle library is missing: $bundlePath"
    }

    Write-Host "Prepared $bundlePath"
}
