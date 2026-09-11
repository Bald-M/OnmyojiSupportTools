[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("x64")]
    [string]$Architecture
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($env:OS -ne "Windows_NT") {
    throw "Windows bundles must be built on Windows. Use GitHub Actions when developing on another platform."
}

$Targets = @{
    x64 = "x86_64-pc-windows-msvc"
}
$ExpectedMachine = @{
    x64 = 0x8664
}

$Target = $Targets[$Architecture]
$RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Package = Get-Content (Join-Path $RepositoryRoot "package.json") -Raw | ConvertFrom-Json
$Version = $Package.version
$TargetRoot = Join-Path $RepositoryRoot "src-tauri/target/$Target/release"
$BinaryPath = Join-Path $TargetRoot "onmyoji-support-tools.exe"
$BundleDirectory = Join-Path $TargetRoot "bundle/nsis"
$DistributionDirectory = Join-Path $RepositoryRoot "dist"
$InstallerName = "OnmyojiSupportTools_${Version}_windows_x64_nsis-setup.exe"
$InstallerPath = Join-Path $DistributionDirectory $InstallerName

function Invoke-CheckedCommand {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Program,
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $Program $($Arguments -join ' ')"
    }
}

function Assert-PeArchitecture {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [int]$Machine
    )

    $Bytes = [System.IO.File]::ReadAllBytes($Path)
    if ($Bytes.Length -lt 64) {
        throw "Built executable is too small to be a valid PE file: $Path"
    }
    if ($Bytes[0] -ne 0x4D -or $Bytes[1] -ne 0x5A) {
        throw "Built executable has an invalid DOS header: $Path"
    }

    $PeOffset = [System.BitConverter]::ToInt32($Bytes, 0x3C)
    if ($PeOffset -lt 0 -or ($PeOffset + 6) -gt $Bytes.Length) {
        throw "Built executable has an invalid PE header offset: $Path"
    }

    $Signature = [System.Text.Encoding]::ASCII.GetString($Bytes, $PeOffset, 4)
    if ($Signature -ne "PE`0`0") {
        throw "Built executable has an invalid PE signature: $Path"
    }

    $ActualMachine = [System.BitConverter]::ToUInt16($Bytes, $PeOffset + 4)
    if ($ActualMachine -ne $Machine) {
        throw ("Expected PE machine 0x{0:X4}, found 0x{1:X4}: {2}" -f $Machine, $ActualMachine, $Path)
    }
}

Push-Location $RepositoryRoot
try {
    Write-Host "Building platform=windows architecture=$Architecture target=$Target version=$Version"
    Invoke-CheckedCommand -Program "rustup" -Arguments @("target", "add", $Target)
    Invoke-CheckedCommand -Program "pnpm.cmd" -Arguments @(
        "tauri", "build", "--target", $Target, "--ci", "--no-sign"
    )

    if (-not (Test-Path $BinaryPath -PathType Leaf)) {
        throw "Tauri did not produce the expected executable: $BinaryPath"
    }
    Assert-PeArchitecture -Path $BinaryPath -Machine $ExpectedMachine[$Architecture]

    $Installers = @(Get-ChildItem -Path $BundleDirectory -Filter "*$Version*.exe" -File)
    if ($Installers.Count -ne 1) {
        throw "Expected exactly one NSIS installer in $BundleDirectory, found $($Installers.Count)."
    }
    $GeneratedInstallerPath = $Installers[0].FullName

    Remove-Item -LiteralPath $DistributionDirectory -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $DistributionDirectory | Out-Null
    Copy-Item -LiteralPath $GeneratedInstallerPath -Destination $InstallerPath -Force

    if ($env:GITHUB_OUTPUT) {
        "target=$Target" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append
        "installer_path=dist/$InstallerName" |
            Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append
    }

    Write-Host "Verified platform=windows architecture=$Architecture target=$Target"
    Write-Host "NSIS: $InstallerPath"
}
finally {
    Pop-Location
}
