$ErrorActionPreference = 'Stop'
# P4 end-to-end apply drill on an owned EDGE window (real clicks only).
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinEdge {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  public const uint LEFTDOWN = 0x02, LEFTUP = 0x04;
}
"@
Add-Type -AssemblyName System.Windows.Forms

function Find-EdgeRect {
  $sig = @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WE {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr hWnd, [MarshalAs(UnmanagedType.LPWStr)] StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L; public int T; public int R2; public int B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out R r);
}
'@
  Add-Type -TypeDefinition $sig
  $script:rect = $null
  $cb = {
    param($h, $l)
    if ([WE]::IsWindowVisible($h)) {
      $sb = New-Object System.Text.StringBuilder 256
      [void][WE]::GetWindowTextW($h, $sb, 256)
      if ($sb.ToString() -like '*WordBuddy checker playground*') {
        $r = New-Object WE+R
        [void][WE]::GetWindowRect($h, [ref]$r)
        $script:rect = @{ L = $r.L; T = $r.T; R2 = $r.R2; B = $r.B }
        return $false
      }
    }
    return $true
  }
  [void][WE]::EnumWindows($cb, [IntPtr]::Zero)
  return $script:rect
}

$rc = Find-EdgeRect
if (-not $rc) { Write-Output 'EDGE_WINDOW_NOT_FOUND'; exit 1 }
Write-Output ("EDGE_RECT L=" + $rc.L + " T=" + $rc.T)

# Click into the textarea (page layout: h1+p+fieldset => textarea ~y+210).
$cx = $rc.L + 320
$cy = $rc.T + 230
[void][WinEdge]::SetCursorPos($cx, $cy)
Start-Sleep -Milliseconds 400
[WinEdge]::mouse_event([WinEdge]::LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 60
[WinEdge]::mouse_event([WinEdge]::LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 500

# Type seeded errors with real keystrokes (foreground is now Edge).
[System.Windows.Forms.SendKeys]::SendWait('^a')
Start-Sleep -Milliseconds 150
[System.Windows.Forms.SendKeys]::SendWait('This is teh smae recieve with a mispeling')
Start-Sleep -Seconds 3
Write-Output 'TYPED_IN_EDGE'
