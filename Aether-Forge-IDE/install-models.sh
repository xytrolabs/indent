#!/usr/bin/env bash
# Install Scrible AI models from HuggingFace into Ollama
# Run this ONCE after installing Ollama + Indent Forge

set -euo pipefail

echo "=== Scrible AI Model Setup ==="
echo ""

# Check Ollama is installed
if ! command -v ollama &>/dev/null; then
  echo "❌ Ollama not found. Install it first: curl -fsSL https://ollama.com/install.sh | sh"
  exit 1
fi

# Ensure Ollama is running
if ! ollama list &>/dev/null; then
  echo "Starting Ollama service..."
  ollama serve &>/dev/null &
  sleep 3
fi

install_model() {
  local name="$1"
  local url="$2"
  local temp="$3"
  local stop="$4"

  if ollama list | grep -q "$name"; then
    echo "✅ $name — already installed"
    return
  fi

  echo "📦 Installing $name from HuggingFace..."
  echo "   (this downloads ~4GB on first run — may take a few minutes)"

  ollama create "$name" -f - <<EOF
FROM $url
PARAMETER temperature $temp
PARAMETER stop "$stop"
EOF

  echo "✅ $name — installed"
}

install_model \
  "scrible-chat" \
  "https://huggingface.co/xytrolabs/scrible-chat/resolve/main/scrible-chat.gguf" \
  "0.2" \
  "<|end|>"

install_model \
  "scrible-inline" \
  "https://huggingface.co/xytrolabs/scrible-inline/resolve/main/scrible-inline.gguf" \
  "0.1" \
  "<|endoftext|>"

echo ""
echo "=== Done! Both models ready. Launch Forge to use Scrible AI. ==="
