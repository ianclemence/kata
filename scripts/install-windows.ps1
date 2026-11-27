# Install Kata for the current user, like any Windows app:
#   %LOCALAPPDATA%\Programs\Kata, on PATH, in the Start menu, and in Settings > Apps.
# Run from the unzipped release folder (or pass -Source <folder>).
param(
    [string]$Source = $PSScriptRoot,
    [string]$Target = (Join-Path $env:LOCALAPPDATA "Programs\Kata")
)
$ErrorActionPreference = "Stop"

$exe = Join-Path $Source "kata.exe"
if (-not (Test-Path $exe)) { throw "kata.exe is not in $Source" }

# Stop a running Kata web page so its files can be replaced.
Get-Process kata-web -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

New-Item -ItemType Directory -Force -Path $Target | Out-Null
foreach ($item in @("kata.exe", "kata-web.exe", "onnxruntime.dll", "onnxruntime_providers_shared.dll", "kata.ico", "README.md", "CHANGELOG.md", "LICENSE-MIT", "LICENSE-APACHE", "uninstall-windows.ps1")) {
    $from = Join-Path $Source $item
    if (Test-Path $from) { Copy-Item $from $Target -Force }
}
foreach ($folder in @("skills", "examples", "docs", "assets")) {
    $from = Join-Path $Source $folder
    if (Test-Path $from) { Copy-Item $from $Target -Recurse -Force }
}

# Start menu shortcut. It must point at Kata's own program: Windows leaves
# shortcuts to script hosts (wscript, powershell) out of its app list.
Remove-Item (Join-Path $Target "launch.vbs") -Force -ErrorAction SilentlyContinue
$programs = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut((Join-Path $programs "Kata.lnk"))
$link.TargetPath = "$Target\kata-web.exe"
$link.Arguments = "--open"
$link.WorkingDirectory = $Target
$link.IconLocation = "$Target\kata.ico,0"
$link.WindowStyle = 7
$link.Description = "Kata: cut, print, compose and review"
$link.Save()

# kata on the user PATH.
$path = [Environment]::GetEnvironmentVariable("Path", "User")
$parts = @($path -split ";" | Where-Object { $_ -ne "" })
if ($parts -notcontains $Target) {
    [Environment]::SetEnvironmentVariable("Path", ((@($parts) + $Target) -join ";"), "User")
}

# Settings > Apps entry with an uninstaller.
$version = (& $exe --version) -replace "^kata\s+", ""
$key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Kata"
New-Item -Path $key -Force | Out-Null
$uninstall = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$Target\uninstall-windows.ps1`""
$values = @{
    DisplayName = "Kata"; DisplayVersion = $version; Publisher = "Ian Clemence"
    DisplayIcon = "$Target\kata.ico"; InstallLocation = $Target
    UninstallString = $uninstall; QuietUninstallString = $uninstall
    URLInfoAbout = "https://github.com/ianclemence/kata"; NoModify = 1; NoRepair = 1
}
foreach ($name in $values.Keys) {
    $type = if ($values[$name] -is [int]) { "DWord" } else { "String" }
    New-ItemProperty -Path $key -Name $name -Value $values[$name] -PropertyType $type -Force | Out-Null
}
$size = [int]((Get-ChildItem $Target -Recurse | Measure-Object Length -Sum).Sum / 1KB)
New-ItemProperty -Path $key -Name EstimatedSize -Value $size -PropertyType DWord -Force | Out-Null

Write-Host "Kata $version installed to $Target"
Write-Host "Open a new terminal and run: kata status"
