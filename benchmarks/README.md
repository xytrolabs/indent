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
| `bench_append_scaling.ind` | Whether `l is append l x` grows linearly or quadratically |
| `bench_list_building.ind` | Fastest way to build a large list (append vs comprehension vs whole-list concat) |
| `bench_index_writes.ind` | Whether `d[k] is v` / `l[i] is v` grow linearly or quadratically |

Helper scripts:

| Script | Purpose |
|---|---|
| `sh benchmarks/verify.sh` | Performance **and** both test suites, in one go |
| `sh benchmarks/measure.sh [binary] [outfile]` | Startup + hot-path timings |
| `sh benchmarks/run_suite.sh` | Run the `.ind` test suite (avoids shell autocorrect of `test`) |
| `sh benchmarks/build.sh` | Release build, printing only compile errors |

## Reference results

Machine: Linux, `--release` (fat LTO), Indent 2.3.0.

### Per-operation cost

| Case | 2.2.2 | 2.3.0 |
|---|---|---|
| No-op call | 184 ns | **138 ns** |
| 1-argument call | 402 ns | **249 ns** |
| 2-argument call | 700 ns | **436 ns** |
| `i is i + 1` | 202 ns | **159 ns** |
| `if` + comparison | 290 ns | **245 ns** |
| String concatenation | 270 ns | **221 ns** |

### Whole-program

| Case | 2.2.2 | 2.3.0 |
|---|---|---|
| Startup, hello-world (per run) | 1.0 ms | **0.8 ms** |
| Startup, 5 std imports (per run) | 7.0 ms | **5.8 ms** |
| Interpreter hot-path suite (per run) | 377 ms | **126 ms** |
| 50 000 appends | 75.6 s | **0.01 s** |
| 100 000 calls | 0.136 s | **0.049 s** |

Careful: `l is l + [x]` in a loop is **quadratic** (20 000 items ≈ 6.5 s). Build
lists with `append` in a loop, or with a comprehension / `map`.

A full Angela-bot local startup (discord + ai + all puzzle modules, no network)
measures ~48 ms, so interpreter startup is not what makes a bot feel slow —
network round-trips are.
