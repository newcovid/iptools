$ErrorActionPreference = "Stop"
$env:NO_COLOR = "true"

function Invoke-Checked {
    param([scriptblock]$Command, [string]$Name)
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$Name failed with exit code $LASTEXITCODE" }
}

Invoke-Checked { cargo fmt --all -- --check } "cargo fmt"
Invoke-Checked { cargo clippy --workspace --all-targets -- -D warnings } "cargo clippy"
Invoke-Checked { cargo test --workspace } "cargo test"
Invoke-Checked { cargo build --release } "native release build"
Invoke-Checked { cargo check -p iptools-web --target wasm32-unknown-unknown } "WASM check"
Invoke-Checked { cargo test --manifest-path vendor/ratzilla/Cargo.toml --lib } "vendored Ratzilla regression tests"

& (Join-Path $PSScriptRoot "check-architecture.ps1")
& (Join-Path $PSScriptRoot "verify-web-font.ps1")
& (Join-Path $PSScriptRoot "build-web.ps1") -PublicUrl "/iptools/"
& (Join-Path $PSScriptRoot "check-web-size.ps1")

Write-Host "iptools verification passed"
