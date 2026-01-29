$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

Invoke-WebRequest -Uri "https://github.com/disig/SoftHSM2-for-Windows/releases/download/v2.5.0/SoftHSM2-2.5.0.msi" -OutFile 'softhsm2.msi'
msiexec /i 'softhsm2.msi' /qn /norestart /log "$env:TEMP\softhsm2_install.log"
