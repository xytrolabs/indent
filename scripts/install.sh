#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Indent Language — Universal Installer for Linux & macOS
# =============================================================================
#
# One-command install:
#   curl -fsSL https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.sh | bash
#
# Local install (from build):
#   bash scripts/install.sh --local
#
# Specific version:
#   curl -fsSL ... | bash -s -- --version v2.3.0
# =============================================================================

DEFAULT_REPO="xytrolabs/indent"
REPO="${DEFAULT_REPO}"
LOCAL_MODE=0
INDENT_VERSION="${INDENT_VERSION:-latest}"

bold()  { printf '\033[1m%s\033[0m' "$1"; }
green() { printf '\033[32m%s\033[0m' "$1"; }
red()   { printf '\033[31m%s\033[0m' "$1"; }
warn()  { printf '\033[33m%s\033[0m' "$1" >&2; }

show_help() {
  cat <<'HELPEOF'
Indent Language Installer — Xytro Labs

Usage:
  curl -fsSL https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.sh | bash
  bash install.sh [--local] [--version VER] [--repo OWNER/REPO]

Options:
  --local           Install from local cargo build
  --version VER     Install a specific release version (default: latest)
  --repo OWNER/REPO GitHub repository for releases
  --help, -h        Show this help
HELPEOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --local) LOCAL_MODE=1 ;;
    --version) INDENT_VERSION="$2"; shift ;;
    --repo) REPO="$2"; shift ;;
    --help|-h) show_help; exit 0 ;;
    *) warn "Unknown option: $1"; show_help; exit 2 ;;
  esac
  shift
done

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
  linux)  OS_PART="unknown-linux-gnu" ;;
  darwin) OS_PART="apple-darwin" ;;
  *) red "Unsupported OS: $OS (Indent supports Linux and macOS)"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64) ARCH_PART="x86_64" ;;
  aarch64|arm64) ARCH_PART="aarch64" ;;
  *) red "Unsupported architecture: $ARCH"; exit 1 ;;
esac

TARGET="${ARCH_PART}-${OS_PART}"
CURL_OPTS=(--fail --silent --show-error --location --connect-timeout 10 --max-time 120 --retry 3 --retry-delay 2)

# ---- install paths ----
INDENT_HOME="${HOME}/.local/share/indent"
BIN_DIR="${INDENT_HOME}/bin"
STD_DIR="${INDENT_HOME}/std"
PKG_DIR="${INDENT_HOME}/packages"
LAUNCHER_DIR="${HOME}/.local/bin"
LAUNCHER="${LAUNCHER_DIR}/indent"
PROFILE_FILES=("${HOME}/.bashrc" "${HOME}/.zshrc" "${HOME}/.profile" "${HOME}/.bash_profile")

echo ""
bold "⚡ Indent Installer"
echo "  Platform: $(green "$TARGET")"
echo "  Home:     $INDENT_HOME"
echo ""

mkdir -p "$BIN_DIR" "$STD_DIR" "$PKG_DIR" "$LAUNCHER_DIR"

# ---- install binary ----
if [[ "$LOCAL_MODE" -eq 1 ]]; then
  echo "→ Installing from local build..."
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  LOCAL_BIN="${SCRIPT_DIR}/../indent-native/target/release/indent"
  if [[ ! -f "$LOCAL_BIN" ]]; then
    red "Local binary not found: $LOCAL_BIN"
    echo "  Build first: cd indent-native && cargo build --release"
    exit 1
  fi
  cp "$LOCAL_BIN" "${BIN_DIR}/indent"
  chmod +x "${BIN_DIR}/indent"
  green "✓ Installed from local build"
else
  echo "→ Fetching Indent ${INDENT_VERSION}..."
  if [[ "$INDENT_VERSION" == "latest" ]]; then
    API_URL="https://api.github.com/repos/${REPO}/releases/latest"
  else
    API_URL="https://api.github.com/repos/${REPO}/releases/tags/${INDENT_VERSION}"
  fi

  RELEASE_JSON="$(curl "${CURL_OPTS[@]}" "$API_URL" 2>/dev/null || true)"
  DOWNLOAD_URL="$(printf "%s" "$RELEASE_JSON" | grep -oE "https://[^\"]*indent-v[^\"]*-${TARGET}\\.tar\\.gz" | head -n1 || true)"

  if [[ -z "$DOWNLOAD_URL" ]]; then
    warn "No pre-built release found — building from source instead..."
    echo "  (This requires Rust. Install: https://rustup.rs)"
    echo ""

    # Check for cargo
    if ! command -v cargo >/dev/null 2>&1; then
      if [[ -f "${HOME}/.cargo/env" ]]; then
        source "${HOME}/.cargo/env" 2>/dev/null || true
      fi
      if ! command -v cargo >/dev/null 2>&1; then
        red "Rust (cargo) not found. Install it first: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        echo "  Then re-run this installer."
        exit 1
      fi
    fi

    # Clone and build from source
    BUILD_DIR="$(mktemp -d)"
    trap 'rm -rf "$BUILD_DIR"' EXIT
    echo "→ Cloning indent-native..."
    git clone --depth 1 "https://github.com/${REPO}.git" "$BUILD_DIR" 2>/dev/null || {
      red "Failed to clone ${REPO}"
      exit 1
    }
    cd "$BUILD_DIR/indent-native"
    echo "→ Building indent (release)..."
    cargo build --release 2>&1 | tail -3
    if [[ ! -f target/release/indent ]]; then
      red "Build failed. Check Rust toolchain: rustup default stable"
      exit 1
    fi
    cp target/release/indent "${BIN_DIR}/indent"
    chmod +x "${BIN_DIR}/indent"
    green "✓ Built and installed indent from source"

    # Copy companion tools from the cloned repo
    for tool in air aetherpkg; do
      if [[ -f "$BUILD_DIR/${tool}" ]]; then
        cp "$BUILD_DIR/${tool}" "${BIN_DIR}/${tool}"
        chmod +x "${BIN_DIR}/${tool}"
        cat > "${LAUNCHER_DIR}/${tool}" <<TOOLEOF
