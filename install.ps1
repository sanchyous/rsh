# rsh installer for Windows: irm https://github.com/sanchyous/rsh/releases/latest/download/install.ps1 | iex
#   $env:RSH_VERSION = 'v0.2.0'   install a specific release
#   $env:RSH_PREFIX  = 'D:\tools\rsh'   install location (default: %LOCALAPPDATA%\Programs\rsh)
#   $env:RSH_UNINSTALL = '1'   remove rsh

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$repo = 'sanchyous/rsh'
$version = if ($env:RSH_VERSION) { $env:RSH_VERSION } else { 'latest' }
$dest = if ($env:RSH_PREFIX) { $env:RSH_PREFIX } else { Join-Path $env:LOCALAPPDATA 'Programs\rsh' }

function Say($m) { Write-Host ":: $m" -ForegroundColor Cyan }
function Ok($m) { Write-Host "OK $m" -ForegroundColor Green }

function Set-UserPath([string]$dir, [bool]$add) {
    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    $parts = @($current -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ne $dir.TrimEnd('\')) })
    if ($add) { $parts += $dir }
    [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User')
    $env:Path = (@($env:Path -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ne $dir.TrimEnd('\')) }) + $(if ($add) { $dir } else { @() })) -join ';'
}

if ($env:RSH_UNINSTALL -eq '1') {
    Say "removing $dest"
    if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
    Set-UserPath $dest $false
    Ok 'rsh removed (your config in %APPDATA%\rsh was kept)'
    return
}

$cpu = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
$arch = switch ($cpu) {
    'AMD64' { 'x86_64' }
    'ARM64' { 'aarch64' }
    default { throw "Unsupported architecture: $cpu (x86_64 and ARM64 builds are published)" }
}

$base = if ($version -eq 'latest') { "https://github.com/$repo/releases/latest/download" }
        elseif ($version.StartsWith('v')) { "https://github.com/$repo/releases/download/$version" }
        else { "https://github.com/$repo/releases/download/v$version" }

$file = "rsh-$arch-windows.zip"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("rsh-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
    Write-Host ""
    Write-Host "  RSH installer  -  windows  -  $arch  -  $version" -ForegroundColor White
    Write-Host ""
    Say "downloading $file"
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$file" -OutFile (Join-Path $tmp $file)

    try {
        Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile (Join-Path $tmp 'SHA256SUMS')
        $line = Get-Content (Join-Path $tmp 'SHA256SUMS') | Where-Object { $_ -match "\s$([regex]::Escape($file))$" } | Select-Object -First 1
        if ($line) {
            $expected = ($line -split '\s+')[0]
            $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $file)).Hash.ToLower()
            if ($actual -ne $expected.ToLower()) { throw "checksum mismatch for $file" }
            Ok 'checksum verified'
        }
    } catch [System.Net.WebException] {
        Write-Host '!  SHA256SUMS not found, skipping verification' -ForegroundColor Yellow
    }

    Say "installing to $dest"
    Expand-Archive -Path (Join-Path $tmp $file) -DestinationPath (Join-Path $tmp 'x') -Force
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Copy-Item -Recurse -Force (Join-Path $tmp 'x\*') $dest
    Set-UserPath $dest $true

    $v = & (Join-Path $dest 'rsh.exe') --version
    Ok "installed $v"
    Write-Host ""
    Write-Host "  Run 'rsh' in a new terminal to start. Docs: https://github.com/$repo" -ForegroundColor White
    Write-Host "  Tab completion: add  . '$dest\completions\rsh.ps1'  to your `$PROFILE" -ForegroundColor DarkGray
    Write-Host ""
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
