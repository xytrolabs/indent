#!/usr/bin/env bash
# Register .ind and .glo files with the Linux desktop
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SHARE="${SCRIPT_DIR}/../share"
SYSTEM_MODE=""

# Optional: --share-dir <path> overrides where the mime/icons/applications
# assets live (defaults to <this-script>/../share). Useful when the assets
# come from a release tarball rather than a cloned repo.
if [[ "${1:-}" == "--share-dir" ]]; then
  SHARE="${2:-$SHARE}"
  shift 2
fi
SYSTEM_MODE="${1:-}"

if [[ ! -f "${SHARE}/mime/packages/indent.xml" ]]; then
  echo "  (install-file-manager: share assets not found under ${SHARE}; skipping)" >&2
  exit 0
fi

echo "🎨 Installing Indent file manager integration..."

# Ensure directories exist
mkdir -p "${HOME}/.local/share/mime/packages"
mkdir -p "${HOME}/.local/share/icons/hicolor/scalable/mimetypes"
mkdir -p "${HOME}/.local/share/applications"

# MIME type
cp "${SHARE}/mime/packages/indent.xml" "${HOME}/.local/share/mime/packages/"
update-mime-database "${HOME}/.local/share/mime" 2>/dev/null || true
echo "  ✓ MIME types registered (.ind, .glo)"

# Icon
cp "${SHARE}/icons/hicolor/scalable/mimetypes/text-x-indent.svg" \
   "${HOME}/.local/share/icons/hicolor/scalable/mimetypes/"
cp "${SHARE}/icons/hicolor/scalable/mimetypes/text-x-indent-env.svg" \
   "${HOME}/.local/share/icons/hicolor/scalable/mimetypes/"

# Ensure hicolor has index.theme (needed for KDE/Dolphin to find fallback icons)
if [[ ! -f "${HOME}/.local/share/icons/hicolor/index.theme" ]]; then
  cat > "${HOME}/.local/share/icons/hicolor/index.theme" << 'THEMEEOF'
[Icon Theme]
Name=Hicolor
Comment=Fallback icon theme
Directories=scalable/mimetypes

[scalable/mimetypes]
Size=48
Type=Scalable
THEMEEOF
fi

gtk-update-icon-cache -f "${HOME}/.local/share/icons/hicolor" 2>/dev/null || true
echo "  ✓ Icons installed (.ind + .glo)"

# Desktop entry
cp "${SHARE}/applications/indent.desktop" "${HOME}/.local/share/applications/"
update-desktop-database "${HOME}/.local/share/applications" 2>/dev/null || true
echo "  ✓ Desktop entry installed"

# System-wide
if [ "$SYSTEM_MODE" = "--system" ]; then
    echo "Installing system-wide..."
    sudo cp "${SHARE}/mime/packages/indent.xml" /usr/share/mime/packages/
    sudo update-mime-database /usr/share/mime
    sudo cp "${SHARE}/icons/hicolor/scalable/mimetypes/text-x-indent.svg" /usr/share/icons/hicolor/scalable/mimetypes/
    sudo gtk-update-icon-cache /usr/share/icons/hicolor 2>/dev/null || true
    echo "  ✓ System-wide complete"
fi

echo ""
echo "✓ Indent file manager integration installed!"
echo "  .ind files should now show the Indent λ icon"
echo "  Log out and back in if changes aren't visible"
