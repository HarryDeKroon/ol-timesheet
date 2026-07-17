param(
    [Parameter(Position = 0)]
    [string]$Command,

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments
)

$ErrorActionPreference = "Stop"

$ImageName = if ($env:IMAGE_NAME) { $env:IMAGE_NAME } else { "oltimesheet:0.9.1" }
$ContainerName = if ($env:CONTAINER_NAME) { $env:CONTAINER_NAME } else { "oltimesheet" }
$ConfigVolume = if ($env:CONFIG_VOLUME) { $env:CONFIG_VOLUME } else { "oltimesheet-config" }
$HostPort = if ($env:HOST_PORT) { $env:HOST_PORT } else { "8081" }
$ContainerPort = if ($env:CONTAINER_PORT) { $env:CONTAINER_PORT } else { "8081" }
$XdgConfigHome = if ($env:XDG_CONFIG_HOME) { $env:XDG_CONFIG_HOME } else { "/root/.config/Timesheet" }
$AppConfigDir = if ($env:APP_CONFIG_DIR) { $env:APP_CONFIG_DIR } else { "$XdgConfigHome/timesheet" }
$SessionsDir = "$AppConfigDir/sessions"
$HelperImage = if ($env:HELPER_IMAGE) { $env:HELPER_IMAGE } else { "alpine:3.20" }
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$EnvFile = Join-Path $ScriptDir ".env"

function Show-Usage {
    Write-Error "Usage: .\oltimesheet.ps1 {start|stop|log|tail|users|sessions|rm <session_id...>}"
}

function Ensure-ImageExists {
    docker image inspect $ImageName *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "Docker image '$ImageName' not found. Build it first (e.g. docker build -t $ImageName .)."
    }
}

function Test-ContainerRunning {
    $name = docker ps --filter "name=^$ContainerName$" --filter "status=running" --format "{{.Names}}"
    return $name -match "^$ContainerName$"
}

function Test-ContainerExists {
    $name = docker ps -a --filter "name=^$ContainerName$" --format "{{.Names}}"
    return $name -match "^$ContainerName$"
}

function Start-TimesheetContainer {
    Ensure-ImageExists

    if (Test-ContainerRunning) {
        Write-Output "$ContainerName already running."
        return
    }

    if (Test-ContainerExists) {
        docker start $ContainerName *> $null
        Write-Output "$ContainerName started."
        return
    }

    $dockerArgs = @(
        "run", "-d",
        "--name", $ContainerName,
        "--restart", "unless-stopped",
        "-p", "$HostPort`:$ContainerPort",
        "-v", "$ConfigVolume`:$XdgConfigHome",
        "-e", "XDG_CONFIG_HOME=$XdgConfigHome"
    )

    if (Test-Path -LiteralPath $EnvFile) {
        $dockerArgs += @("--env-file", $EnvFile)
    }

    $dockerArgs += $ImageName
    docker @dockerArgs *> $null
    Write-Output "$ContainerName created and started."
}

function Stop-TimesheetContainer {
    if (Test-ContainerRunning) {
        docker stop $ContainerName *> $null
        Write-Output "$ContainerName stopped."
    }
    else {
        Write-Output "$ContainerName not running."
    }
}

function Get-Users {
    docker run --rm -v "$ConfigVolume`:$XdgConfigHome" $HelperImage sh -lc 'cfg_dir="$1"; [ -d "$cfg_dir" ] || exit 0; for d in "$cfg_dir"/*; do [ -d "$d" ] || continue; [ -f "$d/prefs.json" ] && basename "$d"; done | sort -u' sh $AppConfigDir
}

function Get-Sessions {
    docker run --rm -v "$ConfigVolume`:$XdgConfigHome" $HelperImage sh -lc 'sessions_dir="$1"; [ -d "$sessions_dir" ] || exit 0; for f in "$sessions_dir"/*.json; do [ -f "$f" ] || continue; basename "$f" .json; done | sort -u' sh $SessionsDir
}

function Remove-Sessions {
    param([string[]]$SessionIds)

    if (-not $SessionIds -or $SessionIds.Count -eq 0) {
        throw "rm requires at least one session id."
    }

    $paths = @()
    foreach ($sid in $SessionIds) {
        $paths += "$SessionsDir/$sid.json"
    }

    docker run --rm -v "$ConfigVolume`:$XdgConfigHome" $HelperImage sh -lc 'for path in "$@"; do if [ -f "$path" ]; then rm -f "$path"; echo "removed $(basename "$path" .json)"; else echo "missing $(basename "$path" .json)"; fi; done' sh @paths
}

switch ($Command) {
    "start" { Start-TimesheetContainer }
    "stop" { Stop-TimesheetContainer }
    "log" { docker logs $ContainerName }
    "tail" { docker logs -f $ContainerName }
    "users" { Get-Users }
    "sessions" { Get-Sessions }
    "rm" { Remove-Sessions -SessionIds $Arguments }
    default { Show-Usage }
}
