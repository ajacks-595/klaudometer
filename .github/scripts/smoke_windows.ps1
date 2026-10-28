# Smoke-test the Windows build on a CI runner. Installs the .msi, gives the
# app a fake Claude Code login in %USERPROFILE%\.claude\.credentials.json and
# a fake Admin API key in Credential Manager, points it at the mock API,
# checks it fetched and cached usage and API spend, and takes screenshots of
# the taskbar, the card and the settings window.
#
#   smoke_windows.ps1 -Msi path\to\Klaudometer.msi -Out output-dir
param(
    [Parameter(Mandatory)] [string] $Msi,
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$port = 18790

function Save-Screen([string] $Path) {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save($Path)
}

Write-Host "Installing $Msi"
$install = Start-Process msiexec -ArgumentList "/i `"$Msi`" /qn /l*v `"$Out\msi.log`"" -Wait -PassThru
if ($install.ExitCode -ne 0) { throw "msiexec failed with exit code $($install.ExitCode); see msi.log" }
$exe = Get-ChildItem "$env:ProgramFiles\Klaudometer", "$env:LOCALAPPDATA\Programs\Klaudometer", "$env:LOCALAPPDATA\Klaudometer" `
    -Filter *.exe -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.Name -notlike "uninstall*" } | Select-Object -First 1
if (-not $exe) { throw "Couldn't find the installed Klaudometer executable" }
Write-Host "Installed to $($exe.FullName)"

$mock = Start-Process python -ArgumentList "`"$PSScriptRoot\mock_anthropic_api.py`"", $port, "demo" -PassThru -NoNewWindow `
    -RedirectStandardOutput "$Out\mock.log" -RedirectStandardError "$Out\mock.err"

$claude = Join-Path $env:USERPROFILE ".claude"
New-Item -ItemType Directory -Force $claude | Out-Null
$expires = ([DateTimeOffset]::UtcNow.ToUnixTimeSeconds() + 86400) * 1000
$login = '{"claudeAiOauth":{"accessToken":"fake-oauth-token","expiresAt":' + $expires + ',"subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}'
Set-Content -Path (Join-Path $claude ".credentials.json") -Value $login -Encoding ascii

# Show API spend as well.
$settings = Join-Path $env:APPDATA "klaudometer"
New-Item -ItemType Directory -Force $settings | Out-Null
Set-Content -Path (Join-Path $settings "desktop.json") -Value '{"show_api": true, "label": "full"}' -Encoding ascii

# Where Klaudometer's settings window saves the Admin API key: a generic
# credential named admin.klaudometer.
cmdkey /generic:admin.klaudometer /user:admin /pass:sk-ant-admin01-fake | Out-Null

$env:KLAUDOMETER_API_BASE = "http://127.0.0.1:$port"
$app = Start-Process $exe.FullName -PassThru

try {
    $cache = Join-Path $env:LOCALAPPDATA "klaudometer"
    for ($i = 0; $i -lt 60; $i++) {
        if ((Test-Path "$cache\usage.json") -and (Test-Path "$cache\api.json")) { break }
        Start-Sleep -Seconds 1
    }
    Copy-Item "$cache\*.json" $Out -ErrorAction SilentlyContinue
    $usage = Get-Content "$cache\usage.json" -Raw | ConvertFrom-Json
    $api = Get-Content "$cache\api.json" -Raw | ConvertFrom-Json
    if ($usage.error) { throw "Usage error: $($usage.error)" }
    if ($usage.plan -ne "Max 5x" -or $usage.windows.five_hour.used -ne 81 -or $usage.extra.used -ne 12.34) {
        throw "Unexpected usage: $($usage | ConvertTo-Json -Depth 5)"
    }
    if ($api.error -or [math]::Abs($api.spent - 42.75) -gt 1e-9) { throw "Unexpected API spend: $($api | ConvertTo-Json)" }
    Write-Host "OK: login read from .credentials.json and Admin key from Credential Manager; usage and API spend fetched and cached"

    if ($app.HasExited) { throw "The app exited with code $($app.ExitCode)" }
    Write-Host "OK: app still running"

    Start-Sleep -Seconds 3
    Save-Screen "$Out\taskbar.png"
    # A second launch hands over to the running copy, which opens its card.
    $second = Start-Process $exe.FullName -PassThru
    $second.WaitForExit(15000) | Out-Null
    Start-Sleep -Seconds 3
    Save-Screen "$Out\card.png"
    # And with --settings, the settings window.
    $third = Start-Process $exe.FullName -ArgumentList "--settings=spending" -PassThru
    $third.WaitForExit(15000) | Out-Null
    Start-Sleep -Seconds 4
    Save-Screen "$Out\settings.png"
}
finally {
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
    Stop-Process -Id $mock.Id -Force -ErrorAction SilentlyContinue
}
