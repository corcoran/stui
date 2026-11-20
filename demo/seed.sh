# Seeds the demo cluster. Sourced by up.sh after devices are connected;
# expects NAS_ID and LAPTOP_ID in the environment.

NAS_DATA="$RUN/nas/data"
LAPTOP_DATA="$RUN/laptop/data"

# age FILE DAYS: backdate mtime so timestamp sorting looks lived-in.
age() { touch -d "$2 days ago" "$1"; }

write() { mkdir -p "$(dirname "$1")"; cat > "$1"; age "$1" "$2"; }

# age_dirs ROOT: gives every directory under ROOT the mtime of its newest entry.
# Deepest first, and only after all files are written: writing into a dir resets its mtime.
age_dirs() {
    local dir entry newest t best
    while IFS= read -r -d '' dir; do
        newest="" best=0
        for entry in "$dir"/* "$dir"/.[!.]*; do
            [ -e "$entry" ] || continue
            t="$(stat -c %Y "$entry")"
            if [ "$t" -gt "$best" ]; then best="$t" newest="$entry"; fi
        done
        if [ -n "$newest" ]; then touch -r "$newest" "$dir"; fi
    done < <(find "$1" -mindepth 1 -depth -type d -print0)
}

seed_photos() {
    local d="$NAS_DATA/Photos" fx="$DEMO/fixtures"
    mkdir -p "$d"/{Cats,Kitten}
    cp "$fx"/{belly-up,blue-blanket,cat-bed,nap-armchair,sunbathing-deck}.jpg "$d/Cats/"
    cp "$fx"/kitten-{string,vhs-shelf}.jpg "$d/Kitten/"
    local i=3; for f in "$d"/*/*; do age "$f" $((i += 4)); done
}

