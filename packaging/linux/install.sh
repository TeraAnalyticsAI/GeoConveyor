#!/usr/bin/env bash
# Per-user install (no root):  ./install.sh        Remove:  ./install.sh --uninstall
set -euo pipefail
P="${PREFIX:-$HOME/.local}"; ID=xyz.teraanalytics.GeoConveyor; D="$(cd "$(dirname "$0")" && pwd)"
if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$P/bin/geoconveyor" "$P/share/applications/$ID.desktop" "$P/share/icons/hicolor/"*/apps/$ID.*
  echo "Removed GeoConveyor from $P"; exit 0
fi
install -Dm755 "$D/geoconveyor" "$P/bin/geoconveyor"
sed "s|^Exec=.*|Exec=$P/bin/geoconveyor|" "$D/$ID.desktop" | install -Dm644 /dev/stdin "$P/share/applications/$ID.desktop"
for n in 256 128 64; do install -Dm644 "$D/icon-$n.png" "$P/share/icons/hicolor/${n}x${n}/apps/$ID.png"; done
install -Dm644 "$D/geoconveyor.svg" "$P/share/icons/hicolor/scalable/apps/$ID.svg"
command -v update-desktop-database >/dev/null && update-desktop-database "$P/share/applications" 2>/dev/null || true
echo "Installed to $P/bin/geoconveyor"
command -v terrio >/dev/null || echo "Note: terrio not found on PATH. Install it with:  curl -LsSf https://terrio.opengeos.org/install.sh | sh   (or: pip install terrio)"
