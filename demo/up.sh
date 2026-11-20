#!/usr/bin/env bash
# Stands up the demo cluster from scratch. Leaves it running.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

"$DEMO/down.sh"
mkdir -p "$RUN"/{nas,laptop}/{config,data} "$RUN/cache"
export DEMO_UID="$(id -u)" DEMO_GID="$(id -g)"
export DEMO_TZ="$(timedatectl show -p Timezone --value 2>/dev/null || echo UTC)"
docker compose -p stui-demo -f "$DEMO/docker-compose.yml" up -d

wait_for 60 "nas api" node_up nas
wait_for 60 "laptop api" node_up laptop

NAS_ID="$(device_id nas)"; LAPTOP_ID="$(device_id laptop)"
export NAS_ID LAPTOP_ID

offline_opts='{"globalAnnounceEnabled":false,"localAnnounceEnabled":false,
  "relaysEnabled":false,"natEnabled":false,"urAccepted":-1,
  "crashReportingEnabled":false,"autoUpgradeIntervalH":0}'
api nas    PATCH /rest/config/options "$offline_opts" >/dev/null
api laptop PATCH /rest/config/options "$offline_opts" >/dev/null

api nas POST /rest/config/devices \
    "{\"deviceID\":\"$LAPTOP_ID\",\"name\":\"laptop\",\"addresses\":[\"tcp://laptop:22000\"]}" >/dev/null
api laptop POST /rest/config/devices \
    "{\"deviceID\":\"$NAS_ID\",\"name\":\"nas\",\"addresses\":[\"tcp://nas:22000\"]}" >/dev/null

device_connected() {
    api laptop GET /rest/system/connections | jq -e ".connections[\"$NAS_ID\"].connected" >/dev/null
}
wait_for 60 "laptop <-> nas connected" device_connected

source "$DEMO/seed.sh"
seed_all

cat > "$DEMO/stui.yaml" <<EOT
api_key: "$LAPTOP_KEY"
base_url: "$LAPTOP_URL"
vim_mode: true
icon_mode: "nerdfont"
image_protocol: "kitty"
path_map:
  "/data": "$RUN/laptop/data"
EOT
log "demo cluster up: stui --config $DEMO/stui.yaml"
