# Indent Standard Packages — Reference

> All 17 packages available via `get <name>` in Indent 1.4.0.

---

## `html` — HTML Templating & Builder

```indent
get Escape from html
get Tag from html
get Page from html
```

| Function | Description |
|---|---|
| `Escape(text)` | HTML-escape `&`, `<`, `>`, `"`, `'` |
| `Tag(name, attrs, content)` | Build `<name attrs>content</name>` |
| `VoidTag(name, attrs)` | Self-closing tag like `<img />` |
| `Render(template, vars)` | Replace `{{key}}` with dict values |
| `Div(attrs, content)` | `<div>` wrapper |
| `Span(attrs, content)` | `<span>` wrapper |
| `Heading(level, attrs, content)` | `<h1>`–`<h6>` |
| `Paragraph(attrs, content)` | `<p>` wrapper |
| `Link(href, text)` | `<a href>` tag |
| `Image(src, alt)` | `<img>` tag |
| `UnorderedList(items)` | `<ul>` from list |
| `OrderedList(items)` | `<ol>` from list |
| `Table(headers, rows)` | Full `<table>` |
| `Page(title, headExtra, body)` | Full HTML5 document |
| `StyleLink(href)` | CSS `<link>` |
| `Script(src)` | JS `<script>` |
| `InlineStyle(css)` | `<style>` block |
| `Form(action, method, content)` | `<form>` wrapper |
| `Input(type, name, attrs)` | Form `<input>` |
| `TextInput(name, attrs)` | Text input shortcut |
| `TextArea(name, attrs, content)` | Textarea |
| `Button(attrs, content)` | `<button>` |

---

## `csv` — CSV Parsing & Generation

```indent
get Parse from csv
get Stringify from csv
get ToDicts from csv
```

| Function | Description |
|---|---|
| `Parse(text)` | CSV text → list of lists |
| `Stringify(rows)` | List of lists → CSV text |
| `Read(path)` | Read CSV file |
| `Write(path, rows)` | Write CSV file |
| `Headers(rows)` | Get first row as headers |
| `ToDicts(rows)` | Convert to list of dicts (first row = keys) |

---

## `jsondb` — JSON File Database

```indent
get Create from jsondb
get Add from jsondb
get FindOne from jsondb
```

| Function | Description |
|---|---|
| `Create(path)` | Create empty database file |
| `Add(path, record)` | Insert a record (dict) |
| `FindAll(path, conditions)` | Find all matching records |
| `FindOne(path, conditions)` | Find first matching record |
| `Update(path, conditions, updates)` | Update matching records |
| `Delete(path, conditions)` | Remove matching records |
| `Size(path)` | Count records |
| `All(path)` | Get all records |

---

## `config` — INI Config Parser

```indent
get Parse from config
get GetSection from config
```

| Function | Description |
|---|---|
| `Parse(text)` | INI text → nested dict |
| `Read(path)` | Read and parse INI file |
| `Get(config, key, default)` | Get value with fallback |
| `GetSection(config, section, key, default)` | Get from `[section]` |

---

## `json` — JSON Helpers

```indent
get Loads from json
get Dumps from json
```

| Function | Description |
|---|---|
| `Loads(text)` | Parse JSON string |
| `Dumps(value, pretty)` | Serialize to JSON |
| `Load(path)` | Read and parse JSON file |
| `Dump(value, path, pretty)` | Write JSON to file |

---

## `http` — HTTP Client

```indent
get Get from http
get PostJson from http
```

| Function | Description |
|---|---|
| `Get(url, auth)` | GET request |
| `Delete(url, auth)` | DELETE request |
| `PostJson(url, payload, auth)` | POST JSON |
| `PutJson(url, payload, auth)` | PUT JSON |
| `PatchJson(url, payload, auth)` | PATCH JSON |
| `IsOk(response)` | Check if response succeeded |
| `StatusCode(response)` | HTTP status code |
| `Text(response)` | Response body as string |
| `Json(response)` | Parse response body as JSON |

---

## `math` — Math Constants & Functions

```indent
get PI from math
get Pow from math
```

