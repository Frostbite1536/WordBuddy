$ErrorActionErrorPreference = 'Stop'
$ErrorActionPreference = 'Stop'
# P4 owned-window apply smoke. Binds STRICTLY to a uniquely-titled
# notepad opened from a temp file with a random name, driven by PID.
$tag = "wbp4-" + [System.IO.Path]::GetRandomFileName().Replace('.','')
$file = Join-Path $env:TEMP "$tag.txt"
Set-Content -Path $file -Value "This is teh smae recieve with a mispeling" -Encoding UTF8
Start-Process notepad.exe -ArgumentList $file
Start-Sleep -Milliseconds 1800
# Identify OUR notepad by title containing the unique tag.
$mine = Get-Process notepad | Where-Object { $_.MainWindowTitle -like "*$tag*" } | Select-Object -First 1
if (-not $mine) { Write-Output "OWNED_NOTEPAD_NOT_FOUND"; exit 1 }
Write-Output ("OWNED pid=" + $mine.Id + " title=[" + $mine.MainWindowTitle + "]")

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinO {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@
Add-Type -AssemblyName System.Windows.Forms
[void][WinO]::SetForegroundWindow($mine.MainWindowHandle)
Start-Sleep -Milliseconds 900
# Nudge a text change (select-all retyped) so the monitor sees OUR field.
[System.Windows.Forms.SendKeys]::SendWait('^a')
[System.Windows.Forms.SendKeys]::SendWait('This is teh smae recieve with a mispeling')
Start-Sleep -Seconds 3
Write-Output "TYPED_OK"