#!/usr/bin/env bash
exec "${BIN_DIR}/${tool}" "\$@"
TOOLEOF
        chmod +x "${LAUNCHER_DIR}/${tool}"
        green "✓ Installed $tool"
      fi
    done
  else

  TMP_DIR="$(mktemp -d)"
  trap 'rm -rf "$TMP_DIR"' EXIT
  curl "${CURL_OPTS[@]}" "$DOWNLOAD_URL" -o "${TMP_DIR}/indent.tar.gz"
  tar -xzf "${TMP_DIR}/indent.tar.gz" -C "$TMP_DIR"

  BIN_SRC="$(find "$TMP_DIR" -type f -name indent | head -n1)"
  if [[ -z "$BIN_SRC" ]]; then
    red "Archive does not contain 'indent' binary"
    exit 1
  fi
  cp "$BIN_SRC" "${BIN_DIR}/indent"
  chmod +x "${BIN_DIR}/indent"
  green "✓ Downloaded indent"

  # Install companion tools from the same release
  for tool in air aetherpkg; do
    TOOL_SRC="$(find "$TMP_DIR" -type f -name "$tool" | head -n1 || true)"
    if [[ -n "$TOOL_SRC" ]]; then
      cp "$TOOL_SRC" "${BIN_DIR}/${tool}"
      chmod +x "${BIN_DIR}/${tool}"
      cat > "${LAUNCHER_DIR}/${tool}" <<TOOLEOF
#!/usr/bin/env bash
exec "${BIN_DIR}/${tool}" "\$@"
TOOLEOF
      chmod +x "${LAUNCHER_DIR}/${tool}"
      green "✓ Installed $tool"
    fi
  done
fi
fi

# ---- install standard library ----
echo "→ Installing standard library..."

