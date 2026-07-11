$ErrorActionPreference = "Stop"
$dist = Join-Path $PSScriptRoot "../crates/iptools-web/dist"
$files = Get-ChildItem $dist -Recurse -File
$wasmAndJs = ($files | Where-Object Extension -in ".wasm", ".js" | Measure-Object Length -Sum).Sum
$total = ($files | Measure-Object Length -Sum).Sum
if ($wasmAndJs -gt 2.5MB) { throw "JS + WASM exceeds 2.5 MiB: $wasmAndJs bytes" }
if ($total -gt 4MB) { throw "Web exhibit exceeds 4 MiB: $total bytes" }
Write-Host "Web size passed: JS+WASM=$wasmAndJs bytes, total=$total bytes"
