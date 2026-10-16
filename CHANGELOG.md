# Indent Changelog

## 2.4.1 — 2026-10-16 (experimental — `exp` channel)

This one is aimed at the **`exp` channel** rather than stable. It closes out the
mechanical phase of the memory plan and lands the measurement scaffolding the plan
asks for *first*, so the phases that follow can be judged on numbers instead of
impressions. It is published as a GitHub *prerelease*, so `releases/latest` still
resolves to 2.4.0 and no stable install is moved onto it by accident.

### The safety net the plan asks for first

Phase 0 of the plan is headed "1–2 weeks, do not skip", because *"you cannot
safely rewrite `Value`, `Frame`, and the dispatch loop without a way to know you
didn't break semantics."* 2.4.0 shipped the representation work without it. It
exists now:

- `scripts/golden/` pins the **text** real programs print: value formatting,
  control flow, the copy-on-write guarantees, and a torture file covering nested
  functions, generators, inheritance dispatch, mutation through aliases, the
  in-place append paths, dict key ordering, unicode and error messages verbatim.
  Every fixture runs through **both engines** (the VM and `INDENT_VM=0`) three
  times and must match byte for byte, so "the compiled path agrees with the
  interpreter" is a check rather than an assumption.
- `benchmarks/baselines.json` records 158 cases; `benchmarks/check_baselines.py`
  compares against them with a 15% tolerance. `benchmarks/verify.sh` claimed to
  "measure performance against the recorded baseline" without ever recording one,
  so a 20% cost was invisible unless someone remembered the old figure.

Worth knowing, because none of it was written down: `say true` prints `TRUE`,
strings inside containers print **unquoted**, dict keys print in **sorted** order,
and `len("héllo 🚀")` counts codepoints, not bytes.

### The mechanical phase, finished

- The named-colour palette is built once per process instead of once per runtime.
  Neutral in the suite's timing (5250 ms before, 5303 ms after), but it removes
  twenty-two allocations from every `Runtime::new`, and the test runner builds one
  per test file.
- Two items on the plan's list turned out to be **already done** and are now
  recorded rather than left looking open: integer formatting is a hand-rolled
  `itoa` (`write_int`, no `format!`), and builtin dispatch already borrows the
  caller's string, folding a copy only when the case differs — the unconditional
  `to_ascii_lowercase()` the plan flags was removed well before 2.4.0.
- **Holding a `StdoutLock` for the life of an `ExecContext` was measured and
  rejected.** 100 000 `say`s cost 33 ms against 24 ms for the same loop with a
  single `say`, so a print is ~90 ns and nearly all of it is the write itself.
  Holding the lock would serialise other threads' output for as long as a context
  runs — in a server, one request that prints would block every other thread's
  `say` — to save a fraction of 90 ns. The plan calls this "no semantic change";
  for a multi-threaded program that is not true.

### New benchmarks, with recorded baselines

- `bench_dict_heavy` — JSON parse → navigate → serialise, plus large-dict insert
  and lookup.
- `bench_string_ops` — build by concatenation, then split, join, replace and case
  folding, each stage timed separately because they stress different paths.
- `bench_concurrency` — 100 spawned tasks gathered, the same work through
  `parallel`, and the sequential spawn-and-wait path.

### Still open, and next

Phase 4 (symbol interning and `Rc`, a one-way door on the AST and the cache
format), the rest of Phase 6 (specialisation, inline caches, constant pool) and
Phase 7 (NaN-boxing). A real heap profile is also still missing: this machine has
no `valgrind`, `heaptrack`, `perf` or `hyperfine`, so `benchmarks/rss.sh` (peak RSS
via `getrusage`) stands in for it — a substitute, not an equivalent.

---

## 2.4.0 — 2026-10-16 (lighter on memory, and two release channels)

### Containers share their buffers

Every value that held a collection owned it, so *reading* a variable deep-copied it.
Collections are now copy-on-write throughout — strings, lists, dictionaries and
groups. A read is a refcount bump; the first write after a second name exists makes
that writer a private copy. That is the same pass-by-value behaviour the language
always had, deferred to the write instead of paid on every read.

| Workload | before | **2.4.0** |
|---|---|---|
| Peak RSS holding 200 copies of one 200 KB string | 84.7 MB | **12.6 MB** |
| Passing a 201 KB string to a function | 130 µs | **7.7 µs** |
| `len()` of a 20 000-item list | 0.0767 s | **0.0000424 s** |
| `has_key()` on a 5 000-key dict | 0.0711 s | **0.0000659 s** |
| Startup importing five `std` modules | 3.51 ms | **2.49 ms** |
| Peak RSS, 20 000 statements of three call arguments each | 30.1 MB | **19.8 MB** |

`Value` is 32 bytes (was 56) and every statement is 112 (was 152); guardrail tests
fail the build if either grows back. The costs are documented rather than hidden:
code that *synthesises* many small strings pays one allocation for the shared header
(`s[3]` ≈ +20%, `s is s + "x"` ≈ +7 ns per step, still linear).

### Release channels: stable and experimental

Two channels now come out of the same pipeline and the same gates.

- **stable** is the default, and is what `releases/latest` resolves to.
- **exp** follows tagged prereleases such as `v2.4.1-exp.1`, which are published as
  GitHub *prereleases* — so an existing stable install is never moved onto an
  experimental build by accident.

```sh
# Install a channel (stable is the default)
curl -fsSL https://indent.xytro.site/install.sh | bash -s -- --channel exp

# Switch an existing install, and back again
indent --switch-to-exp
indent --switch-to-stable
indent --channel          # which channel am I on?
```

`indent --version` reports the channel (`indent 2.4.0 (stable)`), the choice is
recorded beside the binary so `indent --update` keeps following it, and both the
installer and the switcher verify the release's `SHA256SUMS.txt` before installing
anything. The binary being replaced is kept as `<path>.bak`.

Cut an experimental build by pushing an `-exp` tag; the workflow marks it a
prerelease and skips the Homebrew formula, which belongs to stable:

```sh
git tag v2.4.1-exp.1 && git push origin v2.4.1-exp.1
```

The channel lives beside the binary (`$INDENT_HOME/channel`, `~/.local/share/indent`
by default). An install with no such file — anything installed before 2.4.0 — reads
as **stable**, and the first `--update` or `--switch-to-*` writes it. Because a
channel belongs to an `INDENT_HOME`, the channel *inside an active nest* is the
nest's own, not the global install's.

### Copy-on-write, precisely

Sharing never leaks between names — `append` still returns a new value, exactly as
in 2.3:

```sh
var a = [1, 2]
var b = a
b is append b 3
say len(a)          # 2 — a is untouched
```

The only difference is *when* the copy happens: on the first write after a value
becomes shared, rather than on every read.

### Breaking changes

- **`indent --version` now prints the channel**: `indent 2.4.0 (stable)`. Anything
  parsing the version out of that line must take the first word instead of the
  last. Nothing else in the language or the stdlib changed incompatibly.

### Upgrading

- The on-disk AST cache changed format (`AST_CACHE_FORMAT` 3 → 4), so the first
  run after upgrading re-parses and rewrites each cached program once. Later runs
  are as fast as before, and there is nothing to clean up by hand.
