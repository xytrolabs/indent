# Indent → Python Competitiveness Roadmap

> **🆕 Status update (2026-08):** Many items once listed as gaps are now
> **implemented**. For an accurate, current side-by-side with Python, see
> [**Indent vs Python — Side-by-Side Comparison**](indent-vs-python.md).
> This roadmap keeps the remaining gaps and history, with ✅ marking what's
> now shipped.

## Executive Summary
Indent is a small, readable language that already implements most of the
high-impact Python scripting features: regex, string methods, comprehensions,
ordered groups, glob, do/catch error handling, classes with inheritance,
bitwise ops, formatting, and a reversible package manager (`air`). The
remaining gaps vs Python are mostly breadth (stdlib size, async, decorators,
tuples, YAML/TOML/CSV).

---

## Current State Analysis

### What Indent Has ✅
**Built-in Functions (60+)**:
- Data structures: len, range, slice, split, join, append, extend, insert, pop, remove, enumerate, zip, group, set
- Numeric: int, float, abs, add_int, sub_int, mul_int, div_int, mod_int, min, max, sum
- String: upper, lower, trim, capitalize, title, swapcase, replace, starts_with, ends_with, contains, find, count, format, sformat
- Regex: regex_match, regex_search, regex_findall, regex_replace, regex_split
- Collection ops: sort, reverse, map, filter, any, all, count, glob
- Dict ops: keys, values, has_key, items, dict_get, dict_set, dict_remove, dict_update
- Type checks: is_missing, bool, string, type_of, type_name
- Utilities: assert, process_exit, clamp, default, coalesce, uuid, base64, hash_*

**Modules (25+)**:
- std: io, math, strings, testing, time, random, json, os, sys, path, fs, hash,
  base64, datetime, regex, net, collections, ai, ingame, agame, discord
- Native builtins: http, colors, websocket, and many others

**Runtime Features**:
- Static typing with type annotations
- Module imports with external function invocation
- Control flow: if/else, match, do-catch, repeat
- Function definitions with parameter defaults
- Websocket support (native)
- JSON parsing (native)
- File I/O (read, write, append)
- HTTP requests (GET, POST, PUT, PATCH, DELETE)

---

## Critical Gaps vs Python

### Tier 1: Blocking / High-Priority (Most Asked For)

> ✅ **Implemented now:** regex, string methods, list/dict methods,
> comprehensions, groups, exception handling (do/catch/lastly), classes with
> inheritance, bitwise ops, string formatting, context-manager `open as`.
> The remaining true gaps are called out below.

#### ~~1. Regex (re module)~~ ✅ **Implemented**
- Indent: `regex_match`, `regex_search`, `regex_findall`, `regex_replace`, `regex_split`
- Impact: Text parsing, validation, log analysis, data extraction — now native

#### ~~2. String Methods~~ ✅ **Implemented**
- Indent: `.upper/.lower/.strip/.replace/.capitalize/.title/.swapcase` methods,
  plus free functions `Upper/Lower/Trim/...` and `PadLeft/PadRight`

#### ~~3. List/Dict Methods~~ ✅ **Implemented**
- Indent: functional style `append/insert/pop/remove/dict_get/dict_set/...`
  plus the `collections` module (`Append`, `DictGet`, `Filter`, `Map`, ...)

#### ~~4. List/Dict Comprehensions~~ ✅ **Implemented**
- Indent: `[x*2 for x in items if x > 0]` — list, filtered, and group comprehensions

#### 5. **Tuples** 🔴 (Groups ✅ done)
- Python: `(1, 2, 3)` tuples (immutable)
- Indent: `group([...])` gives **ordered** unique collections (dedup + `.contains`),
  but there is no immutable tuple type or hash-based set yet.

#### ~~6. Filesystem Operations~~ ✅ **Implemented (partially)**
- Python: `glob.glob("*.txt")`, `os.walk()`, `Path.iterdir()`, `Path.glob()`
- Indent: `glob(...)` now exists; a recursive `os.walk`-style API is still a gap.

#### ~~7. Exception Handling~~ ✅ **Implemented**
- Python: `try: ... except ValueError: ... finally: ...`
- Indent: `do: / catch as e: / lastly:` plus `flag:` — type hierarchy still a gap.

