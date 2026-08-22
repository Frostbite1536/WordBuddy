$ErrorActionPreference = 'Stop'
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinRead {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@
Add-Type -AssemblyName System.Windows.Forms
$note = Get-Process notepad | Where-Object { $_.MainWindowTitle -like '*Untitled*' } | Select-Object -First 1
[void][WinRead]::SetForegroundWindow($note.MainWindowHandle)
Start-Sleep -Milliseconds 700
[System.Windows.Forms.SendKeys]::SendWait('^a')
Start-Sleep -Milliseconds 200
[System.Windows.Forms.SendKeys]::SendWait('^c')
Start-Sleep -Milliseconds 500
$text = Get-Clipboard -Raw
Write-Output ("NOTEPAD_TEXT=[" + $text + "]")
