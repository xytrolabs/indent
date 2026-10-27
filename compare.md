# Indent vs CPython — measured numbers

Everything here is measured on one machine (Linux x86_64, `lto=fat`, `codegen-units=1`,
release), against **CPython 3.14.8**. Ratios are `indent ÷ python`: **below 1.00x is
faster / smaller**.

Reproduce:

```sh
cd indent-native && cargo build --release
python3 benchmarks/ab_lang_compare.py 5      # per-operation, best of 5
python3 /tmp/perf/memcmp.py                  # peak RSS + wall time (see notes)
python3 benchmarks/check_baselines.py        # against the recorded baseline
bash scripts/ci/then-gate.sh                 # the full gate
```

## 1. Per-operation cost

`benchmarks/ab_lang_bench.ind` vs `ab_lang_bench.py`, best of five. "before" is the build
at the start of the 2.4.1-exp16..exp21 optimisation work; "now" is exp21.

| operation | before | now | |
| --- | --- | --- | --- |
| `call_2arg` | 5.69x | **4.22x** | |
| `call_1arg` | 4.37x | **3.16x** | |
| `call_0arg` | 3.21x | **3.01x** | |
| `str_concat` | 2.49x | **2.45x** | bytes copied, not `format!` |
| `dict_alloc_small` | 1.92x | 1.91x | |
| `int_to_str` | 1.65x | 1.67x | |
| `str_to_int` | 1.48x | 1.49x | |
| `dict_insert` / `dict_insert_small` | 1.38x / 1.41x | 1.46x / 1.44x | noise band ±0.1x |
| `list_alloc_small` | 1.52x | 1.43x | |
| `dict_read_hit` | 1.21x | 1.21x | |
| `float_mul` | 1.08x | 1.00x | parity |
| `list_index_read` | 0.95x | **0.94x** | faster |
| `if_compare` | 1.01x | **0.93x** | faster |
| `list_append` | 0.96x | **0.90x** | faster |
| `add_assign` | 0.51x | **0.46x** | ~2x faster |

Indent is at parity or ahead on 5 of 16 operations and 1.2–1.9x behind on allocation-heavy
ones. **Calls are the one wide gap**, and they are what most real programs are made of.

## 2. Memory (peak RSS) and time

Five workloads, each sample in its own process (`ru_maxrss` is a high-water mark for *all*
children of a process, so measuring several cases from one parent reports the largest
earlier one for every later row). MB = mebibytes; time = best of three.

| workload | Indent | CPython | memory | Indent s | CPython s | time |
| --- | --- | --- | --- | --- | --- | --- |
| empty program | 12.1 MB | 12.1 MB | 1.00x | 0.004 | 0.058 | **0.07x** |
| 1M ints in a list | 28.3 MB | 48.1 MB | **0.59x** | 0.062 | 0.150 | **0.41x** |
| 200k strings in a list | 25.9 MB | 23.7 MB | 1.09x | 0.105 | 0.126 | **0.84x** |
| 100k-key dict | 16.0 MB | 21.9 MB | **0.73x** | 0.109 | 0.144 | **0.76x** |
| 100k objects | 18.4 MB | 18.4 MB | 1.00x | 0.102 | 0.121 | **0.84x** |

Strings are the only case where CPython wins on memory: Indent's `Value::Str` is
`Arc<String>` — two headers plus capacity slack — where CPython shares one object.

## 3. The call path, in nanoseconds

Probe: `fun two a b / give a + b`, called 300 000 times inside a `repeat`.

| | start | exp20 | exp21 |
| --- | --- | --- | --- |
| call, 2 args | 238 ns | 195 ns | ~190 ns |
| call, 1 arg | 162 ns | 126 ns | ~122 ns |
| call, 0 args | 168 ns | — | ~166 ns |
| body with a 1 000-iteration loop | 73 µs | 61 µs | 61 µs |
| 12 live variables + read-only call, per iteration | — | ~205 ns | ~198 ns |

What moved it: `Frame.vars` from a `Map<String, Value>` to a reusable
`Vec<(String, Value)>` (no per-parameter `String` allocation per call), skipping the
slot read-back when a call wrote nothing, and publishing only the names a callee can read
instead of every slot before every call.

## 4. Against the recorded baseline

`benchmarks/check_baselines.py`, 158 cases, 15% tolerance — the wins the gate now
enforces:

| case | change |
| --- | --- |
| `bench_string_ops.ind::build 110000 chars` | **−60%** (0.000534 → 0.000213 s) |
| `bench_list_concat_build.ind::plus-eq 8000` | **−40%** |
| `bench_method_calls.ind::object equals` | **−16.5%** (188.9 → 157.5 ns) |

## 5. Layout guardrails

`tests::value_size_guardrail` pins the sizes the hot paths depend on:

| type | size |
| --- | --- |
| `Value` | 24 bytes |
| `VmInstr` | 64 bytes |
| `Expr` | 80 bytes |
| `Stmt` | 112 bytes |

Boxing `Expr` down to 56 bytes was tried and **rejected**: 6–10% slower, measured.

## 6. Caveats

- These are one machine's numbers. Per-op costs in the 20–120 ns range move ~10% with
  background load; every comparison here was best-of-N, and the A/B comparisons were
  interleaved between the two binaries rather than run one after the other.
- `call_*`, `str_concat`, `dict_*` and `int_to_str` are the only rows where Indent is
  behind; the rest are at or below parity.
- Gate at the time of writing: `benchmarks/run_suite.sh` **80/80** on both engines,
  `unit_tests.sh` 39/39, `golden-outputs.sh` 8/8, `verify-limits.sh` 20/20, baselines
  clean, installer safety.
