$repoRoot = Split-Path -Parent $PSScriptRoot
$binaryPath = Join-Path $repoRoot 'src-tauri\target\release\wordbuddy.exe'
if (-not (Test-Path -LiteralPath $binaryPath)) {
    throw "Release binary not found at $binaryPath. Run npx tauri build first."
}
$t0 = Get-Date
$p = Start-Process -FilePath $binaryPath -PassThru
$deadline = $t0.AddSeconds(20)
$ok = $false
while (-not $ok -and (Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 100
    try {
        $c = New-Object Net.Sockets.TcpClient
        $ok = $c.ConnectAsync('127.0.0.1', 19521).Wait(100)
        $c.Close()
    } catch { $ok = $false }
}
$t1 = Get-Date
if ($ok) { Write-Output ("READY_MS=" + [int]($t1 - $t0).TotalMilliseconds) } else { Write-Output "NOT_READY" }
Get-Process wordbuddy -ErrorAction SilentlyContinue | Select-Object Id, MainWindowTitle, Responding | Format-Table
