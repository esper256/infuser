#!/bin/sh
set -eu
# Package an already-built Rust executable. No compiler/runtime SDK download is
# needed here; the software center installs the standard runtime if necessary.
binary=$1
output=$2
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT HUP INT TERM
mkdir -p "$stage/build/files/bin" "$stage/build/files/share/applications" "$stage/build/files/share/metainfo" "$stage/build/files/share/icons/hicolor/256x256/apps"
install -m755 "$binary" "$stage/build/files/bin/infuser-desktop"
cp "$repo_root/desktop/packaging/io.github.esper256.Infuser.desktop" "$stage/build/files/share/applications/"
cp "$repo_root/desktop/packaging/io.github.esper256.Infuser.metainfo.xml" "$stage/build/files/share/metainfo/"
cp "$repo_root/assets/branding/infuser/icon-256.png" "$stage/build/files/share/icons/hicolor/256x256/apps/io.github.esper256.Infuser.png"
cat > "$stage/build/metadata" <<'EOF'
[Application]
name=io.github.esper256.Infuser
runtime=org.freedesktop.Platform/x86_64/25.08
sdk=org.freedesktop.Sdk/x86_64/25.08
command=infuser-desktop
EOF
flatpak build-finish --share=network --share=ipc --socket=wayland --socket=fallback-x11 --socket=pulseaudio --device=dri "$stage/build"
flatpak build-export --arch=x86_64 "$stage/repo" "$stage/build" stable
flatpak build-bundle --arch=x86_64 --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo "$stage/repo" "$output" io.github.esper256.Infuser stable
