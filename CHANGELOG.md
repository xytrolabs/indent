# Indent Changelog

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

# Indent Changelog

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
