#Requires -Version 7.3
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

Invoke-WebRequest -Uri 'https://github.com/disig/SoftHSM2-for-Windows/releases/download/v2.5.0/SoftHSM2-2.5.0-portable.zip' -OutFile 'softhsm2.zip'
Expand-Archive -Path 'softhsm2.zip' -DestinationPath 'C:\SoftHSM2'
