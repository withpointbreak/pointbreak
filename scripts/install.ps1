# Pointbreak Installation Script for Windows
#
# Usage:
#   irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex
#
# Or with parameters:
#   $version = "v0.2.0"; irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex

param(
    [string]$Version = "latest",
    [string]$InstallDir = "$env:LOCALAPPDATA\Pointbreak\bin",
    [switch]$NoVerify
)

$ErrorActionPreference = "Stop"

# Repository configuration
$Repo = "withpointbreak/pointbreak"

# Colors
function Write-ColorOutput {
    param(
        [string]$Message,
        [string]$ForegroundColor = "White"
    )
    Write-Host $Message -ForegroundColor $ForegroundColor
}

function Write-Success {
    param([string]$Message)
    Write-ColorOutput "✓ $Message" -ForegroundColor Green
}

function Write-Error {
    param([string]$Message)
    Write-ColorOutput "✗ $Message" -ForegroundColor Red
}

function Write-Warning {
    param([string]$Message)
    Write-ColorOutput "⚠ $Message" -ForegroundColor Yellow
}

function Write-Info {
    param([string]$Message)
    Write-ColorOutput "  $Message" -ForegroundColor Cyan
}

# Print header
Write-Host ""
Write-ColorOutput "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Blue
Write-ColorOutput "  Pointbreak Installer" -ForegroundColor Blue
Write-ColorOutput "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Blue
Write-Host ""

# Detect platform
function Get-Platform {
    $arch = $env:PROCESSOR_ARCHITECTURE

    switch ($arch) {
        "AMD64" {
            $platform = "win32-x64"
        }
        "ARM64" {
            $platform = "win32-arm64"
        }
        default {
            Write-Error "Unsupported architecture: $arch"
            Write-Info "Supported architectures: AMD64 (x64), ARM64"
            exit 1
        }
    }

    Write-Success "Detected platform: $platform"
    return $platform
}

# Get download URL
function Get-DownloadUrl {
    param([string]$Platform)

    if ($Version -eq "latest") {
        Write-Info "Fetching latest release..."
        $apiUrl = "https://api.github.com/repos/$Repo/releases/latest"
    }
    else {
        Write-Info "Using version: $Version"
        $apiUrl = "https://api.github.com/repos/$Repo/releases/tags/$Version"
    }

    try {
        $release = Invoke-RestMethod -Uri $apiUrl -Method Get
    }
    catch {
        Write-Error "Could not fetch release information"
        if ($Version -ne "latest") {
            Write-Info "Version $Version not found. Check: https://github.com/$Repo/releases"
        }
        exit 1
    }

    $releaseTag = $release.tag_name
    Write-Success "Found version: $releaseTag"

    $binaryName = "pointbreak-$Platform.exe"
    $binaryUrl = "https://github.com/$Repo/releases/download/$releaseTag/$binaryName"
    $checksumsUrl = "https://github.com/$Repo/releases/download/$releaseTag/checksums.txt"

    return @{
        ReleaseTag   = $releaseTag
        BinaryName   = $binaryName
        BinaryUrl    = $binaryUrl
        ChecksumsUrl = $checksumsUrl
    }
}

