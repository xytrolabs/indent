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
| `bench_append_scaling.ind` | Whether `l is append l x` grows linearly or quadratically |
| `bench_list_building.ind` | Fastest way to build a large list (append vs comprehension vs whole-list concat) |
| `bench_index_writes.ind` | Whether `d[k] is v` / `l[i] is v` grow linearly or quadratically |
| `bench_startup_empty.ind` / `bench_startup_imports.ind` | Cold process startup, with and without std imports |

Helper scripts:

| Script | Purpose |
|---|---|
| `sh benchmarks/verify.sh` | Performance **and** both test suites, in one go |
| `sh benchmarks/ab_compare.sh` | A/B the current build against a previous binary |
| `sh benchmarks/cache_check.sh` | Prove the `__incache__` cache is used, and measure it |
| `sh benchmarks/cache_flags_check.sh` | `--no-cache`, `--clear-cache`, invalidation on edit |
| `sh benchmarks/measure.sh [binary] [outfile]` | Startup + hot-path timings |
| `sh benchmarks/unit_tests.sh` | Rust unit tests, compact output |
| `sh benchmarks/run_suite.sh` | Run the `.ind` test suite (avoids shell autocorrect of `test`) |
| `sh benchmarks/build.sh` | Release build, printing only compile errors |

## Reference results

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
