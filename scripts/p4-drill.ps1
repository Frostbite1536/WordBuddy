param([string]$Tag = "")
$ErrorActionPreference = 'Stop'
# P4 apply drill against the OWNED notepad (unique tag) + clipboard
# canary + palette hotkey. No other window is ever touched.
if (-not $Tag) { Write-Output "USAGE: -Tag <unique-tag>"; exit 1 }
$mine = Get-Process notepad | Where-Object { $_.MainWindowTitle -like "*$Tag*" } | Select-Object -First 1
if (-not $mine) { Write-Output "OWNED_NOTEPAD_GONE"; exit 1 }

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public struct WRECT { public int L; public int T; public int R; public int B; }
public class WinD {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out WRECT r);
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr hWnd, [MarshalAs(UnmanagedType.LPWStr)] StringBuilder sb, int max);
}
"@
Add-Type -AssemblyName System.Windows.Forms

# --- 1. Focus our notepad, focus widget, Enter applies row 0 chip ---
[void][WinD]::SetForegroundWindow($mine.MainWindowHandle)
Start-Sleep -Milliseconds 900
$widgetHwnd = [IntPtr]::Zero
$cb = {
  param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [void][WinD]::GetWindowTextW($h, $sb, 256)
  if ($sb.ToString() -eq 'WordBuddy suggestions') { $script:widgetHwnd = $h }
  return $true
}
[void][WinD]::EnumWindows($cb, [IntPtr]::Zero)
if ($widgetHwnd -eq [IntPtr]::Zero) { Write-Output "NO_WIDGET_VISIBLE"; exit 1 }
Write-Output "WIDGET_PRESENT"
# Real mouse click on the first row's primary chip: SetForegroundWindow
# from a background script is blocked by the OS foreground lock, so
# keyboard injection into the widget is unreliable from a driver. A
# synthesized click at the chip's screen position works regardless.
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class MouseP4 {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  public const uint LEFTDOWN = 0x02, LEFTUP = 0x04;
}
"@
$wr = New-Object WRECT
[void][WinD]::GetWindowRect($widgetHwnd, [ref]$wr)
# First issue row: header ~46px, row top ~56px; primary chip ~y+70, x centered-left ~40px in.
$chipX = $wr.L + 44
$chipY = $wr.T + 76
[void][MouseP4]::SetCursorPos($chipX, $chipY)
Start-Sleep -Milliseconds 350
[MouseP4]::mouse_event([MouseP4]::LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 60
[MouseP4]::mouse_event([MouseP4]::LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
Write-Output ("CLICKED at $chipX,$chipY")
Start-Sleep -Milliseconds 2500

# --- 2. Read our notepad text back (select-all copy, clipboard is
#        expendable at this point) ---
[void][WinD]::SetForegroundWindow($mine.MainWindowHandle)
Start-Sleep -Milliseconds 800
[System.Windows.Forms.SendKeys]::SendWait('^a')
Start-Sleep -Milliseconds 150
[System.Windows.Forms.SendKeys]::SendWait('^c')
Start-Sleep -Milliseconds 600
$text = Get-Clipboard -Raw
Write-Output ("AFTER_APPLY=[" + $text.Replace("`r"," ").Replace("`n"," ") + "]")

# --- 3. Clipboard canary + selection rewrite hotkey ---
Set-Clipboard -Value "CLIPBOARD_CANARY_9f2a"
# Select the first word, then fire the global hotkey.
[System.Windows.Forms.SendKeys]::SendWait('{HOME}')
Start-Sleep -Milliseconds 150
[System.Windows.Forms.SendKeys]::SendWait('+{RIGHT 4}')
Start-Sleep -Milliseconds 300
[System.Windows.Forms.SendKeys]::SendWait('^+w')
Start-Sleep -Seconds 2
$after = Get-Clipboard -Raw
Write-Output ("CLIPBOARD_AFTER_HOTKEY=[" + $after + "]")
Write-Output ("CANARY_SURVIVED=" + ($after -eq 'CLIPBOARD_CANARY_9f2a'))
Write-Output "DRILL_DONE"
