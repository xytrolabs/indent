#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
  linux) OS_PART="unknown-linux-gnu" ;;
  darwin) OS_PART="apple-darwin" ;;
  *)
    echo "Unsupported OS: $OS" >&2
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64|amd64) ARCH_PART="x86_64" ;;
  aarch64|arm64) ARCH_PART="aarch64" ;;
  *)
    echo "Unsupported architecture: $ARCH" >&2
    exit 1
    ;;
esac

TARGET="${ARCH_PART}-${OS_PART}"

VERSION="${1:-local}"
OUT_DIR="${ROOT_DIR}/dist/friend"
STAGE_DIR="${OUT_DIR}/indent-${VERSION}-${TARGET}"
ARCHIVE_PATH="${OUT_DIR}/indent-${VERSION}-${TARGET}.tar.gz"

mkdir -p "$OUT_DIR"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"

echo "Building Indent for target: $TARGET"
(
  cd "${ROOT_DIR}/indent-native"
  cargo build --release --target "$TARGET"
)

cp "${ROOT_DIR}/indent-native/target/${TARGET}/release/indent" "${STAGE_DIR}/indent"
cp "${ROOT_DIR}/scripts/install-local.sh" "${STAGE_DIR}/install-local.sh"
cp -r "${ROOT_DIR}/std" "${STAGE_DIR}/std"
cp "${ROOT_DIR}/README.md" "${STAGE_DIR}/README.md"

chmod +x "${STAGE_DIR}/indent"
chmod +x "${STAGE_DIR}/install-local.sh"

tar -czf "$ARCHIVE_PATH" -C "$OUT_DIR" "indent-${VERSION}-${TARGET}"

echo
echo "Share this file with your friend:"
echo "  $ARCHIVE_PATH"
echo
echo "Friend install commands:"
echo "  tar -xzf $(basename "$ARCHIVE_PATH")"
echo "  cd indent-${VERSION}-${TARGET}"
echo "  bash install-local.sh"
