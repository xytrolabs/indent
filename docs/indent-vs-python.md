# Indent vs Python — Side-by-Side Comparison

> A practical, up-to-date comparison of **Indent** and **Python**, showing a
> Python concept and its Indent equivalent, plus a status for each feature.
>
> Legend: ✅ **Implemented & stable** · 🟡 **Partial / different shape** · ⭕ **Gap / not yet**

---

## 1. Philosophy

| | Python | Indent |
|---|---|---|
| Style | Indentation-based, `:` blocks | Indentation-based, **no colons** |
| Keywords | `def`, `return`, `elif`, `while`, `for`, `try/exept`, `class` | `fun`, `give`, `or`, `repeat`, `for`, `do/catch`, `class` |
| Assign / reassign | `=` (both) | `=` (declare), `is` (reassign) |
| Comments | `#` | `#!` |
| Goals | General purpose, huge stdlib | Simple, readable, easy to learn, scripting + games + bots |

```python
# Python
def greet(name):
    return "Hello " + name
print(greet("Ada"))
```
```indent
# Indent
fun greet name
    give "Hello " + name
say greet("Ada")
```

---

## 2. Variables & Types

| Feature | Python | Indent | Status |
|---|---|---|---|
| Dynamic typing | `x = 5` | `var x = 5` | ✅ |
| Optional type hint | `x: int = 5` | `var x int = 5` | ✅ |
| Type inference | `x = 5` → int | `var x = 5` → int | ✅ |
| Reassignment | `x = 6` | `x is 6` | ✅ |
| Concise reassign | `x += 1` | `x += 1` | ✅ |
| Type conversion | `int(x)`, `str(x)` | `set x int`, `string(x)` | ✅ |
| `None` | `None` | `empty` / `null` | ✅ |
| Constants | — | `TRUE` / `FALSE` / `YES` / `NO` | ✅ |

**Built-in types**

| Python | Indent | Notes |
|---|---|---|
| `int` | `int` | |
| `float` | `float` | |
| `str`/`bool` | `string`/`boolean` | |
| `list` | `list` | ordered, mutable (functional style) |
| `tuple` | _none_ | ⭕ no immutable tuple |
| `dict` | `dict` | |
| `set` | `group` | Indent `group` is **ordered**; Python `set` is unordered |
| `range` | `range` | |
| — | `dynamic` | untyped catch-all |

---

## 3. Control Flow

| Feature | Python | Indent | Status |
|---|---|---|---|
| If / else-if / else | `if` / `elif` / `else` | `if` / `or` / `otherwise` | ✅ |
| Ternary | `a if c else b` | `a if c else b` | ✅ |
| Chained compare | `0 < x < 10` | `0 < x < 10` | ✅ |
| Match | `match` (3.10+) | `match x:` / `case` | ✅ |
| `while` | `while cond:` | `repeat while cond` | ✅ |
| Counted loop | `for i in range(n)` | `repeat n` | ✅ |
| For-each | `for x in xs:` | `for x in xs` / `repeat x in xs` | ✅ |
| Break / continue | `break` / `continue` | `stop` / `next` | ✅ |
| Restart loop | `while True:` workaround | `reset` | ✅ |

```python
# Python
for i in range(5):
    if i == 3:
        break
    print(i)
```
```indent
# Indent
repeat 5
    if i == 3
        stop
    say i
```

---

## 4. Functions

| Feature | Python | Indent | Status |
|---|---|---|---|
| Define | `def f(x):` | `fun f x` | ✅ |
| Return | `return x` | `give x` | ✅ |
| Default args | `def f(x=1)` | `fun f x = 1` | ✅ |
| Return type | `def f() -> int` | `fun f as int` | ✅ |
| Lambdas | `lambda x: x*2` | `fn(x): x*2` | ✅ |
| Varargs | `*args` / `**kwargs` | `...args` | ✅ |
| Named args | `f(a=1)` | `f(a=1)` | ✅ |
| Pass fn as value | `map(f, xs)` | `Map(f, xs)` / `map` | ✅ |
| Call style | `f(x)` | `f(x)` or `f x` | ✅ |

---

## 5. Collections

### List operations

