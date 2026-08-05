# Indent Quick Reference (v1.3.0)

## Basics
```indent
#! Comment                  say "Hello"                 # Output
var x = 42                  # Variable — type inferred from value!
var x int = 42              # Variable with explicit type
var name = "Ada"            # string (inferred)
var flag = true             # boolean (inferred)
var nums = [1,2,3]          # list (inferred)
x is 43                     # Reassign
null                        # Null/none value (alias for empty)
```

## Compound Assignment (v1.3)
```indent
x += 8     # x = x + 8
x -= 10    # x = x - 10
x *= 2     # x = x * 2
x /= 3     # x = x / 3
x %= 5     # x = x % 5
```
Works with any numeric variable. list/dict merge with +=.

## String Interpolation
```indent
var name string = "Ada"
say "Hello %name%!"         # → "Hello Ada!"
```

## Functions
```indent
fun greet person            # Inline params
    say "Hello " + person
greet("Ada")                # Call — parenthesized form works everywhere

fun add a b                 # Multi-param
    give a + b              # Return

fun greet name = "World"    # Default parameter (v1.2)
    say "Hello " + name
greet                       # → "Hello World!"
greet "Ada"                 # → "Hello Ada!"

fun add a b as int          # Return type annotation (v1.2)
    give a + b

# Function references
fun handler x
    say x
register handler            # Pass function without calling it!

# Lambda (v1.2)
var double = fn(x): x * 2
say double 5                # → 10
```

## Classes
```indent
class Person
    var name string
    var age int
    fun greet
        say "I'm " + name

var p dynamic = Person "Ada" 28
p.greet()
say p.name

class Employee from Person
    var role string
    fun greet
        say "I'm " + name + ", " + role

var e dynamic = Employee "Bob" 35 "Engineer"
e.greet()
```

## Control Flow
```indent
if x > 10
    say "big"
or x > 5
    say "medium"
otherwise
    say "small"

match x:
    case "a":
        say "Alpha"
    otherwise:
        say "Other"
```

## Loops
```indent
repeat 5                    # Fixed count (Reps is 0-indexed)
repeat item in list         # Over items
repeat until x == 10        # Conditional
for x in list               # for alias (v1.2)
stop / next / reset         # Break / continue / restart
```

## Expressions (v1.2)
```indent
# Comprehensions
[x * x for x in range 5]          # → [0, 1, 4, 9, 16]
{x: x * 2 for x in range 3}       # → {"0": 0, "1": 2, "2": 4}
[x for x in list if x > 5]        # Filtered

# Ternary
var s string = "adult" if age >= 18 else "child"

# Chained comparisons
if 0 < x < 10                     # → x > 0 and x < 10

# Bitwise operators
5 & 3    # → 1 (AND)
5 | 3    # → 7 (OR)
5 ^ 3    # → 6 (XOR)
~5       # → -6 (NOT)
1 << 2   # → 4 (shift left)
8 >> 2   # → 2 (shift right)

# Identity
x is empty          # true if x is null/empty
x is not y          # identity check
```

## Data
```indent
var list list = [1, 2, 3]
var dict dict = {"key": "val"}
var mixed dynamic = [1, "hi"]
list[0]
dict["key"]
list is list + [4]
person.name

# Sets (v1.3) — unique ordered collections
var s = set [1, 2, 2, 3]     # → {1, 2, 3}
var s2 = set [3, 4]
var u = s + s2                # union: {1, 2, 3, 4}
contains(s, 2)                # → TRUE
repeat item in s              # iteration
[x * 2 for x in s]            # comprehension
```

## Imports
```indent
get math                    # Whole module
import math                 # import alias (v1.2)
get Pow from math           # One function
import Pow from math        # Same with import (v1.2)
get RandInt from random as R # Aliased
```

## File Handling (v1.2)
```indent
open "data.txt" for read as f:
    say f                   # reads file content into f

open "out.txt" for write as f:
    f is "Hello World!"     # writes to file
```

