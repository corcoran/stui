#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
log "stopping demo syncthing"
export DEMO_UID="$(id -u)" DEMO_GID="$(id -g)"
export DEMO_TZ="$(timedatectl show -p Timezone --value 2>/dev/null || echo UTC)"
docker compose -p stui-demo -f "$DEMO/docker-compose.yml" down -v --remove-orphans
rm -rf "$RUN"