| Python | Indent | Status |
|---|---|---|
| `xs.append(x)` | `append(xs, x)` / `Append(xs, x)` | ✅ |
| `xs.extend(xs2)` | `extend(xs, xs2)` / `Extend` | ✅ |
| `xs.insert(i, v)` | `insert(xs, i, v)` / `Insert` | ✅ |
| `xs.pop()` | `pop(xs)` / `Pop` | ✅ |
| `xs.remove(v)` | `remove(xs, v)` / `Remove` | ✅ |
| `v in xs` | `contains(xs, v)` / `Contains` | ✅ |
| `sorted(xs)` | `sort(xs)` / `Sort` | ✅ |
| `reversed(xs)` | `reverse(xs)` / `Reverse` | ✅ |
| `xs[i:j]` | `xs[i:j]` / `Slice` | ✅ |
| `sum(xs)` / `len(xs)` | `sum(xs)` / `len(xs)` | ✅ |
| `enumerate(xs)` | `enumerate(xs)` / `Enumerate` | ✅ |
| `zip(a, b)` | `zip(a, b)` / `Zip` | ✅ |
| `map(f, xs)` | `Map(f, xs)` | ✅ |
| `filter(f, xs)` | `Filter(f, xs)` | ✅ |
| List comp | `[x*2 for x in xs]` | `[x*2 for x in xs]` | ✅ |
| Filtered comp | `[x for x in xs if c]` | `[x for x in xs if c]` | ✅ |

### Dictionary operations

| Python | Indent | Status |
|---|---|---|
| `d[k]` | `d[k]` / `d.k` | ✅ |
| `k in d` | `has_key(d, k)` / `HasKey` | ✅ |
| `d.get(k)` | `d.get(k)` / `dict_get(d, k)` | ✅ |
| `d[k] = v` | `dict_set(d, k, v)` / `DictSet` | ✅ |
| `del d[k]` | `dict_remove(d, k)` / `DictRemove` | ✅ |
| `d.keys()` | `keys(d)` / `Keys` | ✅ |
| `d.values()` | `values(d)` / `Values` | ✅ |
| `d.items()` | `items(d)` / `Items` | ✅ |
| `d.update(d2)` | `dict_update(d, d2)` / `DictUpdate` | ✅ |
| Merging | `{**a, **b}` | `a + b` (dict) | ✅ |

### Sets → Groups

| Python `set` | Indent `group` | Status |
|---|---|---|
| `{1,2,3}` | `group([1,2,3])` | ✅ |
| `s.add(x)` | `g.add(x)` | ✅ |
| `s.discard(x)` | `g.remove(x)` | ✅ |
| `x in s` | `g.contains(x)` | ✅ |
| `len(s)` | `len(g)` | ✅ |
| `s \| t` union | `g + g2` | ✅ |
| comprehension | `[x*2 for x in g]` | ✅ |

> ⚠️ **Ordered vs unordered**: Indent `group` preserves insertion order;
> Python `set` is unordered. Use `group` when order matters.

---

## 6. Strings

| Python | Indent | Status |
|---|---|---|
| `s.upper()` | `upper(s)` / `Upper(s)` / `s.upper()` | ✅ |
| `s.lower()` | `lower(s)` / `Lower(s)` | ✅ |
| `s.strip()` | `s.strip()` / `trim(s)` / `Trim(s)` | ✅ |
| `s.replace(a,b)` | `replace(s,a,b)` / `Replace` | ✅ |
| `s.split(sep)` | `split(s, sep)` / `Split` | ✅ |
| `",".join(xs)` | `join(xs, ",")` / `Join` | ✅ |
| `s.startswith(p)` | `s.starts_with(p)` / `StartsWith` | ✅ |
| `s.endswith(p)` | `s.ends_with(p)` / `EndsWith` | ✅ |
| `s.find(x)` | `find(s, x)` / `Find` | ✅ |
| `s.count(x)` | `count(s, x)` / `Count` | ✅ |
| `s.capitalize()` | `capitalize(s)` | ✅ |
| `s.title()` | `title(s)` | ✅ |
| `s.swapcase()` | `swapcase(s)` | ✅ |
| `s.ljust/rjust/center` | `PadLeft(s,w,c)` / `PadRight` | ✅ |
| `s.zfill(n)` | `PadLeft(s,n,"0")` | ✅ |
| f-string | `f"{x:.2f}"` | `format(...)` / `sformat(...)` | 🟡 |
| Regex | `re.search/m/findall/sub` | `regex_search` / `regex_findall` / `regex_replace` | ✅ |

---

## 7. File I/O & OS

