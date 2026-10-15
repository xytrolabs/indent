# Indent benchmarks

Micro-benchmarks for the interpreter's hot paths. Run them with a **release**
build — a debug build is 10–50× slower and will look alarming:

```sh
cargo build --release --manifest-path indent-native/Cargo.toml
./indent-native/target/release/indent benchmarks/bench_hot_paths.ind
./indent-native/target/release/indent benchmarks/bench_append_scaling.ind
./indent-native/target/release/indent benchmarks/bench_list_building.ind
```

| Benchmark | What it measures |
|---|---|
| `bench_hot_paths.ind` | Call overhead, string concat, list append, dict writes, JSON round-trips, dynamic dispatch |
| `bench_calls.ind` | Per-operation cost in ns of the individual constructs (call, assign, compare, concat) |
| `bench_deep_calls.ind` | Whether per-call cost scales with the SIZE of the function body |
| `bench_loops.ind` | Iterating: ranges, large int lists, string lists, nested loops |
| `bench_container_reads.ind` | The cost of READING a container (`len`, `has_key`, passing it to a function) |
| `bench_append_scaling.ind` | Whether `l is append l x` grows linearly or quadratically |
| `bench_list_building.ind` | Fastest way to build a large list (append vs comprehension vs whole-list concat) |
| `bench_index_writes.ind` | Whether `d[k] is v` / `l[i] is v` grow linearly or quadratically |
| `bench_index_reads.ind` | Whether `xs[i]` / `d[k]` / `xs[a:b]` on a variable read in O(1) or clone the container (was O(n²)) |
| `bench_startup_empty.ind` / `bench_startup_imports.ind` | Cold process startup, with and without std imports |

Helper scripts:

| Script | Purpose |
|---|---|
| `sh benchmarks/verify.sh` | Performance **and** both test suites, in one go |
| `sh benchmarks/best_of.sh <bench.ind> [n]` | Best of N runs — **use this**, single runs vary ±15% |
| `sh benchmarks/startup_ab.sh [file] [n]` | Interleaved startup A/B vs the installed release |
| `sh benchmarks/ab_current.sh` | A/B the working build vs the installed release |
| `sh benchmarks/ab_compare.sh` | A/B against an older binary (`indent-2.2.2.bak`) |
| `sh benchmarks/size_probe.sh` | Core type sizes + the `Value` size guardrail |
| `sh benchmarks/cache_check.sh` | Prove the `__incache__` cache is used, and measure it |
| `sh benchmarks/cache_flags_check.sh` | `--no-cache`, `--clear-cache`, invalidation on edit |
| `sh benchmarks/cache_write_check.sh` | Prove a run *reuses* the cache instead of rewriting it |
| `sh benchmarks/measure.sh [binary] [outfile]` | Startup + hot-path timings |
| `sh benchmarks/unit_tests.sh` | Rust unit tests, compact output |
| `sh benchmarks/run_suite.sh` | Run the `.ind` test suite (avoids shell autocorrect of `test`) |
| `sh benchmarks/build.sh` | Release build, printing only compile errors |

### Measurement method

- Always use a **release** build; a debug build is 10–50× slower.
- Report the **best of several runs** (`best_of.sh`) — a loaded machine moves a
  single run by ±15%, which is larger than most of the wins below.
- A/B **interleaved**, not sequentially: whichever build runs second inherits any
  drift in machine load. A sequential run showed 2.5.0 as 7% slower on startup
  when it is in fact 4% faster.

## Reference results

### `bench_hot_paths.ind` — the same file, both binaries (best of 3)

The cleanest comparison available: one benchmark file, run against the released
2.2.2 binary and the current build.

| Case | 2.2.2 | **2.6.0** | Gain |
|---|---|---|---|
| 100 000 function calls | 0.1238 s | **0.0365 s** | 3.4× |
| 50 000 `call_func` (dynamic) | 0.0707 s | **0.0184 s** | 3.8× |
| 50 000 `call_func_lenient` | *n/a — added in 2.3* | **0.0206 s** | — |
| 50 000 list `append`s | **52.5 s** | **0.0071 s** | **~7 400×** |
| 50 000 dict writes | 0.0226 s | **0.0087 s** | 2.6× |
| 10 000 string concatenations | 0.00745 s | **0.00378 s** | 2.0× |
| 5 000 JSON round-trips | 0.00757 s | **0.00559 s** | 1.35× |

