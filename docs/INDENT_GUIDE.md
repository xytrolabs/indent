# Indent — The Complete Language Reference

> **Version**: 2.2.0  |  **Paradigm**: imperative, functional, object-oriented  |  **Typing**: static with dynamic escape hatch

---

## Table of Contents
1. [Getting Started](#1-getting-started)
2. [Basic Syntax](#2-basic-syntax)
3. [Variables & Types](#3-variables--types)
4. [Functions](#4-functions)
5. [Control Flow](#5-control-flow)
6. [Loops](#6-loops)
7. [Data Structures](#7-data-structures)
8. [Imports & Modules](#8-imports--modules)
9. [Error Handling](#9-error-handling)
10. [Built-in Functions](#10-built-in-functions)
11. [Expression Rules](#11-expression-rules)
12. [Classes](#12-classes)
13. [Standard Packages](#13-standard-packages)
14. [Tooling](#14-tooling)

---

## 1. Getting Started

### Installation

| Platform | Command |
|---|---|
| Debian/Ubuntu | `curl -fsSL https://.../install-pkg.sh \| sudo bash` |
| Fedora/RHEL | Same script — auto-detects dnf |
| Arch | `yay -S indent` |
| macOS | `brew install xytrolabs/indent/indent` |
| Windows | `irm https://.../install.ps1 \| iex` |
| Any Linux | `curl -fsSL https://.../install.sh \| bash` |

### CLI Commands

```bash
indent run file.ind       # Run script
indent repl               # Interactive shell
indent check file.ind     # Syntax check
indent fmt file.ind       # Format code
indent test path/         # Run tests
indent new project-name   # Create project scaffold
indent --debug file.ind   # Run with debugger
indent lint file.ind      # Lint
```

---

## 2. Basic Syntax

### Comments
```indent
#! Single-line comment

#!*
Multi-line
comment block
#!*
```

> **CRITICAL**: `#` alone is a **hex color literal** (e.g., `#ff4d4d`), NOT a comment. Always use `#!`.

### Output
```indent
say "Hello, World!"
say "Count: " + 42
say upper("loud")
```

### Input
```indent
var name string = ask("What is your name? ")
var age int = ask("How old are you? ")
```

---

## 3. Variables & Types

### Declaration
```indent
var name string = "Ada"         # string
var count int = 42               # int
var price float = 3.14           # float
var active boolean = true        # boolean
var anything dynamic = [1, "hi"] # dynamic
var nothing empty                # empty (null)
var nums list = [1, 2, 3]       # list (typed list)
var info dict = {"key": "val"}  # dict (typed dictionary)
```

### Reassignment
```indent
count is 43
name is "Grace"
```

### Type Rules
| Type | Accepts |
|---|---|
| `string` | strings only |
| `int` | integers only (not bool) |
| `float` | int or float |
| `boolean` | true/false |
| `dynamic` | anything |
| `empty` | only empty |
| `list` | list values `[...]` |
| `dict` | dictionary values `{...}` |

---

## 4. Functions

### Definition
```indent
# Inline params
fun greet person
    say "Hello " + person + "!"

# Multi-line params
fun add
    a
    b
    give a + b
```

### Return
```indent
fun max a b
    if a > b
        give a
    otherwise
        give b
```

### Calling
```indent
greet("Ada")                   # ✅ Parens — works everywhere
var result int = add(10, 20)   # ✅ In var declarations
say max(5, 10)                 # ✅ In expressions

greet "Ada"                    # ✅ Space-separated, standalone only
```

> **Rule**: `func(args)` works in ALL contexts. Prefer it.

### Function References (v2.2.0)
Functions can be passed as values without being called:
```indent
fun handler x
    say "Got: " + x

fun register fn
    var name string = string(fn)
    say "Registered: " + name

register handler              # ✅ handler passed as reference, not called!
```
Previously bare function names were auto-called with zero arguments. Now they return a `Func` reference that can be converted to a string for `call_func`.

---

## 5. Control Flow

### If / Or / Otherwise
```indent
if age >= 18
    say "Adult"
or age >= 13
    say "Teenager"
otherwise
    say "Child"
```

Indent uses `or` (not `elif`) and `otherwise` (not `else`).

### Conditions with Functions
```indent
if starts_with(name, "A")     # ✅ Parens work
    say "Starts with A"

if len(items) > 0             # ✅ Operators work too
    say "Has items"
```

### Match
```indent
match color
    "red"
        say "Stop"
    "green"
        say "Go"
    otherwise
        say "Unknown"
```

---

## 6. Loops

### Repeat N
```indent
repeat 5
    say "Iteration " + string(Reps + 1)
```

### Repeat Over List
```indent
var colors dynamic = ["red", "green", "blue"]
repeat color in colors
    say color
```

### Repeat Until
```indent
var x int = 0
repeat until x >= 10
    x is x + 1
```

### Loop Control
| Keyword | Action |
|---|---|
| `stop` | Break out |
| `next` | Skip to next iteration |
| `reset` | Restart from 0 |

### Loop Variables
- `Reps` — iteration counter (0-indexed)
- `Item` — current item (or custom name)

---

## 7. Data Structures

### Lists

Lists hold ordered sequences of values. They are immutable — operations return **new** lists.

```indent
# Declaration
var nums list = [1, 2, 3]
var mixed dynamic = [1, "hello", true]
var empty list = []

# Access (zero-based, negative indexes count from end)
var first int = nums[0]          # 1
var last int = nums[-1]          # 3

# Modification (creates new list)
nums is nums + [4]               # [1, 2, 3, 4]
nums is append(nums, 5)          # [1, 2, 3, 4, 5]

# In-place element mutation
nums[1] is 10                    # [1, 10, 3, 4, 5]

# Slice assignment
nums[1:3] is [7, 8]             # [1, 7, 8, 4, 5]

# Slicing
var slice dynamic = nums[2:4]    # [8, 4]
var copy dynamic = nums[:]       # full copy

# Common operations
var size int = len(nums)         # 5
var sorted dynamic = sort(nums)  # [1, 4, 5, 7, 8]
var found boolean = contains(nums, 7)  # true
```

### Dictionaries

Dictionaries store key-value pairs. They are also immutable — use `is` to reassign.

```indent
# Declaration
var person dict = {"name": "Ada", "age": 28}
var empty dict = {}

# Access (bracket or dot notation)
var name string = person["name"]   # "Ada"
say person.name                    # "Ada" — dot notation

# In-place key mutation
person["age"] is 29
person.city is "Paris"

# Add new keys
var updated dynamic = dict_set(person, "country", "UK")

# Nested modification: get → modify → reassign
var inner dynamic = dict["key"]
inner["sub"] is "value"
dict["key"] is inner

# Common operations
var k dynamic = keys(person)       # ["age", "city", "name"]
var has boolean = has_key(person, "name")  # true
var size int = len(person)         # 3
```

### Common Ops
| Op | Syntax |
|---|---|
| Length | `len(list)` |
| Keys | `keys(dict)` |
| Has key | `has_key(dict, key)` |
| Contains | `contains(list, item)` |
| Sort | `sort(list)` |
| Slice | `slice(list, start, end)` |

---

## 8. Imports & Modules

```indent
get math                        # Whole module
get Pow from math               # Specific function
get RandInt from random as R    # With alias
```

**Resolution order**: Same dir → `INDENT_PATH` → `~/.local/share/indent/site-packages/`

---

## 9. Error Handling

```indent
do:
    flag "Something broke"
catch as err:
    say "Error: " + err
lastly:
    say "Cleanup"
```

---

## 10. Built-in Functions

### I/O
`say`, `ask(prompt)`, `ask(type, prompt)`

### String
`upper(s)`, `lower(s)`, `trim(s)`, `replace(text, from, to)`, `split(text, sep)`, `join(list, sep)`, `starts_with(s, prefix)`, `ends_with(s, suffix)`, `contains(s, sub)`, `slice(s, start, end)`, `len(s)`, `capitalize(s)`, `title(s)`

### List/Dict
`len(coll)`, `keys(dict)`, `values(dict)`, `sort(list)`, `reverse(list)`, `append(list, item)`, `contains(coll, item)`, `has_key(dict, key)`, `range(end)`, `range(start, end, step)`

### JSON
`json_loads(text)`, `json_dumps(value)`

### HTTP
`http_get(url)`, `http_post_json(url, body)`, `http_put_json(url, body)`, `http_patch_json(url, body)`, `http_delete(url)`, `http_serve_dir(path, port)`

### File I/O
`file_read_text(path)`, `file_write_text(path, text)`, `file_append_text(path, text)`

### OS
`os_getcwd()`, `os_exists(path)`, `os_list_dir(path)`, `os_mkdir(path)`, `os_remove(path)`, `os_system(cmd)`

### Math
`math_abs(n)`, `math_pow(base, exp)`, `math_sqrt(n)`, `math_floor(n)`, `math_ceil(n)`, `math_sin(n)`, `math_cos(n)`

### Time
`time_now()`, `time_sleep(ms)`

### Conversion
`int(v)`, `float(v)`, `string(v)`, `bool(v)`, `type_of(v)`

---

## 11. Expression Rules

| Context | `func(args)` | `func arg` |
|---|---|---|
| `say` | ✅ | ❌ |
| `if` condition | ✅ | ❌ |
| `x is` assignment | ✅ | ❌ |
| `var x =` | ✅ | ✅ |
| `give` | ✅ | ✅ |
| Standalone | ✅ | ✅ |

### Common Pitfalls
```indent
# ❌ Bare param in var — treated as function call
fun bad text
    var x string = text         # ERROR

# ✅ Fix
fun good text
    var x string = string(text) # OK
    var y string = "" + text    # OK
```

### Hex Colors
`#ff4d4d`, `#3b82f6` — 3/4/6/8 hex digits. For comments use `#!`.

---

## 12. Classes

Indent classes bundle data and behavior together. Fields are **public by default** — accessible from anywhere without getters or setters.

### Defining a Class
```indent
class Person
    var name string
    var age int

    fun greet
        say "Hello, I'm " + name + ", age " + string(age)

    fun birthday
        age is age + 1
        say "Happy birthday " + name + "!"
```

- `var` declarations become both **constructor parameters** and **instance fields**
- `fun` declarations become **methods** scoped to the class
- No `self`/`this` keyword — fields and methods are accessed directly by name

### Instantiation
```indent
var ada dynamic = Person("Ada", 28)
var bob dynamic = Person("Bob", 25)
```

### Method Calls
```indent
ada.greet()           # "Hello, I'm Ada, age 28"
bob.birthday()        # "Happy birthday Bob!"
```

### Field Access
```indent
say ada.name          # "Ada"
ada.age is 29         # Modify field
```

### Class Design Rules
| Rule | Details |
|---|---|
| Constructor | Matches positional args to `var` fields in order |
| Methods | Use `fun` inside the class — same syntax as regular functions |
| Fields | `var` declarations; become constructor params + instance fields |
| No `self` | Fields and methods accessed by name directly |
| Return | Methods use `give` to return values |

---

## 13. Standard Packages

| Package | Purpose |
|---|---|
| `html` | HTML templating, forms, pages |
| `csv` | CSV parsing and generation |
| `jsondb` | JSON file database |
| `config` | INI config parsing |
| `json` | JSON encode/decode |
| `http` | HTTP client |
| `math` | Math constants and functions |
| `os` | OS and filesystem |
| `time` | Time and sleep |
| `random` | Random numbers |
| `discord` | Discord bot framework |
| `colors` | Named color constants |

---

## 14. Tooling

### Package Manager (air)
```bash
air search color        # Search
air install colors      # Install
air list                # List installed
air publish name file   # Publish
```

### VS Code
- **F5** — Run current file
- **Syntax highlighting** — `indent-language` extension
- **File icons** — `indent-file-icons` extension
- **Snippets** — `fun`, `var`, `if`, `repeat` triggers