- Checksums are a hard failure when a release publishes `SHA256SUMS.txt`. The
  oldest releases (2.1.0 and earlier) have none, so upgrading from one warns and
  installs anyway; everything from 2.2.2 on publishes the file.

---

## 2.3.0 — 2026-10-14 (the performance release) 🚀

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
| Summing a 20 000-item list with `xs[i]` | 2.497 s | **0.00545 s** | **~458×** |
| Walking a 20 000-key dict with `d[k]` | 9.684 s | **0.00897 s** | **~1080×** |
| 2 000 × slicing a 20 000-item list | 0.2468 s | **0.000697 s** | **354×** |
| Call into a 302-statement function | 16.36 µs | **0.33 µs** | **50×** |
| 1-argument / 2-argument call | 402 / 700 ns | **213 / 338 ns** | 1.9× / 2.1× |
| `i is i + 1` in a loop | 202 ns | **156 ns** | 1.3× |
| Startup — hello-world program | 1.0 ms | **0.8 ms** | 1.2× |
| Startup — 5 std imports | 6.19 ms | **3.35 ms** | 1.8× |
| `.ind` test suite (52 files) | 5.55 s | **3.96 s** cold, ~3.2 s warm | 1.4× |
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

The cache key also includes **the interpreter build that wrote it**. `bincode` is
not self-describing, so an AST layout change can deserialise *successfully into a
wrong program* rather than failing — a cache written by one build must never be read
by another, and that now cannot happen even if somebody forgets to bump the format
version when releasing. Upgrading Indent costs one re-parse per source, exactly like
a first run. The alternative — keying on the binary's own mtime/size — was rejected
because it would throw the cache away on every rebuild of the same version, which
would silently change the cost of every local benchmark run.
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

### ♻️ Reading: `len`, `has_key`, `xs[i]` and slices borrow the container
Arguments are passed by value, so `len(bigList)` cloned the entire list to count
it (127 µs for 20 000 items) and `has_key(d, k)` cloned the dictionary. Both now
have a read-only fast path that borrows the container (`ExecContext::get_var_ref`),
taken only for bare-variable arguments of the right type when the builtin has not
been redefined — so shadowing still wins and the builtin still reports every error.
Call frames are also recycled from a pool, so a call with arguments no longer
allocates a `HashMap` and a `Vec` every time.

Indexing and slicing had the same shape and were the **worst** of them: reading a
20 000-item list one element at a time cloned the whole list to hand back each
element, so a loop over a list was O(n²) — 2.5 s just to sum 20 000 items, 9.7 s to
walk a 20 000-key dictionary, 0.25 s for 2 000 slices. A bare-variable target now
borrows the container (`resolve_index_ref`) and clones only the element, which makes
them linear: **458×, 1080× and 354×**. Objects and the `argument`/`argument2`
positional aliases deliberately keep the old path, so `get_item` dispatch and alias
resolution are bit-for-bit unchanged — both are asserted in
`tests/index_read_semantics.ind`, which passes identically on this build and on the
pre-fix binary.

### 🧊 `Value` is 56 bytes, not 128
The `Object` variant held two inline `HashMap`s, which made every variable read,
argument and list element move 128 bytes. Boxing them brings `Value` to 56 (2.3×
less memory traffic on large collections). Honest note: this alone barely moved the
micro-benchmarks — the remaining cost is per-operation dispatch and allocation, not
value size — and a `value_size_guardrail` test now fails the build if it grows.

### 📏 Why a value cannot “start at 1 byte”
A value that is 1 byte when it holds a small number and grows with its contents
sounds ideal, but a `Vec<Value>` cannot express it: every element of a list, every
call argument and every frame slot shares one compile-time stride, so there is no
per-value size to grow — the only lever is a *smallest* fixed size for all values.
What is realisable is two tiers (an inline immediate vs. a boxed payload), which is
how the fast runtimes do it: Lua 16 B, LuaJIT 8 B via NaN-boxing, CPython 8 B
(`PyObject*`, everything heap-allocated), V8 8 B (Smi or pointer). Reaching 16 B
here means boxing `Str`, `List`, `Set`, `Dict`, `Func` and `Object` — an extra
allocation and indirection on the *most common* types. The measured 128 → 56 byte
change says size is a memory win, not a speed win, so that trade is not worth taking
on its own; it earns its keep as a by-product of the copy-on-write migration below,
which has to change exactly those variants anyway.

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
- New tests: `append_semantics`, `index_write_semantics`, `index_read_semantics`,
  `module_dispatch`, `value_size_guardrail`, and six `ast_cache_*` tests covering
  round-trip, same-length-edit invalidation, corruption, a deleted source and the
  disable switch.
- Windows publishes the bare `indent.exe` as its own release asset beside the `.zip`
  (identical bytes), so it can be downloaded without unpacking, and it appears in
  `SHA256SUMS.txt` like every other artifact. `docs/windows.md` no longer claims a
  native `aarch64-pc-windows-msvc` build, which was never produced — the published
  Windows target is `x86_64-pc-windows-msvc`.

### Still O(n), honestly
`len`, `has_key`, `xs[i]` and slices are now O(1) reads, but the underlying rule is
unchanged: `get_var` returns a **copy**, so *passing* a container to a user function
is still a full clone (it is 6.7× faster than 2.2.2 only because calls themselves
got cheap). The general fix
is copy-on-write values — `Value::List(Arc<Vec<Value>>)` with `Arc::make_mut`, which
makes reads O(1) and keeps the in-place write paths O(1). That migration touches
~280 sites; `tools/arc_rewrite.py` is its audited first stage (146 constructions
wrapped, 137 destructuring sites reported) and a full attempt produced **138
compile errors** each needing a clone-vs-move decision, so it is a dedicated pass
with its own verification rather than a rushed change.

**Verification:** 52/52 language tests, 28/28 Rust tests.


## Unreleased — language & package parity

### 🌐 Networking, WebSocket, AI and parser fixes
- **`discord.Command` only accepted one argument shape, and which `discord` module
  the loader found decided whether a bot started at all.** The core definition took
  `(bot, name, desc, handlerName)` while the compatibility shim — and every legacy
  call site — pass the 6.0 shape with a `prefix` first (`Command bot prefix name desc
  handler`) or with trailing options, so a 5-argument call blew up with *"Too many
  positional arguments: expected at most 4"*. Because both the full library and a shim
  can be named `discord`, the failure came and went with module search order, which
  made it look like a heisenbug. `Command` now takes `...args` and dispatches on the
  count, supporting all three shapes (the prefix stays ignored, as the shim
  documented), so it behaves identically no matter which module resolves.
- **The "too many positional arguments" error now names the parameters it expected**
  (`… expected at most 4, got 5 (parameters: bot, name, desc, handlerName)`). Without
  that, this bug was unidentifiable from the message alone; with it, the culprit was
  obvious in one run.

Found by a new offline parity suite (`tests/run_net_tests.sh` against
`tools/mock_fixture_server.py`, which serves HTTP, WebSocket and an
OpenAI-compatible mock): **103 assertions** across `net_http_parity`,
`net_ws_parity` and `ai_mock_parity`.

- **gzip/deflate response bodies were not decoded.** `reqwest` was built without
  its compression features, so any API or CDN that serves compressed JSON handed
  back raw gzip bytes that failed to parse — 404-ing `json_parse`, silently, at the
  protocol level. Also `charset`, so a Latin-1 body came back as mojibake. The
  client now advertises and decodes exactly what Python's `requests` does out of the
  box (`gzip`, `deflate`, `charset`), reusing the `flate2` already in the tree.