| Python | Indent | Status |
|---|---|---|
| `open(f)` read | `read_file(f)` / `os_read` | ✅ |
| `open(f,"w")` write | `write_file(f, text)` | ✅ |
| append | `append_file(f, text)` | ✅ |
| `with open(f) as h:` | `open f for read as h:` | ✅ |
| `os.listdir(d)` | `os_list_dir(d)` | ✅ |
| `glob.glob(...)` | `glob(...)` | ✅ |
| `os.path.join` | `path_join(...)` | ✅ |
| `os.getcwd()` | `os_getcwd()` | ✅ |
| `os.chdir(d)` | `os_chdir(d)` | ✅ |
| `subprocess` | `os_system(cmd)` | ✅ |
| `os.env[]` | `os_getenv` / `os_setenv` | ✅ |
| `os.walk()` | `walk(path)` | ✅ |

---

## 8. Error Handling

| Python | Indent | Status |
|---|---|---|
| `try:` / `except:` | `do:` / `catch as e:` | ✅ |
| `finally:` | `lastly:` | ✅ |
| `raise` | `flag:` | ✅ |
| Typed exceptions | `except ValueError:` | `error_type(err)` / `error_message(err)` | 🟡 via builtins |
| `else` on try | — | 🟡 use flag |

```python
# Python
try:
    x = int(text)
except ValueError as e:
    x = 0
finally:
    cleanup()
```
```indent
# Indent
do:
    set x int
catch as e:
    x is 0
lastly:
    cleanup()
```

---

## 9. Classes & OOP

| Feature | Python | Indent | Status |
|---|---|---|---|
| Define | `class Foo:` | `class Foo` | ✅ |
| Constructor | `def __init__(self)` | `var` fields | ✅ |
| Receiver | `self` | _(none, direct)_ | ✅ simpler |
| Methods | `def m(self)` | `fun m` | ✅ |
| Inheritance | `class C(P)` | `class C from P` | ✅ |
| Instantiation | `Foo()` | `Foo(...)` | ✅ |
| Special methods | `__str__`, `__add__` | — | 🟡 limited |

---

## 10. Modules & Imports

| Python | Indent | Status |
|---|---|---|
| `import math` | `get math` / `import math` | ✅ |
| `from m import f` | `get f from m` | ✅ |
| Alias | `import m as n` | `get m as n` | ✅ |
| Package install | `pip install X` | `air install X` | ✅ |
| stdlib modules | ~200 | 20+ modules + builtins | 🟡 growing |

---

## 11. Advanced / Async

| Feature | Python | Indent | Status |
|---|---|---|---|
| WebSocket | `websockets` pkg | native WebSocket | ✅ |
| HTTP client | `requests` | `http_get` / `http_post_json` | ✅ |
| HTTP server | Flask/FastAPI | built-in server | ✅ |
| JSON | `json` | `json_loads` / `json_dumps` | ✅ |
| Hash | `hashlib` | `hash_*` builtins | ✅ |
| UUID | `uuid` | `uuid()` | ✅ |
| Base64 | `base64` | `base64_*` | ✅ |
| Iterators/generators | `yield` | — | ⭕ |
| Decorators | `@decorator` | — | ⭕ |
| Async/await | `async/await` | — | ⭕ |
| CSV | `csv` module | `csv_read` / `csv_write` | ✅ |
| SQLite | `sqlite3` | `sqlite_exec` / `sqlite_query` / `sqlite_query_one` | ✅ |
| YAML/TOML | stdlib pkgs | JSON only | ⭕ |
| Call Python | `ctypes` / `subprocess` | `python_eval` / `python_exec` | ✅ |

---

## 12. Memory Model & Semantics

> This is the **biggest conceptual difference** between the two languages and
> shapes how you write Indent functions.

| Aspect | Python | Indent |
|---|---|---|
| Object model | **By reference** — variables hold references; mutable objects are shared | **By value** — lists/dicts/groups are cloned when passed |
| Mutating a passed-in list | `def f(xs): xs.append(1)` changes the caller's list | a function's copy is changed; the caller is unaffected |
| Returning changed data | implicit (same object) | must `give` the new container and reassign: `xs is append(xs, 1)` |
| Aliasing | `a = b` → `a` and `b` name the **same** list | containers are copied; they stay independent |
| Dict mutation | `d[k] = v` in place | `dict_set(d, k, v)` returns a **new** dict — reassign |
| Shared module state | module globals readable/writable everywhere | module-level vars are shared across functions (use for cross-fn context) |

**Practical consequence:** in Indent, if a function changes a list/dict, you
must **return it and reassign the result** at the call site (`x is f x ...`).
This makes data flow explicit and side-effects visible, at the cost of a little
extra boilerplate. Think of functions as *transform in → give out*.

```python
# Python — mutates the caller's list
def add_one(xs):
    xs.append(1)
a = []
add_one(a)      # a is now [1]
```
```indent
# Indent — returns a new list, caller reassigns
fun add_one xs
    give append xs 1
var a = []
a is add_one(a)   # a is now [1]
```

