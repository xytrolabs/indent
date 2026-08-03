# Indent Language (.ind)

Indent is a simple, readable, beginner-friendly programming language (v2.3.0). It uses indentation-based blocks, lowercase keywords, and minimal punctuation — designed to be easy to learn while powerful enough for real work.

## Quick Install

**Pick your platform:**

### 🐧 Debian / Ubuntu (apt)
```bash
curl -fsSL https://raw.githubusercontent.com/xytro-labs/indent/main/scripts/install-pkg.sh | sudo bash
```
Then `sudo apt remove indent` to uninstall.

### 🎩 Fedora / RHEL (dnf)
```bash
curl -fsSL https://raw.githubusercontent.com/xytro-labs/indent/main/scripts/install-pkg.sh | sudo bash
```
The script auto-detects dnf/yum and installs the `.rpm`.

### 🟣 Arch Linux (AUR)
```bash
yay -S indent
# or:  paru -S indent
# or:  git clone https://aur.archlinux.org/indent.git && cd indent && makepkg -si
```

### 🍎 macOS (Homebrew)
```bash
brew install xytro-labs/indent/indent
```

### 🪟 Windows (PowerShell)
```powershell
powershell -c "irm https://raw.githubusercontent.com/xytro-labs/indent/main/scripts/install.ps1 | iex"
```

### 🌐 Any Linux (universal installer)
```bash
curl -fsSL https://raw.githubusercontent.com/xytro-labs/indent/main/scripts/install.sh | bash
```
Installs to `~/.local/` — no root needed.

## Quickstart

```indent
var name string = ask "What is your name? "
say "Hello " + name + "!"
```

📖 **Full tutorial:** [`docs/learn/01-quickstart.md`](docs/learn/01-quickstart.md)

## Features

- **Output:** `say expression`
- **Variables:** `var name type = value` with `is` reassignment
- **Functions:** `fun name param1 param2` with `give` returns
- **Imports:** `get module`, `get func from module`, `get module as alias`
- **Branching:** `if` / `or` / `otherwise` (no colons)
- **Loops:** `repeat N`, `repeat item in list`, `repeat until condition`
- **Types:** `string`, `int`, `float`, `boolean`, `dynamic`, `empty`
- **Builtins:** `ask`, `len`, `assert`, `range`, `split`, `join`, math, file I/O, JSON, HTTP, WebSocket
- **Tooling:** `indent fmt`, `indent test`, `indent repl`, `indent --debug`
- **Tooling:** `indent fmt`, `indent test`, `indent repl`, `indent --debug`

## Run Indent

```bash
# Run a script
indent examples/demo.ind

# Run with debugger
indent --debug myfile.ind

# Run tests
indent test tests/

# Format code
indent fmt myfile.ind

# Interactive REPL
indent repl

# Check syntax
indent check myfile.ind

# Create a new project
indent new my-project
```

## Build from Source

```bash
cd indent-native
cargo build --release
./target/release/indent --version
```

## Documentation

| Document | Description |
|----------|-------------|
| [`docs/learn/01-quickstart.md`](docs/learn/01-quickstart.md) | 15-minute getting started |
| [`docs/learn/01-hello-and-vars.md`](docs/learn/01-hello-and-vars.md) | Hello World & Variables |
| [`docs/learn/02-functions-and-types.md`](docs/learn/02-functions-and-types.md) | Functions, branching, loops |
| [`docs/learn/03-testing.md`](docs/learn/03-testing.md) | Testing, modules, project structure |
| [`docs/learn/04-lists-and-dictionaries.md`](docs/learn/04-lists-and-dictionaries.md) | Lists & Dictionaries |
| [`docs/learn/05-strings-and-builtins.md`](docs/learn/05-strings-and-builtins.md) | Strings & Built-in Functions |
| [`docs/learn/06-error-handling.md`](docs/learn/06-error-handling.md) | Error Handling (do/catch/flag) |
| [`docs/learn/07-file-io-and-os.md`](docs/learn/07-file-io-and-os.md) | File I/O & OS Operations |
| [`docs/learn/08-advanced-functions.md`](docs/learn/08-advanced-functions.md) | Advanced Functions & Recursion |
| [`docs/learn/09-data-and-json.md`](docs/learn/09-data-and-json.md) | JSON, HTTP, Time & Random |
| [`docs/INDENT_GUIDE.md`](docs/INDENT_GUIDE.md) | Full language reference |
| [`docs/quick-reference.md`](docs/quick-reference.md) | Syntax cheat sheet |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | How to contribute |
| [`RELEASING.md`](RELEASING.md) | Release process |

