#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

PORT="${GAMENODE_SCREEN_PORT:-4777}"
STATE_FILE="${GAMENODE_SCREEN_STATE:-/tmp/gamenode-ui-state.json}"
SHOT_FILE="${GAMENODE_SCREEN_SHOT:-/tmp/gamenode-screen.png}"
PID_FILE="/tmp/gamenode-dev-screen.pid"
SERVER_PID_FILE="/tmp/gamenode-ui-review-server.pid"
LOG_FILE="/tmp/gamenode-dev-screen.log"

compute_ui_hash() {
    find crates/app/ui crates/app/examples/ui-review.rs i18n -type f 2>/dev/null \
        | sort \
        | xargs sha256sum 2>/dev/null \
        | sha256sum \
        | awk '{print $1}'
}

stop_all() {
    if [[ -f "$PID_FILE" ]]; then
        local wpid
        wpid=$(cat "$PID_FILE" 2>/dev/null || true)
        if [[ -n "$wpid" ]] && kill -0 "$wpid" 2>/dev/null; then
            kill "$wpid" 2>/dev/null || true
        fi
        rm -f "$PID_FILE"
    fi
    if [[ -f "$SERVER_PID_FILE" ]]; then
        local spid
        spid=$(cat "$SERVER_PID_FILE" 2>/dev/null || true)
        if [[ -n "$spid" ]] && kill -0 "$spid" 2>/dev/null; then
            kill "$spid" 2>/dev/null || true
        fi
        rm -f "$SERVER_PID_FILE"
    fi
    pkill -f "target/debug/examples/ui-review serve" 2>/dev/null || true
}

start_server_proc() {
    if [[ -f "$SERVER_PID_FILE" ]]; then
        local old_pid
        old_pid=$(cat "$SERVER_PID_FILE" 2>/dev/null || true)
        if [[ -n "$old_pid" ]] && kill -0 "$old_pid" 2>/dev/null; then
            kill "$old_pid" 2>/dev/null || true
            wait "$old_pid" 2>/dev/null || true
        fi
    fi
    setsid ./target/debug/examples/ui-review serve \
        --port "$PORT" \
        --state-file "$STATE_FILE" \
        --shot-file "$SHOT_FILE" </dev/null >>"$LOG_FILE" 2>&1 &
    echo $! >"$SERVER_PID_FILE"
}

watch_loop() {
    echo "[dev-screen] Building ui-review..." >>"$LOG_FILE"
    cargo build --locked -p gamenode-app --example ui-review >>"$LOG_FILE" 2>&1
    start_server_proc
    local last_hash
    last_hash=$(compute_ui_hash)
    echo "[dev-screen] Watching crates/app/ui, crates/app/examples/ui-review.rs, and i18n for hot-reload..." >>"$LOG_FILE"
    while true; do
        sleep 1
        local next_hash
        next_hash=$(compute_ui_hash)
        if [[ "$next_hash" != "$last_hash" ]]; then
            echo "[dev-screen] Change detected at $(date -Iseconds); rebuilding..." >>"$LOG_FILE"
            if cargo build --locked -p gamenode-app --example ui-review >>"$LOG_FILE" 2>&1; then
                last_hash="$next_hash"
                start_server_proc
                echo "[dev-screen] Hot-reload complete." >>"$LOG_FILE"
            else
                echo "[dev-screen] Build failed; keeping previous screen active." >>"$LOG_FILE"
                last_hash="$next_hash"
            fi
        fi
    done
}

wait_ready() {
    for _ in $(seq 1 40); do
        if curl -fsS "http://127.0.0.1:${PORT}/api/state" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.1
    done
    echo "Server did not become ready on port ${PORT}. Check ${LOG_FILE}" >&2
    return 1
}

cmd="${1:-start}"
shift || true

