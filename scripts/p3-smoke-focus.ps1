$ErrorActionPreference = 'Stop'
$proc = Get-Process notepad | Where-Object { $_.MainWindowTitle -like '*Untitled*' } | Select-Object -First 1
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32 {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@
[Win32]::SetForegroundWindow($proc.MainWindowHandle) | Out-Null
Start-Sleep -Milliseconds 800
# nudge a change so the debounce has fresh input
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait('{END}')
[System.Windows.Forms.SendKeys]::SendWait(' agian')
Start-Sleep -Milliseconds 2000
Write-Output ("FOREGROUND_SET pid=" + $proc.Id)
