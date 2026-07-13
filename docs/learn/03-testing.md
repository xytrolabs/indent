# Lesson 3 — Testing & Modules

## Writing Tests

Indent has built-in assertions. Write test files in a `tests/` folder:

```indent
#! tests/math_test.ind

var result int = 2 + 2
assert_eq result 4 "2 + 2 should equal 4"

fun double n
    give n * 2

assert_eq double(3) 6 "double(3) should be 6"
assert_eq double(0) 0 "double(0) should be 0"
```

Run tests:
```bash
indent test tests/
```

## Using Modules

Import modules with `get`:

```indent
#! Import a whole module
get math
say math.PI           #! 3.141592...

#! Import specific functions
get Pow from math
say Pow 2 8           #! 256.0

#! Import with an alias
get RandInt from random as Random
say Random 1 100       #! random number between 1-100
```

## Available Standard Modules

| Module | What it does |
|--------|-------------|
| `math` | `PI`, `Pow`, `Sqrt`, `Sin`, `Cos`, `Log`, `Abs`, `Floor`, `Ceil`, … |
| `os` | `GetCwd`, `ReadText`, `WriteText`, `Exists`, `Mkdir`, `ListDir`, … |
| `json` | `Loads`, `Dumps` — parse and serialize JSON |
| `time` | `Time`, `Sleep` — timestamps and delays |
| `random` | `Random`, `RandInt`, `Choice`, `Shuffle` |
| `http` | `http_get`, `http_post` — HTTP requests |
| `builtins` | `Len`, `TypeOf`, `Input`, `Assert`, `AssertEq` |

## Creating Your Own Module

Create a `.ind` file in `packages/` or your project folder:

```indent
#! packages/greetings.ind

fun Hello
    name
    say "Hello " + name + "!"

fun Goodbye
    name
    say "Goodbye " + name + "!"
```

Then import it:
```indent
get Hello from greetings
Hello "World"
```

## Project Structure

```
my-project/
├── main.ind          #! entry point
├── packages/         #! your modules
├── tests/            #! test files
└── .env              #! environment variables (optional)
```

## Practice

1. Create a module `calculator.ind` with `add`, `subtract`, `multiply`, `divide` functions.
2. Write tests for each function in `tests/calculator_test.ind`.
3. Import `calculator` into `main.ind` and build a simple CLI calculator.

---

## Next Lessons

| Lesson | Topic |
|--------|-------|
| [Lesson 4](04-lists-and-dictionaries.md) | Lists & Dictionaries |
| [Lesson 5](05-strings-and-builtins.md) | Strings & Built-in Functions |
| [Lesson 6](06-error-handling.md) | Error Handling (do/catch/flag) |
| [Lesson 7](07-file-io-and-os.md) | File I/O & OS Operations |
| [Lesson 8](08-advanced-functions.md) | Advanced Functions & Recursion |
| [Lesson 9](09-data-and-json.md) | JSON, HTTP, Time & Random |
