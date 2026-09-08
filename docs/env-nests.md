# Nests — project-local Indent environments

A **nest** is Indent's answer to Python's `venv` (or `node_modules`): an
isolated, per-project place to install packages so projects don't share one
global package store.

```
indent nest init            # create .nest in the current directory
indent nest init myapp      # create myapp/.nest
```

This creates `.nest/` containing:

```
.nest/
  air-packages/     # packages installed while the nest is active
  bin/              # scripts/launchers (prepended to PATH)
  lib/              # extra local modules (added to INDENT_PATH)
  activate          # bash activation script
  Activate.ps1      # PowerShell activation script
  deps.txt          # optional dependency manifest (one package per line)
  README.txt
```

## Use it

```bash
source .nest/activate          # PowerShell:  . .nest/Activate.ps1

air install colors             # installs into .nest/air-packages (project-local)
air deps                       # or use the deps.txt manifest

indent myapp.ind               # imports resolve from the nest first
```

Activating sets:

- `INDENT_NEST` — the nest path
- `INDENT_HOME` — points `air` at the nest, so `air install` is project-local
- `INDENT_PATH` — includes `.nest/air-packages` and `.nest/lib`, so `get X from pkg`
  resolves from the nest (the interpreter already searches `INDENT_PATH`)

Deactivate with `deactivate`, or close the shell.

## Pin an Indent version (per-nest interpreter)

Like choosing which Python a `venv` is built from, a nest can **pin its own
Indent interpreter version** so the whole project uses a specific release
(including an older one) regardless of what `indent` is on your global PATH.

```bash
indent nest install             # pin the newest published release
indent nest install 2.1.0       # pin a specific (e.g. older) release
indent nest use 2.1.0           # alias of install
indent nest version             # show what this nest is pinned to
```

`install` downloads the **prebuilt** binary that the release CI publishes for
that tag into `.nest/bin/` and records the version in `.nest/indent-version`.
Because activating a nest prepends `.nest/bin` to `PATH`, running `indent` while
the nest is active uses the pinned version:

```bash
source .nest/activate
indent --version                # → the nest's pinned Indent
```

Notes:

- Versions are release tags: `latest` (default) or `2.1.0` / `v2.1.0`.
- The binary is downloaded from GitHub Releases
  (`indent-<version>-<target>.tar.gz`), so the tag must have CI-built binaries
  published. Only tagged releases with binaries can be installed into a nest; if
  none is published yet for that tag you'll get a clear message.
- Pinning is per-nest and isolated: switch to a different project's nest and you
  get that project's interpreter version.

## How it relates to normal Indent

Without a nest, packages install to `~/.local/share/indent/air-packages` and
imports search the usual `INDENT_PATH`. A nest overrides `INDENT_HOME` /
`INDENT_PATH` for just that project, giving isolation and reproducible deps
(put them in `deps.txt`).
