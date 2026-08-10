# Indent Package Registry — Reference

> 50 packages available via the [AIR registry](https://github.com/xytrolabs/air).
> Install with `air install <package>`, then import with `get <Function> from <package>`.

```bash
air install stats
air install markdown yaml logger
```

---

## Core Utilities

### `slug` — URL slugification
| Function | Description |
|---|---|
| `Slugify(text)` | Lowercase, strip non-alphanumerics, join with `-` |

### `textwrap` — Text wrapping
| Function | Description |
|---|---|
| `Wrap(text, width)` | Split into wrapped lines |
| `Fill(text, width)` | Join wrapped lines with newlines |
| `Center(text, width)` | Center text in a width |

### `roman` — Roman numerals
| Function | Description |
|---|---|
| `ToRoman(n)` | int → Roman (1..3999) |
| `FromRoman(text)` | Roman → int |

### `lev` — Levenshtein distance
| Function | Description |
|---|---|
| `Distance(a, b)` | Edit distance between two strings |

### `base` — Base conversion
| Function | Description |
|---|---|
| `ToString(n, base)` | int → string (base 2-36) |
| `FromString(text, base)` | string → int |
| `Bin(n)` / `Oct(n)` / `Hex(n)` | Shorthand conversions |

### `diff` — Line diff
| Function | Description |
|---|---|
| `LCSLength(a, b)` | Longest common subsequence length |
| `Similarity(a, b)` | 0.0-1.0 similarity score |

### `chunk` — List chunking
| Function | Description |
|---|---|
| `Chunk(items, size)` | Split into fixed-size chunks |
| `Pairs(items)` | Chunks of 2 |
| `Windows(items, size)` | Sliding windows |

### `search` — Searching
| Function | Description |
|---|---|
| `BinarySearch(items, target)` | Index or -1 (sorted list) |
| `LinearSearch(items, target)` | Index or -1 |

---

## Data Structures

### `stack` — LIFO stack (immutable)
| Function | Description |
|---|---|
| `New()` | `[]` |
| `Push(stack, item)` | New stack with item |
| `Pop(stack)` | `{"item":.., "stack":..}` |
| `Peek(stack)` | Top without removing |
| `IsEmpty(stack)` / `Size(stack)` | Introspection |

### `queue` — FIFO queue (immutable)
| Function | Description |
|---|---|
| `New()` / `Enqueue(q, item)` | Create / add |
| `Dequeue(q)` | `{"item":.., "queue":..}` |
| `Peek(q)` / `IsEmpty(q)` / `Size(q)` | Introspection |

### `linkedlist` — persistent list
| Function | Description |
|---|---|
| `New()` / `Cons(value, rest)` | Create / prepend |
| `Head(list)` / `Tail(list)` | Access |
| `Append(list, v)` / `Prepend(list, v)` | Add |
| `Nth(list, n)` / `Size` / `IsEmpty` / `ToList` | Access |

### `lru` — LRU cache
| Function | Description |
|---|---|
| `New(capacity)` | `{"capacity":..,"data":{}}` |
| `Get(cache, key)` / `Put(cache, key, v)` | Access |
| `Contains` / `Size` / `Clear` | Management |

### `heap` — min-heap
| Function | Description |
|---|---|
| `Push(heap, value)` | New heap with value |
| `Pop(heap)` | `[min_value, new_heap]` |

### `counter` — counting
| Function | Description |
|---|---|
| `Count(items)` | `{value: count}` dict |
| `MostCommon(items, n)` | Sorted `[count, value]` pairs |
| `Total(items)` | Item count |

---

## Math & Stats

### `stats` — statistics
| Function | Description |
|---|---|
| `Mean(items)` / `Median(items)` / `Mode(items)` | Central tendency |
| `Variance(items)` / `StdDev(items)` | Spread |
| `Min` / `Max` / `Sum` | Aggregates |

### `matrix` — matrices (lists of lists)
| Function | Description |
|---|---|
| `Add(a, b)` / `Multiply(a, b)` | Arithmetic |
| `Transpose(m)` / `Identity(n)` / `ScalarMul(m, s)` | Operations |

### `vector` — vector math
| Function | Description |
|---|---|
| `Add(a, b)` / `Subtract(a, b)` | Arithmetic |
| `Scale(v, s)` / `Dot(a, b)` | Scaling / dot product |
| `Magnitude(v)` / `Normalize(v)` | Length / unit vector |

### `fraction` — rational numbers
| Function | Description |
|---|---|
| `New(num, den)` | Reduced fraction `{"num":..,"den":..}` |
| `Add` / `Subtract` / `Multiply` / `Divide` | Arithmetic |
| `ToFloat(f)` / `ToString(f)` | Conversion |

### `units` — unit conversion
| Function | Description |
|---|---|
| `KmToMiles` / `MilesToKm` | Distance |
| `CToF` / `FToC` | Temperature |
| `KgToLb` / `LbToKg` | Weight |
| `LitersToGal` / `GalToLiters` | Volume |
| `BytesToMb` / `MbToBytes` | Storage |

---

## Text & Encoding

### `markdown` — markdown to HTML
| Function | Description |
|---|---|
| `Render(text)` | Headers, bold/italic/code, lists |

### `htmltable` — HTML tables
| Function | Description |
|---|---|
| `Table(headers, rows)` | From headers + rows |
| `TableFromDicts(records)` | From list of dicts |

### `asciitable` — plain-text tables
| Function | Description |
|---|---|
| `Table(headers, rows)` | ASCII box table |

### `xml` — minimal XML
| Function | Description |
|---|---|
| `Tags(text)` | `[{"name","attrs","content"}]` |
| `DecodeEntities(text)` | `&lt;` → `<` etc. |

### `yaml` — minimal YAML
| Function | Description |
|---|---|
| `Parse(text)` | Flat `key: value` → dict |

### `jsonptr` — JSON Pointer
| Function | Description |
|---|---|
| `Get(data, pointer)` | Navigate `/a/b/c` |
| `Has(data, pointer)` | True if exists |

### `csv` — CSV
| Function | Description |
|---|---|
| `Parse(text)` | Rows as lists |
| `Stringify(rows)` | Rows → CSV text |

### `html` — HTML builder
| Function | Description |
|---|---|
| `Escape(text)` / `Tag(...)` / `VoidTag(...)` / `Render(...)` | Build HTML |

### `ansi` — terminal colors
| Function | Description |
|---|---|
| `Red` / `Green` / `Yellow` / `Blue` / `Magenta` / `Cyan` | Color wrap |
| `Bold` / `Dim` / `Underline` / `Reset` | Style |

---

## Files & Config

### `env` — .env loader
| Function | Description |
|---|---|
| `Load(path)` | Parse KEY=VALUE pairs |
| `Get(path, key, default)` | Typed lookup |

### `config` — INI parser
| Function | Description |
|---|---|
| `Parse(text)` / `Read(path)` | Parse sections |
| `Get(config, key, default)` / `GetSection(...)` | Lookup |

### `jsondb` — JSON database
| Function | Description |
|---|---|
| `Load` / `Save` / `Create` | Persistence |
| `FindAll` / `FindOne` | Query |
| `Add` / `Update` / `Delete` / `Size` / `All` | CRUD |

### `temp` — temp files
| Function | Description |
|---|---|
| `Dir()` / `Path(prefix)` / `Write(prefix, content)` | Create |
| `Read(path)` / `Remove(path)` | Use |

### `filelock` — file locking
| Function | Description |
|---|---|
| `Lock(path)` / `Unlock(path)` / `IsLocked(path)` | Manage locks |

### `globx` — glob matching
| Function | Description |
|---|---|
| `Match(pattern, text)` | `*` and `?` support |
| `Filter(patterns, items)` | Filter list |

### `mime` — MIME types
| Function | Description |
|---|---|
| `ForFile(name)` | Extension → MIME string |

---

## System & CLI

### `args` — CLI argument parsing
| Function | Description |
|---|---|
| `Parse(argv)` | `{"flags":..,"positional":..}` |
| `Has(flags, name)` / `Get(flags, name, default)` | Lookup |

### `logger` — leveled logging
| Function | Description |
|---|---|
| `SetLevel(level)` | debug/info/warn/error |
| `Debug` / `Info` / `Warn` / `Error` | Log with timestamp |

### `progress` — progress bars
| Function | Description |
|---|---|
| `Bar(current, total, width)` | `[====      ] 50%` string |

### `timer` — benchmarking
| Function | Description |
|---|---|
| `Start()` / `Elapsed(timer)` | Stopwatch |
| `Measure(iterations, fn)` | `{"total","avg","iterations"}` |

### `retry` — retry logic
| Function | Description |
|---|---|
| `WithRetries(attempts, fn)` | Call fn, retry on error |

### `password` — password generation
| Function | Description |
|---|---|
| `Generate(length)` | Random alphanumeric + symbols |
| `GeneratePin(length)` | Random digits |

### `semver` — semantic versions
| Function | Description |
|---|---|
| `Parse(version)` | `[major, minor, patch]` |
| `Compare(a, b)` | -1/0/1 |
| `IsGreater` / `IsLess` / `IsEqual` | Comparisons |

---


## `ai` — AI assistant package (like Python's `openai` SDK) (v1.2)

> Full guide: [`docs/ai-package.md`](ai-package.md)

An **OpenAI-native** client that talks to *any* OpenAI-compatible API — works with **real OpenAI** *and* a **local Ollama** server (which exposes the same OpenAI API at `/v1`). Mirrors the OpenAI Python SDK surface: chat completions, embeddings, model listing, cosine similarity, and semantic search, all in pure Indent. **Robust (v1.2): never crashes on transient errors — retries with backoff and exposes `GetLastError()`/`GetLastStatus()`/`WasError()`.**

```indent
get ai as AI
#! Local Ollama (default, no key):
var reply = AI.Chat("qwen2.5:0.5b", [{"role":"user","content":"hi"}])

#! Real OpenAI — just point base + key:
AI.SetBase("https://api.openai.com/v1")
AI.SetApiKey("sk-...")          #! from platform.openai.com
var gpt = AI.Chat("gpt-4o-mini", [{"role":"user","content":"hi"}])
```

| Function | OpenAI SDK equivalent | Description |
|---|---|---|
| `Chat(model, messages)` | `client.chat.completions.create()` | Chat completion; `messages` = list of `{"role","content"}`; returns assistant reply text |
| `Ask(model, prompt)` | `client.completions.create()` | Single-prompt completion; returns generated text |
| `Embed(model, text)` | `client.embeddings.create()` | Single text → embedding vector (list of floats) |
| `EmbedMany(model, texts)` | `client.embeddings.create()` | Batch: list of texts → list of vectors |
| `Models()` | `client.models.list()` | List model names from the server |
| `Similarity(a, b)` | — | Cosine similarity between two embeddings |
| `Search(query, docs)` | — | Semantic search: rank `docs` by similarity to `query`, best-first |
| `SetBase(url)` | `OpenAI(base_url=...)` | Set API base (default `http://localhost:11434/v1`) |
| `SetApiKey(key)` | `OpenAI(api_key=...)` | Set `Authorization: Bearer <key>` (empty = no auth, for local Ollama) |
| `SetDefaultModel(name)` | client config | Default chat model (default `qwen2.5:0.5b`) |
| `SetDefaultEmbedModel(name)` | client config | Default embedding model (default `nomic-embed-text`) |
| `GetBase()` | — | Return the current base URL |
| `SetRetries(n)` | — | Retry failed requests `n` times with backoff (default 2) |
| `GetLastError()` | — | Last error message (`""` = success) |
| `GetLastStatus()` | — | Last HTTP status (0 = no response) |
| `WasError()` | — | True if the last call failed |

Two import styles — dot-call namespace or per-function:
```indent
#! Style 1: namespace (get ai as AI → AI.Chat)
get ai as AI
var reply = AI.Chat("qwen2.5:0.5b", [{"role":"user","content":"Hello"}])
say reply

#! Style 2: per-function import
get Chat from ai
get Embed from ai
var reply2 = Chat("qwen2.5:0.5b", [{"role":"user","content":"2+2?"}])
var vec = Embed("nomic-embed-text", "Indent programming")
```

Semantic search:
```indent
get ai as AI
var docs = ["Python is a language", "Cats are pets"]
var ranked = AI.Search("programming languages", docs)
repeat pair in ranked
    say string(pair[0]) + "  " + string(pair[1])   #! similarity + doc
```

Under the hood it uses Indent's native `http_post_json` / `http_get` builtins (no Python needed) — see `examples/ai_openai_api.ind` for both the package API and the raw low-level way to call any REST API. Local target: [Ollama](https://ollama.com) at `http://localhost:11434/v1`.

---

## `ingame` — PyGame-style 2D game framework (v2.0)

> Full guide: [`docs/ingame-package.md`](ingame-package.md)

**InGame** ("Indent Game") mirrors PyGame's API so games are written entirely in Indent — movement, physics, collision, AI, tilemaps, and rendering all live in Indent. **v2.0 merges the old `agame` package and adds game-dev APIs** (camera, tilemaps, sprites, collision) for building RPGs, tile worlds, and action games.

| Function | PyGame equivalent | Description |
|---|---|---|
| `Init()` | `pygame.init()` | Initialize (state is lazy; no-op) |
| `SetMode(w, h, title)` | `pygame.display.set_mode()` | Spawn the native window, prep IPC files, return workdir |
| `DrawRect(x, y, w, h, color)` | `pygame.draw.rect()` | Rectangle |
| `DrawRectRot(x, y, w, h, color, rot)` | `transform.rotate()` | Rotated rectangle |
| `DrawCircle(cx, cy, r, color)` | `pygame.draw.circle()` | Circle |
| `DrawEllipse(cx, cy, rx, ry, color)` | `pygame.draw.ellipse()` | Ellipse |
| `DrawArc(cx, cy, r, a1, a2, color)` | `pygame.draw.arc()` | Pie slice (health/cooldown wheels) |
| `DrawLine(x1, y1, x2, y2, color, w)` | `pygame.draw.line()` | Line |
| `DrawPolygon(points, color)` | `pygame.draw.polygon()` | Polygon |
| `DrawSprite(x, y, w, h, glyph)` | `pygame.image` | Emoji/glyph sprite (no asset files) |
| `DrawText(x, y, str, color, size)` | `pygame.font` | Text |
| `Flip(clear)` | `pygame.display.flip()` | Flush frame to window, reset shapes |
| `GetEvents()` | `pygame.event.get()` | Read + clear events (`quit`/`keydown`/`keyup`/`mousemove`/`mousedown`/`mouseup`) |
| `GetKeys()` | `pygame.key.get_pressed()` | Held key names |
| `IsKeyDown(key)` | — | Is a key held? |
| `GetMouse()` | `pygame.mouse.get_pos()` | `[x, y]` cursor |
| `Tick(fps)` | `pygame.time.Clock.tick()` | Sleep to target frame rate |
| `Quit()` | `pygame.quit()` | Close window and exit |

**v2.0 game-dev APIs** (merged from `agame` + new):

| Function | Description |
|---|---|
| `SetCamera(x, y)` / `GetCamera()` | Camera / world scrolling (camera-follow) |
| `MakeTilemap(rows, cols)` / `SetTile` / `GetTile` / `IsSolidAt` | Grid tile worlds |
| `DrawTilemap(map, tileSize, legend, w, h)` | Camera-culled tile rendering |
| `NewEntity(x, y, w, h)` / `Move` / `Collides` | Entities + AABB overlap |
| `StepPhysics(entity, g)` / `MoveInMap(entity, dx, dy, map, tileSize, legend)` | Velocity, gravity, tile collision |
| `Clamp` / `Lerp` / `Distance` / `Wrap` | Math helpers |
| `TileToWorld` / `WorldToTile` | Tile ↔ pixel conversion |

Compatibility aliases kept: `Clear`, `Rect`, `Circle`, `Line`, `Polygon`, `Text`, `Sprite`, `Ellipse`, `Present`, `Events`, `Keys`, `Mouse`.

```indent
get Init from ingame
get SetMode from ingame
get DrawRect from ingame
get DrawCircle from ingame
get Flip from ingame
get GetEvents from ingame
get Quit from ingame

Init()
var win = SetMode(400, 400, "My Game")
repeat while running
    repeat e in GetEvents()
        if e["type"] == "quit"
            running is false
        if e["type"] == "keydown"
            #! handle key
    DrawRect 10 10 50 50 "#39d353"
    DrawCircle 200 200 20 "#f85149"
    Flip "#000000"          #! clear color = background
    time_sleep 0.05
Quit()
```

Requires the `indent-ingame` native helper (built by `install.sh`; needs gcc + gtk3 + webkit2gtk). See `examples/snake_game.ind` and `examples/breakout_game.ind` for complete games.

## Web & More

### `url` — URL encoding
| Function | Description |
|---|---|
| `EncodeComponent(text)` | Percent-encode |
| `DecodeComponent(text)` | Percent-decode |

### `cookie` — HTTP cookies
| Function | Description |
|---|---|
| `Parse(header)` / `Stringify(cookies)` | Serialize |
| `Get(cookies, name, default)` | Lookup |

### `colors` — named colors
| Function | Description |
|---|---|
| `Red` ... `Navy` | 16 named color constants |
| `HexToRGB(hex)` / `RGBToHex(r,g,b)` | Conversion |

### `agame` — 2D game helpers (merged into ingame v2.0)
> Full guide: [`docs/agame-package.md`](agame-package.md)

**Merged into `ingame` in v2.0.** `agame` remains as a compatibility shim re-exporting from ingame — old `get X from agame` code still works. **New code should `get X from ingame`.**

| Function | Description |
|---|---|
| `Lerp` / `Clamp` / `Distance` / `Wrap` | Math |
| `NewEntity` / `Move` / `Collides` / `StepPhysics` / `MoveInMap` | Entities + physics |
| `TileToWorld` / `WorldToTile` / `MakeTilemap` / `DrawTilemap` | Tiles |

### `discord` — Discord bot library (v6.0)
> Full guide: [`docs/discord-package.md`](discord-package.md)

| Function | Description |
|---|---|
| `Bot` / `Command` / `Ready` / `Message` / `Start` | v6.0 production bot API (minimal boilerplate) |
| `CtxSend` / `CtxReply` / `CtxEmbed` / `CtxEphemeral` | Ctx helpers |
| `QuickBot` / `QuickStart` / `BotFromEnv` | Easy entry points |
| `AddSlash` / `SyncSlash` / `SlashWithUser` | Slash commands |
| `SetupAudit` / `Audit` | Audit log & monitoring |
| `LoadPuzzles` / `RegisterCog` | Puzzle/cog system |
| `Send` / `Kick` / `Ban` / `Timeout` / `AddRole` | REST actions |
