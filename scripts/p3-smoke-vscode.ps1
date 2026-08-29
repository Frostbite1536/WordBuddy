$ErrorActionPreference = 'Stop'
$codeCommand = Get-Command code -ErrorAction SilentlyContinue
$codePath = if ($codeCommand) {
    $codeCommand.Source
} else {
    Join-Path $env:LOCALAPPDATA 'Programs\Microsoft VS Code\Code.exe'
}
if (-not (Test-Path -LiteralPath $codePath)) {
    throw 'VS Code was not found on PATH or in the default per-user install location.'
}
$smokeFile = Join-Path $env:TEMP 'wordbuddy-vscode-smoke.txt'
Start-Process -FilePath $codePath -ArgumentList $smokeFile
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
