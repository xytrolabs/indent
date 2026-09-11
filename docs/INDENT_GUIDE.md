# Indent — The Language Guide

> **Indent** (formerly **Aether**) is a programming language built for *learning
> and building*. It uses **indentation instead of braces** (like Python) but
> with a deliberately small set of keywords and symbols so beginners can focus
> on ideas, not punctuation. Despite that simplicity it is **general purpose**:
> you can write scripts, CLI tools, web servers, GUI apps, Discord bots, and
> even 2D games — all in one language.
>
> This guide is the primary, in-depth manual. It teaches concepts *and*
> reference details, with runnable examples throughout. For the complete list of
> every built-in function see [`builtins-reference.md`](builtins-reference.md);
> for a fast cheat sheet see [`quick-reference.md`](quick-reference.md).

**Contents**

1. [Installing & running Indent](#1-installing--running-indent)
2. [Hello, World & the mental model](#2-hello-world--the-mental-model)
3. [Variables & the type system](#3-variables--the-type-system)
4. [Lists, dictionaries & groups](#4-lists-dictionaries--groups)
5. [Strings & text](#5-strings--text)
6. [Numbers & math](#6-numbers--math)
7. [Functions](#7-functions)
8. [Classes & objects](#8-classes--objects)
9. [Generators (`yield`)](#9-generators-yield)
10. [Control flow](#10-control-flow)
11. [Loops & iteration](#11-loops--iteration)
12. [Comprehensions & expressions](#12-comprehensions--expressions)
13. [Imports & the module system](#13-imports--the-module-system)
14. [Error handling](#14-error-handling)
15. [File I/O & the operating system](#15-file-io--the-operating-system)
16. [Data formats: JSON, TOML, YAML, CSV, SQLite](#16-data-formats)
17. [Networking: HTTP & WebSockets](#17-networking-http--websockets)
18. [Concurrency](#18-concurrency)
19. [Colors](#19-colors)
20. [GUI & games](#20-gui--games)
21. [The standard library](#21-the-standard-library)
22. [The AIR package manager](#22-the-air-package-manager)
23. [Command line & tooling](#23-command-line--tooling)
24. [Running untrusted code with `--safe`](#24-running-untrusted-code-with---safe)
25. [Best practices & golden rules](#25-best-practices--golden-rules)
26. [Troubleshooting](#26-troubleshooting)

---

## 1. Installing & running Indent

### System requirements

Indent is a single self-contained native binary (written in Rust). It has no
runtime dependencies on Linux/macOS. The optional GUI/game helpers
(`indent-gui`, `indent-ingame`) additionally need `gcc`, `gtk3` and `webkit2gtk`
and are built automatically when you install.

### Linux (any distro)

```bash
curl -fsSL https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.sh | bash
```

This installs the `indent` binary (and `air`, the package manager) to
`~/.local/share/indent/bin/` and symlinks them into `~/.local/bin/`.

### macOS

```bash
brew install xytrolabs/indent/indent
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.ps1 | iex
```

The Windows installer downloads a prebuilt `indent.exe` from the GitHub
Release for your version, but if no binary is published yet it **falls back to a
source build** and bootstraps Rust/MSVC automatically (via `winget`/`rustup`)
for you. See [`windows.md`](windows.md) for a full walkthrough.

### Building from source

```bash
cd indent-native
cargo build --release
./target/release/indent --version
```

### Verifying the install

```bash
indent --version     # e.g. "Indent 2.2.0"
indent repl          # opens an interactive prompt; type quit to leave
```

---

## 2. Hello, World & the mental model

Create `hello.ind`:

```indent
say "Hello, World!"
```

Run it:

```bash
indent run hello.ind        # "indent hello.ind" also works
```

Prints:

```
Hello, World!
```

### The core mental model

- **Indentation defines blocks.** A deeper indent starts a nested block; a
  shallower indent closes it. There are no `{` `}` or `end` markers.
- **A script is a list of statements** executed top to bottom. Statements that
  run a value *at the top level* also print their result, which makes Indent
  feel interactive even in a file (this is how `5` prints `5`).
- **Functions receive arguments **by value**. If a function changes a list or
  dict you pass it, the caller's copy is *not* changed — the function must
  return the new container and you must reassign it.
- **Two assignment forms.** `var x is <expr>` *declares* (introduces) a
  variable; `x = <expr>` *reassigns* an existing one. Both spellings are also
  accepted the other way around for compatibility, but the canonical style is:
  `var x is 42` to declare, `x = 43` to reassign.

```indent
var greeting = "Hello"
greeting = "Hi"            #! reassign an existing var (canonical)
var other is "Yo"          #! 'is' declare (also canonical, older style)
say greeting + " " + other #! → "Hi Yo"
```

---

## 3. Variables & the type system

Indent has nine types: `string`, `int`, `float`, `boolean`, `list`, `group`,
`dict`, `dynamic`, and `empty`.

### Declaring variables

```indent
#! Type inference — the preferred style. The type is taken from the value.
var name = "Ada"           #! string
var age = 28               #! int
var pi = 3.14              #! float
var flag = true            #! boolean
var nums = [1, 2, 3]       #! list
var tags = {"a": 1}        #! dict

#! Explicit type — useful when the value type is not obvious.
var data dynamic = readConfig()   #! dynamic holds anything
var scores list = [95, 87]        #! typed list
var nothing empty                 #! a variable with no value yet
```

### Reassignment

```indent
var age = 28
age = 29                  #! canonical reassign
age is 30                 #! 'is' reassign (older/also accepted)
```

### Compound assignment

Shorthand that reads, computes, and reassigns in one step:

```indent
var n = 10
n += 5     #! n = n + 5   → 15
n -= 3     #! → 12
n *= 2     #! → 24
n /= 4     #! → 6
n %= 4     #! → 2

#! Lists and dicts support += to merge.
var a = [1, 2]
a += [3]            #! → [1, 2, 3]
var d = {"x": 1}
d += {"y": 2}       #! → {"x": 1, "y": 2}
```

### Type conversion

Convert a value from one type to another with the `set` keyword:

```indent
var x = "42"
set x int              #! string→int      → 42

var y = 3
set y float            #! int→float       → 3.0

var z = 0
set z boolean          #! 0→false; any non-zero→true

var s = 42
set s string           #! number→string   → "42"

var data = [1, 2, 2, 3]
set data group         #! list→group (dedupe) → {1, 2, 3}
```

> 💡 The builtins `int(x)`, `float(x)`, `string(x)`/`str(x)`, `bool(x)` do the
> same thing as function calls and can be used inside larger expressions, where
> the `set` keyword cannot. `int_or(x, fallback)` and `float_or(x, fallback)`
> convert but return `fallback` if conversion fails instead of erroring.

### The `empty` / `null` value

`empty` (also spelled `null`) represents "no value". You can test for it with
`is_missing(x)` and substitute a fallback with `default(x, fb)` or
`coalesce(a, b, c, ...)` (first non-empty).

```indent
var maybe empty
is_missing(maybe)          #! → TRUE
default(maybe, "fallback") #! → "fallback"
coalesce(empty, empty, 7)  #! → 7
```

> ⚠️ Accessing a key that does not exist in a dict, or an index out of range,
> throws an error. Guard optional lookups with `has_key(d, "k")` first (see
> [§4](#4-lists-dictionaries--groups)).

### Truthiness

`false`, `0`, `""` (empty string), `empty`/`null`, and empty containers are
falsey; everything else (including the string `"false"` and the number `0.0`'s
string forms) is truthy. Use `bool(x)` to force a decision.

---

## 4. Lists, dictionaries & groups

### Lists — ordered, mutable, indexed by number

```indent
var fruits = ["apple", "banana", "cherry"]
fruits[0]                    #! → "apple"
fruits[-1]                   #! → "cherry" (negative indexes from the end)
len(fruits)                  #! → 3

#! Build a new list with an item appended — containers are value-based.
fruits is append(fruits, "date")     #! reassign!  → 4 items
fruits = extend(fruits, ["fig", "grape"])   #! merge
var third = insert(fruits, 2, "kiwi")       #! insert at index 2
```

> 🔑 **Containers are immutable-by-value.** `append`, `extend`, `insert`,
> `remove`, `pop`, `sort`, `reverse`, `dict_set`, `dict_remove`, etc. all
> return a **new** container. You must capture the return value:
> `l = append(l, x)`. Native `.method` calls (`fruits.append("x")`) follow the
> same rule.

Membership and searching:

```indent
contains(fruits, "kiwi")     #! → TRUE  (also works on strings/dicts)
index(fruits, "date")        #! → 3  (or -1 if absent)
count(fruits, "kiwi")        #! → occurrences
slice(fruits, 1, 3)          #! → ["banana", "cherry"]  (start:end)
reverse(fruits)              #! reversed copy
sort([3, 1, 2])              #! → [1, 2, 3]
```

Looping and transforming:

```indent
sum([1, 2, 3])               #! → 6
min([3, 1, 2])               #! → 1
max([3, 1, 2])               #! → 3
any([false, true])           #! → TRUE
all([true, true])            #! → TRUE

map([1, 2, 3], "double")     #! apply builtin 'double' to each
filter([1, 2, 3, 4], "is_even")
enumerate(["a", "b"])        #! → [[0, "a"], [1, "b"]]
zip([1, 2], ["a", "b"])      #! → [[1, "a"], [2, "b"]]
range(5)                     #! → [0, 1, 2, 3, 4]
range(1, 5)                  #! → [1, 2, 3, 4]
```

> 🔍 `map`/`filter`/`reduce`/`group_by`/`takewhile` accept a **builtin name**
> (a string) as the predicate/function, not a user function.

### Dictionaries — key/value maps

```indent
var person = {"name": "Ada", "age": 28}
person["name"]               #! → "Ada"
person.name                  #! dot notation (same thing)
person["age"] = 29           #! update via index (returns new dict)

keys(person)                 #! → ["name", "age"]
values(person)               #! → ["Ada", 29]
items(person)                #! → [["name", "Ada"], ["age", 29]]

has_key(person, "name")      #! → TRUE   — ALWAYS guard optional access
```

Functional dict updates return a new dict:

```indent
var p2 = dict_set(person, "city", "London")     #! add/replace a key
var p3 = dict_remove(person, "age")             #! remove a key
var p4 = dict_update(person, {"age": 30})       #! merge updates
dict_get(person, "missing", "fallback")         #! safe lookup w/ default
```

### Groups — unique, ordered collections

A **group** holds unique values in insertion order. Build one with `group([...])`
— a "list with no duplicates".

```indent
var colors = group(["red", "blue", "red"])   #! → {"red", "blue"}
var more   = group(["green", "blue"])
len(colors)                                  #! → 2
contains(colors, "red")                      #! → TRUE

var all = colors + more        #! union → {"red", "blue", "green"}

#! Set algebra (the set_* helpers take and return groups):
set_union(colors, more)
set_intersection(colors, more) #! → {"blue"}
set_difference(colors, more)   #! → {"red"}
set_add(colors, "yellow")      #! returns a new group
set_remove(colors, "red")
set_contains(colors, "red")
```

> ⚠️ **`set` is NOT how you build a group.** `set` is the *type-conversion*
> keyword (`set x string`, `set x int`), and `set_*` are the group helper
> functions above. To create a unique collection, use `group([...])`. (The old
> `set([...])` form still works as a deprecated alias but is not recommended.)

---

## 5. Strings & text

Strings are Unicode text. Write them in double quotes (single quotes work too).
Concatenate with `+`:

```indent
var name = "Ada"
say "Hello " + name             #! concatenation
say "Hello " + name + "!"
```

### String interpolation

Put a variable name between percent signs inside any string and its value is
substituted (rendered as text). Works in double- **and** single-quoted strings,
anywhere a string is used (not just `say`):

```indent
var name = "Ada"
var n = 42
say "Hi %name%, n=%n%"     #! → "Hi Ada, n=42"
var msg = "Hello %name%"   #! interpolation happens in any string
say msg                    #! → "Hello Ada"
say 'single quotes %name% too'
```

Rules to know:

- **Simple variable names only** — not expressions or member access. `"%d.k%"`
  is left literally as `%d.k%`; use `"%d%"` (the whole value) or concatenate
  (`"x " + d.k`).
- **Any value type** works — strings, ints, floats, lists, dicts — each renders
  as text (a dict prints its `{...}` form).
- **Undefined names and stray `%` are left as-is** (no error): `"%nope%"` prints
  literally, and `"100% done"` / `"%%"` are unchanged.
- There is **no escape** for a literal `%name%` when `name` exists — build such
  text with concatenation, or rename the placeholder.
- For **positional/named templates**, use `format(...)` / `sformat(...)` below.

### Quotes & escapes

Double quotes are the default; single quotes also work. Recognised escapes:
`\n` (newline), `\t` (tab), `\"`, `\'`, and `\\`. Any other `\x` becomes just `x`.

```indent
say "line1\nline2"        #! newline
say "col1\tcol2"          #! tab
say "quote: \"hi\""       #! escaped quote
say 'it\'s fine'
```

### Case, trimming & padding

```indent
upper("hello")            #! → "HELLO"
lower("HELLO")            #! → "hello"
capitalize("hello world") #! → "Hello world"
title("hello world")      #! → "Hello World"
swapcase("aBc")           #! → "AbC"

trim("  hi  ")            #! → "hi"
lstrip("  hi")            #! → "hi"
rstrip("hi  ")            #! → "hi"

pad_left("5", 3, "0")     #! → "005"
pad_right("5", 3, "0")    #! → "500"
repeat_str("ab", 3)       #! → "ababab"
```

### Search, replace & slicing

```indent
len("hello")              #! → 5
starts_with("hello", "he")  #! → TRUE
ends_with("hello", "lo")    #! → TRUE
contains("hello", "ell")    #! → TRUE
find("hello", "ll")         #! → 2  (or -1 if absent)

replace("aaa", "a", "b")    #! → "bbb"
split("a,b,c", ",")         #! → ["a", "b", "c"]
split("a b c")              #! split on whitespace if no separator
join(["a", "b"], "-")       #! → "a-b"
slice("hello", 1, 3)        #! → "el"

str_zfill("7", 3)              #! → "007"
str_removeprefix("file.txt", "file")   #! → ".txt"
str_removesuffix("file.txt", ".txt")   #! → "file"
str_splitlines("a\nb")                #! → ["a", "b"]
str_partition("a=b", "=")             #! → ["a", "=", "b"]
```

### Formatting templates

```indent
format("{0} and {1}", "x", "y")        #! positional → "x and y"
sformat("{a}-{b}", "a", 1, "b", 2)     #! named      → "1-2"
```

> 🔍 Strings also expose the same operations as `.method` calls:
> `"hello".upper()`, `"a,b".split(",")`, `"hi".contains("i")`, and so on. They
> are sugar for the same functional builtins above.

---

## 6. Numbers & math

Indent has two numeric types: `int` (whole) and `float` (fractional).

```indent
abs(-5)                #! → 5
inc(5)                 #! → 6  (inc(x, step) with a step)
dec(5)                 #! → 4
clamp(11, 0, 10)       #! → 10
between_int(5, 0, 10)  #! → TRUE
is_even(4)             #! → TRUE
is_odd(4)              #! → FALSE
```

The `math` **standard module** mirrors the `math_*` builtins and is the
idiomatic way to do advanced math:

```indent
get Sqrt from math     #! or: import math; math.Sqrt(...)
get Pow from math
get Floor from math
get Round from math

Sqrt(16)               #! → 4.0
Pow(2, 10)             #! → 1024.0
Floor(3.7)             #! → 3.0
Round(3.14159, 2)      #! → 3.14
```

Raw builtins (used in larger expressions):

```indent
math_sqrt(16)          #! → 4.0
math_pow(2, 10)        #! → 1024.0
math_floor(3.7)        #! → 3.0
math_ceil(3.2)         #! → 4.0
math_round(3.14159, 2) #! → 3.14
math_abs(-5)
math_sin(0) / math_cos(0) / math_tan(0)
math_log(8, 2)         #! log base 2 of 8 → 3.0
math_exp(1)            #! e^1
math_factorial(5)      #! → 120 (int)
math_gcd(12, 18)       #! → 6
math_lcm(4, 6)         #! → 12
math_pi / math_e / math_tau    #! constants
```

Bitwise operators work on ints: `&` (and), `|` (or), `^` (xor), `~` (not),
`<<` / `>>` (shifts).

```indent
6 & 3     #! → 2   (110 & 011 = 010)
6 | 3     #! → 7
1 << 4    #! → 16
```

---

## 7. Functions

### Defining & calling

```indent
fun greet person
    say "Hello, " + person

greet "Ada"            #! space-separated call — canonical
greet("Ada")           #! parenthesized call — also works
```

### Return values

`give` returns a value (Indent has no `return` keyword):

```indent
fun add a b
    give a + b

var total = add 2 3      #! → 5
```

> 🔑 A bare function call as a *non-final* statement can swallow the following
> line in some parser edge cases. If a statement's *only* purpose is its side
> effect (and it is not the last statement in a block), you can force-call it
> with `if TRUE: <call>` — but the safest pattern is to keep side-effecting
> calls as the final statement of their block.

### Default parameters

```indent
fun greet name = "World"
greet            #! → "Hello, World"
greet "Ada"      #! → "Hello, Ada"
```

### Return type annotation

```indent
fun add a b as int
    give a + b
```

### Function references

A function name is also a value you can store and pass around:

```indent
fun double x
    give x * 2

var d = double      #! bind the function value
say d(21)           #! → 42
```

You can capture a builtin by name with `get <builtin>`:

```indent
get len
len([1, 2, 3])       #! → 3
```

### Lambda (anonymous) functions

```indent
var double = fn(x): x * 2
say double(5)                #! → 10

#! Lambdas compose cleanly with collection builtins where a builtin isn't enough
#! — but note map/filter accept builtin *names*, not lambdas, as the callable.
```

### Varargs

`...name` collects extra positional arguments into a list:

```indent
fun total ...nums
    give sum(nums)

total 1 2 3 4      #! → 10
```

### Calling by name (dynamic dispatch)

`call_func` invokes a function whose name is only known at runtime (works with
a string name or a bound `Func` value):

```indent
var op = "double"
call_func op 21      #! → 42   (calls the function named "double")
```

---

## 8. Classes & objects

Classes bundle data (variables) and behavior (functions). Construct an instance
by calling the class name with the constructor arguments in declaration order.

```indent
class Person
    var name string
    var age int
    fun greet
        say "I'm " + name

var p dynamic = Person "Ada" 28
p.greet()          #! → I'm Ada
```

Access fields with dot notation and call methods with `()`:

```indent
p.name              #! → "Ada"
p.age               #! → 28
```

### Inheritance

A class inherits from another with `class Child from Parent`:

```indent
class Animal
    var name string
    fun speak
        say name + " makes a sound"

class Dog from Animal
    fun speak
        say name + " barks"

var d = Dog "Rex"
d.speak()          #! → Rex barks
```

### Special methods (natural names)

Instead of Python's `__str__` / `__add__` / `__eq__`, Indent uses
plain-English method names the runtime calls **automatically** in the matching
situation:

| Method name | Triggered by | Purpose |
|---|---|---|
| `to_string` | `say`, `print`, string conversion | human-readable form |
| `add` | `+` | combine two instances |
| `subtract` / `multiply` / `divide` | `-` `*` `/` | arithmetic |
| `equals` | `==` and `!=` | equality |
| `len` | `len(x)` | length/size |
| `get_item` | `x[key]` / `x.key` | indexed access |
| `contains` | `contains(x, y)` | membership test |

Example — a math `Vector`:

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
say Vector(3, 4) == Vector(3, 4)  #! → TRUE
```

### `dataclass`

`dataclass Name` is like `class`, but it **auto-generates** `to_string` and
`equals` from its fields:

```indent
dataclass Point
    var px int
    var py int

say Point(1, 2)                  #! → Point(px: 1, py: 2)
say Point(1, 2) == Point(1, 2)   #! → TRUE
```

---

## 9. Generators (`yield`)

Any function that contains `yield` is a **generator**: it produces a sequence
of values lazily, one per `yield`, without building a list up front.

```indent
fun countdown n
    yield n
    yield n - 1
    yield n - 2

for x in countdown 3    #! 3, 2, 1
    say x
```

Materialize a generator into a list with `to_list`, and test with
`is_generator`:

```indent
to_list(countdown 3)     #! → [3, 2, 1]
var g = countdown 3
is_generator(g)          #! → TRUE
```

> 💡 Generators are the memory-efficient way to represent large or infinite
> sequences. Because they are lazy, you can `for` over a generator that would
> be impractical to materialize.

---

## 10. Control flow

### `if` / `or` / `otherwise`

```indent
if score >= 90
    say "A"
or score >= 80            #! else-if (the keyword is 'or', NOT 'elif')
    say "B"
otherwise                 #! else
    say "F"
```

### `match` / `case`

```indent
match day:
    case "mon":
        say "Monday"
    case "fri":
        say "TGIF!"
    otherwise:
        say "Another day"
```

### Ternary expressions

```indent
var label = "big" if n > 10 else "small"
```

### Chained comparisons

```indent
if 0 < x < 10
    say "single digit (0 excluded)"
```

### Membership & empty tests

`is` only declares/reassigns — it is **not** a comparison operator. For tests
use `in` (membership), `==`, or `is_missing()`:

```indent
"a" in list            #! TRUE if 'a' is in the list/group (or a dict key / substring)
"a" not in list        #! negated membership
contains(list, "a")    #! same thing (list, dict-keys, group, or string)
has_key(d, "k")        #! dictionary key check
is_missing(x)          #! TRUE when x is empty/absent
x == empty             #! also tests for the empty value
```

---

## 11. Loops & iteration

Indent offers several loop shapes for clarity and intent.

### Counted loop

```indent
repeat 5
    say "tick"
```

### Iterating a collection

```indent
repeat item in list
repeat item in my_group
repeat item in my_dict        #! yields keys
for item in list              #! 'for' is an alias of 'repeat ... in'
```

### Conditional loop

```indent
var done = false
repeat until done
    #! ... do work, set done = true to leave ...
```

### Loop control keywords

| Keyword | Meaning |
|---|---|
| `stop` | break out of the loop immediately |
| `next` | skip to the next iteration (continue) |
| `reset` | restart the loop from the beginning |

```indent
repeat item in range(10)
    if item == 2
        next        #! skip 2
    if item == 7
        stop        #! leave at 7
    say item
```

---

## 12. Comprehensions & expressions

Build lists, dicts and groups compactly from an existing collection.

```indent
var nums = [1, 2, 3, 4]

[x * 2 for x in nums]            #! → [2, 4, 6, 8]
[x for x in nums if is_even x]   #! list comprehension w/ filter
[x * 2 for x in nums if x > 1]   #! map + filter together

#! Dict comprehension
var pairs = {"a": 1, "b": 2}
{k: v * 10 for k, v in items pairs}   #! → {"a": 10, "b": 20}

#! Groups have no literal or comprehension syntax of their own — build with group():
group([x for x in [1, 1, 2, 2, 3]])   #! → {1, 2, 3}
```

Operator precedence and expression notes:

- Arithmetic: `* / %` bind tighter than `+ -`.
- Comparison/equality are non-associative except for chained comparisons
  (`0 < x < 10`).
- Boolean: use `and` and `not` (word forms). `or` is **not** a boolean operator —
  it starts the next branch of an `if` chain (`if … or cond … otherwise`).
  `&&`, `||`, and `!` are not supported; negate with `not`, combine with `and`.

---

## 13. Imports & the module system

### Import syntax

```indent
get math                     #! make the whole 'math' module available
import math                  #! 'import' is an alias for 'get'

get Pow from math            #! import a single function
get RandInt from random as R #! import + rename

#! A PascalCase package such as the std lib:
get Upper from strings
get Write from fs
```

> 💡 User-defined and imported functions take precedence over builtins at call
> time. Std-library functions are PascalCase precisely so they never collide
> with the lowercase builtins.

### How modules are found

Modules resolve **in order**:

1. The importing script's directory and its **parent directories** (walking
   up), for `name.ind`, `name.glo`, or `name/__init__.ind`.
2. Every directory in the `INDENT_PATH` environment variable
   (colon-separated on POSIX, semicolon on Windows).
3. `~/.local/share/indent/site-packages/` (where the std library installs).

If a file lives inside a project with a `.nest/` environment (see
[§23](#23-command-line--tooling)), the nest's `air-packages:` and `lib:` paths
are **prepended** to the search path automatically.

> ⚠️ On Windows the module search does **not** use `.ath` files — rename
> `foo.ath` to `foo.ind` (they are the same language; only the extension
> changed). See [`windows.md`](windows.md).

### Running another Indent file

`launch "file.ind"` runs the file **in the current runtime**, so its functions
and module-level variables become available:

```indent
#! helper.ind defines: fun double x   and   var helperMsg = "..."
launch "helper.ind"
var d = double 21          #! → 42
say helperMsg              #! the other file's module var is in scope
```

`run_file "file.ind"` is a deprecated alias. To run a file as a **separate
process**, use `os_run "indent file.ind"`.

---

## 14. Error handling

### `do` / `catch` / `lastly`

```indent
do:
    flag "something broke"      #! raises an error
catch as err:
    say "Error: " + err
lastly:
    say "Cleanup runs always"
```

- The `do:` block runs first.
- If it throws, the `catch as err:` block runs with `err` bound to the error
  text (use `error_message(err)` for just the message, `error_type(err)` for a
  short category word like `key_error`).
- `lastly:` runs in *all* cases (like a `finally` in other languages).

### Typed errors

`error_type(err)` returns a short **category word** (e.g. `key_error`), and
`error_message(err)` the human-readable message:

```indent
do:
    var d = {"a": 1}
    say d["missing"]
catch as err:
    say error_type(err)      #! → key_error
    say error_message(err)   #! → Dictionary key not found: missing
```

### The `Result` value

A lightweight alternative to exceptions: functions return a dict shaped either
`{"ok": true, "value": ...}` or `{"ok": false, "error": "..."}`.

```indent
var r = ok 42        #! {"ok": true, "value": 42}
var e = err "nope"   #! {"ok": false, "error": "nope"}

is_ok(r)             #! → TRUE
is_err(e)            #! → TRUE
unwrap(r)            #! → 42
```

### Assertions

```indent
assert 1 == 1            #! no-op on success; errors on failure
assert_eq(1, 1)          #! assert two values equal
assert 1 == 2, "message" #! optional failure message
```

---

## 15. File I/O & the operating system

### Reading & writing text files

```indent
file_write_text("out.txt", "hello\n")   #! write (overwrites)
file_append_text("out.txt", "more\n")   #! append
var t = file_read_text("out.txt")       #! read whole file
file_size("out.txt")                    #! bytes
file_sha256("out.txt")                  #! hash of the file's contents
```

### Paths & directories

```indent
os_getcwd()               #! current working directory
os_exists("out.txt")      #! → TRUE
os_is_file("out.txt")     #! → TRUE
os_is_dir("src")          #! → TRUE
os_mkdir("build")         #! create a directory
os_remove("out.txt")      #! delete a file
os_list_dir(".")          #! list entries
glob("src/*.ind")         #! files matching a glob
walk("src")               #! recursive file listing

os_copy("a.txt", "b.txt")
os_move("b.txt", "c.txt")   #! also renames
os_copy_tree("dir", "backup")

path_join("a", "b", "c.txt")  #! "a/b/c.txt" (OS-aware)
path_basename("/x/y.txt")     #! "y.txt"
path_dirname("/x/y.txt")      #! "/x"
path_ext("/x/y.txt")          #! ".txt"
path_stem("/x/y.txt")         #! "y"
path_abs(".")                 #! absolute
path_expand("~/x")            #! expand ~
path_norm("a/../b")           #! normalize → "b"
```

### Environment variables

```indent
os_getenv("HOME")            #! value, or empty
os_getenv("MISSING", "dflt") #! with a default
os_setenv("KEY", "value")    #! set for child processes
os_environ()                 #! whole environment as a dict
```

### Running external programs

```indent
os_system("ls -l")           #! run & return exit code (0 = ok)
var r = os_run("echo hi")    #! → {ok, status, stdout, stderr}
say r["stdout"]
os_run_ok("true")            #! → TRUE (did it exit 0?)
os_which("python3")          #! path to an executable on PATH, or empty
```

### The `with` / `open` file context

```indent
with "file.txt" for read as f:
    var contents = f.read()
```

---

## 16. Data formats

### JSON

```indent
var text = json_dumps({"a": 1, "b": [1, 2]})   #! → string
var obj  = json_loads(text)                    #! → dict
```

The `json` std module wraps this in PascalCase (`Loads`, `Dumps`,
`Stringify`, `Parse`).

### TOML & YAML

```indent
toml_loads("title = 'T'")          #! parse TOML → dict
toml_dumps({"title": "T"})         #! dict → TOML
yaml_loads("a: 1\nb: 2")           #! parse YAML → dict
yaml_dumps({"a": 1})               #! dict → YAML
```

### CSV

```indent
csv_write("data.csv", [["name", "age"], ["Ada", 28]])
var rows = csv_read("data.csv")    #! list of rows
```

### SQLite (bundled — no install)

```indent
sqlite_exec("db.sqlite", "CREATE TABLE IF NOT EXISTS t (id INTEGER, v TEXT)")
sqlite_exec("db.sqlite", "INSERT INTO t VALUES (1, 'hi')")
var rows = sqlite_query("db.sqlite", "SELECT * FROM t")
var one  = sqlite_query_one("db.sqlite", "SELECT v FROM t WHERE id = 1")
```

### Compression & archives

```indent
gzip_compress("some text")            #! → base64 string
gzip_decompress(gzip_compress("x"))
zip_list("archive.zip")               #! list entries in a zip
zip_extract("archive.zip", "dest/")   #! extract to a folder
```

---

## 17. Networking: HTTP & WebSockets

### HTTP client

```indent
var body = http_get("https://api.example.com/thing")
var json = http_get_json("https://api.example.com/thing")

http_post_json(url, {"key": "value"})     #! → parsed JSON response
http_put_json(url, data)
http_patch_json(url, data)
http_delete(url)

#! Optional auth header on any request:
http_get(url, "Bearer sk-...")
```

> 🔑 Typed HTTP helpers (`http_get_json`, and the `_json` request verbs) parse
> the response body as JSON and return it as a dict/list, which is almost always
> what you want. The non-JSON `http_get` returns the raw text body.

### Dynamic web server (`http_serve`)

`http_serve(handler, port)` runs a dynamic HTTP server. It calls your handler
function with a **request dict** (`method`, `path`, `query`, `headers`, `body`)
and uses the handler's return value (a string, or a `{status, body,
content_type}` dict) as the response. Routing is ordinary Indent logic:

```indent
fun handle req
    if req.path == "/"
        give "<h1>Hello from Indent</h1>"
    if req.path == "/greet"
        give "<h1>Hi " + req.query["name"] + "!</h1>"
    give {"status": 404, "body": "not found", "content_type": "text/plain"}

http_serve handle 8080
```

> ⚠️ **Handlers run in a fresh scope per request.** Inside a handler you cannot
> call user helper functions or read module-level variables — inline the logic
> and use literal paths. Imports (`get X from web`) and builtins *do* work in
> handlers.

The `web` std module provides response builders and `RunCode` for in-browser
code execution. See [`web-package.md`](web-package.md). Serving a static folder
uses `http_serve_dir("public", 8080)`.

### WebSockets (client)

```indent
var sock = ws_connect("wss://echo.example.com")
ws_send_text(sock, "hello")
var reply = ws_recv_text(sock)          #! blocks until a message arrives
var r2 = ws_recv_text_timeout(sock, 2)  #! wait up to 2 seconds
ws_close(sock)
```

---

## 18. Concurrency

Indent runs work on **real background threads** and gives you several ways to
coordinate them. Pick the model that matches the mental load you want.

### 1. Tasks — `spawn` + `task_*` (lowest ceremony)

`spawn "fn" args...` runs a named function on a background thread and returns a
task id. Poll or wait for it with the `task_*` builtins.

```indent
fun slow n
    time_sleep 1
    give n * 2

var t = spawn "slow" 21        #! starts immediately on a thread
task_done(t)                   #! → FALSE (still running)
var result = task_wait(t)      #! blocks until done → 42
task_wait_timeout(t, 3)        #! wait, but give up after 3s
```

`parallel(fn, list_of_arglists)` gathers a list of calls onto threads:

```indent
parallel("slow", [[1], [2], [3]])   #! run three in parallel
```

### 2. Python-style async — `async fun` / `loop` / `await`

An `async fun` returns a **future** when called. A `loop:` block is an
async context where `await` unwraps a future into `__await_result__`.

```indent
async fun fetch id
    give http_get_json("https://api.example.com/" + id)

loop:
    var f1 = fetch 1
    var f2 = fetch 2
    await f1
    var a = __await_result__
    await f2
    var b = __await_result__
    say a + b
```

### 3. `gather` (concurrent fan-out, ordered results)

```indent
async fun fetch id
    give http_get_json("https://api.example.com/" + id)

loop:
    var f1 = fetch 1
    var f2 = fetch 2
    var results = gather f1 f2    #! both run concurrently; results in order
```

`gather` also accepts a list: `gather [f1, f2]`. `sleep(secs)` returns a future
that completes after a delay (async sleep); `future_wait_for(id, secs)` waits
with a timeout. Future status helpers: `future_done`, `future_result`,
`future_cancel`.

### Async HTTP

`http_get_async`, `http_post_json_async`, `http_put_json_async`,
`http_delete_async` run HTTP on background threads and return futures you can
`gather` or `await` — ideal for firing many requests concurrently without
blocking.

```indent
var f1 = http_get_async "https://api.example.com/a"
var f2 = http_get_async "https://api.example.com/b"
var results = gather f1 f2     #! both run at the same time
```

> ⚠️ Indent's concurrency uses threads, and the runtime is not a
> thread-safe-shared-state model. Each spawned/async call runs in its own
> context; prefer passing inputs as arguments and reading results from the
> returned future/task, rather than relying on shared module globals being
> mutated from many threads.

---

## 19. Colors

The color subsystem wraps text in ANSI truecolor codes. Every color function
takes text and produces a styled string — safe to concatenate or pass to
`say`. Accept a hex code (`#RGB`/`#RRGGBB`), a named color (`RED`, `green`,
`gold`, …), or a color variable.

```indent
fg(text, color)          #! or colored(text, color) — foreground color
bg(text, color)          #! background color
style(text, ...)         #! bold, italic, underline, strikethrough, dim, ...
gradient(text, from, to) #! fade between two colors
multicolor(text, ...)    #! color each segment
rainbow(text)            #! the full spectrum
paint(text, fg, bg, style)   #! combine everything in one call
```

(`bg_gradient`, `bg_multicolor`, `bg_rainbow` style the background plane.)

```indent
var accent color = "#22c55e"
say fg "This is green" "green"
say gradient "Fade" "#ff0000" "#0000ff"
say bg "highlight" "#ffff00"
say rainbow "RAINBOW"
say paint "styled" "#ffffff" "#222222" ["bold", "underline"]
```

The `debug` std module offers ready-made helpers: `debug.warn`,
`debug.error`, `debug.success`, `debug.info` (yellow/red/green/cyan).

```indent
get Warn from debug
get Success from debug
Warn "careful"
Success "all good"
```

---

## 20. GUI & games

### HTML windows

`gui_show_html(html, [title], [width], [height])` opens a native window
(WebKitGTK) that renders HTML. It needs the `indent-gui` helper, which
`install.sh` builds automatically (requires `gcc`, `gtk3`, `webkit2gtk`).

```indent
gui_show_html("<h1>Hello</h1>", "My App", 800, 600)
```

### 2D games with InGame

[`std/ingame.ind`](std/ingame.ind) is a native 2D game framework that **mirrors
PyGame's API** — all game logic lives in Indent; a native window draws frames
and reports input. Install with `air install ingame`, or import the std copy:

```indent
get Init from ingame
get SetMode from ingame
get DrawRect from ingame
get Flip from ingame
get GetEvents from ingame
get Quit from ingame

Init()
var win = SetMode(400, 400, "My Game")   #! display.set_mode
var running = true
repeat while running
    repeat e in GetEvents()              #! event.get
        if e["type"] == "quit"
            running = false
    DrawRect 10 10 50 50 "#39d353"       #! draw.rect
    Flip "#000000"                       #! flush a frame
    Tick 60                              #! ~60 fps
Quit()
```

InGame offers drawing (`DrawRect`, `DrawCircle`, `DrawLine`, `DrawPolygon`,
`DrawText`, `DrawSprite`, `DrawRectRot`, `DrawEllipse`, `DrawArc`), input
(`GetEvents`, `GetKeys`, `IsKeyDown`, `GetMouse`), a scrolling **camera**
(`SetCamera`/`GetCamera`/`ScreenX`/`ScreenY`), math helpers (`Clamp`, `Lerp`,
`Distance`, `Wrap`), **entities** with AABB collision and gravity
(`NewEntity`, `Move`, `Collides`, `StepPhysics`, `MoveInMap`), and **tilemaps**
(`MakeTilemap`, `SetTile`/`GetTile`, `DrawTilemap`, `IsSolidAt`,
`TileToWorld`/`WorldToTile`). `agame` is a deprecated compatibility shim that
re-exports `ingame`.

Playable examples:

```bash
indent examples/snake_game.ind            # play Snake (arrow keys)
INDENT_SNAKE_BOT=1 indent examples/snake_game.ind   # bot autoplay
indent examples/breakout_game.ind         # play Breakout
```

Full API: [`ingame-package.md`](ingame-package.md) and
[`agame-package.md`](agame-package.md).

---

## 21. The standard library

The std library ships with every install (no `air install` needed). It provides
PascalCase functions that wrap the lowercase builtins. Import any module with
`get <Name> from <module>` (or import the whole module and use dot calls).

| Module | Purpose |
|---|---|
| `strings` | case transforms, trim/pad, split/join, search, slice, repeat, count |
| `math` | arithmetic, abs/sqrt/pow/floor/ceil/round, trig, log/exp, min/max/clamp |
| `collections` | list ops (sort/sum/map/filter/zip/range) + dict ops (get/set/remove) |
| `fs` | read/write/append file, exists/delete/copy/rename, list dir, sha256 |
| `json` | load / dump / stringify / parse JSON |
| `os` | env vars, cwd, file/dir checks, mkdir/remove/rename, list dir, run command |
| `io` | print, input (string/int/float), read/write/append file, error |
| `time` | now, utc, sleep, format, parse, perf counter |
| `datetime` | now/utc, format/parse, sleep, timestamp (near-duplicate of `time`) |
| `random` | random int/float, choice, shuffle, seed, uuid |
| `regex` | match, search, findall, replace, split |
| `path` | basename, dirname, join, exists/file/dir checks, ext/stem/abs/expand/norm |
| `hash` | sha256 of text or of a file |
| `base64` | encode / decode |
| `sys` | args, exit, platform, arch, version, executable |
| `testing` | assert, assert-eq, assert-true/false |
| `net` | http get/post/put/patch/delete, static dir server |
| `web` | HTML/JSON/Text/Send responses + `RunCode` (see [`web-package.md`](web-package.md)) |
| `debug` | warn/error/success/info colored logging |
| `ingame` | PyGame-style 2D game framework (see [`ingame-package.md`](ingame-package.md)) |
| `ai` | OpenAI-compatible AI assistant (see [`ai-package.md`](ai-package.md)) |

```indent
get Upper from strings
get Write from fs
get Sha256 from hash
say Upper "hello"           #! → HELLO
```

> 💡 `time` and `datetime` are near-duplicates (both expose now/utc/sleep/
> format/parse). Pick one and be consistent. The `json` module's `Parse`/
> `Stringify` are aliases of `Loads`/`Dumps`.

---

## 22. The AIR package manager

AIR is Indent's package manager ("pip for Indent"). It installs packages from
the [registry](https://github.com/xytrolabs/air) (50+ packages) into
`~/.local/share/indent/air-packages/`.

```bash
air install colors          # install from registry
air install slug            # install another
air uninstall colors        # remove
air search json             # search the registry
air info math               # package details
air list                    # show installed packages
air update                  # update all installed packages
```

Popular packages: `ai`, `stats`, `matrix`, `markdown`, `yaml`, `args`,
`logger`, `url`, `cookie`, `slug`, `textwrap`, `diff`, `fraction`, `semver`,
`asciitable`, `colors`, `agame`, `ingame`, `discord`. AIR auto-detects and
installs a package's `get X from Y` dependencies automatically.

Installed packages resolve automatically (the `air-packages/` folder is on the
module search path). See [`packages-reference.md`](packages-reference.md) for
the full registry reference and [`deliveries.md`](deliveries.md) for bundling a
whole project with `air delivery`.

---

## 23. Command line & tooling

### Running programs

```bash
indent run file.ind          # run a program  ("indent file.ind" also works)
indent repl                  # interactive REPL
```

### Development commands

```bash
indent fmt file.ind          # auto-format code
indent check file.ind        # parse/syntax check
indent lint file.ind         # lint for issues
indent test tests/           # run tests in a folder (expects a directory)
indent --debug file.ind      # debug with breakpoints
```

### Hot reload — `indent --watch`

Re-parses and re-runs the script **whenever it changes on disk** (polls every
~400 ms). There is no compile step, so restarts are near-instant — ideal while
iterating on a server or window.

```bash
indent --watch server.ind
```

Edit `server.ind`, save, and it restarts automatically.

### Environments — `indent nest`

**Nests** are project-local package environments, like a Python `venv`. They
let each project pin its own packages instead of sharing one global set.

```bash
indent nest init                 # scaffold a .nest/ in the current folder
indent nest list                 # show the active nest and its packages
indent nest path                 # print the path to the active nest
```

**Pin an interpreter version** — a nest can use a specific Indent release
(including an older one), independent of your global `indent`:

```bash
indent nest install              # pin the newest published release
indent nest install 2.1.0        # pin a specific (e.g. older) release
indent nest use 2.1.0            # alias of install
indent nest version              # show the pinned version
```

`install` downloads the release-CI **prebuilt** binary for that tag into
`.nest/bin/` and records it in `.nest/indent-version`; when the nest is active
(`source .nest/activate`), `indent` on PATH is that pinned version.

Running any file inside a project that has a `.nest/` **auto-detects** it and
prepends its `air-packages:` and `lib:` paths to the module search path, so the
project's packages resolve without extra flags. A `.nest/` contains
`air-packages/`, `lib/`, `bin/`, `deps.txt`, and `activate` scripts (bash +
`Activate.ps1` for PowerShell). Full details:
[`env-nests.md`](env-nests.md).

### Releasing packages — `air delivery`

`air delivery` bundles and shares a whole project (the reverse of install). Run
`air delivery` in a folder to build, install, or publish it. See
[`deliveries.md`](deliveries.md).

### Version updates

```bash
indent --update              # update Indent to the latest version
indent --version             # show the installed version
```

---

## 24. Running untrusted code with `--safe`

`indent --safe file.ind` runs a script inside a **default-deny sandbox**: only
*pure, computational* builtins are allowed, and anything that touches the
filesystem, the OS, the network, Python, subprocesses, sockets, or the GUI is
**blocked with an error**.

```bash
indent --safe untrusted.ind
```

Safe mode is *opt-in*: run a normal script **without** `--safe` and every
builtin works as documented. Use `--safe` when a script could come from an
untrusted source — e.g. a web playground or a Discord bot that executes
user-submitted code — so a malicious script cannot read or write files, run
shell commands, make network calls, or reach Python.

What stays allowed in `--safe`:

- Pure math & regex: `math_*`, `regex_*`
- Random, strings, path handling, type checks: `random_*`, `str_*`, `path_*`, `is_*`, `set_*`
- Core data & logic: `append`, `extend`, `insert`, `remove`, `pop`, `sort`,
  `slice`, `reverse`, `map`, `filter`, `reduce`, `group_by`, `dict_*`,
  `has_key`, `keys`, `values`, `items`, `contains`, `find`, `len`, `sum`,
  `min`, `max`, `range`, `zip`, `enumerate`, …
- Strings & output: `upper`/`lower`/`trim`/`replace`/`split`/`join`/…,
  `format`, `sformat`, `say`/`print`
- Types & JSON/TOML/YAML parsing: `int`/`float`/`string`/`bool`, `json_*`,
  `toml_*`, `yaml_*`, `uuid`, `hash_sha256`
- Time: `time_now`, `time_utc`, `time_perf_counter`, `time_format`, `time_parse`
- System info: `sys_version`, `sys_platform`, `sys_arch`

What is blocked in `--safe` (denied by default):

- All `os_*` and `file_*` (no file read/write, `os_run`, `os_system`, `glob`,
  `walk`, env vars, etc.)
- All `http_*`, `ws_*`, `gui_*`, `sqlite_*`, `csv_*`, `zip_*`, `base64_*`
- All `python_*` interop, `process_exit`, `ask`, `sleep`/`gather`/`future_*`/
  `task_*`, and the `log` builtin.

Example — this script is blocked under `--safe`:

```indent
file_write_text("pwned.txt", "you got hacked")
```

```bash
$ indent --safe evil.ind
error: 'file_write_text' is not allowed in safe mode
```

---

## 25. Best practices & golden rules

1. **`fun(args)` call syntax works everywhere** — prefer the parenthesized
   form for clarity and to avoid parser edge cases.
2. **Comments use `#!`** — not `#` (which is used for directives/shebang).
3. **`var x is <expr>` declares; `x = <expr>` reassigns.** (The reverse
   spellings also work, but keep it consistent.)
4. **Use type inference** — `var x = 42` infers `int`. Add an explicit type
   only when the value type isn't obvious (e.g. `dynamic`).
5. **Containers are by value.** `append`/`insert`/`remove`/`dict_set`/… return
   a *new* container — always reassign: `l = append(l, x)`.
6. **Guard optional dict access** with `has_key` before reading a key that may
   not exist; missing keys throw errors.
7. **Return with `give`**, not `return`. Declare with `var`, reassign with `=`.
8. **Use `set x type` for type conversion** in statements, or `int(x)`/
   `string(x)`/… inside expressions.
9. **Build unique collections with `group([...])`**, not `set` — `set` is the
   type-conversion keyword (`set x string`).
10. **Use the standard library** — PascalCase helpers are already optimized and
    never collide with builtins.
11. **`indent --update`** keeps you current.
12. **Use `--safe`** when executing untrusted scripts.

### Style notes

- Name files with the `.ind` extension (`.ath` is the legacy name).
- Put reusable functions in their own `.ind` file and `launch` or import them
  rather than copy-pasting.
- Prefer small, single-purpose functions with `give` results.
- For side-effecting statements, keep them as the final statement of a block
  where possible.

---

## 26. Troubleshooting

### "Dictionary key not found: x"

You read a key that doesn't exist. Guard with `has_key`:

```indent
if has_key person "name"
    say person["name"]
```

### My list/dict didn't change after calling a function

You forgot to reassign. `append`/`insert`/`dict_set`/etc. return new values:

```indent
#! WRONG: l doesn't change
append(l, 5)

#! RIGHT
l = append(l, 5)
```

### `indent check` says "No .ind files found under <file>"

`indent check` expects a **directory**. To parse-check a single file, run it
(top-level statements run; package files that only define functions are safe to
run).

### My module isn't found

Check the search order in [§13](#13-imports--the-module-system): script
directory and parents → `INDENT_PATH` → `~/.local/share/indent/site-packages/`.
Make sure the file is named `foo.ind` (not `.ath`) and is in one of those
places. On Windows, the runtime does not look for `.ath`.

### Windows install fails or 404s

The installer downloads a prebuilt `.exe` from the latest GitHub Release; if
none is published it falls back to a source build and bootstraps Rust/MSVC for
you. See [`windows.md`](windows.md) for a full walkthrough.

### Functions can't call other functions I defined

This usually means the callee isn't in scope (defined in another file/module
that wasn't `launch`ed/imported) or a parser edge case with a bare call as a
non-final statement. `launch` the helper file first, or parenthesize the call.

### Colors / GUI show nothing

The GUI and game helpers need `indent-gui`/`indent-ingame`, built from C by the
installer with `gcc`, `gtk3`, and `webkit2gtk`. On a headless system or without
those libraries, the native window cannot open. Check you installed with the
full installer and that the helper binaries exist under
`~/.local/share/indent/bin/`.

---

*Indent is developed by Xytro Labs. See the [CHANGELOG](../CHANGELOG.md) for
version history and the [builtins reference](builtins-reference.md) for the
complete function list.*