## License

MIT

```bash
indent new my-app
cd my-app
indent main.ind
indent test tests
```

## Distribute Indent

Indent is distributed as a standalone binary runtime for each supported OS.

### Quick Share With A Friend (No GitHub Required)

If you want one friend to try Indent quickly, build a shareable archive from your machine:

```bash
bash scripts/package-for-friend.sh v0.1.0
```

This creates an archive in `dist/friend/`.

Your friend installs with:

```bash
tar -xzf indent-v0.1.0-<target>.tar.gz
cd indent-v0.1.0-<target>
bash install-local.sh
```

After install:

```bash
indent --help
```

The local installer also sets up a launcher that includes bundled `std/` module lookup via `INDENT_PATH`.

### GitHub Release Artifacts

Release automation is defined in `.github/workflows/release.yml` and builds:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

Each tagged release (`v*`) publishes archives, Linux packages (`.deb`/`.rpm`), and `SHA256SUMS.txt`.

### Install (Unix)

```bash
bash scripts/install.sh
# optional override:
# bash scripts/install.sh owner/repo
```

The installer auto-configures `air`/`indentpkg` to use the same registry repo, so users can run `air install <package>` immediately without manual registry setup.

Unix installer global commands:

- `indent`
- `air`
- `indentpkg`
- `indent-run`
- `indent-debug`

### Install With Linux Package Managers (.deb/.rpm)

GitHub release assets now include Linux system packages.

Debian/Ubuntu (APT):

```bash
sudo apt install ./indent_<VERSION>_<ARCH>.deb
```

RHEL/Fedora (DNF):

```bash
sudo dnf install ./indent-<VERSION>-1.<ARCH>.rpm
```

Generic RPM install:

```bash
sudo rpm -Uvh ./indent-<VERSION>-1.<ARCH>.rpm
```

Examples:

- `indent_0.1.1_amd64.deb`
- `indent_0.1.1_arm64.deb`
- `indent-0.1.1-1.x86_64.rpm`
- `indent-0.1.1-1.aarch64.rpm`

These packages install command shims into `/usr/bin`, so `indent`, `air`, and `indentpkg` are on PATH right after install.

If you want to publish signed apt/dnf repositories (instead of only single-file package installs), see `RELEASING.md` section `4) Optional signed package repositories (APT/DNF)`.

If `code` (or `codium`) CLI is available, the installer also auto-installs the Indent VS Code extensions from the latest release VSIX assets.

