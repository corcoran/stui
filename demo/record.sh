#!/usr/bin/env bash
# Usage: demo/record.sh [--only stills|hero|previews] [--shot NAME] [--keep] [--reuse]
# Env: STUI_BIN, GIF_FPS (15), GIF_QUALITY (80)
#   --keep   leave the cluster running afterwards
#   --reuse  skip up.sh when the laptop API already answers (implies --keep)
set -euo pipefail
source "$(dirname "$0")/lib.sh"
source "$DEMO/session.sh"
source "$DEMO/seed.sh"
source "$DEMO/shots.sh"

ONLY="" SHOT="" KEEP=0 REUSE=0
while [ $# -gt 0 ]; do
    case "$1" in
        --only) ONLY="$2"; shift 2 ;;
        --shot) SHOT="$2"; ONLY=stills; shift 2 ;;
        --keep) KEEP=1; shift ;;
        --reuse) REUSE=1; KEEP=1; shift ;;
        *) die "unknown flag $1" ;;
    esac
done
case "$ONLY" in ""|stills|hero|previews) ;; *) die "--only takes stills, hero or previews" ;; esac
if [ -n "$SHOT" ] && [ "$(type -t "still_$SHOT")" != function ]; then
    die "unknown shot $SHOT (one of: ${STILLS[*]})"
fi

missing=()
for t in docker kitty grim sway wf-recorder gifski exiftool magick ffmpeg jq curl; do
    command -v "$t" >/dev/null || missing+=("$t")
done
[ ${#missing[@]} -eq 0 ] || die "missing: ${missing[*]} (sudo pacman -S sway wf-recorder gifski perl-image-exiftool imagemagick jq)"

STUI_BIN="${STUI_BIN:-$REPO/target/release/stui}"
[ -x "$STUI_BIN" ] || die "no stui binary at $STUI_BIN (cargo build --release, or set STUI_BIN)"
export STUI_BIN

cleanup() {
    session_stop
    if [ "$KEEP" -eq 1 ]; then log "kept cluster running; demo/down.sh to stop"; else "$DEMO/down.sh"; fi
}
trap cleanup EXIT

if [ "$REUSE" -eq 1 ] && node_up laptop >/dev/null 2>&1; then
    log "reusing running cluster"
else
    "$DEMO/up.sh"
fi
session_start

make_gif() {
    gifski --quiet --fps "${GIF_FPS:-15}" --width 1200 --quality "${GIF_QUALITY:-80}" \
        -o "$ASSETS/$1.gif" "$RUN/$1.mp4"
}
CLIPS=()
case "$ONLY" in ""|hero) CLIPS+=(hero) ;; esac
case "$ONLY" in ""|previews) CLIPS+=(previews) ;; esac

if [ -z "$ONLY" ] || [ "$ONLY" = stills ]; then stills_pass "$SHOT"; fi
mkdir -p "$ASSETS"
if [[ " ${CLIPS[*]} " == *" hero "* ]]; then
    hero_pass
    cp "$RUN/hero.mp4" "$ASSETS/hero.mp4"
    make_gif hero
fi
if [[ " ${CLIPS[*]} " == *" previews "* ]]; then
    previews_pass
    make_gif previews
fi

log "verifying outputs"
ok=1
if [ -z "$ONLY" ] || [ "$ONLY" = stills ]; then
    for s in "${STILLS[@]}"; do
        [ -n "$SHOT" ] && [ "$s" != "$SHOT" ] && continue
        assert_png_dims "$ASSETS/$s.png" 1440 900 || ok=0
    done
fi
for c in "${CLIPS[@]}"; do
    assert_max_size "$ASSETS/$c.gif" $((8 * 1024 * 1024)) || {
        ok=0; echo "lower GIF_QUALITY (now ${GIF_QUALITY:-80}) or GIF_FPS (now ${GIF_FPS:-15})" >&2
    }
done
[ "$ok" -eq 1 ] || die "output verification failed"
log "done: $ASSETS"
