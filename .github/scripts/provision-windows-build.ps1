#Requires -Version 7.3
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

# enormous speedup in CI, but not mandatory
cd C:\vcpkg
git checkout 2025.12.12
.\bootstrap-vcpkg.bat -disableMetrics
$env:VCPKG_BUILD_TYPE = 'release'
.\vcpkg install openssl:x64-windows-static-md
.\vcpkg integrate install
'OPENSSL_STATIC=1' >> $env:GITHUB_ENV
'OPENSSL_NO_VENDOR=1' >> $env:GITHUB_ENV
