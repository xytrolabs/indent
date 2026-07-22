# Lesson 01 — Hello Indent!

**Time**: 15 minutes  |  **Topics**: Install, Hello World, Variables, Functions, Branching, Loops

---

## 🎯 Learning Objectives
- Install Indent and verify it works
- Write and run your first Indent program
- Declare variables with types
- Define and call functions
- Use `if`/`or`/`otherwise` for decisions
- Loop with `repeat`

---

## Install

### Linux / macOS
```bash
curl -fsSL https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.sh | bash
```

### Windows (PowerShell)
```powershell
irm https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.ps1 | iex
```

### Verify
```bash
indent --version
# → indent 0.1.2
```

### Staying Current
```bash
indent --update    # Auto-update to the latest version
```
This pulls the latest code from GitHub, builds it, and replaces your current binary.

---

## 1. Hello World

Create `hello.ind`:

```indent
#! This is a comment. Comments start with #! (hash-bang).
#! The # character alone is reserved for hex colors like #ff0000.
say "Hello, Indent!"
```

Run it:
```bash
indent run hello.ind
```

---

## 2. Variables

Indent has six types. Every variable must declare its type:

```indent
var name string = "Ada"           # string
var age int = 28                   # int
var pi float = 3.14                # float
var active boolean = true          # boolean
var anything dynamic = [1, "two"]  # dynamic (any type)
var nothing empty                  # empty (null, no value)
var scores list = [95, 87, 92]    # list (typed list)
var config dict = {"key": "val"}  # dict (typed dictionary)
```

Reassign with `is`:
```indent
age is 29
name is "Grace"
```

💡 **Tip**: Use `list` for typed lists, `dict` for typed dictionaries, and `dynamic` when you're unsure of the type or need mixed content. Use `empty` for "no value" (like `null`/`None` in other languages).

> **`=` vs `is`**: Use `=` when **declaring** a variable (`var x int = 10`). Use `is` when **reassigning** (`x is 20`). Think of `=` as "define" and `is` as "becomes."

---

## 3. Input

```indent
var name string = ask("What is your name? ")
var age int = ask("How old are you? ")

say "Hello " + name + "! You are " + string(age) + " years old."
```

---

## 4. Functions

Define with `fun`, return with `give`:

```indent
fun greet person
    say "Hello " + person + "!"

greet("World")   # parenthesized call — works everywhere
greet "World"    # bare call — also works (Indent style)
```

Multiple parameters and return values:
```indent
fun add a b
    give a + b

var result int = add(5, 7)
say "5 + 7 = " + string(result)
```

---

## 5. Branching

Indent uses `if`, `or` (not `elif`), and `otherwise` (not `else`):

```indent
var score int = 85

if score >= 90
    say "A"
or score >= 80
    say "B"
or score >= 70
    say "C"
otherwise
    say "F"
```

Combine conditions with `and`, `or`, `not`:
```indent
if score >= 60 and score < 70
    say "Borderline pass"
```

---

## 6. Loops

One keyword for all loops: `repeat`

```indent
# Counted loop — Reps is a built-in variable: 0 on first iteration
repeat 5
    say "Iteration " + string(Reps + 1)

# Loop over a list
var colors dynamic = ["red", "green", "blue"]
repeat color in colors
    say "Color: " + color

# Conditional loop
var n int = 0
repeat until n >= 5
    say n
    n is n + 1
```

Loop control:
| Keyword | Action |
|---|---|
| `stop` | Break out of loop |
| `next` | Skip to next iteration |
| `reset` | Restart loop from 0 |

---

## 7. Lists & Dictionaries

```indent
var fruits list = ["apple", "banana", "cherry"]
say fruits[0]                      # "apple"

var person dict = {"name": "Ada", "age": 28}
say person["name"]                 # "Ada"
say person.name                    # "Ada" — dot notation
person["age"] is 29                # Modify value
```

---

## 8. Imports

```indent
get math                           # Whole module
say math.PI

get Pow from math                  # Single function
say Pow(2, 8)

get RandInt from random as R       # With alias
say R(1, 100)
```

---

## 🎯 Exercises

### Exercise 1: Greeting
Write a program that asks for the user's name and age, then prints: `"Hello <name>! In 5 years you'll be <age+5>."`

### Exercise 2: Even or Odd
Write a function `CheckNumber(n)` that prints `"Even"` or `"Odd"` depending on the input. Test it with numbers 0 through 5 using `repeat`.

### Exercise 3: Favorites List
Create a list of your 3 favorite things. Loop over the list and print each one prefixed with `"I like "`.

### 🔥 Challenge: Countdown
Write a loop that counts down from 10 to 1, printing each number, then prints `"Blastoff!"`. Use `repeat until` and a mutable variable.

---

## 📖 Next Lesson
→ [Lesson 02: Functions & Types](02-functions-and-types.md)
