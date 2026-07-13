#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Create a ready-to-run Angela moderation puzzle bot script from the template.

Usage:
  bash scripts/new-angela.sh [output-file] [token] [guild-id] [audit-channel-id] [--force]

Defaults:
  output-file       examples/angela_mod_puzzle.local.ind
  token             $INDENT_DISCORD_TOKEN or YOUR_BOT_TOKEN
  guild-id          $INDENT_DISCORD_GUILD_ID or YOUR_GUILD_ID
  audit-channel-id  $INDENT_DISCORD_AUDIT_CHANNEL_ID or YOUR_AUDIT_CHANNEL_ID

Examples:
  bash scripts/new-angela.sh
  bash scripts/new-angela.sh examples/my_angela.ind "YOUR_TOKEN" "123456789012345678"
  INDENT_DISCORD_TOKEN="YOUR_TOKEN" INDENT_DISCORD_GUILD_ID="123" bash scripts/new-angela.sh
EOF
}

FORCE=0
POSITIONAL=()
for arg in "$@"; do
  case "$arg" in
    --force)
      FORCE=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      POSITIONAL+=("$arg")
      ;;
  esac
done
set -- "${POSITIONAL[@]}"

OUTPUT_FILE="${1:-examples/angela_mod_puzzle.local.ind}"
TOKEN="${2:-${INDENT_DISCORD_TOKEN:-YOUR_BOT_TOKEN}}"
GUILD_ID="${3:-${INDENT_DISCORD_GUILD_ID:-YOUR_GUILD_ID}}"
AUDIT_CHANNEL_ID="${4:-${INDENT_DISCORD_AUDIT_CHANNEL_ID:-YOUR_AUDIT_CHANNEL_ID}}"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE_FILE="$ROOT_DIR/examples/angela_mod_puzzle.ind"

if [[ ! -f "$TEMPLATE_FILE" ]]; then
  echo "Template not found: $TEMPLATE_FILE" >&2
  exit 1
fi

if [[ -e "$OUTPUT_FILE" && "$FORCE" -ne 1 ]]; then
  echo "Output file already exists: $OUTPUT_FILE" >&2
  echo "Use --force to overwrite." >&2
  exit 1
fi

OUTPUT_DIR="$(dirname "$OUTPUT_FILE")"
mkdir -p "$OUTPUT_DIR"

escape_awk_replacement() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//&/\\&}"
  printf '%s' "$value"
}

TOKEN_SAFE="$(escape_awk_replacement "$TOKEN")"
GUILD_SAFE="$(escape_awk_replacement "$GUILD_ID")"
AUDIT_SAFE="$(escape_awk_replacement "$AUDIT_CHANNEL_ID")"

awk \
  -v token="$TOKEN_SAFE" \
  -v guild="$GUILD_SAFE" \
  -v audit="$AUDIT_SAFE" \
  '{
    gsub(/YOUR_BOT_TOKEN/, token)
    gsub(/YOUR_GUILD_ID/, guild)
    gsub(/YOUR_AUDIT_CHANNEL_ID/, audit)
    print
  }' \
  "$TEMPLATE_FILE" > "$OUTPUT_FILE"

echo "Created Angela script: $OUTPUT_FILE"
echo "Run with: indent $OUTPUT_FILE"
