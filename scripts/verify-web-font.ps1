$ErrorActionPreference = "Stop"

$candidates = @()
foreach ($name in @("python", "python3")) {
    $command = Get-Command $name -ErrorAction SilentlyContinue
    if ($command) { $candidates += $command.Source }
}
if (Test-Path "C:\Python313\python.exe") {
    $candidates += "C:\Python313\python.exe"
}

$fontPython = $null
foreach ($candidate in ($candidates | Select-Object -Unique)) {
    & $candidate -c "import fontTools, brotli" 2>$null
    if ($LASTEXITCODE -eq 0) {
        $fontPython = $candidate
        break
    }
}

if (-not $fontPython) {
    throw 'Font verification requires: python -m pip install "fonttools[woff]==4.61.0"'
}

& $fontPython (Join-Path $PSScriptRoot "subset-web-font.py") --verify-only
if ($LASTEXITCODE -ne 0) { throw "Web font verification failed" }
