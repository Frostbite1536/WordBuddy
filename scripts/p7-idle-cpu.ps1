$pid = (Get-Process wordbuddy | Select-Object -First 1).Id
$coreCount = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
$samples = @()
$intervalSec = 10
for ($i = 0; $i -lt 60; $i++) {
    $p1 = Get-Process -Id $pid
    $c1 = $p1.TotalProcessorTime.TotalSeconds
    Start-Sleep -Seconds $intervalSec
    $p2 = Get-Process -Id $pid -ErrorAction SilentlyContinue
    if (-not $p2) { Write-Output "PROCESS_EXITED at sample $i"; break }
    $cpuDelta = $p2.TotalProcessorTime.TotalSeconds - $c1
    $oneCorePct = [math]::Round(100 * $cpuDelta / $intervalSec, 3)
    $machinePct = [math]::Round(100 * $cpuDelta / ($intervalSec * $coreCount), 3)
    $samples += [pscustomobject]@{ OneCorePct = $oneCorePct; MachinePct = $machinePct }
}
$avg1 = [math]::Round(($samples | Measure-Object OneCorePct -Average).Average, 3)
$avgM = [math]::Round(($samples | Measure-Object MachinePct -Average).Average, 3)
$maxM = [math]::Round(($samples | Measure-Object MachinePct -Maximum).Maximum, 3)
Write-Output ("SAMPLES=" + $samples.Count + " AVG_ONE_CORE_PCT=" + $avg1 + " AVG_MACHINE_PCT=" + $avgM + " MAX_MACHINE_PCT=" + $maxM + " CORES=" + $coreCount)
