# Demo environment

These scripts run stui against a throwaway Syncthing setup with fake data, drive it with scripted keystrokes, and capture the screen. They have two jobs:

- Regenerate the README assets in `docs/assets/` (`record.sh`)
- Check UI changes by looking at them, before and after, alongside the unit tests

Nothing here touches a real Syncthing instance or `~/.config/stui`. Everything runs in the `stui-demo` compose project on `127.0.0.1:18384` (laptop) and `127.0.0.1:18385` (nas). All runtime state lives in `demo/.run/`, and `down.sh` deletes it.

## Requirements

Linux with Docker and a Wayland-capable GPU stack. On Arch:

```
sudo pacman -S sway kitty grim wf-recorder gifski perl-image-exiftool imagemagick ffmpeg jq ttf-jetbrains-mono-nerd
```

The font in `kitty.conf` is JetBrainsMono Nerd Font Mono (`ttf-jetbrains-mono-nerd`). `record.sh` checks for the tools and prints what's missing.

## Regenerating README assets

```bash
cargo build --release      # record.sh never builds; it runs target/release/stui
demo/record.sh             # full run: cluster up, 7 stills, hero GIF, verify, cluster down
```

| Flag / env | Effect |
|---|---|
| `--only stills` / `--only hero` | Record just one pass |
| `--shot NAME` | Redo one still (`folders breadcrumbs preview-image preview-ansi out-of-sync search history`) |
| `--keep` | Leave the cluster running afterwards |
| `--reuse` | Skip `up.sh` if the laptop API already answers (implies `--keep`) |
| `STUI_BIN=path` | Record a different binary |
| `GIF_FPS`, `GIF_QUALITY` | gifski settings (defaults 15 and 80). The GIF must stay under 8 MiB |
| `DEMO_SYNCTHING_TAG=tag` | Syncthing image tag for both nodes (default `2.1.7`), read by `up.sh`. Use e.g. `2.0` to check behavior on Syncthing without folder groups |

`record.sh` checks that stills are 1440x900 and that the GIF is under the size cap. The shot list and the hero's key sequences live in `shots.sh`. The look lives in `kitty.conf` (font, colors) and `sway.conf` (resolution).

Media only trickles in for about 25 minutes after `up.sh`. Shots that should show syncing need a fresh cluster, so skip `--reuse` if the cluster is older than that.

## Poking around by hand

```bash
demo/up.sh
XDG_CACHE_HOME=demo/.run/cache stui --config demo/stui.yaml
demo/down.sh
```

## Before/after checks

Use this for any change to rendering, icons, sync states, navigation, or popups, once the unit tests pass. Save the old binary before changing code, rebuild after, then capture the same screens with both:

```bash
cp target/release/stui /tmp/stui-before     # before changing code
# ... make the change, cargo build --release ...
demo/up.sh
bash -c 'source demo/lib.sh; source demo/session.sh
  ASSETS="$RUN/shots"                       # keep ad-hoc shots out of docs/assets
  session_start
  STUI_BIN=/tmp/stui-before stui_start; key j enter; pause 1.5; shot media-before; stui_stop
  stui_start;                           key j enter; pause 1.5; shot media-after;  stui_stop
  session_stop'
# look at demo/.run/shots/media-before.png and media-after.png, then:
demo/down.sh
```

Compare for the intended change and for anything else that moved.

## Helpers

From `lib.sh` and `session.sh`, after `source`-ing both:

| Helper | Does |
|---|---|
| `session_start` / `session_stop` | Headless sway on a private `XDG_RUNTIME_DIR`. Never attaches to your desktop |
| `stui_start` / `stui_stop` | kitty running `$STUI_BIN` (default `target/release/stui`) with the demo config and cache |
| `key k1 k2 ...` | Send kitty key names (`enter`, `escape`, `down`, `left`, `j`, `g`), 0.15s apart |
| `type_text "str"` | Type characters one at a time |
| `pause SECS` | Wait |
| `shot NAME` | Screenshot to `$ASSETS/NAME.png` |
| `clip_start NAME` / `clip_stop NAME` | Record `demo/.run/NAME.mp4` |
| `api laptop\|nas METHOD PATH [JSON]` | Call a node's REST API |
| `wait_for SECS DESC CMD...` | Poll until CMD succeeds |

Run each session inside a single `bash -c`. Background processes (sway, kitty, wf-recorder) die when the calling shell exits.

## Seeded data

`demo/stui.yaml` sets `vim_mode: true`, and files sort A-Z. On Syncthing 2.1+ the laptop's folders are grouped: Personal (Photos, Vault), Work (Projects), with Documents and Media ungrouped, so the folder list shows them in that order:

| Folder | State on the laptop |
|---|---|
| Documents | Receive-only with 3 local changes (edit, add, delete). `art/` holds `gentoo.ans` and `smiths.ans` |
| Media | Six 100 MB files trickling in at 400 KiB/s: syncing and remote-only |
| Photos | Synced, with `Cats/` and `Kitten/` |
| Projects | Synced. `notes/scratch.txt` is ignored but on disk, and `website/build.log` is ignored and deleted |
| Vault | Paused |

`seed.sh` backdates files and directories over the past few weeks. `deploy_edit` changes `Projects/website/deploy.md` on the nas and waits for the laptop to receive it, which puts a fresh entry at the top of the update history.

## Gotchas

- A paused folder can't be browsed, so it gets no preview panel.
- Syncthing 2.0 accepts a PATCH of `group` with 200 and ignores it, since the field doesn't exist before 2.1.
- Timestamps use the host's timezone (`DEMO_TZ`, passed into the containers).
- `demo/stui.yaml` uses `image_protocol: auto`. The session drops the host terminal's identity variables (`KONSOLE_*`, `TERM_PROGRAM`, `TMUX`, ...) so detection sees kitty, not the terminal you launched the script from.
- Konsole can be driven too: run it in the headless sway with `QT_QPA_PLATFORM=wayland` and send keys with `wtype` (one `wtype` call per sequence, using `-s` for pauses; separate calls drop keys). `qdbus6 org.kde.konsole-<pid> /Sessions/1 getAllDisplayedText` dumps the screen text.

## Fixtures

Images and ANSI art in `fixtures/` are copied into the seed data. To add an image, drop it in and run `prepare-fixtures.sh`, which fixes orientation, caps the long edge at 1600px, and strips all metadata. Rerunning it re-encodes the JPEGs. Seeding stops if any GPS metadata survives. `test.sh` tests that check.
