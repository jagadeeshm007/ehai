# ehai installer (Windows). See install.sh for macOS/Linux.
#
#   irm https://raw.githubusercontent.com/jagadeeshm007/ehai/main/install/install.ps1 | iex
#
# Env overrides: EHAI_NAME, EHAI_CHANNEL (alpha|beta|stable, default alpha),
# GITHUB_TOKEN (used if `gh auth token` isn't available).
$ErrorActionPreference = "Stop"

$Repo = "jagadeeshm007/ehai"
$Name = if ($env:EHAI_NAME) { $env:EHAI_NAME } else { "ehai" }
$Channel = if ($env:EHAI_CHANNEL) { $env:EHAI_CHANNEL } else { "alpha" }
$Api = "https://api.github.com/repos/$Repo"

function Die($msg) {
    Write-Error $msg
    exit 1
}

if (-not (Get-Command minisign -ErrorAction SilentlyContinue)) {
    Die "minisign is required to verify the release signature (scoop install minisign, or download from jedisct1/minisign)"
}

# ---- auth --------------------------------------------------------------------
$Token = $null
if (Get-Command gh -ErrorAction SilentlyContinue) {
    try { $Token = (gh auth token) 2>$null } catch {}
}
if (-not $Token -and $env:GITHUB_TOKEN) { $Token = $env:GITHUB_TOKEN }
$Headers = @{ "User-Agent" = "ehai-install" }
if ($Token) { $Headers["Authorization"] = "Bearer $Token" }

# ---- platform target -----------------------------------------------------------
$Arch = if ([System.Environment]::Is64BitOperatingSystem) { "x86_64" } else { Die "unsupported architecture" }
$Target = "$Arch-pc-windows-msvc"

# ---- pick a release for the channel ---------------------------------------------
Write-Host "Looking up the latest $Channel release..."
$Releases = Invoke-RestMethod -Uri "$Api/releases" -Headers $Headers

# GitHub lists releases newest-first: the first tag the channel accepts is the
# latest for that channel. alpha accepts every tag; beta skips -alpha tags;
# stable wants a tag with no pre-release suffix at all.
$Tag = $null
foreach ($r in $Releases) {
    $t = $r.tag_name
    switch ($Channel) {
        "alpha" { $Tag = $t }
        "beta" { if ($t -notmatch "-alpha") { $Tag = $t } }
        "stable" { if ($t -notmatch "-") { $Tag = $t } }
        default { Die "unknown channel: $Channel" }
    }
    if ($Tag) { break }
}
if (-not $Tag) { Die "no $Channel release found for $Repo" }
Write-Host "Installing $Tag for $Target"

$Version = $Tag.TrimStart("v")
$Asset = "ehai-$Version-$Target.zip"
$BaseUrl = "https://github.com/$Repo/releases/download/$Tag"

$Tmp = Join-Path $env:TEMP "ehai-install-$(Get-Random)"
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    Invoke-WebRequest -Uri "$BaseUrl/$Asset" -Headers $Headers -OutFile (Join-Path $Tmp $Asset)
    Invoke-WebRequest -Uri "$BaseUrl/SHA256SUMS" -Headers $Headers -OutFile (Join-Path $Tmp "SHA256SUMS")
    Invoke-WebRequest -Uri "$BaseUrl/SHA256SUMS.minisig" -Headers $Headers -OutFile (Join-Path $Tmp "SHA256SUMS.minisig")

    Write-Host "Verifying signature and checksum..."
    Push-Location $Tmp
    try {
        & minisign -Vm SHA256SUMS -x SHA256SUMS.minisig -P RWTKHW5dGCGJOywz9PXOM27FdVwe+Eqj9qrgNrFh0faYS9LKS0kL6uhC
        if ($LASTEXITCODE -ne 0) { Die "signature verification failed — refusing to install" }

        $Line = Get-Content SHA256SUMS | Where-Object { $_ -match [regex]::Escape($Asset) }
        if (-not $Line) { Die "$Asset not listed in SHA256SUMS" }
        $Expected = ($Line -split "\s+")[0]
        $Actual = (Get-FileHash $Asset -Algorithm SHA256).Hash.ToLower()
        if ($Expected -ne $Actual) { Die "checksum mismatch for $Asset" }

        Expand-Archive -Path $Asset -DestinationPath $Tmp -Force
        $Dir = Join-Path $Tmp "ehai-$Version-$Target"
        $Exe = Join-Path $Dir "ehai.exe"
        if ($Name -ne "ehai") {
            $Renamed = Join-Path $Dir "$Name.exe"
            Rename-Item $Exe $Renamed
            $Exe = $Renamed
        }

        Write-Host "Running '$Name self install'..."
        & $Exe self install

        $BinDir = Join-Path $env:USERPROFILE ".ehai\bin"
        $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if (-not ($UserPath -split ";" | Where-Object { $_ -eq $BinDir })) {
            [Environment]::SetEnvironmentVariable("Path", "$UserPath;$BinDir", "User")
            Write-Host "Added $BinDir to your user PATH (open a new terminal to pick it up)."
        }

        $Installed = Join-Path $BinDir "$Name.exe"
        Write-Host "Running '$Name init'..."
        & $Installed init

        Write-Host "Done. Open a new terminal to use '$Name'."
    } finally {
        Pop-Location
    }
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
