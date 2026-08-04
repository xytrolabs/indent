# Indent Changelog

## discord.ind 3.0 — 2026-08-03

### 🛰️ Message Monitoring (new gateway events)
The `discord` package now listens to the full non-privileged event set and
dispatches them through the `On` event system (handlers receive `bot data ""`):

| Event name | Fired on |
|---|---|
| `message_edit` | a message is edited (`MESSAGE_UPDATE`) |
| `message_delete` | a message is deleted (with cached content if known) |
| `message_bulk_delete` | bulk message delete |
| `pin_update` | a message is pinned/unpinned (`CHANNEL_PIN_UPDATE`) |
| `reaction_add` / `reaction_remove` | emoji reactions added/removed |
| `reaction_remove_all` / `reaction_remove_emoji` | reactions cleared |
| `ban_add` / `ban_remove` | member banned/unbanned |
| `member_update` | member profile/roles changed |
| `voice_state_update` | member joins/leaves/moves voice |
| `guild_update` | server settings changed |
| `channel_create` / `channel_update` / `channel_delete` | channels created/edited/deleted |

```indent
get On from discord
fun onEdit bot data ""
    say "someone edited a message!"
bot is On bot "message_edit" "onEdit"
```

### 📋 One-call Audit Log
`SetupAudit bot channelId` registers every monitoring event and posts a
coloured embed summary to the given channel. Perfect for moderation oversight.

```indent
get QuickBot, SetupAudit from discord
var bot dynamic = QuickBot "TOKEN" "!"
bot is SetupAudit bot "123456789012345678"
Run bot
```

### 💾 Message Cache
- `CacheMessage msg` — store a message by id (called automatically on create/update)
- `LookupMessage id` — get the last-known message (used so `message_delete` can
  report content that would otherwise be gone)
- `ClearMessageCache` — reset the in-memory cache

### 🎛️ Configurable Intents
- Default intents expanded to the broad **non-privileged** set (`130797`);
  privileged `GUILD_MEMBERS` / `GUILD_PRESENCES` are intentionally excluded so
  bots without portal whitelisting don't get disconnected (4014).
- Override per-bot: `bot.intents is 33281` (minimal) before `Run bot`.

### 📦 Version + Publishing
- New `DiscordVersion` constant (`3.0.0`) and `get DiscordVersion from discord`.
- `air publish` / `aetherpkg` now preserve the source extension (`.ind`) instead
  of forcing `.ath`; `aetherpkg uninstall` removes both.
- Runtime module resolution now also searches `aether_packages/` (local installs)
  and legacy `~/.local/share/aether/site-packages` (global installs), plus a
  `.ath` fallback for old registry packages. `air install discord` now "just
  works" with no `INDENT_PATH` required.
- Fixed `air registry show` crash caused by unbound `LEGACY_REGISTRY_URL_*`.

---

## 1.2.1 — 2026-08-03 (Urgent Patch)

### 🐛 Bug Fixes

#### Module functions can now call their own imports
Previously a function defined in a module (`get X from other`) could NOT call
the functions it imported itself — only imports made in the main script were
visible at call time. This silently broke any modular library (e.g. a Discord
bot split across puzzle/handler files). The runtime now preserves a module's
imported callables and makes them visible to the module's own functions.

#### Fixed frame leak in `exec_call`
Failed function calls (e.g. a variable name parsed as a zero-arg call) left an
argument-evaluation frame behind, which leaked memory and — worse — redirected
top-level variables into a dead scope so they never appeared in the program's
variables. `exec_call` now always pops its frame, even on error.

#### Fixed/updated stale tests
The Rust unit suite was failing 11 tests that still used the removed Aether1
syntax (`def.var:`, `def.fun:`, `Give:`, `say:`, `Get: ... From:`). All have
been rewritten to current Indent syntax. **`cargo test` is now fully green
(12 passed, 0 failed).**

### 🧱 Standard Library

- **`discord` package massively expanded** (173 functions):
  - Zero-config quickstart: `QuickStart` (2-line bot), `MakeBot`
  - Slash commands: `AddSlash`, `SyncSlash`, `SlashWithUser/String/Int/Channel/Role`
  - Presence: `SetStatus`, `SetPlaying`, `SetWatching`, `SetListening`, `SetCompeting`
  - Embeds: `BuildEmbed`, `HexColor`, `AddEmbedField`, `SetEmbedFooter/Image/Thumbnail/Author`
  - Components: buttons, select menus, action rows, `SendWithComponents`, `CtxComponents`
  - Channels/messages/members/roles/guilds/reactions REST helpers (~50 new functions)
- New `Ctx` system (discord.py-style) and `BotHandler` for handler-based commands

### 🚀 Angela bot

- Full moderation suite (16 commands): ban, kick, timeout, mute, unban, purge,
  warn/warnings/delwarn (persistent JSON warning store), slowmode, nick, role,
  userinfo, avatar, serverinfo, modhelp
- Auto-moderation word filter (deletes + warns on bad words)
- Welcome messages on member join

---

