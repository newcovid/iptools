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
} finally {
    Pop-Location
}