- **`ws_connect` returned a handle its own siblings rejected.** Every other `ws_*`
  builtin demanded a bare int, so the documented `ws_send_text(sock, "hi")` failed
  with "socket_id must be an int"; only code that happened to know to index
  `["socket_id"]` worked. Both forms are now accepted, and the result-dict contract
  (`{ok, text}` / `{ok:false, timeout:true}` / `{ok:false, closed:true}`) is
  documented in `docs/builtins-reference.md` instead of the wrong `int`/`string`
  return types that were there.
- **The AI package kept reporting a stale error after a successful retry.**
  `_ApiRequest` reset its error state on entry but not between attempts or on
  success, so `WasError()` stayed true for a call that had actually recovered —
  the opposite of the documented "`{}` = success" and of openai-python, where a
  success simply has no error. Now cleared per attempt and on success.
- **One-line `fun` defaults were parsed as a single whitespace token**, so
  `fun NotFound msg = "Not Found"` declared a parameter literally named `Found"`
  and threw *"Missing argument for parameter 'Found\"'"* at every call. That broke
  `web.NotFound`, `web.BadRequest` and `web.ServerError` for every user. A default
  may now be any expression (`= "Not Found"`, `= [1, 2]`) while
  `fun f a = 1 b = 2` still declares two parameters, and a trailing `as type` still
  parses. `AST_CACHE_FORMAT` bumped to 3, because a cached AST from the old parser
  is not merely stale but wrong.

### ✨ Language (all additive — nothing removed)
- **Native `%name%` interpolation now works in every string**, not just inside
  `say`. `var msg = "Hello %name%"` produces a real interpolated string usable
  in assignments, comparisons, `len()`, function arguments, lists/dicts and
  returns. Dotted chains work too (`"User: %user.name%"`). Undefined names and
  stray `%` (`"100% done"`) are left literal, as before.
- **Exponentiation `**`** — `2 ** 10` → `1024`.
- **Repetition with `*`** — `"ha" * 3` → `"hahaha"`, `[1, 2] * 3` → `[1,2,1,2,1,2]`.
- **Chained member & method calls** — `"  hi  ".strip().upper()`, `d.user.name`,
  `items[0].name`.
- **Number base literals** — `0xFF` (hex), `0b1010` (binary), `0o777` (octal).
- **Parenthesized calls as arguments / in collections** —
  `append l (f x y)` and `[(f 1 2), 0xFF]` now parse.
- `interpolate(template[, vars])` builtin for runtime template evaluation.

### ➕ New builtins
- Strings/Unicode: `chr` (`from_char_code`), `ord` (`char_code`),
  `char_at`, `repeat` (`repeat_str`).
- Lists: `shift`, `unshift`, `flat` (alias of `flatten`).
- Dicts: `entries` (alias of `items`).
- JSON: `json_parse` (alias of `json_loads`), `json_stringify` (alias of `json_dumps`).
- Math/number: `math_clamp`, `trunc` (`math_trunc`), `sign` (`math_sign`),
  `is_nan`, `is_finite`.

### 🕐 `time_format` / `time_parse` rewritten
- `time_format` now implements real `strftime` specifiers (`%Y %y %m %d %H %I %M
  %S %p %A %a %B %b %j %w %F %T %s %%`) instead of returning a fixed string, and
  its date math is calendar-accurate (correct for leap years across the full
  timestamp range). `time_parse` round-trips exactly.

### 📦 Standard library
- **`ai`** — added `ChatWithSystem`, `ChatWithOptions`, `ChatDefault`,
  `AskDefault`, `AskJson`, `Summarize`, `Translate`, `Classify`, `Extract`,
  `NewConversation`/`Converse` (stateful chat), an in-memory **vector store**
  (`NewVectorStore`/`VectorStoreAdd`/`VectorStoreSearch`) and `RagAnswer`.
  Errors from the provider are preserved (`GetLastError`).
- **`json`** — `Clone`, `Pretty`, JSON Pointer (`PointerGet`/`Has`/`GetOr`),
  `Merge`, `DeepMerge`, `FromFile`, `ToFile`.
- **`fs`** — `MkdirAll`, `ReadLines`, `WriteLines`, `ReadJson`, `WriteJson`,
  `ReadOr`, `RemoveIfExists`, `CopyIfMissing`, `CopyTree`, `Move`, `Size`, `Walk`.
- **`strings`** — `Lines`, `RemovePrefix`, `RemoveSuffix`, `Partition`, `Chars`,
  `IsBlank`, `Repeat`, `ZFill`, `CountOccurrences`, `Chr`, `Ord`, `CharAt`.
- **`datetime`** — `DurationText`, `Relative`, `NowPlus`, `DaysBetween`,
  `IsoNow`/`DateNow`/`TimeNow`.
- **`web`** — `Created`, `Redirect`, `NotFound`, `BadRequest`, `ServerError`,
  `StaticFile` (MIME-sniffing static file server).
- **`discord`** — shipped in `std/` for out-of-the-box use.

### 🧪 Tests
- 3 new test suites (`modern_parity_features`, `pkg_enhancements`,
  `datetime_pkg`, `datetime_format_builtins`, `ai_pkg_features`); the full suite
  now reports **45/45 passing** (was 41, with 3 pre-existing failures).

### ⚡ HTTP performance & capabilities
- **One shared, pooled HTTP client** for all `http_*` builtins (was: a brand-new
  `reqwest::Client` per request, i.e. a fresh TCP + TLS handshake every call).
  This is the main fix for "sluggish" bots/AI clients.
- Per-request **timeouts** (default 30s) via `http_set_timeout(seconds)` /
  `http_timeout()`, applied to every request.
- Responses now expose **`headers`** (lowercased name → value) alongside
  `ok`/`status`/`body` — enables rate-limit and request-id handling.
- New **`http_request(method, url, [payload], [auth], [headers])`** builtin: any
  method, optional JSON body, auth, and **custom headers**.

### 🤖 `ai` package — OpenAI-SDK-grade error handling (v1.4)
- **Typed, structured errors.** `LastError()` now returns a dict with `type`,
  `status`, `code`, `message`, `param`, `request_id`, `body`, `provider`;
  `GetLastError()`/`GetLastStatus()`/`WasError()` kept for compatibility.
- **Typed checkers**: `IsAuthError`, `IsPermissionError`, `IsBadRequest`,
  `IsNotFound`, `IsRateLimit`, `IsServerError`, `IsConnectionError`, `IsTimeout`,
  `ErrorType()`, plus pure `ErrorTypeFor(status)` / `IsRetryable(status)`.
- **Smart retries** matching the OpenAI SDK: only connection errors, 408/409/429
  and 5xx are retried; `Retry-After` is honoured; exponential backoff + jitter,
  capped. 4xx client errors return immediately (no pointless retries).
- **Request-id capture** (`GetRequestId()` from `x-request-id`) for support.
- **Timeouts** (`SetTimeout`) and `SetMaxRetries`; `ChatFull` (full choice dict),
  `LastUsage()`, `LastFinishReason()`.

