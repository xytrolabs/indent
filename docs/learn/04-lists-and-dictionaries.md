# Lesson 04 — Lists & Dictionaries

**Time**: 25 minutes  |  **Topics**: Lists, Dictionaries, Slicing, Immutability, `any`/`all`

---

## 🎯 Learning Objectives
- Create and manipulate lists and dictionaries
- Access elements by index and key
- Slice lists and use common operations
- Understand immutability and use `is` for reassignment
- Test collections with `any`, `all`, `contains`

---

## Lists

Lists hold ordered sequences of values:

```indent
var empty list = []
var numbers list = [1, 2, 3, 4, 5]
var mixed dynamic = [1, "hello", true, 3.14]
```

> **Type tip**: Use `list` for homogeneous lists, `dynamic` for mixed content. Using `list` as the type annotation documents intent and helps catch accidental reassignment to non-list values.

### Accessing elements

Use `[index]` — zero-based. Negative indexes count from the end:

```indent
var colors list = ["red", "green", "blue"]
say colors[0]       #! "red"
say colors[1]       #! "green"
say colors[-1]      #! "blue" (last element)
say colors[-2]      #! "green"
```

### Modifying elements

```indent
var items dynamic = [10, 20, 30]
items[0] is 99
say items           #! [99, 20, 30]

#! Slice assignment
items[1:3] is [7, 8]
say items           #! [99, 7, 8]
```

### Slicing

```indent
var nums dynamic = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]

say nums[2:5]        #! [2, 3, 4]
say nums[:4]         #! [0, 1, 2, 3]
say nums[6:]         #! [6, 7, 8, 9]
say nums[::2]        #! [0, 2, 4, 6, 8]
say nums[1:8:3]      #! [1, 4, 7]
```

### List operations

```indent
var items list = [3, 1, 2]

say len items         #! 3
say min items         #! 1
say max items         #! 3
say sum items         #! 6

sort items
say items             #! [1, 2, 3]

reverse items
say items             #! [3, 2, 1]

append items 4
say items             #! [3, 2, 1, 4]

var removed dict = pop items 0
say removed.item      #! 3
say removed.list      #! [2, 1, 4]

insert items 1 99
say items             #! [2, 99, 1, 4]
```

### Checking membership

```indent
var items list = ["apple", "banana", "cherry"]

if "banana" in items
    say "Found it!"

if "grape" not in items
    say "Not in the list"
```

### Looping over lists

```indent
var fruits list = ["apple", "banana", "cherry"]

repeat fruit in fruits
    say "I like " + fruit

#! With index using enumerate
repeat pair in enumerate fruits
    var index int = pair[0]
    var value string = pair[1]
    say index + ": " + value
```

---

## Dictionaries

Dictionaries store key-value pairs:

```indent
var person dict = {"name": "Ada", "age": 28, "city": "London"}
```

### Accessing values

```indent
say person["name"]       #! "Ada"
say person["age"]        #! 28
say person.name           #! "Ada" — dot notation works too!
say person.age            #! 28
```

### Modifying dictionaries

```indent
person["age"] is 29
person.city is "Paris"    #! dot notation for assignment too
say person                #! {"name": "Ada", "age": 29, "city": "Paris"}
```

### Dictionary operations

```indent
var data dict = {"a": 1, "b": 2, "c": 3}

say keys data             #! ["a", "b", "c"]
say values data           #! [1, 2, 3]
say items data            #! [["a", 1], ["b", 2], ["c", 3]]
say has_key data "b"      #! true
say len data              #! 3
say dict_get data "b"     #! 2
say dict_get data "z"     #! empty (not found)
say dict_get data "z" 0   #! 0 (with default)

var merged dynamic = dict_set data "d" 4
say merged                #! {"a": 1, "b": 2, "c": 3, "d": 4}

var cleaned dynamic = dict_remove data "b"
say cleaned               #! {"a": 1, "c": 3}
```

### Looping over dictionaries

```indent
var scores dict = {"Alice": 95, "Bob": 87, "Charlie": 92}

repeat pair in items scores
    var name string = pair[0]
    var score int = pair[1]
    say name + " scored " + score
```

---

## Combining lists and dictionaries

```indent
var students list = [
    {"name": "Alice", "grades": [90, 85, 92]},
    {"name": "Bob", "grades": [78, 88, 84]},
    {"name": "Charlie", "grades": [95, 97, 91]}
]

repeat student in students
    var avg float = sum student.grades / len student.grades
    say student.name + "'s average: " + avg
```

---

## List/Dict Immutability

