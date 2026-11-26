# Install the OpenClicky CLI from the latest release.
#
#   irm https://raw.githubusercontent.com/123oqwe/openclicky2.0/main/scripts/install.ps1 | iex
#
# The release asset is named openclicky-windows.exe because three files called
# "openclicky" would collide in a flat download. This installs it as
# openclicky.exe, which is what the README and CHANGELOG assume.
#
# Override the destination with PREFIX, pin a version with VERSION, and skip the
# launch at the end with NO_LAUNCH=1.
$ErrorActionPreference = 'Stop'

$Repo = '123oqwe/openclicky2.0'
$Prefix = if ($env:PREFIX) { $env:PREFIX } else { Join-Path $HOME '.local\bin' }
$Version = $env:VERSION

$asset = 'openclicky-windows.exe'
$base = if ($env:RELEASE_BASE) {
    $env:RELEASE_BASE
} elseif ($Version) {
    "https://github.com/$Repo/releases/download/$Version"
} else {
    "https://github.com/$Repo/releases/latest/download"
}
$url = "$base/$asset"

# Integrity, not authenticity: SHA256SUMS is unsigned and comes from the host that
# served the asset, so it catches a truncated or swapped download and nothing more.
function Assert-Checksum([string]$Path, [string]$Name) {
    try {
        $response = Invoke-WebRequest -Uri "$base/SHA256SUMS" -UseBasicParsing
    } catch {
        throw "could not fetch SHA256SUMS from $base, so nothing can be verified: $_"
    }
    $sums = $response.Content
    # GitHub serves release assets as octet-stream, so .Content can arrive as bytes.
    if ($sums -is [byte[]]) { $sums = [Text.Encoding]::UTF8.GetString($sums) }
    $expected = $null
    foreach ($line in ($sums -split "`r?`n")) {
        $parts = $line -split '\s+'
        if ($parts.Count -ge 2 -and $parts[1] -eq $Name) { $expected = $parts[0]; break }
    }
    if (-not $expected) {
        throw "$Name is not listed in SHA256SUMS, so it cannot be verified. Nothing was installed."
    }
    $actual = (Get-FileHash -Path $Path -Algorithm SHA256).Hash
    if ($actual -ne $expected) {
        throw "$Name does not match SHA256SUMS. Nothing was installed.`n  expected $expected`n  got      $actual"
    }
    Write-Host "verified $Name"
}

$target = Join-Path $Prefix 'openclicky.exe'
# Staged beside the destination, not in it: a file that fails its checksum must
# not already be sitting where the next `openclicky` call will run it.
$staging = Join-Path ([IO.Path]::GetTempPath()) "openclicky-$([Guid]::NewGuid().ToString('N')).exe"

Write-Host "downloading $asset"
try {
    Invoke-WebRequest -Uri $url -OutFile $staging -UseBasicParsing

    # The marker file GitHub attaches is not the binary itself, so make sure
    # something runnable arrived before telling anyone it worked.
    if (-not (Test-Path $staging) -or (Get-Item $staging).Length -lt 100000) {
        throw "download looks wrong: $staging is missing or too small to be the CLI"
    }

    Assert-Checksum -Path $staging -Name $asset
    New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
    Move-Item -Force -Path $staging -Destination $target
} finally {
    Remove-Item -Force -ErrorAction SilentlyContinue $staging
}

try {
    $version = (& $target --version) 2>&1
    Write-Host "installed $version"
} catch {
    throw "the downloaded file did not run: $_"
}

Write-Host "  at $target"

# The desktop shell, from the NSIS installer the release already publishes. Its
# name carries the release's own version, so it has to come from the API: a
# constructed name goes stale with the next release and 404s.
$api = if ($Version) { "https://api.github.com/repos/$Repo/releases/tags/$Version" } else { "https://api.github.com/repos/$Repo/releases/latest" }
$setup = $null
$installedShell = $false
try {
    $release = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'openclicky-install' }
    $setup = ($release.assets | Where-Object { $_.name -like '*_x64-setup.exe' } | Select-Object -First 1).name
} catch {
    $setup = $null
}

if (-not $setup) {
    Write-Host "  this release has no Windows desktop build; the CLI still works"
} else {
    $tmpSetup = Join-Path $env:TEMP $setup
    $downloaded = $false
    try {
        Write-Host "downloading $setup"
        Invoke-WebRequest -Uri "$base/$setup" -OutFile $tmpSetup -UseBasicParsing
        $downloaded = $true
    } catch {
        Write-Host "  could not download the desktop shell; the CLI still works: $_"
    }
    if ($downloaded) {
        try {
            # Outside the download's catch on purpose: a mismatch is not a network
            # hiccup to shrug at, it means the bytes are not the published bytes.
            Assert-Checksum -Path $tmpSetup -Name $setup
            # A file downloaded from the internet carries a mark-of-the-web that can stop
            # the installer from running without a manual "unblock".
            Unblock-File -Path $tmpSetup
            Write-Host "installing the desktop shell (silent)"
            try {
                Start-Process -FilePath $tmpSetup -ArgumentList '/S' -Wait
                $installedShell = $true
            } catch {
                Write-Host "  could not run the desktop installer; the CLI still works: $_"
            }
        } finally {
            Remove-Item -Force -ErrorAction SilentlyContinue $tmpSetup
        }
    }
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

if ($env:NO_LAUNCH -ne '1' -and $installedShell) {
    Write-Host "starting OpenClicky"
    # `openclicky` with no subcommand only prints usage; `app` is the one that
    # starts the shell, and it finds the shell the installer just put down.
    Start-Process -FilePath $target -ArgumentList 'app'
}
