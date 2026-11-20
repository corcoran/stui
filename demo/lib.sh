#!/usr/bin/env bash
# Shared helpers for demo scripts. Source, don't execute.

DEMO="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$DEMO/.." && pwd)"
RUN="$DEMO/.run"
ASSETS="$REPO/docs/assets"

LAPTOP_URL="http://127.0.0.1:18384"
NAS_URL="http://127.0.0.1:18385"
LAPTOP_KEY="stui-demo-laptop-key"
NAS_KEY="stui-demo-nas-key"

log() { printf '\033[1;34m==>\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# assert_no_gps PATH: fails if any file under PATH carries GPS metadata, or if
# the check cannot run. With -if, exiftool exits non-zero and prints only
# "N files failed condition" on stderr when nothing matches (the clean case);
# any other stderr output is treated as an error.
assert_no_gps() {
    command -v exiftool >/dev/null 2>&1 || { echo "exiftool not found" >&2; return 1; }
    [ -e "$1" ] || { echo "no such path: $1" >&2; return 1; }
    local hits errs
    errs="$(mktemp)"
    hits="$(exiftool -r -q -if '$GPSLatitude or $GPSLongitude or $GPSPosition or $GPSCoordinates' \
        -p '$FileName' "$1" 2>"$errs")" || true
    local other
    other="$(grep -Ev '^ *[0-9]+ (image )?files? failed condition$' "$errs" || true)"
    rm -f "$errs"
    if [ -n "$other" ]; then
        printf 'exiftool error on %s:\n%s\n' "$1" "$other" >&2
        return 1
    fi
    if [ -n "$hits" ]; then
        printf 'GPS metadata found in:\n%s\n' "$hits" >&2
        return 1
    fi
}

# assert_png_dims FILE W H
assert_png_dims() {
    [ -s "$1" ] || { echo "missing or empty: $1" >&2; return 1; }
    local dims
    dims="$(magick identify -format '%w %h' "$1" 2>/dev/null)" || return 1
    [ "$dims" = "$2 $3" ] || { echo "$1 is $dims, expected $2 $3" >&2; return 1; }
}

# assert_max_size FILE BYTES
assert_max_size() {
    local size; size="$(stat -c %s "$1")"
    [ "$size" -le "$2" ] || { echo "$1 is $size bytes, cap $2" >&2; return 1; }
}

# api laptop|nas METHOD PATH [JSON]
api() {
    local url key
    case "$1" in
        laptop) url="$LAPTOP_URL"; key="$LAPTOP_KEY" ;;
        nas)    url="$NAS_URL";    key="$NAS_KEY" ;;
        *) die "unknown node $1" ;;
    esac
    curl -fsS -X "$2" -H "X-API-Key: $key" -H 'Content-Type: application/json' \
        ${4:+--data "$4"} "$url$3"
}

# wait_for TIMEOUT_S DESCRIPTION CMD...: polls CMD every second until it succeeds.
wait_for() {
    local timeout="$1" desc="$2"; shift 2
    local start=$SECONDS
    until "$@" >/dev/null 2>&1; do
        (( SECONDS - start >= timeout )) && die "timed out after ${timeout}s waiting for: $desc"
        sleep 1
    done
    log "ready: $desc"
}

node_up() { api "$1" GET /rest/system/ping; }
device_id() { api "$1" GET /rest/system/status | jq -r .myID; }

# folder_state NODE FOLDER STATE: true when folder state matches (idle, syncing, ...).
folder_state() {
    [ "$(api "$1" GET "/rest/db/status?folder=$2" | jq -r .state)" = "$3" ]
}

# folder_synced FOLDER: true when both nodes are idle on FOLDER and the laptop
# holds as many local items as the nas (at least 1). /rest/db/completion is not
# usable here: it reports 100 before the laptop has received the index.
folder_synced() {
    local n l
    folder_state nas "$1" idle || return 1
    folder_state laptop "$1" idle || return 1
    n="$(api nas GET "/rest/db/status?folder=$1" | jq -r '.localFiles + .localDirectories')"
    l="$(api laptop GET "/rest/db/status?folder=$1" | jq -r '.localFiles + .localDirectories')"
    [ "$n" -gt 0 ] && [ "$n" = "$l" ]
}

SESSION_DIR="$RUN/session"
KITTY_SOCK="$SESSION_DIR/kitty.sock"

# Env for commands aimed at the headless sway, never the host desktop.
_session_env() {
    SESSION_ENV=(-u DISPLAY -u NIRI_SOCKET -u SWAYSOCK -u KITTY_WINDOW_ID -u KITTY_PID
        -u KITTY_LISTEN_ON -u KITTY_PUBLIC_KEY XDG_RUNTIME_DIR="$SESSION_DIR"
        WAYLAND_DISPLAY="$(cat "$SESSION_DIR/display")")
}
in_session() { _session_env; env "${SESSION_ENV[@]}" "$@"; }

# in_session_bg PIDFILE CMD...: backgrounds env itself, so PIDFILE holds the
# real process. Backgrounding in_session would record a bash subshell, and
# killing that leaves CMD running.
in_session_bg() {
    local pidfile="$1"; shift
    _session_env
    env "${SESSION_ENV[@]}" "$@" &
    echo $! > "$pidfile"
}

kit() { kitty @ --to "unix:$KITTY_SOCK" "$@"; }
key() { local k; for k in "$@"; do kit send-key "$k"; sleep 0.15; done; }
type_text() { local i; for ((i=0; i<${#1}; i++)); do kit send-text -- "${1:i:1}"; sleep 0.09; done; }
pause() { sleep "$1"; }

shot() {
    mkdir -p "$ASSETS"
    in_session grim -o HEADLESS-1 "$ASSETS/$1.png"
    log "shot $1.png"
}

clip_start() {
    in_session_bg "$RUN/$1.recpid" wf-recorder -o HEADLESS-1 -r 30 -f "$RUN/$1.mp4" -y \
        >"$RUN/$1.reclog" 2>&1
    sleep 1
}
clip_stop() { kill -INT "$(cat "$RUN/$1.recpid")"; wait "$(cat "$RUN/$1.recpid")" 2>/dev/null || true; }
