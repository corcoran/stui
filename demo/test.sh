#!/usr/bin/env bash
# Unit tests for demo/lib.sh. Run: demo/test.sh
set -uo pipefail
DEMO="$(cd "$(dirname "$0")" && pwd)"
source "$DEMO/lib.sh"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fails=0
check() { if "$@"; then echo "ok   $*"; else echo "FAIL $*"; fails=$((fails+1)); fi; }
refute() { if "$@" 2>/dev/null; then echo "FAIL (expected failure) $*"; fails=$((fails+1)); else echo "ok   ! $*"; fi; }

# ========================================
# assert_no_gps
# ========================================
magick -size 64x48 xc:red "$TMP/clean.jpg"
cp "$TMP/clean.jpg" "$TMP/gps.jpg"
exiftool -q -overwrite_original -GPSLatitude=40.7 -GPSLatitudeRef=N \
    -GPSLongitude=74.0 -GPSLongitudeRef=W "$TMP/gps.jpg"
mkdir -p "$TMP/tree/sub"; cp "$TMP/clean.jpg" "$TMP/tree/sub/a.jpg"
check  assert_no_gps "$TMP/clean.jpg"
refute assert_no_gps "$TMP/gps.jpg"
check  assert_no_gps "$TMP/tree"
cp "$TMP/gps.jpg" "$TMP/tree/sub/b.jpg"
refute assert_no_gps "$TMP/tree"
refute assert_no_gps "$TMP/does-not-exist"
cp "$TMP/clean.jpg" "$TMP/xmp.jpg"
exiftool -q -overwrite_original -XMP-exif:GPSLatitude=40.7 "$TMP/xmp.jpg"
refute assert_no_gps "$TMP/xmp.jpg"
refute /usr/bin/env PATH=/nonexistent /usr/bin/bash -c "source '$DEMO/lib.sh'; assert_no_gps '$TMP/clean.jpg'"

# ========================================
# assert_png_dims
# ========================================
magick -size 320x200 xc:blue "$TMP/s.png"
check  assert_png_dims "$TMP/s.png" 320 200
refute assert_png_dims "$TMP/s.png" 640 400
refute assert_png_dims "$TMP/missing.png" 320 200
: > "$TMP/empty.png"
refute assert_png_dims "$TMP/empty.png" 320 200

# ========================================
# assert_max_size
# ========================================
head -c 2048 /dev/zero > "$TMP/blob"
check  assert_max_size "$TMP/blob" 4096
refute assert_max_size "$TMP/blob" 1024

[ "$fails" -eq 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