### Install (Windows PowerShell)

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1
# optional override:
# powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1 -Repo owner/repo
```

The installer also auto-configures `air`/`indentpkg` registry settings for the selected repo.

If VS Code CLI is available (`code`/`codium`), the installer auto-installs the Indent language and icon extensions from release VSIX assets.

After install on Windows, you can use:

- `indent`
- `air`
- `indentpkg`
- `indent-run`
- `indent-debug`

The installer creates command shims (`air.cmd`, `indentpkg.cmd`) in `%USERPROFILE%\\.local\\bin`.

### Releasing

See `RELEASING.md` for the full process.

### Release Safety And Compatibility

To reduce risk for end users, GitHub release publishing is now gated by automated checks:

- Runtime unit tests (`cargo test --release`)
- Installer safety checks (no admin elevation/destructive disk commands)
- Artifact smoke tests on clean Linux and Windows runners
  - `indent --version`
  - `indent check tests/smoke.ind`
  - `indent test tests`
  - `indent examples/demo.ind`
  - `air index`
  - `indentpkg index`
- VS Code extension packaging and publication as release assets (`.vsix`)

You can run the same readiness checks locally before tagging:

```bash
./scripts/ci/release-readiness.sh
```

Global command usage:

```bash
~/.local/bin/indent examples/demo.ind
```

If `~/.local/bin` is in your `PATH`, you can run:

```bash
indent examples/demo.ind
```

## VS Code Run Integration

This workspace includes ready-to-use VS Code run configs:

- Task: `Run Current Indent File`
- Task: `Lint Current Indent File`
- Task: `Check Current Indent File`
- Task: `Format Current Indent File`
- Task: `Run Indent Tests`
- Task: `Indent REPL`
- Launch configuration: `Debug Current Indent File`
- Launch configuration: `Run Current Indent File`

Language extension command:

- `Indent: New Project` (Command Palette)

From VS Code you can:

- Press `F5` and choose `Run Current Indent File`
- Or run the task from `Terminal -> Run Task`

The workspace calls the installed `indent` command.

Global (any workspace) via extension commands:

- `Indent: Run Current File`
- `Indent: Debug Current File`
- `Indent: Configure Run/Debug For Workspace`

## VS Code Language Support

This workspace includes a local Indent language extension (`indent-language/`) that provides:

- Syntax highlighting for `.ind`
- Language configuration (comments/brackets/autoclose)
- Snippets for common Indent patterns

It is installed locally under `~/.vscode/extensions/indent-local.indent-language-0.0.1`.

Packaging for CI is configured in `.github/workflows/vscode-extensions.yml`.
Publishing guide: `PUBLISHING_EXTENSIONS.md`.

## Script Debugger

Indent now includes a built-in script debugger for `.ind` execution.

CLI usage:

```bash
indent --debug examples/demo.ind
```

With startup breakpoints:

```bash
indent --debug --break 10,25 examples/demo.ind
```

Debugger commands:

- `s` or `step`: execute next statement and pause again
- `c` or `continue`: run until next breakpoint
- `p <expr>`: inspect an expression in current context
- `b <line>`: add breakpoint
- `cl <line>`: clear breakpoint
- `bl`: list breakpoints
- `l` or `list`: show nearby source lines
- `q` or `quit`: stop execution

## Standard Library

Starter standard-library modules live in `std/`:

- `std/math.ind`
- `std/strings.ind`
- `std/io.ind`
- `std/testing.ind`

Example script: `examples/stdlib_demo.ind`.

If your script is outside the repository root, set module search path:

```bash
export INDENT_PATH=/path/to/indent
indent path/to/script.ind
```

## Package Manager (indentpkg)

`indentpkg` is a pip-like package installer with lockfile support.

```bash
./indentpkg init
./indentpkg install colors               # from index
./indentpkg install --global colors      # user-level install (like pip)
./indentpkg install colors ./packages/colors.ind
./indentpkg search color
./indentpkg update
./indentpkg uninstall colors
./indentpkg list
```

Install scope behavior:

- If `indent.toml` exists: installs are local to `./indent_packages`
- If no project file exists: installs default to global user site-packages at `~/.local/share/indent/site-packages`
- You can force scope with `--local` or `--global`

Global packages are auto-discoverable at runtime.

Package index behavior:

- Default: local `packages/index.txt` when present
- Fallback: hosted index URL
- Override with env: `INDENTPKG_INDEX=/path/or/url/to/index.txt`

## AIR (Indent Install Registry)

`air` is the user-friendly package CLI on top of `indentpkg`, similar to `pip` workflows.

```bash
./air registry github <owner/repo> main packages/index.txt
./air search color
./air install colors
./air install --global colors
./air list
./air update colors
./air uninstall colors
```

Windows PowerShell variants:

```powershell
air registry github <owner/repo> main index.txt
air search color
air install colors
air list
air publish colors .\packages\colors.ind "Basic color constants"
```

Publish to registry:

```bash
./air publish colors ./packages/colors.ind "Basic color constants"
```

If push auth is not configured yet:

```bash
./air publish colors ./packages/colors.ind "Basic color constants" --no-push
```

Use any GitHub-hosted registry index (raw URL under the hood):

```bash
./air registry set https://raw.githubusercontent.com/<owner>/<repo>/main/packages/index.txt
```

Show/reset registry:

```bash
./air registry show
./air registry reset
```

If your network is slow or a registry URL is unreachable, tune fetch timeouts:

```bash
export INDENTPKG_CONNECT_TIMEOUT=5
export INDENTPKG_MAX_TIME=20
```

Example with your GitHub registry repo:

```bash
./air registry github xytro-labs/indent-air master index.txt
```

Registry CI validation for PRs is defined in `.github/workflows/registry-validate.yml`.

Lockfile format (`indent.lock`):

- Header: `lockfile_version=1`
- Entry row: `name|source|sha256|installed_file`

## Learn Indent

Starter lessons are in `docs/learn/`:

- `docs/learn/01-hello-and-vars.md`
- `docs/learn/02-functions-and-types.md`
- `docs/learn/03-testing.md`

## Notes

- Indentation is required for block structure.
- For function call argument blocks:
  - Plain expressions are positional arguments.
  - `name is value` entries are named arguments.
  - You can still define local helper variables in the argument block with `def.var:`.