# Determine source for stdlib/packages:
# 1. BUILD_DIR from build-from-source fallback (cloned repo)
# 2. Local checkout (SCRIPT_DIR)
# 3. Download from GitHub
if [[ -n "${BUILD_DIR:-}" && -d "${BUILD_DIR}/std" ]]; then
  cp -r "${BUILD_DIR}/std"/* "$STD_DIR"/
  green "✓ Installed std/ from cloned repo"
  if [[ -d "${BUILD_DIR}/packages" ]]; then
    cp -r "${BUILD_DIR}/packages"/* "$PKG_DIR"/
    green "✓ Installed packages/ from cloned repo"
  fi
else
  # Safe SCRIPT_DIR that doesn't break with set -u when piped
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-}")" 2>/dev/null && pwd || pwd)"
  LOCAL_STD="${SCRIPT_DIR}/../std"
  LOCAL_PKG="${SCRIPT_DIR}/../packages"

  if [[ -d "$LOCAL_STD" ]]; then
    cp -r "$LOCAL_STD"/* "$STD_DIR"/
    green "✓ Installed std/ from local checkout"
  else
    for file in io.ind math.ind strings.ind testing.ind; do
      curl "${CURL_OPTS[@]}" "https://raw.githubusercontent.com/${REPO}/main/std/${file}" -o "${STD_DIR}/${file}" 2>/dev/null || true
    done
    green "✓ Downloaded standard library"
  fi

  if [[ -d "$LOCAL_PKG" ]]; then
    cp -r "$LOCAL_PKG"/* "$PKG_DIR"/
    green "✓ Installed packages/ from local checkout"
  fi
fi

# ---- create launcher ----
cat > "$LAUNCHER" <<'LAUNCHEREOF'
#!/usr/bin/env bash
set -euo pipefail
INDENT_HOME="${HOME}/.local/share/indent"
# Include site-packages, packages, and std in INDENT_PATH (like Python's sys.path)
SITE_PKGS="${INDENT_HOME}/site-packages"
if [[ -z "${INDENT_PATH:-}" ]]; then
  export INDENT_PATH="${SITE_PKGS}:${INDENT_HOME}/packages:${INDENT_HOME}"
else
  export INDENT_PATH="${SITE_PKGS}:${INDENT_HOME}/packages:${INDENT_HOME}:${INDENT_PATH}"
fi
exec "${INDENT_HOME}/bin/indent" "$@"
LAUNCHEREOF
chmod +x "$LAUNCHER"
green "✓ Created launcher: $LAUNCHER"

# ---- auto-configure registry for air/aetherpkg ----
CONFIG_DIR="${HOME}/.config/indent"
mkdir -p "$CONFIG_DIR"
cat > "${CONFIG_DIR}/air.env" <<CFGEOF
AIR_REGISTRY_REPO=xytrolabs/air
AIR_REGISTRY_REF=main
AIR_REGISTRY_INDEX_PATH=packages/index.txt
CFGEOF

# ---- PATH setup ----
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
bold "✓ Indent installed successfully!"
echo ""
echo "  Binary:    ${BIN_DIR}/indent"
echo "  Launcher:  ${LAUNCHER}"
echo "  Stdlib:    ${STD_DIR}"
echo "  Packages:  ${PKG_DIR}"
echo ""

if [[ ":$PATH:" != *":$LAUNCHER_DIR:"* ]]; then
  warn "⚠  Add ${LAUNCHER_DIR} to your PATH:"
  echo ""
  echo "    export PATH=\"${LAUNCHER_DIR}:\$PATH\""
  echo ""
  read -r -p "  Add automatically to shell profile? [Y/n] " answer </dev/tty 2>/dev/null || true
  if [[ -z "$answer" || "$answer" =~ ^[Yy]$ ]]; then
    for profile in "${PROFILE_FILES[@]}"; do
      if [[ -f "$profile" ]] && ! grep -q "$LAUNCHER_DIR" "$profile" 2>/dev/null; then
        printf '\n# Indent language\nexport PATH="%s:$PATH"\n' "$LAUNCHER_DIR" >> "$profile"
        green "✓ Added to $profile"
      fi
    done
  fi
else
  green "✓ PATH already configured"
fi

echo ""
echo "  Try it:   $(bold 'indent --version')"
echo "  Format:   $(bold 'indent fmt myfile.ind')"
echo "  Help:     $(bold 'indent --help')"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# ---- VS Code extension auto-install helper ----
install_vsix_asset() {
  local url="$1"
  local slug="$2"
  local label="$3"

  if [[ -z "$url" ]]; then
    echo "VSIX for $label not found in latest release; skipping auto-install."
    return 0
  fi

  local vsix_path="$TMP_DIR/${slug}.vsix"
  if ! curl "${CURL_OPTS[@]}" "$url" -o "$vsix_path"; then
    echo "Warning: failed to download $label VSIX; skipping." >&2
    return 0
  fi

  if "${VSCODE_CLI:-false}" --install-extension "$vsix_path" --force >/dev/null 2>&1; then
    echo "Installed VS Code extension: $label"
  else
    echo "Warning: could not auto-install VS Code extension: $label" >&2
  fi
}

if [[ -n "${VSCODE_CLI:-}" ]]; then
  install_vsix_asset "${LANGUAGE_VSIX_URL:-}" "indent-language" "Indent Language"
  install_vsix_asset "${ICONS_VSIX_URL:-}" "indent-file-icons" "Indent File Icons"
else
  echo "VS Code CLI not found; skipping extension auto-install."
fi

# Configure VS Code user settings so .ind files are recognized in any folder.
if command -v jq >/dev/null 2>&1; then
  VSCODE_SETTINGS_PATHS=(
    "$HOME/.config/Code/User/settings.json"
    "$HOME/.config/VSCodium/User/settings.json"
  )

  for SETTINGS_FILE in "${VSCODE_SETTINGS_PATHS[@]}"; do
    SETTINGS_DIR="$(dirname "$SETTINGS_FILE")"
    mkdir -p "$SETTINGS_DIR"
    if [[ ! -f "$SETTINGS_FILE" ]]; then
      echo "{}" > "$SETTINGS_FILE"
    fi

    TMP_SETTINGS="$(mktemp)"
    jq '
      .["files.associations"] = ((.["files.associations"] // {}) + {"*.ind": "indent"}) |
      .["workbench.iconTheme"] = (.["workbench.iconTheme"] // "indent-seti-icons")
    ' "$SETTINGS_FILE" > "$TMP_SETTINGS" && mv "$TMP_SETTINGS" "$SETTINGS_FILE"
  done

  echo "Configured VS Code settings for .ind recognition and Indent icon theme defaults."
else
  echo "Optional: install 'jq' to auto-configure VS Code .ind settings during install."
fi
