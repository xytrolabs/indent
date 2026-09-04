# Deliveries — share a whole project as one bundle

> **Concept.** In Indent the natural units are:
> - **module** — one importable `.ind` file (`get X from web`);
> - **package** — what the AIR registry indexes (`air install web`);
> - **Delivery** — a *group of packages / modules*: a whole project you can
>   build, upload, and pull back down **as a single unit**.

A Delivery bundles a project's module files (which become importable
packages once installed), an optional runnable entry point, and its declared
dependencies into **one** distributable `<name>-<version>.dlv` archive (a
`.tar.gz`). Someone can then `air delivery install <name>` and get the whole
project — modules importable, dependencies pulled in — in one command.

## The manifest — `air-delivery.toml`

```toml
[delivery]
name = "myapp"
version = "0.1.0"
description = "An Indent project delivered as one unit"

[entry]
main = "app.ind"        # optional runnable entry (kept local, not imported)

[modules]               # <importName> = <path-to-.ind-file>
mylib   = "src/mylib.ind"
helpers = "src/helpers.ind"

[dependencies]
colors = "*"
```

Each `[modules]` entry is a module that becomes importable **by that name**
after install (`get SomeFn from mylib`). `[entry] main` is an optional
runnable program that ships with the Delivery but isn't turned into an
import.

## Commands

```bash
air delivery init                      # scaffold air-delivery.toml
air delivery build [dir] [out]         # build <name>-<ver>.dlv
air delivery install <src>             # install (name | file.dlv | dir | url)
air delivery publish <file.dlv> [reg]  # stage into a registry checkout
air delivery search <query>            # find on the registry
air delivery list                      # list registry deliveries
air delivery info <name>               # details on one delivery
```

### Build
From a folder containing `air-delivery.toml`:

```bash
air delivery build            # → dist/<name>-<version>.dlv
air delivery build ./myapp ./out
```

Validates that every module/entry file exists, then produces a single
`.dlv` archive whose layout is:

```
<name>-<version>/
  air-delivery.toml
  modules/<import>.ind      # each becomes an importable package
  entry/<file>              # optional runnable entry
```

### Install
`install` accepts a registry name, a local `.dlv`, a directory containing a
manifest, or an HTTP URL:

```bash
air delivery install myapp                     # from the registry
air delivery install ./dist/myapp-0.1.0.dlv    # local archive
air delivery install ./path/to/project         # build + install a folder
```

Installing a Delivery:
1. copies each `modules/*.ind` into `air-packages/` so they become
   importable (`get Fn from <import>`);
2. keeps the `[entry]` program under `~/.local/share/indent/deliveries/`;
3. installs every `[dependencies]` package that isn't already present;
4. records the Delivery in `~/.local/share/indent/deliveries.txt`.

### Publish (upload)
The registry is a git repo (`xytrolabs/air`). Publishing stages a built
archive into a checkout under `deliveries/<name>.dlv` and updates
`deliveries/index.txt`, then you commit and push to make it live:

```bash
air delivery publish ./dist/myapp-0.1.0.dlv /path/to/air-clone
cd /path/to/air-clone
git add deliveries && git commit -m "delivery: add myapp 0.1.0" && git push
```

The registry keeps a `deliveries/` store + `deliveries/index.txt`
(`name|<archive>|<sha256>|<description>`), mirroring how `packages/` works.
Install verifies the SHA256 when present.

## Notes
- A Delivery changes nothing about how modules import — it is purely an
  install/publish grouping on top of the existing module model.
- Modules from a Delivery install into the same `air-packages/` namespace as
  single packages, so pick module names that won't collide.
- `INDENT_PATH` / `air-packages` is searched by the runtime, so an installed
  Delivery's modules are usable immediately.