### 🎮 `discord` package — real-bot hardening (v6.7)
- **Rate-limit-aware REST**: unified `_RestRequest` retries 429 (honouring
  `retry_after` / `Retry-After`) and 5xx with capped backoff; all verbs return
  the standard response dict instead of dropping failures.
- **Gateway `RESUME`** (op 6) with stored `session_id` + sequence — reconnects
  replay missed events instead of re-`IDENTIFY`-ing (avoids identify limits).
- **Heartbeat ACK watchdog** (op 11): zombie sockets are detected and recycled.
- **Fatal close-code handling** (`4004`, `4010`–`4014`) stops cleanly.
- **Bounded message cache** (250, FIFO) — no more unbounded memory growth.
- **Exponential reconnect backoff** with reset after a healthy session.
- **Quote-aware command parsing** (`_SplitArgs`) — `!say "hello world"  x` works.
- `Delete` now accepts an optional JSON body (audit-log reasons are sent again).
- **Connection errors are retried** like `5xx` (capped backoff);
  `SetRestMaxAttempts(n)` and `SetApiBase(url)` added.

### 🐛 Fixes found while verifying
- **`time_sleep _RestBackoff attempt`** was parsed as a **two-argument** call
  (Indent has no inline nested-call argument form), so the Discord retry path
  would have thrown instead of sleeping. Precomputed the value; covered by a new
  REST regression test that exercises the connection-failure/backoff path.
- **Keyword-named functions were uncallable with space arguments.** `Get`,
  `Ask` and `Set` are listed as keywords case-insensitively, so `Get url token`
  failed to parse — silently breaking ~20 `discord` helpers
  (`Get`, `GetUser`, `GetGuild`, `GetChannel`, …). Expression call detection now
  uses a lowercase-only *structural* keyword set, so capitalised names work as
  calls while lowercase keywords (`get`, `set`, `say`, `if`, …) stay reserved.
  The import parser also only treats `get …`/`Get …` as an import when a module
  identifier follows.

### 🔄 Module system hardening
- **Circular imports now fail cleanly.** `a.ind` importing `b.ind` which imports
  `a.ind` (or a file importing itself) used to recurse until the process died
  with `fatal runtime error: stack overflow`. It now raises a normal, catchable
  `import_error`: *"Circular import detected: 'x.ind' imports itself (directly or
  through another module)."* The in-progress marker is always cleared, even on
  failure, so a later import can still succeed.
- **`launch "sibling.ind"` resolves relative to the launching module's own
  directory** (then through the normal module search path) instead of the process
  working directory — so packages work regardless of where they are invoked from.
- **`get X from mod` also binds the module under its own name**, so `mod.Fn ...`
  stays available next to the imported symbol.
- **Namespace objects work across modules.** A module can expose
  `var API dynamic = {}` / `API["Fn"] is Fn` and callers using `API.Fn arg`
  resolve back to the defining module. Unqualified names that no local/imported
  symbol provides are now looked up in imported modules (deterministic, sorted
  order) before reporting `Undefined function`.

### 🧰 Dispatch helpers
- **`call_func_lenient(name, args…)`** (alias `dispatch`) — like `call_func`, but
  adapts to the callee's declared arity: surplus positional arguments are
  dropped and missing ones are filled with `empty` (parameters that declare a
  default keep it). Lets one dispatcher call handlers written with 1, 2 or 3
  parameters. `call_func` stays strict.

### 🤖 `discord` package — command registration & dispatch
- **New `CommandCtx`** — the simplest command API in the package:
  `CommandCtx bot "greet" "Greet someone" "onGreet"` registers both a prefix and
  a slash command whose handler receives a real `(ctx, args)` pair.
- **`ctx_handler` dispatch path** added to `RunCommand`, checked before the
  event-based `handler_name` path.
- **`AddSlash` merges into an existing entry** instead of replacing it, so a
  command first created with `CommandCtx` keeps its handler.
- **`handler_name` commands whose handler nobody dispatches are now called
  directly** with a ctx (previously they silently did nothing unless the user
  registered a `"command"` event listener).
- **All event/command dispatch goes through `call_func_lenient`**, so handlers
  may be declared with fewer parameters than they are called with.
- **`Ctx` no longer throws on events without `guild_id`** (DM messages) — every
  field is `has_key`-guarded.
- **`CtxReply`/`CtxEphemeral` accept a hand-built ctx** containing only
  `token`/`channel_id` (no `source`) and fall back to sending in the channel.

### 🤖 `ai` package — `AI` namespace object (v1.5)
- `get AI from ai` now exposes the whole public API as a namespace, so the
  long-standing `AI.SetBase url` / `AI.Chat model messages` calling convention
  works again (the namespace is a dict of function references — one
  implementation, two calling styles). `AI.GetLastError` returns the
  human-readable error **string**; `AI.LastError`/`LastErrorText` give the
  structured and text forms.
- `LastErrorText` added (text form of the structured error, `""` when clear).

### ⚡ Performance — the list-building idiom is no longer O(n²)
- **`l is append l x` (also `push`/`extend`) is now an in-place fast path.** The
  documented way to build a list copies the whole list on every call, so a loop
  was quadratic and dominated every real workload — measured **9.7 s for 20 000
  appends, 75.6 s for 50 000**. When the assignment target is exactly the list
  being appended to, the runtime now *moves* the list out of the variable,
  pushes, and moves it back — the same result, but O(1) amortised:
  **20 000 appends ≈ 0.007 s (≈1 300× faster), 50 000 ≈ 0.019 s (≈4 000×
  faster)**, with linear scaling (5k/10k/20k/40k = 0.002/0.004/0.009/0.016 s).
  Copy semantics are unchanged: a snapshot taken before the append still sees the
  old list, an appended expression may still read the target, and non-list
  targets still raise — all covered by `tests/append_semantics.ind`.
- Added `benchmarks/` (`bench_hot_paths`, `bench_append_scaling`,
  `bench_list_building`) with a README and reference numbers, so interpreter
  performance is measurable instead of anecdotal.
- Measured local startup for a full Discord bot (discord + ai packages, all
  puzzle modules, no network): **48 ms** — interpreter boot is not what makes a
  bot feel slow.

### 🐛 Gotcha documented, not silently fixed
- **Default parameters are only parsed on a single-line signature.** In the
  block form (`fun f` + one parameter per line), a continuation line like
  `b = 7` is parsed as a **body statement**, not a parameter — the function ends
  up with one parameter fewer. Use `fun f a b = 7` on one line. Documented in
  `docs/known-limitations.md`; changing it would silently reinterpret existing
  function bodies.

## 2.2.2 — 2026-09-11


### 📦 Installer refreshed everywhere
- The installer bundled inside every release archive is now the current one. It
  installs the full `std/` plus AIR even from an archive that contains only the
  runtime, and it never compiles from source unless you ask for it with
  `--build-from-source` (bash) or `-BuildFromSource` (PowerShell).
- Removed the stale, unreferenced `indent-native/install.sh` duplicate.
  `scripts/install.sh` is the single source of truth and is what the release
  stages into the archives.

### 🛡 Batteries-included guarantee
- Release archives are now verified before publishing to contain the runtime,
  AIR, and at least 10 standard-library files — on both the tarball and the
  Windows zip. A bare-binary archive can no longer be published.

