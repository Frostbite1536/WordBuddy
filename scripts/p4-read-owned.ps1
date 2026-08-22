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
$vp = $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
Write-Output ("TEXT=[" + ([System.Windows.Automation.ValuePattern]$vp).Current.Value.Replace("`r"," ").Replace("`n"," ") + "]")
