# Remove Kata: the program folder, Start menu shortcut, PATH entry and Settings > Apps entry.
# Models and caches in %LOCALAPPDATA%\kata are kept; delete that folder too to remove them.
$ErrorActionPreference = "SilentlyContinue"
$Target = Join-Path $env:LOCALAPPDATA "Programs\Kata"

Get-Process kata-web, kata | Stop-Process -Force

Remove-Item (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Kata.lnk") -Force
Remove-Item "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Kata" -Recurse -Force

$path = [Environment]::GetEnvironmentVariable("Path", "User")
$kept = @($path -split ";" | Where-Object { $_ -ne "" -and $_ -ne $Target })
[Environment]::SetEnvironmentVariable("Path", ($kept -join ";"), "User")

# The running script lives in the folder, so delete it after this process exits.
Start-Process -WindowStyle Hidden cmd.exe -ArgumentList "/c timeout /t 2 > nul & rmdir /s /q `"$Target`""
Write-Host "Kata removed. Models and caches remain in $env:LOCALAPPDATA\kata."