# Download and verify binary
function Install-Binary {
    param(
        [hashtable]$DownloadInfo,
        [string]$Platform
    )

    $tempDir = New-Item -ItemType Directory -Path "$env:TEMP\pointbreak-install-$(New-Guid)"
    $binaryPath = Join-Path $tempDir $DownloadInfo.BinaryName

    try {
        Write-Host ""
        Write-Info "Downloading binary..."

        # Download binary
        Invoke-WebRequest -Uri $DownloadInfo.BinaryUrl -OutFile $binaryPath

        if (-not (Test-Path $binaryPath)) {
            Write-Error "Download failed"
            exit 1
        }

        $fileSize = (Get-Item $binaryPath).Length
        $fileSizeMB = [math]::Round($fileSize / 1MB, 2)
        Write-Success "Downloaded: $fileSizeMB MB"

        # Verify checksum
        if (-not $NoVerify) {
            Write-Host ""
            Write-Info "Verifying checksum..."

            try {
                $checksumsPath = Join-Path $tempDir "checksums.txt"
                Invoke-WebRequest -Uri $DownloadInfo.ChecksumsUrl -OutFile $checksumsPath

                $checksums = Get-Content $checksumsPath
                $expectedLine = $checksums | Where-Object { $_ -match $DownloadInfo.BinaryName }

                if ($expectedLine) {
                    $expectedChecksum = $expectedLine.Split()[0]

                    # Validate checksum format (64 hex characters for SHA256)
                    if ($expectedChecksum -notmatch '^[a-f0-9]{64}$') {
                        Write-Warning "Invalid checksum format (expected 64 hex characters)"
                        Write-Info "Skipping checksum verification"
                    }
                    else {
                        $actualHash = Get-FileHash -Path $binaryPath -Algorithm SHA256
                        $actualChecksum = $actualHash.Hash.ToLower()

                        if ($actualChecksum -eq $expectedChecksum) {
                            Write-Success "Checksum verified"
                        }
                        else {
                            Write-Error "Checksum mismatch!"
                            Write-Info "Expected: $expectedChecksum"
                            Write-Info "Got:      $actualChecksum"
                            exit 1
                        }
                    }
                }
                else {
                    Write-Warning "Checksum not found in checksums.txt"
                    Write-Info "Skipping checksum verification"
                }
            }
            catch {
                Write-Warning "Could not verify checksum: $_"
                Write-Info "Continuing anyway..."
            }
        }

        # Install binary
        Write-Host ""
        Write-Info "Installing to: $InstallDir"

        if (-not (Test-Path $InstallDir)) {
            New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        }

        $targetPath = Join-Path $InstallDir "pointbreak.exe"
        Copy-Item -Path $binaryPath -Destination $targetPath -Force

        Write-Success "Installed successfully"

        # Verify installation
        Write-Host ""
        Write-Info "Verifying installation..."
        try {
            $versionOutput = & $targetPath --version 2>&1
            if ($LASTEXITCODE -eq 0) {
                $installedVersion = ($versionOutput | Select-Object -First 1).ToString()
                Write-Success "Verification successful: $installedVersion"
            }
            else {
                Write-Warning "Could not verify installation"
                Write-Info "Binary installed but --version check failed"
            }
        }
        catch {
            Write-Warning "Could not verify installation: $_"
            Write-Info "Binary installed but verification failed"
        }

        return $targetPath
    }
    finally {
        # Cleanup temp directory
        # Add small delay to avoid Windows file locking issues
        Start-Sleep -Seconds 1
        if (Test-Path $tempDir) {
            Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

# Check if directory is in PATH
function Test-PathEntry {
    param([string]$Directory)

    Write-Host ""

    $pathEntries = $env:PATH -split ";"
    $inPath = $pathEntries -contains $Directory

    if ($inPath) {
        Write-Success "Install directory is in PATH"
        return $true
    }
    else {
        Write-Warning "Install directory is not in PATH"
        return $false
    }
}

# Add directory to PATH
function Add-ToPath {
    param([string]$Directory)

    Write-Host ""
    Write-Info "Adding to PATH..."

    try {
        # Get current user PATH
        $currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")

        # Check if already in PATH
        $pathEntries = $currentPath -split ";"
        if ($pathEntries -contains $Directory) {
            Write-Success "Directory already in PATH"
            return
        }

        # Add to PATH
        $newPath = "$currentPath;$Directory"
        [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")

        # Update current session
        $env:PATH = "$env:PATH;$Directory"

        Write-Success "Added to PATH"
        Write-Warning "Please restart your terminal for PATH changes to take effect"
    }
    catch {
        Write-Error "Failed to add to PATH: $_"
        Write-Host ""
        Write-Info "You can manually add to PATH by running:"
        Write-Host "  `$env:PATH += `";$Directory`"" -ForegroundColor Cyan
    }
}

# Print next steps
function Show-NextSteps {
    Write-Host ""
    Write-ColorOutput "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Green
    Write-ColorOutput "  Installation Complete!" -ForegroundColor Green
    Write-ColorOutput "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Green
    Write-Host ""
    Write-Info "Verify installation:"
    Write-Host "  pointbreak --version" -ForegroundColor Cyan
    Write-Host ""
    Write-Info "Next steps:"
    Write-Host "  1. Install the VS Code extension (if not already installed)" -ForegroundColor White
    Write-Host "  2. Configure your AI assistant to use Pointbreak MCP server" -ForegroundColor White
    Write-Host "  3. See setup guides: https://github.com/$Repo/tree/main/docs" -ForegroundColor Cyan
    Write-Host ""
}

# Main installation flow
function Main {
    $platform = Get-Platform
    $downloadInfo = Get-DownloadUrl -Platform $platform
    $binaryPath = Install-Binary -DownloadInfo $downloadInfo -Platform $platform

    $inPath = Test-PathEntry -Directory $InstallDir

    if (-not $inPath) {
        $response = Read-Host "Add install directory to PATH? (Y/n)"
        if ($response -eq "" -or $response -eq "Y" -or $response -eq "y") {
            Add-ToPath -Directory $InstallDir
        }
    }

    Show-NextSteps
}

# Run main function
Main
