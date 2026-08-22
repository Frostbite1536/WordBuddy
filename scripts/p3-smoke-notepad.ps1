$ErrorActionPreference = 'Stop'
# Gate smoke 1: Notepad + seeded errors via SendKeys.
Start-Process notepad.exe
Start-Sleep -Milliseconds 1500
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait('This is teh smae recieve with a mispeling')
Start-Sleep -Milliseconds 2500
Write-Output 'SENT_KEYS_DONE'
