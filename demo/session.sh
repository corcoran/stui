#!/usr/bin/env bash
# session_start / session_stop / stui_start / stui_stop. Source after lib.sh.

session_start() {
    rm -rf "$SESSION_DIR"; mkdir -p "$SESSION_DIR"; chmod 700 "$SESSION_DIR"
    env -u DISPLAY -u WAYLAND_DISPLAY -u NIRI_SOCKET -u SWAYSOCK \
        XDG_RUNTIME_DIR="$SESSION_DIR" WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 \
        sway -c "$DEMO/sway.conf" >"$SESSION_DIR/sway.log" 2>&1 &
    echo $! > "$SESSION_DIR/sway.pid"
    wait_for 15 "headless sway" bash -c "ls \"$SESSION_DIR\"/wayland-* 2>/dev/null | grep -v lock"
    basename "$(ls "$SESSION_DIR"/wayland-* | grep -v lock | head -1)" > "$SESSION_DIR/display"
}

session_stop() {
    [ -f "$SESSION_DIR/sway.pid" ] && kill "$(cat "$SESSION_DIR/sway.pid")" 2>/dev/null || true
}

stui_start() {
    rm -f "$KITTY_SOCK"
    in_session_bg "$SESSION_DIR/kitty.pid" kitty --config "$DEMO/kitty.conf" --class stui-demo \
        --listen-on "unix:$KITTY_SOCK" \
        env XDG_CACHE_HOME="$RUN/cache" "${STUI_BIN:-$REPO/target/release/stui}" --config "$DEMO/stui.yaml" \
        >"$SESSION_DIR/kitty.log" 2>&1
    wait_for 15 "kitty socket" test -S "$KITTY_SOCK"
    sleep 3   # stui initial load
}

stui_stop() {
    kill "$(cat "$SESSION_DIR/kitty.pid")" 2>/dev/null || true
    sleep 1
}
