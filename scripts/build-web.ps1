param(
    [string]$PublicUrl = "/"
)

$ErrorActionPreference = "Stop"
$env:NO_COLOR = "true"

$trunk = Get-Command trunk -ErrorAction SilentlyContinue
if (-not $trunk) {
    $bundled = Join-Path $PSScriptRoot "../target/tools/trunk-0.21.14/trunk.exe"
    if (Test-Path $bundled) {
        $trunkPath = (Resolve-Path $bundled).Path
    } else {
        throw "Trunk 0.21.14 is required; install it with cargo install trunk --version 0.21.14 --locked"
    }
} else {
    $trunkPath = $trunk.Source
}

Push-Location (Join-Path $PSScriptRoot "../crates/iptools-web")
try {
    & $trunkPath build --release --public-url $PublicUrl
    if ($LASTEXITCODE -ne 0) { throw "Trunk build failed with exit code $LASTEXITCODE" }

    # Trunk determines the JS/WASM/CSS hashes at build time. Inject those exact
    # files into the Service Worker install list so the first online visit is
    # already sufficient for a complete offline reload.
    $dist = Join-Path (Get-Location) "dist"
    $assets = @(Get-ChildItem $dist -File | Where-Object {
        $_.Extension -in ".js", ".wasm", ".css" -and $_.Name -ne "service-worker.js"
    } | Sort-Object Name | ForEach-Object { "./$($_.Name)" })
    if ($assets.Count -eq 0) { throw "Trunk produced no hashed Web assets" }

    $assetJson = ConvertTo-Json -Compress -InputObject $assets
    $signature = [Text.Encoding]::UTF8.GetBytes(($assets -join "`n"))
    $digest = [Security.Cryptography.SHA256]::HashData($signature)
    $buildHash = ([BitConverter]::ToString($digest)).Replace("-", "").ToLowerInvariant().Substring(0, 16)
    $serviceWorkerPath = Join-Path $dist "service-worker.js"
    $serviceWorker = Get-Content -Raw $serviceWorkerPath
    if (-not $serviceWorker.Contains("/*__IPTOOLS_ASSETS__*/ []") -or
        -not $serviceWorker.Contains("__IPTOOLS_BUILD_HASH__")) {
        throw "Service Worker build placeholders are missing"
    }
    $serviceWorker = $serviceWorker.Replace("/*__IPTOOLS_ASSETS__*/ []", $assetJson)
    $serviceWorker = $serviceWorker.Replace("__IPTOOLS_BUILD_HASH__", $buildHash)
    [IO.File]::WriteAllText(
        $serviceWorkerPath,
        $serviceWorker,
        [Text.UTF8Encoding]::new($false)
    )
} finally {
    Pop-Location
}
