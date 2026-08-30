# Indent Built-in Functions — API Reference (v1.5.0)

> Complete reference for every built-in function available in Indent 1.5.0.
> **Types**: `string`, `int`, `float`, `boolean`, `dynamic`, `empty`/`null`, `list`, `dict`, `group`
> **🆕 v1.5**: async tasks (`spawn`/`task_wait`/`parallel`), SQLite, CSV, `walk`, `os_run`, TOML, gzip/zip, typed errors, `log`, `counter`
> **🆕 v1.4**: Group type (`group([...])`), type conversion (`set varname type`), group methods
> **🆕 v1.3**: Type inference (`var x = 42`), compound assignment (`x += 5`)
>
> **Note on groups:** unique ordered collections are called **groups** — created
> with the `group` builtin: `group([1, 2, 2, 3])`. The `set` keyword is reserved
> for type conversion (`set varname type`) and does **not** build a group.

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
| `sum(list)` | float | Sum of numeric elements |
| `min(list)` | float | Minimum value |
| `max(list)` | float | Maximum value |
| `any(list)` | boolean | True if any element is truthy |
| `all(list)` | boolean | True if all elements are truthy |
| `count(list, value)` | int | Count occurrences of value |
| `enumerate(list)` | list | List of `[index, value]` pairs |
| `zip(list1, list2)` | list | List of paired elements |
| `range(end)` | list | `[0, 1, ..., end-1]` |
| `range(start, end)` | list | `[start, ..., end-1]` |
| `range(start, end, step)` | list | With custom step |

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

> Groups are unique, ordered collections. Create them with the `group` builtin.
> Methods mutate by returning a **new** group; reassign the result to keep the
> change. (The `set` keyword, by contrast, only performs type conversion.)

| Function / Method | Returns | Description |
|---|---|---|
| `group(list)` | group | Build a group, deduplicating while preserving order |
| `g.add(x)` | group | **New** group with `x` added (no-op if present) |
| `g.remove(x)` | group | **New** group without `x` |
| `g.contains(x)` | boolean | Is `x` in the group? (alias: `g.has(x)`) |
| `len(g)` | int | Number of unique elements |
| `g + g2` | group | Union of two groups |
| `is_missing(g)` | boolean | TRUE if the group is empty |

```indent
var s = group([1, 2, 2, 3])   # → {1, 2, 3}
var t = s.add(4)            # → {1, 2, 3, 4}
var u = t.remove(2)         # → {1, 3, 4}
u.contains(3)               # → TRUE
contains(u, 9)              # → FALSE
type_of(s)                  # → "group"
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

> **v2.2.0**: `type_of` returns `"function"` for function references. `string(fn)` returns the function name.

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
| `gui_show_html(html, title, width, height)` | Open HTML in native desktop window (🆕 v2.2.0) |

---

## GUI (🆕 v2.2.0)

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

### Python-style async (`loop` / `await` / `future`)

Python-flavored syntax over the same thread-based engine:

```indent
fun slow a
    time_sleep 0.05
    give a * 2

loop:
    var f1 = future "slow" 10     # schedule on a background thread
    var f2 = future "slow" 20     # ... concurrently
    await f1                       # block until done; result in __await_result__
    var r1 = __await_result__
    await f2
    var r2 = __await_result__
    say r1 + r2                    # → 60
```

| Keyword / Function | Description |
|---|---|
| `loop:` | Async block; `await` statements inside block until their future completes |
| `await <future>` | Block until the future resolves; result stored in `__await_result__` |
| `future "fn" args...` | Schedule `fn(args...)` as an async future (background thread); returns a future id |
| `future_done(id)` / `future_result(id)` | Non-blocking status / result |
| `future_cancel(id)` | Best-effort cancel (removes the future id) |

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

## Result Type (v2.5+)

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

## Path & Filesystem (🆕 v1.2)

| Function | Returns | Description |
|---|---|---|
| `glob(pattern)` | list | List files matching wildcard (e.g. `"*.ind"`) |
| `path_join(a, b, ...)` | string | Join path components |
| `path_basename(path)` | string | Extract filename from path |
| `path_dirname(path)` | string | Extract directory from path |

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
