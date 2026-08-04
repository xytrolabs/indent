#!/usr/bin/env bash
set -euo pipefail

# Build Linux system packages (.deb + .rpm) from a prepared release stage directory.
#
# Usage:
#   bash scripts/package-linux-system-packages.sh <version> <target-triple> <stage-dir> <out-dir>
#
# Example:
#   bash scripts/package-linux-system-packages.sh v0.1.1 x86_64-unknown-linux-gnu \
#     dist/indent-v0.1.1-x86_64-unknown-linux-gnu dist

usage() {
  cat <<'EOF'
Usage:
  package-linux-system-packages.sh <version> <target-triple> <stage-dir> <out-dir>

Arguments:
  version        Release version (for example v0.1.1)
  target-triple  Linux target triple (x86_64-unknown-linux-gnu or aarch64-unknown-linux-gnu)
  stage-dir      Directory containing staged release files (indent, air, indentpkg, std/, README.md)
  out-dir        Output directory for generated .deb/.rpm files
EOF
}

need_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Missing required command: $cmd" >&2
    exit 1
  fi
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

if [[ $# -ne 4 ]]; then
  usage
  exit 2
fi

VERSION_RAW="$1"
TARGET_TRIPLE="$2"
STAGE_DIR="$3"
OUT_DIR="$4"

VERSION="${VERSION_RAW#v}"

case "$TARGET_TRIPLE" in
  x86_64-unknown-linux-gnu)
    DEB_ARCH="amd64"
    RPM_ARCH="x86_64"
    ;;
  aarch64-unknown-linux-gnu)
    DEB_ARCH="arm64"
    RPM_ARCH="aarch64"
    ;;
  *)
    echo "Unsupported target triple for system packaging: $TARGET_TRIPLE" >&2
    exit 1
    ;;
esac

if [[ ! -d "$STAGE_DIR" ]]; then
  echo "Stage directory not found: $STAGE_DIR" >&2
  exit 1
fi

for required_path in "$STAGE_DIR/indent" "$STAGE_DIR/air" "$STAGE_DIR/indentpkg" "$STAGE_DIR/std" "$STAGE_DIR/README.md"; do
  if [[ ! -e "$required_path" ]]; then
    echo "Missing required staged asset: $required_path" >&2
    exit 1
  fi
done

need_cmd dpkg-deb
need_cmd rpmbuild
need_cmd tar
need_cmd install
need_cmd du

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

PKGROOT="$WORK_DIR/pkgroot"
mkdir -p "$PKGROOT/usr/lib/indent/bin" "$PKGROOT/usr/lib/indent/std" "$PKGROOT/usr/bin" "$PKGROOT/usr/share/doc/indent"

install -m 0755 "$STAGE_DIR/indent" "$PKGROOT/usr/lib/indent/bin/indent-bin"
install -m 0755 "$STAGE_DIR/air" "$PKGROOT/usr/lib/indent/bin/air-bin"
install -m 0755 "$STAGE_DIR/indentpkg" "$PKGROOT/usr/lib/indent/bin/indentpkg-bin"
cp -a "$STAGE_DIR/std/." "$PKGROOT/usr/lib/indent/std/"
install -m 0644 "$STAGE_DIR/README.md" "$PKGROOT/usr/share/doc/indent/README.md"
if [[ -f "$ROOT_DIR/LICENSE" ]]; then
  install -m 0644 "$ROOT_DIR/LICENSE" "$PKGROOT/usr/share/doc/indent/LICENSE"
fi

cat > "$PKGROOT/usr/bin/indent" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
INDENT_HOME="/usr/lib/indent"
if [[ -z "${INDENT_PATH:-}" ]]; then
  export INDENT_PATH="$INDENT_HOME"
else
  export INDENT_PATH="$INDENT_HOME:${INDENT_PATH}"
fi
exec "$INDENT_HOME/bin/indent-bin" "$@"
EOF
chmod +x "$PKGROOT/usr/bin/indent"

cat > "$PKGROOT/usr/bin/air" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
INDENT_HOME="/usr/lib/indent"
if [[ ! -x "$INDENT_HOME/bin/air-bin" ]]; then
  echo "air is not available in this Indent installation." >&2
  exit 1
fi
if [[ -z "${INDENT_PATH:-}" ]]; then
  export INDENT_PATH="$INDENT_HOME"
else
  export INDENT_PATH="$INDENT_HOME:${INDENT_PATH}"
fi
exec "$INDENT_HOME/bin/air-bin" "$@"
EOF
chmod +x "$PKGROOT/usr/bin/air"

