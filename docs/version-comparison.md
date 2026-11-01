# Indent 2.2.2 → 2.4.1: measured comparison

Every number here was measured on the same machine, in one sitting, by running the
same program on four official builds. The point is to have the release claims
(2.3.0 "the performance release", 2.4.0 "lighter on memory") checkable rather than
remembered.

> **Update for the 2.4.1 release.** The tables below were measured against
> `2.4.1-exp1`. Everything after that on the exp channel — exp2 through exp21 — is
> summarised at the end of this file, and `compare.md` in the repository root carries
> the current full per-operation table against CPython plus the memory measurements.
> The 2.4.1 release binary is built from the exp21 tree.

**Machine:** 13th Gen Intel Core i5-13400F, 16 threads, 48 GB RAM, kernel
7.2.9-2-cachyos. Linux x86-64.

**Builds measured**

| version | source |
|---|---|
| 2.2.2 | `indent-v2.2.2-x86_64-unknown-linux-gnu.tar.gz` from the GitHub release |
| 2.3.0 | `indent-v2.3.0-x86_64-unknown-linux-gnu.tar.gz` from the GitHub release |
| 2.4.0 | `indent-v2.4.0-x86_64-unknown-linux-gnu.tar.gz` from the GitHub release |
| 2.4.1-exp1 | built from `v2.4.1-exp1` (this repo, `cargo build --release`) |

## Method

- **One workload per process, timed start to finish.** That is what a user pays, and
  it is the only timing that works across releases — the in-language timers changed
  between them.
- **Best of N runs** (minimum), except for runs slower than 5 s, which are measured
  once: their time is set by the algorithm, not by machine noise. A loaded machine
  moves a single fast run by ~15%.
- **Peak RSS is the child's own**, read from `wait4` rusage. `getrusage(RUSAGE_CHILDREN)`
  is a high-water mark across every child ever reaped, so measuring several binaries
  in one process with it returns the same figure for all of them.
- **Each binary runs in place**, inside its own unpacked release tarball, so it finds
  the `std/` that shipped beside it. (Copying the binary out of the tarball makes
  `get ... from json` fail, which silently turns a module workload into an error path
  that every version "passes" identically.)
- **Identical work is verified first.** Every workload prints a checksum, and the
  checksums are compared across all four versions *before* any timing is reported. Two
  workloads were fixed during this exercise because of it: `loops` tripped the
  interpreter's own `LOOP_MAX_ITERS` (100 000) guard on every version, and `imports`
  used `import std json`, which is not the language's import form.
- Workloads and the harness are committed: `benchmarks/version-compare/*.idnt` and
  `benchmarks/version-compare/run.py`. Reproduce with
  `python3 benchmarks/version-compare/run.py`.

## Time (best of 5, whole process)

| workload | what it does | 2.2.2 | 2.3.0 | 2.4.0 | 2.4.1-exp1 | 2.4.0 vs 2.2.2 |
|---|---|---|---|---|---|---|
| `startup` | one `say` | 1.3 ms | 1.2 ms | 1.0 ms | 1.3 ms | 1.3× |
| `imports` | `get ... from` six std modules | 1.9 ms | 1.5 ms | 1.5 ms | 1.4 ms | 1.3× |
| `calls` | 100 000 one-argument calls | 201.7 ms | 52.9 ms | 29.4 ms | 27.5 ms | **6.9×** |
| `loops` | 50 000 integer additions in a `while` | 89.6 ms | 24.1 ms | 6.5 ms | 6.4 ms | **13.8×** |
| `list_ops` | append 50 000 items to a list | 61.59 s | 20.2 ms | 5.2 ms | 5.1 ms | **≈11 800×** |
| `dict_ops` | write then read 20 000 keys | 21.69 s | 31.5 ms | 17.7 ms | 16.4 ms | **≈1 225×** |
| `string_build` | concatenate 50 000 small strings | 2.68 s | 2.37 s | 6.4 ms | 5.3 ms | **≈420×** |
| `string_read` | read a 200 KB string 20 000 times, through a call | 720.0 ms | 549.6 ms | 156.6 ms | 156.2 ms | 4.6× |
| `list_read` | alias a 10 000-item list 5 000 times | 3.24 s | 372.1 ms | 4.1 ms | 4.2 ms | **≈790×** |
| `list_write_alias` | append 20 000 items through an alias of a 10 000-item list | 18.02 s | 12.8 ms | 3.6 ms | 3.5 ms | **≈5 000×** |
| `memory_strings` | hold 200 references to one 200 KB string | 1.05 s | 178.8 ms | 3.1 ms | 2.8 ms | **≈340×** |
| `memory_lists` | one 10 000-item list, aliased 200 times | 11.51 s | 55.0 ms | 2.1 ms | 2.0 ms | **≈5 480×** |
| `memory_objects` | construct 20 000 objects, 3 fields each | **>90 s** (killed) | 38.0 ms | 24.9 ms | 22.2 ms | **>3 600×** |

