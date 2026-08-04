# Releasing Indent

This project ships standalone runtime binaries via GitHub Releases.

## 1) Tag a release

```bash
git tag v0.1.0
git push origin v0.1.0
```

Pushing a tag matching `v*` triggers [.github/workflows/release.yml](.github/workflows/release.yml).

## 2) Build matrix

The workflow builds and uploads:

- x86_64-unknown-linux-gnu
- aarch64-unknown-linux-gnu
- x86_64-apple-darwin
- aarch64-apple-darwin
- x86_64-pc-windows-msvc

Artifacts are attached to the GitHub Release with `SHA256SUMS.txt`.

Release assets include:

- Runtime archives (`.tar.gz` / `.zip`)
- Linux system packages (`.deb` / `.rpm`)
- VS Code extensions (`.vsix` for `indent-language` and `indent-file-icons`)
- `SHA256SUMS.txt`

## 2.1) Mandatory quality gates before publish

Before GitHub Release publish happens, CI now enforces:

- Runtime unit tests (`cargo test --release` in `indent-native/`)
- Installer safety checks (`scripts/ci/verify-installer-safety.sh`)
- Smoke-test of packaged artifacts in clean runners:
	- Linux x86_64 artifact
	- Windows x86_64 artifact

Smoke tests validate that release archives actually run and can execute:

- `indent --version`
- `indent check tests/smoke.ind`
- `indent test tests`
- `indent examples/demo.ind`
- `air index`
- `indentpkg index`

Package-manager smoke tests also validate installation flows:

- `.deb` install via `apt` on Ubuntu runner
- `.rpm` install via `dnf` in Fedora container
- Post-install PATH commands (`indent`, `air`, `indentpkg`)

If any gate fails, release publishing is blocked.

## 3) End-user installers

- Unix: [scripts/install.sh](scripts/install.sh)
- Windows: [scripts/install.ps1](scripts/install.ps1)

Linux users can also install from release packages with native package managers:

- Debian/Ubuntu: `sudo apt install ./indent_<VERSION>_<ARCH>.deb`
- Fedora/RHEL: `sudo dnf install ./indent-<VERSION>-1.<ARCH>.rpm`

Safety notes:

- Installers write only to user-space locations (`~/.local/bin`, `~/.config/indent`, VS Code user settings paths).
- No admin elevation, package-manager installs, or destructive disk commands are used.

## 4) Optional signed package repositories (APT/DNF)

If you want users to install with `apt install indent` or `dnf install indent`
from your own repository (without downloading individual release files), publish
metadata and signatures alongside package artifacts.

### APT repository metadata + signatures

Prerequisites:

- `apt-ftparchive` (from `apt-utils`)
- `gpg`

Example:

```bash
mkdir -p repo/apt
cp dist/*.deb repo/apt/
cd repo/apt

apt-ftparchive packages . > Packages
gzip -kf Packages
apt-ftparchive release . > Release

gpg --batch --yes --armor --detach-sign -o Release.gpg Release
gpg --batch --yes --clearsign -o InRelease Release
```

### DNF repository metadata + signatures

Prerequisites:

- `createrepo_c`
- `gpg`

Example:

```bash
mkdir -p repo/dnf
cp dist/*.rpm repo/dnf/
createrepo_c repo/dnf

gpg --batch --yes --armor --detach-sign \
	-o repo/dnf/repodata/repomd.xml.asc \
	repo/dnf/repodata/repomd.xml
```

### Consumer setup examples

APT consumers:

```bash
curl -fsSL https://example.org/indent/KEY.gpg | sudo gpg --dearmor -o /usr/share/keyrings/indent.gpg
echo "deb [signed-by=/usr/share/keyrings/indent.gpg] https://example.org/indent/apt ./" | sudo tee /etc/apt/sources.list.d/indent.list
sudo apt update
sudo apt install indent
```

DNF consumers:

```bash
cat <<'EOF' | sudo tee /etc/yum.repos.d/indent.repo
[indent]
name=Indent
baseurl=https://example.org/indent/dnf
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://example.org/indent/KEY.gpg
EOF

sudo dnf makecache
sudo dnf install indent
```

## 5) Versioning

- Runtime crate version is in [indent-native/Cargo.toml](indent-native/Cargo.toml)
- Git tags should be `vX.Y.Z`

Keep tag and runtime version aligned.
