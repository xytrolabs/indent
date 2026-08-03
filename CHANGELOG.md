# Indent Changelog

## 1.2.0 — 2026-08-03

### 🎉 Major Language Features

#### Default Parameters
```indent
fun greet name = "World"
    say "Hello " + name + "!"
greet           # → "Hello World!"
greet "Ada"     # → "Hello Ada!"
```

#### String Interpolation
```indent
var name string = "Ada"
say "Hello %name%!"         # → "Hello Ada!"
```
Variables wrapped in `%...%` are interpolated into strings automatically in `say` statements.

#### Comprehensions
```indent
[x * x for x in range(5)]           # → [0, 1, 4, 9, 16]
{x: x * 2 for x in range(3)}        # → {"0": 0, "1": 2, "2": 4}
[x for x in list if x > 5]          # Filtered comprehension
```

#### Lambda Expressions
```indent
var double = fn(x): x * 2
say double 5                        # → 10
```

#### Ternary Expressions
```indent
var s string = "adult" if age >= 18 else "child"
```

#### Bitwise Operators
```indent
5 & 3    # → 1 (AND)
5 | 3    # → 7 (OR)
5 ^ 3    # → 6 (XOR)
~5       # → -6 (NOT)
1 << 2   # → 4 (left shift)
8 >> 2   # → 2 (right shift)
```

#### Chained Comparisons
```indent
if 0 < x < 10           # → x > 0 and x < 10
```

#### Identity Operators
```indent
x is empty              # true if x is null/none
x is not y              # identity check
```

### 🏷️ New Keywords & Aliases

| Keyword | Description |
|---|---|
| `null` | Alias for `empty` |
| `for` | `for x in list:` alias for `repeat for x in list:` |
| `import` | `import module` alias for `get module` |
| `open` | File context manager: `open "file.txt" for read as f:` |

### 🔧 Syntax Improvements

- **Return type annotation**: `fun add a b as int` (replaces `-> int`)
- **Function references**: pass functions as values without calling them
- **Decorators**: `@log` / `@cache` before function definitions

### 📦 New Built-in Functions (20+)

#### Regex
| Function | Description |
|---|---|
| `regex_match(pattern, text)` | Returns `true` if pattern matches |
| `regex_search(pattern, text)` | Returns `{start, end, text}` of first match |
| `regex_findall(pattern, text)` | Returns list of all matches |
| `regex_replace(pattern, repl, text)` | Replace all matches |
| `regex_split(pattern, text)` | Split text by regex pattern |

#### Datetime
| Function | Description |
|---|---|
| `time_utc()` | Unix timestamp (alias for `time_now`) |
| `time_format(ts, fmt)` | Format timestamp (e.g. `"%Y-%m-%d %H:%M:%S"`) |
| `time_parse(str)` | Parse ISO 8601 string to timestamp |

#### Crypto & Encoding
| Function | Description |
|---|---|
| `uuid()` | Generate random UUID v4 |
| `base64_encode(text)` | Encode text to Base64 |
| `base64_decode(text)` | Decode Base64 to text |
| `hash_sha256(text)` | SHA256 hash of text |

#### Path & Filesystem
| Function | Description |
|---|---|
| `glob(pattern)` | List files matching wildcard pattern |
| `path_join(a, b, ...)` | Join path components |
| `path_basename(path)` | Extract filename from path |
| `path_dirname(path)` | Extract directory from path |

#### Functional Programming
| Function | Description |
|---|---|
| `map(list, func)` | Apply function to each element |
| `filter(list, func)` | Filter list by predicate function |

#### String Helpers
| Function | Description |
|---|---|
| `pad_left(text, width, char)` | Left-pad string to width |
| `pad_right(text, width, char)` | Right-pad string to width |
| `repeat_str(text, count)` | Repeat string N times |

### 📚 Standard Library

- **AIR** (Accessible Indent Registry): Package manager for Indent
  - `air install <pkg>` — install packages
  - `air search <query>` — search registry
  - `air publish <name> <file>` — publish packages
  - Registry: `https://github.com/xytrolabs/air`
- 17 standard packages: agame, builtins, colors, config, csv, datetime, discord, html, http, json, jsondb, math, os, path, random, sys, time

### 🛠️ Tooling

- `indent --version` displays `indent 1.2.0`
- `for` loop alias for `repeat for`
- `import` alias for `get`
- Generator functions with `yield`
- Decorator syntax `@name` before functions

### 🧹 Removals

- Old Aether1 syntax (`def.var:`, `def.fun:`, `say:`, `Give:`, `makeType:`) is deprecated and no longer parsed
- `STOP`/`NEXT`/`RESET` uppercase variants removed — use lowercase `stop`/`next`/`reset`

---

## Earlier Versions

### 1.0.0 — Initial Release
- Core language: variables, functions, control flow, loops
- Classes with single inheritance
- Match/case pattern matching
- Do/Catch/Otherwise/Lastly error handling
- 130+ built-in functions
- HTTP, WebSocket, JSON, file I/O, OS operations
- Interactive debugger (`--debug`)
- LSP server for IDE integration
- Package manager (AIR)
- REPL, formatter, linter, test runner
