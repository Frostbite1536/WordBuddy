$ErrorActionPreference = 'Stop'
# P4 gate smoke A: notepad errors -> widget window appears.
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WinEnum {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr hWnd, [MarshalAs(UnmanagedType.LPWStr)] StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
}
"@
# Focus untitled notepad + type
$proc = Get-Process notepad | Where-Object { $_.MainWindowTitle -like '*Untitled*' } | Select-Object -First 1
if (-not $proc) { Start-Process notepad.exe; Start-Sleep -Milliseconds 1500; $proc = Get-Process notepad | Where-Object { $_.MainWindowTitle -like '*Untitled*' } | Select-Object -First 1 }
[void][WinEnum]::SetForegroundWindow($proc.MainWindowHandle)
Start-Sleep -Milliseconds 700
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait('^a')
[System.Windows.Forms.SendKeys]::SendWait('This is teh smae recieve with a mispeling')
Start-Sleep -Seconds 3

# Look for the widget window
$found = @()
$cb = {
  param($h, $l)
  if ([WinEnum]::IsWindowVisible($h)) {
    $sb = New-Object System.Text.StringBuilder 256
    [void][WinEnum]::GetWindowTextW($h, $sb, 256)
    $t = $sb.ToString()
    if ($t -like '*WordBuddy*') { $script:found += $t }
  }
  return $true
}
[void][WinEnum]::EnumWindows($cb, [IntPtr]::Zero)
Write-Output ("WIDGET_WINDOWS: " + ($found -join ' | '))