cat > "$PKGROOT/usr/bin/indentpkg" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
INDENT_HOME="/usr/lib/indent"
if [[ ! -x "$INDENT_HOME/bin/indentpkg-bin" ]]; then
  echo "indentpkg is not available in this Indent installation." >&2
  exit 1
fi
if [[ -z "${INDENT_PATH:-}" ]]; then
  export INDENT_PATH="$INDENT_HOME"
else
  export INDENT_PATH="$INDENT_HOME:${INDENT_PATH}"
fi
exec "$INDENT_HOME/bin/indentpkg-bin" "$@"
EOF
chmod +x "$PKGROOT/usr/bin/indentpkg"

cat > "$PKGROOT/usr/bin/indent-run" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exec indent "$@"
EOF
chmod +x "$PKGROOT/usr/bin/indent-run"

cat > "$PKGROOT/usr/bin/indent-debug" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exec indent --debug "$@"
EOF
chmod +x "$PKGROOT/usr/bin/indent-debug"

mkdir -p "$OUT_DIR"

# Build .deb
mkdir -p "$PKGROOT/DEBIAN"
INSTALLED_SIZE="$(du -sk "$PKGROOT/usr" | awk '{print $1}')"
cat > "$PKGROOT/DEBIAN/control" <<EOF
Package: indent
Version: $VERSION
Section: devel
Priority: optional
Architecture: $DEB_ARCH
Maintainer: Indent Maintainers <opensource@indent-lang.org>
Depends: bash, curl
Installed-Size: $INSTALLED_SIZE
Description: Indent language runtime and package tooling
 Indent is an indentation-based scripting language with a standalone native runtime,
 package tooling (air/indentpkg), and CLI helpers for run/check/test workflows.
EOF

DEB_OUTPUT="$OUT_DIR/indent_${VERSION}_${DEB_ARCH}.deb"
dpkg-deb --build "$PKGROOT" "$DEB_OUTPUT" >/dev/null

# Build .rpm
RPM_TOPDIR="$WORK_DIR/rpmbuild"
mkdir -p "$RPM_TOPDIR/BUILD" "$RPM_TOPDIR/RPMS" "$RPM_TOPDIR/SOURCES" "$RPM_TOPDIR/SPECS" "$RPM_TOPDIR/SRPMS"

PAYLOAD_ROOT="$WORK_DIR/payload/indent-root"
mkdir -p "$PAYLOAD_ROOT"
cp -a "$PKGROOT/usr" "$PAYLOAD_ROOT/"

tar -czf "$RPM_TOPDIR/SOURCES/indent-root.tar.gz" -C "$WORK_DIR/payload" indent-root

cat > "$RPM_TOPDIR/SPECS/indent.spec" <<EOF
Name: indent
Version: $VERSION
Release: 1%{?dist}
Summary: Indent language runtime and package tooling
License: MIT
URL: https://github.com/xytrolabs/indent
Source0: indent-root.tar.gz
BuildArch: $RPM_ARCH
Requires: bash, curl

%description
Indent is an indentation-based scripting language with a standalone native
runtime, package tooling (air/indentpkg), and CLI helpers.

%prep
%setup -q -n indent-root

%build

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a usr %{buildroot}/

%files
/usr/bin/indent
/usr/bin/air
/usr/bin/indentpkg
/usr/bin/indent-run
/usr/bin/indent-debug
/usr/lib/indent
%doc /usr/share/doc/indent/README.md
%doc /usr/share/doc/indent/LICENSE

%changelog
* Thu Jan 01 1970 Indent Maintainers <opensource@indent-lang.org> - ${VERSION}-1
- Automated release package
EOF

if [[ ! -f "$PKGROOT/usr/share/doc/indent/LICENSE" ]]; then
  # Keep spec valid if LICENSE is not present in repository root.
  sed -i '/^%doc \/usr\/share\/doc\/indent\/LICENSE$/d' "$RPM_TOPDIR/SPECS/indent.spec"
fi

rpmbuild -bb --quiet --define "_topdir $RPM_TOPDIR" --define "_target_cpu $RPM_ARCH" "$RPM_TOPDIR/SPECS/indent.spec"

RPM_OUTPUT="$(find "$RPM_TOPDIR/RPMS" -type f -name '*.rpm' -print -quit)"
if [[ -z "$RPM_OUTPUT" ]]; then
  echo "Failed to locate generated RPM artifact" >&2
  exit 1
fi
cp "$RPM_OUTPUT" "$OUT_DIR/"

echo "Generated packages:"
echo "  $DEB_OUTPUT"
echo "  $OUT_DIR/$(basename "$RPM_OUTPUT")"
