$ErrorActionPreference = "Stop"

$forbidden = @{
    "iptools-core" = @("ratatui", "tokio", "crossterm", "ratzilla", "reqwest", "sysinfo", "socket2", "wmi")
    "iptools-ui"   = @("tokio", "crossterm", "ratzilla", "reqwest", "sysinfo", "socket2", "wmi")
    "iptools-demo" = @("tokio", "crossterm", "ratzilla", "reqwest", "sysinfo", "socket2", "wmi")
}

foreach ($crate in $forbidden.Keys) {
    # Query each package separately. A single workspace-wide metadata graph merges
    # optional features enabled by native (for example Ratatui's Crossterm backend)
    # and would incorrectly attribute those features to iptools-ui.
    $tree = @(& cargo tree --locked -p $crate -e normal --prefix none)
    if ($LASTEXITCODE -ne 0) { throw "failed to inspect dependency tree for $crate" }
    $names = @($tree | ForEach-Object {
        if ($_ -match '^([A-Za-z0-9_-]+) v') { $Matches[1] }
    } | Where-Object { $_ -and $_ -ne $crate } | Sort-Object -Unique)
    $bad = @($forbidden[$crate] | Where-Object { $names -contains $_ })
    if ($bad.Count -gt 0) { throw "$crate has forbidden dependencies: $($bad -join ', ')" }
}

$nativeRoot = Join-Path $PSScriptRoot "../crates/iptools-native/src"
$removedLegacyFiles = @(
    "app.rs",
    "tui.rs",
    "modules/adapter.rs",
    "modules/dashboard.rs",
    "modules/scanner.rs",
    "modules/settings.rs",
    "modules/traffic.rs",
    "modules/diagnostics/port_scan.rs"
)
foreach ($relative in $removedLegacyFiles) {
    if (Test-Path (Join-Path $nativeRoot $relative)) {
        throw "legacy native state/UI file still exists: $relative"
    }
}

$nativeSources = Get-ChildItem $nativeRoot -Recurse -Filter *.rs
$forbiddenNativePatterns = @(
    "unbounded_channel",
    "Arc<Mutex<bool>>",
    "AtomicBool"
)
foreach ($pattern in $forbiddenNativePatterns) {
    $matches = @($nativeSources | Select-String -SimpleMatch $pattern)
    if ($matches.Count -gt 0) {
        throw "native source still contains forbidden lifecycle pattern '$pattern': $($matches[0].Path):$($matches[0].LineNumber)"
    }
}

$main = Get-Content -Raw (Join-Path $nativeRoot "main.rs")
if (-not $main.Contains("native_app::run(args.config).await?")) {
    throw "default native entry is not using the shared AppModel runner"
}

Write-Host "crate dependency and native lifecycle boundaries passed"
