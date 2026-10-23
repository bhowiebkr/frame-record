param([switch]$WithFFmpeg, [switch]$Package)
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
    # Start clean so files from an earlier build never end up in a release.
    if (Test-Path -LiteralPath $outputDirectory) { Remove-Item -LiteralPath $outputDirectory -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
    foreach ($exe in 'frame-record.exe', 'frame-record-cli.exe') {
        Copy-Item -LiteralPath (Join-Path $projectRoot "target\release\$exe") -Destination $outputDirectory
    }
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
    if ($Package) {
        $version = (& cargo pkgid) -replace '.*[#@]', ''
        if ($LASTEXITCODE -ne 0) { throw 'Could not read the package version.' }
        $archive = Join-Path $projectRoot "dist\frame-record-v$version-windows-x64.zip"
        if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive -Force }
        # Windows' bsdtar writes ZIPs; a Git or MSYS tar earlier on PATH cannot.
        & (Join-Path $env:SystemRoot 'System32\tar.exe') -a -c -f $archive -C (Join-Path $projectRoot 'dist') 'Frame Record'
        if ($LASTEXITCODE -ne 0) { throw 'Packaging failed.' }
        Write-Host "Packaged: $archive"
    }
} finally {
    Pop-Location
}
