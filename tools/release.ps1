<#
.SYNOPSIS
  Build an Ultimate Download Manager release and gather what to upload.

.DESCRIPTION
  Writes to release\v<version>\:
    UltimateDownloadManager-Setup-<version>.exe         the installer
    UltimateDownloadManager-Setup-<version>.exe.sha256  its SHA-256
    UltimateDownloadManager-Setup-<version>.exe.sig     its update signature
    latest.json                                          what installed copies read to update
    UltimateDownloadManager-<version>-source.zip         the source (GPL-3.0)
    RELEASE-NOTES.md                                     a draft for the release's text

  Before a release, raise "version" in package.json, Cargo.toml, src-tauri\tauri.conf.json and
  both extension manifests. Then commit as "Ultimate Download Manager <version>", push, and
  publish a GitHub release tagged v<version> with every file above but RELEASE-NOTES.md.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File tools\release.ps1
#>
param(
    [string]$Repo = "D3MON999/Ultimate-Download-Manager",
    [string]$Notes = ""
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$keyFile = Join-Path $root "keys\updater.key"
if (-not (Test-Path $keyFile)) { throw "keys\updater.key is missing: without it updates can't be signed." }
$env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $keyFile -Raw).Trim()
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
# The build goes to the workspace's own target folder.
Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue

$conf = Get-Content (Join-Path $root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
$version = $conf.version
Write-Host "Building Ultimate Download Manager $version..."
npm.cmd run tauri build
if ($LASTEXITCODE -ne 0) { throw "The build failed." }

$nsis = Join-Path $root "target\release\bundle\nsis"
$setup = Get-ChildItem $nsis -Filter "*_${version}_x64-setup.exe" | Select-Object -First 1
$sig = Get-ChildItem $nsis -Filter "*_${version}_x64-setup.exe.sig" | Select-Object -First 1
if (-not $setup -or -not $sig) { throw "The signed installer wasn't made (is createUpdaterArtifacts on?)." }

$out = Join-Path $root "release\v$version"
Remove-Item $out -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $out | Out-Null
$setupName = "UltimateDownloadManager-Setup-$version.exe"
$setupPath = Join-Path $out $setupName
Copy-Item $setup.FullName $setupPath
Copy-Item $sig.FullName "$setupPath.sig"
$hash = (Get-FileHash $setupPath -Algorithm SHA256).Hash.ToLower()
[IO.File]::WriteAllText("$setupPath.sha256", "$hash  $setupName`n", (New-Object Text.UTF8Encoding $false))

# latest.json: what every installed copy reads to learn of this release.
$latest = [ordered]@{
    version   = $version
    notes     = $Notes
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = (Get-Content $sig.FullName -Raw).Trim()
            url       = "https://github.com/$Repo/releases/download/v$version/$setupName"
        }
    }
}
[IO.File]::WriteAllText((Join-Path $out "latest.json"), ($latest | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding $false))

# The source: exactly the files the repository holds (never builds or keys).
$src = Join-Path $env:TEMP "UltimateDownloadManager-$version-source"
Remove-Item $src -Recurse -Force -ErrorAction SilentlyContinue
# git on PATH, else the one inside GitHub Desktop.
$git = (Get-Command git -ErrorAction SilentlyContinue).Source
if (-not $git) {
    $git = Get-ChildItem "$env:LOCALAPPDATA\GitHubDesktop\app-*\resources\app\git\cmd\git.exe" -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
}
if (-not $git) { throw "git not found: install Git or GitHub Desktop." }
$files = & $git -C $root ls-files --cached --others --exclude-standard
if ($LASTEXITCODE -ne 0 -or -not $files) { throw "git ls-files failed: run this inside the repository." }
foreach ($f in $files) {
    $to = Join-Path $src "UltimateDownloadManager-$version\$f"
    New-Item -ItemType Directory -Force (Split-Path $to) | Out-Null
    Copy-Item (Join-Path $root $f) $to
}
Compress-Archive -Path (Join-Path $src "UltimateDownloadManager-$version") -DestinationPath (Join-Path $out "UltimateDownloadManager-$version-source.zip") -Force
Remove-Item $src -Recurse -Force

# A draft of the release's text.
$draft = @"
## Ultimate Download Manager $version

### New
-

### Improved
-

### Install
New users: download ``$setupName`` and run it. If Windows warns you, choose **More info**, then **Run anyway**.
"@
[IO.File]::WriteAllText((Join-Path $out "RELEASE-NOTES.md"), $draft, (New-Object Text.UTF8Encoding $false))

Write-Host ""
Write-Host "Release ready in $out :"
Get-ChildItem $out | ForEach-Object { Write-Host ("  {0}  ({1:N1} MB)" -f $_.Name, ($_.Length / 1MB)) }