---

## 13. Python Interop

Indent can call out to a Python installation, so you're never fully locked out
of the Python ecosystem:

| Indent side | What it does |
|---|---|
| `python_eval(expr)` | Evaluate a Python expression, return stdout as a string |
| `python_eval_json(expr)` | Evaluate, return the result as a **typed** Indent value |
| `python_exec(code)` | Run Python code, return stdout |
| `python_run_file(path)` | Run a `.py` file |

> Indent → Python is **one-way**: Indent code can use Python libraries, but
> Python cannot (yet) import Indent code. This is a pragmatic escape hatch for
> libraries Indent doesn't ship yet (e.g. heavy scientific/ML work).

---

## 14. GUI & Desktop Apps

| Capability | Python | Indent | Status |
|---|---|---|---|
| Native window | Tkinter, PyQt/PySide, Kivy | `gui_show_html(html, title, w, h)` — native GTK+WebKit window | ✅ |
| HTML UI in a window | pywebview | built-in, no browser/server needed | ✅ |
| Web server | Flask/FastAPI/Django | built-in HTTP server | ✅ |

Indent opens a **real desktop window** (via the `indent-gui` helper) that
renders HTML — no browser tab, no localhost server setup. Great for small tools
and dashboards.

---

## 15. Games (InGame vs PyGame)

Indent's `ingame` package deliberately mirrors **PyGame's API** so game ideas
port over easily:

| PyGame | Indent (InGame) |
|---|---|
| `pygame.init()` | `Init()` |
| `display.set_mode((w, h))` | `SetMode(w, h, title)` |
| `draw.rect/circle/line/polygon` | `DrawRect` / `DrawCircle` / `DrawLine` / `DrawPolygon` |
| `display.flip()` | `Flip(clear)` |
| `event.get()` | `GetEvents()` (normalized `type` field) |
| `key.get_pressed()` | `GetKeys()` / `IsKeyDown(key)` |
| `mouse.get_pos()` | `GetMouse()` |
| `time.Clock.tick(fps)` | `Tick(fps)` |

Beyond the PyGame basics, InGame adds tilemaps (`MakeTilemap`/`DrawTilemap`),
emoji sprites (`DrawSprite`), a camera (`SetCamera`), and simple physics
(`StepPhysics`/`MoveInMap`) for RPG/action games — all in pure Indent with a
native window. See `examples/snake_game.ind`, `examples/breakout_game.ind`,
and `examples/rpg_demo.ind`.

---

## 16. Ecosystem & Tooling

| Concern | Python | Indent |
|---|---|---|
| Package manager | `pip install X` | `air install X` (~50 registry packages) |
| REPL | `python` | `indent repl` |
| Tests | pytest / unittest | `testing` module + `indent test` |
| Format / lint | black, ruff | `indent fmt` / `indent lint` / `indent check` |
| Editor | mature everywhere | VS Code extension (`indent-language`), tree-sitter grammar, file icons, themes |
| Docs | huge | built-ins reference, quick reference, guides, learn series |
| Stdlib | ~200 modules | 20+ std modules + native builtins (growing) |
| Distribution | virtualenv / PyInstaller | single native binary |

> **Note:** PyPI has ~500k packages vs `air`'s ~50. For a rare third-party
> library, the fallback is the Python interop in section 13.

---

## 17. Performance & Runtime

| | Python | Indent |
|---|---|---|
| Runtime | CPython (bytecode, GIL) | native Rust runtime |
| Startup | interpreter + site-packages | small single binary, fast start |
| Concurrency | threads (GIL), asyncio | none yet (planned) |
| Typing | dynamic (hints optional) | dynamic with optional type annotations & `set` conversion |

Indent is designed for small, fast-starting tools and scripts, not CPU-bound
numeric workloads. For heavy math/ML you'd call into Python/NumPy via interop
rather than recomputing in Indent.

---

## 18. Quick Decision Guide

- **Use Indent when**: you want simple, readable scripts; learning to program;
  building games (InGame), Discord bots, or small web servers — all without
  fighting syntax.
- **Use Python when**: you need the huge stdlib, async, scientific/ML
  ecosystem (NumPy/PyTorch), or mature third-party packages.

---

## Related

- [Classes vs Python/JS](learn/11-classes.md)
- [Quick Reference](quick-reference.md)
- [Built-in Functions Reference](builtins-reference.md)
- [Competitiveness Roadmap](python-competitiveness-roadmap.md)