---

### Tier 2: Important / Medium-Priority

#### 8. **CSV Support** 🟡
- Python: `csv.DictReader()`, `csv.writer()`
- Impact: Data import/export (very common)
- Current: None (JSON only)
- Difficulty: Medium
- ROI: **High** – CSV is universal data format

#### 9. **Iterator / Generator Support** 🟡
- Python: `yield`, generators for lazy evaluation
- Impact: Memory-efficient iteration over large datasets
- Current: None
- Difficulty: Hard (fundamental runtime change)
- ROI: **Medium** – important for scalability, less critical for simple scripts

#### 10. **Decorators** 🟡
- Python: `@decorator def foo(): ...`
- Impact: Middleware, validation, caching
- Current: None
- Difficulty: Hard (parser/evaluator changes)
- ROI: **Medium** – nice-to-have for frameworks

#### ~~11. Classes & OOP~~ ✅ **Implemented**
- Indent: `class`, inheritance via `class C from P`, fields, methods — see
  [Classes vs Python/JS](learn/11-classes.md)
- Remaining gap: special methods (`__str__`, `__add__`)

#### 12. **YAML/TOML Config Support** 🟡
- Python: `yaml.load()`, `tomllib.loads()`
- Impact: Configuration file parsing
- Current: JSON only
- Difficulty: Medium
- ROI: **Medium** – important for ops/deployment

---

### Tier 3: Nice-to-Have / Lower-Priority

#### 13. **Async/Await (full support)** 🟢
- Current: Basic websocket support (limited)
- Difficulty: Hard
- ROI: **Low-Medium** – powerful but complex

#### ~~14. Type Hints with Runtime Checking~~ ✅ **Implemented**
- Indent: typed `var name type = ...`, return types `fun f as int`, and `set varname type`
  conversion — see INDENT_GUIDE. Runtime enforcement is minimal (hints/documentation).

#### ~~15. Context Managers~~ ✅ **Implemented**
- Indent: `open "file.txt" for read as f:` / `for write as f:` / `for append as f:`

#### ~~16. String Formatting~~ ✅ **Implemented**
- Indent: `format(template, a, b)` (`{0}`, `{1}`) and `sformat(template, k, v)`
  (`{key}`), plus `%name%` interpolation

#### ~~17. Bitwise Operations~~ ✅ **Implemented**
- Indent: `&, |, ^, <<, >>` operators (bitwise builtins) — see quick-reference

---

## Recommended Implementation Roadmap

### Phase 1: Text & Data Processing (Weeks 1-3) 🚀
**Goal**: Make Indent the go-to tool for log parsing, data extraction, and text wrangling.

1. **String Methods** (easiest win)
   - Add `.upper()`, `.lower()`, `.strip()`, `.lstrip()`, `.rstrip()`, `.replace()`, `.capitalize()`, `.title()`, `.split()`, `.startswith()`, `.endswith()`, `.find()`, `.count()`, `.index()` as string type methods
   - Implementation: Extend string value handling in `invoke_builtin`
   - Effort: 1-2 hours
   - Impact: Immediate usability improvement

2. **Regex Module (re)**
   - Add `re.match(pattern, text)`, `re.search(pattern, text)`, `re.findall(pattern, text)`, `re.sub(pattern, replacement, text)`, `re.split(pattern, text)`
   - Implementation: Use Rust `regex` crate, wrap in module
   - Effort: 4-6 hours
   - Impact: **Huge** – essential for text processing

3. **List/Dict Methods**
   - Add `.copy()`, `.clear()`, `.index()`, `.count()` on lists
   - Add `.copy()`, `.clear()`, `.get()`, `.pop()`, `.update()` on dicts
   - Implementation: Extend collection handling in runtime
   - Effort: 2-3 hours
   - Impact: High ergonomics improvement

### Phase 2: File & Data Structures (Weeks 3-5) 🗂️
**Goal**: Handle file discovery and structured data naturally.

4. **Filesystem (glob, walk)**
   - Add `glob.glob(pattern)`, `glob.glob_recursive(pattern)`
   - Add `os.walk(path)` returning iterator-like structure
   - Implementation: Use Rust `glob` and `walkdir` crates
   - Effort: 3-4 hours
   - Impact: **Very High** – file scripting becomes practical