## Common Builtins
```indent
ask("Prompt: ")             # Input
len("hello")                # Length
upper(s) / lower(s) / trim(s) / replace(s, from, to)
split("a,b", ",") / join([1,2], ",")
starts_with(s, pre) / ends_with(s, suf) / contains(s, sub)
slice(s, start, end) / find(s, sub) / index(s, sub)
pad_left(s, 10, " ") / pad_right(s, 10, " ") / repeat_str(s, 3)
keys(dict) / values(dict) / has_key(dict, key) / items(dict)
sort(list) / reverse(list) / append(list, item) / pop(list) / insert(list, idx, item)
extend(list1, list2) / remove(list, value) / enumerate(list) / zip(list1, list2)
map(list, func) / filter(list, func)    # Functional (v1.2)
range(end) / range(start, end, step)
int("42") / float("3.14") / string(42) / bool("true")
int_or(s, fallback) / float_or(s, fallback) / type_of(value)
abs(n) / is_even(n) / is_odd(n) / between_int(v, min, max) / inc(v) / dec(v)
assert(cond) / assert_eq(a, b) / do/catch/flag
copy(val) / clear(val) / count(container, item) / sum(list)
```

## Regex (v1.2)
```indent
regex_match("hel+o", "hello")        # → true
regex_search("\\d+", "abc123")       # → {start: 3, end: 6, text: "123"}
regex_findall("\\d+", "a1b2c3")     # → ["1", "2", "3"]
regex_replace("\\d", "X", "a1b2")   # → "aXbX"
regex_split(",\\s*", "a, b, c")     # → ["a", "b", "c"]
```

## Date/Time (v1.2)
```indent
time_now()                  # Unix timestamp (float)
time_utc()                  # Same as time_now
time_format(ts, "%Y-%m-%d") # Format timestamp
time_parse("2024-01-15")    # Parse ISO date to timestamp
time_sleep(0.5)             # Sleep seconds
```

## Crypto & Encoding (v1.2)
```indent
uuid()                      # Random UUID v4
base64_encode("hello")      # → "aGVsbG8="
base64_decode("aGVsbG8=")   # → "hello"
hash_sha256("hello")        # SHA256 hex string
file_sha256("file.txt")     # SHA256 of file
```

## File & Path (v1.2)
```indent
glob("*.ind")               # List files matching pattern
path_join("/home", "user") # → "/home/user"
path_basename("/a/b.txt")  # → "b.txt"
path_dirname("/a/b.txt")   # → "/a"
```

## JSON, HTTP, WebSocket
```indent
json_loads(text) / json_dumps(value)
http_get(url) / http_post_json(url, body)
ws_connect(url) / ws_send_text(id, text)
```

## OS & System
```indent
file_read_text(path) / file_write_text(path, text) / file_sha256(path)
os_getcwd() / os_exists(path) / os_list_dir(path) / os_system(cmd) / os_mkdir(path)
os_getenv("HOME") / os_setenv("KEY", "val")
process_exit(0)
```

## Error Handling
```indent
do:
    flag "message"
catch as err:
    say err
lastly:
    say "cleanup"

time_now() / time_sleep(seconds)
random_int(min, max) / random_choice(list) / random_shuffle(list) / random_float()
math_pow(base, exp) / math_sqrt(n) / math_sin(n) / math_abs(n) / math_floor(n) / math_ceil(n)
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

## Golden Rules
1. `func(args)` works **everywhere** — `say`, `if`, `is` assignments, nested. Prefer it.
2. `#!` for comments, `#` is for hex colors only
3. Lists/dicts are immutable — use `is` + `+` to accumulate
4. Bare identifiers in `var` are treated as function calls — use `string(param)` instead
5. Imports resolve: same dir → `INDENT_PATH` → `~/.local/share/indent/site-packages/`
6. Type inference: `var x = 42` is `var x int = 42` — let the compiler do the work!
7. Compound assignment: `x += 5` instead of `x is x + 5`
8. `indent --update` keeps you on the latest version — run it anytime!
