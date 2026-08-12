# Your Journey with Indent 1.4

> Indent 1.4 adds **Group type**, **type conversion** (`set varname type`), compound assignment, and type inference — all with the same simple, lovable syntax.

> Indent is a language designed for **learning and building**. Its syntax uses indentation instead of braces — like Python, but with simpler keywords and fewer symbols. You can write scripts, web servers, GUI apps, and Discord bots, all in one language.

---

## What's New in 1.4.1

> 💡 Unique ordered collections are called **groups** — created with the `group` builtin: `group([1, 2, 2, 3])`. The keyword `set` is reserved for **type conversion** (`set varname type`), so it is not used to build groups. Note the difference:
> - `set x string` — *type conversion* (the `set` keyword)
> - `group([1, 2, 2, 3])` — *builds a group* (the `group` builtin)

- **Group type**: `group([1,2,2,3])` → `{1, 2, 3}` — unique ordered collections
- **Type conversion**: `set name string`, `set x int` — clean type casting
- **Group union**: `s1 + s2` — combine groups
- **Group iteration & comprehension**: `repeat item in s`, `[x*2 for x in s]`

## What's New in 1.3.0

- **Type inference**: `var x = 42` — Indent infers `int` from the value. No need to type `var x int = 42` when the value makes the type obvious.
- **Compound assignment**: `x += 8`, `x -= 10`, `x *= 2`, `x /= 3`, `x %= 5` — shorthand for `x is x + N`

## What's New in 1.2.0

- **Default parameters**: `fun greet name = "World"` — call with or without args
- **String interpolation**: `say "Hello %name%"` — variables right in strings
- **Comprehensions**: `[x * 2 for x in list]`, `{k: v for k, v in pairs}`
- **Lambda expressions**: `fn(x): x * 2`
- **Ternary expressions**: `"big" if n > 10 else "small"`
- **Bitwise operators**: `&`, `|`, `^`, `~`, `<<`, `>>`
- **Chained comparisons**: `0 < x < 10`
- **`for` loop alias**: `for item in list`
- **`import` keyword**: `import math`
- **`null` keyword**: alias for `empty`
- **`open` file context**: `open "file.txt" for read as f:`
- **`is`/`is not` operators**: `x is null`, `x is not y`
- **Return type annotation**: `fun add a b as int`
- **Regex, Datetime, UUID, Base64, Hash, Path, Functional builtins**

---

## 1. Getting Started

### Installation

```bash
# Linux (any distro)
curl -fsSL https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.sh | bash

# macOS
brew install xytrolabs/indent/indent

# Windows (PowerShell)
irm https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.ps1 | iex
```

> 📖 **Windows users**: see [Installing & using Indent on Windows](windows.md) for
> a full walkthrough — what gets installed, how modules resolve, PATH setup, and
> troubleshooting.

### Your First Program

Create `hello.ind`:
```indent
say "Hello, World!"
```

Run it:
```bash
indent run hello.ind
```

---

## 2. Variables & Types

Indent has eight types: `string`, `int`, `float`, `boolean`, `list`, `group`, `dict`, `dynamic`, and `empty`.

```indent
# Type inference — preferred style!
var name = "Ada"            # string
var age = 28                # int
var pi = 3.14               # float
var flag = true             # boolean
var nums = [1, 2, 3]        # list

# Explicit types (when needed)
var data dynamic = getData  # dynamic
var scores list = [95, 87]  # typed list
var nothing empty           # null

# Reassignment
age is 29                   # Use "is", not "="

# Compound assignment
age += 5                    # age is age + 5
age *= 2                    # age is age * 2
```

### Type Conversion

```indent
var x = "42"
set x int                   # → 42 (string→int)

var y = 3
set y float                 # → 3.0

var z = 0
set z boolean               # → FALSE (0→false, non-zero→true)

var data = [1, 2, 2, 3]
set data group              # → {1, 2, 3} (deduplicated)
```

### Groups (unique ordered collections)

```indent
var colors = group(["red", "blue", "red"])  # → {"red", "blue"}
var more = group(["green", "blue"])
var all = colors + more                  # Union: {"red", "blue", "green"}
contains(colors, "red")                 # → TRUE
len(colors)                              # → 2
```

---

## 3. Functions

```indent
fun greet person
    say "Hello " + person

greet "Ada"                 # Space-separated call
greet("Ada")                # Parenthesized call

fun add a b
    give a + b              # Return (NOT "return")

fun greet name = "World"    # Default parameter
greet                       # → "Hello World!"

# Lambda
var double = fn(x): x * 2
say double(5)               # → 10
```

---

## 4. Control Flow

```indent
if score >= 90
    say "A"
or score >= 80              # else-if (NOT elif)
    say "B"
otherwise                   # else
    say "F"

match day:
    case "mon":
        say "Monday"
    case "fri":
        say "TGIF!"
    otherwise:
        say "Another day"
```

