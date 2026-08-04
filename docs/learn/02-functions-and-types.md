# Lesson 02 — Functions & Types

**Time**: 20 minutes  |  **Topics**: Functions, Parameters, Branching, Loops, Types, Conversion

---

## 🎯 Learning Objectives
- Define functions with `fun` and return with `give`
- Use inline and multi-line parameter styles
- Branch with `if`/`or`/`otherwise`
- Loop with `repeat`
- Understand Indent's type system and conversions

---

## Functions

Define with `fun`. Return with `give`.

**Inline parameters:**
```indent
fun greet person
    say "Hello " + person + "!"
```

**Multi-line parameters:**
```indent
fun add
    a
    b
    give a + b
```

**Calling functions:**
```indent
greet "World"           #! space-separated arguments
var sum int = add 5 7   #! used in expressions
say sum                 #! 12
```

## Branching

`if` / `or` / `otherwise` — no colons needed:

```indent
var temp int = 25

if temp > 30
    say "It's hot!"
or temp > 20
    say "It's warm."
otherwise
    say "It's cool."
```

Combine conditions with `and` / `not`:
```indent
if temp > 20 and temp < 30
    say "Perfect weather!"
```

## Loops

One keyword: `repeat`

```indent
repeat 5
    say "Iteration " + (Reps + 1)

var colors list = ["red", "green", "blue"]
repeat item in colors
    say "Color: " + item
```

## Lists & Dictionaries

```indent
var colors list = ["red", "green", "blue"]
var person dict = {"name": "Ada", "age": 28}

say colors[1]        #! "green"
say person["name"]   #! "Ada"
say person.name      #! "Ada" — dot notation
say len colors        #! 3
```

## Type System Deep Dive

### All Types

| Type | Example | Description |
|---|---|---|
| `string` | `"hello"` | Text |
| `int` | `42` | Whole number |
| `float` | `3.14` | Decimal number |
| `boolean` | `true`, `false` | Yes/no |
| `list` | `[1, 2, 3]` | Typed ordered collection |
| `dict` | `{"a": 1}` | Typed key-value map |
| `dynamic` | anything | Any type, can change |
| `empty` | `empty` | No value (null) |

### Type Inference (v1.3)

When the value makes the type obvious, you can skip the type annotation:

```indent
var name = "Ada"       # → string
var age = 28           # → int
var pi = 3.14          # → float
var flag = true        # → boolean
var nums = [1, 2, 3]   # → list
```

Explicit types still work — use them when you want to be clear or when inference can't determine the type:

```indent
var x int = 42              # explicit — always fine
var data dynamic = getData  # inference can't help here
```

### The `dynamic` Type

Use `dynamic` when you don't know the type ahead of time, or need mixed content:

```indent
var mixed dynamic = "hello"
mixed is 42              # now it's an int
mixed is [1, "two", 3]   # now it's a mixed list

var anything dynamic = ["red", 42, true]  # mixed list
```

### Type Conversion

Convert between types with these built-in functions:

```indent
var n int = int("42")           # string → int: 42
var f float = float("3.14")     # string → float: 3.14
var s string = string(42)       # int → string: "42"
var b boolean = bool("true")    # string → boolean: true

# Safe conversion with fallback (returns default on failure)
var safe int = int_or("hello", 0)     # → 0
var ok float = float_or("nope", 1.0)  # → 1.0

# Check any value's type
say type_of(42)          # "int"
say type_of([1,2])       # "list"
say type_of("hi")        # "string"
```

### Typed Function Parameters & Returns

Functions can declare parameter types and return types:

```indent
# Typed parameters
fun add a int b int
    give a + b

# Typed return value
fun double n int give int
    give n * 2

var result int = double 21   # → 42
```

## Practice

1. Write a function `max` that returns the larger of two numbers.
2. Write a function `grade` that takes a score and returns "A", "B", "C", or "F".
3. Create a list of your favorite things and loop with `repeat item in list`.

---

## Next Lesson

➡️ [Lesson 03: Control Flow](03-control-flow.md)
