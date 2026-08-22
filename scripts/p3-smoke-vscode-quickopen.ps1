$ErrorActionPreference = 'Stop'
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32c {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@
Add-Type -AssemblyName System.Windows.Forms
$code = Get-Process Code | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
[void][Win32c]::SetForegroundWindow($code.MainWindowHandle)
Start-Sleep -Milliseconds 600
# Quick-open the smoke file: Ctrl+P, filename, Enter.
[System.Windows.Forms.SendKeys]::SendWait('^p')
Start-Sleep -Milliseconds 600
[System.Windows.Forms.SendKeys]::SendWait('wb-smoke')
Start-Sleep -Milliseconds 700
[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
Start-Sleep -Milliseconds 1500
# Select all + retype so the buffer changes with the editor focused.
[System.Windows.Forms.SendKeys]::SendWait('^a')
Start-Sleep -Milliseconds 200
[System.Windows.Forms.SendKeys]::SendWait('This is teh smae recieve with a mispeling')
Start-Sleep -Seconds 4
$title = (Get-Process Code | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1).MainWindowTitle
Write-Output ("VSCODE_TYPED title=" + $title)
