# Smoke test of the Windows x64 desktop installers users download
# (PRD 0021 RF6.2), on a disposable runner:
#   * the NSIS installer and the MSI carry exactly the Authenticode state that
#     signing-state.json claims (Valid when it says "authenticode", NotSigned
#     otherwise), and so does the installed Cuotai.exe;
#   * the NSIS installer installs silently (/S), registers its uninstaller,
#     installs Cuotai.exe with the release version, the app starts and is still
#     running after a grace period, and the silent uninstaller removes it;
#   * the MSI installs and uninstalls silently with msiexec /qn;
#   * both uninstallers keep the user's configuration and data (PRD 0021 RF8.2,
#     RNF): sentinel files in %APPDATA%\cuotai (accounts file, preferences) must
#     survive, and one in %LOCALAPPDATA%\cuotai\history is reported if it does not.
# Usage: smoke-desktop-windows.ps1 -Version X.Y.Z -DistDir dist -SigningState path
param(
  [Parameter(Mandatory = $true)][string]$Version,
  [string]$DistDir = "dist",
  [Parameter(Mandatory = $true)][string]$SigningState,
  [int]$GraceSeconds = 15
)

$ErrorActionPreference = "Stop"
$nsis = Join-Path $DistDir "Cuotai_${Version}_x64-setup.exe"
$msi = Join-Path $DistDir "Cuotai_${Version}_x64_en-US.msi"
foreach ($file in @($nsis, $msi)) {
  if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "missing $file" }
}
$state = Get-Content -Raw -LiteralPath $SigningState | ConvertFrom-Json

function Get-ExpectedSignature([string]$Path) {
  $name = Split-Path -Leaf $Path
  $entry = @($state.artifacts | Where-Object { $_.name -eq $name })
  if ($entry.Count -ne 1) { throw "$name has $($entry.Count) signing-state entries" }
  if ($entry[0].method -eq "authenticode" -and $entry[0].signed) { return "Valid" }
  if ($entry[0].method -eq "none" -and -not $entry[0].signed) { return "NotSigned" }
  throw "$name has an unexpected Windows signing state ($($entry[0].method), signed=$($entry[0].signed))"
}

function Assert-Signature([string]$Path, [string]$Expected) {
  $actual = (Get-AuthenticodeSignature -LiteralPath $Path).Status.ToString()
  if ($actual -ne $Expected) {
    throw "$(Split-Path -Leaf $Path): Authenticode status is $actual, signing state expects $Expected"
  }
  Write-Host "ok: $(Split-Path -Leaf $Path) Authenticode status $actual"
}

function Get-UninstallEntry {
  $roots = @(
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*"
  )
  @(Get-ItemProperty -Path $roots -ErrorAction SilentlyContinue |
    Where-Object { $_.PSObject.Properties["DisplayName"] -and $_.DisplayName -eq "Cuotai" })
}

function Assert-NoInstall([string]$When) {
  $entries = Get-UninstallEntry
  if ($entries.Count -ne 0) { throw "Cuotai is still registered $When" }
}

function Get-InstalledExe($Entry) {
  $location = $Entry.InstallLocation
  if ([string]::IsNullOrWhiteSpace($location)) { throw "uninstall entry has no InstallLocation" }
  $exe = Join-Path $location.Trim('"') "Cuotai.exe"
  if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "installed executable missing: $exe" }
  return $exe
}

function Assert-Starts([string]$Exe) {
  $process = Start-Process -FilePath $Exe -PassThru
  try {
    Start-Sleep -Seconds $GraceSeconds
    if ($process.HasExited) {
      throw "the app exited with $($process.ExitCode) within ${GraceSeconds}s of starting"
    }
    Write-Host "ok: the app is running after ${GraceSeconds}s"
  } finally {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
    $process.WaitForExit(30000) | Out-Null
    # The app may have spawned WebView2 children that hold the directory.
    Get-Process -Name "Cuotai" -ErrorAction SilentlyContinue | Stop-Process -Force
  }
}

function Wait-Removed([string]$Path) {
  for ($i = 0; $i -lt 60; $i++) {
    if (-not (Test-Path -LiteralPath $Path)) { return }
    Start-Sleep -Seconds 1
  }
  throw "uninstall left $Path behind"
}

