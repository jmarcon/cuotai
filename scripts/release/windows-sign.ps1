# Signs and verifies the application before bundling, then signs and verifies
# the outer installers after collection (PRD 0021 RF3.1/RF3.4).
param(
  [ValidateSet("Application", "Installers")]
  [string]$Mode = "Installers",
  [ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")]
  [string]$Target = "x86_64-pc-windows-msvc",
  [string]$DistDir = "dist",
  [string]$FragmentDir = "fragments",
  [string]$FragmentName = "windows.json"
)

$ErrorActionPreference = "Stop"
$targetPrefix = if ($Target -eq "x86_64-pc-windows-msvc") { "" } else { "$Target/" }
$app = "apps/desktop/src-tauri/target/${targetPrefix}release/Cuotai.exe"
$hasCertificate = -not [string]::IsNullOrEmpty($env:WINDOWS_CERTIFICATE)
$hasPassword = -not [string]::IsNullOrEmpty($env:WINDOWS_CERTIFICATE_PASSWORD)
$canSign = $env:DRY_RUN -ne "true" -and $hasCertificate -and $hasPassword
$unsignedReason = if ($env:DRY_RUN -eq "true") {
  "dry-run: signing skipped"
} elseif ($hasCertificate -xor $hasPassword) {
  "incomplete Windows signing credentials"
} else {
  "no Windows certificate configured"
}

function Get-SignTool {
  $tool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
  if (-not $tool) { throw "signtool.exe not found" }
  return $tool
}

function Invoke-SignAndVerify([string]$SignTool, [string]$Pfx, [string]$Path) {
  & $SignTool sign /fd sha256 /td sha256 /tr http://timestamp.digicert.com `
    /f $Pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD $Path
  if ($LASTEXITCODE -ne 0) { throw "signtool sign failed for $Path" }
  & $SignTool verify /pa $Path
  if ($LASTEXITCODE -ne 0) { throw "signtool verify failed for $Path" }
}


if ($Mode -eq "Application") {
  if (-not (Test-Path -LiteralPath $app -PathType Leaf)) {
    throw "application executable not found: $app"
  }
  if (-not $canSign) {
    $reason = $unsignedReason
    Write-Output "application executable left unsigned ($reason)"
    exit 0
  }

  $pfx = Join-Path $env:RUNNER_TEMP "cuotai-application-cert.pfx"
  try {
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    Invoke-SignAndVerify (Get-SignTool) $pfx $app
    Write-Output "signed and verified application executable before bundling"
  } finally {
    Remove-Item $pfx -Force -ErrorAction SilentlyContinue
  }
  exit 0
}

$files = @(Get-ChildItem -Path $DistDir -File | Where-Object { $_.Name -match '\.(exe|msi)$' })
if ($files.Count -eq 0) { throw "no installers found in $DistDir" }
New-Item -ItemType Directory -Force -Path $FragmentDir | Out-Null
$entries = @()

if ($canSign) {
  $signtool = Get-SignTool
  if (-not (Test-Path -LiteralPath $app -PathType Leaf)) {
    throw "application executable not found before installer signing: $app"
  }
  & $signtool verify /pa $app
  if ($LASTEXITCODE -ne 0) {
    throw "application executable is not Authenticode-signed; refusing to sign outer installers"
  }

  $pfx = Join-Path $env:RUNNER_TEMP "cuotai-installer-cert.pfx"
  try {
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    foreach ($file in $files) {
      Invoke-SignAndVerify $signtool $pfx $file.FullName
      $entries += [pscustomobject]@{
        name = $file.Name; signed = $true; method = "authenticode"; status = "signed"
      }
      Write-Output "signed and verified $($file.Name)"
    }
  } finally {
    Remove-Item $pfx -Force -ErrorAction SilentlyContinue
  }
} else {
  $reason = $unsignedReason
  foreach ($file in $files) {
    $entries += [pscustomobject]@{
      name = $file.Name; signed = $false; method = "none"; status = "unsigned";
      note = "$reason; the installed application and installer have no Authenticode trust"
    }
  }
  Write-Output "application and installers left unsigned ($reason)"
}

[pscustomobject]@{ artifacts = $entries } |
  ConvertTo-Json -Depth 5 |
  Set-Content -Encoding utf8 (Join-Path $FragmentDir $FragmentName)
Write-Output "wrote $FragmentDir\$FragmentName"
