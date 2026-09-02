# Known Limitations

Indent aims to be **simple, readable, and beginner-friendly** — but being young
means it has real gaps. This page is an honest list of current limitations, so
you know what to expect (and what not to fight).

## Language

- **Generators / `yield` are not yet implemented.** `yield` is only a stub and
  stops after the first value. Lazy iteration over large/infinite sequences is
  a planned future feature, not something you can rely on today.
- **No decorators, properties, or `dataclass` field validation.** Classes
  support single inheritance, fields, methods, and the natural-name special
  methods (`to_string`, `equals`, `add`, …), but not Python-style decorators or
  computed properties.
- **A few expression quirks** to be aware of:
  - Function calls are **not** allowed inside list literals (e.g. `[f 1 2]`).
    Build lists with `append` from precomputed variables instead.
  - Parenthesized function calls as arguments fail to parse (e.g.
    `append l (f x y)`). Precompute the result into a variable first.
  - String `+` concatenation inside function-call arguments fails to parse.
    Precompute into a variable first. (The `say` keyword accepts it fine.)
  - Chained index + member access (`a.b[0].c`) fails to parse — split into steps.

## Runtime

- **Pass-by-value.** Dicts and lists are cloned when passed to a function, so a
  function that mutates a container must **return** it and the caller must
  reassign (`x is f x ...`). Module-level variables *are* shared across
  functions.
- **Missing dict keys throw** (both `d["k"]` and `d.k`): `Dictionary has no key
  'k'`. Guard with `has_key` before accessing optional keys.
- **Hex literals (`0x3498DB`) are not supported.** Use the `HexColor "RRGGBB"`
  helper for hex colors.
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
