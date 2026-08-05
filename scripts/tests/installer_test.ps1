Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Installer = Join-Path $RepoRoot 'scripts\install.ps1'
$TestRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("agentic-installer-test-" + [Guid]::NewGuid().ToString('N'))
$FixtureDir = Join-Path $TestRoot 'release'
$Script:DownloadLog = [System.Collections.Generic.List[string]]::new()
$Passed = 0
$Failed = 0

function Pass([string]$Name) {
    $Script:Passed++
    Write-Host "ok - $Name"
}

function Fail-Test([string]$Name, [string]$Detail) {
    $Script:Failed++
    Write-Error "not ok - $Name`n  $Detail" -ErrorAction Continue
}

function Assert-True([string]$Name, [bool]$Condition, [string]$Detail) {
    if ($Condition) { Pass $Name } else { Fail-Test $Name $Detail }
}

function Invoke-WebRequest {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][string]$OutFile,
        [switch]$UseBasicParsing
    )
    $Script:DownloadLog.Add($Uri)
    $Asset = [System.IO.Path]::GetFileName(([Uri]$Uri).AbsolutePath)
    Copy-Item -LiteralPath (Join-Path $FixtureDir $Asset) -Destination $OutFile -Force
}

function Write-Checksums([string]$Hash) {
    "$Hash  agentic-windows-x86_64.zip" | Set-Content -LiteralPath (Join-Path $FixtureDir 'checksums-windows.txt') -Encoding ascii
}

function Invoke-InstallerCase {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [string]$Version,
        [switch]$Init
    )
    $CaseDir = Join-Path $TestRoot $Name
    $InstallDir = Join-Path $CaseDir 'bin'
    $env:USERPROFILE = Join-Path $CaseDir 'home'
    $env:LOCALAPPDATA = Join-Path $CaseDir 'local-app-data'
    $env:AGENTIC_RELEASE_BASE_URL = 'https://fixtures.invalid/releases/latest/download'
    $env:AGENTIC_VERSION = $null
    New-Item -ItemType Directory -Path $env:USERPROFILE -Force | Out-Null
    $Script:DownloadLog.Clear()

    if ($Version) {
        & $Installer -InstallDir $InstallDir -Version $Version -SkipPathUpdate -Init:$Init
    } else {
        & $Installer -InstallDir $InstallDir -SkipPathUpdate -Init:$Init
    }
    return @{ CaseDir = $CaseDir; InstallDir = $InstallDir }
}

try {
    New-Item -ItemType Directory -Path $FixtureDir -Force | Out-Null
    $BuildDir = Join-Path $TestRoot 'build'
    New-Item -ItemType Directory -Path $BuildDir -Force | Out-Null
    $FixtureSource = @'
using System;
using System.IO;

public static class FixtureAgentic {
    public static int Main(string[] args) {
        string log = Environment.GetEnvironmentVariable("AGENTIC_INIT_LOG");
        if (!String.IsNullOrEmpty(log)) {
            File.WriteAllText(log, String.Join(" ", args));
        }
        return 0;
    }
}
'@
    Add-Type -TypeDefinition $FixtureSource -OutputType ConsoleApplication -OutputAssembly (Join-Path $BuildDir 'agentic-windows-x86_64.exe')
    $Archive = Join-Path $FixtureDir 'agentic-windows-x86_64.zip'
    Compress-Archive -LiteralPath (Join-Path $BuildDir 'agentic-windows-x86_64.exe') -DestinationPath $Archive -Force
    $GoodHash = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash
    Write-Checksums $GoodHash

    $Result = Invoke-InstallerCase -Name 'verified-install'
    Assert-True 'Windows x64 installs verified executable' `
        (Test-Path -LiteralPath (Join-Path $Result.InstallDir 'agentic.exe') -PathType Leaf) `
        'agentic.exe was not installed'

    $DefaultInitLog = Join-Path $TestRoot 'default-init.log'
    $env:AGENTIC_INIT_LOG = $DefaultInitLog
    Invoke-InstallerCase -Name 'default-skips-init' | Out-Null
    Assert-True 'Windows default install does not invoke config init' `
        (-not (Test-Path -LiteralPath $DefaultInitLog)) `
        'fixture executable was invoked without -Init'

    $ExplicitInitLog = Join-Path $TestRoot 'explicit-init.log'
    $env:AGENTIC_INIT_LOG = $ExplicitInitLog
    Invoke-InstallerCase -Name 'explicit-init' -Init | Out-Null
    Assert-True 'Windows -Init invokes config wizard' `
        ((Get-Content -LiteralPath $ExplicitInitLog -Raw) -eq 'config init --interactive') `
        'initializer arguments did not match config init --interactive'
    $env:AGENTIC_INIT_LOG = $null

    $ConfigCase = Join-Path $TestRoot 'config-preserved'
    $ConfigPath = Join-Path $ConfigCase 'home\.config\agentic\config.json'
    New-Item -ItemType Directory -Path (Split-Path $ConfigPath) -Force | Out-Null
    '{"keep":true}' | Set-Content -LiteralPath $ConfigPath -Encoding ascii
    $Result = Invoke-InstallerCase -Name 'config-preserved'
    Assert-True 'Windows install preserves existing config' `
        ((Get-Content -LiteralPath $ConfigPath -Raw) -match '"keep":true') `
        'config marker changed'

    $Result = Invoke-InstallerCase -Name 'tagged-version' -Version '0.3.2'
    Assert-True 'Windows explicit version selects tagged URL' `
        (($Script:DownloadLog -join "`n") -match '/releases/download/v0\.3\.2/') `
        ($Script:DownloadLog -join ', ')

    $MismatchDir = Join-Path $TestRoot 'checksum-mismatch'
    $MismatchBin = Join-Path $MismatchDir 'bin'
    New-Item -ItemType Directory -Path $MismatchBin -Force | Out-Null
    'old executable' | Set-Content -LiteralPath (Join-Path $MismatchBin 'agentic.exe') -Encoding ascii
    Write-Checksums ('0' * 64)
    $MismatchFailed = $false
    try {
        Invoke-InstallerCase -Name 'checksum-mismatch' | Out-Null
    } catch {
        $MismatchFailed = $_.Exception.Message -match 'Checksum mismatch'
    }
    $OldPreserved = (Get-Content -LiteralPath (Join-Path $MismatchBin 'agentic.exe') -Raw) -match 'old executable'
    Assert-True 'Windows checksum mismatch preserves existing executable' `
        ($MismatchFailed -and $OldPreserved) `
        'checksum failure was not reported or old executable changed'
    Write-Checksums $GoodHash

    $env:AGENTIC_INSTALLER_TESTING = '1'
    $env:AGENTIC_TEST_ARCHITECTURE = 'Arm64'
    $ArmFailed = $false
    try {
        Invoke-InstallerCase -Name 'unsupported-arm64' | Out-Null
    } catch {
        $ArmFailed = $_.Exception.Message -match 'Windows x64 only'
    }
    Assert-True 'Windows ARM64 exits with guidance' $ArmFailed 'unsupported architecture was accepted'
} finally {
    $env:AGENTIC_RELEASE_BASE_URL = $null
    $env:AGENTIC_INSTALLER_TESTING = $null
    $env:AGENTIC_TEST_ARCHITECTURE = $null
    $env:AGENTIC_INIT_LOG = $null
    if (Test-Path -LiteralPath $TestRoot) {
        Remove-Item -LiteralPath $TestRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host "`n$Passed passed; $Failed failed"
if ($Failed -ne 0) { exit 1 }
