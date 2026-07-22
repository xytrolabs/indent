# Lesson 11 — Classes & Objects

**Time**: 30 minutes  |  **Topics**: Classes, Fields, Methods, Instantiation

---

## 🎯 Learning Objectives
- Define a class with fields and methods
- Create objects from a class
- Call methods and access fields
- Understand how Indent classes differ from Python/JS

---

## Why Classes?

Classes let you group data (fields) and behavior (methods) together. Instead of passing around separate variables, you create an **object** that bundles everything:

```indent
# Without classes — data and functions are separate
var name1 string = "Ada"
var age1 int = 28
fun greet1 n a
    say "Hello " + n + ", age " + string(a)
greet1(name1, age1)

# With classes — everything is bundled
var p dynamic = Person("Ada", 28)
p.greet()
```

---

## Defining a Class

```indent
class Rectangle
    var width float
    var height float

    fun area
        give width * height

    fun perimeter
        give 2 * (width + height)

    fun describe
        say "Rectangle " + string(width) + "x" + string(height)
```

### Rules
- `var` → constructor parameter + instance field
- `fun` → method (can access fields by name)
- No `self`/`this` keyword needed
- Fields are matched to constructor args **in order**

---

## Using Objects

```indent
var r1 dynamic = Rectangle(10.0, 5.0)
var r2 dynamic = Rectangle(3.0, 4.0)

say r1.area()           # 50.0
say r2.perimeter()      # 14.0
r1.describe()           # "Rectangle 10x5"

# Access fields directly
say r1.width            # 10.0
r1.height is 7.0        # Modify a field
say r1.area()           # 70.0 (updated)
```

---

## Methods That Modify State

```indent
class Counter
    var value int

    fun increment
        value is value + 1

    fun reset
        value is 0

var c dynamic = Counter(0)
c.increment()
c.increment()
say c.value             # 2
c.reset()
say c.value             # 0
```

---

## Real-World Example: Bank Account

```indent
class BankAccount
    var owner string
    var balance float

    fun deposit amount
        balance is balance + amount
        say owner + " deposited " + string(amount) + ". Balance: " + string(balance)

    fun withdraw amount
        if amount > balance
            say "Insufficient funds!"
        otherwise
            balance is balance - amount
            say owner + " withdrew " + string(amount) + ". Balance: " + string(balance)

var acc dynamic = BankAccount("Ada", 1000.0)
acc.deposit(500.0)
acc.withdraw(200.0)
acc.withdraw(2000.0)    # "Insufficient funds!"
```

---

## Inheritance

Indent supports single inheritance with `from`:

```indent
class Animal
    var name string
    fun speak
        say name + " makes a sound"

class Dog from Animal
    var breed string
    fun speak                  # overrides parent
        say name + " the " + breed + " barks!"

class Cat from Animal
    var color string
    fun speak
        say name + " the " + color + " cat meows!"

var d dynamic = Dog("Rex", "Beagle")
var c dynamic = Cat("Whiskers", "orange")
d.speak()     # "Rex the Beagle barks!"
c.speak()     # "Whiskers the orange cat meows!"
say d.name    # "Rex" — inherited field
```

### Inheritance Rules

- **`class Child from Parent`** — child inherits all fields and methods
- **Constructor args**: parent fields first, then child fields
  - `Dog("Rex", "Beagle")` — `"Rex"` → `name` (from Animal), `"Beagle"` → `breed` (from Dog)
- **Method override**: child's method replaces parent's — no `super` needed
- **Multi-level**: `class C from B` + `class B from A` works
- **Single inheritance only**: one parent per class

```indent
# Multi-level example
class Vehicle
    var make string
    fun start
        say "Starting " + make

class Car from Vehicle
    var model string
    fun drive
        say "Driving " + make + " " + model

class SportsCar from Car
    var top_speed int
    fun race
        say make + " " + model + " racing at " + string(top_speed) + " mph!"

var s dynamic = SportsCar("Porsche", "911", 200)
s.start()     # "Starting Porsche"  (from Vehicle)
s.drive()     # "Driving Porsche 911" (from Car)
s.race()      # "Porsche 911 racing at 200 mph!"
```

---

## Indent Classes vs Python/JS

| Feature | Python | JavaScript | Indent |
|---|---|---|---|
| Keyword | `class` | `class` | `class` |
| Constructor | `__init__` | `constructor()` | `var` fields |
| Self/this | `self` | `this` | _(none — direct access)_ |
| Fields | `self.x` | `this.x` | `x` |
| Methods | `def` | `method()` | `fun` |
| Inheritance | `class C(P)` | `class C extends P` | `class C from P` |
| Super call | `super().m()` | `super.m()` | _(auto-inherited)_ |
| Instantiation | `ClassName()` | `new ClassName()` | `ClassName()` |

Indent's approach is simpler — fields are just variables you can access by name, no `self.` prefix needed.

---

## 🎯 Exercises

### Exercise 1: Book Class
Create a `Book` class with fields `title` (string) and `pages` (int). Add a method `info` that prints `"<title> has <pages> pages"`. Create two books and call `info()` on each.

### Exercise 2: Temperature Converter
Create a `Temperature` class with field `celsius` (float). Add methods:
- `to_fahrenheit()` — returns `celsius * 9/5 + 32`
- `to_kelvin()` — returns `celsius + 273.15`
- `describe()` — prints both conversions

### Exercise 3: Todo List
Create a `TodoList` class with field `items` (list). Add methods:
- `add(task)` — appends to items
- `remove(index)` — removes by index
- `show()` — prints all items with numbers

### 🔥 Challenge: Vector Math
Create a `Vector2D` class with fields `x` and `y` (float). Add methods:
- `length()` — returns sqrt(x² + y²)
- `add(other)` — returns a new Vector2D with summed components
- `scale(factor)` — multiplies both components by factor
- `describe()` — prints `"(x, y)"`

### 🔥 Challenge: Shape Hierarchy
Create a class hierarchy for shapes:
- `Shape` with field `name` (string) and method `area()` (returns 0)
- `Circle from Shape` with field `radius` (float), override `area()` → π × r²
- `Rectangle from Shape` with fields `width` and `height` (float), override `area()` → w × h
- Create one of each, print their names and areas

---

## 🎉 You've Completed the Indent Course!

You now know everything you need to build real programs with Indent. Here's where to go next:

- 📖 [The Complete Indent Guide](../INDENT_GUIDE.md) — Full language reference
- ⚡ [Quick Reference](../quick-reference.md) — One-page cheat sheet
- 📦 [Built-in Functions](../builtins-reference.md) — Every built-in, detailed
- 🔧 Keep Indent current: `indent --update`
