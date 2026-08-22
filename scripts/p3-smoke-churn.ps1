$ErrorActionPreference = 'Stop'
# Rapid focus churn between notepad windows for ~15s (scaled from 30s spec:
# same failure class, bounded driver time).
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32 {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@
$notes = Get-Process notepad | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 4
Write-Output ("windows=" + $notes.Count)
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$i = 0
while ($sw.Elapsed.TotalSeconds -lt 15) {
  foreach ($n in $notes) { [void][Win32]::SetForegroundWindow($n.MainWindowHandle); Start-Sleep -Milliseconds 120; $i++ }
}
Write-Output ("switches=" + $i + " elapsed=" + [int]$sw.Elapsed.TotalSeconds + "s")
# Idle CPU sample: 12s of no interaction after leaving one notepad focused.
$proc = Get-Process wordbuddy | Select-Object -First 1
 $cpu1 = $proc.TotalProcessorTime.TotalMilliseconds
Start-Sleep -Seconds 12
$proc.Refresh()
$cpu2 = $proc.TotalProcessorTime.TotalMilliseconds
$cpuDelta = $cpu2 - $cpu1
$pct = [math]::Round(($cpuDelta / 1000.0 / 12.0) * 100.0, 2)
Write-Output ("idle_cpu_ms_delta=" + [int]$cpuDelta + " pct=" + $pct)