case "$cmd" in
    start)
        stop_all
        : >"$LOG_FILE"
        echo "Building initial ui-review binary..."
        cargo build --locked -p gamenode-app --example ui-review
        setsid bash "${BASH_SOURCE[0]}" __watch_internal </dev/null >>"$LOG_FILE" 2>&1 &
        echo $! >"$PID_FILE"
        wait_ready
        echo "Live Screen + Hot-Reload running at: http://127.0.0.1:${PORT}"
        echo "Live PNG snapshot synced at:         ${SHOT_FILE}"
        ;;
    __watch_internal)
        watch_loop
        ;;
    stop)
        stop_all
        echo "Stopped dev-screen server and watcher."
        ;;
    status|state)
        curl -fsS "http://127.0.0.1:${PORT}/api/state"
        echo
        ;;
    preset)
        preset="${1:?preset name required (e.g. library, protection, editor, settings, match, add, claim)}"
        locale_param=""
        size_param=""
        shift || true
        for arg in "$@"; do
            if [[ "$arg" == "pt_BR" || "$arg" == "en" ]]; then
                locale_param="&locale=${arg}"
            elif [[ "$arg" =~ ^[0-9]+x[0-9]+$ ]]; then
                size_param="&resize=${arg}"
            fi
        done
        curl -fsS "http://127.0.0.1:${PORT}/api/action?preset=${preset}${locale_param}${size_param}"
        echo
        ;;
    locale)
        loc="${1:?locale required (en or pt_BR)}"
        curl -fsS "http://127.0.0.1:${PORT}/api/action?locale=${loc}"
        echo
        ;;
    resize)
        sz="${1:?size WxH required (e.g. 1100x720 or 640x420)}"
        curl -fsS "http://127.0.0.1:${PORT}/api/action?resize=${sz}"
        echo
        ;;
    click)
        x="${1:?x coordinate required}"
        y="${2:?y coordinate required}"
        btn="${3:-left}"
        curl -fsS "http://127.0.0.1:${PORT}/api/action?click=${x},${y}&button=${btn}"
        echo
        ;;
    dblclick)
        x="${1:?x coordinate required}"
        y="${2:?y coordinate required}"
        curl -fsS "http://127.0.0.1:${PORT}/api/action?dblclick=${x},${y}"
        echo
        ;;
    move)
        x="${1:?x coordinate required}"
        y="${2:?y coordinate required}"
        curl -fsS "http://127.0.0.1:${PORT}/api/action?move=${x},${y}"
        echo
        ;;
    drag)
        x1="${1:?x1}"
        y1="${2:?y1}"
        x2="${3:?x2}"
        y2="${4:?y2}"
        curl -fsS "http://127.0.0.1:${PORT}/api/action?drag=${x1},${y1},${x2},${y2}"
        echo
        ;;
    scroll)
        x="${1:?x}"
        y="${2:?y}"
        dx="${3:?dx}"
        dy="${4:?dy}"
        curl -fsS "http://127.0.0.1:${PORT}/api/action?scroll=${x},${y},${dx},${dy}"
        echo
        ;;
    key)
        k="${1:?key name required (e.g. Escape, Tab, Return, DownArrow, F5)}"
        curl -fsS -G "http://127.0.0.1:${PORT}/api/action" --data-urlencode "key=${k}"
        echo
        ;;
    type)
        txt="${1:?text required}"
        curl -fsS -G "http://127.0.0.1:${PORT}/api/action" --data-urlencode "text=${txt}"
        echo
        ;;
    shot)
        out="${1:-$SHOT_FILE}"
        curl -fsS -G "http://127.0.0.1:${PORT}/api/action" --data-urlencode "shot=${out}" >/dev/null
        echo "$out"
        ;;
    *)
        echo "Usage: $0 {start|stop|status|preset <name> [en|pt_BR] [WxH]|locale <en|pt_BR>|resize <WxH>|click <x> <y>|move <x> <y>|drag <x1> <y1> <x2> <y2>|scroll <x> <y> <dx> <dy>|key <Key>|type <text>|shot [out.png]}" >&2
        exit 1
        ;;
esac