Lists and dictionaries are **immutable** — operations return **new** copies:

```indent
var xs list = [1, 2, 3]
var ys list = append xs 4    # xs is STILL [1,2,3], ys is [1,2,3,4]
var zs list = sort xs        # xs unchanged, zs is sorted

# Use `is` to reassign:
xs is append xs 4            # now xs is [1,2,3,4]
```

> ⚠️ `dynamic` variables holding lists/dicts CAN be mutated in place with index assignment: `mixed[0] is 99`. This only works with `dynamic`, not typed `list` or `dict`.

## Testing Elements: `any` & `all`

```indent
var scores list = [85, 92, 78, 95]

say any scores >= 90     # true (92 and 95 are >= 90)
say all scores >= 70     # true (all pass)

var names list = ["Alice", "", "Bob"]
say any names            # true (non-empty strings are truthy)
say all names            # false ("" is falsy)
```

## Clearing & Copying

```indent
var data list = [1, 2, 3]
var blank list = clear data    # → []

var original dict = {"a": 1}
var clone dict = copy original  # deep copy
```

## Merging Dictionaries

```indent
var base dict = {"name": "Ada", "age": 28}
var updates dict = {"age": 29, "city": "London"}
var merged dict = dict_update base updates
# → {"name": "Ada", "age": 29, "city": "London"}
```

---

## Groups (unique ordered collections)

A **group** holds unique values while keeping insertion order. It deduplicates
automatically — handy for removing duplicates or tracking "seen" items.

```indent
var s = group([1, 2, 2, 3])  # → {1, 2, 3}  (the duplicate 2 is dropped)
say len(s)                   # → 3
say type_of(s)               # → "group"
```

> 💡 Use the **`group`** builtin to build a group. The `set` keyword is reserved
> for **type conversion** (`set varname type`), not for building groups.

### Group operations

Methods return a **new** group — reassign to keep the change (just like lists
and dicts):

```indent
var s = group([1, 2, 3])
var t = s.add(4)             # → {1, 2, 3, 4}  (no-op if already present)
var u = t.remove(2)          # → {1, 3, 4}
u.contains(1)                # → TRUE
u.contains(9)                # → FALSE

# Union combines two groups
var a = group(["red", "blue"])
var b = group(["blue", "green"])
var all = a + b              # → {"red", "blue", "green"}
```

### Iterating and testing groups

```indent
var tags = group(["rust", "indent", "go"])
repeat tag in tags
    say tag                  # prints each unique tag once

is_missing(group([]))        # → TRUE (empty group)
is_missing(tags)             # → FALSE
```

### When to use a group

- Removing duplicates from a list: `var unique = group(data)`
- Membership checks: `group(data).contains(target)`
- Combining tags / IDs without duplicates.
- Converting a list in place: `set data group`.

> ⚠️ Groups are **ordered** in Indent (unlike Python's set). They keep the
> order in which elements were first inserted.

---

## List Comprehensions

A **comprehension** builds a new list from an existing one in a single line:

```indent
var nums = [1, 2, 3, 4, 5]

# Square every number
var squares = [x * x for x in nums]       # → [1, 4, 9, 16, 25]

# Filter with `if`
var evens = [x for x in nums if x % 2 == 0]  # → [2, 4]

# Transform + filter
var big = [x * 10 for x in nums if x > 2]    # → [30, 40, 50]
```

Comprehensions also work over **groups**:

```indent
var s = group([1, 2, 3])
var doubled = [x * 2 for x in s]          # → [2, 4, 6]
```

And over **dictionary entries** (grab the key/value via the item's indexes):

```indent
var scores = {"a": 1, "b": 2}
var pairs = items scores                  # → [["a", 1], ["b", 2]]
var labels = [p[0] + "=" + string(p[1]) for p in pairs]
# → ["a=1", "b=2"]
```

> 💡 A comprehension is just a compact `repeat` + `append`. Use it when the
> transformation is short; use an explicit loop when it needs many steps.

---

## Practice

1. Create a list of 10 numbers and print only the even ones using slicing.
2. Build a dictionary representing a book (title, author, year, genres as a list).
3. Write a function that takes a list of numbers and returns a new list with duplicates removed (**hint**: use `group`).
4. Create a phonebook dictionary and add functions to look up, add, and remove contacts.
5. **(Challenge)** Use a comprehension to compute the squares of all odd numbers from 1 to 20, then put the result in a group.

---

## Next Lesson

➡️ [Lesson 5: Strings & Built-in Functions](05-strings-and-builtins.md)
