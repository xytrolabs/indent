#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Indent — System Package Installer (.deb / .rpm)
# =============================================================================
#
# One-command install via system package manager:
#   curl -fsSL https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install-pkg.sh | sudo bash
#
# This downloads the correct .deb or .rpm from GitHub Releases and installs it
# so your package manager tracks the installation — apt remove / dnf remove works.
# =============================================================================

REPO="xytrolabs/indent"
INDENT_VERSION="${INDENT_VERSION:-latest}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

bold()  { printf '\033[1m%s\033[0m' "$1"; }
green() { printf '\033[32m%s\033[0m' "$1"; }
red()   { printf '\033[31m%s\033[0m' "$1"; }

if [[ "$(id -u)" -ne 0 ]]; then
  red "This script must be run as root (use sudo)."
  exit 1
fi

# ---- detect package manager ----
PKG_TYPE=""
if command -v apt-get &>/dev/null; then
  PKG_TYPE="deb"
elif command -v dnf &>/dev/null; then
  PKG_TYPE="rpm"
elif command -v yum &>/dev/null; then
  PKG_TYPE="rpm"
elif command -v zypper &>/dev/null; then
  PKG_TYPE="rpm"
else
  red "Could not detect apt-get, dnf, yum, or zypper."
  echo "  Use the universal installer instead:"
  echo "  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | bash"
  exit 1
fi

# ---- detect architecture ----
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)
    DEB_ARCH="amd64"
    RPM_ARCH="x86_64"
    TARGET="x86_64-unknown-linux-gnu"
    ;;
  aarch64|arm64)
    DEB_ARCH="arm64"
    RPM_ARCH="aarch64"
    TARGET="aarch64-unknown-linux-gnu"
    ;;
  *)
    red "Unsupported architecture: $ARCH"
    exit 1
    ;;
esac

echo ""
bold "⚡ Indent Package Installer"
echo "  Package type: $(green "$PKG_TYPE")"
echo "  Architecture: $ARCH"
echo ""

# ---- resolve version ----
if [[ "$INDENT_VERSION" == "latest" ]]; then
  echo "→ Finding latest release..."
  API_URL="https://api.github.com/repos/${REPO}/releases/latest"
else
  API_URL="https://api.github.com/repos/${REPO}/releases/tags/${INDENT_VERSION}"
fi

RELEASE_JSON="$(curl --fail --silent --show-error --location \
  --connect-timeout 10 --max-time 30 --retry 3 --retry-delay 2 \
  "$API_URL")"

# ---- find download URL ----
if [[ "$PKG_TYPE" == "deb" ]]; then
  # Strip 'v' prefix from version tag
  RAW_VERSION="$(printf "%s" "$RELEASE_JSON" | grep -oE '"tag_name": *"v[^"]*"' | head -n1 | sed 's/.*"v\([^"]*\)".*/\1/')"
  if [[ -z "$RAW_VERSION" ]]; then
    red "Could not determine version from release."
    exit 1
  fi
  PKG_PATTERN="indent_${RAW_VERSION}_${DEB_ARCH}.deb"
  PKG_NAME="indent.deb"
elif [[ "$PKG_TYPE" == "rpm" ]]; then
  PKG_PATTERN="indent-.*\.${RPM_ARCH}\.rpm"
  PKG_NAME="indent.rpm"
fi

DOWNLOAD_URL="$(printf "%s" "$RELEASE_JSON" | grep -oE "https://[^\"]*${PKG_PATTERN}" | head -n1)"

if [[ -z "$DOWNLOAD_URL" ]]; then
  red "No ${PKG_TYPE} package found for ${DEB_ARCH:-$RPM_ARCH}"
  echo ""
  echo "Available assets:"
  printf "%s" "$RELEASE_JSON" | grep -oE '"browser_download_url": *"[^"]*"' | sed 's/.*"\(.*\)".*/  \1/' || true
  echo ""
  echo "Falling back to universal installer..."
  echo "  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | bash"
  exit 1
fi

# ---- download ----
echo "→ Downloading ${PKG_TYPE} package..."
curl --fail --silent --show-error --location \
  --connect-timeout 10 --max-time 120 --retry 3 --retry-delay 2 \
  -o "${TMP_DIR}/${PKG_NAME}" "$DOWNLOAD_URL"
green "✓ Downloaded"

# ---- install ----
echo "→ Installing..."
if [[ "$PKG_TYPE" == "deb" ]]; then
  apt-get update -qq
  apt-get install -y -qq "${TMP_DIR}/${PKG_NAME}" 2>&1 || {
    red "apt-get install failed. Trying dpkg fallback..."
    dpkg -i "${TMP_DIR}/${PKG_NAME}" 2>&1 || {
      apt-get install -y -f 2>&1  # fix any missing deps
      dpkg -i "${TMP_DIR}/${PKG_NAME}" 2>&1
    }
  }
elif [[ "$PKG_TYPE" == "rpm" ]]; then
  if command -v dnf &>/dev/null; then
    dnf install -y "${TMP_DIR}/${PKG_NAME}"
  elif command -v yum &>/dev/null; then
    yum install -y "${TMP_DIR}/${PKG_NAME}"
  elif command -v zypper &>/dev/null; then
    zypper install -y "${TMP_DIR}/${PKG_NAME}"
  fi
fi

# ---- verify ----
echo ""
if command -v indent &>/dev/null; then
  green "✓ Indent installed successfully!"
  indent --version
  echo ""
  echo "To remove:  sudo apt remove indent   (or dnf remove / yum remove)"
else
  red "Installation may have failed. Check the output above."
  exit 1
fi
