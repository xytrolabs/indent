# Indent Package Registry — Reference

> 47 packages available via the [AIR registry](https://github.com/xytrolabs/air).
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


## `ingame` — PyGame-style 2D game framework (v1.5.0)

All game logic lives in Indent; a native window (WebKitGTK canvas) renders frames and reports input.

| Function | Description |
|---|---|
| `Init(w, h, title)` | Spawn the native window, prep IPC files, return workdir |
| `Clear(color)` | Reset the current frame's shape list |
| `Rect(x, y, w, h, color)` | Add a rectangle to the frame |
| `Circle(cx, cy, r, color)` | Add a circle to the frame |
| `Text(x, y, str, color, size)` | Add text to the frame |
| `Present(clear)` | Flush the frame to the window |
| `Events()` | Read + clear input events (list of `{"key","down"}` / `{"type":"quit"}`) |
| `Quit()` | Close the window and exit |

```indent
get Init from ingame
get Rect from ingame
get Present from ingame
get Events from ingame

var win = Init(400, 400, "My Game")
repeat while running
    repeat e in Events()
        if e["type"] == "quit"
            running is false
    Rect 10 10 50 50 "#39d353"
    Present "#000000"
    time_sleep 0.05
Quit()
```

Requires the `indent-ingame` native helper (built by `install.sh`; needs gcc + gtk3 + webkit2gtk). See `examples/snake_game.ind` for a complete game.

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

### `agame` — 2D game helpers
| Function | Description |
|---|---|
| `Lerp` / `Clamp` / `Distance` / `Wrap` | Math |
| `NewEntity` / `Move` / `Collides` | Entities |
| `TileToWorld` / `WorldToTile` | Tile math |

### `discord` — Discord bot library
| Function | Description |
|---|---|
| (see `docs/discord-package.md`) | Bots, commands, slash commands |
