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
| `group` | `group([1, 2, 2, 3])` → `{1, 2, 3}` (`set` is reserved for type conversion) |
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
AIR is Indent's pip — install packages from the [registry](https://github.com/xytrolabs/air) (50 packages and growing):
```bash
air install stats          # Install from registry
air install slug           # Install another
air uninstall stats        # Remove
air search json            # Find packages
air update                 # Update all
air list                   # Show installed
air info math              # Package details
```

Popular packages: `ai`, `stats`, `matrix`, `markdown`, `yaml`, `args`, `logger`, `url`, `cookie`, `slug`, `textwrap`, `diff`, `fraction`, `semver`, `asciitable`, `colors`, `agame`, `ingame`, `discord`. AIR auto-detects and installs `get X from Y` dependencies. Installed packages resolve automatically from `~/.local/share/indent/air-packages/`.

### GUI
Indent can open native windows with `gui_show_html(html, [title], [w], [h])` — a WebKitGTK window rendering HTML:
```indent
gui_show_html("<h1>Hello</h1>", "My App", 800, 600)
```
The `indent-gui` helper builds automatically during install (needs `gcc`, `gtk3`, `webkit2gtk`). See `indent-native/indent-gui.c`.

### AI
The `ai` package is like Python's `openai` SDK — an OpenAI-native client that works with **real OpenAI** or a **local Ollama** server (`air install ai`, then `get ai as AI`):
```indent
get ai as AI
#! Local Ollama (default, no key):
var reply = AI.Chat("qwen2.5:0.5b", [{"role":"user","content":"What is 2+2?"}])
say reply                    # → "4"

#! Real OpenAI — same API, just point base + key:
# AI.SetBase("https://api.openai.com/v1")
# AI.SetApiKey("sk-...")
# var gpt = AI.Chat("gpt-4o-mini", [{"role":"user","content":"hi"}])
```
Functions: `AI.Chat` (chat completions), `AI.Ask` (single prompt), `AI.Embed` / `AI.EmbedMany` (embeddings), `AI.Models` (list models), `AI.Similarity` (cosine), `AI.Search` (semantic search), plus `AI.SetBase` / `AI.SetApiKey` / `AI.SetDefaultModel` config. Under the hood it uses Indent's native `http_post_json` / `http_get` builtins — no Python needed. See `examples/ai_openai_api.ind` for the package API *and* the raw low-level way to call any REST API.

Also has full Python interop (`python_eval`, `python_eval_json`, `python_exec`, `python_run_file`).

### InGame — PyGame-style games in pure Indent
[`std/ingame.ind`](std/ingame.ind) is a native 2D game framework that **mirrors PyGame's API**: `Init` (init), `SetMode` (display.set_mode), `DrawRect`/`DrawCircle`/`DrawLine`/`DrawPolygon`/`DrawText` (draw.*), `Flip` (display.flip), `GetEvents` (event.get), `GetKeys` (key.get_pressed), `GetMouse` (mouse.get_pos), `Tick` (time.Clock.tick), `Quit`. **All game logic lives in Indent** (movement, collision, physics, scoring, rendering); a native window just draws frames and reports input — like PyGame, but entirely in Indent.
```indent
get Init from ingame
get SetMode from ingame
get DrawRect from ingame
get DrawCircle from ingame
get Flip from ingame
get GetEvents from ingame
get Quit from ingame

Init()
var win = SetMode(400, 400, "My Game")   #! display.set_mode
repeat while running
    repeat e in GetEvents()              #! event.get
        if e["type"] == "quit"
            running is false
        if e["type"] == "keydown"
            ...
    DrawRect x y w h "#39d353"           #! draw.rect
    DrawCircle cx cy r "#f85149"         #! draw.circle
    Flip "#000000"                       #! display.flip (flush frame)
    Tick 60                              #! ~60 fps
Quit()
```
Works via `indent-ingame` (a native WebKitGTK canvas helper, built by `install.sh`). `air install ingame` also works.

#### Playable GUI game: Snake
```bash
indent examples/snake_game.ind                 # play (arrow keys)
INDENT_SNAKE_BOT=1 indent examples/snake_game.ind   # auto-play bot
```
Snake is written 100% in Indent using InGame — the bot AI, wall/self collision, growth, and scoring are all Indent logic. `INDENT_SNAKE_BOT=1` makes it play itself (verified: scores 80, snake grows to 11 segments).

#### Playable GUI game: Breakout
```bash
indent examples/breakout_game.ind              # play (arrow keys)
INDENT_BREAKOUT_BOT=1 indent examples/breakout_game.ind   # auto-play bot
```
Breakout is written 100% in Indent: paddle physics, ball bounce, brick collision, scoring, and HUD all in Indent. The bot autoplay verifies the full loop headlessly.

### Examples
Working programs in [`examples/`](examples/): AI package demo (`ai_pkg.ind`), AI semantic search, AI-narrated game with GUI, game simulation, AI chat, embeddings, Python interop, InGame Snake, InGame Breakout.

---

## Documentation

| Document | What |
|---|---|
| [`docs/INDENT_GUIDE.md`](docs/INDENT_GUIDE.md) | Full language guide |
| [`docs/indent-vs-python.md`](docs/indent-vs-python.md) | Indent vs Python side-by-side |
| [`docs/quick-reference.md`](docs/quick-reference.md) | Syntax cheat sheet |
| [`docs/builtins-reference.md`](docs/builtins-reference.md) | All built-in functions |
| [`docs/learn/01-quickstart.md`](docs/learn/01-quickstart.md) | 15-minute quickstart |
| [`docs/learn/COURSE_INDEX.md`](docs/learn/COURSE_INDEX.md) | Full course (11 lessons) |
| [`docs/packages-reference.md`](docs/packages-reference.md) | Registry package reference |
| [`CHANGELOG.md`](CHANGELOG.md) | Version history |

### First-party package docs (Xytro-maintained)

| Package | Doc | What |
|---|---|---|
| `discord` | [`docs/discord-package.md`](docs/discord-package.md) | Discord bot library (v6.0) |
| `ai` | [`docs/ai-package.md`](docs/ai-package.md) | OpenAI-native AI assistant |
| `ingame` | [`docs/ingame-package.md`](docs/ingame-package.md) | PyGame-style 2D game framework |
| `agame` | [`docs/agame-package.md`](docs/agame-package.md) | 2D game helper math/entities |

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
