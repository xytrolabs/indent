# Publishing VS Code Extensions

This repo includes two VS Code extensions:

- `indent-language/`
- `indent-file-icons/`

## Build VSIX Packages in CI

The workflow [.github/workflows/vscode-extensions.yml](.github/workflows/vscode-extensions.yml) packages both extensions and uploads `.vsix` artifacts.

On tagged releases (`v*`), [.github/workflows/release.yml](.github/workflows/release.yml) also packages both VS Code extensions and publishes the `.vsix` files directly in GitHub Release assets.

## Publish to Marketplace

1. Create/pick your publisher in VS Code Marketplace.
2. Update `publisher` in:
   - [indent-language/package.json](indent-language/package.json)
   - [indent-file-icons/package.json](indent-file-icons/package.json)
3. Create a Personal Access Token for VSCE.
4. Run:

```bash
npm install -g @vscode/vsce
cd indent-language && vsce publish
cd ../indent-file-icons && vsce publish
```

Or use VSIX upload flow from CI artifacts.

## Optional: Automatic Marketplace Publish On Tags

The workflow [.github/workflows/vscode-extensions.yml](.github/workflows/vscode-extensions.yml) can auto-publish extensions on `v*` tags when repository secret `VSCE_PAT` is configured.

Requirements:

- `publisher` in both extension manifests must match your Marketplace publisher.
- Extension `version` must be incremented before tagging.
- Repository secret `VSCE_PAT` must be set.

## Install From GitHub Release

Users can install directly from downloaded `.vsix` files:

```bash
code --install-extension indent-language-<version>.vsix
code --install-extension indent-file-icons-<version>.vsix
```
