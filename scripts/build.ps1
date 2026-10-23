param([switch]$WithFFmpeg)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    if ($WithFFmpeg) {
        & (Join-Path $PSScriptRoot 'setup-ffmpeg.ps1')
        if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) { throw 'FFmpeg setup failed.' }
    }
    & cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    $outputDirectory = Join-Path $projectRoot 'dist\Frame Record'
    New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'target\release\frame-record.exe') -Destination $outputDirectory
    Copy-Item -LiteralPath (Join-Path $projectRoot 'README.md') -Destination $outputDirectory
    $imageDirectory = Join-Path $outputDirectory 'assets'
    New-Item -ItemType Directory -Force -Path $imageDirectory | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'assets\frame-record.png') -Destination $imageDirectory
    Copy-Item -LiteralPath (Join-Path $projectRoot 'LICENSE') -Destination $outputDirectory
    $portableFFmpeg = Join-Path $projectRoot 'tools\ffmpeg\bin\ffmpeg.exe'
    if (Test-Path -LiteralPath $portableFFmpeg) {
        Copy-Item -LiteralPath $portableFFmpeg -Destination $outputDirectory
        $ffmpegLicense = Join-Path $projectRoot 'tools\ffmpeg\LICENSE'
        if (Test-Path -LiteralPath $ffmpegLicense) {
            Copy-Item -LiteralPath $ffmpegLicense -Destination (Join-Path $outputDirectory 'FFmpeg-LICENSE.txt')
        }
        Copy-Item -LiteralPath (Join-Path $projectRoot 'THIRD-PARTY.md') -Destination $outputDirectory
    }
    Write-Host "Ready: $outputDirectory\frame-record.exe"
} finally {
    Pop-Location
}