## Peak memory (per process, KB)

`baseline` is that version's own `startup` figure — the cost of the interpreter
itself. The last column is the memory the workload needed *above* that floor, which is
the only part that says anything about the change.

| workload | 2.2.2 RSS | 2.3.0 RSS | 2.4.0 RSS | 2.4.1-exp1 RSS | 2.4.0 above baseline |
|---|---|---|---|---|---|
| `startup` (baseline) | 9 112 | 9 112 | 9 112 | 9 116 | — |
| `calls` | 9 116 | 9 116 | 9 116 | 9 116 | ~0 |
| `loops` | 9 120 | 9 120 | 9 248 | 9 120 | ~0 |
| `list_ops` | 38 932 | 9 120 | 9 324 | 9 120 | 0.2 MB (was 29.8 MB) |
| `dict_ops` | 22 568 | 9 828 | 9 120 | 9 384 | ~0 (was 13.5 MB) |
| `string_build` | 9 124 | 9 124 | 9 264 | 9 124 | ~0 |
| `string_read` | 9 124 | 9 392 | 9 124 | 9 328 | ~0 |
| `list_read` | 12 600 | 9 128 | 9 268 | 9 128 | 0.2 MB (was 3.5 MB) |
| `list_write_alias` | 34 844 | 9 128 | 9 268 | 9 396 | 0.2 MB (was 25.7 MB) |
| `memory_strings` | 122 560 | 44 976 | 9 220 | 9 132 | **0.1 MB (was 113.4 MB)** |
| `memory_lists` | 755 348 | 115 920 | 9 132 | 9 132 | **0.02 MB (was 746.2 MB)** |
| `memory_objects` | — (killed) | 18 864 | 10 072 | 10 588 | ~0.9 MB (2.3.0: 9.8 MB) |

## What the numbers say

**2.3.0 was the speed release, and its biggest wins are on the paths that copied
per operation.** `calls` 3.8× faster, `loops` 3.7×, `list_read` 8.7×. But three
workloads barely moved — `string_build` (2.68 s → 2.37 s), `string_read`
(720 → 550 ms) and `memory_strings` — because each *read* of a string still copied its
buffer, and building a string in place grew one by re-copying it.

**2.4.0's wins are shaped differently: they are the paths that were superlinear.**
`list_ops` 61.59 s → 5.2 ms, `dict_ops` 21.69 s → 17.7 ms, `list_write_alias`
18.02 s → 3.6 ms, `string_build` 2.68 s → 6.4 ms. Those 2.2.2 figures are not a
constant factor being improved — they are the cost of copying a container on every
read, so their ratios say "the quadratic term is gone", not "the same work got faster".
That is also why `memory_objects` is the extreme case: 2.2.2 does not finish
20 000 object constructions within **90 seconds**, while 2.4.0 does the same work in
24.9 ms and 2.3.0 in 38.0 ms — the per-instance cost this release removed is what made
that loop quadratic.

**The memory story is the clearest result.** Holding 200 aliases of a 10 000-item list
needed 746 MB above baseline on 2.2.2, 107 MB on 2.3.0 and 20 KB on 2.4.0 — because a
read used to be a copy and is now a refcount bump, and it is the *reads* that produce
the aliases.

