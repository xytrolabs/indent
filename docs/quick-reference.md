# Indent Quick Reference (v1.4.1)

> Complete syntax reference for the Indent programming language.
> **New in 1.4**: Group type (`group` builtin), type conversion (`set varname type`), compound assignment, type inference.
> 💡 Unique ordered collections are called **groups** — `group([1,2,3])`. The `set` keyword performs **type conversion** (`set x string`); only the `group` builtin builds a group.<｜end▁of▁thinking｜>## Basics
```indent
#! Comments start with #! (hash-bang)
say "Hello"                 # Print to stdout
var x = 42                  # Type inferred (int)
var name = "Ada"            # Type inferred (string)
var x int = 42              # Explicit type
var flag = true             # Boolean inferred
var nums = [1,2,3]          # List inferred
x is 43                     # Reassign (NOT =)
null                        # Null/none (alias for empty)
```

## Type Conversion (v1.4)
```indent
var name1 = 21              # int
set name1 string            # → "21" (int→string)
set name1 float             # → 21.0 (int→float)
var x = "42"
set x int                   # → 42 (string→int)
var z = 0
set z boolean               # → FALSE (int→bool, 0=false, non-zero=true)
var data = [1,2,2,3]
set data set                # → {1, 2, 3} (list→set, deduplicated)
```
Supported conversions: `string`, `int`, `float`, `boolean`, `list`, `set`, `dict`, `dynamic`, `empty`.

## Compound Assignment (v1.3)
```indent
x += 8     # x = x + 8
x -= 10    # x = x - 10
x *= 2     # x = x * 2
x /= 3     # x = x / 3
x %= 5     # x = x % 5
```
Works with numeric variables. `+=` also merges lists and dicts.

## String Interpolation
```indent
var name string = "Ada"
say "Hello %name%!"         # → "Hello Ada!"
```

## Functions
```indent
fun greet person            # Parameters on same line
    say "Hello " + person
greet("Ada")                # Parenthesized call
greet "Ada"                 # Space-separated call

fun add a b                 # Multiple params
    give a + b              # Return value (NOT "return")

fun greet name = "World"    # Default parameter
greet                       # → "Hello World!"

fun add a b as int          # Return type annotation
    give a + b

# Function references
fun handler x
    say x
register handler            # Pass function without calling

# Lambda (v1.2)
var double = fn(x): x * 2
say double(5)               # → 10
```

## Classes
```indent
class Person
    var name string
    var age int
    fun greet
        say "I'm " + name

var p dynamic = Person "Ada" 28
p.greet()                   # → I'm Ada
say p.name                  # → Ada

class Employee from Person  # Single inheritance
    var role string
    fun greet
        say "I'm " + name + ", " + role
```

## Control Flow
```indent
if x > 10
    say "big"
or x > 5                    # else-if (NOT elif)
    say "medium"
otherwise                   # else (NOT else:)
    say "small"

match x:                    # Pattern matching
    case "a":
        say "Alpha"
    case "b":
        say "Bravo"
    otherwise:
        say "Other"
```

## Loops
```indent
repeat 5                    # Counted loop (Reps is 0-indexed)
repeat item in list         # Iterate over list
repeat item in my_group     # Iterate over a group
repeat until x == 10        # Conditional loop
for x in list               # Alias for repeat

stop                        # Break
next                        # Continue
reset                       # Restart loop
```

## Groups (v1.4) — Unique Ordered Collections

> 💡 Unique collections are **groups** — `group([1, 2, 2, 3])`. The keyword `set`
> is reserved for type conversion (`set varname type`), so building a group uses
> the `group` builtin.

```indent
var s = group([1, 2, 2, 3])  # → {1, 2, 3} — deduplicated
var s2 = group([3, 4, 5, 6])
var u = s + s2              # Union: {1, 2, 3, 4, 5, 6}
contains(s, 2)              # → TRUE
len(s)                      # → 3
type_of(s)                  # → "group"
repeat item in s            # Iteration
[x * 2 for x in s]          # Comprehension: [2, 4, 6]
is_missing(group([]))       # → TRUE (empty group)

# Adding / removing / checking membership (methods use bare values)
var t = group([1, 2])
var t2 = t.add(3)           # → {1, 2, 3}
var t3 = t2.remove(2)       # → {1, 3}
t3.contains(1)              # → TRUE
```

## Expressions
```indent
# Comprehensions (v1.2)
[x * x for x in range 5]          # → [0, 1, 4, 9, 16]
{x: x * 2 for x in range 3}       # → {"0": 0, "1": 2, "2": 4}
[x for x in list if x > 5]        # Filtered

# Ternary (v1.2)
var s string = "adult" if age >= 18 else "child"

# Chained comparisons (v1.2)
if 0 < x < 10                     # x > 0 and x < 10

# Bitwise operators (v1.2)
5 & 3    # → 1 (AND)        1 << 2   # → 4 (shift left)
5 | 3    # → 7 (OR)         8 >> 2   # → 2 (shift right)
5 ^ 3    # → 6 (XOR)        ~5       # → -6 (NOT)

# Identity (v1.2)
x is empty          # TRUE if x is null/empty
x is not y          # Strict identity check
```

