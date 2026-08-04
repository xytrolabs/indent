# ⚒ Indent Forge IDE

**Indent Forge** is the official native desktop IDE for the [Indent](https://github.com/xytrolabs/indent) programming language. Built with **Tauri 2.0 + Rust** for maximum performance, featuring a custom dark theme and the **Scrible AI agent** preinstalled.

> **Architecture**: Tauri 2.0 (Rust backend) + Text Editor + Indent CLI + Scrible AI (Ollama)

> **Version**: 2.1.0 — Indent v1.3.0 compatible

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
- **Editor** — Full Indent syntax support with indentation guides
- **Scrible AI Agent** — Ollama-powered AI with:
  - **Inline code completions** (Fill-In-the-Middle)
  - **Chat interface** for code generation, explanation, and fixes
  - **Preinstalled model** + any Ollama model support
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
Aether-Forge-IDE/
├── src-tauri/
│   ├── Cargo.toml              # Tauri 2.0 + Rust dependencies
│   ├── tauri.conf.json         # Window config, CSP, bundling
│   └── src/main.rs             # Rust backend (files, runtime, Scrible AI)
├── src/
│   ├── index.html              # IDE shell layout
│   ├── css/
│   │   └── forge-dark.css      # Complete dark theme
│   ├── js/
│   │   ├── forge-app.js        # State management, tabs, IPC, scrible
│   │   └── lib/                # Third-party libraries
│   └── assets/                 # Icons (SVG, Seti font)
├── build-appimage.sh           # AppImage packaging
├── install-models.sh           # Pulls Scrible AI models from HF
└── run.sh                      # Quick dev launch
```

## Scrible AI Agent

Scrible connects to Ollama for AI-powered code assistance:

| Model | Size | Description |
|---|---|---|
| `scrible-chat` | ~500 MB | **Recommended** — Fine-tuned for Indent chat (HF) |
| `scrible-inline` | varies | Fill-In-the-Middle completions (HF) |
| Any Ollama model | varies | Custom model support via the model selector |

### Installing Models

```bash
# Pull pre-configured Indent models
./install-models.sh

# Or manually via Ollama
ollama pull scrible-chat
ollama pull scrible-inline
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
