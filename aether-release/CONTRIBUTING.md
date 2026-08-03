# Contributing to Indent

Thanks for helping improve Indent.

## Areas You Can Contribute

- Language features in `indent-native/`
- Standard library modules in `std/`
- Package manager and AIR tooling (`indentpkg`, `air`)
- Docs in `README.md` and `docs/`
- Registry packages

## Local Development

1. Build and test runtime:
   - `cd indent-native`
   - `cargo test`
   - `cargo build --release`
2. From repo root, run smoke checks:
   - `indent check examples`
   - `indent run examples/demo.ind`

## Package Registry Contributions

Registry contributions should include:

1. Package source file (`.ind`).
2. Index update with `name|source|description`.
3. Passing CI checks.

### Recommended Flow

1. Fork registry repo.
2. Add package file and index entry.
3. Open a PR.
4. Wait for validation workflow.
5. Get review and merge.

## Commit and PR Guidance

- Keep changes scoped and focused.
- Add or update tests when behavior changes.
- Update docs for user-facing command changes.
- Mention breaking changes clearly in PR description.

## Release Changes

Runtime releases are tag-based.

- `git tag vX.Y.Z`
- `git push origin vX.Y.Z`

This triggers multi-platform release builds in GitHub Actions.
