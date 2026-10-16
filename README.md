# Indent Language (.ind) v2.4.0

Indent is a simple, readable programming language. No braces, no parentheses, no symbols — just clean, indented code. Designed to be easy to learn while powerful enough for real work.

```indent
var name = ask "What is your name? "
say "Hello " + name + "!"
```

> ## 🧠 Indent 2.4.0 — lighter on memory
>
> Building on 2.3's speed work, 2.4 shrinks how much memory Indent uses. Every
> collection — strings, lists, dictionaries and groups — is now **copy-on-write**:
> reading a variable is a refcount bump instead of a deep copy, and the copy only
> happens on the *first write* after a value becomes shared. Pass-by-value
> semantics are unchanged (`var b = a` still never lets `b` affect `a`).
>
> | Workload | before | **2.4.0** |
> |---|---|---|
> | Peak RSS holding 200 copies of one 200 KB string | 84.7 MB | **12.6 MB** |
> | Passing a 201 KB string to a function | 130 µs | **7.7 µs** |
> | `len()` of a 20 000-item list | 0.0767 s | **0.0000424 s** |
> | `has_key()` on a 5 000-key dict | 0.0711 s | **0.0000659 s** |
> | Startup importing five `std` modules | 3.51 ms | **2.49 ms** |
> | Peak RSS, 20 000 statements of three call arguments each | 30.1 MB | **19.8 MB** |
>
> `Value` is 32 bytes (was 56) and every statement 112 (was 152) — guardrail
> tests fail the build if either grows back.
>
> ### 🧪 Release channels: `stable` and `exp`
> Two channels now come out of the same pipeline and the same gates:
> - **stable** is the default, and what `releases/latest` resolves to.
> - **exp** follows tagged prereleases such as `v2.4.1-exp.1`, published as
>   GitHub *prereleases*, so a stable install is never moved onto an
>   experimental build by accident.
>
> Install a channel with `--channel`, or switch an existing install in place:
> `indent --switch-to-exp`, `indent --switch-to-stable`, `indent --channel`.
> The choice is recorded beside the binary, so `indent --update` keeps following
> it, and the installer verifies the release's `SHA256SUMS.txt` first.
>
> ⚠️ **Breaking:** `indent --version` now prints the channel — `indent 2.4.0 (stable)`.
> Any tooling parsing the version from that line should take the **first** word.
>
> Upgrading is automatic: the on-disk program cache changed format, so the first
> run after upgrading re-parses each cached file once. See
> [`CHANGELOG.md`](CHANGELOG.md) for the full notes.
>
> ## 🚀 Indent 2.3.0 — the performance release
>
> Major optimizations and general improvements coming to Indent in 2.3! All of it
> is additive, and every figure below is the same benchmark run against the
> released **2.2.2** binary and this build (`sh benchmarks/verify.sh` reproduces it).
>
> | Workload | 2.2.2 | **2.3.0** | Gain |
> |---|---|---|---|
> | Interpreter hot-path suite | 377 ms | **109 ms** | **3.5×** |
> | 100 000 function calls | 0.1238 s | **0.0365 s** | 3.4× |
> | 50 000 list `append`s | **52.5 s** | **0.0071 s** | **~7 400×** |
> | `len()` of a 20 000-item list | 59.0 ms / 300 | **0.057 ms** | **~1000×** |
> | `has_key()` on a 5 000-key dict | 44.3 ms / 300 | **0.092 ms** | **~480×** |
> | Summing a 20 000-item list with `xs[i]` | 2.497 s | **0.00545 s** | **~458×** |
> | Walking a 20 000-key dict with `d[k]` | 9.684 s | **0.00897 s** | **~1080×** |
> | 2 000 × slicing a 20 000-item list | 0.2468 s | **0.000697 s** | **354×** |
> | Call into a 302-statement function | 16.36 µs | **0.33 µs** | **50×** |
> | 50 000 `call_func` | 0.0707 s | **0.0184 s** | 3.8× |
> | 50 000 dict writes | 0.0226 s | **0.0087 s** | 2.6× |
> | 10 000 string concatenations | 0.00745 s | **0.00378 s** | 2.0× |
> | 1-argument / 2-argument call | 402 / 700 ns | **213 / 338 ns** | 1.9× / 2.1× |
> | `i is i + 1` in a loop | 202 ns | **156 ns** | 1.3× |
> | Startup — 5 std imports | 6.19 ms | **3.35 ms** | 1.8× |
> | `.ind` test suite (52 files) | 5.55 s | **3.96 s** cold | 1.4× |
>
> ### What made it faster
> - 🧠 **Expressions are parsed once, not on every evaluation.** Every statement
>   stored its expressions as *text* and the interpreter re-lexed and re-parsed
>   them inside every loop iteration.
> - 💾 **`__incache__/` — Indent's `__pycache__`.** A file's parsed program is
>   written beside the source and reused, so imports skip lexing and parsing.
>   Edit the file and the cache is invalidated immediately, and a cache is never
>   shared with a different Indent build (a `bincode` layout change would otherwise
>   deserialise into a *wrong* program instead of failing); `--no-cache`,
>   `INDENT_NO_AST_CACHE=1` and `--clear-cache` are available.
> - 📞 **Calls no longer scale with the size of the function.** Resolving a call
>   deep-copied the entire body AST, so a 302-statement function cost 16.4 µs per
>   call even when two of its statements ran.
> - 🧩 **Container writes move instead of copying** (`l is append l x`,
>   `d[k] is v`), turning two quadratic loops into linear ones — 50 000 appends
>   went from 52.5 s to 0.0071 s.
> - 🔢 **Operators stopped copying their operands**, and 🧵 **loops stopped
>   copying their iterable twice**.
> - ♻️ **`len()`/`has_key()` on a container are O(1)** — they used to copy it to
>   measure it — and call frames are recycled instead of reallocated.
> - 🔎 **`xs[i]`, `d["k"]` and slices borrow the container too.** Reading one
>   element cloned the whole list or dict, so a loop reading every element was
>   **O(n²)** — summing a 20 000-item list took 2.5 s and walking a 20 000-key
>   dictionary 9.7 s. Both are now linear (458× and 1080× faster).
> - 🧊 **`Value` is 56 bytes, not 128**, and ⚙️ the release profile is tuned for
>   an interpreter (fat LTO, one codegen unit, no unwind tables, stripped).
>
> ### On “values should only be as big as they need to be”
> A value cannot start at 1 byte and grow, because a list, a call argument list
> and a frame slot all share one compile-time element stride — there is no
> per-value size to grow. The real lever is the *smallest* size for all values,
> and 56 bytes is already close to it except for the `Object` variant; getting to
> the 8–16 bytes Lua/CPython/V8 use means boxing every string, list, set and dict
> (an extra allocation and pointer-chase on the most common types). The measured
> 128 → 56 byte change moved memory, not time, so that trade only pays off as a
> side-effect of the copy-on-write migration below.
>
> ### Still O(n), honestly
> `len`, `has_key`, `xs[i]` and slices are now O(1), but the underlying rule is
> unchanged: `get_var` returns a **copy**, so *passing* a container to a *user*
> function is still a full clone (6.7× faster than 2.2.2 only because calls got
> cheap). The general fix is copy-on-write values (`Arc<Vec<Value>>` +
> `Arc::make_mut`), which makes reads O(1) and keeps the in-place writes O(1); the
> plan, its audited first stage (`tools/arc_rewrite.py`) and the measured cost of
> its first attempt are recorded in the changelog.
>
> ### 🎉 What's New in 2.2.2
> - 🐛 **Module functions can use their own imports** — a function exported by a
>   module can now call modules *it* imported, when invoked from another file
>   (previously: `Unsupported method 'error' for receiver 'debug'`)
> - 📦 **Installer refreshed** — bundles the full `std/` + AIR and never compiles
>   unless you pass `--build-from-source`
> - 🛡 Releases are verified to ship runtime + AIR + std before publishing
> - 🐛 Installer no longer aborts on a broken `rustc`, and now tells you how to fix it
> - 🌐 `indent.xytro.site/install.sh` now downloads pre-built binaries
>
> ### 🎉 What's New in 2.2.1
> - 🚩 **`flag` is consistent** — `flag "message"` now works (just like `say`); the
>   `flag: "message"` colon form still works too
> - 🪺 **Pin an interpreter version per nest** — `indent nest install 2.1.0`
> - ℹ️ `indent --help` now lists `nest` subcommands, `--watch`, and `--safe`
> - 🔁 `indent --update` always re-clones fresh (no more dirty-tree aborts)
> - 🧩 **Groups use `group([...])`** — `set` is the *type-conversion* keyword
>
> ### 🎉 What's New in 2.2.0
> - 🪺 **Nests** — project-local environments (`indent nest init`), with auto-detection
> - 🔁 **`indent --watch`** — hot reload: re-run on save (no compile step)
> - 🔒 **`indent --safe`** — default-deny sandbox for running untrusted scripts
> - 📦 **Web package** — `get RunCode from web` + `Html`/`Json`/`Text` response builders
> - 📦 **AIR Deliveries** — `air delivery` build/install/publish to share a whole project
> - 🛠 Installer (Windows) — robust detection, source-build & dependency fallback
>
> ### 🎉 What's New in 2.1.0
> - ✍️ **`is` for declarations, `=` for reassignment** — `var x is 42` declares,
>   `x = 43` reassigns. The old forms (`var x = 42`, `x is 43`) still work too.
>
> ### 🎉 What's New in 2.0.0
> - 🎨 **Full color output** — `fg`, `bg`, `style`, `gradient`, `multicolor`, `rainbow`, `paint`
> - 🧰 **Std-lib breadth** — itertools/functools/collections helpers, math & random extras, `os_which`, `os_run_ok`
> - 🧩 **Class special methods** (natural names: `to_string`, `add`, `equals`, `len`, `get_item`, `contains`) + **`dataclass`**
> - ⚙️ **Generators / `yield`** — iterate with `for`, materialize with `to_list`, test with `is_generator`
> - 🛠 `not <funcCall args>` parse fix
> - See [`CHANGELOG.md`](CHANGELOG.md) for the full history.

