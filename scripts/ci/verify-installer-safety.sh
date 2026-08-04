#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

dangerous_patterns=(
  '(^|[[:space:]])sudo([[:space:]]|$)'
  'rm[[:space:]]+-rf[[:space:]]+/'
  'mkfs'
  'dd[[:space:]]+if='
  '(^|[[:space:]])shutdown([[:space:]]|$)'
  '(^|[[:space:]])reboot([[:space:]]|$)'
  '(^|[[:space:]])format([[:space:]]|$)'
)

check_file() {
  local file="$1"
  local pattern
  for pattern in "${dangerous_patterns[@]}"; do
    if grep -E -q "$pattern" "$file"; then
      echo "Unsafe pattern '$pattern' found in $file" >&2
      return 1
    fi
  done
}

check_file "scripts/install.sh"
check_file "scripts/install.ps1"

echo "Installer safety checks passed."
