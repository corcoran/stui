#!/usr/bin/env bash
# Normalizes demo/fixtures images in place: applies EXIF orientation, caps the
# long edge at 1600px, and strips all metadata. Rerunning re-encodes the JPEGs.
set -euo pipefail
cd "$(dirname "$0")/fixtures"
shopt -s nullglob nocaseglob
for f in *.jpg *.jpeg *.png; do
    magick "$f" -auto-orient -resize '1600x1600>' -strip -quality 85 "$f"
    echo "prepared $f ($(du -h "$f" | cut -f1))"
done