### 🐛 Module functions can now use their own imports
- Fixed a real dispatch bug: calling a function exported by one module from a
  *different* file failed if that function used a module **it** had imported with
  a bare `get`. Example: `init.ind` does `get debug` and calls `debug.error(...)`;
  running `init.Initialize()` from `neo.ind` (which never imports `debug`) died
  with `Unsupported method 'error' for receiver 'debug'`.

  A bare `get mod` binds the module in two places — the module table (used by
  call dispatch) and a `Value::Module` variable (used by member access). A
  `ModuleInstance` only snapshots variables, so when the imported function ran in
  the caller's runtime the module-table entry was gone while the variable stayed,
  and dispatch fell through to builtin method mapping. Module lookup now falls
  back to the variable binding, so transitive chains (A → B → C) resolve at any
  depth, and a module is never treated as an object receiver.
- Genuine typos now produce a proper `undefined_function` error that names the
  module and lists its available functions, instead of a misleading
  "Unsupported method" message.

### 🛠 Installer
- Fixed the source-build fallback calling an undefined `yellow` helper, which
  aborted the installer under `set -e` on any machine that fell back to building.
- The fallback now sanity-checks `rustc` first and prints concrete rustup repair
  commands when the toolchain is broken or mismatched, instead of a cryptic
  cargo error.
- `https://indent.xytro.site/install.sh` now prefers pre-built release binaries;
  it previously always compiled from source.

## 2.2.1 — 2026-09-11

### 🚩 `flag` consistency
- `flag "message"` now works, matching `say`-style keyword statements (no colon
  needed). The `flag: "message"` colon form still works for back-compat, and a
  bare `flag` raises an empty error.

### 🪺 Nests — pin an interpreter version
- `indent nest install [<version>]` (alias: `indent nest use <version>`) downloads
  a specific — including an older — release's prebuilt binary into `.nest/bin/`
  and records it in `.nest/indent-version`. `indent nest version` shows the pin.
  Activating the nest makes `indent` resolve to that pinned version.

