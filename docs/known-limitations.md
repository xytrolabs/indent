# Known Limitations

Indent aims to be **simple, readable, and beginner-friendly** — but being young
means it has real gaps. This page is an honest list of current limitations, so
you know what to expect (and what not to fight).

## Language

- **Generators are finite and eager.** Functions that `yield` work and can be
  iterated with `for` (also `is_generator(gen)` and `to_list(gen)`), but the
  values are collected up-front — there is **no true lazy/resumable execution
  yet**. Infinite sequences and stateful lazy iteration are a planned future
  feature, not something you can rely on today.
- **No decorators, properties, or `dataclass` field validation.** Classes
  support single inheritance, fields, methods, and the natural-name special
  methods (`to_string`, `equals`, `add`, …), but not Python-style decorators or
  computed properties.
- **A few expression quirks** to be aware of:
  - Space-separated calls without parentheses inside list literals should be parenthesized: `[(f 1 2)]`.
  - String `+` concatenation inside space-separated function arguments should be parenthesized: `foo ("a" + b)`.
- **Default parameters must be on a single-line signature.** `fun f a b = 7` works,
  but in the block form (one parameter per line) a continuation line like `b = 7`
  is read as a **body statement**, so the function ends up with one parameter
  fewer and calls fail with *"Too many positional arguments"*. Keep the whole
  signature on one line when you use defaults.
- **Multi-line dict literals are not supported.** Write
  `var d dynamic = {}` then assign keys (`d["k"] is v`), or keep the literal on
  one line.

## Runtime

- **Pass-by-value.** Dicts and lists are cloned when passed to a function, so a
  function that mutates a container must **return** it and the caller must
  reassign (`x is f x ...`). Module-level variables *are* shared across
  functions. This applies to library calls too: `bot is Command bot "ping" …`,
  not a bare `Command bot "ping" …`.
- **Imports are cycle-safe but not cycle-tolerant.** A module that imports
  itself, directly or through another module, raises a catchable
  *"Circular import detected"* `import_error` instead of crashing. Move the
  shared code into a third module to break the cycle.
- **Missing dict keys throw** (both `d["k"]` and `d.k`): `Dictionary has no key
  'k'`. Guard with `has_key` before accessing optional keys.
- **Number base literals**: `0x...` (hex), `0b...` (binary), and `0o...` (octal)
  are supported integers (e.g. `0xFF` = 255, `0b1010` = 10).
- **Safety limits on loops.** `while`/`repeat`/`range` have built-in caps to
  stop runaway infinite loops. Long-running servers should use the async/task
  model rather than a single tight loop.
- **Comments use `#!`**, not `#` — a bare `#` is treated as a hex-color prefix.

## Performance

- **Interpreted.** Indent is a tree-walking interpreter, so it is much slower
  than compiled languages (Rust, Go, C) and slower than JIT languages (Java,
  modern JS) for CPU-bound work. Startup is fast and it is great for scripts,
  bots, CLIs, games, and I/O-bound servers — but it is not for hot loops.
- **Single interpreter thread** for a script (the async/task model runs
  functions on background threads, but there is no automatic multi-core
  parallelization of plain code).
- **Containers are still passed by value.** The runtime speeds up the common
  write *shapes*, but a container handed to a function is still a copy, and the
  optimised shapes only cover direct variable targets:
  - `l is append l x` and `d[k] is v` / `l[i] is v` where the target is a plain
    variable are **linear** (the container is moved, not copied).
  - `l is append l x` inside **a function that took `l` as a parameter**, or
    `obj.items is append obj.items x`, still copies each time — O(n²) over a
    loop. Build such lists with a comprehension (`[f x for x in xs]`) or `map`,
    or collect items and `extend` once at the end.
  - `l is l + [x]` in a loop is **always** O(n²) — measured 6.5 s for 20 000
    items. Never build a list that way; see `benchmarks/`.
- Measured reference numbers (release build, Indent 2.3; run `benchmarks/`
  yourself): a function call ≈ 250 ns for one argument, `i is i + 1` ≈ 160 ns,
  `if` + comparison ≈ 245 ns, 100 000 calls ≈ 0.05 s, 20 000 `append`s ≈
  0.007 s, 20 000 index writes ≈ 0.015 s. Loading the whole `discord` + `ai`
  package stack plus all puzzle modules costs ≈ 48 ms, so **startup is not the
  bottleneck for a bot — network round-trips are**.


## Ecosystem

- **Smaller ecosystem than Python/JS.** `air` has far fewer packages than PyPI
  or npm. There is one-way Python interop (`python_eval`, `python_eval_json`,
  `python_exec`) as a bridge to any library Indent doesn't ship yet.
- **Windows packaging** is still maturing (see `docs/windows.md`).
- **`repeat` is a reserved keyword**, so the itertools repeat builtin is named
  `repeat_item`.

---

This list will shrink over time. If you hit something not covered here, check
the [Quick Reference](quick-reference.md) and the
[Indent Guide](INDENT_GUIDE.md) first.
