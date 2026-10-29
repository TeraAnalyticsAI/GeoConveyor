#!/usr/bin/env bash
# Per-user install (no root):  ./install.sh        Remove:  ./install.sh --uninstall
# Run it next to the release binary (geoconveyor or geoconveyor-linux-x86_64) and the
# desktop/icon files; the release archive ships them together.
set -euo pipefail
P="${PREFIX:-$HOME/.local}"; ID=xyz.teraanalytics.GeoConveyor; D="$(cd "$(dirname "$0")" && pwd)"
if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$P/bin/geoconveyor" "$P/share/applications/$ID.desktop"
  find "$P/share/icons/hicolor" -path "*/apps/$ID.*" -delete 2>/dev/null || true
  echo "Removed GeoConveyor from $P"; exit 0
fi
BIN="${BIN:-}"
if [ -z "$BIN" ]; then
  for c in geoconveyor geoconveyor-linux-x86_64; do
    if [ -f "$D/$c" ]; then BIN="$D/$c"; break; fi
  done
fi
[ -n "$BIN" ] || { echo "Error: no geoconveyor binary next to install.sh" >&2; exit 1; }
install -Dm755 "$BIN" "$P/bin/geoconveyor"
sed "s|^Exec=.*|Exec=$P/bin/geoconveyor|" "$D/$ID.desktop" | install -Dm644 /dev/stdin "$P/share/applications/$ID.desktop"
for n in 256 128 64; do install -Dm644 "$D/icon-$n.png" "$P/share/icons/hicolor/${n}x${n}/apps/$ID.png"; done
install -Dm644 "$D/geoconveyor.svg" "$P/share/icons/hicolor/scalable/apps/$ID.svg"
command -v update-desktop-database >/dev/null && update-desktop-database "$P/share/applications" 2>/dev/null || true
echo "Installed to $P/bin/geoconveyor"
command -v terrio >/dev/null || echo "Note: terrio not found on PATH. Install it with:  curl -LsSf https://terrio.opengeos.org/install.sh | sh   (or: pip install terrio)"