seed_projects() {
    local d="$NAS_DATA/Projects"
    mkdir -p "$d/stui/src"
    cp "$REPO"/src/logic/*.rs "$d/stui/src/"
    cp "$REPO/Cargo.toml" "$d/stui/"
    # Plain-markdown slice of the real README: its top is an HTML screenshot table.
    { echo "# stui"; echo; sed -n '/^## Features/,/^## Configuration/{/^## Configuration/!p}' "$REPO/README.md"; } \
        > "$d/stui/README.md"
    age "$d/stui/README.md" 6; age "$d/stui/Cargo.toml" 2
    local i=1; for f in "$d"/stui/src/*.rs; do age "$f" $((i++ % 20 + 1)); done
    write "$d/website/index.md" 12 <<'EOF'
# Home

Static site notes. Built with a tiny Makefile and pandoc.
EOF
    write "$d/website/deploy.md" 9 <<'EOF'
# Deploy

1. `make build`
2. rsync `public/` to the box
EOF
    write "$d/website/build.log" 2 <<'EOF'
[ok] 14 pages rendered
EOF
    write "$d/notes/ideas.md" 4 <<'EOF'
# Ideas

- keyboard macro recorder
- weekend: fix the bike
EOF
    write "$d/notes/scratch.txt" 1 <<'EOF'
temporary notes, ignore me
EOF
}

seed_documents() {
    local d="$NAS_DATA/Documents"
    write "$d/recipes/sourdough.md" 30 <<'EOF'
# Sourdough

500g flour, 350g water, 100g starter, 10g salt. Bulk 5h, shape, cold proof overnight.
EOF
    write "$d/recipes/chili.md" 21 <<'EOF'
# Chili

Brown the beef, add onions, beans, tomatoes, cumin. Simmer two hours.
EOF
    write "$d/manuals/router-setup.txt" 40 <<'EOF'
Admin page: 192.168.1.1. Change default password before anything else.
EOF
    write "$d/todo.txt" 3 <<'EOF'
- renew passport
- book dentist
EOF
    mkdir -p "$d/art"
    cp "$DEMO/fixtures/gentoo.ans" "$DEMO/fixtures/smiths.ans" "$d/art/"
    age "$d/art/gentoo.ans" 50; age "$d/art/smiths.ans" 48
}

seed_vault() {
    local d="$NAS_DATA/Vault"
    write "$d/2019/taxes-checklist.txt" 400 <<'EOF'
W-2, 1099s, receipts folder
EOF
    write "$d/2020/move-inventory.txt" 300 <<'EOF'
14 boxes, 1 couch, too many cables
EOF
}

seed_media() {
    local d="$NAS_DATA/Media"; mkdir -p "$d"
    local n
    for n in concert-2024 drone-coast family-reunion timelapse-garden talk-recording ski-trip; do
        head -c 100M /dev/urandom > "$d/$n.mkv"; age "$d/$n.mkv" 1
    done
}

# add_folder NODE ID TYPE PEER_ID [PAUSED]
add_folder() {
    local label="${2^}"
    api "$1" POST /rest/config/folders "$(jq -nc \
        --arg id "$2" --arg label "$label" --arg path "/data/$label" \
        --arg type "$3" --arg peer "$4" --argjson paused "${5:-false}" \
        '{id:$id,label:$label,path:$path,type:$type,paused:$paused,
          devices:[{deviceID:$peer}],fsWatcherEnabled:true,rescanIntervalS:3600}')" >/dev/null
}

seed_all() {
    log "writing seed files on nas"
    seed_photos; seed_projects; seed_documents; seed_vault
    age_dirs "$NAS_DATA"
    mkdir -p "$NAS_DATA/Media" "$LAPTOP_DATA"/{Photos,Projects,Documents,Media,Vault}
    assert_no_gps "$NAS_DATA" || die "refusing to continue: GPS metadata in seed data"

    local f
    for f in photos projects documents media vault; do
        add_folder nas "$f" sendreceive "$LAPTOP_ID"
    done
    for f in photos projects media vault; do
        add_folder laptop "$f" sendreceive "$NAS_ID"
    done
    add_folder laptop documents receiveonly "$NAS_ID"

    for f in photos projects documents vault; do
        wait_for 180 "laptop has $f" folder_synced "$f"
    done

    log "applying post-sync states"
    # Ignored: scratch.txt stays on disk (warning icon); build.log is removed (ignored-deleted icon).
    api laptop POST "/rest/db/ignores?folder=projects" \
        '{"ignore":["notes/scratch.txt","website/build.log"]}' >/dev/null
    rm -f "$LAPTOP_DATA/Projects/website/build.log"

    # Local changes in the receive-only folder.
    echo "- call mom" >> "$LAPTOP_DATA/Documents/todo.txt"
    printf '# Pancakes\n\nFlour, milk, eggs. Rest the batter.\n' > "$LAPTOP_DATA/Documents/recipes/pancakes.md"
    rm -f "$LAPTOP_DATA/Documents/manuals/router-setup.txt"
    api laptop POST "/rest/db/scan?folder=documents" >/dev/null
    api laptop POST "/rest/db/scan?folder=projects" >/dev/null

    api laptop PATCH /rest/config/folders/vault '{"paused":true}' >/dev/null
    # Folder groups exist from Syncthing 2.1; older versions ignore these
    api laptop PATCH /rest/config/folders/photos '{"group":"Personal"}' >/dev/null
    api laptop PATCH /rest/config/folders/vault '{"group":"Personal"}' >/dev/null
    api laptop PATCH /rest/config/folders/projects '{"group":"Work"}' >/dev/null

    # limitBandwidthInLan is required: docker bridge peers count as LAN and are unthrottled otherwise.
    # Media trickles in slowly so syncing and remote-only states last through recording.
    api laptop PATCH /rest/config/options '{"maxRecvKbps":400,"limitBandwidthInLan":true}' >/dev/null
    # A small pending-request window keeps the throttled Media pull from queueing ~80s of
    # requests ahead of other folders, so later edits elsewhere arrive in seconds.
    api laptop PATCH /rest/config/folders/media '{"pullerMaxPendingKiB":256}' >/dev/null
    seed_media
    api nas POST "/rest/db/scan?folder=media" >/dev/null
    wait_for 120 "laptop media syncing" folder_state laptop media syncing
}

# deploy_edit: a fresh NAS-side edit, so it tops the Projects update history
# and the activity feed. Call after stui has started.
deploy_edit() {
    local f="$NAS_DATA/Projects/website/deploy.md"
    echo "3. purge the CDN cache" >> "$f"
    touch "$f"
    api nas POST "/rest/db/scan?folder=projects" >/dev/null
    wait_for 120 "laptop has deploy.md edit" deploy_synced
}

deploy_synced() {
    local j
    j="$(api laptop GET "/rest/db/file?folder=projects&file=website/deploy.md")"
    jq -e '.local.version == .global.version' <<<"$j" >/dev/null || return 1
    [ "$(jq -r .global.size <<<"$j")" = "$(stat -c %s "$NAS_DATA/Projects/website/deploy.md")" ]
}
