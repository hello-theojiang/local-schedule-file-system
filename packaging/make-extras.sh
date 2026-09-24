#!/usr/bin/env bash
# Construit agenda-extras.tar.gz : entrée de menu, icônes, unités systemd, LaunchAgent.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
out=$(cd "${1:-.}" && pwd)
tmp=$(mktemp -d)
d="$tmp/agenda-extras"
mkdir -p "$d/icons"
cp "$root/packaging/common/agenda.desktop" "$d/"
cp -r "$root/packaging/common/systemd" "$root/packaging/common/launchd" "$d/"
mkdir -p "$d/vps" && cp "$root"/packaging/vps/* "$d/vps/"
i="$root/app/src-tauri/icons"
cp "$i/32x32.png" "$d/icons/32.png"
cp "$i/128x128.png" "$d/icons/128.png"
cp "$i/128x128@2x.png" "$d/icons/256.png"
cp "$i/icon.png" "$d/icons/512.png"
cp "$root/assets/icon.svg" "$d/icons/agenda.svg"
cp "$root/docs/AGENDA.md" "$d/" 2>/dev/null || true
tar -C "$tmp" -czf "$out/agenda-extras.tar.gz" agenda-extras
rm -rf "$tmp"
echo "$out/agenda-extras.tar.gz"
