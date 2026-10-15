#!/usr/bin/env python3
"""Consolidate the 2.4.0/2.5.0/2.6.0 release notes into a single 2.3.0 release.

The performance work shipped as four tags (2.3.0 -> 2.6.0) while the plan was one
release called 2.3.0. This rewrites the changelog and the README so 2.3.0 is that
single release, and leaves the older history (2.2.2 and earlier) untouched.
"""
import re
import sys

CHANGELOG = "CHANGELOG.md"
README = "README.md"

CHANGELOG_SECTION = """## 2.3.0 — 2026-10-14 (the performance release) 🚀

> Major optimizations and general improvements coming to Indent in 2.3!
> To become a real competitive force, Indent development has focused on improving
> speed in Indent programs through optimizations of Rust code and functions.

### Measured against 2.2.2

Every figure is the same benchmark run against the released 2.2.2 binary and this
build (`benchmarks/`, best-of-N — run `sh benchmarks/verify.sh` yourself).

| Workload | 2.2.2 | **2.3.0** | Gain |
|---|---|---|---|
| Interpreter hot-path suite | 377 ms | **109 ms** | **3.5×** |
| 100 000 function calls | 0.1238 s | **0.0365 s** | 3.4× |
| 50 000 `call_func` (dynamic) | 0.0707 s | **0.0184 s** | 3.8× |
| 50 000 list `append`s | **52.5 s** | **0.0071 s** | **~7 400×** |
| 50 000 dict writes | 0.0226 s | **0.0087 s** | 2.6× |
| 10 000 string concatenations | 0.00745 s | **0.00378 s** | 2.0× |
| 5 000 JSON round-trips | 0.00757 s | **0.00559 s** | 1.35× |
| 300 × `len(20 000-item list)` | 59.0 ms | **0.057 ms** | **~1000×** |
| 300 × `len(5 000-key dict)` | 44.4 ms | **0.054 ms** | **~820×** |
| 300 × `has_key(5 000-key dict, k)` | 44.3 ms | **0.092 ms** | **~480×** |
| Call into a 302-statement function | 16.36 µs | **0.33 µs** | **50×** |
| 1-argument / 2-argument call | 402 / 700 ns | **213 / 338 ns** | 1.9× / 2.1× |
| `i is i + 1` in a loop | 202 ns | **156 ns** | 1.3× |
| Startup — hello-world program | 1.0 ms | **0.8 ms** | 1.2× |
| Startup — 5 std imports | 6.19 ms | **3.35 ms** | 1.8× |
| `.ind` test suite (51 files) | 5.55 s | **3.96 s** cold, ~3.2 s warm | 1.4× |
| `size_of::<Value>()` | 128 B | **56 B** | 2.3× less memory traffic |

`call_func_lenient` has no 2.2.2 figure because it did not exist yet.

### 🧠 Expressions are parsed once, not on every evaluation
Every statement stored its expressions as **text**, and `eval_expr` re-lexed and
re-parsed that text inside *every* loop iteration — a loop was parsing its own
source hundreds of thousands of times a second. Parsing is context-free, so the
AST is now cached (`Runtime.expr_cache`, shared by all modules) with allocation-free
fast paths in front for bare identifiers, integer literals and plain strings.

### 💾 `__incache__/` — Indent's answer to `__pycache__`
The first run of a file writes its parsed program to `__incache__/<name>.inc` beside
the source; later runs deserialise it and skip reading, pre-processing, lexing and
parsing. Validated by source size, mtime (ns) and an FNV-1a hash plus a format
version, so an edit invalidates it immediately; writes are atomic; a corrupt,
stale, missing or unwritable cache simply falls back to parsing, and a cache file
is never executed as source. Startup with the `discord` import: 4.88 → 2.47 ms.
Controls: `--no-cache`, `INDENT_NO_AST_CACHE=1`, `--clear-cache`.

### 📞 Calls no longer scale with the size of a function
`resolve_callable` clones the resolved callable on every call, and a `FunctionDef`
was stored by value — so each call deep-copied the parameters and the **entire body
AST**. A 302-statement function cost **16.36 µs per call even when only two of its
statements ran**, 54× a small function; that tax landed on every sizable function
(Discord handlers, puzzle handlers, AI helpers, parsers). Definitions are now shared
behind `Arc`, so resolving, storing and passing a function is a refcount bump.

### 🧩 Container writes move instead of copying (two quadratic traps removed)
- `l is append l x` (also `push`/`extend`) and `d[k] is v` / `l[i] is v` copied the
  whole container on every step. When the target is a plain variable the container
  is now moved out, mutated and moved back: 50 000 appends went from **52.5 s to
  0.0071 s**, and filling a 20 000-key dict is 15 ms, both scaling linearly.
- Copy semantics are unchanged and pinned by tests: a snapshot taken before the
  write still sees the old value, the value expression may still read the
  container, non-container and undefined targets still raise, containers passed to
  functions are still copies, and slices still copy.

### 🧵 Loops stopped copying their iterable twice
`repeat x in xs` evaluated `xs` (one copy) and then handed it to a helper that
cloned it **again**, so every list loop copied the collection twice per execution;
the loop now consumes the value it already owns.

### 🔢 Operators stopped copying their operands
`eval_binary` probed for object `add`/`equals`/`contains` methods by *passing the
other operand in*, and that argument was cloned eagerly — so every `+`, `==`, `in`
and list index deep-copied **both** operands (a fresh `String`, or a whole list)
just to discover there was no method. A discriminant test replaces those copies.

### ♻️ Reading: `len`/`has_key` are O(1), and call frames are recycled
Arguments are passed by value, so `len(bigList)` cloned the entire list to count
it (127 µs for 20 000 items) and `has_key(d, k)` cloned the dictionary. Both now
have a read-only fast path that borrows the container (`ExecContext::get_var_ref`),
taken only for bare-variable arguments of the right type when the builtin has not
been redefined — so shadowing still wins and the builtin still reports every error.
Call frames are also recycled from a pool, so a call with arguments no longer
allocates a `HashMap` and a `Vec` every time.

### 🧊 `Value` is 56 bytes, not 128
The `Object` variant held two inline `HashMap`s, which made every variable read,
argument and list element move 128 bytes. Boxing them brings `Value` to 56 (2.3×
less memory traffic on large collections). Honest note: this alone barely moved the
micro-benchmarks — the remaining cost is per-operation dispatch and allocation, not
value size — and a `value_size_guardrail` test now fails the build if it grows.

### ⚙️ Release profile
`[profile.release]`: `lto = "fat"`, `codegen-units = 1`, `panic = "abort"` (nothing
catches panics), `strip`, no incremental. The evaluator is one hot loop spread
across many functions, so cross-crate inlining matters. Build time increases — use
`cargo build` for day-to-day work.

### 🧪 Method and tooling
- New `benchmarks/`: hot paths, per-operation costs, deep calls, loops, container
  reads, append/index-write scaling, startup, plus `measure.sh`, `verify.sh`,
  `best_of.sh`, `ab_current.sh`, `startup_ab.sh`, `size_probe.sh`, `cache_check.sh`
  and a README with the reference tables.
- Benchmarks report the **best of N** and A/B **interleaved**: a loaded machine
  moves a single run by ±15%, and a sequential startup comparison once showed this
  build as 7% slower when it is in fact 4% faster.
- New tests: `append_semantics`, `index_write_semantics`, `module_dispatch`,
  `value_size_guardrail`, and six `ast_cache_*` tests covering round-trip,
  same-length-edit invalidation, corruption, a deleted source and the disable switch.

### Still O(n), honestly
Reading a container still copies it: `get_var` clones a whole list or dict, so
passing one to a user function is 6.7× faster than 2.2.2 but the copy remains
(`len`/`has_key` were given targeted borrowed fast paths instead). The general fix
is copy-on-write values — `Value::List(Arc<Vec<Value>>)` with `Arc::make_mut`, which
makes reads O(1) and keeps the in-place write paths O(1). That migration touches
~280 sites; `tools/arc_rewrite.py` is its audited first stage (146 constructions
wrapped, 137 destructuring sites reported) and a full attempt produced **138
compile errors** each needing a clone-vs-move decision, so it is a dedicated pass
with its own verification rather than a rushed change.

**Verification:** 51/51 language tests, 28/28 Rust tests.

"""

