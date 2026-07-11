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

Write-Host "crate dependency boundaries passed"
