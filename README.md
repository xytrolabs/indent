# Indent Language (.ind) v1.4.1

Indent is a simple, readable programming language. No braces, no parentheses, no symbols — just clean, indented code. Designed to be easy to learn while powerful enough for real work.

```indent
var name = ask "What is your name? "
say "Hello " + name + "!"
```

---

## Quick Install

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.sh | bash

# macOS (Homebrew)
brew install xytrolabs/indent/indent

# Windows (PowerShell)
powershell -c "irm https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.ps1 | iex"
```

---

## Why Indent?

No symbols. No braces. No semicolons. Just words and indentation.

```indent
fun greet name
    say "Hello " + name

greet "Ada"
```

Indent reads like English. `fun` defines a function. `give` returns a value. `repeat` loops. `otherwise` handles fallback. You already know how to read it.

---

## Features

### Core Syntax
| Feature | Indent |
|---|---|
| Comments | `#! this is a comment` |
| Output | `say "Hello"` |
| Variables | `var x = 42` |
| With type | `var name string = "Ada"` |
| Reassign | `x is 43` |
| Short ops | `x += 5`, `x -= 2`, `x *= 10` |
| Type cast | `set x string`, `set y int` |
| Functions | `fun add a b` then `give a + b` |
| Default value | `fun greet name = "World"` |
| Lambda | `fn x: x * 2` |
| Imports | `get math`, `import math`, `get Pow from math` |
| Text in strings | `"Hello %name%!"` |
| Null | `null` (alias for `empty`) |

### Control Flow
| Feature | Indent |
|---|---|
| If / else if / else | `if` / `or` / `otherwise` |
| Pattern match | `match x:` / `case "a":` / `otherwise:` |
| Count loop | `repeat 5` |
| Over items | `repeat item in list` |
| Conditional | `repeat until done` |
| Break / continue | `stop` / `next` / `reset` |
| Error handling | `do:` / `catch as err:` / `lastly:` |
| File context | `open "file.txt" for read as f:` |

### Data Types
| Type | Example |
|---|---|
| `string` | `"hello"` |
| `int` | `42` |
| `float` | `3.14` |
| `boolean` | `true`, `false` |
| `list` | `[1, 2, 3]` |
| `group` | `group [1, 2, 2, 3]` → `{1, 2, 3}` |
| `dict` | `{"key": "val"}` |
| `dynamic` | anything |
| `empty` | nothing |

### Expressions
- **List comprehension**: `[x * 2 for x in list]`
- **Filtered**: `[x for x in list if x > 5]`
- **Ternary**: `"adult" if age >= 18 else "child"`
- **Chained**: `0 < x < 10`
- **Bitwise**: `5 & 3`, `1 << 2`, `~5`
- **Identity**: `x is empty`, `x is not y`
- **Membership**: `"banana" in fruits`

### Built-in Functions (130+)
String ops, list/group/dict ops, math, random, time, regex, JSON, HTTP, WebSocket, file I/O, OS, crypto, path helpers, functional (`map`, `filter`), assertions. See [`docs/quick-reference.md`](docs/quick-reference.md).

### Classes
```indent
class Person
    var name string
    var age int
    fun greet
        say "I'm " + name

var p dynamic = Person "Ada" 28
p.greet()
```

### Tooling
```bash
indent run file.ind        # Run a program
indent fmt file.ind        # Format code
indent check file.ind      # Check syntax
indent lint file.ind       # Lint
indent repl                # Interactive shell
indent test tests/         # Run tests
indent --debug file.ind    # Debug with breakpoints
indent --update            # Update to latest
```

### Standard Library (std/)
Indent ships with 17 std modules — no install needed. Import by name:

```indent
get Pow from math          # math helpers
get Upper from strings     # string utilities
get Sha256 from hash       # hashing
get Write from fs          # file system
```

Modules: `strings`, `math`, `collections`, `fs`, `json`, `os`, `io`, `time`, `datetime`, `random`, `regex`, `path`, `hash`, `base64`, `sys`, `testing`, `net`. Std functions are PascalCase so they never clash with the (lowercase) builtins.

### Package Manager (AIR)
AIR is Indent's pip — install packages from the [registry](https://github.com/xytrolabs/air) (47 packages and growing):
```bash
air install stats          # Install from registry
air install slug           # Install another
air uninstall stats        # Remove
air search json            # Find packages
air update                 # Update all
air list                   # Show installed
air info math              # Package details
```

Popular packages: `stats`, `matrix`, `markdown`, `yaml`, `args`, `logger`, `url`, `cookie`, `slug`, `textwrap`, `diff`, `fraction`, `semver`, `asciitable`, `colors`, `agame`, `discord`. AIR auto-detects and installs `get X from Y` dependencies. Installed packages resolve automatically from `~/.local/share/indent/air-packages/`.

### GUI
Indent can open native windows with `gui_show_html(html, [title], [w], [h])` — a WebKitGTK window rendering HTML:
```indent
gui_show_html("<h1>Hello</h1>", "My App", 800, 600)
```
The `indent-gui` helper builds automatically during install (needs `gcc`, `gtk3`, `webkit2gtk`). See `indent-native/indent-gui.c`.

### AI
Indent talks to Ollama over HTTP — chat, embeddings, and more, all natively:
```indent
var payload = {"model": "qwen2.5:0.5b", "prompt": "What is 2+2?", "stream": false}
var resp = http_post_json("http://localhost:11434/api/generate", payload)
var data = json_loads(resp["body"])
say data["response"]    # → "The answer to 2+2 is 4."
```
Also has full Python interop (`python_eval`, `python_eval_json`, `python_exec`, `python_run_file`).

### Examples
Working programs in [`examples/`](examples/): AI semantic search, AI-narrated game with GUI, game simulation, AI chat, embeddings, Python interop.

#### Playable GUI game: Snake
```bash
indent examples/snake_game.ind       # play Snake in a native window (arrow keys)
```
Opens a playable Snake game in a WebKitGTK window, with an AI-generated tip from Ollama injected into the page. Test it automatically:
```bash
node tests/snake_logic_test.js       # validates game logic headlessly (5 tests)
bash tests/gui_snake_test.sh         # opens the window, verifies render, closes it
```

---

## Documentation

| Document | What |
|---|---|
| [`docs/INDENT_GUIDE.md`](docs/INDENT_GUIDE.md) | Full language guide |
| [`docs/quick-reference.md`](docs/quick-reference.md) | Syntax cheat sheet |
| [`docs/builtins-reference.md`](docs/builtins-reference.md) | All built-in functions |
| [`docs/learn/01-quickstart.md`](docs/learn/01-quickstart.md) | 15-minute quickstart |
| [`docs/learn/COURSE_INDEX.md`](docs/learn/COURSE_INDEX.md) | Full course (11 lessons) |
| [`CHANGELOG.md`](CHANGELOG.md) | Version history |

---

## Build from Source

```bash
cd indent-native
cargo build --release
./target/release/indent --version
```

---

## License

MIT — Xytro Labs © 2026
