if (-not $env:CLAUDE_PLUGIN_DATA) {
    exit 0
}

$binDir = Join-Path $env:CLAUDE_PLUGIN_DATA 'bin'
$bin = Join-Path $binDir 'wizard.exe'
$currentFile = Join-Path $binDir 'current'
$arch = 'x86_64'
if ("$env:PROCESSOR_ARCHITECTURE$env:PROCESSOR_ARCHITEW6432" -match 'ARM64') {
    $arch = 'aarch64'
}
$target = "$arch-pc-windows-msvc"

function Get-WizardProxy([Uri] $uri) {
    $proxy = if ($uri.Scheme -eq 'https') {
        @($env:HTTPS_PROXY, $env:https_proxy) | Where-Object { $_ } | Select-Object -First 1
    } else {
        @($env:HTTP_PROXY, $env:http_proxy) | Where-Object { $_ } | Select-Object -First 1
    }
    if (-not $proxy) {
        return $null
    }
    $noProxy = @($env:NO_PROXY, $env:no_proxy) | Where-Object { $_ } | Select-Object -First 1
    foreach ($entry in "$noProxy".Split(',')) {
        $entry = $entry.Trim()
        $suffix = $entry.TrimStart('*').TrimStart('.').Split(':')[0]
        if ($entry -eq '*' -or ($suffix -and ($uri.Host -eq $suffix -or $uri.Host.EndsWith(".$suffix")))) {
            return $null
        }
    }
    return $proxy
}

function Install-Wizard {
    $lock = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\binaries.lock.json') | ConvertFrom-Json
    $installed = if (Test-Path -LiteralPath $currentFile) { "$(Get-Content -Raw -LiteralPath $currentFile)".Trim() }
    if ((Test-Path -LiteralPath $bin) -and $installed -eq $lock.version) {
        return
    }
    $sha = $lock.sha256.$target
    if (-not $lock.version -or -not $lock.url -or -not $sha) {
        return
    }
    $dir = Join-Path $binDir $lock.version
    $download = Join-Path $dir ".download.$PID"
    $staged = "$bin.$PID"
    $old = "$bin.old.$PID"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        $request = @{ Uri = "$($lock.url)/wizard-$target.exe"; OutFile = $download; UseBasicParsing = $true; TimeoutSec = 60 }
        $proxy = Get-WizardProxy $request.Uri
        if ($proxy) {
            $request.Proxy = $proxy
        }
        Invoke-WebRequest @request
        $stream = [IO.File]::OpenRead($download)
        try {
            $digest = [Security.Cryptography.SHA256]::Create().ComputeHash($stream)
        } finally {
            $stream.Dispose()
        }
        if ([BitConverter]::ToString($digest).Replace('-', '').ToLowerInvariant() -ne $sha.ToLowerInvariant()) {
            throw 'checksum mismatch'
        }
        $versioned = Join-Path $dir 'wizard.exe'
        Move-Item -Force -LiteralPath $download -Destination $versioned
        Copy-Item -Force -LiteralPath $versioned -Destination $staged
        if (Test-Path -LiteralPath $bin) {
            Move-Item -Force -LiteralPath $bin -Destination $old
        }
        try {
            Move-Item -Force -LiteralPath $staged -Destination $bin
        } catch {
            if (Test-Path -LiteralPath $old) {
                Move-Item -Force -LiteralPath $old -Destination $bin
            }
            throw
        }
        Get-ChildItem -LiteralPath $binDir -Filter 'wizard.exe.old.*' | Remove-Item -Force -ErrorAction SilentlyContinue
        Set-Content -NoNewline -LiteralPath "$currentFile.$PID" -Value $lock.version
        Move-Item -Force -LiteralPath "$currentFile.$PID" -Destination $currentFile
    } finally {
        Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath $download, $staged
    }
}

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
try {
    Install-Wizard
    Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath (Join-Path $binDir 'install-error')
} catch {
    try {
        New-Item -ItemType Directory -Force -Path $binDir | Out-Null
        Set-Content -LiteralPath (Join-Path $binDir 'install-error') -Value "$(Get-Date -Format o) $($_.Exception.Message)"
    } catch {
    }
}
if (-not (Test-Path -LiteralPath $bin -PathType Leaf)) {
    exit 0
}
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
& $bin hook
exit $LASTEXITCODE
