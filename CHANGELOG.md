# Indent Changelog

## 2.4.1-exp14 — 2026-10-23 (experimental — execution budgets)

Fourteenth experimental build of 2.4.1. This is the item every embeddable language in
the Rust ecosystem has and Indent did not: **a ceiling on what a program is allowed to
do.** It comes from reading the field (Rhai's `max_operations` / `max_call_levels`,
Rune's `set_max_call_depth`, Wasm's fuel, Deno's permission model) and noticing which
of those guarantees Indent had no equivalent for.

### Two things had no ceiling at all

Indent is used to run code it did not write — `ai.Chat` output, a Discord handler, a
served HTTP route — and in two cases that code could take the process with it:

```
$ indent spin.ind                     # var i = 0 / repeat 1000000000 / i += 1
^C                                    # ...forever; nothing stopped it

$ indent deep.ind                     # fun down n / give down(n - 1) + 1
thread 'main' has overflowed its stack
fatal runtime error: stack overflow, aborting
timeout: the monitored command dumped core
```

`repeat N` and `repeat` with no count were unbounded. `while`/`until` had a fixed
`LOOP_MAX_ITERS` net that could not be raised. And recursion did not fail *the
program* — it failed *the process*, discarding buffered `say` output and the exit code
along with it. Call depth is now an ordinary Indent error.

### The budgets

| flag | environment | default | charges |
| --- | --- | --- | --- |
| `--max-steps N` | `INDENT_MAX_STEPS` | none | loop iterations, in blocks of 32 |
| `--max-depth N` | `INDENT_MAX_DEPTH` | **500** | frames pushed by calls and methods |
| `--max-loop-iters N` | `INDENT_MAX_LOOP_ITERS` | 100 000 | one loop's iterations |
| `--timeout-ms N` | `INDENT_TIMEOUT_MS` | none | wall clock |

`0` disables any of them, and a value that is not a number is a usage error rather
than a silent "no limit". They are enforced identically by the tree-walker and the
compiled engine — the tree-walker charges in `run_loop_iteration`, which every one of
its loops passes through, and the compiler charges the pass runner and `BumpSlot`,
which is where a compiled `while`/`until` passes once per iteration.

The default depth is 500 because that is where the guard is still *safe*: the example
above returns at 1 000 and aborts at 2 000 on this machine's 8 MiB stack, so the
default sits at half the failure point — a body with more parameters uses more stack
per frame and would otherwise move the crash below the guard.

Errors classify as `resource_error` (a new `E013`) and carry their own advice line.
The display used to keep a second copy of the error classifier next to the one behind
`error_type()`, and the copies drifted as soon as a kind was added: the time budget
came out as `network_error` with "check the URL" advice, because its message names
`--timeout-ms` and the network branch matches on "timeout". Both now call
`classify_error_code`, so there is one classifier to keep correct.

### The cost, and what it took to make it zero

The first working version charged every iteration through a function returning
`Result<(), String>` and read the configured ceiling from an atomic inside the
instruction loop. The baseline check caught it immediately and would not let go:

```
SLOWER   +21.2%  bench_calls.ind::add + assign (us/op)   0.018263 -> 0.022141
SLOWER   +19.7%  bench_int_arith.ind::int + (ns/op)      18.5461 -> 22.1955
```

Bisecting one hypothesis at a time — the `Result`, the `OnceLock` lookup, the extra
field on `CheckLimit` (which did not change `VmInstr`'s 64 bytes), the depth check
itself — each removed a piece and none removed the regression. What finally did:

- **charge in blocks of 32**, tested against the loop counter that is already in a
  register (`reps & 31 == 0`), so the per-iteration cost is a test and a branch and
  the budget is touched once per 32 iterations;
- **sample the depth guard every 8 frames** (`depth & 7 != 0`), so a call pays a
  register compare instead of a load. The guard therefore fires at the next multiple
  of eight frames above the limit — 500 stops at 504;
- **keep the unconfigured path to two loads and a branch** (`over_budget`), with the
  counter, the clock and the message behind a `#[inline(never)]` helper.

Result: the baseline checker is clean again in three consecutive interleaved rounds
against the exp13 binary, and a 3 000 000-iteration loop measures 99 ms with no budget
configured and 98–99 ms with `--max-steps 1000000000 --timeout-ms 60000` set.

`scripts/ci/verify-limits.sh` pins all of it — 19 checks covering every budget, both
engines, the compiled and walked loop shapes, `0` meaning "no limit", and that a
program inside its budgets is untouched. It runs in `scripts/ci/then-gate.sh`.

### CI now runs the language suite

`.github/workflows/ci.yml` ran `cargo test` and the golden fixtures, and never ran the
79 behavioural tests in `tests/*.ind`. It now runs `benchmarks/run_suite.sh` (which
fails if the summary line is missing, so an early `process_exit 0` cannot make the job
pass) and `scripts/ci/verify-limits.sh`.

### Gate

`benchmarks/run_suite.sh` 79/79 (VM and `INDENT_VM=0`), `benchmarks/unit_tests.sh`
39/39, `scripts/ci/golden-outputs.sh` 8/8, installer safety, `verify-limits.sh` 19/19,
and `benchmarks/check_baselines.py` clean.

## 2.4.1-exp13 — 2026-10-17 (experimental — `--update` no longer downgrades an exp install)

Thirteenth experimental build of 2.4.1. One bug, found by looking at a real install
that was on exp11: a binary newer than the tag it had recorded was being treated as a
*stable* install, so `--update` would have replaced it with stable 2.4.0 — throwing away
the experimental fixes it was being tested for.

### The record was stale, and stale was read as "stable"

`channel_for_update` honoured an `exp` channel only when the recorded `installed-tag`
was an experimental tag. That is right for the report it was written for (a stable
install carrying a leftover `exp` file), but it also catches the opposite case: an
experimental build that never wrote a tag, or whose tag is older than the binary —
which is what a hand-downloaded exp build, or an install from before tags were
recorded, looks like. On such an install

```
channel: exp
installed-tag: v2.4.0      # recorded by an earlier stable install
indent --version: 2.4.1    # ...but the binary is an exp build
```

`--update` decided "not from an experimental tag", followed stable, and would have
downloaded 2.4.0 over the exp build.

The running version is now an input to the decision. A stable tag *older* than the
binary cannot describe that binary — nothing installs a newer build and records an
older tag — so in that case the `exp` channel is kept, the tag is refreshed by the
update that follows, and `--update` says so instead of silently following stable. A
record that matches the running version still decides alone, so the original report
(a stable 2.4.0 install with a leftover `exp` file) behaves exactly as before.

### Gate

`benchmarks/run_suite.sh` 79/79 (VM and `INDENT_VM=0`), `benchmarks/unit_tests.sh`
38/38, `scripts/ci/golden-outputs.sh` 8/8, installer safety, and
`benchmarks/check_baselines.py` — no case slower than baseline by more than 15%.

## 2.4.1-exp12 — 2026-10-17 (experimental — `if ... then` parses)

Twelfth experimental build of 2.4.1. It fixes a language-surface bug a user hit straight
away on exp11: **`if` could not be written with `then`**, even though the interpreter's
own error help tells people to do exactly that.

### The help text recommended syntax the parser rejected

Every guard-clause error prints advice like

```
use: if divisor != 0 then ...
```

and no parser accepted a `then`. Both `if` forms — `if cond` and the legacy `if cond:` —
took *everything* after `if ` as the condition, so `if x > 3 then` failed with

```
syntax_error: Unexpected token in expression 'x > 3 then'
```

and `if x > 3 then say "big"` failed the same way. The suggestion was unfollowable,
which made `if` look broken to anyone who took it seriously.

### What parses now

- `if x > 3 then` — the `then` is optional and simply precedes the indented block.
- `if x > 3 then say "big"` — a single statement may follow `then` on the same line.
  It is rewritten into a one-line block and handed to the ordinary statement parser, so
  it is not a second, weaker grammar: every statement form works there, including a
  call, an assignment, `give` (guard clauses), `+=`, and a `repeat`/`while` body.
- `or x > 3 then …`, `else if x > 3 then …`, and `if x > 3 then:` — the same keyword on
  every branch header, in both the colon and the colon-less forms.
- `then` inside a string literal is *not* the keyword, so
  `if s == "we did then it" then …` keeps its condition intact. `split_then` tracks
  quote and escape state while scanning.

The no-`then` forms are untouched, both engines run the new test, and `then` stays a
plain identifier everywhere else — it is only recognised as a header keyword.

### Also in this build

`tests/module_dispatch_builtins.ind` is green again. It was red before exp12 for two
reasons that were both in the test, not the interpreter: it asked for a `Bot` export
that `std/discord.ind` no longer has (renamed `QuickBot`), and it asserted
`len(keys bot.commands) >= 4` for three registrations against a `QuickBot` that starts
with an empty `commands` dict. It now checks that each of `two`, `one` and `named` is
present by name, which is what the module actually promises. With that, the suite
prints a summary again — **79 passed, 0 failed** on both engines.

### Gate

`benchmarks/run_suite.sh` 79/79 (VM and `INDENT_VM=0`), `benchmarks/unit_tests.sh`
38/38, `scripts/ci/golden-outputs.sh` 8/8, installer safety, and
`benchmarks/check_baselines.py` (no case slower than baseline by more than 15%; string
build −61.5% and object calls −16.6% versus baseline).

## 2.4.1-exp11 — 2026-10-17 (experimental — a module function runs in its module's scope)

Eleventh experimental build of 2.4.1. This is the audit's module-scope item, and the
largest measured win of the series.

### Calling into a module cost a copy of the module

`invoke_external_function` gave a module's function the caller's runtime and then made
the module's symbols visible the only way that runtime allowed: by *inserting all of
them* — one hash insert per exported function, callable, and variable — and removing
them again after the call, remembering the previous binding of each so it could be put
back. The cost was proportional to the module, not to the call.

Measured with a module exporting 41 functions, calling one of them 20 000 times: **6 280
ns per call, against 295 ns for the same function defined locally — 21x**, on every call
into `std/`.

A function now runs *in its module's scope*. `ExecContext` carries a stack of the
modules whose top level the running code belongs to, and lookups consult it only after
they have missed the frames and the caller's globals. Nothing is inserted, remembered
or restored, and nested module calls stack naturally: the innermost module is searched
first, which is the order the injection produced.

**6 280 ns → 301 ns per module call (−95%)**, now within 3% of a local call — measured
interleaved against the exp10 release build, best of seven.

Precedence is unchanged, because that is what the injection was for:

- a module's own functions and its own imports shadow the caller's, which is how a
  module calls its siblings;
- an existing global variable still shadows a module variable, which is why
  `tests/discord_helpers.ind`'s cache was reachable from a function and why it caught
  the one behaviour that had to be restored explicitly: a *write* to a module-level
  variable (`_MSG_CACHE[mid] is msg`) must land on the caller's copy, as it did when
  the symbol had been copied in. `take_var` and `swap_var` clone from the module scope
  for exactly that case, and the test passes in both engines again.

A spawned task no longer seeds its runtime with the module's entries either: it runs
the module function through the same scope stack, so the injection in
`spawn_task_builtin` is gone rather than reimplemented.

`benchmarks/bench_module_calls.ind` (with `benchmarks/modcalls_fixture.ind`) records
it: local call, module call, and a module call that reads module state.

### Verification

76 tests pass in both engines, golden outputs are byte-identical (8/8), all 37 unit
tests pass — including the six module tests — the installer safety checks pass, and
`check_baselines.py` reports no case slower than the recorded baseline.

### An experiment this build does not contain, and why

`Expr` is 80 bytes for every expression in every program, and `Comprehension` is 80
bytes on its own — two `String`s and four boxed expressions — while the forms that
dominate real programs (`Int`, `Var`, `Binary`, `Call`) are 24 to 56. Boxing that one
payload takes `Expr` to 56, **30% off every expression node in the AST**, and allocates
only for a comprehension.

It measured *slower*, consistently, so it is not here. Against this build, best of
nine: `bench_hot_paths` "100k function calls" −7.4%, `bench_nested_loops` −6.4 to
−8.1% on all four sections, `bench_int_arith` −6.4 to −9.6% on all six arithmetic
sections. None of those paths reads the AST after compilation, so the change cannot
have altered a single instruction they execute — and reverting it restored parity to
within ±2% on every one of them, which is what makes it the cause rather than noise.

That is a compiler code-layout side effect of an AST edit, and paying 7% of execution
speed for 24 bytes per expression is not a trade worth making quietly. If it is tried
again it should be measured the same way: interleaved, per section, against the build
it replaces.

---

## 2.4.1-exp10 — 2026-10-17 (experimental — a spawn no longer copies the program)

Tenth experimental build of 2.4.1. This is the audit's task-registry item.

### Spawning a task cost a copy of every function, callable, class and async name

`spawn`, `future`, `parallel` and every `*_async` builtin build a new `Runtime` for
the task and handed it the spawner's `funcs`, `callables`, `classes` and
`async_funcs` maps — by cloning them. Four hash tables rebuilt per task, sized by the
*program* rather than by the task.

Measured before touching anything: a spawn+wait cost 26 µs when the program defined
one function, 33 µs at 50, 52 µs at 200 and **138 µs at 800**. That is 0.14 µs per
entry in the table — five times the cost of starting the thread, paid by every spawn in
a task-heavy program, for tables the task mostly never reads. A bot with a few hundred
handlers and a spawn per event pays it per event.

The four maps are now `Arc`s, shared copy-on-write. The hand-over is four refcount
bumps, and a write — a `fun`, `class` or `async fun` statement — goes through
`Arc::make_mut`, so the table is copied once by the first definition that happens
*after* a task exists, instead of once per spawn. A task keeps exactly the snapshot it
was given, which is what the clone gave it before: defining a function after a spawn
still does not reach a task that is already running.

After the change the same measurements are **26, 25, 25 and 25 µs** — the slope is
gone. A 300-spawn loop in `benchmarks/bench_spawn_overhead.ind` (new, and not in the
recorded baselines yet, so it is documentation rather than a gate) reports 29.9 µs per
spawn+wait before and 25.3 µs after.

`tests/registry_sharing.ind` pins the semantics that had to survive: a task uses a
function that existed when it was spawned, a definition made after the spawn still
reaches the spawner, a class defined after a spawn is constructible, a function defined
*inside a method* survives the temporary runtime that methods borrow the tables for,
and an `async fun` still schedules. All six checks pass in both engines.

### Verification

76 tests pass in both engines (75 before), golden outputs are byte-identical (8/8), 37
unit tests pass, the installer safety checks pass, and `check_baselines.py` reports no
case slower than the recorded baseline by more than its tolerance.

---

## 2.4.1-exp9 — 2026-10-17 (experimental — object fields are copy-on-write)

Ninth experimental build of 2.4.1. This is the audit's object item, the last of the
memory changes: `Value::Object`'s field array now behaves like `ListShared` and
`StrShared` already did.

### Reading an object stopped copying it

An instance's fields were a `Box<[Value]>`, so *reading* a variable that holds an
object copied the whole field array — every field, and every `Value` inside it. That
made passing an object to a function, storing one in a list, or reading one in a loop
cost a copy of the instance per use, which is the eager half of pass-by-value that
lists and strings had already deferred to the first write.

`fields` is now a `SharedFields`, an `Arc<Box<[Value]>>` with `Deref`/`DerefMut` that
calls `Arc::make_mut`, so a read is a refcount bump and the copy happens on the first
write — and only when a second handle actually exists. `Arc<Box<[Value]>>` rather than
`Arc<[Value]>` for the same reason `SharedStr` holds an `Arc<String>`: `try_unwrap`
needs a `Sized` payload, and the paths that move the array out must not copy.

Nothing observable changed. `tests/object_aliasing.ind` exists for exactly this
change — it pins that a field write through an alias leaves the original alone, that
a method called on an alias does not write through, that objects read out of a list or
out of another object's field are copies, and that writes through the variable itself
do persist. All fifteen of its checks pass in both engines, as do all 75 tests, the
golden outputs and the 37 unit tests.

### `Value` is 24 bytes now, not 32

The widest payload in `Value` was `Object`, at `Arc` + `Box<[Value]>` = 24 bytes plus
the discriminant. Sharing the field array brings the payload to 16, and the
discriminant fits in the niche the `String` and `Arc` variants already leave unused, so
the whole enum drops from 32 bytes to 24 — a quarter off every variable read, every
argument, every list element and every map entry. `value_size_guardrail` measures it
and now fails above 32 rather than the previous 40, which was set when 32 was the
floor.

That is also the likeliest account of the two measurements below: a 24-byte `Value`
changes the stride of every array of them — the operand stack, the VM's slot vector, a
frame's argument list — so instruction timings in code this build never touched move
by a few percent, in both directions.

Measured against the exp8 release build, interleaved, best of nine per section:

| section | exp8 | exp9 | change |
|---|---|---|---|
| `bench_method_calls` "object equals" | 164 ns | 141 ns | **−14%** |
| `bench_object_alloc` "method add" | 1339 ns | 1170 ns | **−13%** |
| `bench_object_alloc` "create object" | 519 ns | 463 ns | **−11%** |
| `bench_method_calls` "object add" | 941 ns | 834 ns | **−11%** |
| `bench_method_calls` "object to_string" | 291 ns | 280 ns | −4% |

Against the recorded baselines (2026-10-16) the object cases are now 19-26% faster,
the list `+=` shape 44% faster and the string-building loop 61% faster.

### Two measurements that went the wrong way, and what they are

`bench_container_reads` came out 3-7% slower on every section, and
`bench_function_bodies` "body loop" 18% slower — on workloads that never construct an
object. With `INDENT_VM=0` the two binaries are identical on both to within 1%, so the
difference is in the compiled path and not in the change's logic: it is the layout of
`vm_run`, whose dispatch loop is large enough that adding to it moves unrelated
instruction timings by a few percent. exp8 hit the same thing when its new arm was
inline (7-10% across `bench_int_arith`, fixed by moving the cold half out of line); this
build did not touch that loop at all, so there is nothing to move out. It is reported
here rather than left out because a 18% figure in a changelog that only lists wins is
how a project stops trusting its own numbers.

### Two holes in the gate itself, closed

- `benchmarks/run_suite.sh` treated the *absence* of a summary line as success. The
  runner is in-process, so `tests/ai_mock_parity.ind` calling `process_exit 0` when its
  fixture is missing ends the run after one test out of seventy-five, with exit status
  0. The script now starts the fixture itself and fails when the summary never appears.
- `benchmarks/check_baselines.py` had a 15% tolerance and printed "re-measure before
  believing it" without ever re-measuring, so a single unlucky sample failed the gate —
  and the case that failed was different on each run (`+21%` on one, `+30%` on another,
  both recovering to *faster than baseline* on a second look). A case that looks slow
  now gets a second measurement with three times the runs, and only fails if it is
  still slow.

---

## 2.4.1-exp8 — 2026-10-17 (experimental — compound assignment, and two bugs it found)

Eighth experimental build of 2.4.1.

### `x += y` is a compiled statement now

`Stmt::AssignOp` — `i += 1`, `total -= n`, `s += chunk` — was not one of the
statements the VM compiler accepted, so a body containing *one* of them went back to
the tree-walker in its entirety. A `while` loop whose counter is bumped with `+=` got
no compiled form at all, however simple everything else in it was. That is why
`bench_string_ops` never moved: its timed loop is `big is big + unit` plus `i += 1`,
and the second statement was enough to disqualify the whole thing.

It compiles to a new instruction, `AssignOpSlot`, rather than to `LoadSlot` + `Bin` +
`StoreSlot`, for two reasons: the operand order (the source evaluates the value
before it reads the variable, and `Bin` pops the left operand first), and the
in-place growth `+=` already relies on — `s += x` in a loop must not copy the
accumulated string per iteration. Two `Int` operands are answered in the instruction
itself, exactly as `Bin` answers them, so `total -= n` does not pay for a call that
`total is total - n` does not.

Measured against the exp7 release build, interleaved, best of nine per section, using
the benchmark's own timings rather than whole-process wall clock:

| section | exp7 | exp8 | change |
|---|---|---|---|
| `bench_string_ops` "build 110000 chars" | 514.7 µs | 216.5 µs | **−58%** |
| `bench_string_ops` "split 20001 parts" | 1105.8 µs | 1102.9 µs | 0% |
| `bench_int_arith` (all eight sections) | — | — | within ±3% |

The first version of this instruction put all of its logic inline in the dispatch
loop, and that measured a uniform 7-10% regression on every `bench_int_arith`
section — loops whose instruction sequence this build does not change at all, so the
only thing that had moved was the size of the loop the CPU has to keep in cache.
Moving the non-integer half out of line into `vm_assign_slot` brought those sections
back to noise. The lesson is in the code: the arm decides the integer case and calls
out for everything else.

### A compiled `p is p + q` threw the operator method's result away

`ConcatSlot`'s `Object` branch computed `eval_binary("+", lhs, rhs)` and then dropped
the value on the floor with `result?` — the slot kept whatever the hand-over had left
in it, which is `Empty`. So in any compiled body, adding two objects with an `add`
method produced `empty`, while the same statement on the tree-walker produced the
sum. `tests/object_concat_self.ind` reproduces it: `(3,6)` on the tree-walker,
`Cannot access '.x' on empty value` on the VM before this build, `(3,6)` on both
after.

Two things were wrong. The result has to be assigned — `x is x + y` *assigns*, and the
operator's answer wins over anything the method did to the name itself. And the
operand has to be *cloned* rather than moved out of the slot, so that a method which
raises leaves the variable exactly as the tree-walker leaves it; moving it out meant
the write-back at the end of the run stored `Empty` over the old value. The same
check now covers an `Object` as the *right* operand, where `+` reaches `b.add(a)` and
so can run user code with stale frames.

### The suite can no longer pass while stopping early

`tests/ai_mock_parity.ind` calls `process_exit 0` when its fixture server is missing.
The runner is in-process, so that ends the whole run — the summary line is never
printed, one test out of seventy-five has executed, and the exit status is 0. A
truncated run and a clean pass were indistinguishable. `benchmarks/run_suite.sh` now
starts the fixture itself, and treats the summary line as the contract: no summary,
no pass, and an error that names the missing fixture.

### Verification

75 tests pass in both engines (73 before; the two new ones are
`tests/compound_assign.ind`, which covers `+=`, `-=`, `*=`, `/=`, `%=` on ints,
floats, strings, lists and dicts plus the error path's line number, and
`tests/object_concat_self.ind`), golden outputs are byte-identical (8/8), and all 37
unit tests pass.

---

## 2.4.1-exp7 — 2026-10-17 (experimental — operators as enums)

Seventh experimental build of 2.4.1. This is the audit's operator item, and it
turned up a second, larger finding on the way.

### A compiled body no longer re-reads its operators as text

`VmInstr::Bin` and `VmInstr::Unary` carried the operator as a `String` — one heap
allocation per operator per compiled body — and handed it to `eval_binary`, which
opens with a match over `&str` literals. So every `<`, `+` and `==` a compiled loop
evaluated paid a length compare and a `memcmp` for each arm it rejected, to work out
something the *parser* already knew.

The set of operators is fixed, so the name is now resolved to a `BinaryOp` /
`UnaryOp` discriminant at compile time and the run-time decision is a jump table. A
body carrying an operator the enums do not know is simply not compiled, which keeps
the tree-walker the fallback for anything unusual.

The pay-off is a second, narrower step that the enum makes possible: for two `Int`
operands the VM now answers the operator itself, with no call into `eval_binary` at
all. Every guard in that fast path mirrors a guard in `eval_binary` — the `2^52`
bound where integer and `f64` arithmetic still agree, the tighter `2^26` bound for
`*`, "only an exact division is an int" so `7 / 2` stays `3.5`, a zero divisor
falling through so `1 / 0` stays infinity, and shift counts outside the type falling
through. `eval_binary` remains the single source of truth for what an operator
*means*: mixed types, strings and containers, `Object` operator methods and every
error message still go through it.

Measured against the exp6 release build, interleaved, best of seven, on this
machine:

| benchmark | exp6 | exp7 | change |
|---|---|---|---|
| `bench_nested_loops` | 833.7 ms | 727.3 ms | **−12%** |
| `bench_branch_loop` | 18.7 ms | 16.4 ms | **−12%** |
| `bench_int_arith` | 54.2 ms | 50.0 ms | **−7%** |
| `bench_while_loop` | 18.8 ms | 17.6 ms | **−6%** |
| `bench_for_loop` | 80.6 ms | 78.2 ms | −2% |
| `bench_loops` | 15.1 ms | 14.8 ms | −2% |
| `bench_hot_paths` | 74.4 ms | 73.7 ms | −1% |
| `bench_calls` | 170.3 ms | 167.5 ms | −1% |
| `bench_dict_heavy` | 53.2 ms | 52.8 ms | 0% |
| `bench_string_ops` | 5.63 ms | 5.83 ms | +3% (see below) |

`bench_string_ops` measured 3-4% slower in two separate 20-repetition runs, and it is
worth saying why that number is not attributable to this change. Its hot loop
increments with `i += 1`, which the parser turns into `Stmt::AssignOp` — and
`AssignOp` is not one of the statements the VM compiler accepts, so that whole
`while` body runs on the tree-walker in **both** builds. Nothing this build changed
is on its path. As a control, the same binary pair measured with `INDENT_VM=0` came
out 1% *faster* on the new build, i.e. within this machine's per-operation noise
floor. It is layout, not operator dispatch — and the reason the loop is not compiled
is the subject of the next build.

### Overflow of `+` now wraps, like every other integer arm

`eval_binary`'s `(Int, Int)` arm for `+` was the one integer operation written as
`x + y` while its neighbours used `wrapping_add`/`wrapping_sub`. A release build wraps
either way, so this changes nothing that ships — but it is the kind of difference
that makes a fast path disagree with the slow path it is meant to reproduce, and the
new integer fast path is exactly that kind of duplicate.

---

## 2.4.1-exp6 — 2026-10-17 (experimental — colour parsing and gradient rendering)

Sixth experimental build of 2.4.1.

### Colour names no longer allocate to be matched

`color_to_rgb` is called by every `fg`/`bg`/`gradient`/`paint`/`style` argument, and
it used `to_ascii_lowercase()` to compare against the name table — a fresh `String`
per colour, per call — then copied the chosen hex literal into another `String`, and
for a three-digit hex `h[0..1].repeat(2)` allocated once per channel. The name table
is now a `const` of `(name, hex)` pairs searched with `eq_ignore_ascii_case`, the hex
digits are borrowed straight out of the caller's text, and the short forms double a
nibble instead of building a string to parse.

### Gradients render into one buffer

`gradient_text` and `multicolor_text` collected the whole string into a `Vec<char>`
and then built one `String` per character with `format!` before pushing it — so a
100-character gradient allocated 100 strings and a vector, for an answer whose final
size is known up front. They now count characters once, reserve the output buffer,
and write each escape straight into it. `format!`'s output is byte-for-byte
unchanged, including the single-character and empty cases.

No performance number is claimed. There is no benchmark for the colour surface, the
old path was not hot enough to be worth one, and inventing a figure for an allocation
count is how a changelog starts lying.

What is verified is that nothing observable moved: `color_to_rgb` gained three unit
tests covering every accepted form (`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, mixed
case, surrounding whitespace, the full name table) and every rejected one (odd
lengths, non-hex digits, empty), and the renderers gained a test asserting the exact
escape bytes, the palette cycle, the empty and single-character cases, and that a
non-ASCII character is one escape rather than one per byte. The suite passes in both
engines (73/73), golden outputs are byte-identical (8/8), and all 37 unit tests pass.

---

## 2.4.1-exp5 — 2026-10-17 (experimental — VM slot roles)

Fifth experimental build of 2.4.1.

### Which slots are loop counters is decided once, not at every boundary

`vm_pull_slots` runs at every point where the shadow VM leaves its fast path — a call,
a builtin, a loop exit — and it skips slots that hold no name. Loop counters are the
other set it must skip: they are VM-private, so restoring one into the tree-walker
would be wrong. That test was `prog.counters.contains(&(i as u16))`, a linear scan of
a `Vec` run once per *slot* at every boundary.

The counters of a program are fixed when it is compiled, so the membership test is now
precomputed: a `OnceLock<Vec<bool>>` indexed by slot, built on first use, read with one
bounds-checked load. `counters` is still there — the compiled loop instructions carry
their own counter slot, and this only changes how the pull path asks the question.

No measurement is claimed for this one. The scan was over the loop-nesting depth, so
the ceiling on the win is small, and the exp4 release build this should have been
compared against had not finished publishing when the change was ready — comparing
against anything else would have been comparing two different builds. What is verified
is that it changes nothing observable: the full suite passes in both engines (73/73),
golden outputs are byte-identical (8/8), and the unit tests are green (34/34).

### Still open from the audit

Module execution scope, copy-on-write object fields, operator enums and registry
sharing for tasks are all real findings and all unstarted. The first three each change
semantics that the suite currently pins, and the fourth's safe half — sharing the
module parse cache — was already tried and measured at 0.4%, then reverted. None of
them belong in a build that claims to be a tidy-up.

---

## 2.4.1-exp4 — 2026-10-17 (experimental — method-call arguments, object aliasing pinned)

Fourth experimental build of 2.4.1.

### Method calls stop allocating their arguments too

exp3 made the VM's ordinary `Call` instruction hand its operand-stack range to the
callee instead of building a `Vec`. The two *method*-call instructions were left alone
because they also clone the receiver — but they never needed a vector for the
arguments, and `invoke_method_on_value`/`invoke_method_on_place` both already took a
slice. They now pass the stack range directly.

Measured against the exp3 release build, interleaved, best of seven, identical
checksums: **`bench_method_calls` 35 → 33 ms (−5.7%)**, **`bench_calls` 178 → 166 ms
(−6.7%)**, and the method-dispatch probe 166 → 164 ms.

### Object pass-by-value pinned before its fields go copy-on-write

`tests/object_aliasing.ind` records the behaviour a future copy-on-write field change
must preserve: a field write through an alias, a field-writing method called on an
alias, an object read out of a list, a field write through a parameter, writes through
the variable itself (which must persist), an object held in another object's field, and
one object referenced from two containers. It passes today for a trivial reason —
object fields are a `Box<[Value]>`, so cloning an object copies the array — which is
exactly what makes it a baseline rather than a description of the change. It runs in
both engines, because the two take different paths to a field write.

---

## 2.4.1-exp3 — 2026-10-16 (experimental — VM call arguments)

Third experimental build of 2.4.1. A second external audit was verified item by item
against the source: all seven of its findings are real. This build carries the one that
is both contained and measurable, plus the correctness unification that came out of
checking the rest.

### Calls no longer build an argument vector in the VM

`invoke_callable_values` took a `Vec<Value>`, so the VM's `Call` instruction built a
fresh vector out of its own operand stack for **every call** — an allocation and an
argument clone on the hottest path in the interpreter. It takes `&[Value]` now, and the
instruction passes its stack range directly. The two places that genuinely need to own
their arguments — the `spawn` and `async fun` thread boundaries — copy explicitly, so
that cost is paid where a copy is unavoidable instead of by every call in the program.

Measured against the exp2 release build, interleaved, best of nine, identical
checksums: **`bench_calls` 181 → 167 ms (−7.7%)**, and the method-call probe
167 → 164 ms.

### Method dispatch unified across both paths

`invoke_method_on_place` and `invoke_method_on_value` both resolve names through
`ClassMeta::find_method` now. The value path used to look methods up as
`methods.get(method.to_ascii_lowercase())` — an exact hit against a *folded* name,
which can never match a method declared `getValue`. It was not reachable from user
syntax (`make(1).getValue 2` and `list[0].getValue 3` do not parse), so the defect
never showed, but the inconsistency is gone, and `tests/method_case_dispatch.ind` pins
case-insensitive dispatch on every path that is reachable — including inside a
compiled loop body. The place path also stopped building a folded copy it no longer
needs, so it can take the exact map hit.

### Not in this build, and why

- **Module-call injection (#01), copy-on-write object fields (#02), shared task
  registries (#06)** are architectural: each changes name-resolution or value-sharing
  semantics across the whole interpreter, and #02 should be preceded by object-aliasing
  tests that do not exist yet. They belong in their own builds, each with the
  measurement that justifies it — the audit itself says it has not benchmarked them.
- **Operator enums (#04)** span the parser, the instruction set and `eval_binary`. A
  closely related change (a specialised `BinOp`) measured as a net loss earlier in this
  project, so this needs its own measurement rather than an assumption.
- **VM slot flush/pull (#05)**: the cheap half is precomputed slot roles, but `counters`
  holds at most the loop-nesting depth, so the flagged `contains` is a handful of
  integer comparisons per slot. The audit's own advice — instrument before adding
  bookkeeping — is the prerequisite, not the change.
- The smaller #07 targets (ANSI rendering, colour parsing, the task-history queue,
  `VM_STACK`) are cold paths, or need fallback statistics before they can be sized.

---

## 2.4.1-exp2 — 2026-10-16 (experimental — per-call overhead)

Second experimental build of 2.4.1. An external source audit of this tree was checked
line by line first: all six of its findings were real, and its list of things already
implemented was accurate (shared class metadata, slot-indexed fields, `SmallDict`,
copy-on-write values, sized thread stacks, frame shrinking, bounded caches, `Arc`
function bodies, boxed AST variants, direct-slice serialisation, borrowing comment
stripping). The two it ranked highest are in this build.

- **Temporary runtimes no longer allocate caches they throw away.** `Runtime::bare()`
  built three cache `Arc`s *and* a fresh 32-slot expression memo, and every caller
  then handed the runtime the caller's own handles immediately afterwards — so all
  four allocations were discarded before the method body ran, on every call (the memo
  being the expensive one: 32 slots of an enum plus a `String`). It is
  `Runtime::bare_sharing(dir, &caller)` now: the handles come from the caller up
  front and the memo starts empty until it is lent. Measured on 100 000 method calls
  plus 20 000 `to_string` lookups: **148 → 134 ms (−9.5%)**, identical output.
  The memo is indexed by the interpreter, so the accessors use `get`/`get_mut`; a
  missing memo means "no cache", never a panic.
- **Method lookup is a hash hit before it is a scan.** `find_method` takes the class's
  method map for the declared spelling and only falls back to the case-insensitive
  scan when a call is spelled differently, so case-insensitive dispatch is unchanged.
  This runs on every object method call, every special-method lookup (`say obj`, `+`,
  `==`, `string()`, `len()`) and the VM's `PlaceMethodCall` guard, each of which
  walked the whole method table. Worst case — calling the last of thirteen declared
  methods, 50 000 times — is **35 → 33 ms (−5.7%)**; it is neutral on the existing
  method benchmarks, which use small tables.

### Found while measuring, and deliberately not changed

**A zero-argument method called as a dotted name inside an expression is not
dispatched.** `w.plain` on its own line calls the method; `say w.plain` or
`var x = w.plain` resolves it as a *field* read and fails with
`Object has no field 'plain'`, while `w.plain()` and `w.calc 0` work. Both engines
behave identically, so it is one resolution path disagreeing with another rather than
a VM divergence — but fixing it changes language semantics, so it is left for a
decision instead of being slipped into a performance build.

### Measured or reasoned, not yet done

- **Field-map churn per method call** (the audit's #3): `invoke_object_method` inserts
  every field by name into the temporary runtime and reads them back out afterwards,
  so a call costs O(fields) whatever the body touches. The fix is a field-backed scope
  or name interning — both larger than this build.
- **AST-cache validation**: the read path loads the whole source to hash it and holds
  the whole cache payload while deserialising. A streaming hash is small; the rest
  needs large-file startup measurements to justify it.
- **Parser-owned `String` per line**: needs a lifetime change in the parser.
- **VM boundary synchronisation**: `vm_pull_slots` scans a `Vec` of counters once per
  slot. The audit itself says to profile before adding bookkeeping, so it waits for
  that profile.

---

## 2.4.1-exp1 — 2026-10-16 (experimental — `exp` channel)

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

### Packaging note

An `exp` tag cannot be an `.rpm` or `.deb` version: in both formats a hyphen
separates version from release, so `rpmbuild` and `dpkg-deb` produce nothing for
`2.4.1-exp1`. The packaging and package-install smoke jobs are now skipped for a
prerelease, and experimental builds ship the archives only — which is all
`--switch-to-exp` needs. (The first attempt at this tag failed exactly here, looking
for an `.rpm` that could not exist, and skipped publishing as a result.)

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

### 🌐 Networking, WebSocket, AI and parser fixes

A second round of these landed after 2.3.0 was tagged, so they are 2.4.0's:

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

Found by a new offline parity suite (`tests/run_net_tests.sh` against
`tools/mock_fixture_server.py`, which serves HTTP, WebSocket and an
OpenAI-compatible mock): **103 assertions** across `net_http_parity`,
`net_ws_parity` and `ai_mock_parity`.

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


## 2.3.0 (also) — language & package parity

> The second half of the 2.3.0 release. It is listed separately because it was
> developed alongside the performance pass and covers the **packages** (`ai`,
> `discord`, `web`) as much as the interpreter.

### 🌐 Networking, WebSocket, AI and parser fixes
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
