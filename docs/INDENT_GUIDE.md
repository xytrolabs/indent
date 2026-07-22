# Your Journey with Indent

> Indent is a language designed for **learning and building**. Its syntax uses indentation instead of braces — like Python, but with simpler keywords and fewer symbols. You can write scripts, web servers, GUI apps, and Discord bots, all in one language.

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

### Your First Program

Create a file called `hello.ind`:

```indent
say "Hello, World!"
```

Run it:

```bash
indent run hello.ind
# → Hello, World!
```

That's it. No `main` function, no semicolons, no boilerplate. Indent runs your code from top to bottom.

---

## 2. Variables — Storing Information

```indent
var name string = "Ada"
var age int = 28
var price float = 9.99
var active boolean = true
var anything dynamic = [1, "hello"]
var nothing empty
```

Indent has six main types: `string`, `int`, `float`, `boolean`, `dynamic`, and `empty`.

Change a variable with `is`:

```indent
name is "Grace"
age is 29
```

### Type Rules

| Type | Accepts | Example |
|---|---|---|
| `string` | Text only | `"Hello"` |
| `int` | Whole numbers | `42` |
| `float` | Decimal numbers | `3.14` |
| `boolean` | `true` or `false` | `true` |
| `dynamic` | Anything at all | `[1, "hi", true]` |
| `empty` | Nothing (null) | `empty` |
| `list` | Lists `[...]` | `[1, 2, 3]` |
| `dict` | Dictionaries `{...}` | `{"key": "val"}` |

---

## 3. Printing & Input — Talking to Your User

### Output

```indent
say "Hello"
say "Your score: " + 42
say upper("loud")        # "LOUD"
```

### Input

```indent
var name string = ask("What is your name? ")
var age int = ask("int", "How old are you? ")
```

> Use `ask("int", ...)` or `ask("float", ...)` for numeric input. Plain `ask("...")` always returns a string.

---

## 4. Functions — Your Own Commands

```indent
fun greet person
    say "Hello " + person + "!"

greet("Ada")
```

Parameters go on the same line (inline) or each on their own line:

```indent
fun add
    a
    b
    give a + b

var result int = add(10, 20)
```

### Returning Values

Use `give` to return a value:

```indent
fun max a b
    if a > b
        give a
    otherwise
        give b
```

### Calling Functions

Parentheses work **everywhere** — prefer them:

```indent
greet("Ada")             # ✅ Works in all contexts
var x int = add(10, 20)  # ✅ In assignments
say max(5, 10)           # ✅ In expressions
```

### Function References (v2.2+)

You can pass a function without calling it:

```indent
fun handler x
    say "Got: " + x

fun register fn
    say "Registered: " + string(fn)

register handler         # handler passed as value, not called!
```

---

## 5. Making Decisions — `if`, `or`, `otherwise`

```indent
var age int = 20

if age >= 18
    say "Adult"
or age >= 13
    say "Teenager"
otherwise
    say "Child"
```

Indent uses `or` (instead of `elif`) and `otherwise` (instead of `else`).

Conditions can use functions directly:

```indent
if starts_with(name, "A")
    say "Starts with A"

if len(items) > 0
    say "Has items"
```

### Match — Multiple Values

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

## 6. Loops — Doing Things Repeatedly

### Repeat N Times

```indent
repeat 5
    say "Hello!"        # prints 5 times
```

Inside a loop, `Reps` gives you the current count (starting at 0):

```indent
repeat 5
    say "Round " + string(Reps + 1)
```

### Repeat Over a List

```indent
var colors dynamic = ["red", "green", "blue"]
repeat color in colors
    say color
```

### Repeat Until a Condition

```indent
var x int = 0
repeat until x >= 10
    x is x + 1
```

### Loop Control

| Keyword | Action |
|---|---|
| `stop` | Exit the loop |
| `next` | Skip to next iteration |
| `reset` | Restart from the beginning |

---

## 7. Lists & Dictionaries

### Lists — Ordered Collections

Lists are **immutable** — operations return new lists:

```indent
var nums list = [1, 2, 3]
var mixed dynamic = [1, "hello", true]

# Access (zero-based)
var first int = nums[0]         # 1
var last int = nums[-1]         # 3

# Add items (creates a new list)
nums is nums + [4]              # [1, 2, 3, 4]
nums is append(nums, 5)         # [1, 2, 3, 4, 5]

# Modify in place
nums[1] is 10                   # [1, 10, 3, 4, 5]

# Slice
var part dynamic = nums[1:3]    # [10, 3]

# Common operations
var size int = len(nums)        # 5
var sorted dynamic = sort(nums) # sorted copy
var found boolean = contains(nums, 10)  # true
```