## Data Types
```indent
# Lists
var list list = [1, 2, 3]
list[0]                     # → 1
list is list + [4]          # → [1, 2, 3, 4]

# Dictionaries
var dict dict = {"key": "val"}
dict["key"]                 # → "val"
dict.key                    # → "val" (dot notation)
dict["new"] is "value"      # Set key

# Dynamic (any type)
var mixed dynamic = [1, "hi", true]
mixed is 42                 # Can change type

# Empty (null)
var nothing empty
```

## Imports
```indent
get math                    # Import whole module
import math                 # Alias (v1.2)
get Pow from math           # Single function
get RandInt from random as R # With alias
```

## File Handling
```indent
open "data.txt" for read as f:
    say f                   # Reads file into f

open "out.txt" for write as f:
    f is "Hello World!"     # Writes to file

open "log.txt" for append as f:
    f is "appended line"
```

## Common Builtins
```indent
# I/O
ask("Prompt: ")             # User input (returns string)
say value                   # Print to stdout

# Type checking & conversion
type_of(value)              # → "int", "string", "list", etc.
string(x) / int(x) / float(x) / bool(x)   # Conversion functions
int_or(s, fallback) / float_or(s, fallback)  # Safe conversion

# String operations
len("hello")                # → 5 (also works on lists, groups, dicts)
upper(s) / lower(s) / trim(s)
replace(s, from, to)
split("a,b", ",") / join([1,2], ",")
starts_with(s, pre) / ends_with(s, suf)
contains(s, sub)            # Substring check
slice(s, start, end)
find(s, sub) / index(s, sub)
pad_left(s, 10, " ") / pad_right(s, 10, " ")
repeat_str(s, 3)            # Repeat string

# List/dict operations
keys(dict) / values(dict) / items(dict)
has_key(dict, key)
sort(list) / reverse(list)
append(list, item) / pop(list)
insert(list, idx, item) / remove(list, value)
extend(list1, list2)
enumerate(list) / zip(list1, list2)
map(list, func) / filter(list, func)
sum(list) / min(list) / max(list)
any(list) / all(list)
count(container, item)

# Numeric
range(end) / range(start, end, step)
abs(n) / is_even(n) / is_odd(n)
between_int(v, min, max)
inc(v) / dec(v)

# Math (via get math)
math.PI / math.pow(base, exp) / math.sqrt(n)
math.sin(n) / math.cos(n) / math.tan(n)
math.abs(n) / math.floor(n) / math.ceil(n)
math.log(n) / math.log10(n) / math.exp(n)

# Random (via get random)
random_int(min, max) / random_choice(list)
random_shuffle(list) / random_float()

# Time
time_now()                  # Unix timestamp (float)
time_utc()                  # Same as time_now
time_format(ts, "%Y-%m-%d") # Format timestamp
time_parse("2024-01-15")    # Parse ISO date
time_sleep(0.5)             # Sleep seconds

# Regex (v1.2)
regex_match("hel+o", "hello")        # → true
regex_search("\\d+", "abc123")       # → {start: 3, end: 6, text: "123"}
regex_findall("\\d+", "a1b2c3")     # → ["1", "2", "3"]
regex_replace("\\d", "X", "a1b2")   # → "aXbX"
regex_split(",\\s*", "a, b, c")     # → ["a", "b", "c"]

# Crypto & Encoding (v1.2)
uuid()                      # Random UUID v4
base64_encode("hello")      # → "aGVsbG8="
base64_decode("aGVsbG8=")   # → "hello"
hash_sha256("hello")        # SHA256 hex string

# File & Path (v1.2)
glob("*.ind")               # List files matching pattern
path_join("/home", "user") # → "/home/user"
path_basename("/a/b.txt")  # → "b.txt"
path_dirname("/a/b.txt")   # → "/a"

# Assertions & testing
assert(cond)                # Panic if false
assert_eq(a, b)             # Panic if not equal
do/catch/flag               # Error handling

# Utilities
copy(val)                   # Shallow copy
clear(val)                  # Empty container
is_missing(val)             # TRUE if empty/null/blank
default(val, fallback)      # Return fallback if val is missing
```

## JSON, HTTP, WebSocket
```indent
json_loads(text) / json_dumps(value)
http_get(url) / http_post_json(url, body)
ws_connect(url) / ws_send_text(id, text)
```

## OS & System
```indent
file_read_text(path) / file_write_text(path, text)
file_sha256(path)
os_getcwd() / os_exists(path)
os_list_dir(path) / os_system(cmd) / os_mkdir(path)
os_getenv("HOME") / os_setenv("KEY", "val")
process_exit(0)
```

## Error Handling
```indent
do:
    flag "something went wrong"
catch as err:
    say err
lastly:
    say "cleanup always runs"
```

