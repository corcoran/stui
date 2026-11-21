#!/usr/bin/env bash
# Builds the working tree (debug profile) and runs it against the demo cluster
# with --debug. Cache and debug log stay under demo/.run/, apart from a real
# stui's ~/.cache/stui and /tmp/stui-debug.log. Extra arguments go to stui.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

node_up laptop >/dev/null 2>&1 || die "demo cluster is not running (run demo/up.sh first)"

cargo build --manifest-path "$REPO/Cargo.toml"

# stui writes its debug log to std::env::temp_dir(), which follows TMPDIR
mkdir -p "$RUN/cache" "$RUN/tmp"
log_file="$RUN/tmp/stui-debug.log"
log "debug log: $log_file"

status=0
env XDG_CACHE_HOME="$RUN/cache" TMPDIR="$RUN/tmp" \
    "$REPO/target/debug/stui" --config "$DEMO/stui.yaml" --debug "$@" || status=$?

log "debug log: $log_file"
exit "$status"