### Dictionaries — Key-Value Pairs

Also immutable — reassign with `is`:

```indent
var person dict = {"name": "Ada", "age": 28}

# Access
say person["name"]              # "Ada"
say person.name                 # "Ada" — dot notation works too!

# Modify in place
person["age"] is 29
person.city is "Paris"

# Check keys
var has boolean = has_key(person, "name")   # true
var k dynamic = keys(person)                # ["name", "age", "city"]
```

---

## 8. Classes — Your Own Types

Classes bundle data and behavior together. Fields are **public by default** — no getters or setters needed.

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
- `fun` declarations become **methods**
- No `self`/`this` keyword — access fields and methods directly by name

### Creating Objects

```indent
var ada dynamic = Person("Ada", 28)
var bob dynamic = Person("Bob", 25)
```

### Using Objects

```indent
ada.greet()           # "Hello, I'm Ada, age 28"
bob.birthday()        # "Happy birthday Bob!"
say ada.name          # "Ada"
ada.age is 29         # Modify field
```

### Inheritance

```indent
class Employee from Person
    var role string

    fun greet             # Override the parent's greet
        say "I'm " + name + ", " + role

var e dynamic = Employee("Bob", 35, "Engineer")
e.greet()                # "I'm Bob, Engineer"
```

---

## 9. Error Handling — When Things Go Wrong

### Do / Catch / Lastly

```indent
do:
    flag "Something went wrong!"
catch as err:
    say "Error: " + err
lastly:
    say "This always runs"
```

- `do:` — the code that might fail
- `catch as err:` — handles the error; `err` is the error message
- `lastly:` — always runs, even if there was no error (like `finally`)

### Raising Errors

Use `flag` to raise an error from anywhere:

```indent
fun divide a b
    if b == 0
        flag "Cannot divide by zero"
    give a / b
```

### Error Codes

Indent gives you specific error codes to help diagnose problems:

| Code | Meaning | Common Fix |
|---|---|---|
| `E001` | Type mismatch | Check what you're passing — use `type_of()` to inspect |
| `E002` | Function not found | Check spelling; available: say, ask, len, range, int, string... |
| `E003` | Import error | Module not found in search paths |
| `E004` | Syntax error | Check indentation, missing quotes, or unexpected characters |
| `E005` | Unwrap error | `.unwrap()` called on an error — handle the error first |
| `E006` | Undefined variable | Did you forget `var`? Or is there a typo? |
| `E007` | Division by zero | Guard with `if divisor != 0` before dividing |
| `E008` | Index out of range | Check list length with `len()` before indexing |
| `E009` | Key not found | Use `has_key(dict, key)` before accessing |
| `E010` | File not found | Use `os_exists(path)` to verify the file exists |
| `E011` | Invalid JSON | Check for missing quotes or commas in your JSON string |
| `E012` | Network error | Check the URL and your connection |

### Debugging

Run with `--debug` to step through your code:

```bash
indent --debug myprogram.ind
```

Debugger commands:
- `s` — step to the next line
- `c` — continue to the end
- `p expr` — print the value of an expression
- `b 10` — set a breakpoint at line 10
- `l` — show surrounding source code
- `q` — quit the debugger

---

## 10. Modules — Organizing Your Code

```indent
get math                    # import math.ind from current directory
get Pow from math           # import just one function
get RandInt from random as R  # import with an alias
```

### Where Indent Looks for Modules

1. Same directory as your script
2. Parent directories (walking up)
3. Directories in the `INDENT_PATH` environment variable
4. `~/.local/share/indent/site-packages/`

---

## 11. Built-in Functions

### String Operations
`upper(s)` `lower(s)` `trim(s)` `replace(text, from, to)` `split(text, sep)` `join(list, sep)` `starts_with(s, prefix)` `ends_with(s, suffix)` `contains(s, sub)` `slice(s, start, end)` `capitalize(s)` `title(s)` `find(s, sub)` `index(s, sub)` `len(s)`

### List & Dictionary
`len(coll)` `keys(dict)` `values(dict)` `items(dict)` `has_key(dict, key)` `sort(list)` `reverse(list)` `append(list, item)` `extend(list1, list2)` `pop(list)` `insert(list, idx, item)` `remove(list, value)` `contains(coll, item)` `count(coll, item)` `sum(list)` `enumerate(list)` `zip(list1, list2)` `range(end)` `range(start, end)` `range(start, end, step)`

