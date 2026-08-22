$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$notes = Get-Process notepad | Where-Object { $_.MainWindowHandle -ne 0 }
foreach ($n in $notes) {
  $el = [System.Windows.Automation.AutomationElement]::FromHandle($n.MainWindowHandle)
  # Find the first Text/Edit descendant exposing a ValuePattern
  $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::IsValuePatternAvailableProperty, $true)
  $edit = $el.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
  $val = ''
  if ($edit) {
    try { $vp = $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern); $val = ([System.Windows.Automation.ValuePattern]$vp).Current.Value } catch {}
  }
  $preview = if ($val.Length -gt 60) { $val.Substring(0, 60) } else { $val }
  Write-Output ("pid=" + $n.Id + " title=[" + $n.MainWindowTitle + "] text=[" + $preview.Replace("`r"," ").Replace("`n"," ") + "]")
}