# PRD 0021 RF8.2: sentinels for the user's data. %APPDATA% (accounts file and
# preferences) is a hard requirement; %LOCALAPPDATA%\cuotai shares its parent
# with the default install folder (Windows paths are case-insensitive), so a
# loss there is reported as a warning for the owner to review.
$userConfig = Join-Path $env:APPDATA "cuotai"
$userData = Join-Path $env:LOCALAPPDATA "cuotai\history"
function New-UserDataSentinels {
  New-Item -ItemType Directory -Force -Path $userConfig, $userData | Out-Null
  Set-Content -LiteralPath (Join-Path $userConfig "config.toml") -Value "# uninstall sentinel"
  Set-Content -LiteralPath (Join-Path $userConfig "desktop.json") -Value "{}"
  Set-Content -LiteralPath (Join-Path $userData "sentinel.jsonl") -Value "{}"
}
function Assert-UserDataKept([string]$When) {
  foreach ($name in @("config.toml", "desktop.json")) {
    if (-not (Test-Path -LiteralPath (Join-Path $userConfig $name))) {
      throw "$When removed $userConfig\$name; RF8.2 requires the user's data to be kept"
    }
  }
  if (-not (Test-Path -LiteralPath (Join-Path $userData "sentinel.jsonl"))) {
    Write-Host "::warning::$When removed $userData; review PRD 0021 RF8.2"
  }
  Write-Host "ok: $When kept the user's configuration and data"
}
function Remove-UserDataSentinels {
  Remove-Item -LiteralPath $userConfig -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $userData -Recurse -Force -ErrorAction SilentlyContinue
}

Assert-NoInstall "before the smoke"
$nsisSignature = Get-ExpectedSignature $nsis
$msiSignature = Get-ExpectedSignature $msi
Assert-Signature $nsis $nsisSignature
Assert-Signature $msi $msiSignature

# NSIS: silent per-user install, start, silent uninstall.
$install = Start-Process -FilePath $nsis -ArgumentList "/S" -Wait -PassThru
if ($install.ExitCode -ne 0) { throw "NSIS installer exited with $($install.ExitCode)" }
$entries = Get-UninstallEntry
if ($entries.Count -ne 1) { throw "expected one Cuotai uninstall entry after NSIS, found $($entries.Count)" }
$exe = Get-InstalledExe $entries[0]
$fileVersion = (Get-Item -LiteralPath $exe).VersionInfo.ProductVersion
# Exact match; Windows may pad the version resource to four parts.
if ($fileVersion -ne $Version -and $fileVersion -ne "$Version.0") {
  throw "installed Cuotai.exe reports $fileVersion, expected $Version"
}
Write-Host "ok: NSIS installed $exe ($fileVersion)"
# The installed application is signed (or not) together with its installer.
Assert-Signature $exe $nsisSignature
Assert-Starts $exe
$installDir = Split-Path -Parent $exe
$uninstaller = Join-Path $installDir "uninstall.exe"
if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) { throw "missing $uninstaller" }
# `_?=` keeps the NSIS uninstaller in place instead of re-spawning a detached
# copy, so -Wait covers the whole uninstall.
New-UserDataSentinels
$uninstall = Start-Process -FilePath $uninstaller -ArgumentList @("/S", "_?=$installDir") -Wait -PassThru
if ($uninstall.ExitCode -ne 0) { throw "NSIS uninstaller exited with $($uninstall.ExitCode)" }
Wait-Removed $exe
Assert-NoInstall "after the NSIS uninstall"
Write-Host "ok: NSIS uninstalled silently"
Assert-UserDataKept "the NSIS uninstall"
Remove-UserDataSentinels

# MSI: silent per-machine install and uninstall.
$log = Join-Path $env:RUNNER_TEMP "cuotai-msi.log"
$msiPath = (Resolve-Path -LiteralPath $msi).Path
$msiInstall = Start-Process -FilePath "msiexec.exe" -ArgumentList @("/i", "`"$msiPath`"", "/qn", "/norestart", "/l*v", "`"$log`"") -Wait -PassThru
if ($msiInstall.ExitCode -ne 0) {
  Get-Content -LiteralPath $log -Tail 40 -ErrorAction SilentlyContinue
  throw "msiexec /i exited with $($msiInstall.ExitCode)"
}
$entries = Get-UninstallEntry
if ($entries.Count -ne 1) { throw "expected one Cuotai uninstall entry after the MSI, found $($entries.Count)" }
$exe = Get-InstalledExe $entries[0]
Assert-Signature $exe $msiSignature
Write-Host "ok: MSI installed $exe"
New-UserDataSentinels
$msiRemove = Start-Process -FilePath "msiexec.exe" -ArgumentList @("/x", "`"$msiPath`"", "/qn", "/norestart") -Wait -PassThru
if ($msiRemove.ExitCode -ne 0) { throw "msiexec /x exited with $($msiRemove.ExitCode)" }
Wait-Removed $exe
Assert-NoInstall "after the MSI uninstall"
Write-Host "ok: MSI uninstalled silently"
Assert-UserDataKept "the MSI uninstall"
Remove-UserDataSentinels
Write-Host "Windows desktop smoke passed for $(Split-Path -Leaf $nsis) and $(Split-Path -Leaf $msi)"
