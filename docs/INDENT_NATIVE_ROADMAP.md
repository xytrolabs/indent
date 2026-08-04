# Indent Native Capability Roadmap

## Goal
Make Indent a complete standalone language with a native-first runtime and ecosystem.

## What Is Done Now
- Core runtime primitives: sys/os/time/random/math/json/http/ws/file/env/process.
- Expanded module ecosystem: sys, os, time, random, math, json, http, path, datetime, builtins, discord.
- Native exception flow:
  - `Do / Catch / Otherwise / Lastly`
  - `flag:` for raising errors.
- **Python-style type inference**: `var x = 42` infers `int`, `var name = "Ada"` infers `string`.
- **Compound assignment operators**: `+=`, `-=`, `*=`, `/=`, `%=`.
- **Default parameters**: `fun greet name = "World"`.
- **String interpolation**: `"Hello %name%"`.
- **Comprehensions**: list and dict comprehensions with optional filters.
- **Lambda expressions**: `fn(x): x * 2`.
- **Ternary expressions**: `"adult" if age >= 18 else "child"`.
- **Bitwise operators**: `&`, `|`, `^`, `~`, `<<`, `>>`.
- **Regex, Datetime, Crypto, Path, Functional builtins**.
- **Classes with single inheritance**.
- **Match/case pattern matching**.

## Remaining Native Milestones
1. Language capability
- Class and inheritance model.
- Richer function metadata and composition utilities.
- Async scheduler and task model.
- Generator/yield and iterator protocol.

2. Runtime capability
- Structured exception object hierarchy.
- Rich tracebacks with stack frames and locals.
- Context-based resource management semantics.

3. Standard library breadth
- re, csv, sqlite, path-complete, process-exec-complete.
- Native threading, workers, and process orchestration.
- Rich testing framework utilities.

4. Discord framework depth
- Native event dispatcher with registration helpers.
- Gateway reconnect/backoff/resume orchestration.
- Stateful cache layer for guild/channel/member/message objects.
- Command framework depth (cooldowns, checks, extensions, converters).

## Current Strategy
- Build native Indent primitives first.
- Keep optional external interop isolated from core runtime behavior.
- Ship capabilities in slices with tests for each feature family.
