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

function Invoke-NativeRuntimeBuild {
    & cargo @cargoArguments
    if ($LASTEXITCODE -ne 0) {
        throw "The native runtime preparation build failed with exit code $LASTEXITCODE."
    }
}

Invoke-NativeRuntimeBuild

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

$missingRuntimeFiles = @(
    $requiredFiles | Where-Object {
        -not (Test-Path -LiteralPath (Join-Path $profileDirectory $_) -PathType Leaf)
    }
)

if ($missingRuntimeFiles.Count -gt 0) {
    # rust-cache intentionally filters non-Cargo artifacts. A restored
    # sherpa-onnx-sys build can therefore look current while its copied shared
    # libraries are absent. Build the dependency in an isolated target so its
    # build script must recreate the runtime files without clearing the shared
    # Cargo cache.
    Write-Host "Native runtime files are missing after the cached build; regenerating sherpa-onnx-sys."
    $resolvedTargetDirectory = [System.IO.Path]::GetFullPath((Join-Path $tauriDirectory "target"))
    $isolatedTargetDirectory = Join-Path $resolvedTargetDirectory "native-runtime-prep"
    if (Test-Path -LiteralPath $isolatedTargetDirectory) {
        $resolvedIsolatedTargetDirectory = [System.IO.Path]::GetFullPath($isolatedTargetDirectory)
        $targetPrefix = $resolvedTargetDirectory.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
        if (-not $resolvedIsolatedTargetDirectory.StartsWith($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove isolated build directory outside Cargo target: $resolvedIsolatedTargetDirectory"
        }
        Remove-Item -LiteralPath $resolvedIsolatedTargetDirectory -Recurse -Force
    }

    $previousCargoTargetDirectory = $env:CARGO_TARGET_DIR
    try {
        $env:CARGO_TARGET_DIR = $isolatedTargetDirectory
        Invoke-NativeRuntimeBuild
    } finally {
        if ([string]::IsNullOrEmpty($previousCargoTargetDirectory)) {
            Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
        } else {
            $env:CARGO_TARGET_DIR = $previousCargoTargetDirectory
        }
    }

    $isolatedProfileDirectory = if ([string]::IsNullOrWhiteSpace($Target)) {
        Join-Path $isolatedTargetDirectory "release"
    } else {
        Join-Path $isolatedTargetDirectory "$Target/release"
    }
    New-Item -ItemType Directory -Path $profileDirectory -Force | Out-Null
    foreach ($fileName in $requiredFiles) {
        $isolatedRuntimePath = Join-Path $isolatedProfileDirectory $fileName
        if (-not (Test-Path -LiteralPath $isolatedRuntimePath -PathType Leaf)) {
            throw "Required $Platform runtime library was not generated in the isolated build: $isolatedRuntimePath"
        }
        Copy-Item -LiteralPath $isolatedRuntimePath -Destination (Join-Path $profileDirectory $fileName) -Force
    }
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
