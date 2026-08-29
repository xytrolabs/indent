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
tuples, YAML/TOML/CSV). For the full **"replace Python" strategy and the
ecosystem / tooling / runtime dimensions**, see [Reaching Full Python
Parity](#reaching-full-python-parity--strategy) below.

---

## Design Principle — Simplicity First (non-negotiable)

> **Indent is made to be *easier to learn* and *simpler to use* than Python.**
> Every parity feature below must pass this test or it gets reworked/rejected.

When closing a Python gap, prefer, in order:
1. **Additive builtins / std functions** — just more functions, *no new syntax
   to learn* (e.g. `walk`, `csv_read`, `sqlite_query`, `error_type`).
2. **Small, consistent syntax** that reuses existing keywords (e.g. `with` as
   an alias for the existing `open ... as` context manager).
3. **A simpler Indent idiom** over cloning Python verbatim — if Python's
   approach is complex and Indent has a cleaner way, keep the cleaner way.

**Rule of thumb:** a feature that adds Python parity but *increases* the
learning curve or cognitive load is rejected. Simplicity wins over parity.

---

## Reaching Full Python Parity — Strategy

### The honest goal: replace Python in its niches, not achieve full parity
Python is 30+ years old with ~200 stdlib modules and a ~500k-package ecosystem.
Literal 100% parity is years of work and the wrong target. The realistic,
high-value goal is to **replace Python in the everyday niches** where most
"I'll just use Python" decisions happen — scripts, CLI tools, bots, games,
small web servers, teaching — while being honest that data science / ML stays
Python's domain (bridged by interop).

Three levers make this work:

1. **Close the everyday-scripting gaps** — remove the friction that makes
   people reach for Python for day-to-day tasks (see the [simplicity-first
   plan](#simplicity-first-parity-plan) below).
2. **Make Python interop a first-class feature** — "need `yaml`?
   `python_eval_json` it." This kills the lock-in fear and buys time to build
   natives (see [interop safety net](#the-interop-safety-net)).
3. **Lead with differentiators** — zero-install native binary, fast startup,
   native GUI/games/Discord/AI, and (after Parity Phase D) GIL-free threads.
   These are places Indent can *beat* Python, not just match it.

### Simplicity-first parity plan (1.5.0+)

Prioritized so the highest-value, **lowest-complexity** items come first —
all additive builtins, no new syntax:

1. **Process capture** — `os_run(command)` → `{status, stdout, stderr}`
   (fills the `subprocess` gap; simple, single call). ✅ shipped
2. **File ops** — `os_copy`, `os_move`, `os_copy_tree`, `os_remove_tree`,
   `file_size` (fills the `shutil`/`pathlib` gap). ✅ shipped (`os_copy`/`os_move`/`os_copy_tree`/`file_size`; note `os_remove` already removes dirs)
3. **Text/data** — `toml_loads`/`toml_dumps`, `gzip_compress`/
   `gzip_decompress`, `zip_extract` (config + compression). ✅ shipped
   (`toml_loads`/`toml_dumps` + `gzip_compress`/`gzip_decompress` + `zip_list`/`zip_extract`)
4. **CLI + logging** — `args(...)` helper and a simple `log(level, msg)`.
   ✅ `log` shipped; `args` is already available as **`sys_argv`** (returns
   command-line args as a list) — no new builtin needed
5. **Collections** — `counter(...)`, `default_dict(...)`, `deque(...)`
   as functions (no new syntax).
6. **Async** — ✅ **SHIPPED (2026-08-26)**. Task-based `spawn`/`task_wait`/
   `task_done`/`task_result`/`task_wait_all` **+ `parallel` (gather) +
   `task_wait_timeout` (wait_for) + function-value `spawn`** implemented on a
   thread-safe runtime (module storage refactored `Rc`→`Arc`/`Mutex`). See
   [Async design](#async-design) below.
7. ~~**Tuples**~~ — ❌ **Dropped** (decision: not needed). Indent's pass-by-value
   semantics + lists already cover multiple-return and fixed grouping without a
   new value type or `(…)` syntax. Complexity avoided.

> Features 1–5 are all *just more builtins* — they keep Indent simple while
> closing the most common scripting gaps. Async (6) is the one genuinely
> architectural item and is treated as a priority; tuples (7) are removed.

### Async design

Goal: give bots, games, HTTP/WS servers, and concurrent scripts real
concurrency **without** the complexity of Python's `async/await` + event loop.

**✅ Implemented (2026-08-26) — task-based concurrency (`spawn`/join).**
Runtime was made thread-safe first (module storage `Rc<ModuleInstance>` →
`Arc<ModuleInstance>`, `Rc<RefCell<module_cache>>` → `Arc<Mutex<...>>`), then
`spawn`/`task_wait`/`task_done`/`task_result`/`task_wait_all` were added.

**Proposed model — task-based concurrency (spawn/join), not async/await:**

```indent
var id = spawn "fetch_page" "https://example.com"   # returns a task id
# ... do other work ...
var result = task_wait id                           # block for the result
```

| Builtin | Behavior |
|---|---|
| `spawn(fn, args...)` | Run `fn(args...)` on a background thread; returns a task id (int). Each task gets its own isolated variable scope (fits Indent's pass-by-value model). |
| `task_wait(id)` | Block until the task completes, return its result. |
| `task_done(id)` | Boolean — is the task finished? (non-blocking) |
| `task_result(id)` | If done, return the result; else `empty` (non-blocking). |
| `task_wait_all(ids)` | Wait for a list of tasks, return their results. |
| `task_cancel(id)` | Best-effort cancellation. |

Why this over async/await:
- **Simpler to learn** — `spawn`/`task_wait` are just functions; no new keywords.
- **Real parallelism** (threads) — actually *beats* Python's GIL-limited asyncio
  for CPU-bound work; a genuine differentiator.
- **Additive** — no parser/interpreter coroutine changes; builtins + a global
  task store, matching the proven builtin pattern.
- Each task runs an **isolated runtime copy** with args passed by value, so
  there are no shared-state races — consistent with Indent's existing model.

### Parity Phases (overview)

```mermaid
gantt
    title Indent → Python-scripting-parity
    dateFormat  YYYY-MM-DD
    section Phase A — Everyday scripting (fast wins)
    Tuples, os.walk, recursive glob, CSV, SQLite     :a1, 2026-09-01, 30d
    Better tracebacks, typed exceptions              :a2, 2026-09-01, 30d
    with-context for more resources, varargs         :a3, 2026-09-01, 30d
    section Phase B — Stdlib depth
    shutil, tempfile, subprocess-full, itertools,
    functools, collections, logging, argparse        :b1, 2026-10-01, 45d
    section Phase C — Advanced language
    Async/await + event loop, generators/yield,
    decorators, class special methods, dataclasses   :c1, 2026-11-15, 60d
    section Phase D — Ecosystem & DX
    Debugger, profiler, air deps/lockfiles, FFI,
    GIL-free threads                                 :d1, 2027-01-15, 60d
```

### Gap dimensions the tactical list below doesn't cover
The "Recommended Implementation Roadmap" later in this doc covers **language &
stdlib** gaps well. Full parity also needs three more dimensions:

#### Ecosystem & Packaging
- `air` has ~50 packages vs PyPI's ~500k — the single biggest adoption blocker.
- Needs: dependency resolution, version ranges, lockfiles, a publishing flow,
  private registries, and a first-party "batteries" distribution so a clean
  install ships the core ~40 modules.
- Strategy: fewer, higher-quality first-party packages + interop for the long
  tail — don't try to out-count PyPI.

#### Tooling & Developer Experience (what makes people stay)
- **Debugger** (breakpoints, step, watch, locals pane) — the biggest DX gap.
- **Profiler** for hot spots.
- Richer IDE: go-to-definition, hover types, autocomplete (VS Code ext +
  tree-sitter grammar already exist).
- Better error messages / tracebacks with frame + locals.

#### Performance & Runtime
- Native Rust runtime already wins on startup and single-binary deploy.
- **GIL-free native threads** would beat CPython's concurrency story.
- **FFI / call C** for speed-critical paths.

### The interop safety net
Indent already ships one-way Python interop (`python_eval`, `python_eval_json`,
`python_exec`, `python_run_file`). Use it as a documented bridge: any library
Indent doesn't ship yet can be reached through Python. This is a strategic
feature — it removes the "what if I get stuck?" objection to switching.

### When each dimension matters
- **Phase A** removes ~90% of the everyday "Python is easier here" friction.
- **Phase B** closes the long tail of common stdlib usage.
- **Phase C** unlocks async frameworks and advanced abstraction.
- **Phase D** makes Indent genuinely competitive on ecosystem and tooling.

> The detailed per-feature breakdown below ("Recommended Implementation
> Roadmap") is the tactical task list behind Phases A–C.

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

#### ~~6. Filesystem Operations~~ ✅ **Implemented**
- Python: `glob.glob("*.txt")`, `os.walk()`, `Path.iterdir()`, `Path.glob()`
- Indent: `glob(...)` **and** recursive `walk(path)` now exist (mirrors `os.walk` / `glob("**/*")`)

#### ~~7. Exception Handling~~ ✅ **Implemented**
- Python: `try: ... except ValueError: ... finally: ...`
- Indent: `do: / catch as e: / lastly:` plus `flag:` — type hierarchy still a gap.

---

### Tier 2: Important / Medium-Priority

#### ~~8. **CSV Support**~~ ✅ **Implemented**
- Python: `csv.DictReader()`, `csv.writer()`
- Indent: `csv_read(path)` / `csv_write(path, rows)` — native, with quoted-field handling
- Impact: Data import/export (very common)

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

### Just shipped (2026-08-26) ✅
- **SQLite** — native `sqlite_exec` / `sqlite_query` / `sqlite_query_one` (bundled SQLite, no system dep).
- **Varargs** — `fun f ...args` collects remaining args into a list.
- **Typed errors** — `error_type(err)` / `error_message(err)` builtins for typed `do/catch` handling.
- **`with` context manager** — `with "f.txt" for read as f:` is now an alias for `open ... as`.
- **Recursive `walk(path)`** and **CSV** (`csv_read`/`csv_write`) — shipped earlier.

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
