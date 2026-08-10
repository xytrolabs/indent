# Contributing to Indent

Thanks for your interest in Indent.

## Core Language

The Indent compiler and runtime (`indent-native/`) is not currently accepting
external pull requests. The language is evolving rapidly and we're keeping
development focused internally for now.

If you find a bug or have a feature request, please join the discussion:

→ **[discord.gg/pG7aCwCy44](https://discord.gg/pG7aCwCy44)**

## Contribute Packages

The best way to contribute to the Indent ecosystem is by creating packages.
Packages are `.ind` files that provide reusable functionality — math utilities,
HTTP helpers, game frameworks, Discord bots, CLI tools, etc.

### What makes a good package?

- A single `.ind` file with a clear purpose
- Uses only the standard library and built-in functions
- Includes comments explaining usage
- Works with the latest Indent release

### How to submit

1. Join the [Xytro Discord](https://discord.gg/pG7aCwCy44)
2. Share your package idea or code in `#contributions`
3. We'll review it and add it to the registry

## Docs

Documentation improvements are welcome. If you spot outdated info, missing
examples, or confusing explanations, open an issue or let us know on Discord.

## Local Development

Build from source:

```bash
cd indent-native
cargo build --release
bash install.sh
```

Run a test file:

```bash
indent v2.ind
```