README_SECTION = """> ## 🚀 Indent 2.3.0 — the performance release
>
> Major optimizations and general improvements coming to Indent in 2.3! All of it
> is additive, and every figure below is the same benchmark run against the
> released **2.2.2** binary and this build (`sh benchmarks/verify.sh` reproduces it).
>
> | Workload | 2.2.2 | **2.3.0** | Gain |
> |---|---|---|---|
> | Interpreter hot-path suite | 377 ms | **109 ms** | **3.5×** |
> | 100 000 function calls | 0.1238 s | **0.0365 s** | 3.4× |
> | 50 000 list `append`s | **52.5 s** | **0.0071 s** | **~7 400×** |
> | `len()` of a 20 000-item list | 59.0 ms / 300 | **0.057 ms** | **~1000×** |
> | `has_key()` on a 5 000-key dict | 44.3 ms / 300 | **0.092 ms** | **~480×** |
> | Call into a 302-statement function | 16.36 µs | **0.33 µs** | **50×** |
> | 50 000 `call_func` | 0.0707 s | **0.0184 s** | 3.8× |
> | 50 000 dict writes | 0.0226 s | **0.0087 s** | 2.6× |
> | 10 000 string concatenations | 0.00745 s | **0.00378 s** | 2.0× |
> | 1-argument / 2-argument call | 402 / 700 ns | **213 / 338 ns** | 1.9× / 2.1× |
> | `i is i + 1` in a loop | 202 ns | **156 ns** | 1.3× |
> | Startup — 5 std imports | 6.19 ms | **3.35 ms** | 1.8× |
> | `.ind` test suite (51 files) | 5.55 s | **3.96 s** cold | 1.4× |
>
> ### What made it faster
> - 🧠 **Expressions are parsed once, not on every evaluation.** Every statement
>   stored its expressions as *text* and the interpreter re-lexed and re-parsed
>   them inside every loop iteration.
> - 💾 **`__incache__/` — Indent's `__pycache__`.** A file's parsed program is
>   written beside the source and reused, so imports skip lexing and parsing.
>   Edit the file and the cache is invalidated immediately; `--no-cache`,
>   `INDENT_NO_AST_CACHE=1` and `--clear-cache` are available.
> - 📞 **Calls no longer scale with the size of the function.** Resolving a call
>   deep-copied the entire body AST, so a 302-statement function cost 16.4 µs per
>   call even when two of its statements ran.
> - 🧩 **Container writes move instead of copying** (`l is append l x`,
>   `d[k] is v`), turning two quadratic loops into linear ones — 50 000 appends
>   went from 52.5 s to 0.0071 s.
> - 🔢 **Operators stopped copying their operands**, and 🧵 **loops stopped
>   copying their iterable twice**.
> - ♻️ **`len()`/`has_key()` on a container are O(1)** — they used to copy it to
>   measure it — and call frames are recycled instead of reallocated.
> - 🧊 **`Value` is 56 bytes, not 128**, and ⚙️ the release profile is tuned for
>   an interpreter (fat LTO, one codegen unit, no unwind tables, stripped).
>
> ### Still O(n), honestly
> Reading a container still copies it: `get_var` clones a whole list or dict, so
> passing one to a *user* function is 6.7× faster than 2.2.2 but the copy remains.
> `len`/`has_key` were given targeted borrowed fast paths. The general fix is
> copy-on-write values (`Arc<Vec<Value>>` + `Arc::make_mut`), which makes reads
> O(1) and keeps the in-place writes O(1); the plan, its audited first stage
> (`tools/arc_rewrite.py`) and the measured cost of its first attempt are recorded
> in the changelog.
>
> ### 🎉 What's New in 2.2.2"""


