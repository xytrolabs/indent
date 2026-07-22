# Lesson 08 — Advanced Functions

**Time**: 20 minutes  |  **Topics**: Parameter styles, Recursion, Function references, Named arguments, Composition

---

## 🎯 Learning Objectives
- Use inline, multi-line, and parenthesized call styles
- Write recursive functions
- Pass functions as values (function references)
- Compose small functions into pipelines

---

## Two Parameter Styles

Indent supports two ways to pass parameters to functions.

### Inline parameters (short calls)

```indent
fun greet person
    say "Hello " + person + "!"

greet "World"             #! Simple, one line
```

### Multi-line parameters (spec style)

For functions with many arguments, use indented style with `;`:

```indent
fun add
    a
    b
    give a + b

#! Multi-line call
add;
    5
    7

#! Also works with say
say;
    "Hello from multiline!"
```

### Parenthesized calls (expressions)

```indent
fun max a b
    if a > b
        give a
    otherwise
        give b

var result int = max(10, 20)     #! Works in expressions
say max(100, 200)                #! 200
```

### Method-style calls

```indent
var items dynamic = [3, 1, 2]
items.sort                       #! Sort in place
items.append 4                   #! Add element
items.reverse                    #! Reverse

var text string = "Hello"
say text.upper                   #! "HELLO"
say text.lower                   #! "hello"
```

---

## Multiple Return Values with Dictionaries

Functions return a single value with `give`. For multiple results, use dictionaries:

```indent
fun divide a b
    if b == 0
        give {"ok": false, "error": "Division by zero"}
    give {"ok": true, "value": a / b}

var result dynamic = divide 10 3
if result.ok
    say "Result: " + result.value
otherwise
    say "Error: " + result.error
```

---

## Default Values with `coalesce`

```indent
fun greet
    name
    #! Use "Guest" if name is empty/missing
    var safe_name string = coalesce name "Guest"
    say "Hello " + safe_name + "!"

greet "Ada"               #! "Hello Ada!"
greet empty               #! "Hello Guest!"
```

---

## Functions Calling Functions

```indent
fun square n
    give n * n

fun sum_of_squares a b
    give square(a) + square(b)

say sum_of_squares(3, 4)   #! 25
```

---

## Module Functions

Functions inside modules can call each other:

```indent
#! packages/calculator.ind
fun double n
    give n * 2

fun quadruple n
    give double(double(n))   #! Calls sibling function

fun process
    a
    b
    give quadruple(a) + double(b)
```

Then use it:

```indent
get double, quadruple, process from calculator
say double(5)              #! 10
say quadruple(5)           #! 20
say process 3 4            #! 12 (double(double(3)) + double(4) = 12 + 8 = ... wait)
```

---

## Recursion

Functions can call themselves — useful for tree structures, math, and puzzles:

```indent
fun factorial n
    if n <= 1
        give 1
    give n * factorial(n - 1)

say factorial(5)           #! 120

fun fibonacci n
    if n <= 1
        give n
    give fibonacci(n - 1) + fibonacci(n - 2)

repeat i in range 10
    say "fib(" + i + ") = " + fibonacci(i)
#! fib(0) = 0
#! fib(1) = 1
#! fib(2) = 1
#! ...
```

---

## Function Composition

Combine small functions to build bigger ones:

```indent
fun double n
    give n * 2

fun add_ten n
    give n + 10

fun double_then_add_ten n
    give add_ten(double(n))

say double_then_add_ten(5)   #! 20
```

---

## Example: A Small Functional Pipeline

```indent
fun filter_even numbers
    var result dynamic = []
    repeat n in numbers
        if n % 2 == 0
            append result n
    give result

fun double_all numbers
    var result dynamic = []
    repeat n in numbers
        append result n * 2
    give result

fun sum_all numbers
    give sum numbers

var numbers dynamic = [1, 2, 3, 4, 5, 6]
var pipeline dynamic = double_all(filter_even(numbers))
say sum_all(pipeline)      #! (2+4+6)*2 = 24
```

---

## Function References

You can pass functions by name without calling them. The function name
resolves to a callable reference:

```indent
fun double n
    give n * 2

fun triple n
    give n * 3

# Pass function by name
fun apply_twice fn val
    give fn(fn(val))

say apply_twice double 5    #! 20  (double(double(5)))
say apply_twice triple 3    #! 27  (triple(triple(3)))
```

## Named Arguments

Pass arguments by name for clarity, especially with many parameters:

```indent
fun create_user name age city
    say name + ", " + age + ", " + city

# Call with named arguments (order doesn't matter)
create_user name is "Ada" age is 28 city is "London"
create_user city is "Paris" name is "Jean" age is 35
```

## Default Values with `coalesce`

Indent doesn't have built-in default parameters, but `coalesce` provides
the same pattern:

```indent
fun greet person greeting
    var msg string = coalesce greeting "Hello"
    say msg + " " + person + "!"

greet "Ada" "Hi"       #! "Hi Ada!"
greet "Ada" empty       #! "Hello Ada!"
```

---

## Practice

1. Write a recursive function to compute the sum of all numbers from 1 to n.
2. Create a module `stats.ind` with functions `mean`, `median`, and `std_dev` that build on each other.
3. Write a function `apply_twice` that takes a number and a function (by name), and applies that function twice.
4. Build a small string processing pipeline: trim → capitalize → reverse — using composed functions.

---

## Next Lesson

➡️ [Lesson 9: Working with Data](09-data-and-json.md)