## 1.2.0 — 2026-08-03

### 🎉 Major Language Features

#### Default Parameters
```indent
fun greet name = "World"
    say "Hello " + name + "!"
greet           # → "Hello World!"
greet "Ada"     # → "Hello Ada!"
```

#### String Interpolation
```indent
var name string = "Ada"
say "Hello %name%!"         # → "Hello Ada!"
```
Variables wrapped in `%...%` are interpolated into strings automatically in `say` statements.

#### Comprehensions
```indent
[x * x for x in range(5)]           # → [0, 1, 4, 9, 16]
{x: x * 2 for x in range(3)}        # → {"0": 0, "1": 2, "2": 4}
[x for x in list if x > 5]          # Filtered comprehension
```

#### Lambda Expressions
```indent
var double = fn(x): x * 2
say double 5                        # → 10
```

#### Ternary Expressions
```indent
var s string = "adult" if age >= 18 else "child"
```

#### Bitwise Operators
```indent
5 & 3    # → 1 (AND)
5 | 3    # → 7 (OR)
5 ^ 3    # → 6 (XOR)
~5       # → -6 (NOT)
1 << 2   # → 4 (left shift)
8 >> 2   # → 2 (right shift)
```

#### Chained Comparisons
```indent
if 0 < x < 10           # → x > 0 and x < 10
```

#### Identity Operators
```indent
x is empty              # true if x is null/none
x is not y              # identity check
```

### 🏷️ New Keywords & Aliases

| Keyword | Description |
|---|---|
| `null` | Alias for `empty` |
| `for` | `for x in list:` alias for `repeat for x in list:` |
| `import` | `import module` alias for `get module` |
| `open` | File context manager: `open "file.txt" for read as f:` |

### 🔧 Syntax Improvements

- **Return type annotation**: `fun add a b as int` (replaces `-> int`)
- **Function references**: pass functions as values without calling them
- **Decorators**: `@log` / `@cache` before function definitions

### 📦 New Built-in Functions (20+)

#### Regex
| Function | Description |
|---|---|
| `regex_match(pattern, text)` | Returns `true` if pattern matches |
| `regex_search(pattern, text)` | Returns `{start, end, text}` of first match |
| `regex_findall(pattern, text)` | Returns list of all matches |
| `regex_replace(pattern, repl, text)` | Replace all matches |
| `regex_split(pattern, text)` | Split text by regex pattern |

#### Datetime
| Function | Description |
|---|---|
| `time_utc()` | Unix timestamp (alias for `time_now`) |
| `time_format(ts, fmt)` | Format timestamp (e.g. `"%Y-%m-%d %H:%M:%S"`) |
| `time_parse(str)` | Parse ISO 8601 string to timestamp |

#### Crypto & Encoding
| Function | Description |
|---|---|
| `uuid()` | Generate random UUID v4 |
| `base64_encode(text)` | Encode text to Base64 |
| `base64_decode(text)` | Decode Base64 to text |
| `hash_sha256(text)` | SHA256 hash of text |

#### Path & Filesystem
| Function | Description |
|---|---|
| `glob(pattern)` | List files matching wildcard pattern |
| `path_join(a, b, ...)` | Join path components |
| `path_basename(path)` | Extract filename from path |
| `path_dirname(path)` | Extract directory from path |

#### Functional Programming
| Function | Description |
|---|---|
| `map(list, func)` | Apply function to each element |
| `filter(list, func)` | Filter list by predicate function |

#### String Helpers
| Function | Description |
|---|---|
| `pad_left(text, width, char)` | Left-pad string to width |
| `pad_right(text, width, char)` | Right-pad string to width |
| `repeat_str(text, count)` | Repeat string N times |

### 📚 Standard Library

- **AIR** (Accessible Indent Registry): Package manager for Indent
  - `air install <pkg>` — install packages
  - `air search <query>` — search registry
  - `air publish <name> <file>` — publish packages
  - Registry: `https://github.com/xytrolabs/air`
- 17 standard packages: agame, builtins, colors, config, csv, datetime, discord, html, http, json, jsondb, math, os, path, random, sys, time

### 🛠️ Tooling

- `indent --version` displays `indent 1.2.0`
- `for` loop alias for `repeat for`
- `import` alias for `get`
- Generator functions with `yield`
- Decorator syntax `@name` before functions

### 🧹 Removals

- Old Aether1 syntax (`def.var:`, `def.fun:`, `say:`, `Give:`, `makeType:`) is deprecated and no longer parsed
- `STOP`/`NEXT`/`RESET` uppercase variants removed — use lowercase `stop`/`next`/`reset`

---

## Earlier Versions

### 1.0.0 — Initial Release
- Core language: variables, functions, control flow, loops
- Classes with single inheritance
- Match/case pattern matching
- Do/Catch/Otherwise/Lastly error handling
- 130+ built-in functions
- HTTP, WebSocket, JSON, file I/O, OS operations
- Interactive debugger (`--debug`)
- LSP server for IDE integration
- Package manager (AIR)
- REPL, formatter, linter, test runner