---

## Quick Install

```bash
# Linux / macOS
curl -fsSL https://indent.xytro.site/install.sh | bash

# macOS (Homebrew)
brew install xytrolabs/indent/indent

# Windows (PowerShell 5.1+)
irm https://indent.xytro.site/install.ps1 | iex

# Experimental channel instead of stable
curl -fsSL https://indent.xytro.site/install.sh | bash -s -- --channel exp
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
| Variables | `var x is 42` |
| With type | `var name string is "Ada"` |
| Reassign | `x = 43` |
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

### Colors (truecolor terminal output)
```indent
say fg "red text" "#ff0000"
say bg "highlighted" "yellow"
say gradient "smooth fade" "#ff0000" "#0000ff"
say rainbow "RAINBOW"
say style "bold" "bold"
say paint "styled" "#ffffff" "#222222" ["bold", "underline"]
```

### Built-in Functions (280+)
String ops, list/group/dict ops, math, random, time, regex, JSON, HTTP, WebSocket, file I/O, OS, crypto, path helpers, functional (`map`, `filter`, `reduce`, `chain`, `group_by`, …), **colors** (`fg`/`bg`/`style`/`gradient`/`rainbow`/`paint`), **generators** (`is_generator`, `to_list`), assertions. See [`docs/builtins-reference.md`](docs/builtins-reference.md).

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

### Class special methods (natural names)
Instead of Python's `__str__`/`__add__`/`__eq__`, Indent uses plain-English method names the runtime calls automatically:
```indent
class Vector
    var x int
    var y int
    fun to_string        #! used by say / print
        give "Vector(" + string(x) + ", " + string(y) + ")"
    fun add other        #! used by +
        give Vector(x + other.x, y + other.y)
    fun equals other     #! used by == and !=
        give x == other.x

