# Installs the latest Casylum release from GitHub on Windows.
#   irm https://raw.githubusercontent.com/DarthDurum/Casylum-App/main/scripts/install.ps1 | iex
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$repo = "DarthDurum/Casylum-App"
$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers @{ "User-Agent" = "casylum-installer" }
$asset = $release.assets | Where-Object { $_.name -like "*_x64-setup.exe" } | Select-Object -First 1
if (-not $asset) {
  $asset = $release.assets | Where-Object { $_.name -like "*_x64_en-US.msi" } | Select-Object -First 1
}
if (-not $asset) { throw "No Windows installer found in release $($release.tag_name)." }

$file = Join-Path $env:TEMP $asset.name
Write-Host "Downloading $($asset.name) ($($release.tag_name))"
Invoke-WebRequest $asset.browser_download_url -OutFile $file

Write-Host "Running the installer"
if ($file.EndsWith(".msi")) {
  Start-Process msiexec.exe -ArgumentList "/i", "`"$file`"", "/passive" -Wait
} else {
  Start-Process $file -ArgumentList "/P" -Wait
}
Remove-Item $file -ErrorAction SilentlyContinue
Write-Host "Done. Find Casylum in the Start menu."
