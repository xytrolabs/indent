#!/usr/bin/env bash
#
# Generate a Homebrew formula for a release from already-built artifacts.
#
# Usage:
#   scripts/update-homebrew-formula.sh <tag> [dist-dir]
#
# <tag> is the release tag (e.g. v2.2.2). [dist-dir] defaults to "dist" and must
# contain the archives named indent-<tag>-<target>.tar.gz.
#
# Why this is a separate file instead of an inline heredoc in
# .github/workflows/release.yml: a bash heredoc terminator must sit at column 0,
# but a YAML block scalar (`run: |`) requires every line to be indented at least
# as far as the first. An inline heredoc therefore makes the entire workflow file
# invalid YAML, and GitHub refuses to run it at all ("This run likely failed
# because of a workflow file issue").
set -euo pipefail

REPO="${INDENT_REPO:-xytrolabs/indent}"
TAG="${1:-${GITHUB_REF_NAME:-}}"
DIST_DIR="${2:-dist}"

if [[ -z "$TAG" ]]; then
  echo "usage: $0 <tag> [dist-dir]" >&2
  exit 2
fi

VERSION="${TAG#v}"

sha_for() {
  local target="$1"
  local archive="${DIST_DIR}/indent-${TAG}-${target}.tar.gz"
  if [[ ! -f "$archive" ]]; then
    echo "warning: ${archive} not found; emitting a placeholder sha256" >&2
    echo "0000000000000000000000000000000000000000000000000000000000000000"
    return 0
  fi
  sha256sum "$archive" | cut -d' ' -f1
}

MACOS_ARM_SHA="$(sha_for aarch64-apple-darwin)"
MACOS_X64_SHA="$(sha_for x86_64-apple-darwin)"
LINUX_ARM_SHA="$(sha_for aarch64-unknown-linux-gnu)"
LINUX_X64_SHA="$(sha_for x86_64-unknown-linux-gnu)"

BASE="https://github.com/${REPO}/releases/download/${TAG}"
OUT="${DIST_DIR}/indent.rb"

mkdir -p "$DIST_DIR"

cat > "$OUT" <<EOF
class Indent < Formula
  desc "Simple, readable, beginner-friendly programming language"
  homepage "https://github.com/${REPO}"
  version "${VERSION}"
  license "MIT"

  on_macos do
    on_arm do
      url "${BASE}/indent-${TAG}-aarch64-apple-darwin.tar.gz"
      sha256 "${MACOS_ARM_SHA}"
    end

    on_intel do
      url "${BASE}/indent-${TAG}-x86_64-apple-darwin.tar.gz"
      sha256 "${MACOS_X64_SHA}"
    end
  end

  on_linux do
    on_arm do
      url "${BASE}/indent-${TAG}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "${LINUX_ARM_SHA}"
    end

    on_intel do
      url "${BASE}/indent-${TAG}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "${LINUX_X64_SHA}"
    end
  end

  def install
    (libexec/"bin").install "indent"
    libexec.install "std"
    libexec.install "air" if File.exist?("air")

    (bin/"indent").write_env_script libexec/"bin/indent",
      INDENT_PATH: "#{libexec}"
  end

  test do
    assert_match "indent #{version}", shell_output("#{bin}/indent --version")
  end
end
EOF

echo "Wrote ${OUT} for ${TAG}"
