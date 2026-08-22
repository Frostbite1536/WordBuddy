param([int]$TargetPid = 0)
$ErrorActionPreference = 'Stop'
# Read the focused-field value of ONE notepad by PID (UIA ValuePattern).
if (-not $TargetPid) { Write-Output "USAGE: -TargetPid <pid>"; exit 1 }
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$proc = Get-Process -Id $TargetPid
$el = [System.Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle)
$editCond = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
  [System.Windows.Automation.ControlType]::Edit)
$edit = $el.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $editCond)
if (-not $edit) {
  $valCond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::IsValuePatternAvailableProperty, $true)
  $edit = $el.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $valCond)
}
if (-not $edit) { Write-Output "NO_VALUE_FIELD"; exit 0 }
try {
  $vp = $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
  Write-Output ("TEXT=[" + ([System.Windows.Automation.ValuePattern]$vp).Current.Value.Replace("`r"," ").Replace("`n"," ") + "]")
} catch {
  # Modern Notepad exposes TextPattern only.
  $tp = $edit.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern)
  $doc = [System.Windows.Automation.Text.TextPatternRange]$tp.DocumentRange
  Write-Output ("TEXT=[" + $doc.GetText(-1).Replace("`r"," ").Replace("`n"," ") + "]")
}