---

## 5. Loops

```indent
repeat 5                    # Counted loop
repeat item in list         # Iterate list
repeat item in my_group     # Iterate group
repeat until done           # Conditional

stop    # break
next    # continue
reset   # restart loop
```

---

## 6. Data Structures

```indent
# Lists
var fruits = ["apple", "banana"]
fruits[0]                   # → "apple"
append(fruits, "cherry")
fruits is fruits + ["date"]

# Dictionaries
var person = {"name": "Ada", "age": 28}
person["name"]              # → "Ada"
person.name                 # Dot notation
keys(person)                # → ["name", "age"]

# Groups
var tags = group(["rust", "indent"])
var more = group(["indent", "go"])
tags + more                 # → {"rust", "indent", "go"}
```

---

## 7. Imports & Modules

```indent
get math                    # Whole module
import math                 # Alias
get Pow from math           # Single function
get RandInt from random as R # Aliased import
```

Imports resolve in order: the script's directory (and parents) → `INDENT_PATH` → `~/.local/share/indent/site-packages/`. The std library installs there automatically, so every module below is available in any script.

### Standard Library (std/)

The std library is a set of PascalCase helper functions that wrap the (lowercase) builtins. PascalCase avoids clashing with builtins, and user-defined/imported functions take precedence over builtins at call time.

| Module | What it provides |
|---|---|
| `strings` | case transforms, trim/pad, split/join, search, slice, repeat |
| `math` | arithmetic, abs/sqrt/pow/floor/ceil/round, trig, log/exp, min/max/clamp |
| `collections` | list ops (sort, sum, map, filter, zip, range) + dict ops (keys, values, items, get/set/remove) |
| `fs` | read/write/append file, exists, delete, copy, rename, list dir, sha256 |
| `json` | load/dump/stringify/parse JSON |
| `os` | env vars, cwd, file/dir checks, mkdir/remove/rename, list dir, run command |
| `io` | print, input (string/int/float), read/write/append file, error |
| `time` | now, utc, sleep, format, parse, perf counter |
| `datetime` | date/time now/utc, format/parse, sleep, timestamp |
| `random` | random int/float, choice, shuffle, seed, uuid |
| `regex` | match, search, findall, replace, split |
| `path` | basename, dirname, join, exists/file/dir checks |
| `hash` | sha256 of text or file |
| `base64` | encode/decode |
| `sys` | args, exit, platform, arch, version, executable |
| `testing` | assert, assert-eq, assert-true/false |

```indent
get Upper from strings
get Write from fs
get Sha256 from hash
say Upper "hello"           # → HELLO
```

---

## 8. Error Handling

```indent
do:
    flag "something broke"
catch as err:
    say "Error: " + err
lastly:
    say "Cleanup"
```

---

## 9. Built-in Functions

Key categories:
- **String**: `len`, `upper`, `lower`, `trim`, `replace`, `split`, `join`, `contains`, `slice`
- **List/Dict/Group**: `sort`, `reverse`, `append`, `pop`, `keys`, `values`, `has_key`, `count`
- **Math**: `abs`, `range`, `is_even`, `is_odd`, plus `math.*` module
- **Type**: `type_of`, `string`, `int`, `float`, `bool`, `int_or`, `float_or`
- **Time**: `time_now`, `time_format`, `time_parse`, `time_sleep`
- **Regex**: `regex_match`, `regex_search`, `regex_findall`, `regex_replace`, `regex_split`
- **Crypto**: `uuid`, `base64_encode`, `base64_decode`, `hash_sha256`
- **Path**: `glob`, `path_join`, `path_basename`, `path_dirname`
- **Functional**: `map`, `filter`, `enumerate`, `zip`
- **IO**: `file_read_text`, `file_write_text`, `file_sha256`
- **OS**: `os_getcwd`, `os_exists`, `os_list_dir`, `os_system`, `os_getenv`

See [quick-reference.md](quick-reference.md) for the complete list.

---

## 10. AIR Package Manager

```bash
air install colors          # Install from registry
air uninstall colors        # Remove
air update                  # Update all packages
air search json             # Search registry
air list                    # List installed
air info math               # Package details
```

---

## Commands

```bash
indent run file.ind          # Run a program
indent fmt file.ind          # Format code
indent check file.ind        # Check syntax
indent lint file.ind         # Lint code
indent repl                  # Interactive REPL
indent test tests/           # Run tests
indent --debug file.ind      # Debug with breakpoints
indent --update              # Update Indent
```

---

## Golden Rules

1. `func(args)` works everywhere — prefer it
2. Comments use `#!`, not `#`
3. Reassign with `is`, not `=`: `x is 42`
4. Type inference: `var x = 42` → int
5. Compound assignment: `x += 5`
6. Type conversion: `set x string`
7. Groups: `group([1,2,3])` for unique collections
8. `indent --update` keeps you current
