$ErrorActionPreference = 'Stop'
Start-Process "C:\Users\LCM\AppData\Local\Programs\Microsoft VS Code\Code.exe" -ArgumentList "C:\Users\LCM\wb-smoke.txt"
Start-Sleep -Seconds 6
# Make sure VS Code's editor has focus: activate its window.
$code = Get-Process Code | Where-Object { $_.MainWindowTitle -like '*wb-smoke*' } | Select-Object -First 1
if (-not $code) { Write-Output 'NO_VSCODE_WINDOW'; exit 0 }
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32b {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@
[void][Win32b]::SetForegroundWindow($code.MainWindowHandle)
Start-Sleep -Milliseconds 800
Add-Type -AssemblyName System.Windows.Forms
# Select-all + retype so the document changes and the editor is focused.
[System.Windows.Forms.SendKeys]::SendWait('^a')
[System.Windows.Forms.SendKeys]::SendWait('This is teh smae recieve with a mispeling')
Start-Sleep -Seconds 3
Write-Output ("VSCODE_TYPED pid=" + $code.Id)
