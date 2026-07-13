# Indent → Python Competitiveness Roadmap

## Executive Summary
Indent currently has ~50 built-in functions and 13 modules. Python has ~200 stdlib modules. To become a true competitor while staying simple, focus on **high-impact, commonly-used features** that solve real scripting problems.

---

## Current State Analysis

### What Indent Has ✅
**Built-in Functions (50+)**:
- Data structures: len, range, slice, split, join, append, extend, insert, pop, remove, enumerate, zip
- Numeric: int, float, abs, inc, dec, add_int, sub_int, mul_int, div_int, mod_int
- String basics: starts_with, ends_with, contains, find
- Collection ops: sort, reverse, sum, min, max, any, all, count
- Dict ops: keys, values, has_key, items, dict_get, dict_set, dict_remove
- Type checks: is_missing, bool, string, type_of
- Utilities: assert, process_exit, clamp, default, coalesce

**Modules (13)**:
- discord, json, os, sys, time, random, math, http, path, colors, agame, datetime, builtins

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

#### 1. **Regex (re module) - CRITICAL** 🔴
- Python: `import re; re.match(), re.search(), re.sub(), re.split()`
- Impact: Text parsing, validation, log analysis, data extraction
- Current workaround: split() + string operations (brittle)
- Difficulty: Medium (need pattern compilation, captures, groups)
- ROI: **Very High** – regex is fundamental to scripting

#### 2. **String Methods (native on string type)** 🔴
- Python: `.upper(), .lower(), .strip(), .lstrip(), .rstrip(), .replace(), .capitalize(), .title(), .swapcase(), .center(), .ljust(), .rjust(), .zfill()`
- Current workaround: None (missing entirely)
- Difficulty: Low (simple string operations)
- ROI: **Very High** – used constantly in text processing

#### 3. **List/Dict Methods (native on collection types)** 🔴
- Python: `.copy(), .clear(), .index(), .count()` on lists; `.copy(), .clear(), .get(), .setdefault()` on dicts
- Current: Functional style only (dict_get, append, etc.)
- Difficulty: Low
- ROI: **High** – ergonomic improvement

#### 4. **List/Dict Comprehensions** 🔴
- Python: `[x*2 for x in items if x > 0]`, `{k: v for k, v in pairs}`
- Impact: Concise data transformation
- Current workaround: repeat loops + append
- Difficulty: Medium (parser/evaluator changes)
- ROI: **High** – widely used, more readable

#### 5. **Tuples & Sets** 🔴
- Python: `(1, 2, 3)` tuples (immutable), `{1, 2, 3}` sets (unique, fast membership)
- Current: Lists only (mutable), no sets
- Difficulty: Medium (new value types in runtime)
- ROI: **High** – essential for correct semantics (e.g., dict keys must be hashable)

#### 6. **Filesystem Operations (glob, walk, pathlib-like)** 🔴
- Python: `glob.glob("*.txt")`, `os.walk()`, `Path.iterdir()`, `Path.glob()`
- Current: `os_list_dir()` only (no recursion, no pattern matching)
- Impact: File discovery, batch operations
- Difficulty: Medium
- ROI: **Very High** – common in file-based scripts

#### 7. **Exception Handling (try-except-finally)** 🔴
- Python: `try: ... except ValueError: ... finally: ...`
- Current: do-catch (basic), no exception types/hierarchy
- Difficulty: Medium
- ROI: **High** – critical for robustness

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

#### 11. **Classes & OOP (full support)** 🟡
- Python: `class Foo: def __init__(): ...`
- Current: Basic object model, no inheritance, no special methods
- Difficulty: Hard (major runtime work)
- ROI: **Medium** – powerful but not essential for scripting

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

#### 14. **Type Hints with Runtime Checking** 🟢
- Python: `def foo(x: int) -> str: ...`
- Current: Type annotations syntax exists, but no runtime enforcement
- Difficulty: Medium
- ROI: **Low** – mostly for documentation

#### 15. **Context Managers (with statement)** 🟢
- Python: `with open("file") as f: ...`
- Current: try-finally required
- Difficulty: Medium
- ROI: **Low-Medium** – syntactic sugar

#### 16. **String Formatting (f-strings, format())** 🟢
- Python: `f"{x:.2f}"`, `"{0} {1}".format(a, b)`
- Current: String interpolation exists via concatenation
- Difficulty: Low
- ROI: **Low** – concatenation works, less elegant

#### 17. **Bitwise Operations** 🟢
- Python: `&, |, ^, <<, >>, ~`
- Impact: Low-level ops, flags, bit manipulation
- Current: None
- Difficulty: Low
- ROI: **Low** – niche use cases

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

5. **Tuples & Sets**
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
4. **Tuples/Sets**: Fix semantic correctness issues
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
