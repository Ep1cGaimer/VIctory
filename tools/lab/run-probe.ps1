param(
    [Parameter(Mandatory=$true)][string]$Exe,
    [Parameter(Mandatory=$true)][string]$RunDir,
    [string]$Keys,
    [int]$Seconds = 35,
    [switch]$Warp,
    [switch]$Validation,
    [string]$Console,
    [string]$EnvVars
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$package = 'C:\Games\GTA6.Xbox.v1.0.2.1-CYBERLEEK'
$originalProfile = Join-Path $env:TEMP 'victory-readonly-run-20261124\hardware\data'
$env:VICTORY_GAME_DIR = $package
$env:VICTORY_DATA_DIR = Join-Path $RunDir 'data'
$env:VICTORY_CACHE_DIR = Join-Path $RunDir 'cache'
$env:VICTORY_CAPTURE = Join-Path $RunDir 'frames'
$env:VICTORY_CAPTURE_FRAMES = '120,300,450,600,750,900,1050,1200,1500,1800,2100,2400,2700,3000,3600'
$env:VICTORY_CONFIG = Join-Path $RunDir 'isolated.toml'
$env:VICTORY_LOG = 'warn,victory_gpu::host=info,victory_gpu::queue=info,victory_gpu::copystat=info,victory_gpu::capture=info'
$env:VICTORY_D3D12_DRED = '1'
[Environment]::SetEnvironmentVariable('VICTORY_D3D12_WARP',$(if ($Warp) {'1'} else {$null}),'Process')
[Environment]::SetEnvironmentVariable('VICTORY_D3D12_DEBUG',$(if ($Validation) {'1'} else {$null}),'Process')
[Environment]::SetEnvironmentVariable('VICTORY_D3D12_GBV',$(if ($Validation) {'1'} else {$null}),'Process')
[Environment]::SetEnvironmentVariable('VICTORY_KEYS',$(if ($Keys) {$Keys} else {$null}),'Process')
if ($EnvVars) { foreach ($kv in $EnvVars.Split(';')) { $p = $kv.Split('=',2); [Environment]::SetEnvironmentVariable($p[0],$p[1],'Process') } }
New-Item -ItemType Directory -Path $env:VICTORY_CAPTURE -Force | Out-Null
if (-not (Test-Path -LiteralPath $env:VICTORY_DATA_DIR)) {
    $profile = Join-Path $originalProfile '7952987A'
    $targetProfile = Join-Path $env:VICTORY_DATA_DIR '7952987A'
    New-Item -ItemType Directory -Path $targetProfile -Force | Out-Null
    foreach ($source in 'storage','saves') {
        $folder = Join-Path $profile $source
        if (Test-Path -LiteralPath $folder) { Copy-Item -LiteralPath $folder -Destination (Join-Path $targetProfile $source) -Recurse }
    }
}
Set-Content -LiteralPath $env:VICTORY_CONFIG -Value '[paths]' -Encoding ASCII
$gameExe = Join-Path $package 'game_scarlett_master.exe'
$arguments = @('run', ('"' + $gameExe + '"'), '--execute')
if ($Console) { $arguments += @('--console', $Console) }
$process = Start-Process -FilePath $Exe -ArgumentList $arguments -WorkingDirectory $RunDir -PassThru -RedirectStandardOutput (Join-Path $RunDir 'stdout.log') -RedirectStandardError (Join-Path $RunDir 'stderr.log')
Write-Output "VIctory PID $($process.Id)"
if ($process.WaitForExit($Seconds * 1000)) {
    Write-Output "Game process exited with $($process.ExitCode)."
} else {
    if ($process.CloseMainWindow()) { $process.WaitForExit(4000) | Out-Null }
    if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit(10000) | Out-Null }
    Write-Output "Ended the bounded test at $Seconds seconds."
}
Get-Content -LiteralPath (Join-Path $RunDir 'stderr.log') -Tail 8
