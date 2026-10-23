#!/usr/bin/env bash
# Build GeoConveyor-0.3.1.flatpak. Needs: flatpak, flatpak-builder, and the Flathub remote
# (for org.freedesktop.Platform/Sdk 24.08):
#   flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo
#   flatpak install --user flathub org.freedesktop.Platform//24.08 org.freedesktop.Sdk//24.08
set -euo pipefail
cd "$(dirname "$0")/../.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ID=xyz.teraanalytics.GeoConveyor
[ -f target/release/geoconveyor ] || cargo build --release
W=$(mktemp -d)
cp target/release/geoconveyor "$W/geoconveyor-linux-x86_64"
cp packaging/linux/$ID.* "$W/"
cp packaging/icon-256.png packaging/icon-128.png packaging/icon-64.png packaging/geoconveyor.svg "$W/"
cd "$W"
flatpak-builder --force-clean --user --install-deps-from=flathub --repo=repo build $ID.yml
flatpak build-bundle repo "GeoConveyor-$VERSION.flatpak" $ID
cp "GeoConveyor-$VERSION.flatpak" "$OLDPWD/"
echo "Built GeoConveyor-$VERSION.flatpak"
echo "Install: flatpak install --user GeoConveyor-$VERSION.flatpak   (terrio must be installed on the host)"
