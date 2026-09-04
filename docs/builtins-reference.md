# Indent Built-in Functions — API Reference (v2.0)

> Complete reference for every built-in function available in Indent 2.0.
> **Types**: `string`, `int`, `float`, `boolean`, `dynamic`, `empty`/`null`, `list`, `dict`, `group`
> **🆕 v2.0**: full color subsystem — `fg`/`bg`, `style`, `gradient`, `multicolor`, `rainbow`, `paint` (see [Colors](#colors-indent-20))
> **🆕 v1.6.2**: `builtins()` returns an organized dict (category → names); type-check helpers `is_list`/`is_dict`/`is_string`/`is_number`/`is_int`/`is_float`/`is_bool`/`is_group`
> **🆕 v1.6.1**: `colored(text, color)` for colored terminal output, `builtins()`, `get <builtin>`
> **🆕 v1.6**: async I/O (`http_get_async` & co), set ops (`set_union`/`set_intersection`/`set_difference`), YAML (`yaml_loads`/`yaml_dumps`), path helpers (`path_ext`/`path_stem`/`path_abs`/`path_expand`/`path_norm`), string methods (`str_zfill` & co)
> **🆕 v1.5**: async tasks (`spawn`/`task_wait`/`parallel`), SQLite, CSV, `walk`, `os_run`, TOML, gzip/zip, typed errors, `log`, `counter`
> **🆕 v1.4**: Group type (`group([...])`), type conversion (`set varname type`), group methods
> **🆕 v1.3**: Type inference (`var x = 42`), compound assignment (`x += 5`)
>
> **Note on groups:** unique ordered collections are called **groups** — created
> with `group([...])` **or** `set([...])` (both build a group; `set` is the
> canonical builder, `group` an alias). The `set` **keyword** is reserved for
> type conversion (`set varname type`) and must not be confused with the
> `set([...])` function call.

---

## How to get a builtin

**Browse every builtin** with `builtins()`, which returns an **organized dict**
grouped by category (like Python's `dir(__builtins__)`, but tidier):

```indent
var b = builtins()          # → {string: [...], math: [...], os/file: [...], ...}
var cats = keys b           # → [async, crypto, data, dict, ...]
var math = b["math"]        # → ["abs", "math_abs", "math_pow", ...]
var osf  = b["os/file"]     # → ["os_exists", "file_read_text", "walk", ...]
```

> Categories: `string`, `list`, `dict`, `group`, `math`, `data`, `text`, `path`,
> `os/file`, `http/net`, `time/random`, `crypto`, `async`, `errors`, `result`,
> `types`, `io`, `system`, `interop`, `misc`.

**Bind a builtin as a value** with `get <builtin>`, so you can pass it around or
call it through a variable (like `import`-ing a module function, but for the
builtin table). This works with an optional `as` alias:

```indent
get len                    # bind len
var n = len [1, 2, 3]      # → 3

get string as to_s         # bind with an alias
var s = to_s 42            # → "42"

get upper
var shout = upper "hi"     # → "HI"
```

> `get <builtin>` is equivalent to `get <name> from module` for module functions,
> but pulls from the builtin table. Builtins passed by value follow the same
> rules as any function value.

---

## Type Checks (🆕 v1.6.2)

Quick boolean tests for a value's type:

| Function | Returns | Description |
|---|---|---|
| `is_list(v)` | boolean | True if `v` is a list |
| `is_dict(v)` | boolean | True if `v` is a dict |
| `is_string(v)` | boolean | True if `v` is a string |
| `is_number(v)` | boolean | True if `v` is an int or float |
| `is_int(v)` | boolean | True if `v` is an int |
| `is_float(v)` | boolean | True if `v` is a float |
| `is_bool(v)` | boolean | True if `v` is a boolean |
| `is_group(v)` | boolean | True if `v` is a group/set |

```indent
is_list([1, 2])   # → TRUE
is_dict({"a": 1}) # → TRUE
is_number(42)     # → TRUE
is_string("hi")   # → TRUE
```

---

## Colors (Indent 2.0)

The color subsystem wraps text in ANSI **truecolor** escape codes — great for
terminal tools, menus, emphasis, and dashboards. You can set the **foreground**,
the **background**, apply **styles**, and even render **gradients**,
**multi-color** and **rainbow** text.

### Color values

Every `color` argument accepts:
- a hex literal — `#RGB`, `#RRGGBB`, or `#RRGGBBAA` (with or without `#`)
- a **named** color (case-insensitive):
  `RED`, `GREEN`, `BLUE`, `CYAN`, `MAGENTA`, `YELLOW`, `ORANGE`, `PURPLE`,
  `PINK`, `BLACK`, `WHITE`, `GRAY`/`GREY`, `SILVER`, `MAROON`, `OLIVE`, `LIME`,
  `TEAL`, `NAVY`, `FUCHSIA`, `BROWN`, `GOLD`, `CORAL`, `SALMON`, `SKY`,
  `INDIGO`, `VIOLET`, `AMBER`, `EMERALD`, `SLATE`, `ZINC`
- a **color variable** — `var accent color = "#22c55e"`

### Foreground & background

| Function | Returns | Description |
|---|---|---|
| `colored(text, color)` / `fg(text, color)` | string | text in foreground `color` |
| `bg(text, color)` | string | text on background `color` |

### Styles

| Function | Returns | Description |
|---|---|---|
| `style(text, style, ...)` | string | apply one or more styles |

`style` names: `bold`, `dim`, `italic`, `underline`, `blink`, `reverse`,
`strikethrough`. Pass several as separate args or as a list:
```indent
say style "bold text" "bold"
say style "bold + italic" ["bold", "italic"]
say style "underlined" "underline"
```

### Gradients, multi-color & rainbow

| Function | Returns | Description |
|---|---|---|
| `gradient(text, from, to)` | string | per-character foreground gradient `from`→`to` |
| `bg_gradient(text, from, to)` | string | per-character background gradient |
| `multicolor(text, color, ...)` | string | cycle colors per character (foreground) |
| `bg_multicolor(text, color, ...)` | string | cycle colors per character (background) |
| `rainbow(text)` | string | classic 7-color foreground rainbow |
| `bg_rainbow(text)` | string | classic 7-color background rainbow |

`multicolor` accepts either separate color args or a single list:
```indent
say gradient "Smooth fade" "#ff0000" "#0000ff"
say multicolor "Multi" "#ff0000" "#00ff00" "#0000ff"
say multicolor "Multi" ["#ff0000", "#00ff00", "#0000ff"]
say rainbow "RAINBOW"
```

### Combine everything with `paint`

`paint(text, fg, bg, style)` applies a foreground, a background and styles in a
single call. Pass `""` (or `[]`) to skip any component.

| Function | Returns | Description |
|---|---|---|
| `paint(text, fg, bg, style)` | string | combined fg + bg + style |

```indent
say paint "Painted" "#ffffff" "#222222" ["bold", "underline"]
say paint "Just background" "" "#ff0000" ""
say paint "Just bold" "" "" "bold"
```

> `colored`, `fg`, `bg`, `style`, `gradient`, `multicolor`, `rainbow` and
> `paint` all return strings, so you can nest and embed them in larger output
> with `+`.

### `debug` package

For quick colored logging, the `debug` std module wraps `colored`:

```indent
get warn from debug
get error from debug
get success from debug
get info from debug

warn "this is a warning"     # yellow
error "this is an error"     # red
success "this is a success"  # green
info "this is info"          # cyan
```

---

---

## Output & Input

### `say expr`
Prints to standard output. Multiple values separated by spaces.
```indent
say "Hello"
say "Count: " + string(42)
```

### `ask(prompt)` → `string`
Reads a line from standard input, returns it as a string.
```indent
var name string = ask("Name: ")
```

### `ask(type, prompt)` → `typed`
Reads input and converts to the specified type (`"string"`, `"int"`, `"float"`, `"boolean"`).
```indent
var age int = ask("int", "Age: ")
```

---

## String Functions

| Function | Returns | Description |
|---|---|---|
| `upper(s)` | string | Convert to UPPERCASE |
| `lower(s)` | string | Convert to lowercase |
| `trim(s)` | string | Remove leading/trailing whitespace |
| `lstrip(s)` | string | Remove leading whitespace |
| `rstrip(s)` | string | Remove trailing whitespace |
| `capitalize(s)` | string | First character uppercase, rest lowercase |
| `title(s)` | string | Title Case Each Word |
| `swapcase(s)` | string | Swap case of each character |
| `replace(text, from, to)` | string | Replace all `from` with `to` |
| `split(text, sep)` | list | Split string by separator |
| `split(text)` | list | Split by whitespace |
| `join(list, sep)` | string | Join list elements with separator |
| `starts_with(s, prefix)` | boolean | Check if string starts with prefix |
| `ends_with(s, suffix)` | boolean | Check if string ends with suffix |
| `str_zfill(text, width)` | string | Pad with leading zeros to `width` (e.g. `"42"`→`"00042"`) |
| `str_ljust(text, width[, pad])` | string | Left-justify in `width` with optional pad char |
| `str_rjust(text, width[, pad])` | string | Right-justify in `width` with optional pad char |
| `str_center(text, width[, pad])` | string | Center in `width` with optional pad char |
| `str_splitlines(text)` | list | Split on line breaks |
| `str_removeprefix(text, prefix)` | string | Strip `prefix` if present |
| `str_removesuffix(text, suffix)` | string | Strip `suffix` if present |
| `str_partition(text, sep)` | list | `[before, sep, after]` |
| `format(template, a, b, ...)` | string | Positional formatting — `{0}`, `{1}`, ... |
| `sformat(template, k, v, ...)` | string | Named formatting — `{key}`; also `%name%` interpolation |
| `contains(s, sub)` | boolean | Check if string contains substring |
| `find(s, sub)` | int | Find position of substring (-1 if not found) |
| `slice(s, start, end)` | string | Extract substring from `start` to `end` |
| `len(s)` | int | Number of characters |
| `reverse(s)` | string | Reverse the string |

---

## List / Collection Functions

| Function | Returns | Description |
|---|---|---|
| `len(coll)` | int | Length of list or dict |
| `append(list, item)` | list | **New** list with item appended (original unchanged) |
| `extend(list, items)` | list | **New** list with items concatenated |
| `insert(list, index, value)` | list | **New** list with value at index |
| `pop(list)` | list | **New** list with last item removed |
| `remove(list, value)` | list | **New** list with first matching value removed |
| `contains(coll, item)` | boolean | Check if item is in collection |
| `sort(list)` | list | **New** sorted list (numbers by value, strings alphabetically) |
| `reverse(list)` | list | **New** reversed list |
| `slice(list, start, end)` | list | Sublist from `start` to `end` |
| `sum(list)` | number | Sum of numeric elements |
| `min(list)` | number | Minimum value |
| `max(list)` | number | Maximum value |
| `any(list)` | boolean | True if any element is truthy |
| `all(list)` | boolean | True if all elements are truthy |
| `count(list, value)` | int | Count occurrences of value |
| `enumerate(list)` | list | List of `[index, value]` pairs |
| `zip(list1, list2)` | list | List of paired elements |
| `range(end)` | list | `[0, 1, ..., end-1]` |
| `range(start, end)` | list | `[start, ..., end-1]` |
| `range(start, end, step)` | list | With custom step |

---

## Std-lib Breadth — itertools / functools / collections (Indent 2.0)

Higher-order and composition helpers, all additive (no new syntax). The
predicate / key / function arguments are **native builtin names** (matching how
`map` and `filter` work).

### itertools-style

| Function | Returns | Description |
|---|---|---|
| `chain(list, ...)` | list | Concatenate several lists into one |
| `flatten(list)` | list | Recursively flatten nested lists |
| `chunk(list, n)` | list | Split into sublists of size `n` |
| `product(list, list, ...)` | list | Cartesian product → list of lists |
| `permutations(list, r?)` | list | All `r`-length permutations (default = full length) |
| `combinations(list, r)` | list | All `r`-length combinations (order-insensitive) |
| `accumulate(list)` | list | Running prefix sums |
| `cycle(list, n)` | list | Repeat the whole list `n` times |
| `repeat_item(item, n)` | list | List of `n` copies of `item` |
| `takewhile(pred, list)` | list | Take elements while `pred` is true |
| `dropwhile(pred, list)` | list | Skip elements while `pred` is true, then take the rest |
| `zip_longest(a, b, fill?)` | list | Pair elements, padding the shorter list with `fill` (default `empty`) |
| `pairwise(list)` | list | Adjacent pairs: `pairwise([1,2,3])` → `[[1,2],[2,3]]` |
| `filterfalse(pred, list)` | list | Elements where `pred` is false |
| `compress(list, selectors)` | list | Elements where the matching selector is truthy |
| `starmap(fn, list_of_lists)` | list | Apply `fn` to each row, unpacking its args |

### collections-style

| Function | Returns | Description |
|---|---|---|
| `unique(list)` | list | Deduplicate, preserving order |
| `partition(list, pred)` | list | `[matching, non-matching]` |
| `group_by(list, keyfn)` | dict | `key → [items]` |
| `max_key(list, keyfn)` | value | Element with the maximum key |
| `min_key(list, keyfn)` | value | Element with the minimum key |
| `first(list, n)` | list | First `n` elements |
| `last(list, n)` | list | Last `n` elements |

### functools-style

| Function | Returns | Description |
|---|---|---|
| `reduce(fn, list, initial?)` | value | Fold left over the list |

```indent
var flat = flatten [[1,2],[3,[4,5]]]        # → [1, 2, 3, 4, 5]
var pr    = product [1,2] [3,4]             # → [[1, 3], [1, 4], [2, 3], [2, 4]]
var parts = partition "is_even" [1,2,3,4]   # → [[2, 4], [1, 3]]
var mx    = max_key ["a","bb","ccc"] "len"  # → "ccc"
var total = reduce "add_int" [1,2,3,4]      # → 10
```

### Math extras

| Function | Returns | Description |
|---|---|---|
| `math_pi` / `math_e` / `math_tau` | float | Constants |
| `math_factorial(n)` | int | `n!` |
| `math_gcd(a, b)` / `math_gcd(list)` | int | Greatest common divisor |
| `math_lcm(a, b)` | int | Least common multiple |
| `math_hypot(a, b)` | float | `sqrt(a² + b²)` |
| `math_log2(x)` | float | Base-2 logarithm |
| `math_degrees(x)` / `math_radians(x)` | float | Angle conversion |

### Random extras

| Function | Returns | Description |
|---|---|---|
| `random_randint(a, b)` | int | Random int in `[a, b]` |
| `random_uniform(a, b)` | float | Random float in `[a, b)` |
| `random_sample(list, k)` | list | `k` distinct random elements |

---

## Dictionary Functions

| Function | Returns | Description |
|---|---|---|
| `keys(dict)` | list | All keys (sorted) |
| `values(dict)` | list | All values (sorted by key) |
| `has_key(dict, key)` | boolean | Check if key exists |
| `items(dict)` | list | List of `[key, value]` pairs |
| `dict_get(dict, key)` | any | Get value by key |
| `dict_set(dict, key, value)` | dict | **New** dict with key set |
| `dict_remove(dict, key)` | dict | **New** dict with key removed |
| `dict_update(dict, updates)` | dict | **New** dict with keys updated |
| `len(dict)` | int | Number of keys |

> ⚠️ Dicts are **copy-on-access**. Modifying a nested dict requires: get → modify → reassign.

---

## Group Functions

> Groups are unique, ordered collections. Create them with `group([...])` **or**
> `set([...])`. Methods mutate by returning a **new** group; reassign the result
> to keep the change. (The `set` **keyword** statement `set x type`, by contrast,
> only performs type conversion.)

| Function / Method | Returns | Description |
|---|---|---|
| `group(list)` | group | Build a group, deduplicating while preserving order (alias of `set`) |
| `set(list)` | group | Build a group, deduplicating while preserving order (canonical builder) |
| `g.add(x)` | group | **New** group with `x` added (no-op if present) |
| `g.remove(x)` | group | **New** group without `x` |
| `g.contains(x)` | boolean | Is `x` in the group? (alias: `g.has(x)`) |
| `len(g)` | int | Number of unique elements |
| `g + g2` | group | Union of two groups |
| `set_union(a, b)` | group | Union of two groups (additive builtin) |
| `set_intersection(a, b)` | group | Elements present in **both** groups |
| `set_difference(a, b)` | group | Elements in `a` but not in `b` |
| `is_missing(g)` | boolean | TRUE if the group is empty |

```indent
var s = group([1, 2, 2, 3])   # → {1, 2, 3}
var t = s.add(4)            # → {1, 2, 3, 4}
var u = t.remove(2)         # → {1, 3, 4}
u.contains(3)               # → TRUE
contains(u, 9)              # → FALSE
type_of(s)                  # → "group"

var a = group([1, 2, 3, 4])
var b = group([3, 4, 5, 6])
set_union(a, b)             # → {1, 2, 3, 4, 5, 6}
set_intersection(a, b)      # → {3, 4}
set_difference(a, b)        # → {1, 2}
```

---

## Type Conversion

| Function | Returns | Description |
|---|---|---|
| `int(v)` | int | Convert to integer |
| `float(v)` | float | Convert to float |
| `string(v)` | string | Convert to string (handles Func → function name) |
| `bool(v)` | boolean | Convert to boolean |
| `type_of(v)` | string | Get type name (`"int"`, `"string"`, `"function"`, etc.) |

> `type_of` returns `"function"` for function references. `string(fn)` returns the function name.

---

## Assertions & Testing

| Function | Description |
|---|---|
| `assert(condition)` | Errors if condition is false |
| `assert(condition, message)` | Errors with custom message |
| `assert_eq(left, right)` | Errors if values differ |
| `assert_eq(left, right, message)` | Errors with custom message |

---

## JSON

| Function | Returns | Description |
|---|---|---|
| `json_loads(text)` | dynamic | Parse JSON string → Indent value |
| `json_dumps(value)` | string | Serialize Indent value → JSON string |

---

## HTTP Client

All HTTP functions return a dict with `status` (int), `body` (string), and `ok` (boolean).

| Function | Description |
|---|---|
| `http_get(url)` | GET request |
| `http_get(url, auth)` | GET with Authorization header |
| `http_post_json(url, payload)` | POST JSON body |
| `http_post_json(url, payload, auth)` | POST with Authorization |
| `http_put_json(url, payload)` | PUT JSON body |
| `http_patch_json(url, payload)` | PATCH JSON body |
| `http_delete(url)` | DELETE request |
| `http_delete(url, auth)` | DELETE with Authorization |
| `http_serve_dir(path, port)` | Start static file server (blocking) |
| `http_serve(handler, port)` | Start a **dynamic** server that calls `handler(request)` per request (blocking) |
| `gui_show_html(html, title, width, height)` | Open HTML in native desktop window |

### Dynamic web server — `http_serve(handler, port)`

`http_serve` runs a web server and, for **every request**, calls your
`handler` function with a **request dict**:
`{method, path, query: {…}, headers: {…}, body}`. Your handler returns a
response — a string (→ `200 text/html`) or a dict with optional
`status`, `body`, `content_type`, and `headers`. Routing is just ordinary
Indent logic in the handler.

```indent
fun handle req
    if req.path == "/"
        give "<h1>Hello from Indent</h1>"
    if req.path == "/greet"
        var q = req.query
        if not has_key q "name"
            give {"status": 400, "body": "missing ?name=", "content_type": "text/plain"}
        give "<h1>Hi " + q["name"] + "!</h1>"
    if req.path == "/json"
        give {"status": 200, "body": "{\"ok\": true}", "content_type": "application/json"}
    give {"status": 404, "body": "not found", "content_type": "text/plain"}

http_serve handle 8080     # blocking: listens until stopped
```

> `http_serve` blocks the script while serving. Each request runs the handler
> in a fresh scope (stateless per request). `req.query["x"]` reads the `?x=…`
> query parameter (URL-decoded). Full example: `examples/web_server.ind`.

---

## GUI

Opens a native GTK+WebKit desktop window — no browser or server needed.
Requires the `indent-gui` helper binary alongside the `indent` executable.

| Function | Returns | Description |
|---|---|---|
| `gui_show_html(html)` | — | Open HTML in window (default title, 1200×800) |
| `gui_show_html(html, title)` | — | Custom title |
| `gui_show_html(html, title, w, h)` | — | Custom title and size |

**Example:**
```indent
var html string = "<h1>Hello, Desktop!</h1>"
gui_show_html html "My App" 800 600
say "Window closed"
```

See also: `agame` package for a cleaner `show(html, title, w, h)` wrapper.

---

## WebSocket

| Function | Returns | Description |
|---|---|---|
| `ws_connect(url)` | int | Connect to WebSocket, returns connection ID |
| `ws_send_text(id, text)` | — | Send text message |
| `ws_recv_text(id)` | string | Receive next text message |

---

## File I/O

| Function | Returns | Description |
|---|---|---|
| `file_read_text(path)` | string | Read entire file as string |
| `file_write_text(path, text)` | — | Write string to file (overwrite) |
| `file_append_text(path, text)` | — | Append string to file |
| `walk(path)` | list | Recursively list **every** file under `path` (sorted depth-first; like `os.walk` / `glob("**/*")`) |

### Running other Indent files

**`launch` is the canonical way** to run another Indent file **in the current
runtime** — the launched file's functions and module-level vars become available
here (include-like):

```indent
# helper.ind defines: fun double x  and  var helperMsg = "..."
launch "helper.ind"
var d = double 21          # → 42  (function from the other file)
say helperMsg              # var from the other file is now in scope
```

| Form | Returns | Notes |
|---|---|---|
| `launch "path"` | — | **Canonical** keyword form |
| `run_file("path")` | — | **Deprecated** — alias of `launch`; kept for compatibility |

> Both forms run the file's top-level code in your script's runtime. To run a
> file as a **separate process** (isolated, via the `indent` binary), use
> `os_run "indent path.ind"` — which returns `{ok, status, stdout, stderr}`.

---

## CSV (🆕 native)

| Function | Returns | Description |
|---|---|---|
| `csv_read(path)` | list | Read a CSV file into a list of rows (each row a list of cell strings). Handles quoted fields, commas, escaped quotes. |
| `csv_write(path, rows)` | — | Write a list of rows (each a list) as CSV. Fields containing commas/quotes/newlines are quoted and escaped. |

```indent
var rows = []
rows is append rows ["name", "age", "city"]
rows is append rows ["Ada", "36", "New York"]
csv_write "people.csv" rows
var back = csv_read "people.csv"     # → [["name","age","city"],["Ada","36","New York"]]
```

---

## SQLite (🆕 native)

| Function | Returns | Description |
|---|---|---|
| `sqlite_exec(path, sql)` | int | Run a non-query statement (CREATE/INSERT/UPDATE/DELETE); returns rows affected |
| `sqlite_query(path, sql)` | list | Run a SELECT; returns a list of rows, each a list of cell values (int/float/string/empty) |
| `sqlite_query_one(path, sql)` | list/empty | Run a SELECT; returns the first row or `empty` |

Each call opens and closes the database file, so it's safe for scripts. `NULL` becomes `empty`.

```indent
sqlite_exec "app.db" "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, age INTEGER)"
sqlite_exec "app.db" "INSERT INTO users (name, age) VALUES ('Ada', 36)"
var rows = sqlite_query "app.db" "SELECT name, age FROM users"   # → [["Ada", 36], ...]
var one = sqlite_query_one "app.db" "SELECT name FROM users WHERE age = 36"
```

---

## Typed Errors (🆕 native)

Errors caught by `catch as err:` are strings. Use these to handle them by type:

| Function | Returns | Description |
|---|---|---|
| `error_type(err)` | string | Human-readable type: `type_error`, `key_error`, `index_error`, `division_by_zero`, `file_not_found`, `json_error`, `network_error`, `syntax_error`, `undefined_variable`, `undefined_function`, `runtime_error`, ... |
| `error_message(err)` | string | Just the message (strips `error[EXXX]: ` prefix and ANSI codes) |

```indent
do:
    var x = d["missing"]
catch as err:
    if error_type(err) == "key_error"
        say "missing key: " + error_message(err)
```

---

## Language Features (🆕)

- **Varargs** — `fun total ...nums` collects extra positional (and unmatched named)
  args into a list `nums`. Any params before `...` are fixed:
  ```indent
  fun greet name ...tags
      # name is fixed, tags is a list of the rest
  ```
- **`with` context manager** — `with` is an alias for `open`, so both
  `open "f.txt" for read as f:` and `with "f.txt" for read as f:` work.

---

## OS & System

| Function | Returns | Description |
|---|---|---|
| `os_getcwd()` | string | Current working directory |
| `os_chdir(path)` | — | Change directory |
| `os_exists(path)` | boolean | Check if path exists |
| `os_is_file(path)` | boolean | Check if regular file |
| `os_is_dir(path)` | boolean | Check if directory |
| `os_list_dir(path)` | list | List directory contents |
| `os_mkdir(path)` | — | Create directory |
| `os_remove(path)` | — | Delete file or empty directory |
| `os_rename(src, dst)` | — | Rename/move file |
| `os_getenv(key, default)` | string | Read environment variable |
| `os_setenv(key, value)` | — | Set environment variable |
| `os_system(command)` | int | Run shell command, returns exit code |
| `os_run(command)` | dict | Run shell command, capture output → `{ok, status, stdout, stderr}` |
| `os_run_ok(command)` | bool | `TRUE` if `command` exits 0 |
| `os_which(command)` | string | Path to an executable on `PATH` (shutil.which); `empty` if not found |
| `os_copy(src, dst)` | — | Copy a file (shutil.copy) |
| `os_move(src, dst)` | — | Move/rename a file (shutil.move) |
| `os_copy_tree(src, dst)` | — | Recursively copy a directory tree (shutil.copytree) |
| `file_size(path)` | int | Size of a file in bytes |
| `process_exit(code)` | — | Exit with code |

---

## Math (via `math` builtins)

| Function | Description |
|---|---|
| `math_abs(n)` | Absolute value |
| `math_pow(base, exp)` | Power (base^exp) |
| `math_sqrt(n)` | Square root |
| `math_floor(n)` | Floor |
| `math_ceil(n)` | Ceiling |
| `math_round(value, digits)` | Round to `digits` decimal places |
| `math_sin(n)` / `math_cos(n)` / `math_tan(n)` | Trigonometry (radians) |
| `math_log(value, base)` / `math_log10(n)` | Log with custom base / base-10 log |
| `math_exp(n)` | e^n |

---

## Time & Random

| Function | Returns | Description |
|---|---|---|
| `time_now()` | float | Current Unix timestamp (seconds) |
| `time_sleep(seconds)` | — | Sleep for N seconds (can be fractional) |
| `random_int(min, max)` | int | Random integer (inclusive) |
| `random_float()` | float | Random float 0.0–1.0 |
| `random_choice(list)` | any | Random element from list |
| `random_shuffle(list)` | list | Shuffled copy of list |

---

## Misc Utilities

| Function | Description |
|---|---|
| `clamp(value, min, max)` | Clamp value between min and max |
| `default(value, fallback)` | Return fallback if value is empty |
| `coalesce(a, b)` | Return first non-empty value |
| `is_missing(value)` | Check if value is `empty` |
| `inc(value)` / `dec(value)` | Increment / decrement (returns new value) |
| `add_int(a, b)` / `sub_int(a, b)` | Integer arithmetic |
| `mul_int(a, b)` / `div_int(a, b)` | Integer arithmetic |
| `mod_int(a, b)` | Integer modulo |
| `counter(list)` | dict — count occurrences of each element → `{element: count}` |
| `log(level, msg)` | — write `[LEVEL] msg` to stderr (simple logging) |
| `builtins()` | list — names of every available builtin (see [How to get a builtin](#how-to-get-a-builtin)) |

---

## TOML (🆕 native)

| Function | Returns | Description |
|---|---|---|
| `toml_loads(text)` | dynamic | Parse TOML text → Indent value (dict/list/int/float/bool/string) |
| `toml_dumps(value)` | string | Serialize an Indent dict → TOML text |

```indent
var cfg = toml_loads "title = \"demo\"\ncount = 3\n"
say cfg["title"]       # → demo
var t = toml_dumps({"name": "Ada", "age": 36})
```

---

## YAML (🆕 native)

| Function | Returns | Description |
|---|---|---|
| `yaml_loads(text)` | dynamic | Parse YAML text → Indent value (dict/list/int/float/bool/string) |
| `yaml_dumps(value)` | string | Serialize an Indent dict → YAML text |

```indent
var cfg = yaml_loads "name: Indent\nfeatures:\n  - async\n  - yaml"
say cfg["name"]            # → Indent
say len(cfg["features"])   # → 2
var y = yaml_dumps({"name": "Ada", "age": 36})
```

---

## Compression (🆕 native)

| Function | Returns | Description |
|---|---|---|
| `gzip_compress(text)` | string | Gzip-compress text; returns compressed bytes **base64-encoded** (text-safe) |
| `gzip_decompress(b64)` | string | Take base64 of gzip data, decompress back to text |
| `zip_list(path)` | list | List the entry names inside a zip archive |
| `zip_extract(path, dest)` | — | Extract a zip archive into a directory (path-safe) |

```indent
var gz = gzip_compress "some text to compress"
var orig = gzip_decompress gz
```

---

## Async / Tasks (🆕 native)

Run functions on background threads and collect results — real concurrency
without `async`/`await` keywords.

| Function | Returns | Description |
|---|---|---|
| `spawn(fn, args...)` | int | Run `fn(args...)` on a background thread; returns a task id. `fn` can be a **name string** or a **function value**. Each task gets its own isolated scope (args passed by value). |
| `task_wait(id)` | value | Block until the task finishes; return its result |
| `task_done(id)` | boolean | Is the task finished? (non-blocking) |
| `task_result(id)` | value | Result if finished, else `empty` (non-blocking) |
| `task_wait_all(ids)` | list | Wait for a list of task ids; return results in order |
| `task_wait_timeout(id, seconds)` | value | Wait up to `seconds`; return the result, or `empty` on timeout (like `asyncio.wait_for`) |
| `parallel(fn, list_of_arglists)` | list | Run `fn` once per arg-list **concurrently**; return results in order (like `asyncio.gather`) |

```indent
fun slow_add a b
    time_sleep 0.1
    give a + b

var id = spawn "slow_add" 2 3
# ... do other work ...
var result = task_wait id        # → 5

# Concurrent batch — each sublist is the args for one call
var results = parallel "slow_add" [[1, 1], [10, 20]]   # → [2, 30]

# Spawn with a function value instead of a name string
var f = slow_add
var id2 = spawn f 4 5

# Wait with a timeout — returns empty if it takes too long
var r = task_wait_timeout id2 2.0
```

> Built on a thread-safe runtime (module storage uses `Arc`/`Mutex`), so tasks
> run in real parallel threads with no shared-state races.

### Python-style async (`loop` / `wait` / `future`)

**`wait` is the single async keyword** — one simple word for both waiting on a
future and a time delay:

- `wait <future>` (an **int**) — await that future; its result is stored in `__await_result__`
- `wait <seconds>` (a **float**) — cooperative delay

```indent
fun slow a
    give a * 2

loop:
    var f1 = future "slow" 10     # schedule on a background thread
    var f2 = future "slow" 20     # ... concurrently
    wait f1                        # await; result in __await_result__
    var r1 = __await_result__
    wait 0.05                      # cooperative delay
    var r2 = __await_result__      # r2 holds the awaited value from f1
```

### Cooperative execution (`coop`)

`coop [[fn, args], ...]` runs several async function bodies **cooperatively on
one thread** — each suspends at `wait` when its future isn't ready and lets
others run:

```indent
fun task_a
    wait 0.02
    give "A"

var res = coop [["task_a", []], ["task_b", []]]   # → [A, B], interleaved
```

| Keyword / Function | Description |
|---|---|
| `wait <future>` / `wait <seconds>` | Await a future (int) or delay (float); the unified async keyword |
| `await <future>` | Alias for waiting on a future; result in `__await_result__` |
| `async fun f ...` | Define an async function — calling it returns a future automatically |
| `loop:` | Async block; `wait` statements inside block until their future completes |
| `async with <future> as name:` | Wait a future, bind its result to `name`, run the block body |
| `future "fn" args...` | Schedule `fn(args...)` as an async future (background thread); returns a future id |
| `future_done(id)` / `future_result(id)` | Non-blocking status / result |
| `future_cancel(id)` | Best-effort cancel (removes the future id) |
| `gather(f1, f2, ...)` / `gather [f1, f2]` | Await many futures; return results in order (`asyncio.gather`) |
| `sleep(secs)` | Alias for `wait secs` (async delay) |
| `future_wait_for(id, secs)` | Wait up to `secs`; return result or `empty` on timeout (`asyncio.wait_for`) |
| `coop [[fn, args], ...]` | Run async function bodies cooperatively on one thread |

> **coop caveat:** `coop` is cooperative only at the **top level** of each task
> function — a `wait`/`await` directly inside the function body suspends and lets
> other tasks run. A `wait` nested inside an `if`/`repeat`/`loop` block still
> blocks on its thread until done (nested cooperative suspension is not yet
> implemented).

### Async I/O (🆕)

Run HTTP requests on background threads and await them — so many requests run
concurrently without blocking the program:

| Function | Returns | Description |
|---|---|---|
| `http_get_async(url, [auth])` | future id | `http_get` on a background thread |
| `http_post_json_async(url, payload, [auth])` | future id | `http_post_json` on a background thread |
| `http_put_json_async(url, payload, [auth])` | future id | `http_put_json` on a background thread |
| `http_delete_async(url, [auth])` | future id | `http_delete` on a background thread |

```indent
var f = http_get_async "https://api.example.com/users"
# ... other work ...
wait f
var resp = __await_result__      # → {status, body, ok}

# concurrent requests
var f1 = http_get_async "https://api.example.com/a"
var f2 = http_get_async "https://api.example.com/b"
var results = gather f1 f2        # both run concurrently
```

```indent
# async def — calling it returns a future automatically
async fun fetch id
    give http_get_json "https://api.example.com/" + id

loop:
    var f1 = fetch 1          # auto-future
    var f2 = fetch 2
    var results = gather f1 f2   # → [data1, data2]

# async with — await + bind + run body
loop:
    async with fetch 3 as data:
        say data
```

> Real OS threads mean this beats Python's GIL-limited asyncio for CPU-bound work.

---

## Type Checking & Conversion

| Function | Returns | Description |
|---|---|---|
| `type_of(value)` | string | Type name: `"int"`, `"string"`, `"list"`, etc. |
| `int(value)` | int | Convert to integer |
| `float(value)` | float | Convert to float |
| `string(value)` | string | Convert to string |
| `bool(value)` | boolean | Convert to boolean |
| `int_or(value, fallback)` | int | Convert to int, return fallback on failure |
| `float_or(value, fallback)` | float | Convert to float, return fallback on failure |
| `abs(n)` | number | Absolute value |
| `is_even(n)` | boolean | True if integer is even |
| `is_odd(n)` | boolean | True if integer is odd |
| `between_int(v, min, max)` | boolean | True if v is between min and max (inclusive) |
| `copy(value)` | — | Deep copy of list, dict, or string |
| `clear(value)` | — | Clear list, dict, or string (returns empty) |
| `count(container, item)` | int | Count occurrences in list or string |
| `index(container, value)` | int | First index of value in list or string (-1 if not found) |

---

## Result Type

| Function | Returns | Description |
|---|---|---|
| `ok(value)` | result | Wrap value in success result |
| `err(message)` | result | Wrap error message in failure result |
| `is_ok(result)` | boolean | True if result is success |
| `is_err(result)` | boolean | True if result is error |
| `unwrap(result)` | value | Extract value (crashes on error) |
| `unwrap(result, fallback)` | value | Extract value or return fallback on error |
| `try(expression)` | result | Evaluate expression, wrap result |

---

## Testing

| Function | Description |
|---|---|
| `assert(condition)` | Crash if condition is falsy |
| `assert(condition, message)` | Crash with message if condition is falsy |
| `assert_eq(actual, expected)` | Crash if values differ |
| `assert_eq(actual, expected, message)` | Crash with message if values differ |

---

## System Info

| Function | Returns | Description |
|---|---|---|
| `sys_version()` | string | Indent version |
| `sys_executable()` | string | Path to indent binary |
| `sys_platform()` | string | OS name (`linux`, `macos`, `windows`) |
| `sys_arch()` | string | CPU architecture (`x86_64`, `aarch64`) |
| `sys_argv()` | list | Command-line arguments |
| `os_environ()` | dict | All environment variables |

## Time & Random (extended)

| Function | Returns | Description |
|---|---|---|
| `time_perf_counter()` | float | High-resolution timer (seconds since boot) |
| `random_seed(n)` | — | Seed the random number generator |

---

## WebSocket (extended)

| Function | Returns | Description |
|---|---|---|
| `ws_recv_text_timeout(id, seconds)` | string | Receive with timeout (seconds, can be fractional) |
| `ws_close(id)` | — | Close WebSocket connection |

---

## Math (extended)

| Function | Description |
|---|---|
| `math_asin(n)` / `math_acos(n)` / `math_atan(n)` | Inverse trigonometry (returns radians) |
| `math_atan2(y, x)` | Two-argument arctangent |

---

## Regex (🆕 v1.2)

| Function | Returns | Description |
|---|---|---|
| `regex_match(pattern, text)` | boolean | True if regex pattern matches text |
| `regex_search(pattern, text)` | dict or empty | First match as `{start, end, text}` |
| `regex_findall(pattern, text)` | list | All matches as list of strings |
| `regex_replace(pattern, repl, text)` | string | Replace all regex matches |
| `regex_split(pattern, text)` | list | Split text by regex pattern |

## Datetime (🆕 v1.2)

| Function | Returns | Description |
|---|---|---|
| `time_utc()` | float | Unix timestamp (alias for `time_now`) |
| `time_format(ts, [fmt])` | string | Format timestamp (default: `"%Y-%m-%d %H:%M:%S"`) |
| `time_parse(str, [fmt])` | float | Parse datetime string to timestamp |

## Crypto & Encoding (🆕 v1.2)

| Function | Returns | Description |
|---|---|---|
| `uuid()` | string | Generate random UUID v4 |
| `base64_encode(text)` | string | Encode text to Base64 |
| `base64_decode(text)` | string | Decode Base64 text |
| `hash_sha256(text)` | string | SHA256 hex hash of text |
| `file_sha256(path)` | string | SHA256 hex hash of a file's contents |

## Path & Filesystem (🆕 v1.2)

| Function | Returns | Description |
|---|---|---|
| `glob(pattern)` | list | List files matching wildcard (e.g. `"*.ind"`) |
| `path_join(a, b, ...)` | string | Join path components |
| `path_basename(path)` | string | Extract filename from path |
| `path_dirname(path)` | string | Extract directory from path |
| `path_ext(path)` | string | File extension with dot (e.g. `".txt"`), or `""` |
| `path_stem(path)` | string | Filename without extension |
| `path_abs(path)` | string | Make the path absolute |
| `path_expand(path)` | string | Expand `~` and `~/...` to the home dir |
| `path_norm(path)` | string | Normalize `.` and `..` components |

## Functional (🆕 v1.2)

| Function | Returns | Description |
|---|---|---|
| `map(list, func_name)` | list | Apply function to each list element |
| `filter(list, func_name)` | list | Filter list by predicate function |

## String Helpers (🆕 v1.2)

| Function | Returns | Description |
|---|---|---|
| `pad_left(text, width, char)` | string | Left-pad string to given width |
| `pad_right(text, width, char)` | string | Right-pad string to given width |
| `repeat_str(text, count)` | string | Repeat string N times |

---

## Python Interop

| Function | Returns | Description |
|---|---|---|
| `python_available()` | boolean | True if Python is installed |
| `python_exec(code)` | string | Run Python code, return stdout |
| `python_eval(expr)` | string | Evaluate Python expression, return stdout |
| `python_eval_json(expr)` | any | Evaluate Python expression, return as Indent value |
| `python_run_file(path)` | string | Run Python file, return stdout |
