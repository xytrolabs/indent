#!/usr/bin/env bash
# Build Indent Forge as an AppImage
# Prerequisites: cargo, linuxdeploy, appimagetool
set -euo pipefail
cd "$(dirname "$0")"

APP="Indent Forge"
OUTDIR="dist/appimage"
APPDIR="$OUTDIR/$APP.AppDir"

echo "Building Indent Forge AppImage..."

# Build the Tauri app in release mode
cd src-tauri
cargo build --release
cd ..

# Create AppDir structure
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/metainfo"

cp src-tauri/target/release/indent-forge "$APPDIR/usr/bin/"
cp share/icons/hicolor/256x256/apps/com.xytrolabs.IndentForge.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/"
cp share/applications/com.xytrolabs.IndentForge.desktop "$APPDIR/usr/share/applications/"
cp share/metainfo/com.xytrolabs.IndentForge.metainfo.xml "$APPDIR/usr/share/metainfo/"

# Create AppRun
cat > "$APPDIR/AppRun" << 'APPRUN'
#!/bin/bash
HERE="$(dirname "$(readlink -f "$0")")"
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export GDK_BACKEND=x11
exec "$HERE/usr/bin/indent-forge" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

# Create the AppImage (if appimagetool available)
if command -v appimagetool &>/dev/null; then
    appimagetool "$APPDIR" "$OUTDIR/Indent-Forge-x86_64.AppImage"
    echo "✅ AppImage created: $OUTDIR/Indent-Forge-x86_64.AppImage"
else
    echo "⚠ appimagetool not found. Install: wget https://github.com/AppImage/AppImageKit/releases/latest/download/appimagetool-x86_64.AppImage"
    echo "AppDir prepared at: $APPDIR"
fi
