#!/usr/bin/env bash
set -euo pipefail

INDEX_FILE="${1:-}"
MODE="${2:-validate}"

if [[ -z "$INDEX_FILE" ]]; then
  if [[ -f "index.txt" ]]; then
    INDEX_FILE="index.txt"
  elif [[ -f "packages/index.txt" ]]; then
    INDEX_FILE="packages/index.txt"
  else
    echo "Registry index not found. Expected index.txt or packages/index.txt" >&2
    exit 1
  fi
fi

if [[ ! -f "$INDEX_FILE" ]]; then
  echo "Registry index file not found: $INDEX_FILE" >&2
  exit 1
fi

INDEX_DIR="$(cd "$(dirname "$INDEX_FILE")" && pwd)"
ERRORS=0
LINE_NO=0

trim() {
  local s="$1"
  s="${s#"${s%%[![:space:]]*}"}"
  s="${s%"${s##*[![:space:]]}"}"
  printf "%s" "$s"
}

declare -A SEEN_NAMES
LOCAL_PATHS=()

auto_source_path() {
  local source="$1"
  if [[ "$source" =~ ^https?:// ]]; then
    printf "%s" "$source"
    return
  fi
  if [[ "$source" = /* ]]; then
    printf "%s" "$source"
    return
  fi
  source="${source#./}"
  printf "%s/%s" "$INDEX_DIR" "$source"
}

while IFS= read -r raw || [[ -n "$raw" ]]; do
  LINE_NO=$((LINE_NO + 1))
  line="$(trim "$raw")"

  if [[ -z "$line" ]]; then
    continue
  fi

  if [[ "$line" =~ ^# ]]; then
    continue
  fi

  IFS='|' read -r name source desc <<< "$line"
  name="$(trim "${name:-}")"
  source="$(trim "${source:-}")"
  desc="$(trim "${desc:-}")"

  if [[ -z "$name" || -z "$source" || -z "$desc" ]]; then
    echo "Index error at line $LINE_NO: expected 'name|source|description'" >&2
    ERRORS=$((ERRORS + 1))
    continue
  fi

  if [[ ! "$name" =~ ^[a-zA-Z_][a-zA-Z0-9_]*$ ]]; then
    echo "Index error at line $LINE_NO: invalid package name '$name'" >&2
    ERRORS=$((ERRORS + 1))
  fi

  if [[ -n "${SEEN_NAMES[$name]:-}" ]]; then
    echo "Index error at line $LINE_NO: duplicate package '$name'" >&2
    ERRORS=$((ERRORS + 1))
  else
    SEEN_NAMES[$name]=1
  fi

  resolved="$(auto_source_path "$source")"
  if [[ ! "$source" =~ ^https?:// ]]; then
    if [[ ! -f "$resolved" ]]; then
      echo "Index error at line $LINE_NO: source file not found '$source' (resolved: $resolved)" >&2
      ERRORS=$((ERRORS + 1))
    else
      LOCAL_PATHS+=("$resolved")
    fi
  fi
done < "$INDEX_FILE"

if [[ $ERRORS -gt 0 ]]; then
  echo "Registry validation failed with $ERRORS issue(s)." >&2
  exit 1
fi

echo "Registry validation passed: $INDEX_FILE"

if [[ "$MODE" == "--list-local-sources" ]]; then
  declare -A LOCAL_SEEN
  for p in "${LOCAL_PATHS[@]}"; do
    if [[ -z "${LOCAL_SEEN[$p]:-}" ]]; then
      LOCAL_SEEN[$p]=1
      echo "$p"
    fi
  done
fi
