$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$destination = Join-Path $projectRoot 'tools\ffmpeg'
$ffmpeg = Join-Path $destination 'bin\ffmpeg.exe'
if (Test-Path -LiteralPath $ffmpeg) {
    Write-Host "FFmpeg is ready: $ffmpeg"
    exit 0
}

# Pinned portable build from a Windows build provider linked by ffmpeg.org.
$version = '9.0.2'
$expectedHash = '60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba'
$archiveName = "ffmpeg-$version-essentials_build.zip"
$url = "https://www.gyan.dev/ffmpeg/builds/packages/$archiveName"
$downloadDirectory = Join-Path $projectRoot 'tools\downloads'
New-Item -ItemType Directory -Force -Path $downloadDirectory | Out-Null
$archivePath = Join-Path $downloadDirectory $archiveName
if (!(Test-Path -LiteralPath $archivePath)) {
    Write-Host "Downloading portable FFmpeg $version..."
    & curl.exe --fail --location --silent --show-error --output $archivePath $url
    if ($LASTEXITCODE -ne 0) { throw 'FFmpeg download failed.' }
}
$actualHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash
if ($actualHash -ne $expectedHash) {
    throw "FFmpeg checksum did not match. Delete $archivePath and retry."
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($archivePath)
try {
    foreach ($entry in $archive.Entries) {
        # Keep the encoder and probe for validation; skip ffplay and the large docs tree.
        $relative = $entry.FullName -replace '^[^/]+/', ''
        if ($relative -in @('bin/ffmpeg.exe', 'bin/ffprobe.exe', 'LICENSE', 'README.txt')) {
            $targetPath = Join-Path $destination $relative
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $targetPath) | Out-Null
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $targetPath, $true)
        }
    }
} finally {
    $archive.Dispose()
}
if (!(Test-Path -LiteralPath $ffmpeg)) { throw 'The archive did not contain ffmpeg.exe.' }
Write-Host "FFmpeg is ready: $ffmpeg"
