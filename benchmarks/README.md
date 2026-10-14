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
| `bench_append_scaling.ind` | Whether `l is append l x` grows linearly or quadratically |
| `bench_list_building.ind` | Fastest way to build a large list (append vs comprehension vs whole-list concat) |

## Reference results

Machine: Linux, `--release`, Indent 2.2.2.

| Case | Time |
|---|---|
| 100 000 function calls | ~0.15 s |
| 10 000 string concatenations | ~0.009 s |
| 50 000 dict writes | ~0.025 s |
| 5 000 JSON round-trips | ~0.008 s |
| 50 000 `call_func` | ~0.075 s |
| 50 000 `call_func_lenient` | ~0.086 s |
| 20 000 `l is append l x` | ~0.007 s (was ~9.7 s before the in-place fast path) |
| 20 000 item comprehension | ~0.003 s |
| 20 000 `l is l + [x]` | ~6.5 s — avoid in loops |

Careful: the last row is the "obvious" alternative and it is **quadratic**. Build
lists with `append` in a loop, or with a comprehension / `map`.

A full Angela-bot local startup (discord + ai + all puzzle modules, no network)
measures ~48 ms, so interpreter startup is not what makes a bot feel slow —
network round-trips are.
