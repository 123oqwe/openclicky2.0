# Install the OpenClicky CLI from the latest release.
#
#   irm https://raw.githubusercontent.com/123oqwe/openclicky2.0/main/scripts/install.ps1 | iex
#
# The release asset is named openclicky-windows.exe because three files called
# "openclicky" would collide in a flat download. This installs it as
# openclicky.exe, which is what the README and CHANGELOG assume.
#
# Override the destination with PREFIX and pin a version with VERSION.
$ErrorActionPreference = 'Stop'

$Repo = '123oqwe/openclicky2.0'
$Prefix = if ($env:PREFIX) { $env:PREFIX } else { Join-Path $HOME '.local\bin' }
$Version = $env:VERSION

$asset = 'openclicky-windows.exe'
$url = if ($Version) {
    "https://github.com/$Repo/releases/download/$Version/$asset"
} else {
    "https://github.com/$Repo/releases/latest/download/$asset"
}

New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
$target = Join-Path $Prefix 'openclicky.exe'

Write-Host "downloading $asset"
Invoke-WebRequest -Uri $url -OutFile $target -UseBasicParsing

# The marker file GitHub attaches is not the binary itself, so make sure
# something runnable arrived before telling anyone it worked.
if (-not (Test-Path $target) -or (Get-Item $target).Length -lt 100000) {
    throw "download looks wrong: $target is missing or too small to be the CLI"
}

try {
    $version = (& $target --version) 2>&1
    Write-Host "installed $version"
} catch {
    throw "the downloaded file did not run: $_"
}

Write-Host "  at $target"

# The desktop shell, from the NSIS installer the release already publishes.
$setup = 'OpenClicky_2.0.0_x64-setup.exe'
$setupUrl = if ($Version) {
    "https://github.com/$Repo/releases/download/$Version/$setup"
} else {
    "https://github.com/$Repo/releases/latest/download/$setup"
}

$tmpSetup = Join-Path $env:TEMP $setup
try {
    Write-Host "downloading $setup"
    Invoke-WebRequest -Uri $setupUrl -OutFile $tmpSetup -UseBasicParsing
    # A file downloaded from the internet carries a mark-of-the-web that can stop
    # the installer from running without a manual "unblock".
    Unblock-File -Path $tmpSetup
    Write-Host "installing the desktop shell (silent)"
    Start-Process -FilePath $tmpSetup -ArgumentList '/S' -Wait
} catch {
    Write-Host "  could not install the desktop shell; the CLI still works: $_"
} finally {
    Remove-Item -Force -ErrorAction SilentlyContinue $tmpSetup
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$Prefix*") {
    Write-Host ""
    Write-Host "  $Prefix is not on your PATH. Add it once:"
    Write-Host "    [Environment]::SetEnvironmentVariable('Path', `"$userPath;$Prefix`", 'User')"
}

Write-Host ""
Write-Host "next:"
Write-Host "  openclicky set-key      # paste your provider key"
Write-Host "  openclicky doctor       # check permissions"
