[CmdletBinding()]
param(
    [switch]$Init,
    [string]$Version = $env:AGENTIC_VERSION,
    [string]$InstallDir = $env:AGENTIC_INSTALL_DIR,
    [switch]$SkipPathUpdate
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Repository = 'dchya24/agentic'
$DefaultReleaseBaseUrl = "https://github.com/$Repository/releases/latest/download"
$Asset = 'agentic-windows-x86_64.zip'
$ChecksumAsset = 'checksums-windows.txt'

function Fail([string]$Message) {
    throw "Agentic installer: $Message"
}

if (-not [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
        [System.Runtime.InteropServices.OSPlatform]::Windows)) {
    Fail 'Windows is required. Use scripts/install.sh on Linux or macOS.'
}

$Architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
if ($env:AGENTIC_INSTALLER_TESTING -eq '1' -and $env:AGENTIC_TEST_ARCHITECTURE) {
    $Architecture = $env:AGENTIC_TEST_ARCHITECTURE
}
if ($Architecture -ne 'X64') {
    Fail "Unsupported Windows architecture '$Architecture'. Prebuilt releases currently support Windows x64 only."
}

if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        Fail 'LOCALAPPDATA is not set; pass -InstallDir explicitly.'
    }
    $InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\agentic\bin'
}
$InstallDir = [System.IO.Path]::GetFullPath($InstallDir)

$ReleaseBaseUrl = if ($env:AGENTIC_RELEASE_BASE_URL) {
    $env:AGENTIC_RELEASE_BASE_URL.TrimEnd('/')
} else {
    $DefaultReleaseBaseUrl
}

if (-not [string]::IsNullOrWhiteSpace($Version)) {
    if (-not $Version.StartsWith('v')) {
        $Version = "v$Version"
    }
    if ($Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$') {
        Fail "Invalid version '$Version'; expected vX.Y.Z or X.Y.Z."
    }
    if ($ReleaseBaseUrl -match '/releases/latest/download$') {
        $ReleaseBaseUrl = $ReleaseBaseUrl -replace '/releases/latest/download$', "/releases/download/$Version"
    } else {
        $ReleaseBaseUrl = "$($ReleaseBaseUrl.TrimEnd('/'))/$Version"
    }
}

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("agentic-install-" + [Guid]::NewGuid().ToString('N'))
$ArchivePath = Join-Path $TempDir $Asset
$ChecksumPath = Join-Path $TempDir $ChecksumAsset
$StageDir = Join-Path $TempDir 'stage'

try {
    New-Item -ItemType Directory -Path $StageDir -Force | Out-Null

    Write-Host "Downloading $Asset..."
    Invoke-WebRequest -UseBasicParsing -Uri "$ReleaseBaseUrl/$Asset" -OutFile $ArchivePath
    Invoke-WebRequest -UseBasicParsing -Uri "$ReleaseBaseUrl/$ChecksumAsset" -OutFile $ChecksumPath

    $EscapedAsset = [Regex]::Escape($Asset)
    $ChecksumLine = Get-Content -LiteralPath $ChecksumPath |
        Where-Object { $_ -match "^([A-Fa-f0-9]{64})\s+\*?$EscapedAsset$" } |
        Select-Object -First 1
    if (-not $ChecksumLine) {
        Fail "Checksum entry for $Asset was not found in $ChecksumAsset."
    }
    $ExpectedHash = ([Regex]::Match($ChecksumLine, '^([A-Fa-f0-9]{64})')).Groups[1].Value
    $ActualHash = (Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash
    if (-not $ActualHash.Equals($ExpectedHash, [StringComparison]::OrdinalIgnoreCase)) {
        Fail "Checksum mismatch for $Asset."
    }
    Write-Host 'Checksum verified.'

    Expand-Archive -LiteralPath $ArchivePath -DestinationPath $StageDir -Force
    $StagedBinary = Join-Path $StageDir 'agentic-windows-x86_64.exe'
    if (-not (Test-Path -LiteralPath $StagedBinary -PathType Leaf)) {
        Fail 'Downloaded archive does not contain agentic-windows-x86_64.exe.'
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $Destination = Join-Path $InstallDir 'agentic.exe'
    $DestinationTemp = Join-Path $InstallDir ('.agentic.install.' + [Guid]::NewGuid().ToString('N') + '.exe')
    Copy-Item -LiteralPath $StagedBinary -Destination $DestinationTemp -Force
    Move-Item -LiteralPath $DestinationTemp -Destination $Destination -Force
    Write-Host "Agentic installed successfully: $Destination"

    $ConfigPath = Join-Path $env:USERPROFILE '.config\agentic\config.json'
    if (Test-Path -LiteralPath $ConfigPath -PathType Leaf) {
        Write-Host "Existing config preserved: $ConfigPath"
    } else {
        Write-Host 'No config was created. Initialize it when ready.'
    }

    if (-not $SkipPathUpdate) {
        $UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        $PathEntries = @($UserPath -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
        $AlreadyPresent = $PathEntries | Where-Object {
            $_.TrimEnd('\') -ieq $InstallDir.TrimEnd('\')
        }
        if (-not $AlreadyPresent) {
            $NewUserPath = (@($PathEntries) + $InstallDir) -join ';'
            [Environment]::SetEnvironmentVariable('Path', $NewUserPath, 'User')
            Write-Host 'Added Agentic to the current user PATH. Open a new terminal to use it.'
        }
    }

    if ($Init) {
        & $Destination config init --interactive
        if ($LASTEXITCODE -ne 0) {
            Fail "Configuration wizard exited with status $LASTEXITCODE."
        }
    } else {
        Write-Host "Next: `"$Destination`" config init --interactive"
    }
} finally {
    if (Test-Path -LiteralPath $TempDir) {
        Remove-Item -LiteralPath $TempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