def replace_between(path, start_marker, end_marker, new_text, include_end=False):
    text = open(path).read()
    start = text.index(start_marker)
    end = text.index(end_marker)
    if include_end:
        end += len(end_marker)
        return text[:start] + new_text, text
    return text[:start] + new_text + "\n" + text[end:], text


def main():
    dry = "--dry-run" in sys.argv

    # --- CHANGELOG: one 2.3.0 section replaces the 2.6.0/2.5.0/2.4.0/2.3.0 block ---
    new_cl, old = replace_between(
        CHANGELOG,
        "## 2.6.0 — 2026-10-14",
        "## Unreleased — language & package parity",
        CHANGELOG_SECTION,
    )
    removed = old.index("## Unreleased — language & package parity") - old.index("## 2.6.0 — 2026-10-14")
    print(f"changelog: replacing {removed} chars of release notes with one 2.3.0 section")

    # --- README: the 2.6/2.5/2.4/2.3 blocks collapse into one 2.3.0 block ---
    new_r, old = replace_between(
        README,
        "> ## 🚀 Indent 2.6 — the performance work, compared to 2.2.2",
        "> ### 🎉 What's New in 2.2.2",
        README_SECTION + "\n",
    )
    new_r = new_r.replace("# Indent Language (.ind) v2.6.0", "# Indent Language (.ind) v2.3.0", 1)
    print("readme: collapsed the 2.6/2.5/2.4/2.3 blocks into one 2.3.0 block")

    if dry:
        print("(dry run — nothing written)")
        return
    open(CHANGELOG, "w").write(new_cl)
    open(README, "w").write(new_r)
    print("written")


if __name__ == "__main__":
    main()
