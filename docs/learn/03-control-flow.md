# Lesson 03 — Control Flow

**Time**: 25 minutes  |  **Topics**: If/Or/Otherwise, Match, Boolean logic, Conditions

---

## 🎯 Learning Objectives
- Use `if`/`or`/`otherwise` for decision-making
- Understand how Indent's `or` differs from `elif`/`else if`
- Use `match` for multi-way branching
- Write conditions with function calls and operators

---

## If / Or / Otherwise

Indent's branching reads like English:

```indent
var temp int = 25

if temp > 30
    say "It's hot outside!"
or temp > 20
    say "It's warm outside!"
or temp > 10
    say "It's cool outside!"
otherwise
    say "It's cold outside!"
```

Key differences from other languages:
- `or` instead of `elif` / `else if`
- `otherwise` instead of `else`
- **No colons** after conditions
- **No parentheses** around conditions

### How It Works
Indent checks each condition in order. The first `true` branch runs. If none match, `otherwise` runs. Only ONE branch ever executes.

```indent
var score int = 85

if score >= 90
    say "A"
or score >= 80
    say "B"          # ← This runs, then the chain stops
or score >= 70
    say "C"          # ← Skipped even though condition is true
otherwise
    say "F"          # ← Skipped
```

---

## Conditions With Function Calls

Use **parenthesized syntax** for calling functions inside conditions:

```indent
var name string = "Ada"

# ✅ Parenthesized calls work
if starts_with(name, "A")
    say "Name starts with A"

# ✅ Multiple conditions with and/or
if len(name) > 0 and starts_with(name, "A")
    say "Non-empty name starting with A"

# ❌ Space-separated calls do NOT work in conditions
# if starts_with name "A"         ← ERROR!
```

### Common Condition Patterns

```indent
# Check if list is non-empty
var items dynamic = [1, 2, 3]
if len(items) > 0
    say "We have items!"

# Check if value is in a list
if contains(items, 2)
    say "Found 2!"

# Check dict key
var person dynamic = {"name": "Ada"}
if has_key(person, "name")
    say "Has name: " + person["name"]
```

---

## Boolean Logic

### Comparison Operators
| Operator | Meaning |
|---|---|
| `==` | Equal to |
| `!=` | Not equal to |
| `>` | Greater than |
| `<` | Less than |
| `>=` | Greater or equal |
| `<=` | Less or equal |

### Logical Operators
```indent
if age >= 18 and country == "US"
    say "Can vote in US"

if color == "red" or color == "blue"
    say "Primary color"

if not active
    say "Account is inactive"
```

### Truthiness
```indent
# These are "falsy" (evaluate to false in conditions)
empty              # null
FALSE / false      # boolean false
0                  # integer zero
0.0                # float zero
""                 # empty string
[]                 # empty list
{}                 # empty dict

# Everything else is "truthy"
```

---

## Match Statement

For comparing one value against multiple possibilities:

```indent
var day string = "Monday"

match day
    "Monday"
        say "Start of the week!"
    "Friday"
        say "Almost weekend!"
    "Saturday"
        say "Weekend!"
    "Sunday"
        say "Weekend!"
    otherwise
        say "Midweek day"
```

Match is cleaner than chained `if`/`or` when comparing the same variable to multiple values.

---

## Nested Conditions

```indent
var logged_in boolean = true
var is_admin boolean = false

if logged_in
    if is_admin
        say "Welcome, Admin!"
    otherwise
        say "Welcome, User!"
otherwise
    say "Please log in"
```

💡 **Tip**: Deep nesting can be hard to read. Use early returns (`give`) in functions to reduce nesting.

---

## 🎯 Exercises

### Exercise 1: Grade Calculator
Write a program that takes a score (0-100) and prints the letter grade:
- 90+ → A
- 80-89 → B
- 70-79 → C
- 60-69 → D
- Below 60 → F

### Exercise 2: Season Detector
Given a month name (string), print which season it belongs to:
- December, January, February → Winter
- March, April, May → Spring
- June, July, August → Summer
- September, October, November → Fall

Use `match` for this one.

### Exercise 3: Login Validator
Write a function that takes `username` and `password` strings. Print:
- "Welcome!" if username is "admin" and password is "secret"
- "Wrong password" if username is "admin" but password is wrong
- "Unknown user" otherwise

### 🔥 Challenge: FizzBuzz
Print numbers 1 to 20. For multiples of 3 print "Fizz", for multiples of 5 print "Buzz", for multiples of both print "FizzBuzz", otherwise print the number. Use `repeat` and `if`/`or`.

---

## 📖 Next Lesson
→ [Lesson 04: Lists & Dictionaries](04-lists-and-dictionaries.md)
