# Lesson 05 — Strings & Built-ins

**Time**: 25 minutes  |  **Topics**: String operations, Type conversion, `range`, `enumerate`, `zip`, Math helpers

---

## 🎯 Learning Objectives
- Manipulate strings: case, trim, split, join, search
- Convert between types safely with `int_or`/`float_or`
- Use `range`, `enumerate`, and `zip` for iteration
- Apply math and utility helpers (`clamp`, `inc`, `abs`, etc.)

---

## Strings

Strings are text values, created with double quotes:

```indent
var greeting string = "Hello, Indent!"
var empty string = ""
```

### Concatenation

Use `+` to join strings:

```indent
var first string = "Ada"
var last string = "Lovelace"
var full string = first + " " + last
say full                 #! "Ada Lovelace"
```

Numbers are automatically converted when concatenating:

```indent
var age int = 28
say "I am " + age + " years old"   #! "I am 28 years old"
```

### String indexing

```indent
var text string = "Indent"
say text[0]              #! "A"
say text[1]              #! "e"
say text[-1]             #! "r"
say text[0:3]            #! "Aet"
say text[3:]             #! "her"
say text[::-1]           #! "rehtA" (reversed)
```

### String operations

```indent
var msg string = "  Hello, World!  "

say len msg               #! 17
say upper msg             #! "  HELLO, WORLD!  "
say lower msg             #! "  hello, world!  "
say trim msg              #! "Hello, World!"
say lstrip msg            #! "Hello, World!  "
say rstrip msg            #! "  Hello, World!"
say capitalize msg        #! "  hello, world!  "
say title "hello world"  #! "Hello World"
say swapcase "Hello"      #! "hELLO"

#! Search and replace
say replace "foo bar foo" "foo" "baz"  #! "baz bar baz"
say contains "hello world" "world"     #! true
say starts_with "Indent" "Ae"          #! true
say ends_with "Indent" "er"            #! true
say find "hello world" "world"         #! 6
say index "hello world" "world"        #! 6
```

### Splitting and joining

```indent
var csv string = "apple,banana,cherry"
var parts dynamic = split csv ","
say parts                #! ["apple", "banana", "cherry"]

var joined string = join parts " | "
say joined               #! "apple | banana | cherry"
```

### Converting types

```indent
var n int = int "42"
var f float = float "3.14"
var b boolean = bool "true"
var s string = string 42

say n + 8                #! 50
say f + 1.0              #! 4.14
say s                    #! "42"

#! Safe conversion with fallback
var safe int = int_or "not-a-number" 0
say safe                 #! 0
```

---

## Built-in Collection Functions

### Working with lists

```indent
var nums dynamic = [3, 7, 2, 9, 1, 7]

say count nums 7         #! 2
say any [false, true, false]   #! true
say all [true, true, true]     #! true
say sum nums             #! 29

#! Copy a list
var backup dynamic = copy nums

#! Check membership
say contains nums 9      #! true

#! Extend one list with another
var more dynamic = [10, 11]
extend nums more
say nums                 #! [3, 7, 2, 9, 1, 7, 10, 11]
```

### Using `range`

```indent
say range 5              #! [0, 1, 2, 3, 4]
say range 2 6            #! [2, 3, 4, 5]
say range 1 10 2         #! [1, 3, 5, 7, 9]

var squares dynamic = []
repeat i in range 5
    append squares i * i
say squares              #! [0, 1, 4, 9, 16]
```

### Using `enumerate` and `zip`

```indent
var names dynamic = ["Alice", "Bob", "Charlie"]

#! enumerate gives [index, value] pairs
repeat pair in enumerate names
    say pair[0] + ": " + pair[1]
#! 0: Alice
#! 1: Bob
#! 2: Charlie

#! zip pairs up two lists
var scores dynamic = [95, 87, 92]
repeat pair in zip names scores
    say pair[0] + " scored " + pair[1]
#! Alice scored 95
#! Bob scored 87
#! Charlie scored 92
```

---

## Utility Helpers

### Default values and coalescing

```indent
var config dynamic = {}

#! Provide a default if a value is missing/empty
var timeout int = default dict_get config "timeout" 30
say timeout              #! 30

#! Coalesce — first non-missing value wins
var name string = coalesce dict_get config "name" "Guest"
say name                 #! "Guest"

#! Check if something is "empty"
say is_missing empty     #! true
say is_missing ""        #! true
say is_missing []        #! true
say is_missing {}        #! true
say is_missing 0         #! false
```

### Clamping and bounds

```indent
say clamp 150 0 100      #! 100
say clamp -5 0 100       #! 0
say clamp 42 0 100       #! 42

var age int = 25
say between_int age 18 65  #! true
```

### Increment and decrement

```indent
var count int = 0
count is inc count       #! 1
count is inc count 5     #! 6
count is dec count 2     #! 4
```

## More String Functions

```indent
# Left/right trim
say lstrip "  hello  "   #! "hello  "
say rstrip "  hello  "   #! "  hello"

# Case transformations
say capitalize "hello"    #! "Hello"
say title "hello world"   #! "Hello World"
say swapcase "Hello"      #! "hELLO"
```

## Safe Integer Arithmetic

When you need guaranteed integer results (no float surprises):

```indent
say add_int 5 3           #! 8
say sub_int 10 4          #! 6
say mul_int 6 7           #! 42
say div_int 10 3          #! 3  (truncates, no decimal)
say mod_int 10 3          #! 1  (remainder)

# Quick checks
say is_even 42            #! true
say is_odd 7              #! true
say between_int 50 0 100  #! true
say abs -5                #! 5
```

## Practice

1. Write a function `count_words` that takes a sentence and returns the number of words.
2. Write a function `is_palindrome` that checks if a string reads the same forwards and backwards.
3. Use `range`, `enumerate`, and `zip` together to print a numbered list of students and their grades.
4. Write a script that safely reads a number from the user (using `int_or`) and prints its square.

---

## Next Lesson

➡️ [Lesson 6: Error Handling](06-error-handling.md)
