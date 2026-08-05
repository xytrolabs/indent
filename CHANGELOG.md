# Indent Changelog

## 1.4.1 — 2026-08-04

### 🎯 Group Type — Unique Collections
```indent
var s = group [1, 2, 2, 3]     # → {1, 2, 3}
var u = s + group [3, 4]        # → {1, 2, 3, 4}
contains(s, 2)                # → TRUE
repeat item in s              # iteration
[x * 2 for x in s]            # comprehension
```

### 🔄 Type Conversion Syntax
```indent
var name1 = 21
set name1 string               # → "21"
set name1 int                  # → 42
set name1 boolean              # → TRUE (non-zero=true)
set name1 set                  # → {21} (list→set)
```

### 🧹 Code Cleanup
- Removed duplicate keywords (get/next were listed twice)
- Removed dead makeType keyword
- Deleted unused parse_function_signature and parse_return_type
- Zero compiler warnings

---

## 1.3.0 — 2026-08-04

### 🐍 Python-Style Type Inference
```indent
var x = 42          # → int (inferred)
var name = "Ada"    # → string (inferred)
var flag = true     # → boolean (inferred)
var nums = [1,2,3]  # → list (inferred)
```

No more typing `var x int = 42` when the value makes the type obvious. Explicit types still work: `var x int = 42`.

### ➕ Compound Assignment Operators
```indent
x += 8     # x = x + 8
x -= 10    # x = x - 10
x *= 2     # x = x * 2
x /= 3     # x = x / 3
x %= 5     # x = x % 5
```

Works with any numeric variable and supports list/dict merge with `+=`.

---

## 1.2.0 — 2026-08-03

### 🎉 Major Features
**Default parameters**, **string interpolation**, **comprehensions**, **lambdas**, **ternary**, **bitwise ops**, **chained comparisons**, **identity operators**, `null`/`for`/`import`/`open` keywords.

### 📦 New Builtins (20+)
**Regex** (5), **Datetime** (3), **Crypto** (4), **Path** (4), **Functional** (2), **String helpers** (3)

### 📚 AIR Package Manager
`air install`, `air search`, `air publish` — 17 standard packages.

---

## 1.0.0 — Initial Release
Core language, 130+ builtins, classes, match/case, error handling, debugger, LSP, REPL.