say Vector(3, 4) + Vector(1, 2)   #! → Vector(4, 6)
```
Also supported: `subtract`, `multiply`, `divide`, `len`, `get_item`, `contains`.

### dataclass
`dataclass Name` is like `class`, but auto-generates `to_string` and `equals`:
```indent
dataclass Point
    var px int
    var py int

say Point(1, 2)                    #! → Point(px: 1, py: 2)
say Point(1,2) == Point(1,2)       #! → TRUE
```

### Generators / `yield`
A function containing `yield` is a generator — iterate it with `for`:
```indent
fun countdown n
    yield n
    yield n - 1
    yield n - 2

for x in countdown 3    #! 3, 2, 1
    say x

to_list (countdown 3)   #! → [3, 2, 1]
is_generator g          #! → TRUE
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
indent --update            # Update to latest (keeps your channel)
indent --channel           # Show the release channel (stable | exp)
indent --switch-to-exp     # Follow experimental prereleases
indent --switch-to-stable  # Go back to stable
```

### Standard Library (std/)
Indent ships with 17 std modules — no install needed. Import by name:

```indent
get Pow from math          #! math helpers
get Upper from strings     #! string utilities
get Sha256 from hash       #! hashing
get Write from fs          #! file system
```

Modules: `strings`, `math`, `collections`, `fs`, `json`, `os`, `io`, `time`,
`datetime`, `random`, `regex`, `path`, `hash`, `base64`, `sys`, `testing`,
`net` — plus the first-party packages `web` (in-browser code running),
`debug` (colored logs), `ingame` (2D games), and `ai` (OpenAI client). Std
functions are PascalCase so they never clash with the (lowercase) builtins.
See [`docs/INDENT_GUIDE.md`](docs/INDENT_GUIDE.md) for the full in-depth guide.

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

### Dynamic Web Server
`http_serve(handler, port)` runs a dynamic web server — it calls your handler
with a request dict (`method`, `path`, `query`, `headers`, `body`) and uses the
returned string/dict as the response. Routing is ordinary Indent logic:
```indent
fun handle req
    if req.path == "/"
        give "<h1>Hello from Indent</h1>"
    if req.path == "/greet"
        give "<h1>Hi " + req.query["name"] + "!</h1>"
    give {"status": 404, "body": "not found", "content_type": "text/plain"}

http_serve handle 8080
```

### AI
The `ai` package is like Python's `openai` SDK — an OpenAI-native client that works with **real OpenAI** or a **local Ollama** server (`air install ai`, then `get ai as AI`):
```indent
get ai as AI
#! Local Ollama (default, no key):
var reply = AI.Chat("qwen2.5:0.5b", [{"role":"user","content":"What is 2+2?"}])
say reply                    #! → "4"

#! Real OpenAI — same API, just point base + key:
#! AI.SetBase("https://api.openai.com/v1")
#! AI.SetApiKey("sk-...")
#! var gpt = AI.Chat("gpt-4o-mini", [{"role":"user","content":"hi"}])
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
| [`docs/known-limitations.md`](docs/known-limitations.md) | Known limitations & gotchas |
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
