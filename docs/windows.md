# Indent on Windows

> Indent ships a native `indent.exe` for Windows (`x86_64-pc-windows-msvc` and
> `aarch64-pc-windows-msvc`). The installer is a single PowerShell command and
> writes **only to user-space locations** — no admin elevation required.

---

## Quick Install

Open **PowerShell** (the terminal, not a browser) and run:

```powershell
irm https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.ps1 | iex
```

That single command:

1. Detects your CPU architecture (`AMD64` → 64-bit x86, `ARM64` → 64-bit ARM).
2. Downloads the matching `indent.exe` from the latest GitHub release.
3. Installs companion tools (`air`, `indentpkg`) alongside it.
4. Downloads or copies the standard library (`*.ind` std modules).
5. Creates `indent.cmd` / `indent-debug.cmd` launchers in `%USERPROFILE%\.local\bin`.
6. Adds `%USERPROFILE%\.local\bin` to your **user** PATH (asks first).
7. Configures VS Code to recognise `.ind` files and use the Indent icon theme.

### Install from a local build

If you built `indent.exe` yourself from `indent-native/` (e.g. for development),
install that instead:

```powershell
powershell -ExecutionPolicy Bypass -File install.ps1 -Local
```

The script looks for `indent-native\target\release\indent.exe` relative to the
script and copies it into place.

---

## Where Things Go

| Component | Location |
|---|---|
| Binary | `%USERPROFILE%\.local\share\indent\bin\indent.exe` |
| Launcher | `%USERPROFILE%\.local\bin\indent.cmd` |
| Standard library | `%USERPROFILE%\.local\share\indent\std\` |
| Packages (site-packages) | `%USERPROFILE%\.local\share\indent\site-packages\` |
| AIR-installed packages | `%USERPROFILE%\.local\share\indent\air-packages\` |
| Config (`air.env`) | `%USERPROFILE%\.config\indent\air.env` |

> Always launch Indent through `indent.cmd` (or ensure `%USERPROFILE%\.local\bin`
> is on your `PATH`). The launcher sets `INDENT_PATH` so the runtime can find
> the standard library and installed packages — exactly like `PYTHONPATH`.

---

## Verifying the Install

Open a **new** PowerShell window (so the updated PATH takes effect) and run:

```powershell
indent --version
# → indent 1.4.1 (or newer)

indent run hello.ind
```

A quick end-to-end check that the standard library resolves:

```indent
# std_check.ind
get math
say "2^10 = " + string(math.Pow(2, 10))
get Upper from strings
say Upper("windows ready")
```

```powershell
indent run std_check.ind
# → 2^10 = 1024.0
# → WINDOWS READY
```

If `get math` fails with a "module not found" error, your `INDENT_PATH` isn't
set correctly — re-run the installer or check that the launcher is on your PATH.

---

## How the Runtime Finds Modules on Windows

The runtime resolves `get X from Y` imports in this order:

1. The script's own directory, then each parent (`aether_packages/` too).
2. Every directory listed in the `INDENT_PATH` environment variable.
   - The launcher sets this for you. **Windows uses `;` as the path separator**
     (not `:`), and drive letters like `C:\` are handled correctly.
3. Default folders under your home directory:
   - `~/.local/share/indent/site-packages/`
   - `~/.local/share/indent/std/`
   - `~/.local/share/indent/air-packages/`
   - `~/.local/share/aether/site-packages/` (legacy)

On Windows the home directory is resolved from `USERPROFILE` (Indent
understands that Windows doesn't set `HOME` the way Unix does).

> 💡 If you prefer not to use the launcher, set `INDENT_PATH` yourself in your
> PowerShell profile so every session finds the stdlib:
>
> ```powershell
> [Environment]::SetEnvironmentVariable(
>     "INDENT_PATH",
>     "$env:USERPROFILE\.local\share\indent\site-packages;$env:USERPROFILE\.local\share\indent\std;$env:USERPROFILE\.local\share\indent\air-packages;",
>     "User"
> )
> ```
>
> Then open a new terminal. (Note the `;` separators, and the trailing `;`.)

---

## Common Windows Issues

### "The term 'indent' is not recognized"
Your `%USERPROFILE%\.local\bin` isn't on your PATH, or you haven't opened a new
terminal. Re-run the installer and answer **Y** when it asks to add PATH, then
open a fresh PowerShell.

### "get math" fails with module not found
`INDENT_PATH` is empty or wrong. The launcher normally sets it — run via
`indent.cmd`, or set the user-level `INDENT_PATH` as shown above.

### Execution policy blocks the installer
```powershell
powershell -ExecutionPolicy Bypass -c "irm ... | iex"
```

### `group` / `set` confusion
`group([1, 1, 2])` builds a unique ordered collection → `{1, 2}`. The `set`
keyword is for **type conversion** (`set x string`) and does **not** build a
collection. If an old script calls `set([...])` expecting a collection, it
works as a compatibility alias, but new code should use `group([...])`.

---

## Uninstalling

Remove the folders listed in *Where Things Go* and delete the `indent` /
`air` / `indentpkg` launchers from `%USERPROFILE%\.local\bin`. Then remove
`%USERPROFILE%\.local\bin` from your user PATH if you no longer want it there.

---

## Companion Tools on Windows

- `air` / `air.ps1` — the AIR package manager (`air install x`, `air list`, …).
  The `.ps1` version is installed alongside `indent.exe`.
- `indentpkg` / `indentpkg.ps1` — legacy package manager.
- The CI (`windows-tooling.yml`) validates both PowerShell tools on every push,
  and the release pipeline smoke-tests the `indent.exe` on a real Windows runner
  before publishing.
