# Indent Classes — Design & Implementation

> Classes bring object-oriented programming to Indent with a natural-language syntax.

---

## Syntax Design

```indent
class Person
    var name string
    var age int

    fun greet
        say "Hello, I'm " + name + ", age " + string(age)

    fun birthday
        age is age + 1
        say "Happy birthday " + name + "! Now " + string(age)

# Instantiation
var ada dynamic = Person("Ada", 28)
ada.greet()

var bob dynamic = Person("Bob", 25)
bob.birthday()
```

### Key Design Decisions
- **No `self`/`this` keyword** — fields and methods are accessed directly by name within the class body
- **Constructor = class body** — the `var` declarations define constructor parameters, executed in order
- **Fields are the constructor params** — any `var` declared in the class body becomes both a constructor parameter and an instance field
- **Methods use `fun`** — same as regular functions, but scoped to the class
- **Instantiation looks like a function call** — `ClassName(args)`
- **Method calls use dot notation** — `instance.method(args)`

---

## Implementation Plan

### 1. New AST Nodes (in `main.rs`)

```rust
#[derive(Debug, Clone)]
struct ClassField {
    name: String,
    ty: String,
    default: Option<ValueSource>,
}

#[derive(Debug, Clone)]
struct ClassDef {
    name: String,
    fields: Vec<ClassField>,
    methods: Vec<FunctionDef>,
}

// New Stmt variant:
DefClass {
    line: usize,
    name: String,
    fields: Vec<ClassField>,
    methods: Vec<Stmt>,  // DefFun statements
}
```

### 2. New Value Type

```rust
enum Value {
    // ... existing variants ...
    Object {
        class_name: String,
        fields: HashMap<String, Value>,
        methods: HashMap<String, FunctionDef>,
    },
}
```

### 3. Parser — `class` keyword

```indent
class ClassName
    var field1 type
    var field2 type = default
    fun method1 param
        body
```

Parser logic:
- `class Name` starts a class definition
- Indented block contains `var` (fields) and `fun` (methods)
- Fields become constructor params + instance fields
- Methods are stored as callable functions

### 4. Runtime — Instantiation

`ClassName(arg1, arg2)`:
1. Look up `ClassName` in the scope's class definitions
2. Match positional arguments to fields in order
3. Create an `Object` value with fields initialized
4. Return the object

### 5. Runtime — Method Dispatch

`instance.method(args)`:
1. Resolve `instance` to an `Object` value
2. Look up `method` in the object's methods
3. Create a new scope with `self` fields + method params
4. Execute the method body

---

## Code Changes Required

### File: `indent-native/src/main.rs`

**Add after `FunctionParam` (around line 160):**
```rust
#[derive(Debug, Clone)]
struct ClassField {
    name: String,
    ty: String,
}

#[derive(Debug, Clone)]
struct ClassDef {
    name: String,
    fields: Vec<ClassField>,
    methods: HashMap<String, FunctionDef>,
}
```

**Add to `Stmt` enum:**
```rust
DefClass {
    line: usize,
    name: String,
    fields: Vec<ClassField>,
    methods: Vec<Stmt>,
},
```

**Add to `Value` enum:**
```rust
Object {
    class_name: String,
    fields: HashMap<String, Value>,
    methods: HashMap<String, FunctionDef>,
},
```

**Add to `Scope` struct:**
```rust
classes: HashMap<String, ClassDef>,
```

**Parser — add to `parse_stmt` (around line 5950):**
```rust
if let Some(rest) = text.strip_prefix("class ") {
    let name = rest.trim().to_string();
    let child_indent = match self.peek() {
        Some(next) if next.indent > line.indent => next.indent,
        _ => return Err("Class body expected".to_string()),
    };

    let mut fields = Vec::new();
    let mut methods = Vec::new();

    while let Some(next) = self.peek() {
        if next.indent < child_indent { break; }
        if next.indent > child_indent {
            return Err(format!("Bad indent in class at line {}", next.line_no));
        }

        if next.text.starts_with("var ") {
            fields.push(self.parse_class_field(child_indent)?);
        } else if next.text.starts_with("fun ") {
            methods.push(self.parse_stmt(child_indent)?);
        } else {
            break;
        }
    }

    return Ok(Stmt::DefClass { line: line.line_no, name, fields, methods });
}
```

**Runtime — execute `DefClass`:**
```rust
Stmt::DefClass { name, fields, methods, .. } => {
    let mut method_map = HashMap::new();
    for m in methods {
        if let Stmt::DefFun { name: mname, params, body, .. } = m {
            method_map.insert(mname.clone(), FunctionDef {
                params: params.clone(),
                return_type: None,
                body: body.clone(),
            });
        }
    }
    let class_fields: Vec<ClassField> = fields.iter().map(|f| {
        ClassField { name: f.name.clone(), ty: f.ty.clone() }
    }).collect();
    ctx.rt.classes.insert(name.clone(), ClassDef {
        name: name.clone(),
        fields: class_fields,
        methods: method_map,
    });
    Ok(Control::None)
}
```

**Runtime — Handle `Call` for class instantiation:**
Before invoking as a regular function, check if the callee is a class name:
```rust
if let Some(class_def) = ctx.rt.classes.get(callee) {
    return instantiate_class(class_def, &positional, ctx);
}
```

**New function `instantiate_class`:**
```rust
fn instantiate_class(
    class_def: &ClassDef,
    args: &[Value],
    ctx: &mut ExecContext,
) -> Result<Value, String> {
    let mut fields = HashMap::new();
    for (i, field) in class_def.fields.iter().enumerate() {
        let value = if i < args.len() {
            args[i].clone()
        } else {
            Value::Empty
        };
        fields.insert(field.name.clone(), value);
    }
    Ok(Value::Object {
        class_name: class_def.name.clone(),
        fields,
        methods: class_def.methods.clone(),
    })
}
```

**Extend `resolve_var_chain` for method dispatch (around line 4600):**
After the module/dict chain, add an Object chain handler:
```rust
Value::Object { ref methods, .. } => {
    if let Some(method_def) = methods.get(p) {
        // Return the method for later invocation
        // Or handle inline
    }
}
```

---

## Usage After Implementation

```indent
# Define a class
class Rectangle
    var width float
    var height float

    fun area
        give width * height

    fun describe
        say "Rectangle " + string(width) + "x" + string(height)

# Use it
var r dynamic = Rectangle(10.0, 5.0)
say r.area()            # 50.0
r.describe()            # "Rectangle 10x5"
```

---

## Status: ✅ IMPLEMENTED — Classes with single inheritance are fully supported in the native runtime (`class … from Parent`, fields, methods, instantiation).
