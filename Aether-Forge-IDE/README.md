# ⚒ Indent Forge IDE

**Indent Forge** is the official native desktop IDE for the [Indent](https://github.com/XytroLabs-Indent/source) programming language. Built with **Tauri 2.0 + Rust** for maximum performance, featuring a custom dark theme and the **Scrible AI agent** preinstalled — powered by **StarCoder2-3B**, heavily modified and trained for coding in Indent.

> **Architecture**: Tauri 2.0 (Rust backend) + CodeMirror 6 (editor) + Tree-sitter (syntax) + Tower-LSP (intelligence)

```
┌──────────────────────────────────────────────────────────────┐
│  ⚒ Indent Forge                              🤖 Scrible  ●  │  ← TOOLBAR
├──────────────────────────────────────────────────────────────┤
│  📄 Untitled.ind  │  🧪 main.ind  │  +                       │  ← TABS
├──────────┬────────────────────────┬──────────────────────────┤
│  📁 FILES│                        │  🤖 AI Coding Agent      │
│          │                        │     (Scrible)            │
│  📁 src/ │   Active File          │                          │
│  🧪 main │   (Text Editor)        │  ┌────────────────────┐  │
│  📄 READ │                        │  │ scrible-chat      │  │
│  📁 tests│   Monaco Editor        │  │ User can use our   │  │
│  🧪 smoke│   with Indent syntax   │  │ preinstalled model │  │
│          │   highlighting         │  │ or Ollama model    │  │
│          │                        │  └────────────────────┘  │
│          │                        │  [Chat messages here]    │
│          │                        │  [Ask Scrible...]  [➤]  │
├──────────┴────────────────────────┴──────────────────────────┤
│  📟 Output — Indent Forge ready.          ⚡ StarCoder2-3B Q4 │  ← STATUS CENTER
└──────────────────────────────────────────────────────────────┘
```

## Features

- **Beautiful Dark Theme** — Custom Catppuccin-inspired palette, sleek and modern
- **Monaco Editor** — Same editor that powers VS Code, with full Indent syntax highlighting
- **Scrible AI Agent** — StarCoder2-3B fine-tuned for Indent, with:
  - **Inline code completions** (Fill-In-the-Middle)
  - **Chat interface** for code generation, explanation, and fixes
  - **Preinstalled pretrained model** + any Ollama model support
- **File Explorer** — Tree view with directory expansion
- **Tab System** — Multi-file editing with dirty-state indicators
- **Integrated Terminal** — Run, debug, and test Indent code directly
- **Status Center** — Real-time output, model status, and cursor position

## Quick Start

### Prerequisites

- **Node.js** >= 18
- **npm** >= 9
- **Rust** >= 1.75 (for the Tauri backend)
- **Node.js** >= 18 (for Tree-sitter CLI only)
- **Indent CLI** installed and in PATH
- **Ollama** (for Scrible AI — optional but recommended)

### Install & Run

```bash
cd Indent-Forge-IDE

# Install Tauri CLI
cargo install tauri-cli --version "^2"

# Run in development mode
cargo tauri dev
```

### Build Standalone Packages

```bash
cargo tauri build           # Native binary for current OS
cargo tauri build --target x86_64-unknown-linux-gnu
```

## Architecture

```
Indent-Forge-IDE/
├── src-tauri/
│   ├── Cargo.toml              # Tauri 2.0 + Rust dependencies
│   ├── tauri.conf.json         # Window config, CSP, bundling
│   └── src/main.rs             # Rust backend (files, LSP, Scrible AI)
│   ├── preload.js            # Secure context bridge
│   ├── indent-runtime/       # Bundled Indent interpreter (Python)
│   └── renderer/
│       ├── index.html        # IDE shell layout
│       ├── css/
│       │   └── forge-dark.css    # Complete dark theme
│       ├── js/
│       │   ├── forge-core.js     # State management, tabs, IPC
│       │   ├── forge-editor.js   # Monaco editor + Indent language support
│       │   ├── forge-files.js    # File explorer panel
│       │   ├── forge-scrible.js  # Scrible AI chat panel
│       │   ├── forge-status.js   # Status center + terminal output
│       │   └── forge-app.js      # Bootstrap
│       └── icons/
│           └── forge.png
```

## Scrible AI Agent

Scrible is powered by **StarCoder2-3B**, heavily modified and trained for coding in Indent:

| Model | Size | Description |
|---|---|---|
| `indent-scrible:3b-q4` | ~2 GB | **Recommended** — Trained on Indent code (preinstalled) |
| `starcoder2:3b` | ~2 GB | Base StarCoder2 model |
| Any Ollama model | varies | Custom model support |

### Training Scrible

The Scrible agent is trained using the pipeline in `../scripts/train-starcoder/`:

```bash
cd ../scripts/train-starcoder
python prepare_data.py    # Collects all .ind files
python finetune.py        # Fine-tunes StarCoder2-3B with QLoRA
python convert_to_ollama.py  # Converts to Ollama format
ollama pull indent-scrible:3b-q4
```

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+N` | New file |
| `Ctrl+O` | Open file |
| `Ctrl+S` | Save |
| `Ctrl+K` | Open folder |
| `F5` | Run current file |
| `Ctrl+F5` | Debug current file |
| `Shift+F5` | Stop execution |
| `Ctrl+T` | Run tests |
| `Ctrl+B` | Toggle Files panel |
| `Ctrl+J` | Toggle Scrible panel |
| `Ctrl+`` | Toggle Status Center |

## License

MIT — Xytro Labs © 2026
