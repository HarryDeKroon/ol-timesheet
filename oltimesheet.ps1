param(
    [Parameter(Position = 0)]
    [string]$Command,

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Arguments
)

$ErrorActionPreference = "Stop"

$ImageName     = if ($env:IMAGE_NAME)     { $env:IMAGE_NAME }     else { "oltimesheet:0.9.1" }
$ContainerName = if ($env:CONTAINER_NAME) { $env:CONTAINER_NAME } else { "oltimesheet" }
$HostPort      = if ($env:HOST_PORT)      { $env:HOST_PORT }      else { "8081" }
$ContainerPort = if ($env:CONTAINER_PORT) { $env:CONTAINER_PORT } else { "8081" }
$XdgConfigHome = if ($env:XDG_CONFIG_HOME) { $env:XDG_CONFIG_HOME } else { "/root/.config/Timesheet" }
$AppConfigDir  = if ($env:APP_CONFIG_DIR)  { $env:APP_CONFIG_DIR }  else { "$XdgConfigHome/timesheet" }
$SessionsDir   = "$AppConfigDir/sessions"
$CacheFile     = "$AppConfigDir/cache.yaml"
$HelperImage   = if ($env:HELPER_IMAGE) { $env:HELPER_IMAGE } else { "alpine:3.20" }
$ScriptDir     = Split-Path -Parent $MyInvocation.MyCommand.Path
$ConfigDir     = if ($env:CONFIG_DIR) { $env:CONFIG_DIR } else { Join-Path $ScriptDir "server-config" }
$EnvFile       = Join-Path $ScriptDir ".env"

function Show-Usage {
    Write-Error "Usage: .\oltimesheet.ps1 {start|stop|log|tail|users|sessions|rm cache|rm all|rm <session_id...>}"
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
    New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null

    if (Test-ContainerRunning) {
        Write-Output "$ContainerName already running."
        return
    }

    if (Test-ContainerExists) {
        docker start $ContainerName *> $null
        Write-Output "$ContainerName started."
        return
    }

    # Docker Desktop on Windows requires forward slashes in bind-mount paths.
    $configDirDocker = $ConfigDir -replace '\\', '/'
    if ($configDirDocker -match '^([A-Za-z]):') {
        $configDirDocker = '/' + $configDirDocker[0].ToString().ToLower() + $configDirDocker.Substring(2)
    }

    $dockerArgs = @(
        "run", "-d",
        "--name", $ContainerName,
        "--restart", "unless-stopped",
        "-p", "$HostPort`:$ContainerPort",
        "-v", "$configDirDocker`:$XdgConfigHome",
        "-e", "XDG_CONFIG_HOME=$XdgConfigHome"
    )

    if (Test-Path -LiteralPath $EnvFile) {
        $dockerArgs += @("--env-file", $EnvFile)
    }

    $dockerArgs += $ImageName
    docker @dockerArgs *> $null
    Write-Output "$ContainerName created and started. Config: $ConfigDir"
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

function Get-ConfigMount {
    $d = $ConfigDir -replace '\\', '/'
    if ($d -match '^([A-Za-z]):') { $d = '/' + $d[0].ToString().ToLower() + $d.Substring(2) }
    return "$d`:$XdgConfigHome"
}

function Get-Users {
    docker run --rm -v (Get-ConfigMount) $HelperImage sh -lc 'cfg_dir="$1"; [ -d "$cfg_dir" ] || exit 0; for d in "$cfg_dir"/*; do [ -d "$d" ] || continue; [ -f "$d/prefs.json" ] && basename "$d"; done | sort -u' sh $AppConfigDir
}

function Get-Sessions {
    docker run --rm -v (Get-ConfigMount) $HelperImage sh -lc 'sessions_dir="$1"; [ -d "$sessions_dir" ] || exit 0; for f in "$sessions_dir"/*.json; do [ -f "$f" ] || continue; basename "$f" .json; done | sort -u' sh $SessionsDir
}

function Remove-Cache {
    docker run --rm -v (Get-ConfigMount) $HelperImage sh -lc 'f="$1"; if [ -f "$f" ]; then rm -f "$f"; echo "removed cache.yaml"; else echo "cache.yaml not found"; fi' sh $CacheFile
}

function Remove-AllSessions {
    docker run --rm -v (Get-ConfigMount) $HelperImage sh -lc 'sessions_dir="$1"; [ -d "$sessions_dir" ] || { echo "sessions directory not found"; exit 0; }; count=0; for f in "$sessions_dir"/*.json; do [ -f "$f" ] || continue; rm -f "$f"; count=$((count+1)); done; echo "removed $count session(s)"' sh $SessionsDir
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

    docker run --rm -v (Get-ConfigMount) $HelperImage sh -lc 'for path in "$@"; do if [ -f "$path" ]; then rm -f "$path"; echo "removed $(basename "$path" .json)"; else echo "missing $(basename "$path" .json)"; fi; done' sh @paths
}

switch ($Command) {
    "start"    { Start-TimesheetContainer }
    "stop"     { Stop-TimesheetContainer }
    "log"      { docker logs $ContainerName }
    "tail"     { docker logs -f $ContainerName }
    "users"    { Get-Users }
    "sessions" { Get-Sessions }
    "rm" {
        if ($Arguments.Count -gt 0 -and $Arguments[0] -eq "cache") {
            Remove-Cache
        } elseif ($Arguments.Count -gt 0 -and $Arguments[0] -eq "all") {
            Remove-AllSessions
        } else {
            Remove-Sessions -SessionIds $Arguments
        }
    }
    default { Show-Usage }
}