5. **Tuples & Groups**
   - Add tuple type: `(1, 2, 3)` - immutable lists
   - Add set type: `{1, 2, 3}` - unique collections with fast membership
   - Add set operations: `.add()`, `.remove()`, `.union()`, `.intersection()`, `.difference()`
   - Implementation: New value types in runtime, extend parser
   - Effort: 6-8 hours
   - Impact: **High** – correct semantics for many problems

6. **CSV Module**
   - Add `csv.read_file(path)`, `csv.write_file(path, rows)`
   - Add simple CSV reader/writer (no fancy quoting for v1)
   - Implementation: CSV parsing logic in module
   - Effort: 2-3 hours
   - Impact: **High** – data interchange

### Phase 3: Robustness & Ergonomics (Weeks 5-7) 💪
**Goal**: Better error handling, more idiomatic code.

7. **Exception Types & Better Error Handling**
   - Extend do-catch to support typed exceptions
   - Add standard exception types: ValueError, TypeError, KeyError, IndexError, FileNotFoundError
   - Implementation: Exception value type in runtime, catch logic
   - Effort: 4-5 hours
   - Impact: **High** – production-quality error handling

8. **List/Dict Comprehensions**
   - Add syntax: `[expr for item in list if condition]`
   - Add dict comprehensions: `{key: value for ...}`
   - Implementation: Parser extension, evaluator support
   - Effort: 6-8 hours
   - Impact: **High** – more concise, readable code

### Phase 4: Advanced Features (Weeks 7+) 🎯
**Goal**: Approach Python's breadth while maintaining simplicity.

9. **YAML/TOML Config Support**
10. **Generators/Iterators** (if high demand)
11. **Decorators** (if high demand)
12. **Full Class Support with Inheritance**
13. **Async/Await Expansion**

---

## Quick Wins (Can Start Immediately)

These can be done in parallel without blocking other work:

| Feature | Effort | Impact | Start |
|---------|--------|--------|-------|
| String methods (.upper, .lower, .strip, etc.) | 1-2h | Very High | NOW |
| List/Dict methods (.copy, .clear, .index, etc.) | 2-3h | High | NOW |
| Regex (re module) | 4-6h | Very High | NOW |
| Bitwise operators (&, \|, ^, <<, >>, ~) | 1h | Low | Later |
| Exception types | 4-5h | High | Week 2 |

---

## Why This Order?

1. **String methods + regex**: 80% of scripting is text processing
2. **List/Dict methods**: Immediate ergonomic gain (method chaining vs function calls)
3. **Filesystem (glob/walk)**: File-based scripting is common
5. **Tuples/Sets**: Fix semantic correctness issues
5. **CSV**: Data import/export (universal format)
6. **Exception types**: Robustness at scale
7. **Comprehensions**: Readability & conciseness
8. **Advanced features**: Only after fundamentals are solid

---

## Competitive Positioning After Phase 1

After Phase 1 (string methods + regex + list/dict methods):
- ✅ Text processing: On par with Python's `re`, `str` modules
- ✅ Data transformation: Functional style + method chaining
- ✅ Ergonomics: Native string/list methods, no boilerplate
- ✅ Simplicity: **Still simpler than Python** (no classes, clearer control flow)

**Pitch**: "Indent: Python's simplicity + scripting power. No classes, no pip hell, no dependency nightmares."

---

## Effort Estimates (One Dev)

| Phase | Features | Time | Version |
|-------|----------|------|---------|
| Phase 1 | String methods, Regex, List/Dict methods | 1-2 weeks | 0.2.0 |
| Phase 2 | Filesystem, Tuples/Sets, CSV | 2 weeks | 0.3.0 |
| Phase 3 | Exception types, Comprehensions | 1-2 weeks | 0.4.0 |
| Phase 4 | Advanced features (on-demand) | Ongoing | 0.5.0+ |

---

## Next Steps

1. **Confirm priorities** – Do you want to start with Phase 1 (text processing focus)?
2. **Choose starting point** – String methods + regex? Or another combination?
3. **Define "simple"** – What's the acceptable complexity level for new features?
4. **Measure success** – What makes Indent "competitive"? Feature count? Developer experience? Speed?
