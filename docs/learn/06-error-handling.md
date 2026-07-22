# Lesson 06 — Error Handling

**Time**: 25 minutes  |  **Topics**: do/catch/flag, Result type, Assertions, Error codes E001–E012

---

## 🎯 Learning Objectives
- Handle errors with `do`/`catch`/`otherwise`/`lastly`
- Raise your own errors with `flag`
- Use assertions for debugging
- Work with the Result type for explicit success/failure
- Understand all 12 Indent error codes

---

Errors happen — files missing, network down, bad input. Indent gives you clean tools to handle them.

## The `do` / `catch` / `otherwise` / `lastly` Block

```indent
do:
    var data string = file_read_text "missing.txt"
    say "Got: " + data
catch as err:
    say "Something went wrong: " + err
lastly:
    say "This always runs, error or not"
```

The four parts:
- **`do:`** — The code that might fail
- **`catch as err:`** — Runs if an error happens; `err` holds the error message
- **`otherwise:`** — Runs only if `do:` completes WITHOUT an error
- **`lastly:`** — Always runs, like a cleanup block

### Using `otherwise`

```indent
do:
    var result int = int_or "42" 0
    say "Parsed successfully!"
otherwise:
    say "This won't run — no error occurred"

#! Only do or otherwise runs, never both
```

### Example: retry logic

```indent
fun read_with_retry
    path
    max_attempts
    var attempt int = 0
    repeat while attempt < max_attempts
        do:
            var content string = file_read_text path
            give content
        catch as err:
            say "Attempt " + (attempt + 1) + " failed: " + err
            attempt is attempt + 1
            if attempt < max_attempts
                say "Retrying..."
            otherwise
                give ""

say read_with_retry "config.json" 3
```

---

## Raising Errors with `flag`

Use `flag` to raise your own errors:

```indent
fun divide a b
    if b == 0
        flag "Division by zero!"
    give a / b

do:
    var result int = divide 10 0
catch as err:
    say "Error: " + err     #! "Error: Division by zero!"
```

### Validation example

```indent
fun validate_age age
    if age < 0
        flag "Age cannot be negative"
    or age > 150
        flag "Age seems unrealistic"
    give true

do:
    validate_age -5
catch as err:
    say "Validation failed: " + err
```

---

## Defensive Programming Patterns

### Checking before acting

```indent
var path string = "/tmp/data.txt"

if os_exists path
    var content string = file_read_text path
    say "Read " + len content + " characters"
otherwise
    say "File doesn't exist, creating it..."
    file_write_text path "default data"
```

### Safe type conversion

```indent
#! Use int_or / float_or instead of raw int() / float()
var port int = int_or os_getenv "PORT" "8080"
var timeout float = float_or os_getenv "TIMEOUT" "30.0"

#! If env vars are missing or invalid, you get the fallback
say "Port: " + port
say "Timeout: " + timeout
```

### Handling network errors

```indent
do:
    var response dynamic = http_get "https://api.example.com/data"
    say "Got response!"
catch as err:
    say "Network error: " + err
    #! Maybe try a fallback
    var response dynamic = http_get "https://backup-api.example.com/data"
```

---

## Common Error Patterns

| Pattern | How to handle it |
|---------|-----------------|
| File not found | Check `os_exists` first, or catch the error |
| Invalid input | Use `int_or` / `float_or` with fallbacks |
| Network failure | Wrap in `do/catch` with retry logic |
| Division by zero | Check divisor before dividing |
| Missing config | Use `default` or `coalesce` with fallbacks |
| API error response | Check response status after HTTP call |

### Error Codes Reference

When Indent reports an error, it gives you a code to help diagnose:

| Code | Meaning | Example |
|---|---|---|
| E001 | Type mismatch | Passing a string where an int is expected |
| E002 | Undefined function | Calling a function that doesn't exist |
| E003 | Import error | Module not found in search paths |
| E004 | Syntax error | Missing quote, bad indentation, unexpected token |
| E005 | Unwrap error | `.unwrap()` on an error value |
| E006 | Undefined variable | Using a variable before declaring it with `var` |
| E007 | Division by zero | `x / 0` or `x % 0` |
| E008 | Index out of range | `list[99]` when list has 3 items |
| E009 | Key not found | `dict["missing_key"]` |
| E010 | File not found | Trying to read a file that doesn't exist |
| E011 | Invalid JSON | Malformed JSON string in `json_loads` |
| E012 | Network error | Connection refused, timeout, DNS failure |

---

## Assertions

Assertions are a simple way to validate assumptions and catch bugs early. If the condition is falsy, the program crashes with a clear message.

```indent
#! Basic assertion
assert 2 + 2 == 4

#! With a custom message
var age int = ask("int", "Age: ")
assert age > 0 "Age must be positive"

#! Compare values (shows both values on failure)
assert_eq calc_total() 100
assert_eq get_name() "Alice" "Name should match"
```

**When to use asserts vs do/catch:**
- `assert` — for programming errors (bugs that should never happen)
- `do`/`catch` — for expected runtime failures (network, files, user input)
- `assert_eq` — for testing expected vs actual values

---

## Result Type (v2.5+)

Indent supports a Result type for explicit success/failure handling without exceptions:

```indent
#! Create success and error results
var good result = ok 42
var bad result = err "Something went wrong"

#! Check result state
if is_ok good
    say "Success!"

if is_err bad
    say "Got an error"

#! Unwrap with fallback
var value int = unwrap good 0       # → 42
var safe int = unwrap bad 0         # → 0 (fallback)

#! Unwrap without fallback (crashes on error)
var required int = unwrap good      # → 42
# var crash int = unwrap bad        # would crash

#! Try: wraps expression evaluation in a result
var result = try int_or "abc" 0     # catches conversion failure
```

**Chaining with unwrap:**
```indent
#! Build a pipeline where each step may fail
fun fetch_user id
    if id <= 0
        give err "Invalid user ID"
    give ok "user_" + id

fun lookup_name user
    if user == "user_42"
        give ok "Arthur"
    otherwise
        give err "User not found"

#! Chain them safely
var user_result = fetch_user ask("int", "ID: ")
if is_err user_result
    flag "Failed to fetch user"
var name_result = lookup_name (unwrap user_result)
var name string = unwrap name_result "Unknown"
say "Hello, " + name
```

---

## Practice

1. Write a function `safe_divide` that takes two numbers and returns the result, or raises an error if the divisor is zero.
2. Write a script that tries to read a config file, and creates it with defaults if it doesn't exist.
3. Write a function `parse_number` that takes a string and returns the integer, or flags an error with a helpful message if parsing fails.
4. Build a robust CLI calculator that handles invalid input gracefully using `do/catch`.

---

## Next Lesson

➡️ [Lesson 7: File I/O & OS Operations](07-file-io-and-os.md)
