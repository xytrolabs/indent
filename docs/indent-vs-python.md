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
| Varargs | `*args` / `**kwargs` | — | 🟡 limited |
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
| `os.walk()` | — | ⭕ no recursive walk |

---

## 8. Error Handling

| Python | Indent | Status |
|---|---|---|
| `try:` / `except:` | `do:` / `catch as e:` | ✅ |
| `finally:` | `lastly:` | ✅ |
| `raise` | `flag:` | ✅ |
| Typed exceptions | `except ValueError:` | 🟡 no type hierarchy |
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
| stdlib modules | ~200 | 13 modules + builtins | 🟡 growing |

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
| YAML/TOML/CSV | stdlib pkgs | JSON only | 🟡/⭕ |

---

## 12. Quick Decision Guide

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
