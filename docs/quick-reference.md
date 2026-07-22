# Indent Quick Reference

## Basics
```indent
#! Comment                  say "Hello"                 # Output
var x int = 42              # Variable (string/int/float/boolean/dynamic/empty/list/dict)
x is 43                     # Reassign
```

## Functions
```indent
fun greet person            # Inline params
    say "Hello " + person
greet("Ada")                # Call — parenthesized form works everywhere

fun add a b                 # Multi-param
    give a + b              # Return

# Function references (v2.2.0)
fun handler x
    say x
register handler            # Pass function without calling it!
```

## Classes
```indent
class Person                # Define a class
    var name string         # Fields = constructor params (public by default)
    var age int
    fun greet               # Methods
        say "I'm " + name

var p dynamic = Person "Ada" 28    # Natural — no () needed
p.greet()                   # Method call
say p.name                  # Field access — public!

# Inheritance (v2.8+)
class Employee from Person
    var role string
    fun greet               # Override parent method
        say "I'm " + name + ", " + role

var e dynamic = Employee "Bob" 35 "Engineer"
e.greet()                   # "I'm Bob, Engineer"
```

## Control Flow
```indent
if x > 10                   # if / or / otherwise
    say "big"
or x > 5
    say "medium"
otherwise
    say "small"

match x:                    # Match — requires colon + case keyword
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
stop / next / reset         # Break / continue / restart
```

## Data
```indent
var list list = [1, 2, 3]      # Typed list
var dict dict = {"key": "val"}  # Typed dict
var mixed dynamic = [1, "hi"]   # Dynamic list
list[0]                         # Index access
dict["key"]                     # Key access
list is list + [4]              # Append (lists are immutable)
person.name                     # Dot notation for dicts
```

## Imports
```indent
get math                    # Whole module
get Pow from math           # One function
get RandInt from random as R # Aliased
```

## Common Builtins
```indent
ask("Prompt: ")             # Input
len("hello")                # Length
upper(s) / lower(s) / trim(s) / replace(s, from, to)
split("a,b", ",") / join([1,2], ",")
starts_with(s, pre) / ends_with(s, suf) / contains(s, sub)
slice(s, start, end) / find(s, sub) / index(s, sub)
keys(dict) / values(dict) / has_key(dict, key) / items(dict)
sort(list) / reverse(list) / append(list, item) / pop(list) / insert(list, idx, item)
extend(list1, list2) / remove(list, value) / enumerate(list) / zip(list1, list2)
range(end) / range(start, end, step)
int("42") / float("3.14") / string(42) / bool("true")
int_or(s, fallback) / float_or(s, fallback) / type_of(value)
abs(n) / is_even(n) / is_odd(n) / between_int(v, min, max) / inc(v) / dec(v)
assert(cond) / assert_eq(a, b) / do/catch/flag
copy(val) / clear(val) / count(container, item) / sum(list)
json_loads(text) / json_dumps(value)
http_get(url) / http_post_json(url, body)
ws_connect(url) / ws_send_text(id, text)
file_read_text(path) / file_write_text(path, text) / file_sha256(path)
os_getcwd() / os_exists(path) / os_list_dir(path) / os_system(cmd) / os_mkdir(path)
time_now() / time_sleep(seconds)
random_int(min, max) / random_choice(list) / random_shuffle(list) / random_float()
math_pow(base, exp) / math_sqrt(n) / math_sin(n) / math_abs(n) / math_floor(n) / math_ceil(n)
```

## Error Handling
```indent
do:
    flag "message"
catch as err:
    say err
lastly:
    say "cleanup"
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
6. `indent --update` keeps you on the latest version — run it anytime!