## Error Codes
| Code | Meaning |
|---|---|
| E001 | Type mismatch |
| E002 | Undefined function |
| E003 | Import error |
| E004 | Syntax error |
| E005 | Unwrap on error |
| E006 | Undefined variable |
| E007 | Division by zero |
| E008 | Index out of range |
| E009 | Key not found in dict |
| E010 | File not found |
| E011 | Invalid JSON |
| E012 | Network error |

## Commands
```bash
indent run file.ind          # Run a program
indent fmt file.ind          # Format code
indent check file.ind        # Check syntax
indent lint file.ind         # Lint code
indent repl                  # Interactive REPL
indent test tests/           # Run tests
indent --debug file.ind      # Debug mode
indent --update              # Update to latest version
```


## Standard Library (std/)

The std library ships with Indent and is installed to `~/.local/share/indent/site-packages/`. Import functions by name:

```indent
get Pow from math
get Upper from strings
get Sha256 from hash
get Write from fs
```

Available modules:

| Module | Functions |
|---|---|
| `strings` | `Upper`, `Lower`, `Trim`, `LStrip`, `RStrip`, `Capitalize`, `Title`, `SwapCase`, `Replace`, `Split`, `Join`, `StartsWith`, `EndsWith`, `Contains`, `Find`, `Length`, `Reverse`, `Slice`, `PadLeft`, `PadRight`, `Repeat`, `Count`, `Concat`, `FirstChar`, `IsEmpty`, `Lines` |
| `math` | `Add`, `Sub`, `Mul`, `Div`, `Mod`, `Abs`, `Sqrt`, `Pow`, `Floor`, `Ceil`, `Round`, `Sin`, `Cos`, `Tan`, `Asin`, `Acos`, `Atan`, `Atan2`, `Log`, `Log10`, `Exp`, `Min`, `Max`, `Clamp`, `IsEven`, `IsOdd` |
| `collections` | `Append`, `Extend`, `Insert`, `Pop`, `Remove`, `Contains`, `Sort`, `SortDesc`, `Reverse`, `Slice`, `Sum`, `Min`, `Max`, `Any`, `All`, `Count`, `Enumerate`, `Zip`, `Range`, `Length`, `First`, `Last`, `Filter`, `Map`, `DictGet`, `DictSet`, `DictRemove`, `DictUpdate`, `Keys`, `Values`, `Items`, `HasKey`, `ContainsKey` |
| `fs` | `Read`, `Write`, `Append`, `Exists`, `IsFile`, `IsDir`, `Delete`, `Copy`, `Rename`, `ListDir`, `Mkdir`, `Sha256` |
| `json` | `Loads`, `Dumps`, `Stringify`, `Parse` |
| `os` | `GetEnv`, `SetEnv`, `Environ`, `GetCwd`, `Chdir`, `Exists`, `IsFile`, `IsDir`, `Mkdir`, `Remove`, `Rename`, `ListDir`, `Run`, `WriteText`, `ReadText` |
| `io` | `Print`, `PrintLn`, `Input`, `InputInt`, `InputFloat`, `ReadLine`, `ReadFile`, `WriteFile`, `AppendFile`, `Error` |
| `time` | `Time`, `Now`, `Utc`, `Sleep`, `Format`, `Parse`, `PerfCounter` |
| `datetime` | `Now`, `Utc`, `Format`, `Parse`, `Sleep`, `PerfCounter`, `Timestamp` |
| `random` | `RandInt`, `RandFloat`, `Choice`, `Shuffle`, `Seed`, `Uuid` |
| `regex` | `Match`, `Search`, `FindAll`, `Replace`, `Split` |
| `path` | `Basename`, `Dirname`, `Join`, `Exists`, `IsFile`, `IsDir` |
| `hash` | `Sha256`, `Sha256File` |
| `base64` | `Encode`, `Decode` |
| `net` | `HttpGet`, `PostJson`, `PutJson`, `PatchJson`, `Delete`, `ServeDir` |
| `sys` | `Args`, `Exit`, `Platform`, `Arch`, `Version`, `Executable` |
| `testing` | `Assert`, `AssertEq`, `AssertTrue`, `AssertFalse` |

> Note: std functions use PascalCase so they never collide with the (lowercase) builtins. User-defined functions take precedence over builtins.

## Golden Rules
1. `func(args)` works **everywhere** — in `say`, `if`, `is` assignments, nested calls
2. `#!` for comments, `#` is for hex colors only
3. Lists/dicts are immutable — use `is` + `+` to accumulate
4. Bare identifiers in `var` are treated as function calls — use `string(param)` instead
5. Type inference: `var x = 42` infers `int`, `var name = "Ada"` infers `string`
6. Compound assignment: `x += 5` instead of `x is x + 5`
7. Reassign with `is`, not `=`: `x is 42`
8. `set varname type` converts types: `set x string`
9. `group([1,2,3])` creates a Group (unique collection); `set` is only for type conversion
10. Imports resolve: same dir → `INDENT_PATH` → `~/.local/share/indent/site-packages/`
11. `indent --update` keeps you on the latest version