### 🛠 Tooling
- **`indent --update` now downloads the prebuilt binary** — fast, and no Rust
  toolchain required (it swaps in the latest release's binary). It only falls
  back to a slow source build if no prebuilt release exists for your platform.
  If you are already on the latest version it says so and does nothing.
- `indent --help` now lists the `nest` subcommands, `--watch`, and `--safe`.
- `indent nest install` and `--update` share one download path (tar.gz on
  Linux/macOS, `.zip` on Windows).

### 📖 Docs
- Corrected the language syntax throughout the docs: comments are `#!` (a bare
  `#` is a hex-color token, not a comment); `or` is the `if`-chain keyword, not a
  boolean operator (`and`/`not` are; `&&`/`||`/`!` are unsupported); `is` is
  assignment only (use `in`, `==`, `is_missing()` for tests); groups are built
  with `group([...])` and have no literal/comprehension syntax; and errors carry
  category words (`error_type`) rather than `E###` codes.
- Large Arch-style expansion of `docs/INDENT_GUIDE.md`, plus updates to the quick
  reference and builtins reference.

### 🧩 Groups
- `group([...])` is the group builder; `set([...])` is a deprecated alias, and
  `set` remains the type-conversion keyword (`set x string`).

## 2.2 — 2026-09-07 (nests & hot reload)

### 🪺 Nests — project-local environments
- `indent nest init` creates a `.nest/` (like a Python venv) with
  `air-packages/`, `bin/`, `lib/`, and activation scripts.
- `source .nest/activate` sets `INDENT_NEST`/`INDENT_HOME`/`INDENT_PATH` so
  `air install <pkg>` installs project-locally and imports resolve from the nest.
- Running `indent <file>` inside a project **auto-detects** a `.nest/` and adds
  its `air-packages/` + `lib/` to the module search path (no manual activate needed).
- **Per-nest interpreter versions** — `indent nest install [<version>]` / `use`
  download a specific (e.g. older) release's prebuilt binary into `.nest/bin/`
  and record it in `.nest/indent-version`; `indent nest version` shows the pin.
  When the nest is active, `indent` on PATH is that pinned version.
- Docs: `docs/env-nests.md`.

### 🔁 Hot reload — `indent --watch <file>`
- Re-parses and re-runs the script whenever it changes on disk (no compile
  step, so near-instant restarts). Edit while a window/server is open.

### 🔒 Safe mode — `indent --safe <file>`
- Default-deny sandbox: only pure/computational builtins are allowed; anything
  touching files/OS/network/process/Python/shell is blocked. Run untrusted
  scripts safely (e.g. in web playgrounds or Discord bots).

## 2.2 (also) — 2026-09-04: web package, AIR Deliveries & installer

### 📦 Web package — `web` (std/web.ind)
- New first-party `web` package layered on `http_serve`: `Html`/`Json`/`Text`/`Send`
  response builders, and `RunCode(code)` which runs an Indent string in a fresh
  `indent` process and returns `{ok, status, stdout, stderr}`. Powers a real
  browser "Run" button.
- Docs: `docs/web-package.md`; demo: `examples/web_run.ind`.

### 📦 AIR "Deliveries" — share a whole project
- `air delivery` subcommands: `init`/`build`/`install`/`publish`/`search`/`list`/`info`.
- A Delivery bundles a whole project into one `<name>-<ver>.dlv` archive
  (`.dlv`, not `.del`). Installing makes its modules importable and pulls deps.
- Docs: `docs/deliveries.md`.

### 🖥️ Installer
- `install.ps1`: robust cross-platform OS/arch detection; graceful message when
  run off-Windows; source-build fallback (with Git/Rust/MSVC dependency bootstrap)
  when no GitHub Release exists yet.

## 2.1 — 2026-09-03 (`is` declares, `=` reassigns)

### 🌐 Dynamic web server — `http_serve(handler, port)`
- `http_serve` runs a **dynamic** web server: each request is parsed into a
  `{method, path, query, headers, body}` dict and passed to your `handler`
  function, whose return value (string or dict with `status`/`body`/
  `content_type`/`headers`) becomes the response. Routing is plain Indent.
- Example: `examples/web_server.ind`.

### ✍️ Assignment syntax — `is` canonical for declarations, `=` for reassignment
- `var x is 42` now declares (previously `var x = 42`). `=` is also accepted
  after `var` (`var x = 42` still works).
- `x = 43` now reassigns (previously `x is 43`). `is` reassignment still works.
- Also: `var n int is 5`, `a[0] = 99`, `name = "Grace"`.
- Fully **additive / non-breaking** — every existing program keeps working.
- Test: `tests/assign_syntax_builtins.ind`.

## 2.0 — 2026-09-01 (Color subsystem — gradients, backgrounds, styles)

### 🎨 Full color subsystem (truecolor)
The `colored(text, color)` builtin grew into a complete color API:
- `fg(text, color)` / `colored(text, color)` — foreground color.
- `bg(text, color)` — background color (`48;2;r;g;b`).
- `style(text, style, ...)` — `bold`, `dim`, `italic`, `underline`, `blink`,
  `reverse`, `strikethrough` (also accepts a list of styles).
- `gradient(text, from, to)` / `bg_gradient(text, from, to)` — per-character
  linear color gradient (foreground or background).
- `multicolor(text, color, ...)` / `bg_multicolor(text, color, ...)` — cycle
  colors per character (also accepts a list of colors).
- `rainbow(text)` / `bg_rainbow(text)` — classic 7-color rainbow.
- `paint(text, fg, bg, style)` — combine foreground, background and styles in
  one call; pass `""`/`[]` to skip any component.
- Extended named palette: `GRAY/GREY`, `SILVER`, `MAROON`, `OLIVE`, `LIME`,
  `TEAL`, `NAVY`, `FUCHSIA`, `BROWN`, `GOLD`, `CORAL`, `SALMON`, `SKY`,
  `INDIGO`, `VIOLET`, `AMBER`, `EMERALD`, `SLATE`, `ZINC` added.
- Test: `tests/colors_builtins.ind`.

### 🧰 Std-lib breadth (itertools / functools / collections)
Added higher-order list builtins (all additive, no new syntax):
- **itertools-style**: `chain`, `flatten`, `chunk`, `product`, `permutations`,
  `combinations`, `accumulate`, `cycle`, `repeat_item`, `takewhile`,
  `dropwhile`.
- **collections-style**: `unique`, `partition`, `group_by`, `max_key`, `min_key`.
- **functools-style**: `reduce(fn, list, initial?)`.
- Higher-order builtins call native builtins by name (matching `map`/`filter`),
  e.g. `reduce "add_int" [1,2,3,4]` → `10`, `max_key ["a","bb"] "len"` → `"bb"`.
- Test: `tests/stdlib_breadth_builtins.ind`.

### 🔢 Math extras
- Constants: `math_pi`, `math_e`, `math_tau`.
- `math_factorial`, `math_gcd` (also accepts a list), `math_lcm`, `math_hypot`,
  `math_log2`, `math_degrees`, `math_radians`.

### 🎲 Random extras
- `random_randint(a, b)` (int in `[a,b]`), `random_uniform(a, b)` (float),
  `random_sample(list, k)` (k distinct elements, no replacement).

### 🧰 Final additive batch (pre-2.0.0)
- `os_which(command)` (shutil.which), `os_run_ok(command)` (bool exit status).
- More itertools: `zip_longest`, `pairwise`, `filterfalse`, `compress`, `starmap`.
- `first(list, n)` / `last(list, n)`.
- Test: `tests/final_builtins.ind`.

### 🧩 Class special methods — natural names
- Plain-English special methods instead of Python dunders: `to_string` (`say`),
  `add` (`+`), `subtract` (`-`), `multiply` (`*`), `divide` (`/`),
  `equals` (`==`/`!=`), `len` (`len()`), `get_item` (`obj[i]`),
  `contains` (`x in obj`).
- `dataclass Name` — like `class` but auto-generates `to_string` + `equals`;
  user methods still allowed.
- Test: `tests/class_special_methods.ind`.

### 🛠 `not <funcCall args>` now parses
- `if not has_key d "k"`, `if not is_positive x`, etc. previously failed to
  parse (space-arg call wrapped in `not`). Fixed by preprocessing a leading
  `not ` before the space-separated-call rewrite. `x not in y` unaffected.
- Test: `tests/not_call_builtins.ind`.

### ⚙️ Generators / `yield`
- A function containing `yield` is a **generator**: calling it returns a
  generator you can iterate with `for x in gen`, materialize with
  `to_list(gen)`, or test with `is_generator(gen)`.
- Supports multiple `yield`s and `yield` inside loops. (Finite/eager for now;
  lazy/infinite generators are a documented follow-on.)
- `is_generator` added to the type-check helpers.
- Test: `tests/generators_builtins.ind`.


### 🗂 `builtins()` returns an organized dict
- `builtins()` now returns `{category: [names]}` grouped by category instead of a
  flat list — browse string, math, os/file, async, … separately.

### ✅ Type-check helpers
- `is_list`, `is_dict`, `is_string`, `is_number`, `is_int`, `is_float`, `is_bool`,
  `is_group` — quick boolean tests for a value's type.
- Test: `tests/type_checks_builtins.ind`.

### 🐞 `debug` package
- New std module `debug` with colored logging: `warn(msg)` (yellow), `error(msg)`
  (red), `success(msg)` (green), `info(msg)` (cyan) — built on the `colored`
  builtin. `get warn from debug`.
- Test: `tests/debug_builtins.ind`.

### ✨ Nicer error output
- Errors now render once with a colored header showing the human-readable type
  (e.g. `type_error`), the message, and a `help:` hint, framed by rules. The
  double-render (runtime nesting) is fixed by formatting once at the top level.

### 🏷 `launch` is canonical; `run_file` deprecated
- `launch "path"` is the canonical way to run another Indent file in the current
  runtime; `run_file(path)` is kept as a deprecated alias.

## 1.6.1 — 2026-08-31 (Colored output + builtin discovery)

### 🎨 Colored text output
- **`colored(text, color)`** — wrap text in ANSI truecolor escape codes so
  `say colored "hi" color` prints in color. Accepts hex (`#RGB`, `#RRGGBB`,
  `#RRGGBBAA`, with or without `#`) or a named color (`RED`, `green`, `blue`, …)
  or a color variable (`var c color = "#22c55e"`).
- Test: `tests/colored_builtins.ind`.

### 🔍 Builtin discovery
- **`builtins()`** — returns the names of every available builtin (like Python's
  `dir(__builtins__)`).
- **`get <builtin>`** (with optional `as alias`) — bind a builtin as a value so
  it can be passed around / called via a variable, consistent with
  `get X from module`. Backed by a new `Callable::Builtin` variant.
- Tests: `tests/builtins_discovery.ind`, `tests/get_builtin.ind`.

## 1.6.0 — 2026-08-30 (Async I/O + parity builtins)

### 🌐 Async I/O (non-blocking HTTP on background threads)
- **`http_get_async` / `http_post_json_async` / `http_put_json_async` /
  `http_delete_async`** — run any HTTP request on a background thread and
  return a **future id**; await with `wait`, `gather`, or `future_done`/`result`.
  Fire many requests concurrently without blocking the program.
- Backed by a generic `spawn_builtin_future` helper (runs any builtin on a thread).
- Test: `tests/async_io_builtins.ind`.

### 🧮 Collections
- **`set_union(a, b)` / `set_intersection(a, b)` / `set_difference(a, b)`** —
  complete the group/set type. `collections` wrappers: `SetUnion`/`SetIntersection`/
  `SetDifference`/`SetAdd`/`SetRemove`/`SetContains`.
- Test: `tests/set_ops_builtins.ind`.

### 🗂 Config
- **`yaml_loads(text)` / `yaml_dumps(value)`** — native YAML config parsing
  (config parity alongside TOML/JSON).
- Test: `tests/yaml_builtins.ind`.

### 📁 Path (pathlib parity)
- **`path_ext` / `path_stem` / `path_abs` / `path_expand` / `path_norm`** —
  extension, stem, absolute, `~` expansion, `.`/`..` normalization. `path`
  wrappers: `Ext`/`Stem`/`Abs`/`Expand`/`Norm`.
- Test: `tests/path_helpers_builtins.ind`.

### 🔤 String methods
- **`str_zfill` / `str_ljust` / `str_rjust` / `str_center` / `str_splitlines` /
  `str_removeprefix` / `str_removesuffix` / `str_partition`** — Python `str`
  method parity as additive builtins.
- Test: `tests/string_helpers_builtins.ind`.

## 1.5.2 — 2026-08-30 (Async parity: async def / gather / async with)

### 🐍 Async parity additions
- **`async fun`** — calling an async function returns a future automatically
  (no manual `future "f"`). Auto-schedules on a background thread.
- **`gather(f1, f2, ...)`** or `gather [f1, f2]` — await many futures, results in
  order (`asyncio.gather`).
- **`async with <future> as name:`** — await a future, bind its result, run body.
- **`sleep(secs)`** — async sleep; returns a future (`asyncio.sleep`).
- **`future_wait_for(id, secs)`** — `asyncio.wait_for`.
- Tests: `tests/async_def_builtins.ind`, `tests/async_helpers_builtins.ind`,
  `tests/async_with_builtins.ind`.

## 1.5.1 — 2026-08-30 (Async / await)

### 🐍 Python-style async (`loop` / `await` / `future`)
- New **`loop:`** block, **`await <future>`** statement, and **`future "fn" args...`**
  scheduler — schedule work on background threads, then await results.
- `future_done(id)` / `future_result(id)` / `future_cancel(id)` status helpers.
- `await` exposes its result as `__await_result__`.
- Built on the thread-safe runtime (real OS threads, no GIL) — real concurrency.
- Test: `tests/async_await_builtins.ind`.

## 1.5.0 — 2026-08-29 (Parity + Async runtime)

### ⚡ Async / concurrency (native, no new keywords)
- Task-based concurrency on **real OS threads**: `spawn(fn, args...)`,
  `task_wait(id)`, `task_done(id)`, `task_result(id)`, `task_wait_all(ids)`,
  `parallel(fn, list_of_arglists)` (gather), `task_wait_timeout(id, secs)`
  (wait_for). `spawn` accepts a function value or a name string.
- Runtime made thread-safe: module storage refactored `Rc` → `Arc`/`Mutex`.
- Tests: `tests/async_tasks_builtins.ind`, `tests/async_ergo_builtins.ind`.

### 🗂️ Stdlib parity builtins (native)
- **SQLite**: `sqlite_exec` / `sqlite_query` / `sqlite_query_one` (bundled).
- **CSV**: `csv_read` / `csv_write`; recursive **`walk(path)`**.
- **Process**: `os_run` (capture stdout/stderr), `os_copy`, `os_move`,
  `os_copy_tree`, `file_size`.
- **Data**: `toml_loads` / `toml_dumps`, `gzip_compress` / `gzip_decompress`,
  `zip_list` / `zip_extract`.
- **Typed errors**: `error_type` / `error_message`; logging `log(level, msg)`;
  `counter(list)`.

### 🧩 Language
- Varargs: `fun f ...args`.
- `with ... as ...:` context-manager alias for `open ... as`.

### ✅ Tests
- 12 regression tests pass (`tests/*_builtins.ind`); smoke tests green.

## 1.5.0 — 2026-08-09

### 🎮 InGame — PyGame-style games written entirely in Indent
New native 2D game framework that **mirrors PyGame's API**: all game logic lives in Indent, a native window just draws frames and reports input.
- `std/ingame.ind` — PyGame-mirroring API: `Init()` (init), `SetMode(w,h,title)` (display.set_mode), `DrawRect`/`DrawCircle`/`DrawLine`/`DrawPolygon`/`DrawText` (draw.*), `Flip(clear)` (display.flip), `GetEvents()` (event.get), `GetKeys()` (key.get_pressed), `GetMouse()` (mouse.get_pos), `Tick(fps)` (time.Clock.tick), `Quit()` (quit). Compat aliases kept: `Clear`/`Rect`/`Circle`/`Line`/`Polygon`/`Text`/`Present`/`Events`/`Keys`/`Mouse`
- Events normalized — every event has a `"type"`: `quit` / `keydown` / `keyup` / `mousemove` / `mousedown` / `mouseup` (key events carry `"key"` + `"down"`)
- `indent-native/indent-ingame.c` — native WebKitGTK canvas window; reads JSON frames from `frame.json` (rect/circle/line/polygon/text shapes), emits keyboard **and mouse** events to `events.txt`, tracks held keys in `keys.txt` and cursor in `mouse.txt`
- Framework spawns the window in the background via `os_system("... &")`; Indent's game loop runs independently
- `examples/snake_game.ind` — **100% Indent logic** (bot AI, wall/self collision, growth, scoring, rendering). `INDENT_SNAKE_BOT=1` auto-plays (verified: score 80, snake grows 3→11 segments). Arrow keys play manually.
- `examples/breakout_game.ind` — **NEW**: paddle physics, ball bounce, brick collision, scoring, HUD all in Indent. `INDENT_BREAKOUT_BOT=1` auto-plays (verified: 300 frames, score 50).
- `air install ingame` — updated in the registry to the PyGame-style API (48 packages)

### 🎮 InGame 2.0 — agame merged in + game-dev APIs (RPG/Minecraft-style)
`std/ingame.ind` v2.0 unifies the game packages and adds the APIs needed to build RPGs, tile worlds, and action games entirely in Indent:
- **agame merged into ingame** — `Clamp`/`Lerp`/`Distance`/`Wrap`, `NewEntity`/`Move`/`Collides`, `TileToWorld`/`WorldToTile` now live in ingame. `agame` kept as a compat shim (old `get X from agame` still works).
- **Camera**: `SetCamera`/`GetCamera`/`ScreenX`/`ScreenY` — camera-follow world scrolling
- **Tilemaps**: `MakeTilemap`/`SetTile`/`GetTile`/`IsSolidAt`/`DrawTilemap` (camera-culled, string-keyed legend with color or emoji tiles)
- **Sprites**: `DrawSprite(x,y,w,h,glyph)` — emoji sprites, zero asset files
- **New shapes**: `DrawEllipse`, `DrawArc` (pie slices), `DrawRectRot` (rotated rects)
- **Physics**: `StepPhysics(entity, g)` and `MoveInMap(entity, dx, dy, map, tileSize, legend)` — per-axis AABB tile collision with `hitX`/`hitY` flags
- **Input**: `IsKeyDown(key)`
- `indent-native/indent-ingame.c` renderer extended: ellipse, arc, sprite, rotated rect
- `examples/rpg_demo.ind` — tilemap RPG (procedural world, camera-follow, collision, emoji sprites); `INDENT_RPG_BOT=1` auto-walks
- `tests/rpg_ingame_logic.ind` — headless regression (world gen, collision, bot movement) PASSES
- Registry updated: `ingame` 2.0 + `agame` shim (xytrolabs/air 714949f)
- Docs: `ingame-package.md` rewritten (full v2.0 API), `agame-package.md` merge notice, site synced

### 📚 Docs for all Xytro-maintained packages
Dedicated reference docs for every first-party package (linked from `docs/packages-reference.md` and the README):
- `docs/ai-package.md` — OpenAI-native AI assistant (config, chat/embed/search API, examples)
- `docs/ingame-package.md` — PyGame-style 2D game framework (API, events, compat aliases, skeleton)
- `docs/agame-package.md` — 2D game helper (math, entities, collision, tile math)
- `docs/discord-package.md` — updated to **v6.0**: new production Bot API (`Bot`/`Command`/`Ready`/`Message`/`Start`), ctx helpers, handler signatures, refreshed Quick Reference
- Registry `discord` package synced to v6.0 (was 3.0) — xytrolabs/air 5b4b02e

### 🛡️ `ai` package → v1.2 robust (never crashes on transient errors)
Hardened after a 5000-iteration API loop (`api.sh`) crashed on intermittent server errors (`Dictionary key not found: choices`, `json_loads failed`):
- Safe `_PostJson`/`_GetJson` helpers — check HTTP ok/status, catch connection errors, parse JSON defensively (`do/catch`), never throw
- All calls (`Chat`/`Ask`/`Embed`/`EmbedMany`/`Models`/`Search`) guard indexing with `has_key` and return `empty`/`[]` gracefully on failure
- Automatic retry with backoff — `SetRetries(n)` (default 2)
- Diagnostics — `GetLastError()` / `GetLastStatus()` / `WasError()`
- Fixed `Models()` to use GET `/models` (was POST → 405)
- Verified: success, 404 model error, connection failure, empty body, 20-call stress test, and the real remote (`ai.xytro.site`) all handled without crashing

### 🤖 `ai` package → v1.1 OpenAI-native (works with real OpenAI)
The `ai` package now uses the **native OpenAI API format** (`POST /v1/chat/completions`, `/v1/embeddings`, `GET /v1/models`), which both real OpenAI and a local Ollama (at `/v1`) speak — so the same code hits either:
- `AI.SetBase("https://api.openai.com/v1")` + `AI.SetApiKey("sk-...")` → **real OpenAI**
- default `AI.SetBase("http://localhost:11434/v1")` → **local Ollama** (no key)
- `AI.Chat`/`AI.Ask`/`AI.Embed`/`AI.EmbedMany`/`AI.Models`/`AI.Similarity`/`AI.Search` unchanged
- New `AI.SetApiKey(key)` sends `Authorization: Bearer <key>` (verified via mock server)
- `examples/ai_openai_api.ind` — shows the package API **and** the raw low-level way to call any REST API with `http_post_json`/`http_get`
- Registry updated (xytrolabs/air d73c455)

### 🤖 `ai` package — Python `openai`-style SDK for Indent
New registry package `std/ai.ind` — a clean client for a local Ollama server that mirrors the OpenAI Python SDK surface, all in pure Indent:
- `AI.Chat(model, messages)` — chat completions (→ `client.chat.completions.create`)
- `AI.Ask(model, prompt)` — single-prompt completion (→ `client.completions.create`)
- `AI.Embed(model, text)` / `AI.EmbedMany(model, texts)` — embeddings (→ `client.embeddings.create`)
- `AI.Models()` — list models (→ `client.models.list`)
- `AI.Similarity(a, b)` — cosine similarity; `AI.Search(query, docs)` — semantic ranking
- `AI.SetBase` / `AI.SetDefaultModel` / `AI.SetDefaultEmbedModel` — client config
- Two import styles: `get ai as AI` (dot-call namespace) or `get Chat from ai` (per-function)
- `air install ai` — added to the registry (50 packages). Verified live: chat, generate, single/batch embeddings, model listing, cosine similarity, and semantic ranking.
- `examples/ai_pkg.ind` — demo of the full API

### 🐛 Bug fix: `and` / `or` now short-circuit
`and`/`or` previously evaluated **both** operands eagerly, so guards like `e["type"] == "key" and e["down"] == true` errored on dicts lacking `down`. Now Python-style short-circuit: RHS is only evaluated when needed. Found via the InGame input loop. All 13 tests pass.

---

## 1.4.4 — 2026-08-09

### 🎮 Playable GUI game: Snake
Added a fully playable **Snake** game that renders in a native WebKitGTK window via `gui_show_html`:
- `examples/snake_game.html` — canvas + JS game (movement, food, growth, wall/self collision, score, high score, pause, restart)
- `examples/snake_game.ind` — Indent wrapper that loads the game, asks **Ollama AI** for a one-line tip, injects it, and opens the window
- **Automated tests** (both pass):
  - `node tests/snake_logic_test.js` — 5 headless tests of game logic (movement, eat+score, wall collision, self collision, bot plays 200 frames → 120 pts)
  - `bash tests/gui_snake_test.sh` — opens the real window, verifies it renders, closes it, confirms the script completes cleanly

Verified end-to-end: window opens → AI tip generated → game renders → close → Indent completes.

---

## 1.4.3 — 2026-08-09

### 🔬 Rigorous testing: AI, GUI, and games
Validated Indent end-to-end against a live Ollama server, WebKitGTK GUI, and full game simulations. Found and fixed real bugs:

### 🐛 Bug fixes
- **`sort()` destroyed nested lists** — sorting a list of pairs (e.g. `[[similarity, item], ...]`) flattened every element to a *string*, corrupting data. Now `sort` uses `compare_values`: numbers numerically, strings lexicographically, booleans, and lists element-wise (Python-style). Scored-result sorting (semantic search, leaderboards) now works.
- **`agame.WorldToTile` parse error** — used space-separated calls inside a dict literal. Fixed with temp vars. (Pushed to registry.)

### ✨ New: native GUI helper (`indent-gui`)
- Added `indent-native/indent-gui.c` — a WebKitGTK helper that renders HTML from stdin in a native window. `install.sh` builds it automatically when gcc + gtk3 + webkit2gtk dev headers are present.
- `gui_show_html(html, [title], [w], [h])` now works end-to-end (verified on display `:1`).

### ✅ Verified working (rigorous tests)
- **AI**: chat generation (qwen2.5:0.5b), embeddings (nomic-embed-text, 768-dim), and a **semantic search engine built entirely in Indent** (embed → cosine similarity → sort) that correctly ranks programming docs above non-programming ones.
- **Python interop**: `python_eval`, `python_eval_json` (typed results), `python_exec`, `python_run_file` all working.
- **Games**: full 30-frame game simulation with AI state machine (hunt/retreat), collision, score tracking, and random spawning — final score 40. Plus an **AI-narrated game rendered to the GUI**.
- 13/13 Rust tests pass, zero warnings.

---

## 1.4.2 — 2026-08-08

### 📦 Registry expanded: 7 → 47 packages
AIR now ships **47 packages** (up from 7). New additions include:

- **Core**: `slug`, `textwrap`, `roman`, `lev`, `base`, `diff`, `chunk`, `search`
- **Data structures**: `stack`, `queue`, `linkedlist`, `lru`, `heap`, `counter`
- **Math/Stats**: `stats`, `matrix`, `vector`, `fraction`, `units`
- **Text/Encoding**: `markdown`, `htmltable`, `asciitable`, `xml`, `yaml`, `jsonptr`, `ansi`
- **Files/Config**: `env`, `temp`, `filelock`, `globx`, `mime`
- **System/CLI**: `args`, `logger`, `progress`, `timer`, `retry`, `password`, `semver`
- **Web**: `url`, `cookie`
- Modernized `agame` (legacy syntax → Indent) and `colors` (real hex/RGB impl)

### 🔧 Runtime fixes
- **Expression-level shadowing**: `invoke_callable_expr` now checks user/imported functions before builtins — so module functions calling sibling functions with builtin-colliding names (e.g. `Count` inside `counter`) work correctly. (Matches the statement-level fix from 1.4.1.)
- **AIR package resolution**: the runtime now searches `~/.local/share/indent/air-packages/` by default, so `air install`-ed packages import without setting `INDENT_PATH`.

---

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