| Constant | Value |
|---|---|
| `PI` | 3.141592653589793 |
| `TAU` | 6.283185307179586 |
| `E` | 2.718281828459045 |

| Function | Description |
|---|---|
| `Abs(n)` | Absolute value |
| `Sqrt(n)` | Square root |
| `Pow(base, exp)` | Power |
| `Floor(n)` | Floor |
| `Ceil(n)` | Ceiling |
| `Round(n)` | Round |
| `Sin(n)` / `Cos(n)` / `Tan(n)` | Trig (radians) |
| `Log(n)` / `Log10(n)` | Natural / base-10 log |
| `Exp(n)` | e^n |

---

## `os` — OS & Filesystem

```indent
get GetCwd from os
get Exists from os
get ReadText from os
```

| Function | Description |
|---|---|
| `GetCwd()` | Current directory |
| `Chdir(path)` | Change directory |
| `Environ()` | Get environment variable |
| `GetEnv(key, default)` | Get env var with default |
| `SetEnv(key, value)` | Set env var |
| `Exit(code)` | Exit process |
| `System(cmd)` | Run shell command |
| `Exists(path)` | Check path exists |
| `IsFile(path)` | Check if file |
| `IsDir(path)` | Check if directory |
| `ListDir(path)` | List directory |
| `Mkdir(path)` | Create directory |
| `Remove(path)` | Delete file/dir |
| `Rename(src, dst)` | Rename/move |
| `ReadText(path)` | Read file |
| `WriteText(path, text)` | Write file |
| `AppendText(path, text)` | Append to file |

---

## `time` — Time & Sleep

| Function | Description |
|---|---|
| `Time()` | Current Unix timestamp |
| `Sleep(ms)` | Sleep milliseconds |

---

## `random` — Random Numbers

| Function | Description |
|---|---|
| `RandInt(min, max)` | Random integer |
| `RandFloat()` | Random float 0–1 |
| `Choice(list)` | Random element |
| `Shuffle(list)` | Shuffled copy |

---

## `sys` — System Info

| Constant | Description |
|---|---|
| `name` | Platform name (`"linux"`, `"darwin"`, `"win32"`) |
| `sep` | Path separator (`"/"`) |
| `linesep` | Line separator (`"\n"`) |

---

## `discord` — Discord Bot Framework

```indent
get NewBot from discord
get kick from discord
get say from discord
get add from discord
get start from discord
```

**Clean block API (v2.2.0)** — build bots with pre-made blocks:

| Category | Functions |
|---|---|
| **Bot** | `NewBot(token, prefix)`, `add(bot, name, handler)`, `on(bot, event, handler)`, `start(bot)` |
| **Actions** | `kick(bot, user, reason)`, `ban(bot, user, reason)`, `addRole(bot, user, role)`, `removeRole(bot, user, role)`, `dm(bot, user, msg)` |
| **Reply** | `say(bot, message)` |
| **Puzzles** | `load(bot, dir)` — auto-load command modules from a directory |

Example:
```indent
fun kickCmd args
    var user string = args["1"]
    kick bot user "No reason"
    say bot "✅ Kicked " + user
add bot "kick" kickCmd        # ✅ Handler as reference!
start bot
```

Full REST API, gateway, embeds, buttons, slash commands also available in the base package.

---

## `colors` — Color Constants

```indent
get RED from colors
get BLUE from colors
```

Predefined: `RED`, `GREEN`, `BLUE`, `CYAN`, `MAGENTA`, `YELLOW`, `ORANGE`, `PURPLE`, `PINK`, `BLACK`, `WHITE` — all as hex strings (e.g., `"#ff4d4d"`).

---

## `datetime` — Date/Time Helpers

Built on the `time` module. Provides structured date/time utilities.

---

## `path` — Path Utilities

Filesystem path manipulation helpers. Joins, splits, extensions, basename, dirname.

---

## `agame` — Starter 2D Game API

A simple game development API for learning and prototyping 2D games in Indent.

---

## `builtins` — Function Aliases

Provides convenience aliases for common built-in functions: `Print`, `Input`, `Len`, `Assert`, etc.
