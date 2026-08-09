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
```bash
air install colors         # Install from registry
air uninstall colors       # Remove
air search json            # Find packages
air update                 # Update all
air list                   # Show installed
air info math              # Package details
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