**2.4.1-exp1 matches 2.4.0 within noise on all thirteen workloads**, which is the
expected result (it carries the Phase 1 cleanups and no runtime semantics change) and
is worth having on the record before further changes land on the exp channel.

**Where 2.4.0 did not win:** `string_read` is still 156 ms — 4.6× better than 2.2.2,
but far from the ~0 of a refcount bump's cost. The remaining time is the *call
overhead* of the 20 000 `measure(big)` calls, not the string: the buffer itself is no
longer copied at all.

## What changed after exp1 (exp2 → exp21)

The exp1 column above is the starting point for the rest of the 2.4.1 work. Everything
below is measured on the same machine against the build named, and each entry names the
comparison it was measured against — none of these is a cross-release table.

**Calls, the one wide gap.** A two-argument call went from 238 ns to 190 ns
(`bench_calls.idnt`, best of seven, interleaved against the previous binary), and against
CPython 3.14 the ratio went 5.69x → 4.22x. What moved it:

| change | effect |
| --- | --- |
| frame locals as a reusable vector, not a `Map<String, Value>` | no `String` per parameter per call |
| slot read-back skipped when the call wrote nothing | the read-back was 71 ns of 168 ns for a read-only body |
| publish only the names a callee can read, not every slot | ~205 ns → ~198 ns on a 12-variable loop |

**Strings.** `a + b` copies bytes instead of going through `format!`
(`str_concat` 2.49x → 2.45x), and `x += y` grows the buffer in place: building a
110 000-character string is **60% faster** than the recorded baseline
(0.53 ms → 0.21 ms), and a list built with `+=` 4000 times is **40% faster**.

**Modules.** A function called through a module's namespace ran in a *copy* of the
module's symbol table, one map insert per exported name per call: 6 280 ns per call with
41 exports, against 301 ns for the same function defined locally — a **95% reduction**,
now within 3% of a local call.

**Objects.** Field reads are copy-on-write (an alias costs a refcount bump), which took
`bench_method_calls.idnt::object equals` from 188.9 ns to 157.5 ns (**−16.5%**), and made
`Value` 24 bytes.

**Memory.** Peak RSS per process, against CPython 3.14: 1M integers in a list
**28.3 MB vs 48.1 MB (0.59x)**, a 100k-key dict **16.0 MB vs 21.9 MB (0.73x)**,
100k objects 18.4 MB vs 18.4 MB, startup 12.1 MB both. Strings are the one loss at
1.09x, because `Value::Str` is an `Arc<String>` with two headers and capacity slack.

**Robustness, which is what the release is really about.** An unbounded loop used to run
until Ctrl-C and deep recursion ended the *process* with `fatal runtime error: stack
overflow`. Both are now ordinary errors: `--max-steps`, `--max-depth` (default 500),
`--max-loop-iters` and `--timeout-ms`, enforced identically by both engines, with
`0` meaning no limit. `scripts/ci/verify-limits.sh` pins twenty checks across both.

**Language fixes** found by using the thing: `if ... then` (both forms, including a
statement on the same line), classes exported from modules, a module function keeping
the state it writes, and `--update` no longer moving an experimental install onto an
older stable build.

## Caveats

- One machine, one sitting. The slow figures are single runs (they are algorithmic);
  the fast ones are minima of five.
- The ~9.1 MB floor means percentage claims about "memory use" are only meaningful
  above it: a workload that adds 20 KB to a 9.1 MB process is not "0.2% of 2.2.2".
- `memory_objects` is not in the time table's ratio column as a divisor: 2.2.2 produced
  no checksum within the cap, and without identical work a ratio would be meaningless.
  Its 2.3.0/2.4.0/exp1 figures (and their RSS) come from a separate single run of the
  same workload with a 30 s cap; the 2.2.2 entry is the *main* run's "killed at 90 s".
- These are interpreter workloads, not I/O or network bound; nothing here says
  anything about the async, HTTP or Discord paths.
