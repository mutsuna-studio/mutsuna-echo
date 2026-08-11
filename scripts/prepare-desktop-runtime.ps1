param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("windows", "macos")]
    [string]$Platform,
    [string]$Target = "",
    [string]$OutputDirectory = ""
)

$ErrorActionPreference = "Stop"
$version = "1.13.4"
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$archiveName = if ($Platform -eq "windows") {
    "sherpa-onnx-v$version-win-x64-shared-MT-Release-lib.tar.bz2"
} else {
    "sherpa-onnx-v$version-osx-arm64-shared-lib.tar.bz2"
}
$required = if ($Platform -eq "windows") {
    @("onnxruntime.dll", "onnxruntime_providers_shared.dll", "sherpa-onnx-c-api.dll")
} else {
    @("libonnxruntime.dylib", "libonnxruntime.1.27.0.dylib", "libsherpa-onnx-c-api.dylib")
}
$cacheRoot = Join-Path ([System.IO.Path]::GetTempPath()) "mutsuna-local-ai-runtime-v$version-$Platform"
$archivePath = Join-Path $cacheRoot $archiveName
$extracted = Join-Path $cacheRoot "extracted"
$releaseUrl = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$version/$archiveName"

New-Item -ItemType Directory -Force -Path $cacheRoot | Out-Null
if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf)) {
    Invoke-WebRequest -Uri $releaseUrl -OutFile "$archivePath.part"
    Move-Item -LiteralPath "$archivePath.part" -Destination $archivePath
}
$release = Invoke-RestMethod -Uri "https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/tags/v$version"
$asset = $release.assets | Where-Object { $_.name -eq $archiveName } | Select-Object -First 1
if ($null -eq $asset -or $asset.digest -notmatch '^sha256:([0-9a-f]{64})$') {
    throw "GitHub did not return a SHA-256 digest for $archiveName."
}
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant() -ne $Matches[1]) {
    throw "The downloaded Sherpa archive failed SHA-256 verification."
}
if (-not (Test-Path -LiteralPath $extracted -PathType Container)) {
    New-Item -ItemType Directory -Force -Path $extracted | Out-Null
    & tar -xjf $archivePath -C $extracted
    if ($LASTEXITCODE -ne 0) { throw "Could not extract $archiveName." }
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $profile = if ([string]::IsNullOrWhiteSpace($Target)) { "target/release" } else { "target/$Target/release" }
    $OutputDirectory = Join-Path (Join-Path $repositoryRoot "src-tauri") $profile
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
foreach ($name in $required) {
    $matches = @(Get-ChildItem -LiteralPath $extracted -Recurse -File -Filter $name)
    if ($matches.Count -lt 1) { throw "Required runtime file is missing: $name" }
    Copy-Item -LiteralPath $matches[0].FullName -Destination (Join-Path $OutputDirectory $name) -Force
}
Write-Host "Prepared on-demand runtime in $OutputDirectory"
