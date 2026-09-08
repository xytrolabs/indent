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

## How it relates to normal Indent

Without a nest, packages install to `~/.local/share/indent/air-packages` and
imports search the usual `INDENT_PATH`. A nest overrides `INDENT_HOME` /
`INDENT_PATH` for just that project, giving isolation and reproducible deps
(put them in `deps.txt`).
