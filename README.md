# Indent Language (.ind) v1.4.1

Indent is a simple, readable programming language. It uses indentation-based blocks, lowercase keywords, and minimal punctuation — designed to be easy to learn while powerful enough for real work.

```indent
var name = ask("What is your name? ")
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

## Features

### Core Syntax
| Feature | Syntax |
|---|---|
| Comments | `#! comment` |
| Output | `say "Hello"` |
| Variables | `var x = 42` (type inferred) or `var x int = 42` |
| Reassignment | `x is 43` |
| Compound assignment | `x += 5`, `x *= 2` |
| Type conversion | `set x string`, `set y int` |
| Functions | `fun greet name` with `give` returns |
| Default params | `fun greet name = "World"` |
| Lambdas | `fn(x): x * 2` |
| Imports | `get math`, `get Pow from math`, `import math` |
| String interpolation | `"Hello %name%!"` |
| `null` keyword | Alias for `empty` |
| Indexing | `list[0]`, `dict["key"]`, `dict.key` |
| Slicing | `list[1:3]`, `list[::2]` |
| `open` context | `open "file.txt" for read as f:` |

### Control Flow
| Feature | Syntax |
|---|---|
| If/else-if/else | `if` / `or` / `otherwise` |
| Pattern matching | `match x:` / `case "a":` / `otherwise:` |
| Counted loops | `repeat 5` |
| Iteration | `repeat item in list`, `for x in list` |
| Conditional loops | `repeat until x == 10` |
| Loop control | `stop` / `next` / `reset` |
| Error handling | `do:` / `catch as err:` / `lastly:` |

### Data Types
| Type | Example |
|---|---|
| `string` | `"hello"` |
| `int` | `42` |
| `float` | `3.14` |
| `boolean` | `true`, `false` |
| `list` | `[1, 2, 3]` |
| `group` | `group [1, 2, 2, 3]` → `{1, 2, 3}` (unique) |
| `dict` | `{"key": "val"}` |
| `dynamic` | any type |
| `empty` | null/none |

### Expressions
- **Comprehensions**: `[x * 2 for x in list]`, `[x for x in list if x > 5]`
- **Ternary**: `"adult" if age >= 18 else "child"`
- **Chained comparisons**: `0 < x < 10`
- **Bitwise**: `&`, `|`, `^`, `~`, `<<`, `>>`
- **Identity**: `x is empty`, `x is not y`

### Built-in Functions (130+)
String ops, list/dict/group ops, math, random, time, regex, JSON, HTTP, WebSocket, file I/O, OS operations, crypto (UUID, Base64, SHA256), path helpers, functional (`map`, `filter`), and more. See [`docs/quick-reference.md`](docs/quick-reference.md).

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
indent lint file.ind       # Lint code
indent repl                # Interactive REPL
indent test tests/         # Run tests
indent --debug file.ind    # Debug with breakpoints
indent --update            # Update to latest
```

### Package Manager (AIR)
```bash
air install colors         # Install from registry
air uninstall colors       # Remove a package
air search json            # Search packages
air update                 # Update all packages
air list                   # List installed
air info math              # Package details
```

---

## Documentation

| Document | Description |
|---|---|
| [`docs/INDENT_GUIDE.md`](docs/INDENT_GUIDE.md) | Full language guide |
| [`docs/quick-reference.md`](docs/quick-reference.md) | Syntax cheat sheet |
| [`docs/builtins-reference.md`](docs/builtins-reference.md) | Built-in functions reference |
| [`docs/packages-reference.md`](docs/packages-reference.md) | Standard packages reference |
| [`docs/learn/01-quickstart.md`](docs/learn/01-quickstart.md) | 15-minute quickstart |
| [`docs/learn/COURSE_INDEX.md`](docs/learn/COURSE_INDEX.md) | Full learning course (11 lessons) |
| [`CHANGELOG.md`](CHANGELOG.md) | Version history |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | How to contribute |
| [`RELEASING.md`](RELEASING.md) | Release process |

---

## Build from Source

```bash
cd indent-native
cargo build --release
./target/release/indent --version
```

Requires Rust 1.75+.

---

## Distribute

### GitHub Release Artifacts
Tagged releases (`v*`) publish prebuilt binaries for:
- Linux (x86_64, aarch64) — `.deb`, `.rpm`, tarball
- macOS (x86_64, aarch64) — tarball
- Windows (x86_64) — zip

### Quick Share
```bash
bash scripts/package-for-friend.sh v1.4.1
```

### Release Safety
Automated checks gate every release:
- Runtime unit tests
- Installer safety validation
- Artifact smoke tests
- VS Code extension packaging

Run checks locally before tagging:
```bash
./scripts/ci/release-readiness.sh
```

---

## License

MIT — Xytro Labs © 2026
