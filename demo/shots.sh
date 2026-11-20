# Shot sequences. Sourced by record.sh with lib.sh, session.sh and seed.sh loaded.
# Indexes are zero-based positions in the default A-Z sort.

FOLDER_DOCUMENTS=0   # Documents, Media, Photos, Projects, Vault
FOLDER_MEDIA=1
FOLDER_PHOTOS=2
FOLDER_PROJECTS=3
PHOTOS_CATS=0        # Cats, Kitten
CATS_HERO_IMAGE=1    # blue-blanket.jpg
DOCS_ART=0           # art, manuals, recipes, todo.txt
DOCS_MANUALS=1
DOCS_RECIPES=2
ART_ANSI=1           # smiths.ans
PROJECTS_NOTES=0     # notes, stui, website
PROJECTS_STUI=1
PROJECTS_WEBSITE=2
STUI_README=2        # src, Cargo.toml, README.md

down() { local n; for ((n=0; n<$1; n++)); do key j; done; }
top()  { key g g; }
stui_home() { key left left left; pause 0.5; top; }
to_folder() { stui_home; down "$1"; }

STILLS=(folders breadcrumbs preview-image preview-ansi out-of-sync search history)

still_folders() {
    pause 2
    shot folders
}

still_breadcrumbs() {
    to_folder "$FOLDER_PROJECTS"; key enter; pause 1.5
    down "$PROJECTS_NOTES"; key enter; pause 1.5
    shot breadcrumbs
}

still_preview-image() {
    to_folder "$FOLDER_PHOTOS"; key enter; pause 1.5
    down "$PHOTOS_CATS"; key enter; pause 1.5
    down "$CATS_HERO_IMAGE"; key enter; pause 3
    shot preview-image
}

still_preview-ansi() {
    to_folder "$FOLDER_DOCUMENTS"; key enter; pause 1.5
    down "$DOCS_ART"; key enter; pause 1.5
    down "$ART_ANSI"; key enter; pause 2.5
    shot preview-ansi
}

# Directory icons only reflect local changes once the dir has been browsed.
still_out-of-sync() {
    to_folder "$FOLDER_DOCUMENTS"; key enter; pause 1.5
    down "$DOCS_MANUALS"; key enter; pause 1.5; key left; pause 1
    top; down "$DOCS_RECIPES"; key enter; pause 1.5; key left; pause 1
    key f; pause 2.5
    shot out-of-sync
}

still_search() {
    to_folder "$FOLDER_PROJECTS"; key enter; pause 1.5
    key slash; type_text "*.md"; pause 1.5; key enter; pause 1
    down "$PROJECTS_WEBSITE"; key enter; pause 2
    shot search
}

still_history() {
    deploy_edit
    to_folder "$FOLDER_PROJECTS"; pause 1
    key u; pause 2.5
    down 3; pause 1
    shot history
}

stills_pass() {
    local only="${1:-}" s
    for s in "${STILLS[@]}"; do
        [ -n "$only" ] && [ "$s" != "$only" ] && continue
        stui_start; "still_$s"; stui_stop
    done
}

# Hero pacing: slower keys and typing than the stills, so each step reads on screen.
hkey()  { local k; for k in "$@"; do kit send-key "$k"; sleep 0.38; done; }
hdown() { local n; for ((n=0; n<$1; n++)); do hkey j; done; }
htype() { local i; for ((i=0; i<${#1}; i++)); do kit send-text -- "${1:i:1}"; sleep 0.2; done; }

hero_pass() {
    stui_start
    deploy_edit
    clip_start hero
    pause 4                                                      # folder list, Documents preview

    hdown "$FOLDER_PHOTOS"; pause 0.6; hkey enter; pause 1.5     # Photos
    hdown "$PHOTOS_CATS"; hkey enter; pause 1.5                  # Cats
    hdown "$CATS_HERO_IMAGE"; pause 0.6; hkey enter; pause 4     # cat preview
    hkey escape; pause 1.2

    hkey left left; pause 0.8                                    # folder list
    hdown $((FOLDER_PROJECTS - FOLDER_PHOTOS)); pause 0.4
    hkey enter; pause 2                                          # Projects
    hkey slash; pause 0.5; htype "*.md"; pause 2                 # search
    hkey enter; pause 1.5                                        # accept, filter stays
    hdown "$PROJECTS_WEBSITE"; pause 0.4; hkey enter; pause 3.5  # website: no build.log
    hkey left; pause 1; hkey escape; pause 2.5                   # clear search

    hkey g g; hdown "$PROJECTS_STUI"; pause 0.4
    hkey enter; pause 1.5                                        # stui/
    hdown "$STUI_README"; pause 0.6; hkey enter; pause 2.5       # README preview
    hdown 8; pause 3                                             # scroll
    hkey escape; pause 1.2

    hkey i; pause 2.5                                            # ignored + toast
    hkey i; pause 2                                              # back to synced

    hkey left left; pause 0.8; hkey g g
    hdown "$FOLDER_MEDIA"; pause 0.4; hkey enter; pause 3        # Media syncing
    hkey left; pause 1

    hdown $((FOLDER_PROJECTS - FOLDER_MEDIA)); pause 1           # Projects
    hkey u; pause 3                                              # update history
    hkey enter; pause 4                                          # jump to deploy.md
    clip_stop hero
    stui_stop
}
