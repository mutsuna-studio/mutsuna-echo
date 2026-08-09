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
    $requiredNativeSdkFiles = @(
        "sherpa-onnx-c-api.lib",
        "onnxruntime.lib"
    )
    $bundleSourceDirectory = $profileDirectory
} else {
    $requiredFiles = @(
        "libsherpa-onnx-c-api.dylib",
        "libonnxruntime.dylib",
        "libonnxruntime.1.27.0.dylib"
    )
    $requiredNativeSdkFiles = $requiredFiles

    # tauri.macos.conf.json also supports native Apple Silicon builds where
    # Cargo writes directly to target/release. Keep that stable bundle path and
    # copy cross-target output into it on GitHub-hosted runners.
    $bundleSourceDirectory = Join-Path $tauriDirectory "target/release"
    New-Item -ItemType Directory -Path $bundleSourceDirectory -Force | Out-Null
}

$resolvedTargetDirectory = [System.IO.Path]::GetFullPath((Join-Path $tauriDirectory "target"))
$nativeCacheDirectory = Join-Path $resolvedTargetDirectory "sherpa-onnx-prebuilt"
$nativeSdkReady = Test-Path -LiteralPath $nativeCacheDirectory -PathType Container
if ($nativeSdkReady) {
    foreach ($fileName in $requiredNativeSdkFiles) {
        $matchingSdkFile = Get-ChildItem -LiteralPath $nativeCacheDirectory -Recurse -File -Filter $fileName -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($null -eq $matchingSdkFile) {
            $nativeSdkReady = $false
            break
        }
    }
}

if ($nativeSdkReady) {
    Invoke-NativeRuntimeBuild
} else {
    Write-Host "The cached native SDK is incomplete; using an isolated sherpa-onnx-sys build."
}

$missingRuntimeFiles = @(
    $requiredFiles | Where-Object {
        -not (Test-Path -LiteralPath (Join-Path $profileDirectory $_) -PathType Leaf)
    }
)

if (-not $nativeSdkReady -or $missingRuntimeFiles.Count -gt 0) {
    # rust-cache intentionally filters non-Cargo artifacts. A restored
    # sherpa-onnx-sys build can therefore look current while its copied shared
    # libraries are absent. Build the dependency in an isolated target so its
    # build script must recreate the runtime files without clearing the shared
    # Cargo cache.
    Write-Host "Native runtime files are missing after the cached build; regenerating sherpa-onnx-sys."
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

    # Cached sherpa-onnx-sys metadata points the linker at this native SDK
    # tree. Restore it as well as the runtime DLLs/dylibs; Windows needs the
    # import .lib files from its lib directory during tests and app builds.
    $isolatedNativeCacheDirectory = Join-Path $isolatedTargetDirectory "sherpa-onnx-prebuilt"
    if (-not (Test-Path -LiteralPath $isolatedNativeCacheDirectory -PathType Container)) {
        throw "The isolated native SDK was not generated: $isolatedNativeCacheDirectory"
    }
    if (Test-Path -LiteralPath $nativeCacheDirectory) {
        $resolvedNativeCacheDirectory = [System.IO.Path]::GetFullPath($nativeCacheDirectory)
        $targetPrefix = $resolvedTargetDirectory.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
        if (-not $resolvedNativeCacheDirectory.StartsWith($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to replace native SDK outside Cargo target: $resolvedNativeCacheDirectory"
        }
        Remove-Item -LiteralPath $resolvedNativeCacheDirectory -Recurse -Force
    }
    Copy-Item -LiteralPath $isolatedNativeCacheDirectory -Destination $nativeCacheDirectory -Recurse -Force

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
