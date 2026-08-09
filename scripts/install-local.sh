#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Indent — Build & Install from Source
# =============================================================================
#
# Usage:
#   bash scripts/install-local.sh              # build release + install
#   bash scripts/install-local.sh --debug      # build debug + install
#   bash scripts/install-local.sh --no-build   # install pre-built binary
# =============================================================================

bold()  { printf '\033[1m%s\033[0m' "$1"; }
green() { printf '\033[32m%s\033[0m' "$1"; }
red()   { printf '\033[31m%s\033[0m' "$1"; }

BUILD_MODE="release"
NO_BUILD=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug) BUILD_MODE="debug" ;;
    --no-build) NO_BUILD=1 ;;
    --help|-h)
      echo "Usage: install-local.sh [--debug] [--no-build]"
      echo "  --debug     Build debug binary (faster compile)"
      echo "  --no-build  Skip cargo build, use existing binary"
      exit 0
      ;;
    *) red "Unknown option: $1"; exit 2 ;;
  esac
  shift
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
NATIVE_DIR="${PROJECT_DIR}/indent-native"

# ---- install paths ----
INDENT_HOME="${HOME}/.local/share/indent"
BIN_DIR="${INDENT_HOME}/bin"
STD_DIR="${INDENT_HOME}/std"
PKG_DIR="${INDENT_HOME}/packages"
LAUNCHER_DIR="${HOME}/.local/bin"
LAUNCHER="${LAUNCHER_DIR}/indent"

echo ""
bold "⚡ Indent — Build & Install from Source"
echo "  Mode: $(green "$BUILD_MODE")"
echo "  Source: $PROJECT_DIR"
echo ""

mkdir -p "$BIN_DIR" "$STD_DIR" "$PKG_DIR" "$LAUNCHER_DIR"

# ---- build ----
if [[ "$NO_BUILD" -eq 0 ]]; then
  echo "→ Building indent ($BUILD_MODE)..."
  cd "$NATIVE_DIR"

  if ! command -v cargo &>/dev/null; then
    red "Rust (cargo) is not installed."
    echo "  Install Rust:  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
  fi

  if [[ "$BUILD_MODE" == "release" ]]; then
    cargo build --release
    BIN_SRC="${NATIVE_DIR}/target/release/indent"
  else
    cargo build
    BIN_SRC="${NATIVE_DIR}/target/debug/indent"
  fi

  if [[ ! -f "$BIN_SRC" ]]; then
    red "Build failed — binary not found at: $BIN_SRC"
    exit 1
  fi
  green "✓ Build complete"
else
  BIN_SRC="${NATIVE_DIR}/target/release/indent"
  if [[ ! -f "$BIN_SRC" ]]; then
    BIN_SRC="${NATIVE_DIR}/target/debug/indent"
  fi
  if [[ ! -f "$BIN_SRC" ]]; then
    red "No pre-built binary found. Run without --no-build first."
    exit 1
  fi
fi

# ---- install binary ----
cp "$BIN_SRC" "${BIN_DIR}/indent"
chmod +x "${BIN_DIR}/indent"
green "✓ Installed indent binary"

# ---- install stdlib and packages ----
if [[ -d "${PROJECT_DIR}/std" ]]; then
  cp -r "${PROJECT_DIR}/std"/* "$STD_DIR"/
  green "✓ Installed std/"
fi
if [[ -d "${PROJECT_DIR}/packages" ]]; then
  cp -r "${PROJECT_DIR}/packages"/* "$PKG_DIR"/
  green "✓ Installed packages/"
fi

# ---- install companion scripts ----
for tool in air aetherpkg; do
  if [[ -f "${PROJECT_DIR}/${tool}" ]]; then
    cp "${PROJECT_DIR}/${tool}" "${BIN_DIR}/${tool}"
    chmod +x "${BIN_DIR}/${tool}"
    cat > "${LAUNCHER_DIR}/${tool}" <<TOOLEOF
#!/usr/bin/env bash
exec "${BIN_DIR}/${tool}" "\$@"
TOOLEOF
    chmod +x "${LAUNCHER_DIR}/${tool}"
    green "✓ Installed $tool"
  fi
done

# ---- create launcher ----
cat > "$LAUNCHER" <<'LAUNCHEREOF'
#!/usr/bin/env bash
set -euo pipefail
INDENT_HOME="${HOME}/.local/share/indent"
if [[ -z "${INDENT_PATH:-}" ]]; then
  export INDENT_PATH="${INDENT_HOME}/packages:${INDENT_HOME}/std:${INDENT_HOME}"
else
  export INDENT_PATH="${INDENT_HOME}/packages:${INDENT_HOME}/std:${INDENT_HOME}:${INDENT_PATH}"
fi
exec "${INDENT_HOME}/bin/indent" "$@"
LAUNCHEREOF
chmod +x "$LAUNCHER"
green "✓ Created launcher: $LAUNCHER"

# ---- PATH check ----
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
bold "✓ Indent installed from source!"
echo ""
echo "  Binary:    ${BIN_DIR}/indent"
echo "  Launcher:  ${LAUNCHER}"
echo "  Stdlib:    ${STD_DIR}"
echo "  Packages:  ${PKG_DIR}"
echo ""

if [[ ":$PATH:" != *":$LAUNCHER_DIR:"* ]]; then
  echo "  Add to PATH:"
  echo ""
  green "  export PATH=\"${LAUNCHER_DIR}:\$PATH\""
  echo ""
  read -r -p "  Add automatically to ~/.bashrc? [Y/n] " answer </dev/tty 2>/dev/null || true
  if [[ -z "$answer" || "$answer" =~ ^[Yy]$ ]]; then
    if ! grep -q "$LAUNCHER_DIR" "${HOME}/.bashrc" 2>/dev/null; then
      printf '\n# Indent language\nexport PATH="%s:$PATH"\n' "$LAUNCHER_DIR" >> "${HOME}/.bashrc"
      green "✓ Added to ~/.bashrc"
    fi
  fi
else
  green "✓ PATH already configured"
fi

echo ""
echo "  Try it:   $(bold 'indent --version')"
echo "  Format:   $(bold 'indent fmt myfile.ind')"
echo "  REPL:     $(bold 'indent repl')"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
