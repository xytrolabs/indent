#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Indent — AUR Submission Helper
# =============================================================================
#
# Prepares a tarball for AUR web upload at https://aur.archlinux.org/submit/
#
# Usage:
#   bash scripts/aur-submit.sh
#
# Then upload the generated tarball at: https://aur.archlinux.org/submit/
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AUR_DIR="${SCRIPT_DIR}/aur"
OUT_DIR="${SCRIPT_DIR}/../dist"
VERSION="${1:-0.1.2}"

mkdir -p "$OUT_DIR"

echo "→ Preparing AUR submission tarball..."
echo "  Version: $VERSION"

# Verify required files exist
for f in PKGBUILD .SRCINFO; do
  if [[ ! -f "${AUR_DIR}/${f}" ]]; then
    echo "ERROR: Missing ${AUR_DIR}/${f}" >&2
    exit 1
  fi
done

# Create submission in a temp directory
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

cp "${AUR_DIR}/PKGBUILD" "$TMP_DIR/"
cp "${AUR_DIR}/.SRCINFO" "$TMP_DIR/"

# Update .SRCINFO to match PKGBUILD
cd "$TMP_DIR"
if command -v makepkg &>/dev/null; then
  makepkg --printsrcinfo > .SRCINFO 2>/dev/null || true
fi

# Create the tarball
TARBALL="${OUT_DIR}/indent-aur-${VERSION}.tar.gz"
tar -czf "$TARBALL" PKGBUILD .SRCINFO

echo ""
echo "========================================="
echo "  AUR submission tarball ready!"
echo "  $TARBALL"
echo ""
echo "  Next steps:"
echo "  1. Go to: https://aur.archlinux.org/submit/"
echo "  2. Upload: $TARBALL"
echo "  3. That's it!"
echo "========================================="

# Also print instructions for SSH-based approach
echo ""
echo "For future SSH-based updates:"
echo "  1. Generate key:  ssh-keygen -t ed25519 -C 'aur@indent'"
echo "  2. Register:      https://aur.archlinux.org/register"
echo "  3. Add key:       https://aur.archlinux.org/account"
echo "  4. Then:          git clone ssh://aur@aur.archlinux.org/indent.git"
