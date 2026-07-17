#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME="${IMAGE_NAME:-oltimesheet:0.9.1}"
CONTAINER_NAME="${CONTAINER_NAME:-oltimesheet}"
CONFIG_VOLUME="${CONFIG_VOLUME:-oltimesheet-config}"
HOST_PORT="${HOST_PORT:-8081}"
CONTAINER_PORT="${CONTAINER_PORT:-8081}"
XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-/root/.config/Timesheet}"
APP_CONFIG_DIR="${APP_CONFIG_DIR:-$XDG_CONFIG_HOME/timesheet}"
SESSIONS_DIR="$APP_CONFIG_DIR/sessions"
HELPER_IMAGE="${HELPER_IMAGE:-alpine:3.20}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
ENV_FILE="$SCRIPT_DIR/.env"

usage() {
    echo "Usage: $0 {start|stop|log|tail|users|sessions|rm <session_id...>}" >&2
    exit 1
}

ensure_image_exists() {
    if ! docker image inspect "$IMAGE_NAME" >/dev/null 2>&1; then
        echo "Docker image '$IMAGE_NAME' not found. Build it first (e.g. docker build -t $IMAGE_NAME .)." >&2
        exit 1
    fi
}

start_container() {
    ensure_image_exists

    if docker ps --filter "name=^${CONTAINER_NAME}$" --filter "status=running" --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
        echo "$CONTAINER_NAME already running."
        return 0
    fi

    if docker ps -a --filter "name=^${CONTAINER_NAME}$" --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
        docker start "$CONTAINER_NAME" >/dev/null
        echo "$CONTAINER_NAME started."
        return 0
    fi

    args=(
        run -d
        --name "$CONTAINER_NAME"
        --restart unless-stopped
        -p "${HOST_PORT}:${CONTAINER_PORT}"
        -v "${CONFIG_VOLUME}:${XDG_CONFIG_HOME}"
        -e "XDG_CONFIG_HOME=${XDG_CONFIG_HOME}"
    )
    if [[ -f "$ENV_FILE" ]]; then
        args+=(--env-file "$ENV_FILE")
    fi
    args+=("$IMAGE_NAME")

    docker "${args[@]}" >/dev/null
    echo "$CONTAINER_NAME created and started."
}

stop_container() {
    if docker ps --filter "name=^${CONTAINER_NAME}$" --filter "status=running" --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
        docker stop "$CONTAINER_NAME" >/dev/null
        echo "$CONTAINER_NAME stopped."
    else
        echo "$CONTAINER_NAME not running."
    fi
}

list_users() {
    docker run --rm -v "${CONFIG_VOLUME}:${XDG_CONFIG_HOME}" "$HELPER_IMAGE" sh -lc \
        'cfg_dir="$1"; [ -d "$cfg_dir" ] || exit 0; for d in "$cfg_dir"/*; do [ -d "$d" ] || continue; [ -f "$d/prefs.json" ] && basename "$d"; done | sort -u' \
        sh "$APP_CONFIG_DIR"
}

list_sessions() {
    docker run --rm -v "${CONFIG_VOLUME}:${XDG_CONFIG_HOME}" "$HELPER_IMAGE" sh -lc \
        'sessions_dir="$1"; [ -d "$sessions_dir" ] || exit 0; for f in "$sessions_dir"/*.json; do [ -f "$f" ] || continue; basename "$f" .json; done | sort -u' \
        sh "$SESSIONS_DIR"
}

remove_sessions() {
    if [[ $# -eq 0 ]]; then
        echo "rm requires at least one session id." >&2
        exit 1
    fi

    args=()
    for sid in "$@"; do
        args+=("${SESSIONS_DIR}/${sid}.json")
    done

    docker run --rm -v "${CONFIG_VOLUME}:${XDG_CONFIG_HOME}" "$HELPER_IMAGE" sh -lc \
        'for path in "$@"; do if [ -f "$path" ]; then rm -f "$path"; echo "removed $(basename "$path" .json)"; else echo "missing $(basename "$path" .json)"; fi; done' \
        sh "${args[@]}"
}

cmd="${1:-}"
case "$cmd" in
    start)
        start_container
        ;;
    stop)
        stop_container
        ;;
    log)
        docker logs "$CONTAINER_NAME"
        ;;
    tail)
        docker logs -f "$CONTAINER_NAME"
        ;;
    users)
        list_users
        ;;
    sessions)
        list_sessions
        ;;
    rm)
        shift
        remove_sessions "$@"
        ;;
    *)
        usage
        ;;
esac
