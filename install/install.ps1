# ehai installer (Windows). See install.sh for macOS/Linux.
#
# Public repo (no token needed):
#   irm https://raw.githubusercontent.com/jagadeeshm007/ehai/main/install/install.ps1 | iex
#
# Private repo (org deployment) — provide a GitHub PAT with `contents: read`:
#   $env:EHAI_TOKEN = "ghp_xxx"
#   $script = Invoke-RestMethod -Uri "https://api.github.com/repos/YOUR_ORG/ehai/contents/install/install.ps1" `
#     -Headers @{ Authorization = "Bearer $env:EHAI_TOKEN"; Accept = "application/vnd.github.raw" }
#   Invoke-Expression $script
#
# After install the token is saved to %USERPROFILE%\.ehai\token so `ehai update`
# works without re-entering it.
#
# Env overrides: EHAI_TOKEN, EHAI_NAME, EHAI_CHANNEL (alpha|beta|stable, default alpha),
#   EHAI_REQUIRE_SIGNATURE=1 (fail instead of falling back to checksum-only when
#   no minisign verifier can be found or fetched)
$ErrorActionPreference = "Stop"

$Repo = "jagadeeshm007/ehai"
$Name = if ($env:EHAI_NAME) { $env:EHAI_NAME } else { "ehai" }
$Channel = if ($env:EHAI_CHANNEL) { $env:EHAI_CHANNEL } else { "alpha" }
$Api = "https://api.github.com/repos/$Repo"

function Die($msg) { Write-Error $msg; exit 1 }

# ---- auth --------------------------------------------------------------------
# Resolution order: EHAI_TOKEN → GITHUB_TOKEN/GH_TOKEN → gh auth token
$Token = $null
if ($env:EHAI_TOKEN) {
    $Token = $env:EHAI_TOKEN
} elseif ($env:GITHUB_TOKEN) {
    $Token = $env:GITHUB_TOKEN
} elseif ($env:GH_TOKEN) {
    $Token = $env:GH_TOKEN
} elseif (Get-Command gh -ErrorAction SilentlyContinue) {
    try { $Token = (gh auth token 2>$null) } catch {}
}

$Headers = @{ "User-Agent" = "ehai-install" }
if ($Token) { $Headers["Authorization"] = "Bearer $Token" }

# Download a URL and follow GitHub's two-step redirect for release assets:
# 1st hop (api.github.com) needs auth; 2nd hop (CDN) needs no auth.
function FetchUrl($url, $outFile) {
    try {
        # Attempt direct download first (works for public repos and non-redirected URLs)
        Invoke-WebRequest -Uri $url -Headers $Headers -OutFile $outFile
    } catch {
        if ($_.Exception.Response -and $_.Exception.Response.StatusCode -in 301, 302, 307, 308) {
            $location = $_.Exception.Response.Headers.Location
            Invoke-WebRequest -Uri $location -OutFile $outFile
        } else {
            throw
        }
    }
}

# ---- platform target -----------------------------------------------------------
$Arch = if ([System.Environment]::Is64BitOperatingSystem) { "x86_64" } else { Die "unsupported architecture" }
$Target = "$Arch-pc-windows-msvc"

# ---- pick a release for the channel ---------------------------------------------
Write-Host "Looking up the latest $Channel release..."
$Releases = Invoke-RestMethod -Uri "$Api/releases" -Headers $Headers

$Tag = $null
foreach ($r in $Releases) {
    $t = $r.tag_name
    switch ($Channel) {
        "alpha"  { $Tag = $t }
        "beta"   { if ($t -notmatch "-alpha") { $Tag = $t } }
        "stable" { if ($t -notmatch "-") { $Tag = $t } }
        default  { Die "unknown channel: $Channel" }
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
    FetchUrl "$BaseUrl/$Asset" (Join-Path $Tmp $Asset)
    FetchUrl "$BaseUrl/SHA256SUMS" (Join-Path $Tmp "SHA256SUMS")

    Push-Location $Tmp
    try {
        # No minisign on the machine? Fetch the official build into $Tmp (deleted
        # afterwards, nothing installed system-wide). Hash pinned and checked against
        # the minisign author's key RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3.
        $Minisign = (Get-Command minisign -ErrorAction SilentlyContinue).Source
        if (-not $Minisign) {
            Write-Host "minisign not installed; fetching a temporary copy to verify the signature..."
            try {
                $MsZip = Join-Path $Tmp "minisign-0.12-win64.zip"
                Invoke-WebRequest -Uri "https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-win64.zip" -OutFile $MsZip -UseBasicParsing
                if ((Get-FileHash $MsZip -Algorithm SHA256).Hash.ToLower() -ne "37b600344e20c19314b2e82813db2bfdcc408b77b876f7727889dbd46d539479") {
                    Write-Warning "downloaded minisign failed its pinned checksum, not using it"
                } else {
                    Expand-Archive -Path $MsZip -DestinationPath (Join-Path $Tmp "ms") -Force
                    $Minisign = Join-Path $Tmp "ms\minisign-win64\x86_64\minisign.exe"
                }
            } catch { Write-Warning "could not fetch minisign: $_" }
        }

        if ($Minisign) {
            Write-Host "Verifying signature..."
            FetchUrl "$BaseUrl/SHA256SUMS.minisig" (Join-Path $Tmp "SHA256SUMS.minisig")
            & $Minisign -Vm SHA256SUMS -x SHA256SUMS.minisig -P RWTKHW5dGCGJOywz9PXOM27FdVwe+Eqj9qrgNrFh0faYS9LKS0kL6uhC | Out-Null
            if ($LASTEXITCODE -ne 0) { Die "signature verification failed — refusing to install" }
        } elseif ($env:EHAI_REQUIRE_SIGNATURE -eq "1") {
            Die "could not get minisign and EHAI_REQUIRE_SIGNATURE=1 (scoop install minisign)"
        } else {
            Write-Warning "could not get minisign, skipping signature check (SHA-256 checksum is still verified)."
        }

        Write-Host "Verifying checksum..."

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

        # Persist the token so `ehai update` works without re-entering it
        if ($Token) {
            $EhaiHome = Join-Path $env:USERPROFILE ".ehai"
            $TokenFile = Join-Path $EhaiHome "token"
            $Token | Set-Content -Path $TokenFile -NoNewline
            # Restrict to current user only
            $acl = Get-Acl $TokenFile
            $acl.SetAccessRuleProtection($true, $false)
            $rule = New-Object System.Security.AccessControl.FileSystemAccessRule(
                [System.Security.Principal.WindowsIdentity]::GetCurrent().Name,
                "FullControl", "Allow")
            $acl.AddAccessRule($rule)
            Set-Acl -Path $TokenFile -AclObject $acl
            Write-Host "Token saved to $TokenFile for future updates."
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