The append row is the quadratic bug: `l is append l x` copied the whole list on
every step. The old binary also cannot run `call_func_lenient` at all — it did not
exist before 2.3 — which is why that row is empty for 2.2.2.

### 2.2.2 → now, container reads (best of 3)

| Case | 2.2.2 | now | gain |
|---|---|---|---|
| 300 × `len(20k list)` | 59.0 ms | **0.057 ms** | ~1000× |
| 300 × `len(5k dict)` | 44.4 ms | **0.054 ms** | ~820× |
| 300 × `has_key(5k dict, k)` | 44.3 ms | **0.092 ms** | ~480× |
| 300 × pass a 20k list to a reader function | 659 ms | **98 ms** | 6.7× |
| 20 × loop over a 5k string list | 101 ms | **25 ms** | 4.0× |

`len` and `has_key` take a read-only fast path for bare-variable arguments, so
they no longer copy the container. Everything else still clones on read — passing
a container to a user function is the remaining O(n) case (copy-on-write values
would fix it).

### 2.4.0 → 2.5.0 (best of N, interleaved where it matters)

| Case | 2.4.0 | **2.5.0** |
|---|---|---|
| `bench_loops` | 56.5 ms | **44.5 ms** (−21%) |
| `bench_hot_paths` | 122.1 ms | **108.8 ms** (−11%) |
| `bench_calls` | 317.4 ms | **292.5 ms** (−8%) |
| `bench_deep_calls` | 14.3 ms | **13.7 ms** (−4%) |
| Startup, 5 std imports | 3489 µs | **3350 µs** (−4%) |
| `size_of::<Value>()` | 128 B | **56 B** |

What produced it: loops stopped copying their iterable twice; `eval_binary`/
`Index` stopped deep-copying operands to test for object methods; the `Object`
variant's maps were boxed; call frames are recycled instead of reallocated.

### 2.2.2 → 2.4.0

Machine: Linux, `--release` (fat LTO), Indent 2.3.0.

### Per-operation cost

| Case | 2.2.2 | 2.4.0 |
|---|---|---|
| No-op call | 184 ns | **128 ns** |
| 1-argument call | 402 ns | **250 ns** |
| 2-argument call | 700 ns | **395 ns** |
| Call into a 302-statement body | 16355 ns | **359 ns** |
| `i is i + 1` | 202 ns | **174 ns** |
| `if` + comparison | 290 ns | **265 ns** |
| String concatenation | 270 ns | **253 ns** |

Function calls are now **independent of body size** — that row used to be the
biggest hidden cost in real programs.

### Whole-program

| Case | 2.2.2 | 2.4.0 |
|---|---|---|
| Startup, hello-world (per run) | 1.0 ms | **0.8 ms** |
| Startup, 5 std imports (per run) | 7.0 ms | **5.8 ms** → **3.3 ms** with `__incache__` |
| Startup, `discord` import (per run) | 4.88 ms | **2.47 ms** with `__incache__` |
| `.ind` test suite, 51 files | 5.55 s | **3.96 s** (cold), faster warm |
| Interpreter hot-path suite (per run) | 377 ms | **120 ms** |
| 20 000 calls into a 302-statement body | 407 ms | **13 ms** |
| 50 000 appends | 75.6 s | **0.01 s** |
| 100 000 calls | 0.136 s | **0.049 s** |

Careful: `l is l + [x]` in a loop is **quadratic** (20 000 items ≈ 6.5 s). Build
lists with `append` in a loop, or with a comprehension / `map`.

Note: examples that talk to a service that is not running
(`examples/ai_semantic_search.ind` wants a local Ollama) are dominated by
network timeouts, so `ab_compare.sh` reports them as noise.

A full Angela-bot local startup (discord + ai + all puzzle modules, no network)
measures ~48 ms, so interpreter startup is not what makes a bot feel slow —
network round-trips are.
