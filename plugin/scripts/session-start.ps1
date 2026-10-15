if (-not $env:CLAUDE_PLUGIN_DATA) {
    exit 0
}
$wizardBin = Join-Path $env:CLAUDE_PLUGIN_DATA 'bin/wizard.exe'
if (-not (Test-Path -LiteralPath $wizardBin -PathType Leaf)) {
    exit 0
}
& $wizardBin hook
exit $LASTEXITCODE
