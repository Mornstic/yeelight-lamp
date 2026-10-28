param(
  [string]$Version = "",
  [string]$Arch = "x64"
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$root = Split-Path -Parent $PSScriptRoot
if (-not $Version) {
  $Version = (Get-Content (Join-Path $root "package.json") -Raw | ConvertFrom-Json).version
}
$bundleDir = Join-Path $root "src-tauri\target\release\bundle\nsis"
$releaseDir = Join-Path $root "src-tauri\target\release\release-assets"
$exe = Join-Path $root "src-tauri\target\release\yeelight-lamp.exe"

$setup = Get-ChildItem $bundleDir -Filter "*_$($Version)_$($Arch)-setup.exe" |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
if (-not $setup) {
  throw "没有找到安装包，请先运行 npm run tauri build"
}
if (-not (Test-Path $exe)) {
  throw "没有找到 yeelight-lamp.exe，请先运行 npm run tauri build"
}

if (Test-Path $releaseDir) {
  Remove-Item $releaseDir -Recurse -Force
}
New-Item -ItemType Directory -Path $releaseDir | Out-Null

$setupName = "yeelight-lamp_${Version}_${Arch}-setup.exe"
$zipName = "yeelight-lamp_${Version}_${Arch}-portable.zip"
Copy-Item $setup.FullName (Join-Path $releaseDir $setupName)
Compress-Archive -Path $exe -DestinationPath (Join-Path $releaseDir $zipName) -CompressionLevel Optimal

$lines = Get-ChildItem $releaseDir -File |
  Sort-Object Name |
  ForEach-Object {
    $hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
    "$hash  $($_.Name)"
  }
$text = ($lines -join "`n") + "`n"
[System.IO.File]::WriteAllText(
  (Join-Path $releaseDir "SHA256SUMS.txt"),
  $text,
  [System.Text.UTF8Encoding]::new($false)
)

Write-Output "发布资产：$releaseDir"
Get-ChildItem $releaseDir -File |
  Sort-Object Name |
  Format-Table Name, @{ n = "Size"; e = { "{0:N2} MB" -f ($_.Length / 1MB) } } -AutoSize
