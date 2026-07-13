#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Indent Language — Build & Install
# =============================================================================
#   bash install.sh          → user-local  (~/.local/)
#   sudo bash install.sh     → system-wide (/usr/local/)
# =============================================================================

bold()  { printf '\033[1m%s\033[0m' "$1"; }
green() { printf '\033[32m%s\033[0m' "$1"; }
red()   { printf '\033[31m%s\033[0m' "$1"; }

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Detect system-wide vs user-local
if [[ "$(id -u)" -eq 0 ]]; then
  PREFIX="/usr/local"
  INDENT_HOME="${PREFIX}/share/indent"
else
  PREFIX="${HOME}/.local"
  INDENT_HOME="${PREFIX}/share/indent"
fi

BIN_DIR="${PREFIX}/bin"
STD_DIR="${INDENT_HOME}/std"
PKG_DIR="${INDENT_HOME}/packages"

echo ""
bold "⚡ Indent Installer"
echo "  Install: ${INDENT_HOME}  ($([ "$(id -u)" -eq 0 ] && echo 'system-wide' || echo 'user-local'))"
echo ""

# ── Build ─────────────────────────────────────────────────
if ! command -v cargo >/dev/null 2>&1; then
  source "${HOME}/.cargo/env" 2>/dev/null || true
  if ! command -v cargo >/dev/null 2>&1; then
    red "Rust not found. Install: https://rustup.rs"
    exit 1
  fi
fi

cd "$SCRIPT_DIR"
echo "→ Building indent (release)..."
cargo build --release
green "✓ Build complete"

# ── Install ───────────────────────────────────────────────
mkdir -p "$BIN_DIR" "$STD_DIR" "$PKG_DIR"
cp target/release/indent "$BIN_DIR/indent"
chmod +x "$BIN_DIR/indent"
green "✓ Installed: ${BIN_DIR}/indent"

# Stdlib + packages
if [[ -d "${REPO_ROOT}/std" ]]; then
  cp -r "${REPO_ROOT}/std"/* "$STD_DIR/"
  green "✓ Installed std/"
fi
if [[ -d "${REPO_ROOT}/packages" ]]; then
  cp -r "${REPO_ROOT}/packages"/* "$PKG_DIR/"
  green "✓ Installed packages/"
fi

# Companion tools
for tool in air indentpkg; do
  if [[ -f "${REPO_ROOT}/${tool}" ]]; then
    cp "${REPO_ROOT}/${tool}" "$BIN_DIR/$tool"
    chmod +x "$BIN_DIR/$tool"
    green "✓ Installed $tool"
  fi
done

echo ""
bold "✅ Indent installed!"
echo "   Run: indent --version"
echo "   REPL: indent repl"
