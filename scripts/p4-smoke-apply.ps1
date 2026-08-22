$ErrorActionPreference = 'Stop'
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WinP4 {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr hWnd, [MarshalAs(UnmanagedType.LPWStr)] StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
}
"@
$widgetHwnd = [IntPtr]::Zero
$cb = {
  param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [void][WinP4]::GetWindowTextW($h, $sb, 256)
  if ($sb.ToString() -eq 'WordBuddy suggestions') { $script:widgetHwnd = $h }
  return $true
}
[void][WinP4]::EnumWindows($cb, [IntPtr]::Zero)
if ($widgetHwnd -eq [IntPtr]::Zero) { Write-Output 'NO_WIDGET'; exit 0 }
$r = New-Object WinP4+RECT
[void][WinP4]::GetWindowRect($widgetHwnd, [ref]$r)
Write-Output ("WIDGET_RECT left=$($r.L) top=$($r.T) right=$($r.R) bottom=$($r.B)")

# Notepad rect for the near-field assertion
$note = Get-Process notepad | Where-Object { $_.MainWindowTitle -like '*Untitled*' } | Select-Object -First 1
$nr = New-Object WinP4+RECT
[void][WinP4]::GetWindowRect($note.MainWindowHandle, [ref]$nr)
Write-Output ("NOTEPAD_RECT left=$($nr.L) top=$($nr.T) right=$($nr.R) bottom=$($nr.B)")

# Drive apply: focus the widget, Tab into the card, Enter applies row 0
# chip 1 (the first listed correction).
Add-Type -AssemblyName System.Windows.Forms
[void][WinP4]::SetForegroundWindow($widgetHwnd)
Start-Sleep -Milliseconds 600
[System.Windows.Forms.SendKeys]::SendWait('{TAB}')
Start-Sleep -Milliseconds 250
[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
Start-Sleep -Milliseconds 1500
# Read the notepad text via its automation value
$np = Get-Process notepad | Where-Object { $_.MainWindowTitle -like '*Untitled*' } | Select-Object -First 1
Write-Output ("WIDGET_KEYBOARD_APPLY_DONE")