### Conversion
`int(v)` `float(v)` `string(v)` `bool(v)` `type_of(v)` `int_or(s, fallback)` `float_or(s, fallback)`

### Math
`abs(n)` `is_even(n)` `is_odd(n)` `clamp(v, min, max)` `math_pow(base, exp)` `math_sqrt(n)` `math_sin(n)` `math_cos(n)` `math_abs(n)` `math_floor(n)` `math_ceil(n)`

### JSON
`json_loads(text)` `json_dumps(value)`

### Time
`time_now()` `time_sleep(seconds)`

### Random
`random_int(min, max)` `random_choice(list)` `random_shuffle(list)` `random_float()`

---

## 12. Web & Network

### HTTP Client

```indent
var data string = http_get("https://api.example.com/data")
var resp string = http_post_json("https://api.example.com/submit", "{\"key\": \"val\"}")
http_put_json(url, body)
http_patch_json(url, body)
http_delete(url)
```

### WebSocket Client

```indent
var id int = ws_connect("ws://localhost:8080")
ws_send_text(id, "Hello server!")
var msg string = ws_recv(id)
```

### HTTP Server

Serve a directory of static files:

```indent
http_serve_dir("./public", 8080)
say "Server running at http://localhost:8080"
```

---

## 13. File I/O

```indent
# Read
var text string = file_read_text("data.txt")

# Write
file_write_text("output.txt", "Hello, file!")

# Append
file_append_text("log.txt", "New entry\n")

# Check
if os_exists("data.txt")
    say "File exists"

# List directory
var files dynamic = os_list_dir(".")

# Other
os_getcwd()
os_mkdir("newfolder")
os_remove("oldfile.txt")
os_system("ls -la")
```

---

## 14. Python Interop

Call Python from Indent:

```indent
var result dynamic = python_eval("2 + 2")         # 4
python_exec("print('Hello from Python!')")
```

Access Python modules:

```indent
var sys_version dynamic = py.sys.version
var os_name dynamic = py.os.name
```

---

## 15. GUI Applications

Show HTML in a window:

```indent
gui_show_html("<h1>Hello!</h1><p>Rendered in a window.</p>")
```

---

## 16. The Result Type

Functions that can fail return a `Result`:

```indent
var r dynamic = ok("success")      # Create a success result
var e dynamic = err("failed")      # Create an error result

if is_ok(r)
    say "Success: " + unwrap(r)    # Extract the value

if is_err(e)
    say "Error: " + error_message(e)
```

---

## 17. Standard Packages

| Package | Purpose |
|---|---|
| `html` | HTML templating and forms |
| `csv` | CSV parsing and generation |
| `jsondb` | JSON file database |
| `config` | INI config file parsing |
| `json` | JSON encode/decode |
| `http` | HTTP client & server |
| `math` | Math constants and functions |
| `os` | OS and filesystem |
| `time` | Time utilities |
| `random` | Random number generation |
| `discord` | Discord bot framework |
| `colors` | Named color constants |

---

## 18. Tooling

### Package Manager

```bash
air search colors          # Search for packages
air install colors         # Install a package
air list                   # List installed packages
air publish name file      # Publish your package
```

### VS Code

- **F5** — Run current file
- **Syntax highlighting** — `indent-language` extension
- **File icons** — `indent-file-icons` extension
- **Snippets** — Type `fun`, `var`, `if`, `repeat` for templates

### Staying Current

```bash
indent --update              # Auto-update to the latest version
```

This pulls the latest code from GitHub, builds it, and replaces your current binary — no manual reinstall needed.

### CLI Cheat Sheet

```bash
indent run file.ind          # Run a script
indent repl                  # Interactive shell
indent check file.ind        # Syntax check only
indent fmt file.ind          # Auto-format code
indent test path/            # Run tests
indent new project-name      # Create project scaffold
indent --debug file.ind      # Run with debugger
indent lint file.ind         # Lint for issues
indent --update              # Update to latest version
```

---

## Golden Rules

1. **`func(args)` works everywhere** — in `say`, `if`, assignments, nested calls. Prefer it.
2. **`#!` for comments** — `#` alone is for hex colors like `#ff4d4d`
3. **Lists and dicts are immutable** — use `is` + `+` to "change" them
4. **Bare names in `var` are function calls** — use `string(param)` instead
5. **Imports resolve** from same dir → `INDENT_PATH` → `~/.local/share/indent/site-packages/`

---

Next: [Quick Reference](quick-reference.md) · [Built-in Functions](builtins-reference.md) · [Classes Deep Dive](classes-design.md)
