#!/usr/bin/env bash
# =============================================================================
# Indent Universal Publisher
# =============================================================================
# Builds and publishes Indent to every distribution channel.
#
# Usage:
#   bash scripts/publish.sh v2.3.0          # Publish specific version
#   bash scripts/publish.sh --dry-run v2.3.0  # Dry run (show what would happen)
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
VERSION=""
DRY_RUN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --help|-h)
      echo "Usage: publish.sh [--dry-run] <version>"
      echo "  version: v2.3.0 (must match a tag)"
      exit 0
      ;;
    v*) VERSION="$1"; shift ;;
    *) echo "Unknown: $1"; exit 2 ;;
  esac
done

if [[ -z "$VERSION" ]]; then
  echo "Usage: publish.sh <version>"
  echo "Example: bash scripts/publish.sh v2.3.0"
  exit 1
fi

NUM="${VERSION#v}"
STAGE_DIR="$PROJECT_DIR/dist/indent-${VERSION}"
OUT_DIR="$PROJECT_DIR/dist/release"
mkdir -p "$STAGE_DIR" "$OUT_DIR"

run() {
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "  [DRY-RUN] $*"
  else
    echo "  → $*"
    "$@"
  fi
}

echo ""
echo "╔═══════════════════════════════════════════════════════════╗"
echo "║   INDENT v${NUM} — UNIVERSAL PUBLISHER                          ║"
echo "╚═══════════════════════════════════════════════════════════╝"
echo ""

# ── 1. Build release binary ──────────────────────────────────────────
echo "📦 Step 1: Build release binary"
run cd "$PROJECT_DIR/indent-native"
run cargo build --release
BIN="$PROJECT_DIR/indent-native/target/release/indent"
if [[ ! -f "$BIN" ]]; then
  echo "  ❌ Build failed — no binary at $BIN"
  exit 1
fi
echo "  ✅ Binary built: $(du -h "$BIN" | cut -f1)"
echo ""

# ── 2. Stage files ────────────────────────────────────────────────────
echo "📁 Step 2: Stage distribution files"
run cp "$BIN" "$STAGE_DIR/indent"
run cp -r "$PROJECT_DIR/std" "$STAGE_DIR/std"
run cp -r "$PROJECT_DIR/packages" "$STAGE_DIR/packages" 2>/dev/null || true
run cp "$PROJECT_DIR/air" "$STAGE_DIR/air" 2>/dev/null || true
run cp "$PROJECT_DIR/indentpkg" "$STAGE_DIR/indentpkg" 2>/dev/null || true
run cp "$PROJECT_DIR/README.md" "$STAGE_DIR/README.md"
run cp "$PROJECT_DIR/LICENSE" "$STAGE_DIR/LICENSE" 2>/dev/null || true

# Create install-local.sh for staged build
cat > "$STAGE_DIR/install-local.sh" << 'INSTALL'
#!/usr/bin/env bash
set -euo pipefail
INSTALL_DIR="${HOME}/.local/share/indent"
LAUNCHER_DIR="${HOME}/.local/bin"
mkdir -p "$INSTALL_DIR/bin" "$INSTALL_DIR/std" "$INSTALL_DIR/packages" "$LAUNCHER_DIR"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cp "$SCRIPT_DIR/indent" "$INSTALL_DIR/bin/indent"
cp -r "$SCRIPT_DIR/std/." "$INSTALL_DIR/std/"
cp -r "$SCRIPT_DIR/packages/." "$INSTALL_DIR/packages/" 2>/dev/null || true
cat > "$LAUNCHER_DIR/indent" << 'EOF'
#!/usr/bin/env bash
INDENT_HOME="${HOME}/.local/share/indent"
export INDENT_PATH="${INDENT_HOME}/packages:${INDENT_HOME}/std:${INDENT_HOME}:${INDENT_PATH:-}"
exec "${INDENT_HOME}/bin/indent" "$@"
EOF
chmod +x "$LAUNCHER_DIR/indent"
echo "✅ Installed to $LAUNCHER_DIR/indent"
INSTALL
chmod +x "$STAGE_DIR/install-local.sh"
echo "  ✅ Stage ready at $STAGE_DIR"
echo ""

# ── 3. Create tarball ────────────────────────────────────────────────
echo "📦 Step 3: Create distribution tarball"
TARBALL="$OUT_DIR/indent-${VERSION}-x86_64-linux.tar.gz"
run cd "$PROJECT_DIR/dist"
run tar -czf "$TARBALL" "indent-${VERSION}"
echo "  ✅ Tarball: $TARBALL ($(du -h "$TARBALL" | cut -f1))"
echo ""

# ── 4. Build .deb and .rpm ───────────────────────────────────────────
if command -v dpkg-deb &>/dev/null && command -v rpmbuild &>/dev/null; then
  echo "🐧 Step 4: Build Linux packages (.deb + .rpm)"
  run bash "$SCRIPT_DIR/package-linux-system-packages.sh" \
    "$VERSION" "x86_64-unknown-linux-gnu" "$STAGE_DIR" "$OUT_DIR"
  echo "  ✅ Packages in $OUT_DIR:"
  ls -lh "$OUT_DIR/"*.deb "$OUT_DIR/"*.rpm 2>/dev/null || echo "  (check logs)"
else
  echo "🐧 Step 4: Skip Linux packages (dpkg-deb/rpmbuild not found)"
fi
echo ""

# ── 5. Build Windows zip ─────────────────────────────────────────────
echo "🪟 Step 5: Create Windows package (ZIP)"
WIN_DIR="$PROJECT_DIR/dist/indent-${VERSION}-x86_64-windows"
mkdir -p "$WIN_DIR"
run cp "$PROJECT_DIR/indent-native/target/x86_64-pc-windows-msvc/release/indent.exe" "$WIN_DIR/" 2>/dev/null || echo "  (Windows binary not built locally — CI handles this)"
ZIP="$OUT_DIR/indent-${VERSION}-x86_64-windows.zip"
if [[ -f "$WIN_DIR/indent.exe" ]]; then
  run cd "$PROJECT_DIR/dist"
  run zip -r "$ZIP" "indent-${VERSION}-x86_64-windows"
else
  echo "  ⚠️  Skipping — Windows cross-compile not available on this machine"
fi
echo ""

# ── 6. Print instructions ────────────────────────────────────────────
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "✅ Release artifacts built for v${NUM}"
echo ""
echo "📦 To publish:"
echo ""
echo "  1. Tag the release:"
echo "     git tag -a ${VERSION} -m \"Indent ${VERSION}\""
echo "     git push origin ${VERSION}"
echo ""
echo "  2. CI does the rest automatically:"
echo "     • Builds for 5 platforms"
echo "     • Creates GitHub Release"
echo "     • Attaches all binaries"
echo ""
echo "  3. Manual steps (one-time):"
echo "     • APT: upload ${OUT_DIR}/*.deb to a PPA"
echo "     • RPM: upload ${OUT_DIR}/*.rpm to a COPR"
echo "     • Homebrew: update SHA256 in scripts/homebrew/indent.rb"
echo "     • Snap: snapcraft push (if snap package is set up)"
echo ""
echo "═══════════════════════════════════════════════════════════"
echo ""
