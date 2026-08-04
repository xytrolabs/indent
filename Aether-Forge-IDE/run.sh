#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════
# Indent Forge IDE — Launch Script
# ═══════════════════════════════════════════════════════════════════
# Fixes Wayland/GPU rendering issues with webkit2gtk on Linux.
# Usage:
#   ./run.sh          # Development mode
#   ./run.sh --build  # Production build
# ═══════════════════════════════════════════════════════════════════
set -euo pipefail
cd "$(dirname "$0")"

# Environment fixes for Wayland + webkit2gtk GPU rendering
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export GDK_BACKEND=x11

if [[ "${1:-}" == "--build" ]]; then
    echo "⚒ Building Indent Forge..."
    cargo tauri build
else
    echo "⚒ Starting Indent Forge (dev mode)..."
    cargo tauri dev
fi
