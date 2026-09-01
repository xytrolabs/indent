use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::path::{Path, PathBuf};
use std::process;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tungstenite::{connect as ws_connect_blocking, Message as WsMessage};

const DEBUGGER_STOP_MSG: &str = "Execution stopped by debugger";

const INDENT_VERSION: &str = env!("CARGO_PKG_VERSION");

type WsClient = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

static WS_CLIENTS: LazyLock<Mutex<HashMap<i64, WsClient>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static WS_NEXT_ID: AtomicI64 = AtomicI64::new(1);
static RNG_STATE: AtomicI64 = AtomicI64::new(0x4d595df4d0f33173u64 as i64);
static PERF_START: LazyLock<Instant> = LazyLock::new(Instant::now);

// ── Async task store ────────────────────────────────────────
struct TaskState {
    handle: Option<std::thread::JoinHandle<()>>,
    done: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Value>>>,
}

static TASKS: LazyLock<Mutex<HashMap<i64, TaskState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static TASK_NEXT_ID: AtomicI64 = AtomicI64::new(1);

// ── Event loop (async/await) ────────────────────────────────
/// A future representing an async computation.
enum FutureState {
    Pending,
    Ready(Value),
    Cancelled,
}

struct AsyncFuture {
    state: FutureState,
    /// If Some, this is a spawned thread's JoinHandle (for thread-backed futures).
    handle: Option<std::thread::JoinHandle<Result<Value, String>>>,
}

impl AsyncFuture {
    fn new() -> Self {
        Self { state: FutureState::Pending, handle: None }
    }

    fn is_ready(&self) -> bool {
        matches!(self.state, FutureState::Ready(_))
    }

    fn is_cancelled(&self) -> bool {
        matches!(self.state, FutureState::Cancelled)
    }

    fn take_result(self) -> Option<Value> {
        match self.state {
            FutureState::Ready(v) => Some(v),
            _ => None,
        }
    }
}

/// The global event loop scheduler.
/// Stores all active futures keyed by id, plus a queue of ready tasks.
struct Scheduler {
    /// All registered futures: id → future
    futures: HashMap<i64, AsyncFuture>,
    /// Next available future id
    next_id: i64,
    /// Queue of future ids that are ready to resume (pop from front)
    ready_queue: Vec<i64>,
}

impl Scheduler {
    fn new() -> Self {
        Self {
            futures: HashMap::new(),
            next_id: 1,
            ready_queue: Vec::new(),
        }
    }

    fn register(&mut self, future: AsyncFuture) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.futures.insert(id, future);
        id
    }

    fn resolve(&mut self, id: i64, value: Value) {
        if let Some(fut) = self.futures.get_mut(&id) {
            fut.state = FutureState::Ready(value);
            self.ready_queue.push(id);
        }
    }

    fn cancel(&mut self, id: i64) {
        if let Some(fut) = self.futures.get_mut(&id) {
            fut.state = FutureState::Cancelled;
        }
    }

    fn get(&self, id: i64) -> Option<&AsyncFuture> {
        self.futures.get(&id)
    }

    fn take_next_ready(&mut self) -> Option<(i64, Value)> {
        while let Some(id) = self.ready_queue.first().cloned() {
            if let Some(fut) = self.futures.get(&id) {
                if fut.is_ready() {
                    // Remove from map and return
                    let removed = self.futures.remove(&id)?;
                    self.ready_queue.retain(|&x| x != id);
                    return Some((id, removed.take_result()?));
                } else if fut.is_cancelled() {
                    self.futures.remove(&id);
                    self.ready_queue.retain(|&x| x != id);
                    continue;
                }
            } else {
                self.ready_queue.retain(|&x| x != id);
                continue;
            }
            break; // No more ready futures
        }
        None
    }

    fn len(&self) -> usize {
        self.futures.len()
    }
}

static SCHEDULER: LazyLock<Mutex<Scheduler>> =
    LazyLock::new(|| Mutex::new(Scheduler::new()));
static SCHEDULER_NEXT_ID: AtomicI64 = AtomicI64::new(1);

#[derive(Debug, Clone)]
struct SourceLine {
    line_no: usize,
    indent: usize,
    text: String,
}

#[derive(Debug, Clone)]
enum Stmt {
    Say { line: usize, expr: String },
    DefVar {
        line: usize,
        name: String,
        ty: String,
        value: ValueSource,
    },
    Assign {
        line: usize,
        name: String,
        value: ValueSource,
    },
    AssignOp {
        line: usize,
        name: String,
        op: String,  // "+=", "-=", "*=", "/=", "%="
        value: ValueSource,
    },
    AssignIndex {
        line: usize,
        name: String,
        index_expr: String,
        value: ValueSource,
    },
    AssignSlice {
        line: usize,
        name: String,
        start_expr: Option<String>,
        end_expr: Option<String>,
        step_expr: Option<String>,
        value: ValueSource,
    },
    DefFun {
        line: usize,
        name: String,
        params: Vec<FunctionParam>,
        return_type: Option<String>,
        body: Vec<Stmt>,
        is_generator: bool,
        is_async: bool,
    },
    Give { line: usize, expr: String },
    IfChain {
        line: usize,
        branches: Vec<(Option<String>, Vec<Stmt>)>,
    },
    Match {
        line: usize,
        subject_expr: String,
        branches: Vec<(String, Vec<Stmt>)>,
        otherwise_body: Option<Vec<Stmt>>,
    },
    DoChain {
        line: usize,
        do_body: Vec<Stmt>,
        catches: Vec<(Option<String>, Vec<Stmt>)>,
        otherwise_body: Option<Vec<Stmt>>,
        lastly_body: Option<Vec<Stmt>>,
    },
    Repeat {
        line: usize,
        mode: RepeatMode,
        body: Vec<Stmt>,
    },
    Stop { line: usize },
    Next { line: usize },
    Reset { line: usize },
    Import {
        line: usize,
        module_name: String,
        symbol_name: Option<String>,
        alias: Option<String>,
    },
    Call {
        line: usize,
        callee: String,
        args: Vec<ArgItem>,
    },
    BareExpr { line: usize, expr: String },
    MakeType {
        line: usize,
        target_type: String,
        name: String,
    },
    DefClass {
        line: usize,
        name: String,
        parent: Option<String>,
        fields: Vec<ClassField>,
        methods: Vec<Stmt>,
    },
    Flag { line: usize, expr: String },
    /// Async event loop: `loop { ... }` — runs until all futures complete
    Loop {
        line: usize,
        body: Vec<Stmt>,
    },
    /// Async await: `await expr` — suspends until future resolves
    Await {
        line: usize,
        expr: String,
    },
    /// Async wait: `wait <future>` (int) awaits a future; `wait <seconds>` (float) delays.
    Wait {
        line: usize,
        expr: String,
    },
    /// Async context: `async with <future> as name:` — awaits, binds, runs body
    AsyncWith {
        line: usize,
        expr: String,
        binding: String,
        body: Vec<Stmt>,
    },
    Yield { line: usize, expr: String },
    Decorator {
        line: usize,
        name: String,
        args: Vec<ArgItem>,
        target: Box<Stmt>,
    },
    Open {
        line: usize,
        mode: String,
        path_expr: String,
        binding: Option<String>,
        body: Vec<Stmt>,
    },
}

#[derive(Debug, Clone)]
enum RepeatMode {
    Infinite,
    Count(String),
    ForEach(String),
    ForIn { item_name: String, iterable_expr: String },
    While(String),
    Until(String),
}

#[derive(Debug, Clone)]
enum ValueSource {
    Expr(String),
    Call { callee: String, args: Vec<ArgItem> },
}

#[derive(Debug, Clone)]
enum SubscriptAssignTarget {
    Index {
        name: String,
        index_expr: String,
    },
    Slice {
        name: String,
        start_expr: Option<String>,
        end_expr: Option<String>,
        step_expr: Option<String>,
    },
}

#[derive(Debug, Clone)]
enum ArgItem {
    Positional(String),
    Named { name: String, expr: String },
    DefVar(Stmt),
}

#[derive(Debug, Clone)]
struct FunctionParam {
    name: String,
    ty: Option<String>,
    default_value: Option<String>,
    is_varargs: bool,
}

#[derive(Debug, Clone)]
struct FunctionDef {
    params: Vec<FunctionParam>,
    return_type: Option<String>,
    body: Vec<Stmt>,
    is_generator: bool,
}

// ---- Classes (OOP) ----
#[derive(Debug, Clone)]
struct ClassField {
    name: String,
    _ty: String,
}

#[derive(Debug, Clone)]
struct ClassDef {
    name: String,
    parent: Option<String>,
    fields: Vec<ClassField>,
    methods: HashMap<String, FunctionDef>,
}

#[derive(Debug, Clone)]
enum Callable {
    Local(FunctionDef),
    External { module: Arc<ModuleInstance>, name: String },
    Builtin(String),
}

#[derive(Debug, Clone)]
struct ModuleInstance {
    vars: HashMap<String, Value>,
    funcs: HashMap<String, FunctionDef>,
    // Functions this module imported from other modules (via `get X from Y`).
    // Preserved so a module's functions can call its own imports.
    callables: HashMap<String, Callable>,
}

#[derive(Debug, Clone)]
enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    List(Vec<Value>),
    Set(Vec<Value>),  // ordered set — unique values, insertion order
    Dict(HashMap<String, Value>),
    Object {
        class_name: String,
        fields: HashMap<String, Value>,
        methods: HashMap<String, FunctionDef>,
    },
    Func(String),
    Module(Arc<ModuleInstance>),
    Empty,
}

fn default_color_vars() -> HashMap<String, Value> {
    let palette = [
        ("RED", "#ff4d4d"),
        ("GREEN", "#32c671"),
        ("BLUE", "#3b82f6"),
        ("CYAN", "#06b6d4"),
        ("MAGENTA", "#d946ef"),
        ("YELLOW", "#facc15"),
        ("ORANGE", "#fb923c"),
        ("PURPLE", "#8b5cf6"),
        ("PINK", "#ec4899"),
        ("BLACK", "#0f172a"),
        ("WHITE", "#f8fafc"),
    ];

    let mut out = HashMap::new();
    for (name, hex) in palette {
        out.insert(name.to_string(), Value::Str(hex.to_string()));
    }
    out
}

fn is_valid_color_literal(value: &str) -> bool {
    let s = value.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let len_ok = matches!(hex.len(), 3 | 4 | 6 | 8);
        return len_ok && hex.chars().all(|c| c.is_ascii_hexdigit());
    }

    let lower = s.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "red"
            | "green"
            | "blue"
            | "cyan"
            | "magenta"
            | "yellow"
            | "orange"
            | "purple"
            | "pink"
            | "black"
            | "white"
    )
}

/// Resolve a color (hex `#RGB`/`#RRGGBB`/`#RRGGBBAA`, with or without `#`, or a
/// named color like `RED`) into `(r, g, b)` for ANSI truecolor output.
fn color_to_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();
    let hex = if let Some(h) = s.strip_prefix('#') {
        h.to_string()
    } else {
        let named = match s.to_ascii_lowercase().as_str() {
            "red" => "#ff4d4d",
            "green" => "#32c671",
            "blue" => "#3b82f6",
            "cyan" => "#06b6d4",
            "magenta" => "#d946ef",
            "yellow" => "#facc15",
            "orange" => "#fb923c",
            "purple" => "#8b5cf6",
            "pink" => "#ec4899",
            "black" => "#0f172a",
            "white" => "#f8fafc",
            _ => return None,
        };
        named.trim_start_matches('#').to_string()
    };
    let h = hex.trim_start_matches('#');
    match h.len() {
        // #RGB
        3 => {
            let r = u8::from_str_radix(&h[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&h[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&h[2..3].repeat(2), 16).ok()?;
            Some((r, g, b))
        }
        // #RGBA
        4 => {
            let r = u8::from_str_radix(&h[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&h[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&h[2..3].repeat(2), 16).ok()?;
            Some((r, g, b))
        }
        // #RRGGBB
        6 => {
            let r = u8::from_str_radix(&h[0..2], 16).ok()?;
            let g = u8::from_str_radix(&h[2..4], 16).ok()?;
            let b = u8::from_str_radix(&h[4..6], 16).ok()?;
            Some((r, g, b))
        }
        // #RRGGBBAA (alpha ignored)
        8 => {
            let r = u8::from_str_radix(&h[0..2], 16).ok()?;
            let g = u8::from_str_radix(&h[2..4], 16).ok()?;
            let b = u8::from_str_radix(&h[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

/// Wrap `text` in ANSI truecolor escape codes for foreground `color`.
fn colorize_text(text: &str, color: &str) -> Result<String, String> {
    match color_to_rgb(color) {
        Some((r, g, b)) => Ok(format!("\x1b[38;2;{};{};{}m{}\x1b[0m", r, g, b, text)),
        None => Err(format!("colored: unknown color '{}'", color)),
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(v) => write!(f, "{v}"),
            Value::Float(v) => {
                if (v.fract()).abs() < f64::EPSILON {
                    write!(f, "{:.1}", v)
                } else {
                    write!(f, "{v}")
                }
            }
            Value::Bool(v) => write!(f, "{}", if *v { "TRUE" } else { "FALSE" }),
            Value::Str(v) => write!(f, "{v}"),
            Value::List(items) => {
                let joined = items
                    .iter()
                    .map(|x| format!("{x}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "[{joined}]")
            }
            Value::Set(items) => {
                let joined = items
                    .iter()
                    .map(|x| format!("{x}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{{{joined}}}")
            }
            Value::Dict(items) => {
                let mut keys = items.keys().cloned().collect::<Vec<_>>();
                keys.sort();
                let joined = keys
                    .iter()
                    .map(|k| {
                        let v = items.get(k).map_or(&Value::Empty, |v| v);
                        format!("\"{}\": {}", k, v)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{{{joined}}}")
            }
            Value::Object { class_name, fields, .. } => {
                let mut keys: Vec<&String> = fields.keys().collect();
                keys.sort();
                let joined = keys
                    .iter()
                    .map(|k| {
                        let v = fields.get(*k).map_or(&Value::Empty, |v| v);
                        format!("{}: {}", k, v)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{}({})", class_name, joined)
            }
            Value::Func(name) => write!(f, "{name}"),
            Value::Module(_) => write!(f, "<module>"),
            Value::Empty => write!(f, "empty"),
        }
    }
}

impl Value {
    fn to_bool(&self) -> bool {
        match self {
            Value::Bool(v) => *v,
            Value::Int(v) => *v != 0,
            Value::Float(v) => *v != 0.0,
            Value::Str(v) => !v.is_empty(),
            Value::List(v) => !v.is_empty(),
            Value::Set(v) => !v.is_empty(),
            Value::Dict(v) => !v.is_empty(),
            Value::Func(_) => true,
            Value::Module(_) => true,
            Value::Object { .. } => true,
            Value::Empty => false,
        }
    }

    fn as_number(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Float(v) => Some(*v),
            Value::Str(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::Str(_) => "string",
            Value::List(_) => "list",
            Value::Set(_) => "group",
            Value::Dict(_) => "dict",
            Value::Func(_) => "function",
            Value::Module(_) => "module",
            Value::Object { .. } => "object",
            Value::Empty => "empty",
        }
    }
}

// Instantiate a class: create an Object with fields initialized from positional args.
// If the class has a parent, parent fields come first, then child fields.
fn instantiate_class(class_def: &ClassDef, args: &[Value], classes: &HashMap<String, ClassDef>) -> Value {
    // Collect all fields: parent fields first, then child fields
    let mut all_fields: Vec<ClassField> = Vec::new();
    if let Some(ref parent_name) = class_def.parent {
        if let Some(parent_def) = classes.get(parent_name) {
            // Recursively collect parent fields (grandparent, etc.)
            let parent_fields = collect_all_fields(parent_def, classes);
            all_fields.extend(parent_fields);
        }
    }
    all_fields.extend(class_def.fields.clone());

    let mut fields = HashMap::new();
    for (i, field) in all_fields.iter().enumerate() {
        let value = if i < args.len() {
            args[i].clone()
        } else {
            Value::Empty
        };
        fields.insert(field.name.clone(), value);
    }

    // Merge methods: parent methods first, child methods override
    let mut methods = HashMap::new();
    if let Some(ref parent_name) = class_def.parent {
        if let Some(parent_def) = classes.get(parent_name) {
            methods.extend(collect_all_methods(parent_def, classes));
        }
    }
    methods.extend(class_def.methods.clone());

    Value::Object {
        class_name: class_def.name.clone(),
        fields,
        methods,
    }
}

/// Recursively collect all fields from a class and its ancestors (parent first)
fn collect_all_fields(class_def: &ClassDef, classes: &HashMap<String, ClassDef>) -> Vec<ClassField> {
    let mut fields = Vec::new();
    if let Some(ref parent_name) = class_def.parent {
        if let Some(parent_def) = classes.get(parent_name) {
            fields.extend(collect_all_fields(parent_def, classes));
        }
    }
    fields.extend(class_def.fields.clone());
    fields
}

/// Recursively collect all methods from a class and its ancestors (parent first, child overrides)
fn collect_all_methods(class_def: &ClassDef, classes: &HashMap<String, ClassDef>) -> HashMap<String, FunctionDef> {
    let mut methods = HashMap::new();
    if let Some(ref parent_name) = class_def.parent {
        if let Some(parent_def) = classes.get(parent_name) {
            methods.extend(collect_all_methods(parent_def, classes));
        }
    }
    methods.extend(class_def.methods.clone());
    methods
}

fn is_missing_value(value: &Value) -> bool {
    match value {
        Value::Empty => true,
        Value::Str(v) => v.is_empty(),
        Value::List(v) => v.is_empty(),
        Value::Set(v) => v.is_empty(),
        Value::Dict(v) => v.is_empty(),
        _ => false,
    }
}

/// Interpolate %varname% placeholders in a string with values from the execution context.
fn interpolate_string(s: &str, ctx: &ExecContext<'_>) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            // Find closing %
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'%' {
                j += 1;
            }
            if j < bytes.len() && j > start {
                let var_name = &s[start..j];
                if let Some(val) = ctx.get_var(var_name) {
                    out.push_str(&val.to_string());
                } else {
                    // Keep literal %varname% if var not found
                    out.push('%');
                    out.push_str(var_name);
                    out.push('%');
                }
                i = j + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn to_i64_value(value: &Value) -> Result<i64, String> {
    match value {
        Value::Int(v) => Ok(*v),
        Value::Float(v) => Ok(*v as i64),
        Value::Bool(v) => Ok(if *v { 1 } else { 0 }),
        Value::Str(s) => match s.trim().parse::<i64>() {
            Ok(v) => Ok(v),
            Err(_) => Err(format!("Cannot convert string \"{}\" to int", s)),
        },
        _ => Err(format!("Cannot convert {} to int", value.type_name())),
    }
}

fn to_f64_value(value: &Value) -> Result<f64, String> {
    match value {
        Value::Int(v) => Ok(*v as f64),
        Value::Float(v) => Ok(*v),
        Value::Bool(v) => Ok(if *v { 1.0 } else { 0.0 }),
        Value::Str(s) => match s.trim().parse::<f64>() {
            Ok(v) => Ok(v),
            Err(_) => Err(format!("Cannot convert string \"{}\" to float", s)),
        },
        _ => Err(format!("Cannot convert {} to float", value.type_name())),
    }
}

struct Runtime {
    module_dir: PathBuf,
    vars: HashMap<String, Value>,
    funcs: HashMap<String, FunctionDef>,
    callables: HashMap<String, Callable>,
    modules: HashMap<String, Arc<ModuleInstance>>,
    classes: HashMap<String, ClassDef>,
    module_cache: Arc<Mutex<HashMap<PathBuf, Arc<ModuleInstance>>>>,
    debugger: Option<Debugger>,
    async_funcs: HashSet<String>,
}

struct Debugger {
    stepping: bool,
    breakpoints: HashSet<usize>,
    source_lines: Vec<String>,
}

impl Runtime {
    fn new(module_dir: PathBuf) -> Self {
        Self {
            module_dir,
            vars: default_color_vars(),
            funcs: HashMap::new(),
            callables: HashMap::new(),
            modules: HashMap::new(),
            classes: HashMap::new(),
            module_cache: Arc::new(Mutex::new(HashMap::new())),
            debugger: None,
            async_funcs: HashSet::new(),
        }
    }

    fn with_cache(module_dir: PathBuf, cache: Arc<Mutex<HashMap<PathBuf, Arc<ModuleInstance>>>>) -> Self {
        Self {
            module_dir,
            vars: default_color_vars(),
            funcs: HashMap::new(),
            callables: HashMap::new(),
            modules: HashMap::new(),
            classes: HashMap::new(),
            module_cache: cache,
            debugger: None,
            async_funcs: HashSet::new(),
        }
    }

    fn enable_debugger(&mut self, file: &Path, breakpoints: HashSet<usize>) -> Result<(), String> {
        let source = fs::read_to_string(file)
            .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;
        let source_lines = source.lines().map(|s| s.to_string()).collect::<Vec<_>>();
        self.debugger = Some(Debugger {
            stepping: false,
            breakpoints,
            source_lines,
        });
        Ok(())
    }

    fn run_file(&mut self, file: &Path) -> Result<(), String> {
        let source = fs::read_to_string(file)
            .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;
        self.module_dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        self.run_source(&source)
    }

    fn run_source(&mut self, source: &str) -> Result<(), String> {
        let lines = preprocess(source)
            .map_err(|e| format_error_with_source(source, &e))?;
        let mut parser = Parser::new(lines);
        let program = parser
            .parse()
            .map_err(|e| format_error_with_source(source, &e))?;
        let mut ctx = ExecContext::new(self);
        exec_block(&program, &mut ctx)
            .map(|_| ())
            .map_err(|e| format_error_with_source(source, &e))
    }
}

fn extract_line_number(err: &str) -> Option<usize> {
    let lower = err.to_lowercase();
    let needle = "line ";
    let idx = lower.find(needle)? + needle.len();
    let rest = &err[idx..];
    let digits = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<usize>().ok()
}

fn format_error_with_source(source: &str, err: &str) -> String {
    let line_no = extract_line_number(err);
    let lines: Vec<&str> = source.lines().collect();

    // Determine error code — ordered from most specific to least
    let code = if err.contains("division by zero") {
        "E007" // division by zero
    } else if err.contains("index out of range") || err.contains("index out of bounds") {
        "E008" // index out of range
    } else if err.contains("key not found") {
        "E009" // key not found
    } else if err.contains("file not found") || err.contains("no such file") || err.contains("Cannot open") {
        "E010" // file not found
    } else if err.contains("JSON") || err.contains("json") || err.contains("Invalid JSON") {
        "E011" // JSON parse error
    } else if err.contains("connection") || err.contains("network") || err.contains("timeout") || err.contains("refused") {
        "E012" // network error
    } else if err.contains("expects") || err.contains("Cannot convert") || err.contains("Cannot add") || err.contains("Cannot subtract") || err.contains("Cannot multiply") {
        "E001" // type mismatch
    } else if err.contains("no function") || err.contains("has no function") || err.contains("is not callable") {
        "E002" // undefined function
    } else if err.contains("import") || err.contains("Cannot import") || err.contains("Cannot load") {
        "E003" // import error
    } else if err.contains("syntax") || err.contains("expected") || err.contains("unexpected") || err.contains("Unexpected indentation") {
        "E004" // syntax error
    } else if err.contains("unwrap") {
        "E005" // unwrap on err
    } else if err.contains("variable") || err.contains("undefined") || err.contains("not defined") {
        "E006" // undefined variable
    } else {
        "E000" // generic error
    };

    let mut out = format!("\x1b[1;31merror[{code}]\x1b[0m: {err}\n");

    if let Some(ln) = line_no {
        if ln > 0 && ln <= lines.len() {
            let source_line = lines[ln - 1];
            let trimmed = source_line.trim_start();
            let indent = source_line.len() - trimmed.len();

            // Show the source line
            out.push_str(&format!("  \x1b[1m{}\x1b[0m {}\n", arrow_style_right(ln, lines.len()), lines[ln - 1]));

            // Underline with carets — point to the approximate error position
            let caret_pos = if trimmed.is_empty() { 0 } else { trimmed.len() / 2 };
            let spaces = " ".repeat(indent + caret_pos + 2); // +2 for "→ "
            out.push_str(&format!("  {} \x1b[1;31m{}^\x1b[0m\n", arrow_style_empty(), spaces));

            // Show surrounding context
            let start = ln.saturating_sub(2).max(1);
            let end = (ln + 1).min(lines.len());
            if start < ln {
                out.push_str(&format!("  {} \x1b[90m(see lines {}-{})\x1b[0m\n", arrow_style_empty(), start, end));
            }
        }
    }

    // Add helpful note if we recognize the error
    if err.contains("expects a list") || err.contains("expects first argument to be list") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m use [...] to create a list, or check the variable you passed\n", arrow_style_empty()));
    } else if err.contains("expects a dictionary") || err.contains("expects first argument to be dictionary") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m use {{...}} to create a dictionary, or check the variable you passed\n", arrow_style_empty()));
    } else if err.contains("expects") && (err.contains("string") || err.contains("list") || err.contains("int")) {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m check the type of the value — use type_of() to inspect it\n", arrow_style_empty()));
    } else if err.contains("expects") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m check the type or number of arguments you are passing\n", arrow_style_empty()));
    }
    if err.contains("no function") || err.contains("has no function") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m available functions: say, ask, len, range, split, join, type_of, int, string\n", arrow_style_empty()));
    }
    if err.contains("Expected a number but got") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m use int() or float() to convert, or ask(\"int\", \"prompt\") for numeric input\n", arrow_style_empty()));
    }
    if err.contains("Cannot add") && err.contains("with '+'") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m '+' works for number+number, string+anything, or list+list / dict+dict\n", arrow_style_empty()));
    }
    if err.contains("ask(") && (err.contains("Expected") || err.contains("syntax")) {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m ask with a type needs parentheses: ask(\"int\", \"How old? \")\n", arrow_style_empty()));
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m or for plain string input: var name string = ask \"Prompt? \"\n", arrow_style_empty()));
    }
    if err.contains("not defined") || err.contains("Undefined") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m did you forget to declare this variable with 'var'? Or is there a typo?\n", arrow_style_empty()));
    }
    if err.contains("division by zero") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m check that the divisor is not zero before dividing. Use: if divisor != 0 then ...\n", arrow_style_empty()));
    }
    if err.contains("index out of range") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m list indices start at 0. Use len() to check the list size before indexing.\n", arrow_style_empty()));
    }
    if err.contains("key not found") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m check if the key exists with has_key(dict, key) before accessing it.\n", arrow_style_empty()));
    }
    if err.contains("file not found") || err.contains("no such file") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m check the file path. Use os_exists(path) to verify a file exists before reading.\n", arrow_style_empty()));
    }
    if err.contains("JSON") || err.contains("json") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m check that your JSON string is valid. Use a JSON validator or check for missing quotes/commas.\n", arrow_style_empty()));
    }
    if err.contains("connection") || err.contains("refused") || err.contains("timeout") {
        out.push_str(&format!("  {} \x1b[1;36mhelp:\x1b[0m check the URL and your network connection. The server may be down or the address may be wrong.\n", arrow_style_empty()));
    }

    out
}

fn arrow_style_right(line: usize, total: usize) -> String {
    let digits = total.to_string().len();
    format!("\x1b[1;34m-->\x1b[0m {:>width$} |", line, width = digits)
}

fn arrow_style_empty() -> String {
    "   |".to_string()
}

struct ExecContext<'a> {
    rt: &'a mut Runtime,
    frames: Vec<HashMap<String, Value>>,
}

impl<'a> ExecContext<'a> {
    fn new(rt: &'a mut Runtime) -> Self {
        Self { rt, frames: vec![] }
    }

    fn push_frame(&mut self) {
        self.frames.push(HashMap::new());
    }

    fn pop_frame(&mut self) {
        let _ = self.frames.pop();
    }

    fn define_local(&mut self, name: &str, value: Value) {
        if let Some(frame) = self.frames.last_mut() {
            frame.insert(name.to_string(), value);
        } else {
            self.rt.vars.insert(name.to_string(), value);
        }
    }

    fn set_var(&mut self, name: &str, value: Value) {
        for frame in self.frames.iter_mut().rev() {
            if frame.contains_key(name) {
                frame.insert(name.to_string(), value);
                return;
            }
        }
        self.rt.vars.insert(name.to_string(), value);
    }

    fn get_var(&self, name: &str) -> Option<Value> {
        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.get(name) {
                return Some(v.clone());
            }
        }
        self.rt.vars.get(name).cloned()
    }

    fn get_module(&self, name: &str) -> Option<Arc<ModuleInstance>> {
        self.rt.modules.get(name).cloned()
    }
}

#[derive(Debug)]
enum Control {
    None,
    Return(Value),
    Yield(Value),
    Stop,
    Next,
    Reset,
}

fn exec_block(body: &[Stmt], ctx: &mut ExecContext<'_>) -> Result<Control, String> {
    for stmt in body {
        let control = exec_stmt(stmt, ctx)?;
        match control {
            Control::None => {}
            Control::Yield(v) => {
                // In a generator context, accumulate and continue
                // For now, propagate yield up to the function caller
                return Ok(Control::Yield(v));
            }
            _ => return Ok(control),
        }
    }
    Ok(Control::None)
}

fn eval_value_source(value: &ValueSource, ctx: &mut ExecContext<'_>) -> Result<Value, String> {
    match value {
        ValueSource::Expr(expr) => eval_expr(expr, ctx),
        ValueSource::Call { callee, args } => {
            if args.is_empty() {
                // Zero-arg call: try callable first, fall back to variable lookup
                if let Ok(v) = exec_call(callee, args, ctx) {
                    return Ok(v);
                }
                eval_expr(callee, ctx)
            } else {
                exec_call(callee, args, ctx)
            }
        }
    }
}

fn exec_stmt(stmt: &Stmt, ctx: &mut ExecContext<'_>) -> Result<Control, String> {
    maybe_debug(stmt_line(stmt), ctx)?;

    match stmt {
        Stmt::Say { expr, line } => {
            let v = eval_expr(expr, ctx).map_err(|e| format!("Line {}: {}", line, e))?;
            let interpolated = match v {
                Value::Str(ref s) => Value::Str(interpolate_string(s, ctx)),
                _ => v,
            };
            println!("{interpolated}");
            Ok(Control::None)
        }
        Stmt::DefVar {
            line,
            name,
            ty,
            value,
        } => {
            let v = eval_value_source(value, ctx)?;
            let coerced = coerce_type(*line, name, ty, v)?;
            ctx.define_local(name, coerced);
            Ok(Control::None)
        }
        Stmt::MakeType {
            line,
            target_type,
            name,
        } => {
            let value = ctx
                .get_var(name)
                .ok_or_else(|| format!("Line {}: variable '{}' is not defined", line, name))?;
            let converted = convert_type(*line, name, target_type, value)?;
            ctx.set_var(name, converted);
            Ok(Control::None)
        }
        Stmt::DefClass {
            name,
            parent,
            fields,
            methods,
            ..
        } => {
            let mut method_map = HashMap::new();
            for m in methods {
                if let Stmt::DefFun {
                    name: mname,
                    params,
                    body,
                    ..
                } = m
                {
                    method_map.insert(
                        mname.clone(),
                        FunctionDef {
                            params: params.clone(),
                            return_type: None,
                            body: body.clone(),
                            is_generator: false,
                        },
                    );
                }
            }
            ctx.rt.classes.insert(
                name.clone(),
                ClassDef {
                    name: name.clone(),
                    parent: parent.clone(),
                    fields: fields.clone(),
                    methods: method_map,
                },
            );
            Ok(Control::None)
        }
        Stmt::Assign { name, value, .. } => {
            let v = eval_value_source(value, ctx)?;
            assign_var_chain(name, v, ctx);
            Ok(Control::None)
        }
        Stmt::AssignOp { line, name, op, value } => {
            let rhs = eval_value_source(value, ctx)?;
            let lhs = ctx.get_var(name)
                .ok_or_else(|| format!("Line {}: variable '{}' is not defined", line, name))?;
            let result = eval_binary(&op[..op.len()-1], lhs, rhs)
                .map_err(|e| format!("Line {}: {}", line, e))?;
            assign_var_chain(name, result, ctx);
            Ok(Control::None)
        }
        Stmt::AssignIndex {
            line,
            name,
            index_expr,
            value,
        } => {
            let replacement = eval_value_source(value, ctx)?;
            let target = ctx
                .get_var(name)
                .ok_or_else(|| format!("Line {}: variable '{}' is not defined", line, name))?;
            let index = eval_expr(index_expr, ctx)?;
            let updated = assign_index_value(target, index, replacement)?;
            ctx.set_var(name, updated);
            Ok(Control::None)
        }
        Stmt::AssignSlice {
            line,
            name,
            start_expr,
            end_expr,
            step_expr,
            value,
        } => {
            let replacement = eval_value_source(value, ctx)?;
            let target = ctx
                .get_var(name)
                .ok_or_else(|| format!("Line {}: variable '{}' is not defined", line, name))?;

            let start = if let Some(expr) = start_expr {
                let parsed = eval_expr(expr, ctx)?;
                parse_optional_slice_bound(&parsed, "slice start")?
            } else {
                None
            };
            let end = if let Some(expr) = end_expr {
                let parsed = eval_expr(expr, ctx)?;
                parse_optional_slice_bound(&parsed, "slice end")?
            } else {
                None
            };
            let step = if let Some(expr) = step_expr {
                let parsed = eval_expr(expr, ctx)?;
                parse_slice_step_value(&parsed)?
            } else {
                1
            };

            let updated = assign_slice_value(target, start, end, step, replacement)?;
            ctx.set_var(name, updated);
            Ok(Control::None)
        }
        Stmt::DefFun {
            name,
            params,
            return_type,
            body,
            is_generator,
            is_async,
            ..
        } => {
            let f = FunctionDef {
                params: params.clone(),
                return_type: return_type.clone(),
                body: body.clone(),
                is_generator: *is_generator,
            };
            ctx.rt.funcs.insert(name.clone(), f.clone());
            ctx.rt.callables.insert(name.clone(), Callable::Local(f));
            if *is_async {
                ctx.rt.async_funcs.insert(name.clone());
            }
            Ok(Control::None)
        }
        Stmt::Give { expr, line } => {
            let v = eval_expr(expr, ctx).map_err(|e| format!("Line {}: {}", line, e))?;
            Ok(Control::Return(v))
        }
        Stmt::IfChain { branches, line } => {
            for (cond, body) in branches {
                let is_true = match cond {
                    None => true,  // otherwise branch
                    Some(c) => eval_expr(c, ctx)
                        .map_err(|e| format!("Line {}: {}", line, e))?
                        .to_bool(),
                };
                if is_true {
                    return exec_block(body, ctx);
                }
            }
            Ok(Control::None)
        }
        Stmt::Match {
            subject_expr,
            branches,
            otherwise_body,
            line,
        } => {
            let subject = eval_expr(subject_expr, ctx).map_err(|e| format!("Line {}: {}", line, e))?;
            for (case_expr, body) in branches {
                let case_val = eval_expr(case_expr, ctx)?;
                if eq_values(&subject, &case_val) {
                    return exec_block(body, ctx);
                }
            }
            if let Some(body) = otherwise_body {
                exec_block(body, ctx)
            } else {
                Ok(Control::None)
            }
        }
        Stmt::DoChain {
            do_body,
            catches,
            otherwise_body,
            lastly_body,
            ..
        } => {
            let mut flow = Control::None;
            let mut pending_error: Option<String> = None;

            match exec_block(do_body, ctx) {
                Ok(control) => {
                    flow = control;
                }
                Err(err) => {
                    pending_error = Some(err);
                }
            }

            if let Some(err_text) = pending_error.clone() {
                if !catches.is_empty() {
                    let (binding, body) = &catches[0];
                    ctx.push_frame();
                    if let Some(name) = binding {
                        ctx.define_local(name, Value::Str(err_text));
                    }
                    let catch_result = exec_block(body, ctx);
                    ctx.pop_frame();
                    match catch_result {
                        Ok(control) => {
                            flow = control;
                            pending_error = None;
                        }
                        Err(err) => {
                            pending_error = Some(err);
                        }
                    }
                }
            } else if let Some(otherwise) = otherwise_body {
                match exec_block(otherwise, ctx) {
                    Ok(control) => flow = control,
                    Err(err) => pending_error = Some(err),
                }
            }

            if let Some(lastly) = lastly_body {
                match exec_block(lastly, ctx) {
                    Ok(Control::None) => {}
                    Ok(control) => {
                        flow = control;
                        pending_error = None;
                    }
                    Err(err) => pending_error = Some(err),
                }
            }

            if let Some(err) = pending_error {
                Err(err)
            } else {
                Ok(flow)
            }
        }
        Stmt::Repeat { mode, body, .. } => exec_repeat(mode, body, ctx),
        Stmt::Stop { .. } => Ok(Control::Stop),
        Stmt::Next { .. } => Ok(Control::Next),
        Stmt::Reset { .. } => Ok(Control::Reset),
        Stmt::Import {
            module_name,
            symbol_name,
            alias,
            line,
        } => {
            import_stmt(module_name, symbol_name.as_deref(), alias.as_deref(), ctx)
                .map_err(|e| format!("Line {}: {}", line, e))?;
            Ok(Control::None)
        }
        Stmt::Call { callee, args, line } => {
            let _ = exec_call(callee, args, ctx).map_err(|e| format!("Line {}: {}", line, e))?;
            Ok(Control::None)
        }
        Stmt::BareExpr { expr, line } => {
            let _ = eval_expr(expr, ctx).map_err(|e| format!("Line {}: {}", line, e))?;
            Ok(Control::None)
        }
        Stmt::Flag { line, expr } => {
            let value = eval_expr(expr, ctx)?;
            Err(format!("Line {}: {}", line, value))
        }
        Stmt::Loop { body, .. } => run_event_loop(body, ctx),
        Stmt::Await { expr, .. } => {
            // Evaluate expr to a future id, then block until it resolves.
            let future_val = eval_expr(expr, ctx)?;
            let future_id = future_to_id(&future_val)?;
            let result = task_wait_builtin(future_id)?;
            // Expose the awaited result so it can be captured if needed.
            ctx.define_local("__await_result__", result);
            Ok(Control::None)
        }
        Stmt::Wait { expr, .. } => {
            // wait <future> (int) awaits a future; wait <seconds> (float) delays.
            let val = eval_expr(expr, ctx)?;
            match val {
                Value::Int(id) => {
                    let result = task_wait_builtin(id)?;
                    ctx.define_local("__await_result__", result);
                }
                Value::Float(secs) => {
                    let fut = sleep_builtin(secs)?;
                    let _ = task_wait_builtin(future_to_id(&fut)?)?;
                }
                _ => return Err(format!("wait expects a future id (int) or seconds (float), got {}", val.type_name())),
            }
            Ok(Control::None)
        }
        Stmt::AsyncWith { expr, binding, body, .. } => {
            // Await the future, bind its result, run the body.
            let future_val = eval_expr(expr, ctx)?;
            let future_id = future_to_id(&future_val)?;
            let result = task_wait_builtin(future_id)?;
            ctx.define_local(binding, result);
            exec_block(body, ctx)
        }
        Stmt::Yield { line, expr } => {
            let value = eval_expr(expr, ctx)
                .map_err(|e| format!("Line {}: {}", line, e))?;
            Ok(Control::Yield(value))
        }
        Stmt::Decorator { line: _, name, args, target } => {
            // Decorators: apply the decorator function to the target
            // For now, execute the decorator as a function call and bind the result
            let _ = exec_call(name, args, ctx)?;
            // Then execute the decorated target
            exec_stmt(target, ctx)
        }
        Stmt::Open { line, mode, path_expr, binding, body } => {
            let path_val = eval_expr(path_expr, ctx)
                .map_err(|e| format!("Line {}: {}", line, e))?;
            let path = path_val.to_string();

            let file = std::fs::OpenOptions::new()
                .read(mode == "read")
                .write(mode == "write" || mode == "append")
                .append(mode == "append")
                .create(mode == "write" || mode == "append")
                .truncate(mode == "write")
                .open(&path)
                .map_err(|e| format!("Line {}: open failed for '{}': {}", line, path, e))?;

            // Read entire file into string if in read mode
            let content = if mode == "read" {
                Some(std::fs::read_to_string(&path)
                    .map_err(|e| format!("Line {}: read failed for '{}': {}", line, path, e))?)
            } else {
                None
            };

            // Bind the value
            if let Some(bind) = binding {
                let val = match mode.as_str() {
                    "read" => Value::Str(content.unwrap_or_default()),
                    _ => Value::Str(path.clone()),
                };
                ctx.define_local(bind, val);
            }

            // Execute the body
            let result = exec_block(body, ctx);

            // Write back if in write mode
            if mode == "write" || mode == "append" {
                if let Some(bind) = binding {
                    if let Some(val) = ctx.get_var(bind) {
                        let text = val.to_string();
                        std::fs::write(&path, text)
                            .map_err(|e| format!("Line {}: write failed for '{}': {}", line, path, e))?;
                    }
                }
            }

            // Close the file (drop happens automatically)
            drop(file);

            result
        }
    }
}

fn stmt_line(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Say { line, .. }
        | Stmt::DefVar { line, .. }
        | Stmt::MakeType { line, .. }
        | Stmt::DefClass { line, .. }
        | Stmt::Assign { line, .. }
        | Stmt::AssignOp { line, .. }
        | Stmt::AssignIndex { line, .. }
        | Stmt::AssignSlice { line, .. }
        | Stmt::DefFun { line, .. }
        | Stmt::Give { line, .. }
        | Stmt::IfChain { line, .. }
        | Stmt::Match { line, .. }
        | Stmt::DoChain { line, .. }
        | Stmt::Repeat { line, .. }
        | Stmt::Stop { line }
        | Stmt::Next { line }
        | Stmt::Reset { line }
        | Stmt::Import { line, .. }
        | Stmt::Call { line, .. }
        | Stmt::BareExpr { line, .. }
        | Stmt::Flag { line, .. }
        | Stmt::Yield { line, .. }
        | Stmt::Decorator { line, .. }
        | Stmt::Open { line, .. }
        | Stmt::Loop { line, .. }
        | Stmt::Await { line, .. }
        | Stmt::Wait { line, .. }
        | Stmt::AsyncWith { line, .. } => *line,
    }
}

fn maybe_debug(line: usize, ctx: &mut ExecContext<'_>) -> Result<(), String> {
    let Some(debugger) = ctx.rt.debugger.as_ref() else {
        return Ok(());
    };

    let should_pause = debugger.stepping || debugger.breakpoints.contains(&line);
    if !should_pause {
        return Ok(());
    }

    loop {
        let src = {
            let dbg = ctx.rt.debugger.as_ref().ok_or_else(|| {
                "Debugger state disappeared unexpectedly".to_string()
            })?;
            dbg.source_lines
                .get(line.saturating_sub(1))
                .cloned()
                .unwrap_or_default()
        };
        println!("[Indent Debug] line {line}: {src}");
        print!("(indentdb) ");
        io::stdout()
            .flush()
            .map_err(|e| format!("Debugger I/O error: {e}"))?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| format!("Debugger I/O error: {e}"))?;
        let cmd = input.trim();

        if cmd.is_empty() || cmd == "s" || cmd == "step" {
            if let Some(dbg) = ctx.rt.debugger.as_mut() {
                dbg.stepping = true;
            }
            return Ok(());
        }

        if cmd == "c" || cmd == "continue" {
            if let Some(dbg) = ctx.rt.debugger.as_mut() {
                dbg.stepping = false;
            }
            return Ok(());
        }

        if cmd == "q" || cmd == "quit" {
            return Err(DEBUGGER_STOP_MSG.to_string());
        }

        if cmd == "l" || cmd == "list" {
            let source_len = ctx
                .rt
                .debugger
                .as_ref()
                .map(|d| d.source_lines.len())
                .unwrap_or(0);
            let start = line.saturating_sub(3).max(1);
            let end = (line + 3).min(source_len);
            for i in start..=end {
                if let Some(text) = ctx
                    .rt
                    .debugger
                    .as_ref()
                    .and_then(|d| d.source_lines.get(i - 1))
                {
                    let marker = if i == line { ">" } else { " " };
                    println!("{marker} {i:4} | {text}");
                }
            }
            continue;
        }

        if let Some(rest) = cmd.strip_prefix("b ") {
            let line_num = rest.trim().parse::<usize>().map_err(|_| {
                "Breakpoint command expects a line number: b <line>".to_string()
            })?;
            if let Some(dbg) = ctx.rt.debugger.as_mut() {
                dbg.breakpoints.insert(line_num);
            }
            println!("Breakpoint added at line {line_num}");
            continue;
        }

        if let Some(rest) = cmd.strip_prefix("cl ") {
            let line_num = rest.trim().parse::<usize>().map_err(|_| {
                "Clear command expects a line number: cl <line>".to_string()
            })?;
            if let Some(dbg) = ctx.rt.debugger.as_mut() {
                dbg.breakpoints.remove(&line_num);
            }
            println!("Breakpoint removed at line {line_num}");
            continue;
        }

        if cmd == "bl" {
            let mut bps = ctx
                .rt
                .debugger
                .as_ref()
                .map(|d| d.breakpoints.iter().copied().collect::<Vec<_>>())
                .unwrap_or_default();
            if bps.is_empty() {
                println!("No breakpoints set");
            } else {
                bps.sort_unstable();
                println!("Breakpoints: {bps:?}");
            }
            continue;
        }

        if let Some(expr) = cmd.strip_prefix("p ") {
            match eval_expr(expr.trim(), ctx) {
                Ok(v) => println!("{v}"),
                Err(e) => println!("Expression error: {e}"),
            }
            continue;
        }

        println!(
            "Commands: s(step), c(continue), p <expr>, b <line>, cl <line>, bl, l(list), q(quit)"
        );
    }
}

fn exec_repeat(mode: &RepeatMode, body: &[Stmt], ctx: &mut ExecContext<'_>) -> Result<Control, String> {
    let max_iters = 100_000usize;

    match mode {
        RepeatMode::Infinite => {
            let mut reps = 0usize;
            loop {
                if reps > max_iters {
                    return Err("Repeat loop exceeded safety limit".to_string());
                }
                match run_loop_iteration(body, ctx, reps, None)? {
                    Control::None | Control::Next => reps += 1,
                    Control::Stop => break,
                    Control::Reset => reps = 0,
                    other => return Ok(other),
                }
            }
            Ok(Control::None)
        }
        RepeatMode::Count(expr) => {
            let total = eval_expr(expr, ctx)?;
            let n = total
                .as_number()
                .ok_or_else(|| format!("Repeat count must be a number, got {}: {}", total.type_name(), total))? as usize;
            let mut reps = 0usize;
            while reps < n {
                match run_loop_iteration(body, ctx, reps, None)? {
                    Control::None | Control::Next => reps += 1,
                    Control::Stop => break,
                    Control::Reset => reps = 0,
                    other => return Ok(other),
                }
            }
            Ok(Control::None)
        }
        RepeatMode::ForEach(expr) => {
            let iterable = eval_expr(expr, ctx)?;
            let items = match iterable {
                Value::List(v) => v,
                Value::Set(v) => v,
                _ => return Err("Repeat for expects a list or set".to_string()),
            };
            let mut reps = 0usize;
            for item in items {
                match run_loop_iteration(body, ctx, reps, Some(("Item", item)))? {
                    Control::None | Control::Next => reps += 1,
                    Control::Stop => break,
                    Control::Reset => reps = 0,
                    other => return Ok(other),
                }
            }
            Ok(Control::None)
        }
        RepeatMode::ForIn {
            item_name,
            iterable_expr,
        } => {
            let iterable = eval_expr(iterable_expr, ctx)?;
            let items = match iterable {
                Value::List(v) => v,
                Value::Set(v) => v,
                _ => return Err("Repeat for <Item> in expects a list or set".to_string()),
            };
            let mut reps = 0usize;
            for item in items {
                match run_loop_iteration(body, ctx, reps, Some((item_name, item)))? {
                    Control::None | Control::Next => reps += 1,
                    Control::Stop => break,
                    Control::Reset => reps = 0,
                    other => return Ok(other),
                }
            }
            Ok(Control::None)
        }
        RepeatMode::While(cond_expr) => {
            let mut reps = 0usize;
            loop {
                if reps > max_iters {
                    return Err("Repeat while loop exceeded safety limit".to_string());
                }
                if !eval_expr(cond_expr, ctx)?.to_bool() {
                    break;
                }
                match run_loop_iteration(body, ctx, reps, None)? {
                    Control::None | Control::Next => reps += 1,
                    Control::Stop => break,
                    Control::Reset => reps = 0,
                    other => return Ok(other),
                }
            }
            Ok(Control::None)
        }
        RepeatMode::Until(cond_expr) => {
            let mut reps = 0usize;
            loop {
                if reps > max_iters {
                    return Err("Repeat until loop exceeded safety limit".to_string());
                }
                if eval_expr(cond_expr, ctx)?.to_bool() {
                    break;
                }
                match run_loop_iteration(body, ctx, reps, None)? {
                    Control::None | Control::Next => reps += 1,
                    Control::Stop => break,
                    Control::Reset => reps = 0,
                    other => return Ok(other),
                }
            }
            Ok(Control::None)
        }
    }
}

fn run_loop_iteration(
    body: &[Stmt],
    ctx: &mut ExecContext<'_>,
    reps: usize,
    item: Option<(&str, Value)>,
) -> Result<Control, String> {
    ctx.push_frame();
    ctx.define_local("Reps", Value::Int(reps as i64));
    if let Some((name, value)) = item {
        ctx.define_local(name, value);
    }
    let out = exec_block(body, ctx);
    ctx.pop_frame();
    out
}

fn import_stmt(
    module_name: &str,
    symbol_name: Option<&str>,
    alias: Option<&str>,
    ctx: &mut ExecContext<'_>,
) -> Result<(), String> {
    // `get <builtin>` (no "from") binds a builtin as a value, e.g. `get len`.
    // Otherwise `get <module>` / `get X from module` imports a module/symbol.
    if symbol_name.is_none() && INDENT_BUILTINS.contains(&module_name) {
        let bind = alias.unwrap_or(module_name).to_string();
        ctx.rt
            .callables
            .insert(bind, Callable::Builtin(module_name.to_string()));
        return Ok(());
    }

    let module = load_module(module_name, ctx)?;

    match symbol_name {
        None => {
            let bind = alias.unwrap_or(module_name).to_string();
            ctx.rt.modules.insert(bind.clone(), module.clone());
            ctx.rt.vars.insert(bind, Value::Module(module));
        }
        Some(symbol) => {
            let bind = alias.unwrap_or(symbol).to_string();
            if let Some(f) = module.funcs.get(symbol) {
                let callable = Callable::External {
                    module: module.clone(),
                    name: symbol.to_string(),
                };
                ctx.rt.callables.insert(bind, callable);
                let _ = f;
            } else if let Some(v) = module.vars.get(symbol) {
                ctx.rt.vars.insert(bind, v.clone());
            } else {
                return Err(format!(
                    "Module '{}' does not export '{}'",
                    module_name, symbol
                ));
            }
        }
    }
    Ok(())
}

fn load_module(module_name: &str, ctx: &mut ExecContext<'_>) -> Result<Arc<ModuleInstance>, String> {
    let path = find_module_file(&ctx.rt.module_dir, module_name)
        .ok_or_else(|| format!("Cannot import module '{}': file not found", module_name))?;
    let canonical = path
        .canonicalize()
        .map_err(|_| format!("Cannot import module '{}': file not found", module_name))?;

    if let Some(m) = ctx.rt.module_cache.lock().unwrap().get(&canonical) {
        return Ok(m.clone());
    }

    let mut module_runtime = Runtime::with_cache(
        canonical
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf(),
        ctx.rt.module_cache.clone(),
    );
    module_runtime.run_file(&canonical)?;
    let module = Arc::new(ModuleInstance {
        vars: module_runtime.vars,
        funcs: module_runtime.funcs,
        callables: module_runtime.callables,
    });

    ctx.rt
        .module_cache
        .lock()
        .unwrap()
        .insert(canonical, module.clone());
    Ok(module)
}

/// Check a single directory for a module file:
/// name.ind, name.ath (legacy), name.glo (env/global), name/__init__.ind.
fn module_in_dir(dir: &Path, module_rel: &str) -> Option<PathBuf> {
    for ext in [".ind", ".ath", ".glo"] {
        let candidate = dir.join(format!("{module_rel}{ext}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    let init_candidate = dir.join(module_rel).join("__init__.ind");
    if init_candidate.exists() {
        return Some(init_candidate);
    }
    None
}

/// Split a path-list environment variable into its entries.
/// Unix uses ':' as the separator; Windows uses ';'. Splitting on ':' on
/// Windows would also mangle drive letters (e.g. C:\Users\...), so the
/// separator must match the host OS.
fn split_path_list(raw: &str) -> Vec<&str> {
    #[cfg(target_os = "windows")]
    let sep = ';';
    #[cfg(not(target_os = "windows"))]
    let sep = ':';
    raw.split(sep).map(str::trim).filter(|s| !s.is_empty()).collect()
}

/// Resolve the user's home directory. Unix sets HOME; Windows does not
/// (it uses USERPROFILE). Fall back to the current dir if neither is set.
fn home_dir() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    let candidates = ["USERPROFILE", "HOME", "HOMEDRIVE"];
    #[cfg(not(target_os = "windows"))]
    let candidates = ["HOME"];
    for var in candidates {
        if let Ok(v) = std::env::var(var) {
            #[cfg(target_os = "windows")]
            if var == "HOMEDRIVE" {
                if let Ok(home_path) = std::env::var("HOMEPATH") {
                    return Some(PathBuf::from(format!("{v}{home_path}")));
                }
                continue;
            }
            return Some(PathBuf::from(v));
        }
    }
    None
}

fn find_module_file(current_dir: &Path, module_name: &str) -> Option<PathBuf> {
    let module_rel = module_name.replace('.', "/");

    // Walk up from the script dir: check each dir, then its aether_packages/
    // (where `air install --local` / `aetherpkg install` place packages).
    let mut cursor = Some(current_dir);
    while let Some(dir) = cursor {
        if let Some(found) = module_in_dir(dir, &module_rel) {
            return Some(found);
        }
        let pkg_dir = dir.join("aether_packages");
        if let Some(found) = module_in_dir(&pkg_dir, &module_rel) {
            return Some(found);
        }
        cursor = dir.parent();
    }

    if let Ok(raw_paths) = env::var("INDENT_PATH") {
        for base in split_path_list(&raw_paths) {
            if let Some(found) = module_in_dir(Path::new(base), &module_rel) {
                return Some(found);
            }
        }
    }

    // Default site-packages (like Python's ~/.local/lib/python*/site-packages)
    if let Some(home) = home_dir() {
        let site_pkg = home.join(".local/share/indent/site-packages");
        if let Some(found) = module_in_dir(&site_pkg, &module_rel) {
            return Some(found);
        }
        // Standard library folder (installers place std modules here).
        let std_dir = home.join(".local/share/indent/std");
        if let Some(found) = module_in_dir(&std_dir, &module_rel) {
            return Some(found);
        }
        // AIR-installed packages (air install <pkg>).
        let air_pkg = home.join(".local/share/indent/air-packages");
        if let Some(found) = module_in_dir(&air_pkg, &module_rel) {
            return Some(found);
        }
        // Legacy Aether global installs (aetherpkg --global) also resolve.
        let legacy_pkg = home.join(".local/share/aether/site-packages");
        if let Some(found) = module_in_dir(&legacy_pkg, &module_rel) {
            return Some(found);
        }
    }

    None
}

fn exec_call(callee: &str, args: &[ArgItem], ctx: &mut ExecContext<'_>) -> Result<Value, String> {
    // Always evaluate the call in a fresh argument scope, and ALWAYS pop it —
    // even on error — so failed calls (e.g. a variable name parsed as a
    // zero-arg call) cannot leak frames or corrupt variable scoping.
    ctx.push_frame();
    let result = exec_call_inner(callee, args, ctx);
    ctx.pop_frame();
    result
}

fn exec_call_inner(callee: &str, args: &[ArgItem], ctx: &mut ExecContext<'_>) -> Result<Value, String> {
    let mut positional: Vec<Value> = vec![];
    let mut named: HashMap<String, Value> = HashMap::new();

    for arg in args {
        match arg {
            ArgItem::Positional(expr) => positional.push(eval_expr(expr, ctx)?),
            ArgItem::Named { name, expr } => {
                named.insert(name.clone(), eval_expr(expr, ctx)?);
            }
            ArgItem::DefVar(stmt) => {
                let c = exec_stmt(stmt, ctx)?;
                if !matches!(c, Control::None) {
                    return Err("Loop/control statements are not allowed in call argument blocks".to_string());
                }
            }
        }
    }

    if let Some(target) = callee.strip_prefix("py.") {
        return python_prefixed_call_builtin(target, &positional, &named);
    }

    // Check if callee is a class name — instantiate it
    if let Some(class_def) = ctx.rt.classes.get(callee).cloned() {
        return Ok(instantiate_class(&class_def, &positional, &ctx.rt.classes));
    }

    // Handle call_func builtin — dynamic function invocation by name
    if callee == "call_func" {
        if positional.is_empty() {
            return Err("call_func expects at least 1 argument".to_string());
        }
        let func_name = positional[0].to_string();
        let func_args: Vec<Value> = positional[1..].to_vec();

        if named.is_empty() {
            if let Some(result) = invoke_builtin(&func_name, &func_args) {
                return result;
            }
        }

        let callable = resolve_callable(&func_name, ctx)?;
        return match callable {
            Callable::Local(f) => invoke_function(&f, &func_args, &named, ctx),
            Callable::External { module, name } => {
                invoke_external_function(module, &name, &func_args, &named, ctx)
            }
            Callable::Builtin(name) => invoke_builtin_callable(&name, &func_args, &named),
        };
    }

    // Handle spawn builtin — run a function on a background thread.
    if callee == "spawn" {
        if positional.is_empty() {
            return Err("spawn expects at least 1 argument (function_name, args...)".to_string());
        }
        let fn_name = positional[0].to_string();
        let task_args: Vec<Value> = positional[1..].to_vec();
        return spawn_task_builtin(&fn_name, &task_args, ctx);
    }

    // Handle future builtin — schedule a function as an async future (background thread).
    if callee == "future" {
        if positional.is_empty() {
            return Err("future expects at least 1 argument (function_name, args...)".to_string());
        }
        let fn_name = positional[0].to_string();
        let task_args: Vec<Value> = positional[1..].to_vec();
        return spawn_task_builtin(&fn_name, &task_args, ctx);
    }

    // Handle coop builtin — run async function bodies cooperatively on one thread.
    if callee == "coop" {
        if positional.len() != 1 {
            return Err("coop expects exactly 1 argument: coop [[fn, args], ...]".to_string());
        }
        return coop_builtin(&positional[0], ctx);
    }

    // Handle parallel builtin — run a function for each arg-list concurrently.
    if callee == "parallel" {
        if positional.len() != 2 {
            return Err("parallel expects exactly 2 arguments (function, list_of_arglists)".to_string());
        }
        let fn_name = positional[0].to_string();
        return parallel_builtin(&fn_name, &positional[1], ctx);
    }

    // Handle run_file builtin — run another Indent file in this runtime.
    // Module-level vars/functions it defines become available here (include-like).
    if callee == "run_file" {
        if positional.len() != 1 {
            return Err("run_file expects exactly 1 argument (path)".to_string());
        }
        let path = PathBuf::from(positional[0].to_string());
        ctx.rt.run_file(&path).map_err(|e| format!("run_file: {e}"))?;
        return Ok(Value::Empty);
    }

    // User-defined and imported functions take precedence over builtins
    // (Python-style shadowing), so std library functions like `Upper` or
    // `Append` are not hidden by case-insensitive builtin lookup.
    if let Ok(callable) = resolve_callable(callee, ctx) {
        // An `async fun` call returns a future: schedule it on a background thread.
        if ctx.rt.async_funcs.contains(callee) && matches!(callable, Callable::Local(_)) {
            let task_args: Vec<Value> = positional.clone();
            return spawn_task_builtin(callee, &task_args, ctx);
        }
        return match callable {
            Callable::Local(f) => invoke_function(&f, &positional, &named, ctx),
            Callable::External { module, name } => {
                invoke_external_function(module, &name, &positional, &named, ctx)
            }
            Callable::Builtin(name) => invoke_builtin_callable(&name, &positional, &named),
        };
    }

    if named.is_empty() {
        if let Some(result) = invoke_builtin(callee, &positional) {
            return result;
        }
    }

    if let Some(result) = invoke_object_method_call(callee, &positional, &named, ctx) {
        return result;
    }

    resolve_callable(callee, ctx)?;
    unreachable!()
}

fn map_object_method_builtin(receiver: &Value, method: &str) -> Option<&'static str> {
    match receiver {
        Value::List(_) => match method {
            "append" => Some("append"),
            "extend" => Some("extend"),
            "insert" => Some("insert"),
            "pop" => Some("pop"),
            "remove" => Some("remove"),
            "index" => Some("index"),
            "copy" => Some("copy"),
            "clear" => Some("clear"),
            "count" => Some("count"),
            "sort" => Some("sort"),
            "reverse" => Some("reverse"),
            "sum" => Some("sum"),
            "min" => Some("min"),
            "max" => Some("max"),
            "any" => Some("any"),
            "all" => Some("all"),
            "enumerate" => Some("enumerate"),
            "zip" => Some("zip"),
            "contains" | "has" => Some("contains"),
            "len" => Some("len"),
            "slice" => Some("slice"),
            _ => None,
        },
        Value::Set(_) => match method {
            "add" => Some("set_add"),
            "remove" => Some("set_remove"),
            "contains" | "has" => Some("set_contains"),
            "len" => Some("len"),
            "copy" => Some("copy"),
            "clear" => Some("clear"),
            _ => None,
        },
        Value::Str(_) => match method {
            "upper" => Some("upper"),
            "lower" => Some("lower"),
            "trim" | "strip" => Some("trim"),
            "lstrip" => Some("lstrip"),
            "rstrip" => Some("rstrip"),
            "capitalize" => Some("capitalize"),
            "title" => Some("title"),
            "swapcase" => Some("swapcase"),
            "replace" => Some("replace"),
            "split" => Some("split"),
            "starts_with" | "startswith" => Some("starts_with"),
            "ends_with" | "endswith" => Some("ends_with"),
            "contains" | "has" => Some("contains"),
            "find" => Some("find"),
            "index" => Some("index"),
            "copy" => Some("copy"),
            "clear" => Some("clear"),
            "count" => Some("count"),
            "len" => Some("len"),
            "reverse" => Some("reverse"),
            "slice" => Some("slice"),
            _ => None,
        },
        Value::Dict(_) => match method {
            "keys" => Some("keys"),
            "values" => Some("values"),
            "items" => Some("items"),
            "get" => Some("dict_get"),
            "set" => Some("dict_set"),
            "update" => Some("dict_update"),
            "remove" => Some("dict_remove"),
            "copy" => Some("copy"),
            "clear" => Some("clear"),
            "has_key" | "contains_key" => Some("has_key"),
            "contains" | "has" => Some("contains"),
            "len" => Some("len"),
            _ => None,
        },
        _ => None,
    }
}

fn invoke_object_method(
    method_def: &FunctionDef,
    fields: &HashMap<String, Value>,
    args: &[Value],
    _ctx: &ExecContext<'_>,
) -> Result<Value, String> {
    // We need a mutable context for exec_block. Create a temporary runtime wrapper.
    // The method body executes with fields as local variables.
    // For now, we use a simplified approach — create a standalone scope.
    
    let mut temp_rt = Runtime::new(PathBuf::from("."));
    temp_rt.vars = fields.clone();
    let mut temp_ctx = ExecContext::new(&mut temp_rt);
    temp_ctx.push_frame();
    
    // Set up positional params
    let params = &method_def.params;
    for (i, param) in params.iter().enumerate() {
        let val = if i < args.len() {
            args[i].clone()
        } else {
            Value::Empty
        };
        temp_ctx.define_local(&param.name, val);
    }
    
    match exec_block(&method_def.body, &mut temp_ctx)? {
        Control::Return(v) => Ok(v),
        Control::Yield(v) => Ok(Value::List(vec![v])),
        Control::None => Ok(Value::Empty),
        _ => Err("STOP/NEXT/RESET cannot be used in a method".to_string()),
    }
}

fn invoke_object_method_call(
    callee: &str,
    positional: &[Value],
    named: &HashMap<String, Value>,
    ctx: &ExecContext<'_>,
) -> Option<Result<Value, String>> {
    let (receiver_name, method_name) = callee.split_once('.')?;
    if receiver_name.is_empty() || method_name.is_empty() || method_name.contains('.') {
        return None;
    }

    // Keep module function dispatch precedence for imported modules.
    if ctx.get_module(receiver_name).is_some() {
        return None;
    }

    let receiver = ctx.get_var(receiver_name)?;

    if !named.is_empty() {
        return Some(Err(format!(
            "Object method call '{}' does not support named arguments",
            callee
        )));
    }

    // Handle Object method dispatch (classes)
    if let Value::Object {
        fields,
        methods,
        class_name,
    } = &receiver
    {
        let method_lc = method_name.to_ascii_lowercase();
        if let Some(method_def) = methods.get(&method_lc) {
            // Create a scope with object fields as local variables
            let result = invoke_object_method(method_def, fields, positional, ctx);
            return Some(result);
        }
        return Some(Err(format!(
            "Object '{}' of class '{}' has no method '{}'",
            receiver_name, class_name, method_name
        )));
    }

    let method_lc = method_name.to_ascii_lowercase();
    let builtin_name = match map_object_method_builtin(&receiver, &method_lc) {
        Some(name) => name,
        None => {
            return Some(Err(format!(
                "Unsupported method '{}' for receiver '{}'",
                method_name, receiver_name
            )))
        }
    };

    let mut args = Vec::with_capacity(positional.len() + 1);
    args.push(receiver);
    args.extend(positional.iter().cloned());

    Some(match invoke_builtin(builtin_name, &args) {
        Some(result) => result,
        None => Err(format!(
            "Internal error: mapped method '{}' is not implemented",
            builtin_name
        )),
    })
}

fn resolve_callable(callee: &str, ctx: &ExecContext<'_>) -> Result<Callable, String> {
    if let Some((left, right)) = callee.split_once('.') {
        let module = ctx
            .get_module(left)
            .ok_or_else(|| format!("Undefined module '{}'", left))?;
        if module.funcs.contains_key(right) {
            return Ok(Callable::External {
                module,
                name: right.to_string(),
            });
        }
        return Err(format!("Module '{}' has no function '{}'", left, right));
    }

    ctx.rt
        .callables
        .get(callee)
        .cloned()
        .ok_or_else(|| format!("Undefined function '{}'", callee))
}

fn invoke_external_function(
    module: Arc<ModuleInstance>,
    name: &str,
    positional: &[Value],
    named: &HashMap<String, Value>,
    ctx: &mut ExecContext<'_>,
) -> Result<Value, String> {
    let f = module
        .funcs
        .get(name)
        .ok_or_else(|| format!("Module function '{}' not found", name))?;

    // Temporarily inject module symbols so module functions can compose
    // with sibling module functions and module-level variables.
    let mut previous_callables: HashMap<String, Option<Callable>> = HashMap::new();

    // 1) Inject the functions this module imported from other modules, so a
    //    module function can call its own `get X from other` imports.
    for (name, callable) in &module.callables {
        if module.funcs.contains_key(name) {
            continue; // own functions are injected in step 2 as externals
        }
        previous_callables.insert(name.clone(), ctx.rt.callables.get(name).cloned());
        ctx.rt.callables.insert(name.clone(), callable.clone());
    }

    // 2) Inject the module's own functions.
    for func_name in module.funcs.keys() {
        previous_callables.insert(func_name.clone(), ctx.rt.callables.get(func_name).cloned());
        ctx.rt.callables.insert(
            func_name.clone(),
            Callable::External {
                module: module.clone(),
                name: func_name.clone(),
            },
        );
    }

    for (var_name, var_value) in &module.vars {
        if !ctx.rt.vars.contains_key(var_name) {
            ctx.rt.vars.insert(var_name.clone(), var_value.clone());
        }
    }

    let result = invoke_function(f, positional, named, ctx);

    for (key, old_value) in previous_callables {
        match old_value {
            Some(v) => {
                ctx.rt.callables.insert(key, v);
            }
            None => {
                ctx.rt.callables.remove(&key);
            }
        }
    }

    result
}

fn invoke_function(
    f: &FunctionDef,
    positional: &[Value],
    named: &HashMap<String, Value>,
    ctx: &mut ExecContext<'_>,
) -> Result<Value, String> {
    ctx.push_frame();

    if !f.params.is_empty() {
        let has_varargs = f.params.last().map_or(false, |p| p.is_varargs);
        if !has_varargs && positional.len() > f.params.len() {
            ctx.pop_frame();
            return Err(format!(
                "Too many positional arguments: expected at most {}, got {}",
                f.params.len(),
                positional.len()
            ));
        }

        for (idx, param) in f.params.iter().enumerate() {
            if param.is_varargs {
                // Collect all remaining positional args (and any unmatched named) into a list
                let mut rest = Vec::new();
                for (i, v) in positional.iter().enumerate() {
                    if i >= idx {
                        rest.push(v.clone());
                    }
                }
                for (k, v) in named {
                    if !f.params.iter().any(|p| p.name == *k) {
                        rest.push(v.clone());
                    }
                }
                ctx.define_local(&param.name, Value::List(rest));
                continue;
            }
            let raw_value = if let Some(v) = named.get(&param.name) {
                v.clone()
            } else if let Some(v) = positional.get(idx) {
                v.clone()
            } else if let Some(default_expr) = &param.default_value {
                // Evaluate the default expression
                eval_expr(default_expr, ctx).unwrap_or(Value::Empty)
            } else {
                ctx.pop_frame();
                return Err(format!("Missing argument for parameter '{}'", param.name));
            };

            let value = if let Some(ty) = &param.ty {
                coerce_type(0, &param.name, ty, raw_value).map_err(|_| {
                    format!(
                        "Parameter '{}' expects type '{}', got different value",
                        param.name, ty
                    )
                })?
            } else {
                raw_value
            };

            ctx.define_local(&param.name, value);
        }
    }

    for (idx, value) in positional.iter().enumerate() {
        let key = match idx {
            0 => "argument".to_string(),
            1 => "argument2".to_string(),
            _ => format!("argument{}", idx + 1),
        };
        ctx.define_local(&key, value.clone());
    }

    for (k, v) in named {
        ctx.define_local(k, v.clone());
    }

    let output = if f.is_generator {
        // Generator: execute body, collect all yields into a list
        let mut yielded_values = Vec::new();
        loop {
            match exec_block(&f.body, ctx)? {
                Control::Yield(v) => {
                    yielded_values.push(v);
                    // Continue collecting — generators yield multiple values
                    // Simple model: yield from top-level only, no re-entry
                    // For now, break after first yield (simplified generator)
                    break;
                }
                Control::Return(v) => {
                    yielded_values.push(v);
                    break;
                }
                Control::None => break,
                _ => return Err("STOP/NEXT/RESET cannot be used outside a loop".to_string()),
            }
        }
        Ok(Value::List(yielded_values))
    } else {
        match exec_block(&f.body, ctx)? {
            Control::Return(v) => {
                if let Some(rt) = &f.return_type {
                    let checked = coerce_type(0, "return", rt, v)
                        .map_err(|_| format!("Return value does not match declared type '{}'", rt))?;
                    Ok(checked)
                } else {
                    Ok(v)
                }
            }
            Control::None => {
                if f.return_type.is_some() {
                    Err("Function declares a return type but no Give statement returned a value".to_string())
                } else {
                    Ok(Value::Empty)
                }
            }
            Control::Yield(v) => {
                // Single yield in non-generator function wraps in list
                Ok(Value::List(vec![v]))
            }
            _ => Err("STOP/NEXT/RESET cannot be used outside a loop".to_string()),
        }
    };

    ctx.pop_frame();
    output
}

fn invoke_callable_expr(
    callee: &str,
    args: &[Expr],
    ctx: &mut ExecContext<'_>,
) -> Result<Value, String> {
    let mut positional = Vec::with_capacity(args.len());
    for arg in args {
        positional.push(eval_ast(arg, ctx)?);
    }

    if let Some(target) = callee.strip_prefix("py.") {
        return python_prefixed_call_builtin(target, &positional, &HashMap::new());
    }

    // Handle spawn builtin — run a function on a background thread.
    if callee == "spawn" {
        if positional.is_empty() {
            return Err("spawn expects at least 1 argument (function_name, args...)".to_string());
        }
        let fn_name = positional[0].to_string();
        let task_args: Vec<Value> = positional[1..].to_vec();
        return spawn_task_builtin(&fn_name, &task_args, ctx);
    }

    // Handle future builtin — schedule a function as an async future (background thread).
    if callee == "future" {
        if positional.is_empty() {
            return Err("future expects at least 1 argument (function_name, args...)".to_string());
        }
        let fn_name = positional[0].to_string();
        let task_args: Vec<Value> = positional[1..].to_vec();
        return spawn_task_builtin(&fn_name, &task_args, ctx);
    }

    // Handle coop builtin — run async function bodies cooperatively on one thread.
    if callee == "coop" {
        if positional.len() != 1 {
            return Err("coop expects exactly 1 argument: coop [[fn, args], ...]".to_string());
        }
        return coop_builtin(&positional[0], ctx);
    }

    // Handle parallel builtin — run a function for each arg-list concurrently.
    if callee == "parallel" {
        if positional.len() != 2 {
            return Err("parallel expects exactly 2 arguments (function, list_of_arglists)".to_string());
        }
        let fn_name = positional[0].to_string();
        return parallel_builtin(&fn_name, &positional[1], ctx);
    }

    // Handle run_file builtin — run another Indent file in this runtime.
    if callee == "run_file" {
        if positional.len() != 1 {
            return Err("run_file expects exactly 1 argument (path)".to_string());
        }
        let path = PathBuf::from(positional[0].to_string());
        ctx.rt.run_file(&path).map_err(|e| format!("run_file: {e}"))?;
        return Ok(Value::Empty);
    }

    // User-defined and imported functions take precedence over builtins
    // (Python-style shadowing) — must match exec_call_inner so expression-level
    // calls to module functions (e.g. `Count(items)` inside a module) are not
    // shadowed by case-insensitive builtin lookup.
    if let Ok(callable) = resolve_callable(callee, ctx) {
        // An `async fun` call returns a future: schedule it on a background thread.
        if ctx.rt.async_funcs.contains(callee) && matches!(callable, Callable::Local(_)) {
            let task_args: Vec<Value> = positional.clone();
            return spawn_task_builtin(callee, &task_args, ctx);
        }
        return match callable {
            Callable::Local(f) => invoke_function(&f, &positional, &HashMap::new(), ctx),
            Callable::External { module, name } => {
                invoke_external_function(module, &name, &positional, &HashMap::new(), ctx)
            }
            Callable::Builtin(name) => invoke_builtin_callable(&name, &positional, &HashMap::new()),
        };
    }

    if let Some(result) = invoke_builtin(callee, &positional) {
        return result;
    }

    if let Some(result) = invoke_object_method_call(callee, &positional, &HashMap::new(), ctx) {
        return result;
    }

    // Check if callee is a class name — instantiate it
    if let Some(class_def) = ctx.rt.classes.get(callee).cloned() {
        return Ok(instantiate_class(&class_def, &positional, &ctx.rt.classes));
    }

    let callable = resolve_callable(callee, ctx)?;
    match callable {
        Callable::Local(f) => invoke_function(&f, &positional, &HashMap::new(), ctx),
        Callable::External { module, name } => {
            invoke_external_function(module, &name, &positional, &HashMap::new(), ctx)
        }
        Callable::Builtin(name) => invoke_builtin_callable(&name, &positional, &HashMap::new()),
    }
}

/// Compare two Values for sorting: numbers numerically, strings
/// lexicographically, booleans false<true, lists element-wise (like Python),
/// and mixed types fall back to string comparison. Used by `sort` so that
/// lists of pairs (e.g. [similarity, item]) sort by their first element
/// instead of being flattened to strings.
fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::List(x), Value::List(y)) => {
            let n = x.len().min(y.len());
            for i in 0..n {
                let ord = compare_values(&x[i], &y[i]);
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            x.len().cmp(&y.len())
        }
        (Value::Empty, Value::Empty) => std::cmp::Ordering::Equal,
        // Mixed types: fall back to string representation for a stable order.
        _ => a.to_string().cmp(&b.to_string()),
    }
}

/// Names of every builtin function available in Indent, including the ctx-aware
/// ones dispatched outside invoke_builtin (spawn/parallel/future/coop). Returned
/// by the `builtins()` function so scripts can discover what's available.
/// NOTE: keep this in sync when adding a builtin to invoke_builtin (or the exec path).
const INDENT_BUILTINS: &[&str] = &[
    "abs", "add_int", "all", "any", "append", "ask", "assert", "assert_eq",
    "base64_decode", "base64_encode", "between_int", "bool", "boolean", "builtins",
    "capitalize", "clamp", "clear", "coalesce", "colored", "contains", "coop", "copy", "count",
    "counter", "csv_read", "csv_write", "dec", "default", "dict_get", "dict_items",
    "dict_remove", "dict_set", "dict_update", "div_int", "ends_with", "enumerate",
    "err", "error_message", "error_type", "extend", "false", "file_append_text",
    "file_read_text", "file_sha256", "file_size", "file_write_text", "filter", "find",
    "float", "float_or", "format", "future", "future_cancel", "future_done",
    "future_result", "future_wait_for", "gather", "glob", "group", "gui_show_html",
    "gzip_compress", "gzip_decompress", "hash_sha256", "has_key", "http_delete",
    "http_delete_async", "http_get", "http_get_async", "http_patch_json",
    "http_post_json", "http_post_json_async", "http_put_json", "http_put_json_async",
    "http_serve_dir", "inc", "index", "insert", "int", "int_or", "is_err", "is_even",
    "is_missing", "is_odd", "is_ok", "items", "join", "json_dumps", "json_loads",
    "keys", "len", "log", "lower", "lstrip", "map", "math_abs", "math_acos",
    "math_asin", "math_atan", "math_atan2", "math_ceil", "math_cos", "math_exp",
    "math_floor", "math_log", "math_log10", "math_pow", "math_round", "math_sin",
    "math_sqrt", "math_tan", "max", "min", "mod_int", "mul_int", "ok", "os_chdir",
    "os_copy", "os_copy_tree", "os_environ", "os_exists", "os_getcwd", "os_getenv",
    "os_is_dir", "os_is_file", "os_list_dir", "os_mkdir", "os_move", "os_remove",
    "os_rename", "os_run", "os_setenv", "os_system", "pad_left", "pad_right",
    "parallel", "path_abs", "path_basename", "path_dirname", "path_expand",
    "path_ext", "path_join", "path_norm", "path_stem", "pop", "print",
    "process_exit", "python_available", "python_eval", "python_eval_json",
    "python_exec", "python_run_file", "random_choice", "random_float", "random_int",
    "random_seed", "random_shuffle", "range", "regex_findall", "regex_match",
    "regex_replace", "regex_search", "regex_split", "remove", "repeat_str",
    "replace", "reverse", "rstrip", "run_file", "say", "set", "set_add", "set_contains",
    "set_difference", "set_intersection", "set_remove", "set_union", "sformat",
    "sleep", "slice", "sort", "spawn", "split", "sqlite_exec", "sqlite_query",
    "sqlite_query_one", "starts_with", "str", "str_center", "string", "str_ljust",
    "str_partition", "str_removeprefix", "str_removesuffix", "str_rjust",
    "str_splitlines", "str_zfill", "sub_int", "sum", "swapcase", "sys_arch",
    "sys_argv", "sys_executable", "sys_platform", "sys_version", "task_done",
    "task_result", "task_wait", "task_wait_all", "task_wait_timeout", "time_format",
    "time_now", "time_parse", "time_perf_counter", "time_sleep", "time_utc",
    "title", "toml_dumps", "toml_loads", "trim", "true", "try", "type_of", "typeof",
    "unwrap", "upper", "uuid", "values", "walk", "ws_close", "ws_connect",
    "ws_recv_text", "ws_recv_text_timeout", "ws_send_text", "yaml_dumps",
    "yaml_loads", "zip", "zip_extract", "zip_list",
];

/// Invoke a builtin that was bound as a value via `get <builtin>`.
fn invoke_builtin_callable(name: &str, positional: &[Value], named: &HashMap<String, Value>) -> Result<Value, String> {
    if !named.is_empty() {
        return Err(format!("builtin '{}' does not accept named arguments", name));
    }
    invoke_builtin(name, positional).unwrap_or_else(|| Err(format!("Unknown builtin '{name}'")))
}

fn invoke_builtin(callee: &str, positional: &[Value]) -> Option<Result<Value, String>> {
    let builtin = callee.to_ascii_lowercase();
    match builtin.as_str() {
        "builtins" => {
            if !positional.is_empty() {
                return Some(Err("builtins takes no arguments".to_string()));
            }
            Some(Ok(Value::List(
                INDENT_BUILTINS.iter().map(|n| Value::Str(n.to_string())).collect(),
            )))
        }
        "say" | "print" => {
            if positional.is_empty() {
                println!();
                return Some(Ok(Value::Empty));
            }
            let line = positional
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            println!("{line}");
            Some(Ok(Value::Empty))
        }
        "colored" | "colorize" => {
            if positional.len() != 2 {
                return Some(Err("colored expects exactly 2 arguments: colored(text, color)".to_string()));
            }
            let text = positional[0].to_string();
            let color = positional[1].to_string();
            Some(colorize_text(&text, &color).map(Value::Str))
        }
        "ask" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err("ask expects 1 or 2 arguments: ask(prompt) or ask(type, prompt)".to_string()));
            }

            // Indent-2: single argument = prompt, always returns string
            if positional.len() == 1 {
                let prompt = positional[0].to_string();
                let line = match read_input_line(&prompt) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                return Some(Ok(Value::Str(line)));
            }

            // Two arguments: ask(type, prompt) - backward compat
            let requested_ty = positional[0].to_string().to_ascii_lowercase();
            let prompt = positional[1].to_string();

            let line = match read_input_line(&prompt) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };

            let out = match requested_ty.as_str() {
                "string" => Value::Str(line),
                "int" => match line.trim().parse::<i64>() {
                    Ok(v) => Value::Int(v),
                    Err(_) => {
                        return Some(Err(format!("ask(int): expected an integer but got '{}'", line.trim())))
                    }
                },
                "float" => match line.trim().parse::<f64>() {
                    Ok(v) => Value::Float(v),
                    Err(_) => {
                        return Some(Err(format!("ask(float): expected a number but got '{}'", line.trim())))
                    }
                },
                "boolean" | "bool" => {
                    let normalized = line.trim().to_ascii_lowercase();
                    match normalized.as_str() {
                        "true" | "yes" | "y" | "1" => Value::Bool(true),
                        "false" | "no" | "n" | "0" => Value::Bool(false),
                        _ => {
                            return Some(Err(format!(
                                "ask(boolean): expected true/false but got '{}'. Use true/false, yes/no, or 1/0",
                                line.trim()
                            )))
                        }
                    }
                }
                _ => {
                    return Some(Err(format!(
                        "Unsupported ask type '{}'. Use string, int, float, or boolean",
                        requested_ty
                    )))
                }
            };

            Some(Ok(out))
        }
        "assert" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err("assert expects 1 or 2 arguments".to_string()));
            }
            let condition = positional[0].to_bool();
            if condition {
                Some(Ok(Value::Empty))
            } else {
                let msg = positional
                    .get(1)
                    .map(|v| format!("{v}"))
                    .unwrap_or_else(|| "Assertion failed".to_string());
                Some(Err(msg))
            }
        }
        "assert_eq" => {
            if positional.len() < 2 || positional.len() > 3 {
                return Some(Err("assert_eq expects 2 or 3 arguments".to_string()));
            }
            if eq_values(&positional[0], &positional[1]) {
                Some(Ok(Value::Empty))
            } else {
                let msg = positional
                    .get(2)
                    .map(|v| format!("{v}"))
                    .unwrap_or_else(|| {
                        format!(
                            "Assertion failed: left={} right={}",
                            positional[0], positional[1]
                        )
                    });
                Some(Err(msg))
            }
        }
        "len" => {
            if positional.len() != 1 {
                return Some(Err("len expects exactly 1 argument".to_string()));
            }
            let out = match &positional[0] {
                Value::Str(s) => Value::Int(s.chars().count() as i64),
                Value::List(v) => Value::Int(v.len() as i64),
                Value::Set(v) => Value::Int(v.len() as i64),
                Value::Dict(v) => Value::Int(v.len() as i64),
                _ => return Some(Err(format!("len expects a string, list, set, or dictionary, got {}", positional[0].type_name()))),
            };
            Some(Ok(out))
        }
        // `group` is a deprecated alias for `set` (unique ordered collection).
        // Kept so older Indent scripts that used `group [...]` keep working.
        "group" => {
            if positional.is_empty() || positional.len() > 1 {
                return Some(Err("group expects exactly 1 argument: a list of values".to_string()));
            }
            let items = match &positional[0] {
                Value::List(v) => v.clone(),
                Value::Set(v) => v.clone(),
                _ => return Some(Err(format!("group expects a list, got {}", positional[0].type_name()))),
            };
            let mut seen: HashSet<String> = HashSet::new();
            let mut unique: Vec<Value> = Vec::new();
            for item in items {
                let key = format!("{}", item);
                if seen.insert(key) {
                    unique.push(item);
                }
            }
            Some(Ok(Value::Set(unique)))
        }
        "set" => {
            if positional.is_empty() || positional.len() > 1 {
                return Some(Err("set expects exactly 1 argument: a list of values".to_string()));
            }
            let items = match &positional[0] {
                Value::List(v) => v.clone(),
                Value::Set(v) => v.clone(),  // set of set = dedup again
                _ => return Some(Err(format!("set expects a list, got {}", positional[0].type_name()))),
            };
            // Deduplicate, preserving insertion order
            let mut seen: HashSet<String> = HashSet::new();
            let mut unique: Vec<Value> = Vec::new();
            for item in items {
                let key = format!("{}", item);
                if seen.insert(key) {
                    unique.push(item);
                }
            }
            Some(Ok(Value::Set(unique)))
        }
        "range" => {
            if positional.is_empty() || positional.len() > 3 {
                return Some(Err(
                    "range expects 1..3 integer arguments: range(end) or range(start, end, [step])"
                        .to_string(),
                ));
            }

            let to_int = |v: &Value| -> Result<i64, String> {
                match v {
                    Value::Int(i) => Ok(*i),
                    _ => Err(format!("range expects integer arguments, got {}", v.type_name())),
                }
            };

            let (start, end, step) = match positional.len() {
                1 => {
                    let end = match to_int(&positional[0]) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };
                    (0, end, 1)
                }
                2 => {
                    let start = match to_int(&positional[0]) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };
                    let end = match to_int(&positional[1]) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };
                    (start, end, 1)
                }
                _ => {
                    let start = match to_int(&positional[0]) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };
                    let end = match to_int(&positional[1]) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };
                    let step = match to_int(&positional[2]) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };
                    (start, end, step)
                }
            };

            if step == 0 {
                return Some(Err("range step cannot be 0".to_string()));
            }

            let mut out: Vec<Value> = vec![];
            let mut cur = start;
            let max_items = 1_000_000usize;

            if step > 0 {
                while cur < end {
                    if out.len() > max_items {
                        return Some(Err("range exceeded safety limit".to_string()));
                    }
                    out.push(Value::Int(cur));
                    cur += step;
                }
            } else {
                while cur > end {
                    if out.len() > max_items {
                        return Some(Err("range exceeded safety limit".to_string()));
                    }
                    out.push(Value::Int(cur));
                    cur += step;
                }
            }

            Some(Ok(Value::List(out)))
        }
        "split" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err(
                    "split expects 1 or 2 arguments: split(text, [separator])".to_string(),
                ));
            }
            let text = positional[0].to_string();
            let pieces = if positional.len() == 1 {
                text.split_whitespace().map(|s| Value::Str(s.to_string())).collect::<Vec<_>>()
            } else {
                let sep = positional[1].to_string();
                if sep.is_empty() {
                    return Some(Err("split separator cannot be empty".to_string()));
                }
                text.split(&sep).map(|s| Value::Str(s.to_string())).collect::<Vec<_>>()
            };
            Some(Ok(Value::List(pieces)))
        }
        "join" => {
            if positional.len() != 2 {
                return Some(Err("join expects exactly 2 arguments: join(list, separator)".to_string()));
            }
            let items = match &positional[0] {
                Value::List(v) => v,
                _ => return Some(Err(format!("join expects a list, got {}", positional[0].type_name()))),
            };
            let sep = positional[1].to_string();
            let out = items.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(&sep);
            Some(Ok(Value::Str(out)))
        }
        "upper" => {
            if positional.len() != 1 {
                return Some(Err("upper expects exactly 1 argument".to_string()));
            }
            Some(Ok(Value::Str(positional[0].to_string().to_uppercase())))
        }
        "lower" => {
            if positional.len() != 1 {
                return Some(Err("lower expects exactly 1 argument".to_string()));
            }
            Some(Ok(Value::Str(positional[0].to_string().to_lowercase())))
        }
        "trim" => {
            if positional.len() != 1 {
                return Some(Err("trim expects exactly 1 argument".to_string()));
            }
            Some(Ok(Value::Str(positional[0].to_string().trim().to_string())))
        }
        "lstrip" => {
            if positional.len() != 1 {
                return Some(Err("lstrip expects exactly 1 argument".to_string()));
            }
            Some(Ok(Value::Str(
                positional[0].to_string().trim_start().to_string(),
            )))
        }
        "rstrip" => {
            if positional.len() != 1 {
                return Some(Err("rstrip expects exactly 1 argument".to_string()));
            }
            Some(Ok(Value::Str(
                positional[0].to_string().trim_end().to_string(),
            )))
        }
        "capitalize" => {
            if positional.len() != 1 {
                return Some(Err("capitalize expects exactly 1 argument".to_string()));
            }

            let text = positional[0].to_string();
            let mut chars = text.chars();
            let out = if let Some(first) = chars.next() {
                let mut value = String::new();
                value.extend(first.to_uppercase());
                value.push_str(&chars.as_str().to_lowercase());
                value
            } else {
                String::new()
            };

            Some(Ok(Value::Str(out)))
        }
        "title" => {
            if positional.len() != 1 {
                return Some(Err("title expects exactly 1 argument".to_string()));
            }

            let text = positional[0].to_string();
            let mut out = String::with_capacity(text.len());
            let mut new_word = true;

            for ch in text.chars() {
                if ch.is_alphanumeric() {
                    if new_word {
                        out.extend(ch.to_uppercase());
                        new_word = false;
                    } else {
                        out.extend(ch.to_lowercase());
                    }
                } else {
                    new_word = true;
                    out.push(ch);
                }
            }

            Some(Ok(Value::Str(out)))
        }
        "swapcase" => {
            if positional.len() != 1 {
                return Some(Err("swapcase expects exactly 1 argument".to_string()));
            }

            let text = positional[0].to_string();
            let mut out = String::with_capacity(text.len());
            for ch in text.chars() {
                if ch.is_lowercase() {
                    out.extend(ch.to_uppercase());
                } else if ch.is_uppercase() {
                    out.extend(ch.to_lowercase());
                } else {
                    out.push(ch);
                }
            }

            Some(Ok(Value::Str(out)))
        }
        "replace" => {
            if positional.len() != 3 {
                return Some(Err("replace expects exactly 3 arguments: replace(text, from, to)".to_string()));
            }
            let text = positional[0].to_string();
            let from = positional[1].to_string();
            let to = positional[2].to_string();
            Some(Ok(Value::Str(text.replace(&from, &to))))
        }
        "str_zfill" => {
            if positional.len() != 2 {
                return Some(Err("str_zfill expects exactly 2 arguments: str_zfill(text, width)".to_string()));
            }
            let text = positional[0].to_string();
            let width = to_i64_value(&positional[1]).unwrap_or(0);
            let pad = width as usize - text.len();
            let out = if pad > 0 { format!("{}{}", "0".repeat(pad), text) } else { text };
            Some(Ok(Value::Str(out)))
        }
        "str_ljust" | "str_rjust" | "str_center" => {
            if positional.len() < 2 || positional.len() > 3 {
                return Some(Err(format!("{} expects (text, width[, pad])", callee)));
            }
            let text = positional[0].to_string();
            let width = to_i64_value(&positional[1]).unwrap_or(0);
            let pad_char = if positional.len() == 3 { positional[2].to_string().chars().next().unwrap_or(' ') } else { ' ' };
            let total = width as usize;
            let out = if text.len() >= total {
                text
            } else {
                let extra = total - text.len();
                if callee == "str_ljust" {
                    format!("{}{}", text, pad_char.to_string().repeat(extra))
                } else if callee == "str_rjust" {
                    format!("{}{}", pad_char.to_string().repeat(extra), text)
                } else {
                    let left = extra / 2;
                    let right = extra - left;
                    format!("{}{}{}", pad_char.to_string().repeat(left), text, pad_char.to_string().repeat(right))
                }
            };
            Some(Ok(Value::Str(out)))
        }
        "str_splitlines" => {
            if positional.len() != 1 {
                return Some(Err("str_splitlines expects exactly 1 argument".to_string()));
            }
            let text = positional[0].to_string();
            let lines: Vec<Value> = text.lines().map(|l| Value::Str(l.to_string())).collect();
            Some(Ok(Value::List(lines)))
        }
        "str_removeprefix" => {
            if positional.len() != 2 {
                return Some(Err("str_removeprefix expects exactly 2 arguments".to_string()));
            }
            let text = positional[0].to_string();
            let prefix = positional[1].to_string();
            let out = if let Some(stripped) = text.strip_prefix(&prefix) { stripped.to_string() } else { text };
            Some(Ok(Value::Str(out)))
        }
        "str_removesuffix" => {
            if positional.len() != 2 {
                return Some(Err("str_removesuffix expects exactly 2 arguments".to_string()));
            }
            let text = positional[0].to_string();
            let suffix = positional[1].to_string();
            let out = if let Some(stripped) = text.strip_suffix(&suffix) { stripped.to_string() } else { text };
            Some(Ok(Value::Str(out)))
        }
        "str_partition" => {
            if positional.len() != 2 {
                return Some(Err("str_partition expects exactly 2 arguments: str_partition(text, sep)".to_string()));
            }
            let text = positional[0].to_string();
            let sep = positional[1].to_string();
            if sep.is_empty() {
                return Some(Err("str_partition: separator cannot be empty".to_string()));
            }
            let out = match text.find(&sep) {
                Some(idx) => {
                    let before = text[..idx].to_string();
                    let after = text[idx + sep.len()..].to_string();
                    Value::List(vec![Value::Str(before), Value::Str(sep), Value::Str(after)])
                }
                None => Value::List(vec![Value::Str(text), Value::Str(String::new()), Value::Str(String::new())]),
            };
            Some(Ok(out))
        }
        "keys" => {
            if positional.len() != 1 {
                return Some(Err("keys expects exactly 1 argument (dictionary)".to_string()));
            }
            let map = match &positional[0] {
                Value::Dict(v) => v,
                _ => return Some(Err(format!("keys expects a dictionary, got {}", positional[0].type_name()))),
            };
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            Some(Ok(Value::List(
                keys.into_iter().map(Value::Str).collect::<Vec<_>>(),
            )))
        }
        "values" => {
            if positional.len() != 1 {
                return Some(Err("values expects exactly 1 argument (dictionary)".to_string()));
            }
            let map = match &positional[0] {
                Value::Dict(v) => v,
                _ => return Some(Err(format!("values expects a dictionary, got {}", positional[0].type_name()))),
            };
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            let out = keys
                .into_iter()
                .filter_map(|k| map.get(&k).cloned())
                .collect::<Vec<_>>();
            Some(Ok(Value::List(out)))
        }
        "has_key" => {
            if positional.len() != 2 {
                return Some(Err("has_key expects exactly 2 arguments: has_key(dict, key)".to_string()));
            }
            let map = match &positional[0] {
                Value::Dict(v) => v,
                _ => return Some(Err(format!("has_key expects a dictionary, got {}", positional[0].type_name()))),
            };
            let key = positional[1].to_string();
            Some(Ok(Value::Bool(map.contains_key(&key))))
        }
        "sort" => {
            if positional.len() != 1 {
                return Some(Err("sort expects exactly 1 argument (list)".to_string()));
            }
            let items = match &positional[0] {
                Value::List(v) => v.clone(),
                _ => return Some(Err(format!("sort expects a list, got {}", positional[0].type_name()))),
            };

            let all_numbers = items.iter().all(|v| matches!(v, Value::Int(_) | Value::Float(_)));
            if all_numbers {
                let mut nums = items
                    .iter()
                    .map(|v| v.as_number().unwrap_or(0.0))
                    .collect::<Vec<_>>();
                nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let out = nums
                    .into_iter()
                    .map(|n| {
                        if (n.fract()).abs() < f64::EPSILON {
                            Value::Int(n as i64)
                        } else {
                            Value::Float(n)
                        }
                    })
                    .collect::<Vec<_>>();
                Some(Ok(Value::List(out)))
            } else {
                // Generic stable sort that preserves element values. Handles
                // strings, booleans, and nested lists (sorted element-wise by
                // compare_values, like Python's default list comparison) so
                // e.g. [similarity, item] pairs sort by their score.
                let mut sorted = items.clone();
                sorted.sort_by(|a, b| compare_values(a, b));
                Some(Ok(Value::List(sorted)))
            }
        }
        "reverse" => {
            if positional.len() != 1 {
                return Some(Err("reverse expects exactly 1 argument (list or string)".to_string()));
            }
            match &positional[0] {
                Value::List(v) => {
                    let mut out = v.clone();
                    out.reverse();
                    Some(Ok(Value::List(out)))
                }
                Value::Str(s) => {
                    let out = s.chars().rev().collect::<String>();
                    Some(Ok(Value::Str(out)))
                }
                _ => Some(Err(format!("reverse expects a list or string, got {}", positional[0].type_name()))),
            }
        }
        "slice" => {
            if positional.is_empty() || positional.len() > 4 {
                return Some(Err(
                    "slice expects 1..4 arguments: slice(value, [start], [end], [step])"
                        .to_string(),
                ));
            }

            let start = if positional.len() >= 2 {
                match parse_optional_slice_bound(&positional[1], "slice start") {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                }
            } else {
                None
            };

            let end = if positional.len() >= 3 {
                match parse_optional_slice_bound(&positional[2], "slice end") {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                }
            } else {
                None
            };

            let step = if positional.len() == 4 {
                match &positional[3] {
                    Value::Int(v) => *v,
                    Value::Empty => 1,
                    _ => return Some(Err("slice step must be int or empty".to_string())),
                }
            } else {
                1
            };

            Some(slice_builtin(&positional[0], start, end, step))
        }
        "sum" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err("sum expects 1 or 2 arguments: sum(list, [start])".to_string()));
            }

            let items = match &positional[0] {
                Value::List(v) => v,
                _ => return Some(Err(format!("sum expects a list, got {}", positional[0].type_name()))),
            };

            let (mut total, mut all_integral) = if positional.len() == 2 {
                match &positional[1] {
                    Value::Int(v) => (*v as f64, true),
                    Value::Float(v) => (*v, false),
                    _ => {
                        return Some(Err(
                            "sum start value must be int or float".to_string(),
                        ))
                    }
                }
            } else {
                (0.0, true)
            };

            for item in items {
                match item {
                    Value::Int(v) => total += *v as f64,
                    Value::Float(v) => {
                        total += *v;
                        all_integral = false;
                    }
                    _ => {
                        return Some(Err(
                            "sum list items must be int or float".to_string(),
                        ))
                    }
                }
            }

            if all_integral && total.fract().abs() < f64::EPSILON {
                Some(Ok(Value::Int(total as i64)))
            } else {
                Some(Ok(Value::Float(total)))
            }
        }
        "min" => {
            if positional.len() != 1 {
                return Some(Err("min expects exactly 1 argument (list)".to_string()));
            }
            let items = match &positional[0] {
                Value::List(v) => v,
                _ => return Some(Err(format!("min expects a list, got {}", positional[0].type_name()))),
            };

            if items.is_empty() {
                return Some(Err("min cannot operate on an empty list".to_string()));
            }

            let all_numeric = items.iter().all(|v| v.as_number().is_some());
            if all_numeric {
                let mut best = items[0].clone();
                let mut best_num = items[0].as_number().unwrap_or(0.0);
                for item in items.iter().skip(1) {
                    let n = item.as_number().unwrap_or(0.0);
                    if n < best_num {
                        best_num = n;
                        best = item.clone();
                    }
                }
                return Some(Ok(best));
            }

            let all_strings = items.iter().all(|v| matches!(v, Value::Str(_)));
            if all_strings {
                let mut best = match &items[0] {
                    Value::Str(s) => s.clone(),
                    _ => String::new(),
                };
                for item in items.iter().skip(1) {
                    if let Value::Str(s) = item {
                        if s < &best {
                            best = s.clone();
                        }
                    }
                }
                return Some(Ok(Value::Str(best)));
            }

            Some(Err(
                "min expects a list of all numbers or all strings".to_string(),
            ))
        }
        "max" => {
            if positional.len() != 1 {
                return Some(Err("max expects exactly 1 argument (list)".to_string()));
            }
            let items = match &positional[0] {
                Value::List(v) => v,
                _ => return Some(Err(format!("max expects a list, got {}", positional[0].type_name()))),
            };

            if items.is_empty() {
                return Some(Err("max cannot operate on an empty list".to_string()));
            }

            let all_numeric = items.iter().all(|v| v.as_number().is_some());
            if all_numeric {
                let mut best = items[0].clone();
                let mut best_num = items[0].as_number().unwrap_or(0.0);
                for item in items.iter().skip(1) {
                    let n = item.as_number().unwrap_or(0.0);
                    if n > best_num {
                        best_num = n;
                        best = item.clone();
                    }
                }
                return Some(Ok(best));
            }

            let all_strings = items.iter().all(|v| matches!(v, Value::Str(_)));
            if all_strings {
                let mut best = match &items[0] {
                    Value::Str(s) => s.clone(),
                    _ => String::new(),
                };
                for item in items.iter().skip(1) {
                    if let Value::Str(s) = item {
                        if s > &best {
                            best = s.clone();
                        }
                    }
                }
                return Some(Ok(Value::Str(best)));
            }

            Some(Err(
                "max expects a list of all numbers or all strings".to_string(),
            ))
        }
        "any" => {
            if positional.len() != 1 {
                return Some(Err("any expects exactly 1 argument (list)".to_string()));
            }
            let items = match &positional[0] {
                Value::List(v) => v,
                _ => return Some(Err(format!("any expects a list, got {}", positional[0].type_name()))),
            };
            Some(Ok(Value::Bool(items.iter().any(|v| v.to_bool()))))
        }
        "all" => {
            if positional.len() != 1 {
                return Some(Err("all expects exactly 1 argument (list)".to_string()));
            }
            let items = match &positional[0] {
                Value::List(v) => v,
                _ => return Some(Err(format!("all expects a list, got {}", positional[0].type_name()))),
            };
            Some(Ok(Value::Bool(items.iter().all(|v| v.to_bool()))))
        }
        "count" => {
            if positional.len() != 2 {
                return Some(Err(
                    "count expects exactly 2 arguments: count(container, value)".to_string(),
                ));
            }

            match &positional[0] {
                Value::List(items) => {
                    let count = items
                        .iter()
                        .filter(|v| eq_values(v, &positional[1]))
                        .count() as i64;
                    Some(Ok(Value::Int(count)))
                }
                Value::Str(text) => {
                    let needle = match &positional[1] {
                        Value::Str(s) => s,
                        _ => {
                            return Some(Err(
                                "count on string expects string value".to_string(),
                            ))
                        }
                    };
                    if needle.is_empty() {
                        Some(Ok(Value::Int(text.chars().count() as i64 + 1)))
                    } else {
                        Some(Ok(Value::Int(text.matches(needle).count() as i64)))
                    }
                }
                Value::Dict(map) => {
                    let key = match &positional[1] {
                        Value::Str(s) => s,
                        _ => {
                            return Some(Err(
                                "count on dictionary expects string key".to_string(),
                            ))
                        }
                    };
                    Some(Ok(Value::Int(if map.contains_key(key) { 1 } else { 0 })))
                }
                _ => Some(Err(
                    "count expects list, string, or dictionary container".to_string(),
                )),
            }
        }
        "append" => {
            if positional.len() != 2 {
                return Some(Err(
                    "append expects exactly 2 arguments: append(list, value)".to_string(),
                ));
            }
            let mut out = match &positional[0] {
                Value::List(v) => v.clone(),
                _ => return Some(Err(format!("append expects a list, got {}", positional[0].type_name()))),
            };
            out.push(positional[1].clone());
            Some(Ok(Value::List(out)))
        }
        "extend" => {
            if positional.len() != 2 {
                return Some(Err(
                    "extend expects exactly 2 arguments: extend(list, list)".to_string(),
                ));
            }
            let mut out = match &positional[0] {
                Value::List(v) => v.clone(),
                _ => return Some(Err(format!("extend expects a list, got {}", positional[0].type_name()))),
            };
            let extra = match &positional[1] {
                Value::List(v) => v,
                _ => return Some(Err(format!("extend expects a list, got {}", positional[1].type_name()))),
            };
            out.extend(extra.clone());
            Some(Ok(Value::List(out)))
        }
        "insert" => {
            if positional.len() != 3 {
                return Some(Err(
                    "insert expects exactly 3 arguments: insert(list, index, value)".to_string(),
                ));
            }

            let mut out = match &positional[0] {
                Value::List(v) => v.clone(),
                _ => return Some(Err(format!("insert expects a list, got {}", positional[0].type_name()))),
            };
            let raw_index = match &positional[1] {
                Value::Int(v) => *v,
                _ => return Some(Err("insert index must be int".to_string())),
            };

            let len = out.len() as i64;
            let idx = if raw_index >= 0 {
                (raw_index as usize).min(out.len())
            } else {
                let from_end = len + raw_index;
                if from_end <= 0 {
                    0
                } else {
                    (from_end as usize).min(out.len())
                }
            };

            out.insert(idx, positional[2].clone());
            Some(Ok(Value::List(out)))
        }
        "pop" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err(
                    "pop expects 1 or 2 arguments: pop(list, [index])".to_string(),
                ));
            }

            let mut out = match &positional[0] {
                Value::List(v) => v.clone(),
                _ => return Some(Err(format!("pop expects a list, got {}", positional[0].type_name()))),
            };

            if out.is_empty() {
                return Some(Err("pop cannot operate on an empty list".to_string()));
            }

            let raw_index = if positional.len() == 2 {
                match &positional[1] {
                    Value::Int(v) => *v,
                    _ => return Some(Err("pop index must be int".to_string())),
                }
            } else {
                -1
            };

            let idx = match normalize_index(out.len(), raw_index) {
                Some(v) => v,
                None => return Some(Err(format!("pop index out of range: {}", raw_index))),
            };

            let item = out.remove(idx);
            let mut result = HashMap::new();
            result.insert("item".to_string(), item);
            result.insert("list".to_string(), Value::List(out));
            Some(Ok(Value::Dict(result)))
        }
        "remove" => {
            if positional.len() != 2 {
                return Some(Err(
                    "remove expects exactly 2 arguments: remove(list, value)".to_string(),
                ));
            }

            let mut out = match &positional[0] {
                Value::List(v) => v.clone(),
                _ => return Some(Err(format!("remove expects a list, got {}", positional[0].type_name()))),
            };

            let pos = out.iter().position(|v| eq_values(v, &positional[1]));
            match pos {
                Some(index) => {
                    out.remove(index);
                    Some(Ok(Value::List(out)))
                }
                None => Some(Err("remove value not found in list".to_string())),
            }
        }
        "enumerate" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err(
                    "enumerate expects 1 or 2 arguments: enumerate(list, [start])".to_string(),
                ));
            }

            let items = match &positional[0] {
                Value::List(v) => v,
                _ => return Some(Err(format!("enumerate expects a list, got {}", positional[0].type_name()))),
            };

            let start = if positional.len() == 2 {
                match &positional[1] {
                    Value::Int(v) => *v,
                    _ => return Some(Err("enumerate start must be int".to_string())),
                }
            } else {
                0
            };

            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                out.push(Value::List(vec![Value::Int(start + i as i64), item.clone()]));
            }
            Some(Ok(Value::List(out)))
        }
        "zip" => {
            if positional.len() != 2 {
                return Some(Err("zip expects exactly 2 list arguments".to_string()));
            }

            let left = match &positional[0] {
                Value::List(v) => v,
                _ => return Some(Err(format!("zip expects a list, got {}", positional[0].type_name()))),
            };
            let right = match &positional[1] {
                Value::List(v) => v,
                _ => return Some(Err("zip expects second argument to be list".to_string())),
            };

            let size = left.len().min(right.len());
            let mut out = Vec::with_capacity(size);
            for i in 0..size {
                out.push(Value::List(vec![left[i].clone(), right[i].clone()]));
            }
            Some(Ok(Value::List(out)))
        }
        "items" | "dict_items" => {
            if positional.len() != 1 {
                return Some(Err("items expects exactly 1 argument (dictionary)".to_string()));
            }
            let map = match &positional[0] {
                Value::Dict(v) => v,
                _ => return Some(Err(format!("items expects a dictionary, got {}", positional[0].type_name()))),
            };

            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            let mut out = Vec::with_capacity(keys.len());
            for key in keys {
                if let Some(value) = map.get(&key) {
                    out.push(Value::List(vec![Value::Str(key), value.clone()]));
                }
            }
            Some(Ok(Value::List(out)))
        }
        "dict_get" => {
            if positional.len() < 2 || positional.len() > 3 {
                return Some(Err(
                    "dict_get expects 2 or 3 arguments: dict_get(dict, key, [default])"
                        .to_string(),
                ));
            }
            let map = match &positional[0] {
                Value::Dict(v) => v,
                _ => return Some(Err(format!("dict_get expects a dictionary, got {}", positional[0].type_name()))),
            };
            let key = match &positional[1] {
                Value::Str(v) => v,
                _ => return Some(Err("dict_get key must be string".to_string())),
            };

            if let Some(value) = map.get(key) {
                Some(Ok(value.clone()))
            } else if positional.len() == 3 {
                Some(Ok(positional[2].clone()))
            } else {
                Some(Ok(Value::Empty))
            }
        }
        "dict_set" => {
            if positional.len() != 3 {
                return Some(Err(
                    "dict_set expects exactly 3 arguments: dict_set(dict, key, value)"
                        .to_string(),
                ));
            }
            let mut map = match &positional[0] {
                Value::Dict(v) => v.clone(),
                _ => return Some(Err(format!("dict_set expects a dictionary, got {}", positional[0].type_name()))),
            };
            let key = match &positional[1] {
                Value::Str(v) => v.clone(),
                _ => return Some(Err("dict_set key must be string".to_string())),
            };

            map.insert(key, positional[2].clone());
            Some(Ok(Value::Dict(map)))
        }
        "dict_update" => {
            if positional.len() != 2 {
                return Some(Err(
                    "dict_update expects exactly 2 arguments: dict_update(dict, updates)"
                        .to_string(),
                ));
            }

            let mut map = match &positional[0] {
                Value::Dict(v) => v.clone(),
                _ => {
                    return Some(Err(format!(
                        "dict_update expects a dictionary, got {}", positional[0].type_name()
                    )))
                }
            };
            let updates = match &positional[1] {
                Value::Dict(v) => v,
                _ => {
                    return Some(Err(
                        "dict_update expects second argument to be dictionary".to_string(),
                    ))
                }
            };

            for (key, value) in updates {
                map.insert(key.clone(), value.clone());
            }

            Some(Ok(Value::Dict(map)))
        }
        "dict_remove" => {
            if positional.len() != 2 {
                return Some(Err(
                    "dict_remove expects exactly 2 arguments: dict_remove(dict, key)".to_string(),
                ));
            }
            let mut map = match &positional[0] {
                Value::Dict(v) => v.clone(),
                _ => return Some(Err(format!("dict_remove expects a dictionary, got {}", positional[0].type_name()))),
            };
            let key = match &positional[1] {
                Value::Str(v) => v,
                _ => return Some(Err("dict_remove key must be string".to_string())),
            };

            map.remove(key);
            Some(Ok(Value::Dict(map)))
        }
        "starts_with" => {
            if positional.len() != 2 {
                return Some(Err(
                    "starts_with expects exactly 2 arguments (text, prefix)".to_string(),
                ));
            }
            let text = positional[0].to_string();
            let prefix = positional[1].to_string();
            Some(Ok(Value::Bool(text.starts_with(&prefix))))
        }
        "ends_with" => {
            if positional.len() != 2 {
                return Some(Err(
                    "ends_with expects exactly 2 arguments (text, suffix)".to_string(),
                ));
            }
            let text = positional[0].to_string();
            let suffix = positional[1].to_string();
            Some(Ok(Value::Bool(text.ends_with(&suffix))))
        }
        "contains" => {
            if positional.len() != 2 {
                return Some(Err(
                    "contains expects exactly 2 arguments: contains(container, item)".to_string(),
                ));
            }
            match contains_value(&positional[0], &positional[1]) {
                Ok(v) => Some(Ok(Value::Bool(v))),
                Err(e) => Some(Err(e)),
            }
        }
        "copy" => {
            if positional.len() != 1 {
                return Some(Err("copy expects exactly 1 argument".to_string()));
            }

            Some(Ok(positional[0].clone()))
        }
        "clear" => {
            if positional.len() != 1 {
                return Some(Err("clear expects exactly 1 argument".to_string()));
            }

            match &positional[0] {
                Value::List(_) => Some(Ok(Value::List(vec![]))),
                Value::Set(_) => Some(Ok(Value::Set(vec![]))),
                Value::Dict(_) => Some(Ok(Value::Dict(HashMap::new()))),
                Value::Str(_) => Some(Ok(Value::Str(String::new()))),
                _ => Some(Err(format!("clear expects a string, list, set, or dictionary, got {}", positional[0].type_name()))),
            }
        }
        "set_add" => {
            if positional.len() != 2 {
                return Some(Err("set.add expects exactly 1 argument: the element to add".to_string()));
            }
            match &positional[0] {
                Value::Set(items) => {
                    let mut new_items = items.clone();
                    let key = format!("{}", positional[1]);
                    if !items.iter().any(|v| format!("{}", v) == key) {
                        new_items.push(positional[1].clone());
                    }
                    Some(Ok(Value::Set(new_items)))
                }
                _ => Some(Err(format!("add expects a set, got {}", positional[0].type_name()))),
            }
        }
        "set_remove" => {
            if positional.len() != 2 {
                return Some(Err("set.remove expects exactly 1 argument: the element to remove".to_string()));
            }
            match &positional[0] {
                Value::Set(items) => {
                    let key = format!("{}", positional[1]);
                    let new_items: Vec<Value> = items.iter()
                        .filter(|v| format!("{}", v) != key)
                        .cloned()
                        .collect();
                    Some(Ok(Value::Set(new_items)))
                }
                _ => Some(Err(format!("remove expects a set, got {}", positional[0].type_name()))),
            }
        }
        "set_contains" => {
            if positional.len() != 2 {
                return Some(Err("set.contains expects exactly 1 argument".to_string()));
            }
            match &positional[0] {
                Value::Set(items) => {
                    let key = format!("{}", positional[1]);
                    Some(Ok(Value::Bool(items.iter().any(|v| format!("{}", v) == key))))
                }
                _ => Some(Err(format!("contains expects a set, got {}", positional[0].type_name()))),
            }
        }
        "set_union" | "set_intersection" | "set_difference" => {
            if positional.len() != 2 {
                return Some(Err(format!("{} expects exactly 2 set arguments", callee)));
            }
            let a = match &positional[0] { Value::Set(v) => v.clone(), _ => return Some(Err(format!("{} expects a set, got {}", callee, positional[0].type_name()))) };
            let b = match &positional[1] { Value::Set(v) => v.clone(), _ => return Some(Err(format!("{} expects a set, got {}", callee, positional[1].type_name()))) };
            let b_keys: HashSet<String> = b.iter().map(|v| format!("{}", v)).collect();
            let result: Vec<Value> = if callee == "set_union" {
                // a ∪ b : all from a, then b items not already present
                let mut seen: HashSet<String> = HashSet::new();
                let mut out: Vec<Value> = Vec::new();
                for item in a.iter().chain(b.iter()) {
                    let key = format!("{}", item);
                    if seen.insert(key) { out.push(item.clone()); }
                }
                out
            } else if callee == "set_intersection" {
                a.iter().filter(|v| b_keys.contains(&format!("{}", v))).cloned().collect()
            } else {
                a.iter().filter(|v| !b_keys.contains(&format!("{}", v))).cloned().collect()
            };
            Some(Ok(Value::Set(result)))
        }
        "is_missing" => {
            if positional.len() != 1 {
                return Some(Err(
                    "is_missing expects exactly 1 argument".to_string(),
                ));
            }
            Some(Ok(Value::Bool(is_missing_value(&positional[0]))))
        }
        "default" => {
            if positional.len() != 2 {
                return Some(Err(
                    "default expects exactly 2 arguments: default(value, fallback)".to_string(),
                ));
            }

            if is_missing_value(&positional[0]) {
                Some(Ok(positional[1].clone()))
            } else {
                Some(Ok(positional[0].clone()))
            }
        }
        "coalesce" => {
            if positional.is_empty() {
                return Some(Err(
                    "coalesce expects at least 1 argument: coalesce(value1, value2, ...)".to_string(),
                ));
            }

            for value in positional {
                if !is_missing_value(value) {
                    return Some(Ok(value.clone()));
                }
            }

            Some(Ok(Value::Empty))
        }
        "clamp" => {
            if positional.len() != 3 {
                return Some(Err(
                    "clamp expects exactly 3 arguments: clamp(value, min, max)".to_string(),
                ));
            }

            let value = match positional[0].as_number() {
                Some(v) => v,
                None => {
                    return Some(Err(
                        "clamp value must be int or float".to_string(),
                    ))
                }
            };
            let mut min = match positional[1].as_number() {
                Some(v) => v,
                None => {
                    return Some(Err(
                        "clamp min must be int or float".to_string(),
                    ))
                }
            };
            let mut max = match positional[2].as_number() {
                Some(v) => v,
                None => {
                    return Some(Err(
                        "clamp max must be int or float".to_string(),
                    ))
                }
            };

            if min > max {
                std::mem::swap(&mut min, &mut max);
            }

            let out = if value < min {
                min
            } else if value > max {
                max
            } else {
                value
            };

            let all_int_inputs = matches!(positional[0], Value::Int(_))
                && matches!(positional[1], Value::Int(_))
                && matches!(positional[2], Value::Int(_));
            if all_int_inputs {
                Some(Ok(Value::Int(out as i64)))
            } else {
                Some(Ok(Value::Float(out)))
            }
        }
        "find" => {
            if positional.len() != 2 {
                return Some(Err("find expects exactly 2 arguments (text, needle)".to_string()));
            }
            let text = positional[0].to_string();
            let needle = positional[1].to_string();
            if let Some(byte_index) = text.find(&needle) {
                let char_index = text[..byte_index].chars().count() as i64;
                Some(Ok(Value::Int(char_index)))
            } else {
                Some(Ok(Value::Int(-1)))
            }
        }
        "index" => {
            if positional.len() != 2 {
                return Some(Err(
                    "index expects exactly 2 arguments: index(container, value)".to_string(),
                ));
            }

            match &positional[0] {
                Value::List(items) => {
                    let idx = items
                        .iter()
                        .position(|v| eq_values(v, &positional[1]))
                        .map(|v| v as i64)
                        .unwrap_or(-1);
                    Some(Ok(Value::Int(idx)))
                }
                Value::Str(text) => {
                    let needle = match &positional[1] {
                        Value::Str(s) => s,
                        _ => {
                            return Some(Err(
                                "index on string expects string value".to_string(),
                            ))
                        }
                    };

                    if needle.is_empty() {
                        return Some(Ok(Value::Int(0)));
                    }

                    if let Some(byte_index) = text.find(needle) {
                        let char_index = text[..byte_index].chars().count() as i64;
                        Some(Ok(Value::Int(char_index)))
                    } else {
                        Some(Ok(Value::Int(-1)))
                    }
                }
                _ => Some(Err(format!(
                    "index expects a list or string, got {}", positional[0].type_name()
                ))),
            }
        }
        "int" => {
            if positional.len() != 1 {
                return Some(Err("int expects exactly 1 argument".to_string()));
            }
            let out = match to_i64_value(&positional[0]) {
                Ok(v) => Value::Int(v),
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(out))
        }
        "int_or" => {
            if positional.len() != 2 {
                return Some(Err(
                    "int_or expects exactly 2 arguments: int_or(value, fallback)".to_string(),
                ));
            }

            let out = match to_i64_value(&positional[0]) {
                Ok(v) => v,
                Err(_) => match to_i64_value(&positional[1]) {
                    Ok(v) => v,
                    Err(e) => {
                        return Some(Err(format!(
                            "int_or fallback must be int-convertible: {e}"
                        )))
                    }
                },
            };

            Some(Ok(Value::Int(out)))
        }
        "float" => {
            if positional.len() != 1 {
                return Some(Err("float expects exactly 1 argument".to_string()));
            }
            let out = match to_f64_value(&positional[0]) {
                Ok(v) => Value::Float(v),
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(out))
        }
        "float_or" => {
            if positional.len() != 2 {
                return Some(Err(
                    "float_or expects exactly 2 arguments: float_or(value, fallback)"
                        .to_string(),
                ));
            }

            let out = match to_f64_value(&positional[0]) {
                Ok(v) => v,
                Err(_) => match to_f64_value(&positional[1]) {
                    Ok(v) => v,
                    Err(e) => {
                        return Some(Err(format!(
                            "float_or fallback must be float-convertible: {e}"
                        )))
                    }
                },
            };

            Some(Ok(Value::Float(out)))
        }
        "bool" => {
            if positional.len() != 1 {
                return Some(Err("bool expects exactly 1 argument".to_string()));
            }
            Some(Ok(Value::Bool(positional[0].to_bool())))
        }
        // ── Result type (v2.5.0) ──────────────────────────────
        "ok" => {
            if positional.len() != 1 {
                return Some(Err("ok expects exactly 1 argument: ok(value)".to_string()));
            }
            let mut map = HashMap::new();
            map.insert("ok".to_string(), Value::Bool(true));
            map.insert("value".to_string(), positional[0].clone());
            Some(Ok(Value::Dict(map)))
        }
        "err" => {
            if positional.len() != 1 {
                return Some(Err("err expects exactly 1 argument: err(message)".to_string()));
            }
            let mut map = HashMap::new();
            map.insert("ok".to_string(), Value::Bool(false));
            map.insert("error".to_string(), positional[0].clone());
            Some(Ok(Value::Dict(map)))
        }
        "is_ok" => {
            if positional.len() != 1 {
                return Some(Err("is_ok expects exactly 1 argument".to_string()));
            }
            match &positional[0] {
                Value::Dict(m) => Some(Ok(Value::Bool(m.get("ok").map(|v| v.to_bool()).unwrap_or(false)))),
                _ => Some(Ok(Value::Bool(false))),
            }
        }
        "is_err" => {
            if positional.len() != 1 {
                return Some(Err("is_err expects exactly 1 argument".to_string()));
            }
            match &positional[0] {
                Value::Dict(m) => Some(Ok(Value::Bool(!m.get("ok").map(|v| v.to_bool()).unwrap_or(true)))),
                _ => Some(Ok(Value::Bool(true))),
            }
        }
        "unwrap" => {
            if positional.len() < 1 || positional.len() > 2 {
                return Some(Err("unwrap expects 1 or 2 arguments: unwrap(result, [fallback])".to_string()));
            }
            match &positional[0] {
                Value::Dict(m) => {
                    if m.get("ok").map(|v| v.to_bool()).unwrap_or(false) {
                        Some(Ok(m.get("value").cloned().unwrap_or(Value::Empty)))
                    } else {
                        if positional.len() == 2 {
                            Some(Ok(positional[1].clone()))
                        } else {
                            let err_msg = m.get("error").map(|v| v.to_string()).unwrap_or_else(|| "unknown error".to_string());
                            Some(Err(format!("unwrap on err: {err_msg}")))
                        }
                    }
                }
                _ => Some(Ok(positional[0].clone())),
            }
        }
        "try" => {
            if positional.len() != 1 {
                return Some(Err("try expects exactly 1 argument: try(expression)".to_string()));
            }
            let mut map = HashMap::new();
            map.insert("ok".to_string(), Value::Bool(true));
            map.insert("value".to_string(), positional[0].clone());
            Some(Ok(Value::Dict(map)))
        }
        "file_sha256" => {
            if positional.len() != 1 {
                return Some(Err("file_sha256 expects exactly 1 argument (path)".to_string()));
            }
            let path = positional[0].to_string();
            match std::fs::read(&path) {
                Ok(data) => {
                    use sha2::{Sha256, Digest};
                    let mut hasher = Sha256::new();
                    hasher.update(&data);
                    let hash = format!("{:x}", hasher.finalize());
                    Some(Ok(Value::Str(hash)))
                }
                Err(e) => Some(Err(format!("file_sha256 failed: {e}"))),
            }
        }
        "string" | "str" => {
            if positional.len() != 1 {
                return Some(Err("string expects exactly 1 argument".to_string()));
            }
            Some(Ok(Value::Str(positional[0].to_string())))
        }
        "abs" => {
            if positional.len() != 1 {
                return Some(Err("abs expects exactly 1 argument".to_string()));
            }
            match &positional[0] {
                Value::Int(v) => Some(Ok(Value::Int(v.abs()))),
                Value::Float(v) => Some(Ok(Value::Float(v.abs()))),
                _ => Some(Err("abs expects int or float".to_string())),
            }
        }
        "inc" => {
            if positional.len() != 1 && positional.len() != 2 {
                return Some(Err(
                    "inc expects 1 or 2 arguments: inc(value, [step])".to_string(),
                ));
            }

            let base = match to_i64_value(&positional[0]) {
                Ok(v) => v,
                Err(e) => return Some(Err(format!("inc value must be int-convertible: {e}"))),
            };

            let step = if positional.len() == 2 {
                match to_i64_value(&positional[1]) {
                    Ok(v) => v,
                    Err(e) => {
                        return Some(Err(format!("inc step must be int-convertible: {e}")))
                    }
                }
            } else {
                1
            };

            Some(Ok(Value::Int(base + step)))
        }
        "dec" => {
            if positional.len() != 1 && positional.len() != 2 {
                return Some(Err(
                    "dec expects 1 or 2 arguments: dec(value, [step])".to_string(),
                ));
            }

            let base = match to_i64_value(&positional[0]) {
                Ok(v) => v,
                Err(e) => return Some(Err(format!("dec value must be int-convertible: {e}"))),
            };

            let step = if positional.len() == 2 {
                match to_i64_value(&positional[1]) {
                    Ok(v) => v,
                    Err(e) => {
                        return Some(Err(format!("dec step must be int-convertible: {e}")))
                    }
                }
            } else {
                1
            };

            Some(Ok(Value::Int(base - step)))
        }
        "add_int" => {
            if positional.len() != 2 {
                return Some(Err(
                    "add_int expects exactly 2 arguments: add_int(a, b)".to_string(),
                ));
            }

            let left = match to_i64_value(&positional[0]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "add_int first argument must be int-convertible: {e}"
                    )))
                }
            };
            let right = match to_i64_value(&positional[1]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "add_int second argument must be int-convertible: {e}"
                    )))
                }
            };

            Some(Ok(Value::Int(left + right)))
        }
        "sub_int" => {
            if positional.len() != 2 {
                return Some(Err(
                    "sub_int expects exactly 2 arguments: sub_int(a, b)".to_string(),
                ));
            }

            let left = match to_i64_value(&positional[0]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "sub_int first argument must be int-convertible: {e}"
                    )))
                }
            };
            let right = match to_i64_value(&positional[1]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "sub_int second argument must be int-convertible: {e}"
                    )))
                }
            };

            Some(Ok(Value::Int(left - right)))
        }
        "mul_int" => {
            if positional.len() != 2 {
                return Some(Err(
                    "mul_int expects exactly 2 arguments: mul_int(a, b)".to_string(),
                ));
            }

            let left = match to_i64_value(&positional[0]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "mul_int first argument must be int-convertible: {e}"
                    )))
                }
            };
            let right = match to_i64_value(&positional[1]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "mul_int second argument must be int-convertible: {e}"
                    )))
                }
            };

            Some(Ok(Value::Int(left * right)))
        }
        "div_int" => {
            if positional.len() != 2 {
                return Some(Err(
                    "div_int expects exactly 2 arguments: div_int(a, b)".to_string(),
                ));
            }

            let left = match to_i64_value(&positional[0]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "div_int first argument must be int-convertible: {e}"
                    )))
                }
            };
            let right = match to_i64_value(&positional[1]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "div_int second argument must be int-convertible: {e}"
                    )))
                }
            };

            if right == 0 {
                return Some(Err("div_int division by zero".to_string()));
            }

            Some(Ok(Value::Int(left / right)))
        }
        "mod_int" => {
            if positional.len() != 2 {
                return Some(Err(
                    "mod_int expects exactly 2 arguments: mod_int(a, b)".to_string(),
                ));
            }

            let left = match to_i64_value(&positional[0]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "mod_int first argument must be int-convertible: {e}"
                    )))
                }
            };
            let right = match to_i64_value(&positional[1]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "mod_int second argument must be int-convertible: {e}"
                    )))
                }
            };

            if right == 0 {
                return Some(Err("mod_int division by zero".to_string()));
            }

            Some(Ok(Value::Int(left % right)))
        }
        "is_even" => {
            if positional.len() != 1 {
                return Some(Err(
                    "is_even expects exactly 1 argument: is_even(value)".to_string(),
                ));
            }

            let value = match to_i64_value(&positional[0]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "is_even argument must be int-convertible: {e}"
                    )))
                }
            };

            Some(Ok(Value::Bool(value % 2 == 0)))
        }
        "is_odd" => {
            if positional.len() != 1 {
                return Some(Err(
                    "is_odd expects exactly 1 argument: is_odd(value)".to_string(),
                ));
            }

            let value = match to_i64_value(&positional[0]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "is_odd argument must be int-convertible: {e}"
                    )))
                }
            };

            Some(Ok(Value::Bool(value % 2 != 0)))
        }
        "between_int" => {
            if positional.len() != 3 {
                return Some(Err(
                    "between_int expects exactly 3 arguments: between_int(value, min, max)"
                        .to_string(),
                ));
            }

            let value = match to_i64_value(&positional[0]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "between_int value must be int-convertible: {e}"
                    )))
                }
            };
            let mut min = match to_i64_value(&positional[1]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "between_int min must be int-convertible: {e}"
                    )))
                }
            };
            let mut max = match to_i64_value(&positional[2]) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(format!(
                        "between_int max must be int-convertible: {e}"
                    )))
                }
            };

            if min > max {
                std::mem::swap(&mut min, &mut max);
            }

            Some(Ok(Value::Bool(value >= min && value <= max)))
        }
        "type_of" | "typeof" => {
            if positional.len() != 1 {
                return Some(Err("typeOf expects exactly 1 argument".to_string()));
            }
            let ty = match &positional[0] {
                Value::Int(_) => "int",
                Value::Float(_) => "float",
                Value::Bool(_) => "boolean",
                Value::Str(_) => "string",
                Value::List(_) => "list",
                Value::Set(_) => "group",
                Value::Dict(_) => "dict",
                Value::Func(_) => "function",
                Value::Object { .. } => "object",
                Value::Module(_) => "module",
                Value::Empty => "empty",
            };
            Some(Ok(Value::Str(ty.to_string())))
        }
        "http_get" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err(
                    "http_get expects 1 or 2 arguments: http_get(url, [authorization])"
                        .to_string(),
                ));
            }
            let url = positional[0].to_string();
            let auth = positional.get(1).map(|v| v.to_string()).unwrap_or_default();
            Some(http_get_builtin(&url, &auth))
        }
        "http_post_json" => {
            if positional.len() < 2 || positional.len() > 3 {
                return Some(Err(
                    "http_post_json expects 2 or 3 arguments: http_post_json(url, payload, [authorization])"
                        .to_string(),
                ));
            }
            let url = positional[0].to_string();
            let payload = positional[1].clone();
            let auth = positional.get(2).map(|v| v.to_string()).unwrap_or_default();
            Some(http_post_json_builtin(&url, payload, &auth))
        }
        "http_put_json" => {
            if positional.len() < 2 || positional.len() > 3 {
                return Some(Err(
                    "http_put_json expects 2 or 3 arguments: http_put_json(url, payload, [authorization])"
                        .to_string(),
                ));
            }
            let url = positional[0].to_string();
            let payload = positional[1].clone();
            let auth = positional.get(2).map(|v| v.to_string()).unwrap_or_default();
            Some(http_put_json_builtin(&url, payload, &auth))
        }
        "http_patch_json" => {
            if positional.len() < 2 || positional.len() > 3 {
                return Some(Err(
                    "http_patch_json expects 2 or 3 arguments: http_patch_json(url, payload, [authorization])"
                        .to_string(),
                ));
            }
            let url = positional[0].to_string();
            let payload = positional[1].clone();
            let auth = positional.get(2).map(|v| v.to_string()).unwrap_or_default();
            Some(http_patch_json_builtin(&url, payload, &auth))
        }
        "http_delete" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err(
                    "http_delete expects 1 or 2 arguments: http_delete(url, [authorization])"
                        .to_string(),
                ));
            }
            let url = positional[0].to_string();
            let auth = positional.get(1).map(|v| v.to_string()).unwrap_or_default();
            Some(http_delete_builtin(&url, &auth))
        }
        "http_serve_dir" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err(
                    "http_serve_dir expects 1 or 2 arguments: http_serve_dir(path, [port])"
                        .to_string(),
                ));
            }
            let dir = positional[0].to_string();
            let port: u16 = if positional.len() >= 2 {
                match positional[1].to_string().parse() {
                    Ok(p) => p,
                    Err(_) => {
                        return Some(Err(
                            "http_serve_dir: port must be a number".to_string(),
                        ))
                    }
                }
            } else {
                8080
            };
            Some(http_serve_dir_builtin(&dir, port))
        }
        // ── Async I/O — run an HTTP builtin on a background thread, return a future id ──
        "http_get_async" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err("http_get_async expects 1 or 2 arguments: http_get_async(url, [authorization])".to_string()));
            }
            Some(spawn_builtin_future("http_get", positional.to_vec()))
        }
        "http_post_json_async" => {
            if positional.len() < 2 || positional.len() > 3 {
                return Some(Err("http_post_json_async expects 2 or 3 arguments: http_post_json_async(url, payload, [authorization])".to_string()));
            }
            Some(spawn_builtin_future("http_post_json", positional.to_vec()))
        }
        "http_put_json_async" => {
            if positional.len() < 2 || positional.len() > 3 {
                return Some(Err("http_put_json_async expects 2 or 3 arguments: http_put_json_async(url, payload, [authorization])".to_string()));
            }
            Some(spawn_builtin_future("http_put_json", positional.to_vec()))
        }
        "http_delete_async" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err("http_delete_async expects 1 or 2 arguments: http_delete_async(url, [authorization])".to_string()));
            }
            Some(spawn_builtin_future("http_delete", positional.to_vec()))
        }
        "gui_show_html" => {
            // gui_show_html(html, [title], [width], [height]) — opens HTML in a native window
            if positional.is_empty() || positional.len() > 4 {
                return Some(Err(
                    "gui_show_html expects 1-4 arguments: gui_show_html(html, [title], [width], [height])"
                        .to_string(),
                ));
            }
            let html = positional[0].to_string();
            let title = positional.get(1).map(|v| v.to_string()).unwrap_or_else(|| "Indent App".to_string());
            let width: i32 = positional.get(2).and_then(|v| v.to_string().parse().ok()).unwrap_or(1200);
            let height: i32 = positional.get(3).and_then(|v| v.to_string().parse().ok()).unwrap_or(800);
            return Some(gui_show_html_builtin(&html, &title, width, height));
        }
        "json_dumps" => {
            if positional.len() != 1 {
                return Some(Err("json_dumps expects exactly 1 argument".to_string()));
            }
            let json_payload = value_to_json(&positional[0]);
            match serde_json::to_string(&json_payload) {
                Ok(text) => Some(Ok(Value::Str(text))),
                Err(e) => Some(Err(format!("json_dumps failed: {e}"))),
            }
        }
        "json_loads" => {
            if positional.len() != 1 {
                return Some(Err("json_loads expects exactly 1 argument".to_string()));
            }
            let text = positional[0].to_string();
            match serde_json::from_str::<JsonValue>(&text) {
                Ok(parsed) => Some(Ok(json_to_value(&parsed))),
                Err(e) => Some(Err(format!("json_loads failed: {e}"))),
            }
        }
        "ws_connect" => {
            if positional.len() != 1 {
                return Some(Err("ws_connect expects exactly 1 argument (url)".to_string()));
            }
            let url = positional[0].to_string();
            Some(ws_connect_builtin(&url))
        }
        "ws_send_text" => {
            if positional.len() != 2 {
                return Some(Err(
                    "ws_send_text expects exactly 2 arguments (socket_id, text)".to_string(),
                ));
            }
            let socket_id = match parse_socket_id(&positional[0]) {
                Ok(id) => id,
                Err(e) => return Some(Err(e)),
            };
            let text = positional[1].to_string();
            Some(ws_send_text_builtin(socket_id, &text))
        }
        "ws_recv_text" => {
            if positional.len() != 1 {
                return Some(Err(
                    "ws_recv_text expects exactly 1 argument (socket_id)".to_string(),
                ));
            }
            let socket_id = match parse_socket_id(&positional[0]) {
                Ok(id) => id,
                Err(e) => return Some(Err(e)),
            };
            Some(ws_recv_text_builtin(socket_id))
        }
        "ws_recv_text_timeout" => {
            if positional.len() != 2 {
                return Some(Err(
                    "ws_recv_text_timeout expects exactly 2 arguments (socket_id, timeout_seconds)"
                        .to_string(),
                ));
            }
            let socket_id = match parse_socket_id(&positional[0]) {
                Ok(id) => id,
                Err(e) => return Some(Err(e)),
            };
            let timeout_seconds = match positional[1].as_number() {
                Some(value) => value,
                None => {
                    return Some(Err(
                        "ws_recv_text_timeout timeout_seconds must be numeric".to_string(),
                    ))
                }
            };
            Some(ws_recv_text_timeout_builtin(socket_id, timeout_seconds))
        }
        "ws_close" => {
            if positional.len() != 1 {
                return Some(Err("ws_close expects exactly 1 argument (socket_id)".to_string()));
            }
            let socket_id = match parse_socket_id(&positional[0]) {
                Ok(id) => id,
                Err(e) => return Some(Err(e)),
            };
            Some(ws_close_builtin(socket_id))
        }
        "sys_version" => {
            if !positional.is_empty() {
                return Some(Err("sys_version expects no arguments".to_string()));
            }
            Some(Ok(Value::Str(format!("Indent {INDENT_VERSION}"))))
        }
        "sys_executable" => {
            if !positional.is_empty() {
                return Some(Err("sys_executable expects no arguments".to_string()));
            }
            Some(sys_executable_builtin())
        }
        "sys_platform" => {
            if !positional.is_empty() {
                return Some(Err("sys_platform expects no arguments".to_string()));
            }
            Some(Ok(Value::Str(env::consts::OS.to_string())))
        }
        "sys_arch" => {
            if !positional.is_empty() {
                return Some(Err("sys_arch expects no arguments".to_string()));
            }
            Some(Ok(Value::Str(env::consts::ARCH.to_string())))
        }
        "sys_argv" => {
            if !positional.is_empty() {
                return Some(Err("sys_argv expects no arguments".to_string()));
            }
            let args = env::args().skip(1).map(Value::Str).collect::<Vec<_>>();
            Some(Ok(Value::List(args)))
        }
        "process_exit" => {
            if positional.len() != 1 {
                return Some(Err("process_exit expects exactly 1 argument (code)".to_string()));
            }
            let code = match &positional[0] {
                Value::Int(v) => *v,
                Value::Float(v) => *v as i64,
                _ => return Some(Err("process_exit expects a numeric code".to_string())),
            };
            process::exit(code as i32);
        }
        "os_getcwd" => {
            if !positional.is_empty() {
                return Some(Err("os_getcwd expects no arguments".to_string()));
            }
            Some(os_getcwd_builtin())
        }
        "os_chdir" => {
            if positional.len() != 1 {
                return Some(Err("os_chdir expects exactly 1 argument (path)".to_string()));
            }
            Some(os_chdir_builtin(&positional[0].to_string()))
        }
        "os_system" => {
            if positional.len() != 1 {
                return Some(Err("os_system expects exactly 1 argument (command)".to_string()));
            }
            Some(os_system_builtin(&positional[0].to_string()))
        }
        "os_run" => {
            if positional.len() != 1 {
                return Some(Err("os_run expects exactly 1 argument (command)".to_string()));
            }
            Some(os_run_builtin(&positional[0].to_string()))
        }
        "os_copy" => {
            if positional.len() != 2 {
                return Some(Err("os_copy expects exactly 2 arguments (source, destination)".to_string()));
            }
            Some(os_copy_builtin(
                &positional[0].to_string(),
                &positional[1].to_string(),
            ))
        }
        "os_move" => {
            if positional.len() != 2 {
                return Some(Err("os_move expects exactly 2 arguments (source, destination)".to_string()));
            }
            Some(os_move_builtin(
                &positional[0].to_string(),
                &positional[1].to_string(),
            ))
        }
        "os_copy_tree" => {
            if positional.len() != 2 {
                return Some(Err("os_copy_tree expects exactly 2 arguments (source, destination)".to_string()));
            }
            Some(os_copy_tree_builtin(
                &positional[0].to_string(),
                &positional[1].to_string(),
            ))
        }
        "file_size" => {
            if positional.len() != 1 {
                return Some(Err("file_size expects exactly 1 argument (path)".to_string()));
            }
            Some(file_size_builtin(&positional[0].to_string()))
        }
        "os_exists" => {
            if positional.len() != 1 {
                return Some(Err("os_exists expects exactly 1 argument (path)".to_string()));
            }
            Some(Ok(Value::Bool(Path::new(&positional[0].to_string()).exists())))
        }
        "os_is_file" => {
            if positional.len() != 1 {
                return Some(Err("os_is_file expects exactly 1 argument (path)".to_string()));
            }
            Some(Ok(Value::Bool(Path::new(&positional[0].to_string()).is_file())))
        }
        "os_is_dir" => {
            if positional.len() != 1 {
                return Some(Err("os_is_dir expects exactly 1 argument (path)".to_string()));
            }
            Some(Ok(Value::Bool(Path::new(&positional[0].to_string()).is_dir())))
        }
        "os_list_dir" => {
            if positional.len() != 1 {
                return Some(Err("os_list_dir expects exactly 1 argument (path)".to_string()));
            }
            Some(os_list_dir_builtin(&positional[0].to_string()))
        }
        "os_mkdir" => {
            if positional.len() != 1 {
                return Some(Err("os_mkdir expects exactly 1 argument (path)".to_string()));
            }
            Some(os_mkdir_builtin(&positional[0].to_string()))
        }
        "os_remove" => {
            if positional.len() != 1 {
                return Some(Err("os_remove expects exactly 1 argument (path)".to_string()));
            }
            Some(os_remove_builtin(&positional[0].to_string()))
        }
        "os_rename" => {
            if positional.len() != 2 {
                return Some(Err("os_rename expects exactly 2 arguments (source, destination)".to_string()));
            }
            Some(os_rename_builtin(
                &positional[0].to_string(),
                &positional[1].to_string(),
            ))
        }
        "os_getenv" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err("os_getenv expects 1 or 2 arguments (key, [default])".to_string()));
            }
            let key = positional[0].to_string();
            let default = positional.get(1).map(|v| v.to_string()).unwrap_or_default();
            Some(Ok(Value::Str(env::var(&key).unwrap_or(default))))
        }
        "os_setenv" => {
            if positional.len() != 2 {
                return Some(Err("os_setenv expects exactly 2 arguments (key, value)".to_string()));
            }
            let key = positional[0].to_string();
            let value = positional[1].to_string();
            // SAFETY: Indent runtime uses this in single-process script execution context.
            unsafe {
                env::set_var(key, value);
            }
            Some(Ok(Value::Empty))
        }
        "os_environ" => {
            if !positional.is_empty() {
                return Some(Err("os_environ expects no arguments".to_string()));
            }
            Some(Ok(os_environ_builtin()))
        }
        "file_read_text" => {
            if positional.len() != 1 {
                return Some(Err("file_read_text expects exactly 1 argument (path)".to_string()));
            }
            Some(file_read_text_builtin(&positional[0].to_string()))
        }
        "file_write_text" => {
            if positional.len() != 2 {
                return Some(Err("file_write_text expects exactly 2 arguments (path, text)".to_string()));
            }
            Some(file_write_text_builtin(
                &positional[0].to_string(),
                &positional[1].to_string(),
            ))
        }
        "file_append_text" => {
            if positional.len() != 2 {
                return Some(Err("file_append_text expects exactly 2 arguments (path, text)".to_string()));
            }
            Some(file_append_text_builtin(
                &positional[0].to_string(),
                &positional[1].to_string(),
            ))
        }
        "time_now" => {
            if !positional.is_empty() {
                return Some(Err("time_now expects no arguments".to_string()));
            }
            Some(time_now_builtin())
        }
        "time_sleep" => {
            if positional.len() != 1 {
                return Some(Err("time_sleep expects exactly 1 argument (seconds)".to_string()));
            }
            let seconds = match positional[0].as_number() {
                Some(v) if v >= 0.0 => v,
                Some(_) => return Some(Err("time_sleep seconds must be >= 0".to_string())),
                None => return Some(Err("time_sleep expects a numeric seconds argument".to_string())),
            };
            Some(time_sleep_builtin(seconds))
        }
        "time_perf_counter" => {
            if !positional.is_empty() {
                return Some(Err("time_perf_counter expects no arguments".to_string()));
            }
            Some(Ok(Value::Float(PERF_START.elapsed().as_secs_f64())))
        }
        "random_seed" => {
            if positional.len() != 1 {
                return Some(Err("random_seed expects exactly 1 argument".to_string()));
            }
            let seed = match &positional[0] {
                Value::Int(v) => *v,
                Value::Float(v) => *v as i64,
                Value::Str(s) => match s.trim().parse::<i64>() {
                    Ok(v) => v,
                    Err(_) => return Some(Err("random_seed requires int-like value".to_string())),
                },
                _ => return Some(Err("random_seed requires int-like value".to_string())),
            };
            if seed == 0 {
                RNG_STATE.store(0x4d595df4d0f33173u64 as i64, Ordering::SeqCst);
            } else {
                RNG_STATE.store(seed, Ordering::SeqCst);
            }
            Some(Ok(Value::Empty))
        }
        "random_float" => {
            if !positional.is_empty() {
                return Some(Err("random_float expects no arguments".to_string()));
            }
            Some(Ok(Value::Float(random_next_f64())))
        }
        "random_int" => {
            if positional.len() != 2 {
                return Some(Err("random_int expects exactly 2 arguments (a, b)".to_string()));
            }
            let a = match &positional[0] {
                Value::Int(v) => *v,
                _ => return Some(Err("random_int expects integer arguments".to_string())),
            };
            let b = match &positional[1] {
                Value::Int(v) => *v,
                _ => return Some(Err("random_int expects integer arguments".to_string())),
            };
            Some(random_int_builtin(a, b))
        }
        "random_choice" => {
            if positional.len() != 1 {
                return Some(Err("random_choice expects exactly 1 argument (list)".to_string()));
            }
            Some(random_choice_builtin(&positional[0]))
        }
        "random_shuffle" => {
            if positional.len() != 1 {
                return Some(Err("random_shuffle expects exactly 1 argument (list)".to_string()));
            }
            Some(random_shuffle_builtin(&positional[0]))
        }
        "math_abs" => {
            if positional.len() != 1 {
                return Some(Err("math_abs expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.abs()))
        }
        "math_sqrt" => {
            if positional.len() != 1 {
                return Some(Err("math_sqrt expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.sqrt()))
        }
        "math_pow" => {
            if positional.len() != 2 {
                return Some(Err("math_pow expects exactly 2 arguments".to_string()));
            }
            Some(math_binary_float_builtin(&positional[0], &positional[1], |a, b| a.powf(b)))
        }
        "math_floor" => {
            if positional.len() != 1 {
                return Some(Err("math_floor expects exactly 1 argument".to_string()));
            }
            Some(math_floor_builtin(&positional[0]))
        }
        "math_ceil" => {
            if positional.len() != 1 {
                return Some(Err("math_ceil expects exactly 1 argument".to_string()));
            }
            Some(math_ceil_builtin(&positional[0]))
        }
        "math_round" => {
            if positional.len() != 2 {
                return Some(Err("math_round expects exactly 2 arguments (value, digits)".to_string()));
            }
            Some(math_round_builtin(&positional[0], &positional[1]))
        }
        "math_sin" => {
            if positional.len() != 1 {
                return Some(Err("math_sin expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.sin()))
        }
        "math_cos" => {
            if positional.len() != 1 {
                return Some(Err("math_cos expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.cos()))
        }
        "math_tan" => {
            if positional.len() != 1 {
                return Some(Err("math_tan expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.tan()))
        }
        "math_asin" => {
            if positional.len() != 1 {
                return Some(Err("math_asin expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.asin()))
        }
        "math_acos" => {
            if positional.len() != 1 {
                return Some(Err("math_acos expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.acos()))
        }
        "math_atan" => {
            if positional.len() != 1 {
                return Some(Err("math_atan expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.atan()))
        }
        "math_atan2" => {
            if positional.len() != 2 {
                return Some(Err("math_atan2 expects exactly 2 arguments".to_string()));
            }
            Some(math_binary_float_builtin(&positional[0], &positional[1], |a, b| a.atan2(b)))
        }
        "math_log" => {
            if positional.len() != 2 {
                return Some(Err("math_log expects exactly 2 arguments (value, base)".to_string()));
            }
            Some(math_binary_float_builtin(&positional[0], &positional[1], |a, b| a.log(b)))
        }
        "math_log10" => {
            if positional.len() != 1 {
                return Some(Err("math_log10 expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.log10()))
        }
        "math_exp" => {
            if positional.len() != 1 {
                return Some(Err("math_exp expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.exp()))
        }
        "python_available" => {
            if !positional.is_empty() {
                return Some(Err("python_available expects no arguments".to_string()));
            }
            Some(Ok(Value::Bool(find_python_command().is_some())))
        }
        "python_exec" => {
            if positional.len() != 1 {
                return Some(Err("python_exec expects exactly 1 argument (code)".to_string()));
            }
            Some(python_exec_builtin(&positional[0].to_string()))
        }
        "python_eval" => {
            if positional.len() != 1 {
                return Some(Err("python_eval expects exactly 1 argument (expression)".to_string()));
            }
            Some(python_eval_builtin(&positional[0].to_string()))
        }
        "python_eval_json" => {
            if positional.len() != 1 {
                return Some(Err("python_eval_json expects exactly 1 argument (expression)".to_string()));
            }
            Some(python_eval_json_builtin(&positional[0].to_string()))
        }
        "python_run_file" => {
            if positional.len() != 1 {
                return Some(Err("python_run_file expects exactly 1 argument (path)".to_string()));
            }
            Some(python_run_file_builtin(&positional[0].to_string()))
        }
        // ── Regex (v2.6.0) ──────────────────────────────────────
        "regex_match" => {
            if positional.len() != 2 {
                return Some(Err("regex_match expects exactly 2 arguments: regex_match(pattern, text)".to_string()));
            }
            let pattern = positional[0].to_string();
            let text = positional[1].to_string();
            match regex::Regex::new(&pattern) {
                Ok(re) => Some(Ok(Value::Bool(re.is_match(&text)))),
                Err(e) => Some(Err(format!("regex_match: invalid pattern: {e}"))),
            }
        }
        "regex_search" => {
            if positional.len() != 2 {
                return Some(Err("regex_search expects exactly 2 arguments: regex_search(pattern, text)".to_string()));
            }
            let pattern = positional[0].to_string();
            let text = positional[1].to_string();
            match regex::Regex::new(&pattern) {
                Ok(re) => match re.find(&text) {
                    Some(m) => {
                        let mut map = HashMap::new();
                        map.insert("start".to_string(), Value::Int(m.start() as i64));
                        map.insert("end".to_string(), Value::Int(m.end() as i64));
                        map.insert("text".to_string(), Value::Str(m.as_str().to_string()));
                        Some(Ok(Value::Dict(map)))
                    }
                    None => Some(Ok(Value::Empty)),
                },
                Err(e) => Some(Err(format!("regex_search: invalid pattern: {e}"))),
            }
        }
        "regex_findall" => {
            if positional.len() != 2 {
                return Some(Err("regex_findall expects exactly 2 arguments: regex_findall(pattern, text)".to_string()));
            }
            let pattern = positional[0].to_string();
            let text = positional[1].to_string();
            match regex::Regex::new(&pattern) {
                Ok(re) => {
                    let matches: Vec<Value> = re.find_iter(&text)
                        .map(|m| Value::Str(m.as_str().to_string()))
                        .collect();
                    Some(Ok(Value::List(matches)))
                },
                Err(e) => Some(Err(format!("regex_findall: invalid pattern: {e}"))),
            }
        }
        "regex_replace" => {
            if positional.len() != 3 {
                return Some(Err("regex_replace expects exactly 3 arguments: regex_replace(pattern, replacement, text)".to_string()));
            }
            let pattern = positional[0].to_string();
            let replacement = positional[1].to_string();
            let text = positional[2].to_string();
            match regex::Regex::new(&pattern) {
                Ok(re) => Some(Ok(Value::Str(re.replace_all(&text, replacement.as_str()).to_string()))),
                Err(e) => Some(Err(format!("regex_replace: invalid pattern: {e}"))),
            }
        }
        "regex_split" => {
            if positional.len() != 2 {
                return Some(Err("regex_split expects exactly 2 arguments: regex_split(pattern, text)".to_string()));
            }
            let pattern = positional[0].to_string();
            let text = positional[1].to_string();
            match regex::Regex::new(&pattern) {
                Ok(re) => {
                    let parts: Vec<Value> = re.split(&text).map(|s| Value::Str(s.to_string())).collect();
                    Some(Ok(Value::List(parts)))
                },
                Err(e) => Some(Err(format!("regex_split: invalid pattern: {e}"))),
            }
        }
        // ── Datetime (v2.6.0) ──────────────────────────────────
        "time_utc" => {
            if !positional.is_empty() {
                return Some(Err("time_utc expects no arguments".to_string()));
            }
            Some(time_utc_builtin())
        }
        "time_format" => {
            if positional.len() < 1 || positional.len() > 2 {
                return Some(Err("time_format expects 1 or 2 arguments: time_format(timestamp, [format])".to_string()));
            }
            Some(time_format_builtin(&positional[0], positional.get(1)))
        }
        "time_parse" => {
            if positional.len() < 1 || positional.len() > 2 {
                return Some(Err("time_parse expects 1 or 2 arguments: time_parse(datetime_str, [format])".to_string()));
            }
            Some(time_parse_builtin(&positional[0], positional.get(1)))
        }
        // ── String extras ──────────────────────────────────────
        "pad_left" => {
            if positional.len() != 3 {
                return Some(Err("pad_left expects exactly 3 arguments: pad_left(text, width, char)".to_string()));
            }
            let text = positional[0].to_string();
            let width = match &positional[1] { Value::Int(v) => *v as usize, _ => return Some(Err("pad_left width must be int".to_string())) };
            let pad = positional[2].to_string();
            let pad_char = pad.chars().next().unwrap_or(' ');
            let len = text.chars().count();
            if len >= width {
                Some(Ok(Value::Str(text)))
            } else {
                let padding: String = std::iter::repeat(pad_char).take(width - len).collect();
                Some(Ok(Value::Str(format!("{}{}", padding, text))))
            }
        }
        "pad_right" => {
            if positional.len() != 3 {
                return Some(Err("pad_right expects exactly 3 arguments: pad_right(text, width, char)".to_string()));
            }
            let text = positional[0].to_string();
            let width = match &positional[1] { Value::Int(v) => *v as usize, _ => return Some(Err("pad_right width must be int".to_string())) };
            let pad = positional[2].to_string();
            let pad_char = pad.chars().next().unwrap_or(' ');
            let len = text.chars().count();
            if len >= width {
                Some(Ok(Value::Str(text)))
            } else {
                let padding: String = std::iter::repeat(pad_char).take(width - len).collect();
                Some(Ok(Value::Str(format!("{}{}", text, padding))))
            }
        }
        "repeat_str" => {
            if positional.len() != 2 {
                return Some(Err("repeat_str expects exactly 2 arguments: repeat_str(text, count)".to_string()));
            }
            let text = positional[0].to_string();
            let count = match &positional[1] { Value::Int(v) => *v as usize, _ => return Some(Err("repeat_str count must be int".to_string())) };
            Some(Ok(Value::Str(text.repeat(count))))
        }
        // ── String format (v1.3) ──────────────────────────────
        "format" => {
            if positional.is_empty() {
                return Some(Err("format expects at least 1 argument: format(template, values...)".to_string()));
            }
            let template = positional[0].to_string();
            let mut result = template.clone();
            // Replace {0}, {1}, etc. with positional arguments
            for (i, val) in positional.iter().enumerate().skip(1) {
                let placeholder = format!("{{{}}}", i - 1);
                result = result.replace(&placeholder, &val.to_string());
            }
            Some(Ok(Value::Str(result)))
        }
        "sformat" => {
            if positional.len() < 2 {
                return Some(Err("sformat expects at least 2 arguments: sformat(template, key=value, ...)".to_string()));
            }
            let template = positional[0].to_string();
            let mut result = template.clone();
            // Replace {key} with named values from named args (passed as positional pairs)
            let mut i = 1;
            while i + 1 < positional.len() {
                let key = positional[i].to_string();
                let val = positional[i + 1].to_string();
                let placeholder = format!("{{{}}}", key);
                result = result.replace(&placeholder, &val);
                i += 2;
            }
            Some(Ok(Value::Str(result)))
        }
        // ── UUID ───────────────────────────────────────────────
        "uuid" => {
            if !positional.is_empty() {
                return Some(Err("uuid expects no arguments".to_string()));
            }
            Some(Ok(Value::Str(uuid_v4())))
        }
        // ── Base64 ─────────────────────────────────────────────
        "base64_encode" => {
            if positional.len() != 1 {
                return Some(Err("base64_encode expects exactly 1 argument: base64_encode(text)".to_string()));
            }
            let text = positional[0].to_string();
            Some(Ok(Value::Str(base64_encode(&text))))
        }
        "base64_decode" => {
            if positional.len() != 1 {
                return Some(Err("base64_decode expects exactly 1 argument: base64_decode(text)".to_string()));
            }
            let text = positional[0].to_string();
            match base64_decode(&text) {
                Ok(s) => Some(Ok(Value::Str(s))),
                Err(e) => Some(Err(e)),
            }
        }
        // ── File glob ──────────────────────────────────────────
        "glob" => {
            if positional.len() != 1 {
                return Some(Err("glob expects exactly 1 argument: glob(pattern)".to_string()));
            }
            let pattern = positional[0].to_string();
            Some(Ok(Value::List(simple_glob(&pattern))))
        }
        // Recursive file walk (os.walk / glob ** style)
        "walk" => {
            if positional.len() != 1 {
                return Some(Err("walk expects exactly 1 argument (path)".to_string()));
            }
            Some(walk_builtin(&positional[0].to_string()))
        }
        // ── CSV ──────────────────────────────────────────────
        "csv_read" => {
            if positional.len() != 1 {
                return Some(Err("csv_read expects exactly 1 argument (path)".to_string()));
            }
            Some(csv_read_builtin(&positional[0].to_string()))
        }
        "csv_write" => {
            if positional.len() != 2 {
                return Some(Err("csv_write expects exactly 2 arguments (path, rows)".to_string()));
            }
            Some(csv_write_builtin(&positional[0].to_string(), &positional[1]))
        }
        // ── SQLite ───────────────────────────────────────────
        "sqlite_exec" => {
            if positional.len() != 2 {
                return Some(Err("sqlite_exec expects exactly 2 arguments (path, sql)".to_string()));
            }
            Some(sqlite_exec_builtin(
                &positional[0].to_string(),
                &positional[1].to_string(),
            ))
        }
        "sqlite_query" => {
            if positional.len() != 2 {
                return Some(Err("sqlite_query expects exactly 2 arguments (path, sql)".to_string()));
            }
            Some(sqlite_query_builtin(
                &positional[0].to_string(),
                &positional[1].to_string(),
            ))
        }
        "sqlite_query_one" => {
            if positional.len() != 2 {
                return Some(Err("sqlite_query_one expects exactly 2 arguments (path, sql)".to_string()));
            }
            Some(sqlite_query_one_builtin(
                &positional[0].to_string(),
                &positional[1].to_string(),
            ))
        }
        // ── Typed errors ─────────────────────────────────────
        "error_type" => {
            if positional.len() != 1 {
                return Some(Err("error_type expects exactly 1 argument (error string)".to_string()));
            }
            Some(error_type_builtin(&positional[0].to_string()))
        }
        "error_message" => {
            if positional.len() != 1 {
                return Some(Err("error_message expects exactly 1 argument (error string)".to_string()));
            }
            Some(error_message_builtin(&positional[0].to_string()))
        }
        // ── Collections / utility ────────────────────────────
        "counter" => {
            if positional.len() != 1 {
                return Some(Err("counter expects exactly 1 argument (list)".to_string()));
            }
            Some(counter_builtin(&positional[0]))
        }
        "log" => {
            if positional.len() != 2 {
                return Some(Err("log expects exactly 2 arguments (level, message)".to_string()));
            }
            Some(log_builtin(
                &positional[0].to_string(),
                &positional[1].to_string(),
            ))
        }
        // ── TOML ──────────────────────────────────────────────
        "toml_loads" => {
            if positional.len() != 1 {
                return Some(Err("toml_loads expects exactly 1 argument (text)".to_string()));
            }
            Some(toml_loads_builtin(&positional[0].to_string()))
        }
        "toml_dumps" => {
            if positional.len() != 1 {
                return Some(Err("toml_dumps expects exactly 1 argument (dict)".to_string()));
            }
            Some(toml_dumps_builtin(&positional[0]))
        }
        // ── YAML ──────────────────────────────────────────────
        "yaml_loads" => {
            if positional.len() != 1 {
                return Some(Err("yaml_loads expects exactly 1 argument (text)".to_string()));
            }
            Some(yaml_loads_builtin(&positional[0].to_string()))
        }
        "yaml_dumps" => {
            if positional.len() != 1 {
                return Some(Err("yaml_dumps expects exactly 1 argument (dict)".to_string()));
            }
            Some(yaml_dumps_builtin(&positional[0]))
        }
        // ── Compression ───────────────────────────────────────
        "gzip_compress" => {
            if positional.len() != 1 {
                return Some(Err("gzip_compress expects exactly 1 argument (text)".to_string()));
            }
            Some(gzip_compress_builtin(&positional[0].to_string()))
        }
        "gzip_decompress" => {
            if positional.len() != 1 {
                return Some(Err("gzip_decompress expects exactly 1 argument (base64)".to_string()));
            }
            Some(gzip_decompress_builtin(&positional[0].to_string()))
        }
        // ── Zip ──────────────────────────────────────────────
        "zip_list" => {
            if positional.len() != 1 {
                return Some(Err("zip_list expects exactly 1 argument (path)".to_string()));
            }
            Some(zip_list_builtin(&positional[0].to_string()))
        }
        "zip_extract" => {
            if positional.len() != 2 {
                return Some(Err("zip_extract expects exactly 2 arguments (path, destination)".to_string()));
            }
            Some(zip_extract_builtin(
                &positional[0].to_string(),
                &positional[1].to_string(),
            ))
        }
        // ── Path helpers ───────────────────────────────────────
        "path_join" => {
            if positional.len() < 2 {
                return Some(Err("path_join expects at least 2 arguments".to_string()));
            }
            let mut path = PathBuf::from(positional[0].to_string());
            for p in &positional[1..] {
                path = path.join(p.to_string());
            }
            Some(Ok(Value::Str(path.to_string_lossy().to_string())))
        }
        "path_basename" => {
            if positional.len() != 1 {
                return Some(Err("path_basename expects exactly 1 argument".to_string()));
            }
            let path_str = positional[0].to_string();
            let p = Path::new(&path_str);
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            Some(Ok(Value::Str(name)))
        }
        "path_dirname" => {
            if positional.len() != 1 {
                return Some(Err("path_dirname expects exactly 1 argument".to_string()));
            }
            let path_str = positional[0].to_string();
            let p = Path::new(&path_str);
            let parent = p.parent().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            Some(Ok(Value::Str(parent)))
        }
        "path_ext" => {
            if positional.len() != 1 {
                return Some(Err("path_ext expects exactly 1 argument".to_string()));
            }
            let path_str = positional[0].to_string();
            let p = Path::new(&path_str);
            let ext = p.extension().map(|n| format!(".{}", n.to_string_lossy())).unwrap_or_default();
            Some(Ok(Value::Str(ext)))
        }
        "path_stem" => {
            if positional.len() != 1 {
                return Some(Err("path_stem expects exactly 1 argument".to_string()));
            }
            let path_str = positional[0].to_string();
            let p = Path::new(&path_str);
            let fname = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let stem = match p.extension() {
                Some(_) => {
                    let f = Path::new(&fname);
                    f.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                }
                None => fname,
            };
            Some(Ok(Value::Str(stem)))
        }
        "path_abs" => {
            if positional.len() != 1 {
                return Some(Err("path_abs expects exactly 1 argument".to_string()));
            }
            let p = PathBuf::from(positional[0].to_string());
            let abs = if p.is_absolute() {
                p
            } else {
                std::env::current_dir().map(|c| c.join(&p)).unwrap_or(p)
            };
            Some(Ok(Value::Str(abs.to_string_lossy().to_string())))
        }
        "path_expand" => {
            if positional.len() != 1 {
                return Some(Err("path_expand expects exactly 1 argument".to_string()));
            }
            let raw = positional[0].to_string();
            let expanded = if raw == "~" || raw.starts_with("~/") {
                if let Some(home) = std::env::var_os("HOME") {
                    let home = home.to_string_lossy().to_string();
                    if raw == "~" { home } else { format!("{}/{}", home, &raw[2..]) }
                } else {
                    raw
                }
            } else {
                raw
            };
            Some(Ok(Value::Str(expanded)))
        }
        "path_norm" => {
            if positional.len() != 1 {
                return Some(Err("path_norm expects exactly 1 argument".to_string()));
            }
            let p = PathBuf::from(positional[0].to_string());
            let mut parts: Vec<String> = Vec::new();
            for comp in p.components() {
                match comp {
                    std::path::Component::RootDir => {}
                    std::path::Component::ParentDir => { parts.pop(); }
                    std::path::Component::CurDir => {}
                    other => parts.push(other.as_os_str().to_string_lossy().to_string()),
                }
            }
            let joined = parts.join("/");
            let out = if p.is_absolute() { format!("/{}", joined) } else { joined };
            Some(Ok(Value::Str(out)))
        }
        // ── Hash text ──────────────────────────────────────────
        "hash_sha256" => {
            if positional.len() != 1 {
                return Some(Err("hash_sha256 expects exactly 1 argument".to_string()));
            }
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            hasher.update(positional[0].to_string().as_bytes());
            let hash = format!("{:x}", hasher.finalize());
            Some(Ok(Value::Str(hash)))
        }
        // ── Map / Filter ───────────────────────────────────────
        "map" => {
            if positional.len() != 2 {
                return Some(Err("map expects exactly 2 arguments: map(list, function_name)".to_string()));
            }
            let items = match &positional[0] { Value::List(v) => v.clone(), _ => return Some(Err("map expects a list".to_string())) };
            let func_name = positional[1].to_string();
            let mut out = Vec::with_capacity(items.len());
            for item in &items {
                // Note: map can't call user functions from builtins — needs runtime context
                // For now, try builtin lookup
                match invoke_builtin(&func_name, &[item.clone()]) {
                    Some(Ok(result)) => out.push(result),
                    Some(Err(e)) => return Some(Err(e)),
                    None => return Some(Err(format!("map: function '{}' not found", func_name))),
                }
            }
            Some(Ok(Value::List(out)))
        }
        "filter" => {
            if positional.len() != 2 {
                return Some(Err("filter expects exactly 2 arguments: filter(list, function_name)".to_string()));
            }
            let items = match &positional[0] { Value::List(v) => v.clone(), _ => return Some(Err("filter expects a list".to_string())) };
            let func_name = positional[1].to_string();
            let mut out = Vec::new();
            for item in &items {
                match invoke_builtin(&func_name, &[item.clone()]) {
                    Some(Ok(result)) if result.to_bool() => out.push(item.clone()),
                    Some(Err(e)) => return Some(Err(e)),
                    _ => {}
                }
            }
            Some(Ok(Value::List(out)))
        }
        // ── Async tasks ───────────────────────────────────────
        "task_wait" => {
            if positional.len() != 1 {
                return Some(Err("task_wait expects exactly 1 argument (task id)".to_string()));
            }
            match &positional[0] {
                Value::Int(id) => Some(task_wait_builtin(*id)),
                _ => Some(Err("task_wait expects an integer task id".to_string())),
            }
        }
        "task_done" => {
            if positional.len() != 1 {
                return Some(Err("task_done expects exactly 1 argument (task id)".to_string()));
            }
            match &positional[0] {
                Value::Int(id) => Some(task_done_builtin(*id)),
                _ => Some(Err("task_done expects an integer task id".to_string())),
            }
        }
        "task_result" => {
            if positional.len() != 1 {
                return Some(Err("task_result expects exactly 1 argument (task id)".to_string()));
            }
            match &positional[0] {
                Value::Int(id) => Some(task_result_builtin(*id)),
                _ => Some(Err("task_result expects an integer task id".to_string())),
            }
        }
        "task_wait_all" => {
            if positional.len() != 1 {
                return Some(Err("task_wait_all expects exactly 1 argument (list of task ids)".to_string()));
            }
            Some(task_wait_all_builtin(&positional[0]))
        }
        "task_wait_timeout" => {
            if positional.len() != 2 {
                return Some(Err("task_wait_timeout expects exactly 2 arguments (task id, seconds)".to_string()));
            }
            match (&positional[0], &positional[1]) {
                (Value::Int(id), Value::Float(secs)) => Some(task_wait_timeout_builtin(*id, *secs)),
                (Value::Int(id), Value::Int(secs)) => Some(task_wait_timeout_builtin(*id, *secs as f64)),
                _ => Some(Err("task_wait_timeout expects (int task id, number seconds)".to_string())),
            }
        }
        // ── Async futures (Python-flavored) ──────────────────
        "future_done" => {
            if positional.len() != 1 {
                return Some(Err("future_done expects exactly 1 argument (future id)".to_string()));
            }
            match &positional[0] {
                Value::Int(id) => Some(task_done_builtin(*id)),
                _ => Some(Err("future_done expects an integer future id".to_string())),
            }
        }
        "future_result" => {
            if positional.len() != 1 {
                return Some(Err("future_result expects exactly 1 argument (future id)".to_string()));
            }
            match &positional[0] {
                Value::Int(id) => Some(task_result_builtin(*id)),
                _ => Some(Err("future_result expects an integer future id".to_string())),
            }
        }
        "future_cancel" => {
            if positional.len() != 1 {
                return Some(Err("future_cancel expects exactly 1 argument (future id)".to_string()));
            }
            match &positional[0] {
                Value::Int(id) => Some(future_cancel_builtin(*id)),
                _ => Some(Err("future_cancel expects an integer future id".to_string())),
            }
        }
        "gather" => {
            if positional.is_empty() {
                return Some(Err("gather expects at least 1 future id".to_string()));
            }
            Some(gather_builtin(&positional))
        }
        "sleep" => {
            if positional.len() != 1 {
                return Some(Err("sleep expects exactly 1 argument (seconds)".to_string()));
            }
            match &positional[0] {
                Value::Float(secs) => Some(sleep_builtin(*secs)),
                Value::Int(secs) => Some(sleep_builtin(*secs as f64)),
                _ => Some(Err("sleep expects a number of seconds".to_string())),
            }
        }
        "future_wait_for" => {
            if positional.len() != 2 {
                return Some(Err("future_wait_for expects exactly 2 arguments (future id, seconds)".to_string()));
            }
            match (&positional[0], &positional[1]) {
                (Value::Int(id), Value::Float(secs)) => Some(task_wait_timeout_builtin(*id, *secs)),
                (Value::Int(id), Value::Int(secs)) => Some(task_wait_timeout_builtin(*id, *secs as f64)),
                _ => Some(Err("future_wait_for expects (int future id, number seconds)".to_string())),
            }
        }
        _ => None,
    }
}

fn simple_glob(pattern: &str) -> Vec<Value> {
    let mut results = Vec::new();
    let p = Path::new(pattern);
    // Determine base directory and pattern
    let (base_dir, file_pattern) = if let Some(parent) = p.parent() {
        if parent.as_os_str().is_empty() {
            (PathBuf::from("."), pattern.to_string())
        } else {
            (parent.to_path_buf(), p.file_name().unwrap_or_default().to_string_lossy().to_string())
        }
    } else {
        (PathBuf::from("."), pattern.to_string())
    };
    // Simple glob: only supports * at end or beginning, and exact matches
    if let Ok(entries) = std::fs::read_dir(&base_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let matches = if file_pattern == "*" {
                true
            } else if file_pattern.starts_with('*') {
                name.ends_with(&file_pattern[1..])
            } else if file_pattern.ends_with('*') {
                name.starts_with(&file_pattern[..file_pattern.len()-1])
            } else {
                name == file_pattern
            };
            if matches {
                if let Ok(full) = entry.path().canonicalize() {
                    results.push(Value::Str(full.to_string_lossy().to_string()));
                } else {
                    results.push(Value::Str(entry.path().to_string_lossy().to_string()));
                }
            }
        }
    }
    results
}

fn sys_executable_builtin() -> Result<Value, String> {
    let exe = env::current_exe().map_err(|e| format!("sys_executable failed: {e}"))?;
    Ok(Value::Str(exe.to_string_lossy().to_string()))
}

fn os_getcwd_builtin() -> Result<Value, String> {
    let cwd = env::current_dir().map_err(|e| format!("os_getcwd failed: {e}"))?;
    Ok(Value::Str(cwd.to_string_lossy().to_string()))
}

fn os_chdir_builtin(path: &str) -> Result<Value, String> {
    env::set_current_dir(path).map_err(|e| format!("os_chdir failed: {e}"))?;
    Ok(Value::Empty)
}

fn os_system_builtin(command: &str) -> Result<Value, String> {
    #[cfg(target_os = "windows")]
    let status = Command::new("cmd").args(["/C", command]).status();
    #[cfg(not(target_os = "windows"))]
    let status = Command::new("sh").args(["-c", command]).status();

    let status = status.map_err(|e| format!("os_system failed: {e}"))?;
    Ok(Value::Int(status.code().unwrap_or(1) as i64))
}

/// Run a shell command and capture its output — fills the `subprocess` gap.
/// Returns a dict `{ok, status, stdout, stderr}`.
fn os_run_builtin(command: &str) -> Result<Value, String> {
    #[cfg(target_os = "windows")]
    let output = Command::new("cmd").args(["/C", command]).output();
    #[cfg(not(target_os = "windows"))]
    let output = Command::new("sh").args(["-c", command]).output();

    let output = output.map_err(|e| format!("os_run failed: {e}"))?;
    let mut out = HashMap::new();
    out.insert(
        "ok".to_string(),
        Value::Bool(output.status.success()),
    );
    out.insert(
        "status".to_string(),
        Value::Int(output.status.code().unwrap_or(1) as i64),
    );
    out.insert(
        "stdout".to_string(),
        Value::Str(String::from_utf8_lossy(&output.stdout).to_string()),
    );
    out.insert(
        "stderr".to_string(),
        Value::Str(String::from_utf8_lossy(&output.stderr).to_string()),
    );
    Ok(Value::Dict(out))
}

fn os_copy_builtin(src: &str, dst: &str) -> Result<Value, String> {
    fs::copy(src, dst).map_err(|e| format!("os_copy failed: {e}"))?;
    Ok(Value::Empty)
}

fn os_move_builtin(src: &str, dst: &str) -> Result<Value, String> {
    fs::rename(src, dst).map_err(|e| format!("os_move failed: {e}"))?;
    Ok(Value::Empty)
}

/// Recursively copy a directory tree (shutil.copytree equivalent).
fn os_copy_tree_builtin(src: &str, dst: &str) -> Result<Value, String> {
    fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            let to = dst.join(entry.file_name());
            if ft.is_dir() {
                copy_dir(&entry.path(), &to)?;
            } else {
                fs::copy(entry.path(), &to)?;
            }
        }
        Ok(())
    }
    copy_dir(Path::new(src), Path::new(dst))
        .map_err(|e| format!("os_copy_tree failed: {e}"))?;
    Ok(Value::Empty)
}

fn file_size_builtin(path: &str) -> Result<Value, String> {
    let meta = fs::metadata(path).map_err(|e| format!("file_size failed: {e}"))?;
    Ok(Value::Int(meta.len() as i64))
}

fn find_python_command() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|candidate| {
        Command::new(candidate)
            .arg("--version")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    })
}

fn python_exec_builtin(code: &str) -> Result<Value, String> {
    let python = find_python_command().ok_or_else(|| {
        "python_exec failed: no python interpreter found (expected python3 or python)".to_string()
    })?;

    let script = r#"
import os

_code = os.environ.get('INDENT_PY_CODE', '')
exec(_code)
"#;

    let output = Command::new(python)
        .arg("-c")
        .arg(script)
        .env("INDENT_PY_CODE", code)
        .output()
        .map_err(|e| format!("python_exec failed: {e}"))?;

    let mut out = HashMap::new();
    out.insert("ok".to_string(), Value::Bool(output.status.success()));
    out.insert(
        "status".to_string(),
        Value::Int(output.status.code().unwrap_or(1) as i64),
    );
    out.insert(
        "stdout".to_string(),
        Value::Str(String::from_utf8_lossy(&output.stdout).to_string()),
    );
    out.insert(
        "stderr".to_string(),
        Value::Str(String::from_utf8_lossy(&output.stderr).to_string()),
    );
    Ok(Value::Dict(out))
}

fn python_eval_builtin(expression: &str) -> Result<Value, String> {
    let python = find_python_command().ok_or_else(|| {
        "python_eval failed: no python interpreter found (expected python3 or python)".to_string()
    })?;

    let script = r#"
import os

_expr = os.environ.get('INDENT_PY_EXPR', '')
_value = eval(_expr)
print(repr(_value))
"#;

    let output = Command::new(python)
        .arg("-c")
        .arg(script)
        .env("INDENT_PY_EXPR", expression)
        .output()
        .map_err(|e| format!("python_eval failed: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!(
            "python_eval failed with status {}: {}",
            output.status.code().unwrap_or(1),
            err.trim()
        ));
    }

    Ok(Value::Str(
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string(),
    ))
}

fn python_eval_json_builtin(expression: &str) -> Result<Value, String> {
    let python = find_python_command().ok_or_else(|| {
        "python_eval_json failed: no python interpreter found (expected python3 or python)"
            .to_string()
    })?;

    let script = r#"
import json
import os

_expr = os.environ.get('INDENT_PY_EXPR', '')
_value = eval(_expr)
print(json.dumps(_value))
"#;

    let output = Command::new(python)
        .arg("-c")
        .arg(script)
        .env("INDENT_PY_EXPR", expression)
        .output()
        .map_err(|e| format!("python_eval_json failed: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!(
            "python_eval_json failed with status {}: {}",
            output.status.code().unwrap_or(1),
            err.trim()
        ));
    }

    let text = String::from_utf8_lossy(&output.stdout).trim_end().to_string();
    let parsed = serde_json::from_str::<JsonValue>(&text)
        .map_err(|e| format!("python_eval_json produced non-JSON output: {e}"))?;
    Ok(json_to_value(&parsed))
}

fn python_run_file_builtin(path: &str) -> Result<Value, String> {
    let python = find_python_command().ok_or_else(|| {
        "python_run_file failed: no python interpreter found (expected python3 or python)"
            .to_string()
    })?;

    let output = Command::new(python)
        .arg(path)
        .output()
        .map_err(|e| format!("python_run_file failed: {e}"))?;

    let mut out = HashMap::new();
    out.insert("ok".to_string(), Value::Bool(output.status.success()));
    out.insert(
        "status".to_string(),
        Value::Int(output.status.code().unwrap_or(1) as i64),
    );
    out.insert(
        "stdout".to_string(),
        Value::Str(String::from_utf8_lossy(&output.stdout).to_string()),
    );
    out.insert(
        "stderr".to_string(),
        Value::Str(String::from_utf8_lossy(&output.stderr).to_string()),
    );
    Ok(Value::Dict(out))
}

fn python_prefixed_call_builtin(
    target: &str,
    positional: &[Value],
    named: &HashMap<String, Value>,
) -> Result<Value, String> {
    let python = find_python_command().ok_or_else(|| {
        "py.<name> call failed: no python interpreter found (expected python3 or python)"
            .to_string()
    })?;

    let args_json = serde_json::to_string(&value_to_json(&Value::List(positional.to_vec())))
        .map_err(|e| format!("py.{target} failed to serialize args: {e}"))?;

    let mut named_map = HashMap::new();
    for (k, v) in named {
        named_map.insert(k.clone(), v.clone());
    }
    let kwargs_json = serde_json::to_string(&value_to_json(&Value::Dict(named_map)))
        .map_err(|e| format!("py.{target} failed to serialize kwargs: {e}"))?;

    let script = r#"
import builtins
import importlib
import json
import os
import sys

target = os.environ.get('INDENT_PY_TARGET', '')
args = json.loads(os.environ.get('INDENT_PY_ARGS', '[]'))
kwargs = json.loads(os.environ.get('INDENT_PY_KWARGS', '{}'))

def resolve_target(name: str):
    if '.' not in name:
        if hasattr(builtins, name):
            return getattr(builtins, name)
        return eval(name)

    parts = name.split('.')
    obj = importlib.import_module(parts[0])
    for part in parts[1:]:
        obj = getattr(obj, part)
    return obj

def normalize(value):
    try:
        json.dumps(value)
        return {'kind': 'json', 'value': value}
    except Exception:
        return {'kind': 'repr', 'value': repr(value)}

func = resolve_target(target)
result = func(*args, **kwargs)
print(json.dumps(normalize(result)))
"#;

    let output = Command::new(python)
        .arg("-c")
        .arg(script)
        .env("INDENT_PY_TARGET", target)
        .env("INDENT_PY_ARGS", args_json)
        .env("INDENT_PY_KWARGS", kwargs_json)
        .output()
        .map_err(|e| format!("py.{target} failed: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "py.{target} failed with status {}: {}",
            output.status.code().unwrap_or(1),
            err
        ));
    }

    let stdout_text = String::from_utf8_lossy(&output.stdout).to_string();
    let mut lines: Vec<&str> = stdout_text.lines().collect();
    while lines.last().is_some_and(|v| v.trim().is_empty()) {
        lines.pop();
    }

    if lines.len() > 1 {
        for line in &lines[..lines.len() - 1] {
            println!("{line}");
        }
    }

    let last = lines
        .last()
        .ok_or_else(|| format!("py.{target} returned no output"))?
        .trim();

    let envelope = serde_json::from_str::<JsonValue>(last)
        .map_err(|e| format!("py.{target} returned invalid result payload: {e}"))?;

    let kind = envelope
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("json");
    let value = envelope.get("value").unwrap_or(&JsonValue::Null);

    if kind == "json" {
        Ok(json_to_value(value))
    } else {
        Ok(Value::Str(value.as_str().unwrap_or_default().to_string()))
    }
}

fn os_list_dir_builtin(path: &str) -> Result<Value, String> {
    let mut entries = Vec::new();
    let read_dir = fs::read_dir(path).map_err(|e| format!("os_list_dir failed: {e}"))?;
    for entry in read_dir {
        let entry = entry.map_err(|e| format!("os_list_dir failed: {e}"))?;
        entries.push(Value::Str(entry.file_name().to_string_lossy().to_string()));
    }
    Ok(Value::List(entries))
}

fn os_mkdir_builtin(path: &str) -> Result<Value, String> {
    fs::create_dir_all(path).map_err(|e| format!("os_mkdir failed: {e}"))?;
    Ok(Value::Empty)
}

fn os_remove_builtin(path: &str) -> Result<Value, String> {
    let p = Path::new(path);
    if p.is_dir() {
        fs::remove_dir_all(p).map_err(|e| format!("os_remove failed: {e}"))?;
    } else {
        fs::remove_file(p).map_err(|e| format!("os_remove failed: {e}"))?;
    }
    Ok(Value::Empty)
}

fn os_rename_builtin(source: &str, destination: &str) -> Result<Value, String> {
    fs::rename(source, destination).map_err(|e| format!("os_rename failed: {e}"))?;
    Ok(Value::Empty)
}

fn os_environ_builtin() -> Value {
    let mut out = HashMap::new();
    for (k, v) in env::vars() {
        out.insert(k, Value::Str(v));
    }
    Value::Dict(out)
}

fn file_read_text_builtin(path: &str) -> Result<Value, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("file_read_text failed: {e}"))?;
    Ok(Value::Str(content))
}

fn file_write_text_builtin(path: &str, text: &str) -> Result<Value, String> {
    fs::write(path, text).map_err(|e| format!("file_write_text failed: {e}"))?;
    Ok(Value::Empty)
}

fn file_append_text_builtin(path: &str, text: &str) -> Result<Value, String> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("file_append_text failed: {e}"))?;
    file.write_all(text.as_bytes())
        .map_err(|e| format!("file_append_text failed: {e}"))?;
    Ok(Value::Empty)
}

/// Recursively walk a directory tree, returning every file path
/// (sorted depth-first, directories traversed in order). Mirrors
/// Python's `os.walk` / `glob.glob("**/*")` use case.
fn walk_builtin(path: &str) -> Result<Value, String> {
    let root = PathBuf::from(path);
    if !root.exists() {
        return Err(format!("walk: path '{}' does not exist", path));
    }
    let mut results = Vec::new();
    fn recurse(dir: &Path, out: &mut Vec<Value>) -> Result<(), String> {
        let entries = fs::read_dir(dir).map_err(|e| format!("walk failed: {e}"))?;
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let p = entry.path();
            if p.is_dir() {
                recurse(&p, out)?;
            } else {
                out.push(Value::Str(p.to_string_lossy().to_string()));
            }
        }
        Ok(())
    }
    recurse(&root, &mut results)?;
    Ok(Value::List(results))
}

/// Read a CSV file into a list of rows, each row a list of cell strings.
/// Handles quoted fields containing commas, escaped quotes, and newlines.
fn csv_read_builtin(path: &str) -> Result<Value, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("csv_read failed: {e}"))?;
    let mut rows = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        rows.push(Value::List(parse_csv_line(line)));
    }
    Ok(Value::List(rows))
}

fn parse_csv_line(line: &str) -> Vec<Value> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(Value::Str(current.trim().to_string()));
            current.clear();
        } else {
            current.push(c);
        }
    }
    fields.push(Value::Str(current.trim().to_string()));
    fields
}

/// Write a list of rows (each a list) to a CSV file.
/// Fields containing commas, quotes, or newlines are quoted/escaped.
fn csv_write_builtin(path: &str, rows: &Value) -> Result<Value, String> {
    let rows = match rows {
        Value::List(rows) => rows,
        _ => return Err("csv_write expects a list of rows (list of lists)".to_string()),
    };
    let mut content = String::new();
    for row in rows {
        let row_items = match row {
            Value::List(items) => items.clone(),
            _ => return Err("csv_write expects each row to be a list".to_string()),
        };
        let fields: Vec<String> = row_items
            .iter()
            .map(|v| {
                let s = v.to_string();
                if s.contains(',') || s.contains('"') || s.contains('\n') {
                    format!("\"{}\"", s.replace('"', "\"\""))
                } else {
                    s
                }
            })
            .collect();
        content.push_str(&fields.join(","));
        content.push('\n');
    }
    fs::write(path, content).map_err(|e| format!("csv_write failed: {e}"))?;
    Ok(Value::Empty)
}

/// Convert a rusqlite SQL value into an Indent Value.
fn sqlite_to_value(v: rusqlite::types::Value) -> Value {
    match v {
        rusqlite::types::Value::Null => Value::Empty,
        rusqlite::types::Value::Integer(i) => Value::Int(i),
        rusqlite::types::Value::Real(r) => Value::Float(r),
        rusqlite::types::Value::Text(s) => Value::Str(s),
        rusqlite::types::Value::Blob(b) => Value::Str(String::from_utf8_lossy(&b).to_string()),
    }
}

/// Run a single non-query SQL statement (CREATE/INSERT/UPDATE/DELETE).
/// Returns the number of affected rows.
fn sqlite_exec_builtin(path: &str, sql: &str) -> Result<Value, String> {
    let conn =
        rusqlite::Connection::open(path).map_err(|e| format!("sqlite_exec: {e}"))?;
    let changed =
        conn.execute(sql, []).map_err(|e| format!("sqlite_exec: {e}"))?;
    Ok(Value::Int(changed as i64))
}

/// Run a SELECT and return all rows as a list of lists (each cell an Indent value).
fn sqlite_query_builtin(path: &str, sql: &str) -> Result<Value, String> {
    let conn =
        rusqlite::Connection::open(path).map_err(|e| format!("sqlite_query: {e}"))?;
    let mut stmt =
        conn.prepare(sql).map_err(|e| format!("sqlite_query: {e}"))?;
    let col_count = stmt.column_count();
    let mut rows = Vec::new();
    let mut query = stmt
        .query([])
        .map_err(|e| format!("sqlite_query: {e}"))?;
    while let Some(row) = query
        .next()
        .map_err(|e| format!("sqlite_query: {e}"))?
    {
        let mut cells = Vec::with_capacity(col_count);
        for i in 0..col_count {
            let cell = row
                .get::<_, rusqlite::types::Value>(i)
                .map_err(|e| format!("sqlite_query: {e}"))?;
            cells.push(sqlite_to_value(cell));
        }
        rows.push(Value::List(cells));
    }
    Ok(Value::List(rows))
}

/// Run a SELECT and return only the first row (list), or `empty` if none.
fn sqlite_query_one_builtin(path: &str, sql: &str) -> Result<Value, String> {
    let conn =
        rusqlite::Connection::open(path).map_err(|e| format!("sqlite_query_one: {e}"))?;
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("sqlite_query_one: {e}"))?;
    let col_count = stmt.column_count();
    let mut query = stmt
        .query([])
        .map_err(|e| format!("sqlite_query_one: {e}"))?;
    if let Some(row) = query
        .next()
        .map_err(|e| format!("sqlite_query_one: {e}"))?
    {
        let mut cells = Vec::with_capacity(col_count);
        for i in 0..col_count {
            let cell = row
                .get::<_, rusqlite::types::Value>(i)
                .map_err(|e| format!("sqlite_query_one: {e}"))?;
            cells.push(sqlite_to_value(cell));
        }
        Ok(Value::List(cells))
    } else {
        Ok(Value::Empty)
    }
}

/// Classify an error string into an error code, mirroring the codes produced
/// by `format_error_with_source` (E000–E012).
fn classify_error_code(err: &str) -> &'static str {
    if err.contains("division by zero") {
        "E007"
    } else if err.contains("index out of range") || err.contains("index out of bounds") {
        "E008"
    } else if err.contains("key not found") || err.contains("has no key")
        || err.contains("no key")
    {
        "E009"
    } else if err.contains("file not found") || err.contains("no such file")
        || err.contains("Cannot open")
    {
        "E010"
    } else if err.contains("JSON") || err.contains("Invalid JSON") {
        "E011"
    } else if err.contains("connection") || err.contains("network")
        || err.contains("timeout") || err.contains("refused")
    {
        "E012"
    } else if err.contains("expects") || err.contains("Cannot convert")
        || err.contains("Cannot add") || err.contains("Cannot subtract")
        || err.contains("Cannot multiply")
    {
        "E001"
    } else if err.contains("no function") || err.contains("has no function")
        || err.contains("is not callable")
    {
        "E002"
    } else if err.contains("import") || err.contains("Cannot import")
        || err.contains("Cannot load")
    {
        "E003"
    } else if err.contains("syntax") || err.contains("expected")
        || err.contains("unexpected")
    {
        "E004"
    } else if err.contains("unwrap") {
        "E005"
    } else if err.contains("variable") || err.contains("undefined")
        || err.contains("not defined")
    {
        "E006"
    } else {
        "E000"
    }
}

fn error_code_to_name(code: &str) -> &'static str {
    match code {
        "E001" => "type_error",
        "E002" => "undefined_function",
        "E003" => "import_error",
        "E004" => "syntax_error",
        "E005" => "unwrap_error",
        "E006" => "undefined_variable",
        "E007" => "division_by_zero",
        "E008" => "index_error",
        "E009" => "key_error",
        "E010" => "file_not_found",
        "E011" => "json_error",
        "E012" => "network_error",
        _ => "runtime_error",
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for x in chars.by_ref() {
                if x == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Return a human-readable error type name (e.g. "key_error", "type_error")
/// for an error string captured by `catch as err:`.
fn error_type_builtin(err: &str) -> Result<Value, String> {
    let cleaned = strip_ansi(err);
    // If an explicit `error[EXXX]` code is present, use it directly.
    if let Some(start) = cleaned.find("error[") {
        if let Some(end) = cleaned[start..].find(']') {
            let code = &cleaned[start + 6..start + end];
            return Ok(Value::Str(error_code_to_name(code).to_string()));
        }
    }
    let code = classify_error_code(&cleaned);
    Ok(Value::Str(error_code_to_name(code).to_string()))
}

/// Return just the message portion of an error string (strips the
/// `error[EXXX]: ` prefix and any ANSI codes).
fn error_message_builtin(err: &str) -> Result<Value, String> {
    let cleaned = strip_ansi(err);
    let msg = if let Some(idx) = cleaned.find("]: ") {
        cleaned[idx + 3..].trim().to_string()
    } else {
        cleaned.trim().to_string()
    };
    Ok(Value::Str(msg))
}

/// Count occurrences of each element in a list → dict `{element: count}`.
fn counter_builtin(list: &Value) -> Result<Value, String> {
    let items = match list {
        Value::List(v) => v,
        _ => return Err("counter expects a list".to_string()),
    };
    let mut out = HashMap::new();
    for it in items {
        let e = out.entry(it.to_string()).or_insert(Value::Int(0));
        if let Value::Int(n) = e {
            *n += 1;
        }
    }
    Ok(Value::Dict(out))
}

/// Write a log line `[LEVEL] message` to stderr.
fn log_builtin(level: &str, msg: &str) -> Result<Value, String> {
    eprintln!("[{}] {}", level.to_uppercase(), msg);
    Ok(Value::Empty)
}

fn toml_to_value(v: &toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::Str(s.clone()),
        toml::Value::Integer(i) => Value::Int(*i),
        toml::Value::Float(f) => Value::Float(*f),
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(d) => Value::Str(d.to_string()),
        toml::Value::Array(a) => Value::List(a.iter().map(toml_to_value).collect()),
        toml::Value::Table(t) => {
            let mut m = HashMap::new();
            for (k, v) in t {
                m.insert(k.clone(), toml_to_value(v));
            }
            Value::Dict(m)
        }
    }
}

fn toml_loads_builtin(text: &str) -> Result<Value, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("toml_loads: {e}"))?;
    Ok(toml_to_value(&v))
}

fn value_to_toml(v: &Value) -> Option<toml::Value> {
    match v {
        Value::Int(i) => Some(toml::Value::Integer(*i)),
        Value::Float(f) => Some(toml::Value::Float(*f)),
        Value::Bool(b) => Some(toml::Value::Boolean(*b)),
        Value::Str(s) => Some(toml::Value::String(s.clone())),
        Value::List(items) => Some(toml::Value::Array(
            items.iter().filter_map(value_to_toml).collect(),
        )),
        Value::Set(items) => Some(toml::Value::Array(
            items.iter().filter_map(value_to_toml).collect(),
        )),
        Value::Dict(d) => {
            let mut t = toml::map::Map::new();
            for (k, v) in d {
                if let Some(tv) = value_to_toml(v) {
                    t.insert(k.clone(), tv);
                }
            }
            Some(toml::Value::Table(t))
        }
        _ => None,
    }
}

fn toml_dumps_builtin(value: &Value) -> Result<Value, String> {
    let tv = value_to_toml(value).ok_or_else(|| "toml_dumps expects a dict".to_string())?;
    let s = toml::to_string(&tv).map_err(|e| format!("toml_dumps: {e}"))?;
    Ok(Value::Str(s))
}

/// Parse YAML text into an Indent value (reuses the JSON value converters,
/// since YAML is a superset-ish of JSON's data model).
fn yaml_loads_builtin(text: &str) -> Result<Value, String> {
    let v: JsonValue =
        serde_yaml::from_str(text).map_err(|e| format!("yaml_loads: {e}"))?;
    Ok(json_to_value(&v))
}

/// Serialize an Indent value to YAML text (reuses value_to_json).
fn yaml_dumps_builtin(value: &Value) -> Result<Value, String> {
    let jv = value_to_json(value);
    let s = serde_yaml::to_string(&jv).map_err(|e| format!("yaml_dumps: {e}"))?;
    Ok(Value::Str(s))
}

/// Gzip-compress text, return the compressed bytes base64-encoded (text-safe).
fn gzip_compress_builtin(text: &str) -> Result<Value, String> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(text.as_bytes())
        .map_err(|e| format!("gzip_compress: {e}"))?;
    let bytes = enc.finish().map_err(|e| format!("gzip_compress: {e}"))?;
    use base64::Engine;
    Ok(Value::Str(
        base64::engine::general_purpose::STANDARD.encode(&bytes),
    ))
}

/// Take base64 of gzip data and decompress it back to text.
fn gzip_decompress_builtin(b64: &str) -> Result<Value, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("gzip_decompress: {e}"))?;
    use flate2::read::GzDecoder;
    use std::io::Read;
    let mut dec = GzDecoder::new(&bytes[..]);
    let mut out = String::new();
    dec.read_to_string(&mut out)
        .map_err(|e| format!("gzip_decompress: {e}"))?;
    Ok(Value::Str(out))
}

/// List the entry names in a zip archive.
fn zip_list_builtin(path: &str) -> Result<Value, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("zip_list: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("zip_list: {e}"))?;
    let mut names = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("zip_list: {e}"))?;
        names.push(Value::Str(entry.name().to_string()));
    }
    Ok(Value::List(names))
}

/// Extract a zip archive into a destination directory (path-safe).
fn zip_extract_builtin(path: &str, dest: &str) -> Result<Value, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("zip_extract: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("zip_extract: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("zip_extract: {e}"))?;
        let out_path = Path::new(dest).join(entry.mangled_name());
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| format!("zip_extract: {e}"))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("zip_extract: {e}"))?;
            }
            let mut out = std::fs::File::create(&out_path)
                .map_err(|e| format!("zip_extract: {e}"))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("zip_extract: {e}"))?;
        }
    }
    Ok(Value::Empty)
}

/// Run a named function on a background thread with its own isolated runtime.
/// Returns a task id (int). Args are passed by value (consistent with Indent's
/// pass-by-value model). The spawned runtime shares no mutable state.
fn spawn_task_builtin(
    fn_name: &str,
    args: &[Value],
    ctx: &mut ExecContext<'_>,
) -> Result<Value, String> {
    // Resolve the callable first (handles both local funcs and module externals).
    let callable = resolve_callable(fn_name, ctx)
        .map_err(|e| format!("spawn: function '{}' not found: {}", fn_name, e))?;

    let (module_dir, func_def, module_ref, func_key, builtin_name) = match &callable {
        Callable::Local(f) => {
            (ctx.rt.module_dir.clone(), Some(f.clone()), None, None, None)
        }
        Callable::External { module, name } => {
            (ctx.rt.module_dir.clone(), None, Some(module.clone()), Some(name.clone()), None)
        }
        Callable::Builtin(name) => {
            (ctx.rt.module_dir.clone(), None, None, None, Some(name.clone()))
        }
    };

    let funcs = ctx.rt.funcs.clone();
    let callables = ctx.rt.callables.clone();
    let classes = ctx.rt.classes.clone();
    let async_funcs = ctx.rt.async_funcs.clone();
    let args = args.to_vec();

    let done = Arc::new(AtomicBool::new(false));
    let result = Arc::new(Mutex::new(None::<Value>));
    let done2 = done.clone();
    let result2 = result.clone();

    let id = TASK_NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let handle = std::thread::spawn(move || {
        let mut rt = Runtime::new(module_dir);
        rt.funcs = funcs;
        rt.callables = callables;
        rt.classes = classes;
        rt.async_funcs = async_funcs;

        // Inject the external module so its functions are callable.
        if let Some(ref m) = module_ref {
            for (n, c) in &m.callables {
                if !rt.callables.contains_key(n.as_str()) {
                    rt.callables.insert(n.clone(), c.clone());
                }
            }
            for (n, v) in &m.vars {
                if !rt.vars.contains_key(n.as_str()) {
                    rt.vars.insert(n.clone(), v.clone());
                }
            }
        }

        let out = {
            let mut ctx2 = ExecContext::new(&mut rt);
            // Local function: we have the cloned FunctionDef directly.
            // External function: invoke via module ref + name.
            if let Some(f) = func_def {
                invoke_function(&f, &args, &HashMap::new(), &mut ctx2)
            } else if let (Some(em), Some(ek)) = (&module_ref, &func_key) {
                invoke_external_function(em.clone(), ek, &args, &HashMap::new(), &mut ctx2)
            } else if let Some(bn) = &builtin_name {
                invoke_builtin(bn, &args)
                    .unwrap_or_else(|| Err(format!("Unknown builtin '{bn}'")))
            } else {
                Err("spawn: internal error — no callable resolved".to_string())
            }
        };
        let val = match out {
            Ok(v) => v,
            Err(_e) => Value::Empty,
        };
        *result2.lock().unwrap() = Some(val);
        done2.store(true, Ordering::SeqCst);
    });

    TASKS.lock().unwrap().insert(
        id,
        TaskState {
            handle: Some(handle),
            done,
            result,
        },
    );
    Ok(Value::Int(id))
}

/// Block until the task finishes and return its result.
fn task_wait_builtin(id: i64) -> Result<Value, String> {
    let handle = {
        let mut tasks = TASKS.lock().unwrap();
        let t = tasks
            .get_mut(&id)
            .ok_or_else(|| format!("task_wait: unknown task id {}", id))?;
        t.handle.take()
    };
    if let Some(h) = handle {
        let _ = h.join();
    }
    let tasks = TASKS.lock().unwrap();
    let val = tasks
        .get(&id)
        .and_then(|t| t.result.lock().unwrap().clone())
        .unwrap_or(Value::Empty);
    Ok(val)
}

/// Non-blocking: is the task finished?
fn task_done_builtin(id: i64) -> Result<Value, String> {
    let tasks = TASKS.lock().unwrap();
    let t = tasks
        .get(&id)
        .ok_or_else(|| format!("task_done: unknown task id {}", id))?;
    Ok(Value::Bool(t.done.load(Ordering::SeqCst)))
}

/// Non-blocking: return the result if finished, else `empty`.
fn task_result_builtin(id: i64) -> Result<Value, String> {
    let tasks = TASKS.lock().unwrap();
    let t = tasks
        .get(&id)
        .ok_or_else(|| format!("task_result: unknown task id {}", id))?;
    if !t.done.load(Ordering::SeqCst) {
        return Ok(Value::Empty);
    }
    Ok(t.result.lock().unwrap().clone().unwrap_or(Value::Empty))
}

/// Wait for a list of task ids, return their results in order.
fn task_wait_all_builtin(ids: &Value) -> Result<Value, String> {
    let ids = match ids {
        Value::List(v) => v,
        _ => return Err("task_wait_all expects a list of task ids".to_string()),
    };
    let mut results = Vec::new();
    for item in ids {
        let id = match item {
            Value::Int(i) => *i,
            _ => return Err("task_wait_all expects a list of task ids".to_string()),
        };
        results.push(task_wait_builtin(id)?);
    }
    Ok(Value::List(results))
}

/// Run a function for each argument-list concurrently; return results in order.
/// Like Python's `asyncio.gather` — `parallel "f" [[a,b],[c,d]]` runs `f a b`
/// and `f c d` in parallel and returns `[f(a,b), f(c,d)]`.
fn parallel_builtin(
    fn_name: &str,
    arglists: &Value,
    ctx: &mut ExecContext<'_>,
) -> Result<Value, String> {
    let lists = match arglists {
        Value::List(v) => v,
        _ => return Err("parallel expects a list of argument-lists".to_string()),
    };
    let mut ids = Vec::new();
    for item in lists {
        let args = match item {
            Value::List(a) => a.clone(),
            _ => {
                return Err("parallel expects each item to be a list of arguments".to_string())
            }
        };
        ids.push(spawn_task_builtin(fn_name, &args, ctx)?);
    }
    let mut results = Vec::new();
    for id in ids {
        if let Value::Int(i) = id {
            results.push(task_wait_builtin(i)?);
        }
    }
    Ok(Value::List(results))
}

/// Wait up to `seconds` for a task; return its result, or `empty` on timeout.
/// Like Python's `asyncio.wait_for`.
fn task_wait_timeout_builtin(id: i64, seconds: f64) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    loop {
        let done = {
            let tasks = TASKS.lock().unwrap();
            match tasks.get(&id) {
                Some(t) => t.done.load(Ordering::SeqCst),
                None => return Err(format!("task_wait_timeout: unknown task id {}", id)),
            }
        };
        if done {
            break;
        }
        if Instant::now() >= deadline {
            return Ok(Value::Empty);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let handle = {
        let mut tasks = TASKS.lock().unwrap();
        tasks.get_mut(&id).and_then(|t| t.handle.take())
    };
    if let Some(h) = handle {
        let _ = h.join();
    }
    let tasks = TASKS.lock().unwrap();
    Ok(tasks
        .get(&id)
        .and_then(|t| t.result.lock().unwrap().clone())
        .unwrap_or(Value::Empty))
}

/// Convert a future value (task id) to an i64 id. Accepts an int or a string
/// that parses as an int.
fn future_to_id(v: &Value) -> Result<i64, String> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Str(s) => s
            .trim()
            .parse::<i64>()
            .map_err(|_| format!("await: invalid future id '{}'", s)),
        _ => Err(format!("await: expected a future id (int), got {}", v.type_name())),
    }
}

/// Run the body of a `loop:` block. `await` statements inside block until their
/// future (a background thread task) completes, so multiple `future` calls made
/// before awaiting run concurrently.
fn run_event_loop(body: &[Stmt], ctx: &mut ExecContext<'_>) -> Result<Control, String> {
    exec_block(body, ctx)
}

/// Best-effort cancel: remove the task from the store (thread keeps running,
/// but the future id is no longer awaitable).
fn future_cancel_builtin(id: i64) -> Result<Value, String> {
    let mut tasks = TASKS.lock().unwrap();
    match tasks.remove(&id) {
        Some(_t) => Ok(Value::Bool(true)),
        None => Err(format!("future_cancel: unknown future id {}", id)),
    }
}

/// Await a list of futures and collect their results in order.
/// Like Python's `asyncio.gather`. Accepts `gather f1 f2 ...` or `gather [f1, f2]`.
fn gather_builtin(ids: &[Value]) -> Result<Value, String> {
    let mut results = Vec::new();
    for v in ids {
        match v {
            Value::Int(id) => results.push(task_wait_builtin(*id)?),
            Value::List(l) => {
                for item in l {
                    if let Value::Int(id) = item {
                        results.push(task_wait_builtin(*id)?);
                    } else {
                        return Err("gather: list must contain only future ids (int)".to_string());
                    }
                }
            }
            _ => return Err("gather expects future ids (int) or a list of them".to_string()),
        }
    }
    Ok(Value::List(results))
}

/// Async sleep: returns a future that resolves after `seconds`.
/// Usable with `await sleep(0.1)` inside a `loop:` block.
fn sleep_builtin(seconds: f64) -> Result<Value, String> {
    let done = Arc::new(AtomicBool::new(false));
    let result = Arc::new(Mutex::new(None::<Value>));
    let done2 = done.clone();
    let result2 = result.clone();
    let id = TASK_NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs_f64(seconds.max(0.0)));
        *result2.lock().unwrap() = Some(Value::Empty);
        done2.store(true, Ordering::SeqCst);
    });
    TASKS.lock().unwrap().insert(
        id,
        TaskState {
            handle: Some(handle),
            done,
            result,
        },
    );
    Ok(Value::Int(id))
}

/// Run any builtin on a background thread and return a future id for it.
/// Enables async I/O: `var f = http_get_async url` then `wait f`.
fn spawn_builtin_future(builtin_name: &str, args: Vec<Value>) -> Result<Value, String> {
    let builtin_name = builtin_name.to_string();
    let done = Arc::new(AtomicBool::new(false));
    let result = Arc::new(Mutex::new(None::<Value>));
    let done2 = done.clone();
    let result2 = result.clone();
    let id = TASK_NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let handle = std::thread::spawn(move || {
        let out = invoke_builtin(&builtin_name, &args).unwrap_or_else(|| {
            Err(format!("async {}: no such builtin", builtin_name))
        });
        let val = out.unwrap_or(Value::Empty);
        *result2.lock().unwrap() = Some(val);
        done2.store(true, Ordering::SeqCst);
    });
    TASKS.lock().unwrap().insert(
        id,
        TaskState {
            handle: Some(handle),
            done,
            result,
        },
    );
    Ok(Value::Int(id))
}

// ── Cooperative coroutine engine (Milestone 1) ─────────────
/// A lightweight green thread: a function body plus an instruction pointer and
/// its own locals. Suspendable at `await` statements.
struct Coro {
    body: Vec<Stmt>,
    ip: usize,
    locals: HashMap<String, Value>,
    result: Value,
    done: bool,
    awaiting: Option<i64>,
}

impl Coro {
    fn new(body: Vec<Stmt>) -> Self {
        Self {
            body,
            ip: 0,
            locals: HashMap::new(),
            result: Value::Empty,
            done: false,
            awaiting: None,
        }
    }
}

/// Run several async function bodies cooperatively on the current thread.
/// Each coroutine executes one statement at a time; when it hits `await` on a
/// future that isn't ready it suspends and other coroutines get to run. Once a
/// suspended coroutine's future resolves it resumes.
///
/// Milestone-1 scope: cooperative at top-level `await` statements in each body;
/// `await` nested inside `if`/`repeat` blocks still blocks (thread-backed).
fn coop_builtin(fn_list: &Value, ctx: &mut ExecContext<'_>) -> Result<Value, String> {
    let items = match fn_list {
        Value::List(v) => v,
        _ => return Err("coop expects a list of [fn_name, [args]]".to_string()),
    };

    let mut coros: Vec<Coro> = Vec::new();
    for item in items {
        let pair = match item {
            Value::List(p) => p,
            _ => return Err("coop expects each item to be [fn_name, [args]]".to_string()),
        };
        let name = pair[0].to_string();
        let args: Vec<Value> = match pair.get(1) {
            Some(Value::List(a)) => a.clone(),
            _ => vec![],
        };
        let f = ctx
            .rt
            .funcs
            .get(&name)
            .cloned()
            .ok_or_else(|| format!("coop: function '{}' not found", name))?;
        let mut c = Coro::new(f.body.clone());
        for (i, p) in f.params.iter().enumerate() {
            c.locals.insert(p.name.clone(), args.get(i).cloned().unwrap_or(Value::Empty));
        }
        coros.push(c);
    }

    let mut results: Vec<Value> = vec![Value::Empty; coros.len()];

    loop {
        let mut any_progress = false;

        for (ci, c) in coros.iter_mut().enumerate() {
            if c.done {
                continue;
            }

            // If suspended on an await, check whether the future is now ready.
            if let Some(id) = c.awaiting {
                if matches!(task_done_builtin(id)?, Value::Bool(true)) {
                    let r = task_wait_builtin(id)?;
                    c.awaiting = None;
                    c.locals.insert("__await_result__".to_string(), r);
                    any_progress = true;
                } else {
                    continue; // still waiting on this future
                }
            }

            // Run statements until we return a result or suspend on a not-ready await.
            loop {
                if c.ip >= c.body.len() {
                    c.done = true;
                    results[ci] = c.result.clone();
                    break;
                }
                let stmt = c.body[c.ip].clone();
                c.ip += 1;

                match &stmt {
                    Stmt::Await { expr, .. } => {
                        // The awaited expression may reference coroutine locals.
                        ctx.frames.push(c.locals.clone());
                        let fv = eval_expr(expr, ctx);
                        ctx.frames.pop();
                        let fv = fv?;
                        let id = future_to_id(&fv)?;
                        if matches!(task_done_builtin(id)?, Value::Bool(true)) {
                            let r = task_wait_builtin(id)?;
                            c.locals.insert("__await_result__".to_string(), r);
                            any_progress = true;
                        } else {
                            c.awaiting = Some(id);
                            break; // suspend
                        }
                    }
                    Stmt::Wait { expr, .. } => {
                        // wait <future> (int) awaits; wait <seconds> (float) delays.
                        ctx.frames.push(c.locals.clone());
                        let v = eval_expr(expr, ctx);
                        ctx.frames.pop();
                        let v = v?;
                        let id = match v {
                            Value::Int(id) => id,
                            Value::Float(secs) => future_to_id(&sleep_builtin(secs)?)?,
                            other => {
                                return Err(format!(
                                    "wait expects a future id (int) or seconds (float), got {}",
                                    other.type_name()
                                ))
                            }
                        };
                        if matches!(task_done_builtin(id)?, Value::Bool(true)) {
                            let r = task_wait_builtin(id)?;
                            c.locals.insert("__await_result__".to_string(), r);
                            any_progress = true;
                        } else {
                            c.awaiting = Some(id);
                            break; // suspend
                        }
                    }
                    _ => {
                        // Run the statement with this coroutine's locals on top.
                        ctx.frames.push(c.locals.clone());
                        let control = exec_stmt(&stmt, ctx);
                        c.locals = ctx.frames.pop().unwrap();
                        let control = control?;
                        any_progress = true;
                        match control {
                            Control::None => {}
                            Control::Return(v) | Control::Yield(v) => {
                                c.result = v;
                                c.done = true;
                                results[ci] = c.result.clone();
                                break;
                            }
                            other => {
                                return Err(format!(
                                    "coop: unexpected control flow in coroutine: {:?}",
                                    other
                                ));
                            }
                        }
                    }
                }
            }
        }

        if coros.iter().all(|c| c.done) {
            break;
        }
        if !any_progress {
            // Nothing ready — let thread-backed futures make progress.
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    Ok(Value::List(results))
}

fn time_now_builtin() -> Result<Value, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("time_now failed: {e}"))?;
    Ok(Value::Float(now.as_secs_f64()))
}

fn time_utc_builtin() -> Result<Value, String> {
    time_now_builtin()
}

fn time_format_builtin(timestamp: &Value, format: Option<&Value>) -> Result<Value, String> {
    use std::time::UNIX_EPOCH;
    let ts_secs = match timestamp {
        Value::Float(f) => *f,
        Value::Int(i) => *i as f64,
        _ => return Err("time_format expects a numeric timestamp".to_string()),
    };
    let fmt_str = format.map(|v| v.to_string()).unwrap_or_else(|| "%Y-%m-%d %H:%M:%S".to_string());
    // Simple formatting using chrono-style specifiers
    let secs = ts_secs.trunc() as i64;
    let nanos = ((ts_secs - ts_secs.trunc()) * 1_000_000_000.0) as u32;
    let d = UNIX_EPOCH + std::time::Duration::new(secs as u64, nanos);
    // Basic format: just return ISO 8601 for now
    let total_secs = d.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let days = total_secs / 86400;
    let time_secs = total_secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let result = if fmt_str.contains("%Y") {
        // Very basic date calc (approximately correct for ~50 years around 2000)
        let year = 1970 + (days / 365) as i64;
        let day_of_year = days % 365;
        let mut month = 1;
        let months_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut remaining = day_of_year as usize;
        for &md in &months_days {
            if remaining < md { break; }
            remaining -= md;
            month += 1;
        }
        let day = remaining + 1;
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    };
    Ok(Value::Str(result))
}

fn time_parse_builtin(datetime_str: &Value, _format: Option<&Value>) -> Result<Value, String> {
    let text = datetime_str.to_string();
    // Try ISO 8601: YYYY-MM-DD HH:MM:SS
    let parts: Vec<&str> = text.split(&['-', ' ', ':', 'T'][..]).collect();
    if parts.len() >= 6 {
        if let (Ok(y), Ok(m), Ok(d), Ok(h), Ok(min), Ok(s)) = (
            parts[0].parse::<i64>(), parts[1].parse::<i64>(), parts[2].parse::<i64>(),
            parts[3].parse::<i64>(), parts[4].parse::<i64>(), parts[5].parse::<f64>(),
        ) {
            let days = (y - 1970) * 365 + (m - 1) * 30 + (d - 1) as i64;
            let ts = days as f64 * 86400.0 + h as f64 * 3600.0 + min as f64 * 60.0 + s;
            return Ok(Value::Float(ts));
        }
    }
    Err(format!("time_parse: cannot parse '{}'", text))
}

fn uuid_v4() -> String {
    let r = || random_next_u64();
    let a = r();
    let b = r();
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (a >> 32) as u32,
        ((a >> 16) & 0xFFFF) as u16,
        (a & 0xFFF) as u16,
        (0x8000 | ((b >> 48) & 0x3FFF)) as u16,
        (b & 0xFFFF_FFFF_FFFF) as u64
    )
}

fn base64_encode(input: &str) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };
        let triple = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if i + 1 < bytes.len() { CHARS[((triple >> 6) & 0x3F) as usize] as char } else { '=' });
        out.push(if i + 2 < bytes.len() { CHARS[(triple & 0x3F) as usize] as char } else { '=' });
        i += 3;
    }
    out
}

fn base64_decode(input: &str) -> Result<String, String> {
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for c in input.chars() {
        if c == '=' { break; }
        let val = match c {
            'A'..='Z' => c as u8 - b'A',
            'a'..='z' => c as u8 - b'a' + 26,
            '0'..='9' => c as u8 - b'0' + 52,
            '+' => 62,
            '/' => 63,
            _ => return Err(format!("base64_decode: invalid character '{}'", c)),
        } as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    String::from_utf8(out).map_err(|e| format!("base64_decode: invalid UTF-8: {e}"))
}

fn time_sleep_builtin(seconds: f64) -> Result<Value, String> {
    std::thread::sleep(Duration::from_secs_f64(seconds));
    Ok(Value::Empty)
}

fn random_next_u64() -> u64 {
    loop {
        let current = RNG_STATE.load(Ordering::Relaxed) as u64;
        let mut x = current;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        if RNG_STATE
            .compare_exchange(current as i64, x as i64, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return x;
        }
    }
}

fn random_next_f64() -> f64 {
    let x = random_next_u64() >> 11;
    (x as f64) / ((1u64 << 53) as f64)
}

fn random_int_builtin(a: i64, b: i64) -> Result<Value, String> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let span = (hi - lo + 1) as u64;
    let value = lo + (random_next_u64() % span) as i64;
    Ok(Value::Int(value))
}

fn random_choice_builtin(value: &Value) -> Result<Value, String> {
    match value {
        Value::List(items) => {
            if items.is_empty() {
                return Err("random_choice requires a non-empty list".to_string());
            }
            let idx = (random_next_u64() % (items.len() as u64)) as usize;
            Ok(items[idx].clone())
        }
        _ => Err(format!("random_choice expects a list, got {}", value.type_name())),
    }
}

fn random_shuffle_builtin(value: &Value) -> Result<Value, String> {
    match value {
        Value::List(items) => {
            let mut out = items.clone();
            if out.len() > 1 {
                for i in (1..out.len()).rev() {
                    let j = (random_next_u64() % ((i + 1) as u64)) as usize;
                    out.swap(i, j);
                }
            }
            Ok(Value::List(out))
        }
        _ => Err(format!("random_shuffle expects a list, got {}", value.type_name())),
    }
}

fn value_to_f64(name: &str, value: &Value) -> Result<f64, String> {
    value
        .as_number()
        .ok_or_else(|| format!("{name} expects numeric arguments"))
}

fn math_unary_float_builtin(value: &Value, op: fn(f64) -> f64) -> Result<Value, String> {
    let v = value_to_f64("math function", value)?;
    Ok(Value::Float(op(v)))
}

fn math_binary_float_builtin(a: &Value, b: &Value, op: fn(f64, f64) -> f64) -> Result<Value, String> {
    let av = value_to_f64("math function", a)?;
    let bv = value_to_f64("math function", b)?;
    Ok(Value::Float(op(av, bv)))
}

fn math_floor_builtin(value: &Value) -> Result<Value, String> {
    let v = value_to_f64("math_floor", value)?;
    Ok(Value::Int(v.floor() as i64))
}

fn math_ceil_builtin(value: &Value) -> Result<Value, String> {
    let v = value_to_f64("math_ceil", value)?;
    Ok(Value::Int(v.ceil() as i64))
}

fn math_round_builtin(value: &Value, digits: &Value) -> Result<Value, String> {
    let v = value_to_f64("math_round", value)?;
    let d = match digits {
        Value::Int(v) => *v,
        _ => return Err("math_round digits must be int".to_string()),
    };
    let factor = 10f64.powi(d as i32);
    Ok(Value::Float((v * factor).round() / factor))
}

fn json_to_value(value: &JsonValue) -> Value {
    match value {
        JsonValue::Null => Value::Empty,
        JsonValue::Bool(v) => Value::Bool(*v),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Empty
            }
        }
        JsonValue::String(s) => Value::Str(s.clone()),
        JsonValue::Array(items) => Value::List(items.iter().map(json_to_value).collect()),
        JsonValue::Object(map) => {
            let mut out = HashMap::new();
            for (k, v) in map {
                out.insert(k.clone(), json_to_value(v));
            }
            Value::Dict(out)
        }
    }
}

fn parse_socket_id(value: &Value) -> Result<i64, String> {
    match value {
        Value::Int(v) => Ok(*v),
        Value::Str(s) => s
            .trim()
            .parse::<i64>()
            .map_err(|_| "socket_id must be an int".to_string()),
        _ => Err("socket_id must be an int".to_string()),
    }
}

fn ws_connect_builtin(url: &str) -> Result<Value, String> {
    let (socket, _) = ws_connect_blocking(url).map_err(|e| format!("ws_connect failed: {e}"))?;

    let socket_id = WS_NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let mut sockets = WS_CLIENTS
        .lock()
        .map_err(|_| "ws_connect failed: socket registry lock poisoned".to_string())?;
    sockets.insert(socket_id, socket);

    let mut out = HashMap::new();
    out.insert("ok".to_string(), Value::Bool(true));
    out.insert("socket_id".to_string(), Value::Int(socket_id));
    Ok(Value::Dict(out))
}

fn ws_send_text_builtin(socket_id: i64, text: &str) -> Result<Value, String> {
    let mut sockets = WS_CLIENTS
        .lock()
        .map_err(|_| "ws_send_text failed: socket registry lock poisoned".to_string())?;
    let socket = sockets
        .get_mut(&socket_id)
        .ok_or_else(|| format!("ws_send_text failed: unknown socket_id {}", socket_id))?;

    socket
        .send(WsMessage::Text(text.to_string().into()))
        .map_err(|e| format!("ws_send_text failed: {e}"))?;

    let mut out = HashMap::new();
    out.insert("ok".to_string(), Value::Bool(true));
    out.insert("socket_id".to_string(), Value::Int(socket_id));
    Ok(Value::Dict(out))
}

fn set_ws_read_timeout(socket: &mut WsClient, timeout: Option<Duration>) -> Result<(), String> {
    match socket.get_mut() {
        tungstenite::stream::MaybeTlsStream::Plain(stream) => stream
            .set_read_timeout(timeout)
            .map_err(|e| format!("ws socket timeout update failed: {e}")),
        tungstenite::stream::MaybeTlsStream::Rustls(stream) => {
            let tcp_stream = stream.get_mut();
            tcp_stream
                .set_read_timeout(timeout)
                .map_err(|e| format!("ws socket timeout update failed: {e}"))
        }
        _ => Err("ws socket timeout update failed: unsupported stream type".to_string()),
    }
}

fn ws_recv_text_builtin(socket_id: i64) -> Result<Value, String> {
    let mut sockets = WS_CLIENTS
        .lock()
        .map_err(|_| "ws_recv_text failed: socket registry lock poisoned".to_string())?;
    let socket = sockets
        .get_mut(&socket_id)
        .ok_or_else(|| format!("ws_recv_text failed: unknown socket_id {}", socket_id))?;

    set_ws_read_timeout(socket, None)?;

    loop {
        let msg = socket
            .read()
            .map_err(|e| format!("ws_recv_text failed: {e}"))?;
        match msg {
            WsMessage::Text(text) => {
                let mut out = HashMap::new();
                out.insert("ok".to_string(), Value::Bool(true));
                out.insert("socket_id".to_string(), Value::Int(socket_id));
                out.insert("text".to_string(), Value::Str(text.to_string()));
                return Ok(Value::Dict(out));
            }
            WsMessage::Binary(data) => {
                let mut out = HashMap::new();
                out.insert("ok".to_string(), Value::Bool(true));
                out.insert("socket_id".to_string(), Value::Int(socket_id));
                out.insert(
                    "text".to_string(),
                    Value::Str(String::from_utf8_lossy(&data).to_string()),
                );
                return Ok(Value::Dict(out));
            }
            WsMessage::Ping(payload) => {
                socket
                    .send(WsMessage::Pong(payload))
                    .map_err(|e| format!("ws_recv_text failed to reply pong: {e}"))?;
            }
            WsMessage::Pong(_) => {}
            WsMessage::Close(frame) => {
                let mut out = HashMap::new();
                out.insert("ok".to_string(), Value::Bool(false));
                out.insert("socket_id".to_string(), Value::Int(socket_id));
                out.insert("closed".to_string(), Value::Bool(true));
                if let Some(f) = frame {
                    out.insert("code".to_string(), Value::Int(u16::from(f.code) as i64));
                    out.insert("reason".to_string(), Value::Str(f.reason.to_string()));
                }
                return Ok(Value::Dict(out));
            }
            WsMessage::Frame(_) => {}
        }
    }
}

fn ws_recv_text_timeout_builtin(socket_id: i64, timeout_seconds: f64) -> Result<Value, String> {
    if timeout_seconds < 0.0 {
        return Err("ws_recv_text_timeout timeout must be >= 0".to_string());
    }

    let mut sockets = WS_CLIENTS
        .lock()
        .map_err(|_| "ws_recv_text_timeout failed: socket registry lock poisoned".to_string())?;
    let socket = sockets
        .get_mut(&socket_id)
        .ok_or_else(|| format!("ws_recv_text_timeout failed: unknown socket_id {}", socket_id))?;

    let timeout = Duration::from_secs_f64(timeout_seconds);
    set_ws_read_timeout(socket, Some(timeout))?;

    loop {
        let msg = match socket.read() {
            Ok(value) => value,
            Err(tungstenite::Error::Io(err))
                if matches!(err.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) =>
            {
                let mut out = HashMap::new();
                out.insert("ok".to_string(), Value::Bool(false));
                out.insert("socket_id".to_string(), Value::Int(socket_id));
                out.insert("timeout".to_string(), Value::Bool(true));
                return Ok(Value::Dict(out));
            }
            Err(tungstenite::Error::ConnectionClosed)
            | Err(tungstenite::Error::AlreadyClosed) => {
                let mut out = HashMap::new();
                out.insert("ok".to_string(), Value::Bool(false));
                out.insert("socket_id".to_string(), Value::Int(socket_id));
                out.insert("closed".to_string(), Value::Bool(true));
                return Ok(Value::Dict(out));
            }
            Err(err) => {
                let msg = err.to_string().to_lowercase();
                if msg.contains("closed") || msg.contains("connection reset") || msg.contains("broken pipe") {
                    let mut out = HashMap::new();
                    out.insert("ok".to_string(), Value::Bool(false));
                    out.insert("socket_id".to_string(), Value::Int(socket_id));
                    out.insert("closed".to_string(), Value::Bool(true));
                    return Ok(Value::Dict(out));
                }
                return Err(format!("ws_recv_text_timeout failed: {err}"));
            }
        };

        match msg {
            WsMessage::Text(text) => {
                let mut out = HashMap::new();
                out.insert("ok".to_string(), Value::Bool(true));
                out.insert("socket_id".to_string(), Value::Int(socket_id));
                out.insert("text".to_string(), Value::Str(text.to_string()));
                return Ok(Value::Dict(out));
            }
            WsMessage::Binary(data) => {
                let mut out = HashMap::new();
                out.insert("ok".to_string(), Value::Bool(true));
                out.insert("socket_id".to_string(), Value::Int(socket_id));
                out.insert(
                    "text".to_string(),
                    Value::Str(String::from_utf8_lossy(&data).to_string()),
                );
                return Ok(Value::Dict(out));
            }
            WsMessage::Ping(payload) => {
                socket
                    .send(WsMessage::Pong(payload))
                    .map_err(|e| format!("ws_recv_text_timeout failed to reply pong: {e}"))?;
            }
            WsMessage::Pong(_) => {}
            WsMessage::Close(frame) => {
                let mut out = HashMap::new();
                out.insert("ok".to_string(), Value::Bool(false));
                out.insert("socket_id".to_string(), Value::Int(socket_id));
                out.insert("closed".to_string(), Value::Bool(true));
                if let Some(f) = frame {
                    out.insert("code".to_string(), Value::Int(u16::from(f.code) as i64));
                    out.insert("reason".to_string(), Value::Str(f.reason.to_string()));
                }
                return Ok(Value::Dict(out));
            }
            WsMessage::Frame(_) => {}
        }
    }
}

fn ws_close_builtin(socket_id: i64) -> Result<Value, String> {
    let mut sockets = WS_CLIENTS
        .lock()
        .map_err(|_| "ws_close failed: socket registry lock poisoned".to_string())?;

    let Some(mut socket) = sockets.remove(&socket_id) else {
        return Err(format!("ws_close failed: unknown socket_id {}", socket_id));
    };

    let _ = socket.close(None);

    let mut out = HashMap::new();
    out.insert("ok".to_string(), Value::Bool(true));
    out.insert("socket_id".to_string(), Value::Int(socket_id));
    Ok(Value::Dict(out))
}

fn value_to_json(value: &Value) -> JsonValue {
    match value {
        Value::Int(v) => JsonValue::from(*v),
        Value::Float(v) => JsonValue::from(*v),
        Value::Bool(v) => JsonValue::from(*v),
        Value::Str(v) => JsonValue::from(v.clone()),
        Value::List(items) => JsonValue::Array(items.iter().map(value_to_json).collect()),
        Value::Set(items) => JsonValue::Array(items.iter().map(value_to_json).collect()),
        Value::Dict(items) => {
            let mut out = JsonMap::new();
            for (k, v) in items {
                out.insert(k.clone(), value_to_json(v));
            }
            JsonValue::Object(out)
        }
        Value::Func(name) => JsonValue::String(name.clone()),
        Value::Module(_) | Value::Empty => JsonValue::Null,
        Value::Object { fields, class_name, .. } => {
            let mut out = JsonMap::new();
            out.insert("__class__".to_string(), JsonValue::from(class_name.clone()));
            for (k, v) in fields {
                out.insert(k.clone(), value_to_json(v));
            }
            JsonValue::Object(out)
        },
    }
}

fn http_response_value(status: i64, body: String) -> Value {
    let mut out = HashMap::new();
    out.insert("ok".to_string(), Value::Bool((200..300).contains(&status)));
    out.insert("status".to_string(), Value::Int(status));
    out.insert("body".to_string(), Value::Str(body));
    Value::Dict(out)
}

fn http_get_builtin(url: &str, authorization: &str) -> Result<Value, String> {
    let client = reqwest::blocking::Client::new();
    let mut req = client.get(url);
    if !authorization.trim().is_empty() {
        req = req.header(reqwest::header::AUTHORIZATION, authorization.trim());
    }

    let resp = req.send().map_err(|e| format!("http_get request failed: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let body = resp
        .text()
        .map_err(|e| format!("http_get failed to read response body: {e}"))?;
    Ok(http_response_value(status, body))
}

fn http_post_json_builtin(url: &str, payload: Value, authorization: &str) -> Result<Value, String> {
    let client = reqwest::blocking::Client::new();
    let json_payload = value_to_json(&payload);

    let mut req = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&json_payload);

    if !authorization.trim().is_empty() {
        req = req.header(reqwest::header::AUTHORIZATION, authorization.trim());
    }

    let resp = req
        .send()
        .map_err(|e| format!("http_post_json request failed: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let body = resp
        .text()
        .map_err(|e| format!("http_post_json failed to read response body: {e}"))?;
    Ok(http_response_value(status, body))
}

fn http_put_json_builtin(url: &str, payload: Value, authorization: &str) -> Result<Value, String> {
    let client = reqwest::blocking::Client::new();
    let json_payload = value_to_json(&payload);

    let mut req = client
        .put(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&json_payload);

    if !authorization.trim().is_empty() {
        req = req.header(reqwest::header::AUTHORIZATION, authorization.trim());
    }

    let resp = req
        .send()
        .map_err(|e| format!("http_put_json request failed: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let body = resp
        .text()
        .map_err(|e| format!("http_put_json failed to read response body: {e}"))?;
    Ok(http_response_value(status, body))
}

fn http_patch_json_builtin(url: &str, payload: Value, authorization: &str) -> Result<Value, String> {
    let client = reqwest::blocking::Client::new();
    let json_payload = value_to_json(&payload);

    let mut req = client
        .patch(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&json_payload);

    if !authorization.trim().is_empty() {
        req = req.header(reqwest::header::AUTHORIZATION, authorization.trim());
    }

    let resp = req
        .send()
        .map_err(|e| format!("http_patch_json request failed: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let body = resp
        .text()
        .map_err(|e| format!("http_patch_json failed to read response body: {e}"))?;
    Ok(http_response_value(status, body))
}

fn http_delete_builtin(url: &str, authorization: &str) -> Result<Value, String> {
    let client = reqwest::blocking::Client::new();
    let mut req = client.delete(url);

    if !authorization.trim().is_empty() {
        req = req.header(reqwest::header::AUTHORIZATION, authorization.trim());
    }

    let resp = req
        .send()
        .map_err(|e| format!("http_delete request failed: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let body = resp
        .text()
        .map_err(|e| format!("http_delete failed to read response body: {e}"))?;
    Ok(http_response_value(status, body))
}

fn http_serve_dir_builtin(dir: &str, port: u16) -> Result<Value, String> {
    let bind_addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&bind_addr)
        .map_err(|e| format!("http_serve_dir: cannot bind to {bind_addr}: {e}"))?;

    let dir_path = PathBuf::from(dir);
    if !dir_path.is_dir() {
        return Err(format!(
            "http_serve_dir: '{}' is not a directory",
            dir
        ));
    }

    eprintln!("Indent HTTP server listening on http://{bind_addr}");
    eprintln!("Serving files from: {}", dir_path.display());

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let _ = handle_http_request(&mut stream, &dir_path);
            }
            Err(e) => {
                eprintln!("Connection error: {e}");
            }
        }
    }

    Ok(Value::Empty)
}

fn gui_show_html_builtin(html: &str, title: &str, width: i32, height: i32) -> Result<Value, String> {
    // Find the gui_window helper binary
    let gui_binary = find_gui_binary();

    // Spawn the native window — pipe HTML via stdin (no temp files)
    let mut child = Command::new(&gui_binary)
        .arg(title)
        .arg("--stdin")
        .arg(width.to_string())
        .arg(height.to_string())
        .env("UBUNTU_MENUPROXY", "0")
        .env("GDK_BACKEND", "x11")
        .env("WEBKIT_DISABLE_COMPOSITING_MODE", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("gui_show_html: failed to open native window: {e}\n\nInstall webkit2gtk and gtk3, then ensure 'indent-gui' is in PATH or next to the indent binary."))?;

    // Write HTML content to the child's stdin
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(html.as_bytes());
        // stdin is closed when dropped
    }

    let result = child.wait()
        .map(|_| ())
        .map_err(|e| format!("gui_show_html: window process error: {e}"));

    result?;
    Ok(Value::Empty)
}

/// Find the gui_window helper binary — looks next to the indent executable first,
/// then in PATH.
fn find_gui_binary() -> PathBuf {
    // Check next to the current executable
    if let Ok(exe) = std::env::current_exe() {
        let beside = exe.parent().unwrap_or(Path::new(".")).join("indent-gui");
        if beside.exists() { return beside; }
        let beside2 = exe.parent().unwrap_or(Path::new(".")).join("gui_window");
        if beside2.exists() { return beside2; }
    }
    // Check PATH manually
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in split_path_list(&path_var) {
            for name in &["indent-gui", "gui_window"] {
                let p = Path::new(dir).join(name);
                if p.exists() { return p; }
            }
        }
    }
    // Fallback
    PathBuf::from("indent-gui")
}

fn handle_http_request(stream: &mut TcpStream, root: &PathBuf) -> Result<(), String> {
    let mut buf = [0u8; 8192];
    let n = stream
        .read(&mut buf)
        .map_err(|e| format!("Failed to read request: {e}"))?;

    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err("Invalid HTTP request".to_string());
    }
    let _method = parts[0];
    let path = parts[1];

    // Normalize path (prevent directory traversal)
    let clean_path = path.trim_start_matches('/');
    let clean_path = if clean_path.is_empty() { "index.html" } else { clean_path };
    let file_path = root.join(clean_path);

    // Security: ensure the resolved path is inside the root directory
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.clone());
    let resolved = file_path.canonicalize();

    match resolved {
        Ok(resolved_path) if resolved_path.starts_with(&canonical_root) => {
            if resolved_path.is_file() {
                let content = fs::read(&resolved_path).unwrap_or_default();
                let mime = mime_type_for_path(&resolved_path);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    mime,
                    content.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&content);
            } else {
                let _ = write_404(stream);
            }
        }
        _ => {
            let _ = write_404(stream);
        }
    }

    Ok(())
}

fn write_404(stream: &mut TcpStream) -> Result<(), String> {
    let body = "<h1>404 Not Found</h1><p>The requested file was not found on this server.</p>";
    let response = format!(
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|e| format!("Failed to write 404: {e}"))
}

fn mime_type_for_path(path: &Path) -> &str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") | Some("htm") => "text/html",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("txt") | Some("ath") => "text/plain",
        Some("xml") => "application/xml",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn read_input_line(prompt: &str) -> Result<String, String> {
    if !prompt.is_empty() {
        println!("{prompt}");
        io::stdout()
            .flush()
            .map_err(|e| format!("Input I/O error: {e}"))?;
    }

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("Input I/O error: {e}"))?;

    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }

    Ok(line)
}

fn coerce_type(line: usize, name: &str, ty: &str, value: Value) -> Result<Value, String> {
    match ty.to_lowercase().as_str() {
        "string" => match value {
            Value::Str(_) => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' expects string")),
        },
        "int" => match value {
            Value::Int(_) => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' expects int")),
        },
        "float" => match value {
            Value::Float(_) => Ok(value),
            Value::Int(v) => Ok(Value::Float(v as f64)),
            _ => Err(format!("Line {line}: variable '{name}' expects float")),
        },
        "boolean" => match value {
            Value::Bool(_) => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' expects boolean")),
        },
        "list" => match value {
            Value::List(_) => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' expects list")),
        },
        "dictionary" | "dict" => match value {
            Value::Dict(_) => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' expects dictionary")),
        },
        "module" => match value {
            Value::Module(_) => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' expects module")),
        },
        "empty" => match value {
            Value::Empty => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' expects empty")),
        },
        "color" => match value {
            Value::Str(s) if is_valid_color_literal(&s) => Ok(Value::Str(s)),
            _ => Err(format!("Line {line}: variable '{name}' expects color")),
        },
        "dynamic" => Ok(value),
        _ => Err(format!("Unknown type '{ty}' for '{name}'")),
    }
}

fn convert_type(line: usize, name: &str, ty: &str, value: Value) -> Result<Value, String> {
    match ty.to_lowercase().as_str() {
        "string" => Ok(Value::Str(value.to_string())),
        "int" => match value {
            Value::Int(_) => Ok(value),
            Value::Float(v) if v.fract().abs() < f64::EPSILON => Ok(Value::Int(v as i64)),
            Value::Str(text) => text.trim().parse::<i64>().map(Value::Int).map_err(|_| {
                format!("Line {line}: variable '{name}' cannot be converted to int")
            }),
            _ => Err(format!("Line {line}: variable '{name}' cannot be converted to int")),
        },
        "float" => match value {
            Value::Float(_) => Ok(value),
            Value::Int(v) => Ok(Value::Float(v as f64)),
            Value::Str(text) => text.trim().parse::<f64>().map(Value::Float).map_err(|_| {
                format!("Line {line}: variable '{name}' cannot be converted to float")
            }),
            _ => Err(format!("Line {line}: variable '{name}' cannot be converted to float")),
        },
        "boolean" => match value {
            Value::Bool(_) => Ok(value),
            Value::Int(v) => Ok(Value::Bool(v != 0)),
            Value::Float(v) => Ok(Value::Bool(v != 0.0)),
            Value::Str(text) => {
                let normalized = text.trim().to_ascii_lowercase();
                match normalized.as_str() {
                    "true" | "1" => Ok(Value::Bool(true)),
                    "false" | "0" => Ok(Value::Bool(false)),
                    _ => Err(format!(
                        "Line {line}: variable '{name}' cannot be converted to boolean"
                    )),
                }
            }
            _ => Err(format!(
                "Line {line}: variable '{name}' cannot be converted to boolean"
            )),
        },
        "list" => match value {
            Value::List(_) => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' cannot be converted to list")),
        },
        "group" | "set" => match value {
            Value::Set(_) => Ok(value),
            Value::List(v) => {
                let mut seen: HashSet<String> = HashSet::new();
                let mut unique: Vec<Value> = Vec::new();
                for item in v {
                    let key = format!("{}", item);
                    if seen.insert(key) {
                        unique.push(item);
                    }
                }
                Ok(Value::Set(unique))
            }
            _ => Err(format!("Line {line}: variable '{name}' cannot be converted to group")),
        },
        "dictionary" | "dict" => match value {
            Value::Dict(_) => Ok(value),
            _ => Err(format!(
                "Line {line}: variable '{name}' cannot be converted to dictionary"
            )),
        },
        "module" => match value {
            Value::Module(_) => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' cannot be converted to module")),
        },
        "empty" => match value {
            Value::Empty => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' cannot be converted to empty")),
        },
        "color" => match convert_type(line, name, "string", value)? {
            Value::Str(text) if is_valid_color_literal(&text) => Ok(Value::Str(text)),
            _ => Err(format!("Line {line}: variable '{name}' cannot be converted to color")),
        },
        "dynamic" => Ok(value),
        _ => Err(format!("Unknown type '{ty}' for '{name}'")),
    }
}

#[derive(Debug, Clone)]
enum Expr {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Empty,
    Var(Vec<String>),
    List(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
    Index { target: Box<Expr>, index: Box<Expr> },
    Slice {
        target: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        step: Option<Box<Expr>>,
    },
    Call { callee: Vec<String>, args: Vec<Expr> },
    Unary { op: String, rhs: Box<Expr> },
    Binary {
        op: String,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Comprehension {
        kind: String,      // "list" or "dict"
        result_expr: Box<Expr>,
        key_expr: Option<Box<Expr>>,   // for dict comprehensions
        item_name: String,
        iterable: Box<Expr>,
        condition: Option<Box<Expr>>,  // optional "if" filter
    },
    Lambda {
        params: Vec<String>,
        body: Box<Expr>,
    },
    Ternary {
        cond: Box<Expr>,
        true_expr: Box<Expr>,
        false_expr: Box<Expr>,
    },
}

// Preprocess an expression string: convert space-separated function calls
// like "len text" into parenthesized form "len(text)" so they work in
// all expression contexts (say, if, is-assign, etc.).
fn preprocess_expr_calls(expr: &str) -> String {
    // Only convert if no parentheses are already present (avoid double-processing)
    if expr.contains('(') || expr.contains(')') {
        return expr.to_string();
    }
    // Check if it looks like a simple space-separated call: callee arg1 arg2 ...
    if let Some((callee, args_text)) = expr.split_once(' ') {
        if looks_like_callee(callee)
            && !is_keyword(callee)
            && !args_text.is_empty()
            && !contains_expr_operators(args_text)
        {
            let args: Vec<String> = parse_inline_args(args_text)
                .into_iter()
                .map(|a| match a {
                    ArgItem::Positional(e) => e,
                    ArgItem::Named { name, expr: e } => format!("{name}={e}"),
                    ArgItem::DefVar(_) => String::new(),
                })
                .collect();
            return format!("{}({})", callee, args.join(", "));
        }
    }
    expr.to_string()
}

fn eval_expr(expr: &str, ctx: &mut ExecContext<'_>) -> Result<Value, String> {
    // Preprocess: convert space-separated calls like "len text" to "len(text)"
    // so they work in say, if, is-assign, and other expression contexts.
    let processed = preprocess_expr_calls(expr);
    let mut lexer = Lexer::new(&processed);
    let tokens = lexer.tokenize()?;
    let mut parser = ExprParser::new(tokens);
    let ast = parser.parse_expr()?;
    if !parser.is_done() {
        return Err(format!("Unexpected token in expression '{}'", expr));
    }
    eval_ast(&ast, ctx)
}

fn eval_ast(expr: &Expr, ctx: &mut ExecContext<'_>) -> Result<Value, String> {
    match expr {
        Expr::Int(v) => Ok(Value::Int(*v)),
        Expr::Float(v) => Ok(Value::Float(*v)),
        Expr::Bool(v) => Ok(Value::Bool(*v)),
        Expr::Str(v) => Ok(Value::Str(v.clone())),
        Expr::Empty => Ok(Value::Empty),
        Expr::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(eval_ast(item, ctx)?);
            }
            Ok(Value::List(out))
        }
        Expr::Dict(items) => {
            let mut out = HashMap::new();
            for (k_expr, v_expr) in items {
                let k_val = eval_ast(k_expr, ctx)?;
                let key = match k_val {
                    Value::Str(s) => s,
                    _ => return Err("Dictionary keys must evaluate to string values".to_string()),
                };
                let value = eval_ast(v_expr, ctx)?;
                out.insert(key, value);
            }
            Ok(Value::Dict(out))
        }
        Expr::Var(parts) => resolve_var_chain(parts, ctx),
        Expr::Index { target, index } => {
            let target_val = eval_ast(target, ctx)?;
            let index_val = eval_ast(index, ctx)?;
            resolve_index(target_val, index_val)
        }
        Expr::Slice {
            target,
            start,
            end,
            step,
        } => {
            let target_val = eval_ast(target, ctx)?;

            let start_bound = if let Some(expr) = start {
                let value = eval_ast(expr, ctx)?;
                parse_optional_slice_bound(&value, "slice start")?
            } else {
                None
            };

            let end_bound = if let Some(expr) = end {
                let value = eval_ast(expr, ctx)?;
                parse_optional_slice_bound(&value, "slice end")?
            } else {
                None
            };

            let step_value = if let Some(expr) = step {
                let value = eval_ast(expr, ctx)?;
                parse_slice_step_value(&value)?
            } else {
                1
            };

            slice_builtin(&target_val, start_bound, end_bound, step_value)
        }
        Expr::Call { callee, args } => {
            let name = callee.join(".");
            invoke_callable_expr(&name, args, ctx)
        }
        Expr::Unary { op, rhs } => {
            let v = eval_ast(rhs, ctx)?;
            match op.as_str() {
                "-" => match v {
                    Value::Int(i) => Ok(Value::Int(-i)),
                    Value::Float(f) => Ok(Value::Float(-f)),
                    _ => Err(format!("Unary '-' expects a number but got {}: {}", v.type_name(), v)),
                },
                "+" => match v {
                    Value::Int(_) | Value::Float(_) => Ok(v),
                    _ => Err(format!("Unary '+' expects a number but got {}: {}", v.type_name(), v)),
                },
                "not" => Ok(Value::Bool(!v.to_bool())),
                "~" => match v {
                    Value::Int(i) => Ok(Value::Int(!i)),
                    _ => Err(format!("Bitwise NOT '~' expects int, got {}: {}", v.type_name(), v)),
                },
                _ => Err(format!("Unsupported unary operator '{op}'")),
            }
        }
        Expr::Binary { op, lhs, rhs } => {
            // Python-style short-circuit: for `and`/`or`, only evaluate the
            // right operand when the left doesn't already decide the result.
            if op == "and" {
                let a = eval_ast(lhs, ctx)?;
                if !a.to_bool() {
                    return Ok(Value::Bool(false));
                }
                let b = eval_ast(rhs, ctx)?;
                return Ok(Value::Bool(b.to_bool()));
            }
            if op == "or" {
                let a = eval_ast(lhs, ctx)?;
                if a.to_bool() {
                    return Ok(Value::Bool(true));
                }
                let b = eval_ast(rhs, ctx)?;
                return Ok(Value::Bool(b.to_bool()));
            }
            let a = eval_ast(lhs, ctx)?;
            let b = eval_ast(rhs, ctx)?;
            eval_binary(op, a, b)
        }
        Expr::Comprehension { kind, result_expr, key_expr, item_name, iterable, condition } => {
            let iter = eval_ast(iterable, ctx)?;
            let items = match iter {
                Value::List(v) => v,
                Value::Set(v) => v,
                _ => return Err("Comprehension iterable must be a list or set".to_string()),
            };
            match kind.as_str() {
                "list" => {
                    let mut out = Vec::new();
                    for item in &items {
                        ctx.push_frame();
                        ctx.define_local(item_name, item.clone());
                        let include = if let Some(cond) = condition {
                            eval_ast(cond, ctx)?.to_bool()
                        } else {
                            true
                        };
                        if include {
                            out.push(eval_ast(result_expr, ctx)?);
                        }
                        ctx.pop_frame();
                    }
                    Ok(Value::List(out))
                }
                "dict" => {
                    let mut out = HashMap::new();
                    for item in &items {
                        ctx.push_frame();
                        ctx.define_local(item_name, item.clone());
                        let include = if let Some(cond) = condition {
                            eval_ast(cond, ctx)?.to_bool()
                        } else {
                            true
                        };
                        if include {
                            let k = eval_ast(key_expr.as_ref().unwrap(), ctx)?;
                            let v = eval_ast(result_expr, ctx)?;
                            out.insert(k.to_string(), v);
                        }
                        ctx.pop_frame();
                    }
                    Ok(Value::Dict(out))
                }
                _ => Err("Unknown comprehension kind".to_string()),
            }
        }
        Expr::Lambda { params, body } => {
            // Lambda creates a callable dict with captured parameter names and body expression
            let mut lambda_obj = HashMap::new();
            lambda_obj.insert("__lambda__".to_string(), Value::Bool(true));
            lambda_obj.insert("__lambda_params__".to_string(), Value::List(params.iter().map(|p| Value::Str(p.clone())).collect()));
            lambda_obj.insert("__lambda_body__".to_string(), Value::Str(format!("{:?}", body)));
            // For now, store as dict representation — full callable lambda needs runtime closure capture
            Ok(Value::Dict(lambda_obj))
        }
        Expr::Ternary { cond, true_expr, false_expr } => {
            let cond_val = eval_ast(cond, ctx)?;
            if cond_val.to_bool() {
                eval_ast(true_expr, ctx)
            } else {
                eval_ast(false_expr, ctx)
            }
        }
    }
}

fn resolve_var_chain(parts: &[String], ctx: &mut ExecContext<'_>) -> Result<Value, String> {
    if parts.is_empty() {
        return Err("Invalid identifier".to_string());
    }
    let first = &parts[0];
    let mut cur = if let Some(v) = ctx.get_var(first) {
        v
    } else if parts.len() == 1 {
        // Check if it's a known function — return as reference, don't call!
        if ctx.rt.callables.contains_key(first) || ctx.rt.funcs.contains_key(first) {
            return Ok(Value::Func(first.clone()));
        }
        // Recognized builtins that take arguments (invoked via Stmt::Call or Expr::Call)
        if matches!(first.as_str(), "set") {
            return Ok(Value::Func(first.clone()));
        }
        // Fallback: try as a zero-arg builtin
        if let Some(result) = invoke_builtin(first, &[]) {
            return result;
        }
        return Err(format!("Unknown identifier '{first}'"));
    } else {
        return Err(format!("Unknown identifier '{first}'"));
    };

    for p in parts.iter().skip(1) {
        match cur {
            Value::Module(ref m) => {
                let module_name = parts[0].clone();
                if let Some(v) = m.vars.get(p) {
                    cur = v.clone();
                } else if m.funcs.contains_key(p) {
                    return Err(format!("Module '{}' has function '{}' — use 'get {} from {}' to import it", module_name, p, p, module_name));
                } else {
                    return Err(format!("Module '{}' has no attribute '{}'", module_name, p));
                }
            }
            Value::Dict(ref map) => {
                if let Some(v) = map.get(p) {
                    cur = v.clone();
                } else {
                    return Err(format!("Dictionary has no key '{p}'"));
                }
            }
            Value::Object { ref fields, .. } => {
                if let Some(v) = fields.get(p) {
                    cur = v.clone();
                } else {
                    return Err(format!("Object has no field '{p}'"));
                }
            }
            _ => return Err(format!("Cannot access '.{}' on {} value", p, cur.type_name())),
        }
    }
    Ok(cur)
}

/// Assign a value to a dotted variable chain like `bot.commands`.
/// For simple names (no dots), works like ctx.set_var.
/// For dotted chains, resolves the parent dict and sets the key on it.
fn assign_var_chain(name: &str, value: Value, ctx: &mut ExecContext<'_>) {
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() == 1 {
        ctx.set_var(name, value);
        return;
    }

    // 2-part case: bot.commands — get bot dict, set commands key, write back
    let root_name = parts[0];
    if let Some(Value::Dict(mut map)) = ctx.get_var(root_name) {
        map.insert(parts[1].to_string(), value);
        ctx.set_var(root_name, Value::Dict(map));
    }
}

fn normalize_index(len: usize, raw: i64) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if raw >= 0 {
        let idx = raw as usize;
        if idx < len {
            Some(idx)
        } else {
            None
        }
    } else {
        let from_end = len as i64 + raw;
        if from_end >= 0 {
            Some(from_end as usize)
        } else {
            None
        }
    }
}

fn parse_optional_slice_bound(value: &Value, label: &str) -> Result<Option<i64>, String> {
    match value {
        Value::Empty => Ok(None),
        Value::Int(v) => Ok(Some(*v)),
        _ => Err(format!("{} must be int or empty", label)),
    }
}

fn parse_slice_step_value(value: &Value) -> Result<i64, String> {
    match value {
        Value::Empty => Ok(1),
        Value::Int(step) if *step != 0 => Ok(*step),
        Value::Int(_) => Err("slice step cannot be 0".to_string()),
        _ => Err("slice step must be int or empty".to_string()),
    }
}

fn build_slice_indices(
    len: usize,
    start: Option<i64>,
    end: Option<i64>,
    step: i64,
) -> Result<Vec<usize>, String> {
    if step == 0 {
        return Err("slice step cannot be 0".to_string());
    }

    if len == 0 {
        return Ok(vec![]);
    }

    let len_i = len as i64;
    let mut out = vec![];

    if step > 0 {
        let mut s = start.map(|v| if v < 0 { v + len_i } else { v }).unwrap_or(0);
        let mut e = end
            .map(|v| if v < 0 { v + len_i } else { v })
            .unwrap_or(len_i);

        s = s.clamp(0, len_i);
        e = e.clamp(0, len_i);

        let mut i = s;
        while i < e {
            out.push(i as usize);
            i += step;
        }
    } else {
        let mut s = start
            .map(|v| if v < 0 { v + len_i } else { v })
            .unwrap_or(len_i - 1);
        let mut e = end
            .map(|v| if v < 0 { v + len_i } else { v })
            .unwrap_or(-1);

        if s >= len_i {
            s = len_i - 1;
        }
        if s < -1 {
            s = -1;
        }
        if e >= len_i {
            e = len_i - 1;
        }
        if e < -1 {
            e = -1;
        }

        let mut i = s;
        while i > e {
            if i >= 0 && i < len_i {
                out.push(i as usize);
            }
            i += step;
        }
    }

    Ok(out)
}

fn slice_builtin(container: &Value, start: Option<i64>, end: Option<i64>, step: i64) -> Result<Value, String> {
    match container {
        Value::List(items) => {
            let indices = build_slice_indices(items.len(), start, end, step)?;
            let mut out = Vec::with_capacity(indices.len());
            for idx in indices {
                if let Some(value) = items.get(idx) {
                    out.push(value.clone());
                }
            }
            Ok(Value::List(out))
        }
        Value::Str(text) => {
            let chars = text.chars().collect::<Vec<_>>();
            let indices = build_slice_indices(chars.len(), start, end, step)?;
            let mut out = String::new();
            for idx in indices {
                if let Some(ch) = chars.get(idx) {
                    out.push(*ch);
                }
            }
            Ok(Value::Str(out))
        }
        _ => Err(format!("slice expects a list or string, got {}", container.type_name())),
    }
}

fn resolve_index(target: Value, index: Value) -> Result<Value, String> {
    match target {
        Value::List(items) => {
            let raw = match index {
                Value::Int(i) => i,
                _ => return Err(format!("List index must be an int, got {}: {}", index.type_name(), index)),
            };
            let idx = normalize_index(items.len(), raw)
                .ok_or_else(|| format!("List index out of range: {raw}"))?;
            items
                .get(idx)
                .cloned()
                .ok_or_else(|| format!("List index out of range: {idx}"))
        }
        Value::Str(s) => {
            let raw = match index {
                Value::Int(i) => i,
                _ => return Err(format!("String index must be an int, got {}: {}", index.type_name(), index)),
            };
            let idx = normalize_index(s.chars().count(), raw)
                .ok_or_else(|| format!("String index out of range: {raw}"))?;
            let ch = s
                .chars()
                .nth(idx)
                .ok_or_else(|| format!("String index out of range: {idx}"))?;
            Ok(Value::Str(ch.to_string()))
        }
        Value::Dict(map) => {
            let key = match index {
                Value::Str(s) => s,
                _ => return Err(format!("Dictionary key must be a string, got {}: {}", index.type_name(), index)),
            };
            map.get(&key)
                .cloned()
                .ok_or_else(|| format!("Dictionary key not found: {key}"))
        }
        _ => Err(format!("Indexing is not supported for {} values", target.type_name())),
    }
}

fn assign_index_value(target: Value, index: Value, replacement: Value) -> Result<Value, String> {
    match target {
        Value::List(mut items) => {
            let raw = match index {
                Value::Int(i) => i,
                _ => return Err(format!("List index must be an int, got {}: {}", index.type_name(), index)),
            };
            let idx = normalize_index(items.len(), raw)
                .ok_or_else(|| format!("List index out of range: {raw}"))?;
            items[idx] = replacement;
            Ok(Value::List(items))
        }
        Value::Dict(mut map) => {
            let key = match index {
                Value::Str(s) => s,
                _ => return Err(format!("Dictionary key must be a string, got {}: {}", index.type_name(), index)),
            };
            map.insert(key, replacement);
            Ok(Value::Dict(map))
        }
        _ => Err("Indexed assignment is only supported for list and dictionary values".to_string()),
    }
}

fn normalize_slice_window_step_one(
    len: usize,
    start: Option<i64>,
    end: Option<i64>,
) -> (usize, usize) {
    let len_i = len as i64;

    let normalize = |value: Option<i64>, default: i64| {
        let raw = value.unwrap_or(default);
        let shifted = if raw < 0 { raw + len_i } else { raw };
        shifted.clamp(0, len_i)
    };

    let start_i = normalize(start, 0);
    let mut end_i = normalize(end, len_i);
    if end_i < start_i {
        end_i = start_i;
    }

    (start_i as usize, end_i as usize)
}

fn assign_slice_value(
    target: Value,
    start: Option<i64>,
    end: Option<i64>,
    step: i64,
    replacement: Value,
) -> Result<Value, String> {
    let replacement_items = match replacement {
        Value::List(items) => items,
        _ => return Err(format!("Slice assignment expects a list, got {}", replacement.type_name())),
    };

    match target {
        Value::List(mut items) => {
            if step == 1 {
                let (start_idx, end_idx) = normalize_slice_window_step_one(items.len(), start, end);
                items.splice(start_idx..end_idx, replacement_items);
                return Ok(Value::List(items));
            }

            let indices = build_slice_indices(items.len(), start, end, step)?;
            if replacement_items.len() != indices.len() {
                return Err(format!(
                    "Slice assignment with step {} expects {} replacement items, got {}",
                    step,
                    indices.len(),
                    replacement_items.len()
                ));
            }

            for (idx, value) in indices.iter().copied().zip(replacement_items.into_iter()) {
                items[idx] = value;
            }
            Ok(Value::List(items))
        }
        _ => Err("Slice assignment is only supported for list values".to_string()),
    }
}

fn eval_binary(op: &str, a: Value, b: Value) -> Result<Value, String> {
    match op {
        "+" => {
            let a_type = a.type_name();
            let b_type = b.type_name();
            match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x + y)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x + y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(x as f64 + y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x + y as f64)),
            (Value::Str(x), y) => Ok(Value::Str(format!("{}{}", x, y))),
            (x, Value::Str(y)) => Ok(Value::Str(format!("{}{}", x, y))),
            (Value::List(mut x), Value::List(y)) => {
                x.extend(y);
                Ok(Value::List(x))
            }
            (Value::Set(mut x), Value::Set(y)) => {
                // Union: add y elements not already in x
                for item in y {
                    let key = format!("{}", item);
                    if !x.iter().any(|v| format!("{}", v) == key) {
                        x.push(item);
                    }
                }
                Ok(Value::Set(x))
            }
            (Value::Dict(mut x), Value::Dict(y)) => {
                x.extend(y);
                Ok(Value::Dict(x))
            }
            _ => Err(format!("Cannot add {} and {} with '+'", a_type, b_type)),
            }
        },
        "-" => numeric_bin(a, b, |x, y| x - y),
        "*" => numeric_bin(a, b, |x, y| x * y),
        "/" => numeric_bin(a, b, |x, y| x / y),
        "%" => numeric_bin(a, b, |x, y| x % y),
        "==" => Ok(Value::Bool(eq_values(&a, &b))),
        "!=" => Ok(Value::Bool(!eq_values(&a, &b))),
        ">" => cmp_bin(a, b, |x, y| x > y),
        ">=" => cmp_bin(a, b, |x, y| x >= y),
        "<" => cmp_bin(a, b, |x, y| x < y),
        "<=" => cmp_bin(a, b, |x, y| x <= y),
        "in" => Ok(Value::Bool(contains_value(&b, &a)?)),
        "not in" => Ok(Value::Bool(!contains_value(&b, &a)?)),
        "is" => Ok(Value::Bool(is_identity_equal(&a, &b))),
        "is not" => Ok(Value::Bool(!is_identity_equal(&a, &b))),
        "and" => Ok(Value::Bool(a.to_bool() && b.to_bool())),
        "or" => Ok(Value::Bool(a.to_bool() || b.to_bool())),
        "|" => bitwise_int_bin(a, b, |x, y| x | y),
        "&" => bitwise_int_bin(a, b, |x, y| x & y),
        "^" => bitwise_int_bin(a, b, |x, y| x ^ y),
        "<<" => bitwise_int_bin(a, b, |x, y| x << y),
        ">>" => bitwise_int_bin(a, b, |x, y| x >> y),
        _ => Err(format!("Unsupported operator '{}' for {} and {}", op, a.type_name(), b.type_name())),
    }
}

fn contains_value(container: &Value, item: &Value) -> Result<bool, String> {
    match container {
        Value::List(items) => Ok(items.iter().any(|v| eq_values(v, item))),
        Value::Set(items) => Ok(items.iter().any(|v| eq_values(v, item))),
        Value::Dict(map) => {
            let key = match item {
                Value::Str(s) => s,
                _ => return Err(format!("'in' with dict expects a string key, got {}: {}", item.type_name(), item)),
            };
            Ok(map.contains_key(key))
        }
        Value::Str(text) => {
            let needle = match item {
                Value::Str(s) => s,
                _ => return Err(format!("'in' with string expects a string needle, got {}: {}", item.type_name(), item)),
            };
            Ok(text.contains(needle))
        }
        _ => Err(format!("'in' expects a list, set, dict, or string container, got {}", container.type_name())),
    }
}

fn numeric_bin(a: Value, b: Value, op: fn(f64, f64) -> f64) -> Result<Value, String> {
    let x = a
        .as_number()
        .ok_or_else(|| format!("Expected a number but got {}: {}", a.type_name(), a))?;
    let y = b
        .as_number()
        .ok_or_else(|| format!("Expected a number but got {}: {}", b.type_name(), b))?;
    let out = op(x, y);
    if (out.fract()).abs() < f64::EPSILON {
        Ok(Value::Int(out as i64))
    } else {
        Ok(Value::Float(out))
    }
}

fn cmp_bin(a: Value, b: Value, op: fn(f64, f64) -> bool) -> Result<Value, String> {
    let x = a
        .as_number()
        .ok_or_else(|| format!("Expected a number but got {}: {}", a.type_name(), a))?;
    let y = b
        .as_number()
        .ok_or_else(|| format!("Expected a number but got {}: {}", b.type_name(), b))?;
    Ok(Value::Bool(op(x, y)))
}

fn bitwise_int_bin(a: Value, b: Value, op: fn(i64, i64) -> i64) -> Result<Value, String> {
    let x = match a {
        Value::Int(v) => v,
        _ => return Err(format!("Expected int but got {}: {}", a.type_name(), a)),
    };
    let y = match b {
        Value::Int(v) => v,
        _ => return Err(format!("Expected int but got {}: {}", b.type_name(), b)),
    };
    Ok(Value::Int(op(x, y)))
}

fn eq_values(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => (x - y).abs() < f64::EPSILON,
        (Value::Int(x), Value::Float(y)) => (*x as f64 - *y).abs() < f64::EPSILON,
        (Value::Float(x), Value::Int(y)) => (*x - *y as f64).abs() < f64::EPSILON,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::List(x), Value::List(y)) => {
            x.len() == y.len()
                && x
                    .iter()
                    .zip(y.iter())
                    .all(|(left, right)| eq_values(left, right))
        }
        (Value::Set(x), Value::Set(y)) => {
            x.len() == y.len()
                && x.iter().all(|item| {
                    let key = format!("{}", item);
                    y.iter().any(|other| format!("{}", other) == key && eq_values(item, other))
                })
        }
        (Value::Dict(x), Value::Dict(y)) => {
            x.len() == y.len()
                && x
                    .iter()
                    .all(|(k, left)| y.get(k).is_some_and(|right| eq_values(left, right)))
        }
        (Value::Module(x), Value::Module(y)) => Arc::ptr_eq(x, y),
        (Value::Empty, Value::Empty) => true,
        _ => false,
    }
}

fn is_identity_equal(a: &Value, b: &Value) -> bool {
    // Identity comparison: stricter than equality — checks same type + same value
    // For modules, uses pointer equality
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => (x - y).abs() < f64::EPSILON,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Func(x), Value::Func(y)) => x == y,
        (Value::Module(x), Value::Module(y)) => Arc::ptr_eq(x, y),
        (Value::Empty, Value::Empty) => true,
        (Value::Object { class_name: cn1, .. }, Value::Object { class_name: cn2, .. }) => {
            // Object identity: same class AND same field values
            cn1 == cn2 && eq_values(a, b)
        }
        (Value::List(_), Value::List(_)) | (Value::Dict(_), Value::Dict(_)) => {
            // Collections: identity means same reference (pointer equality), but since
            // we don't track references, fall back to structural equality
            eq_values(a, b)
        }
        _ => false, // different types are never identity-equal
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(String),
    String(String),
    Ident(String),
    Sym(String),
}

fn describe_token(tok: &Token) -> String {
    match tok {
        Token::Number(n) => format!("number '{}'", n),
        Token::String(s) => format!("string \"{}\"", s),
        Token::Ident(i) => format!("identifier '{}'", i),
        Token::Sym(s) => format!("symbol '{}'", s),
    }
}

struct Lexer<'a> {
    src: &'a str,
    i: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, i: 0 }
    }

    fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let bytes = self.src.as_bytes();
        let mut out = vec![];

        while self.i < bytes.len() {
            let c = bytes[self.i] as char;
            if c.is_whitespace() {
                self.i += 1;
                continue;
            }

            if c == '#' {
                out.push(Token::String(self.read_hex_color()?));
                continue;
            }

            if c == '"' || c == '\'' {
                out.push(Token::String(self.read_string(c)?));
                continue;
            }

            if c.is_ascii_digit() {
                out.push(Token::Number(self.read_number()));
                continue;
            }

            if is_ident_start(c) {
                let ident = self.read_ident();
                out.push(Token::Ident(ident));
                continue;
            }

            let two = if self.i + 1 < bytes.len() {
                Some(&self.src[self.i..self.i + 2])
            } else {
                None
            };
            if let Some(sym) = two {
                if ["==", "!=", ">=", "<=", "<<", ">>"].contains(&sym) {
                    out.push(Token::Sym(sym.to_string()));
                    self.i += 2;
                    continue;
                }
            }

            if [
                "+", "-", "*", "/", "%", "(", ")", "[", "]", "{", "}", ",", ".", ":", ">", "<",
                "&", "|", "^", "~", "!",
            ]
            .contains(&&self.src[self.i..self.i + 1])
            {
                out.push(Token::Sym(self.src[self.i..self.i + 1].to_string()));
                self.i += 1;
                continue;
            }

            return Err(format!("Unexpected character '{}' at position {} in expression", c, self.i));
        }

        Ok(out)
    }

    fn read_hex_color(&mut self) -> Result<String, String> {
        let bytes = self.src.as_bytes();
        let start = self.i;
        self.i += 1; // skip '#'

        while self.i < bytes.len() && (bytes[self.i] as char).is_ascii_hexdigit() {
            self.i += 1;
        }

        let hex = &self.src[start + 1..self.i];
        if !matches!(hex.len(), 3 | 4 | 6 | 8) {
            // Check if this looks like a mistaken comment (# instead of #!)
            let remainder = &self.src[self.i..];
            if remainder.trim_start().len() > 0
                && !remainder.trim_start().chars().next().map_or(false, |c| c.is_ascii_hexdigit())
            {
                return Err(
                    "Comments use #! not #. For hex colors, use a valid hex code like #ff4d4d."
                        .to_string(),
                );
            }
            return Err(format!(
                "Invalid hex color '{}'. Hex colors need 3, 4, 6, or 8 hex digits.",
                hex
            ));
        }

        Ok(format!("#{}", hex))
    }

    fn read_string(&mut self, quote: char) -> Result<String, String> {
        self.i += 1;
        let mut out = String::new();
        let src: &str = &self.src;
        let byte_len = src.len();
        while self.i < byte_len {
            if src.is_char_boundary(self.i) {
                let remaining = &src[self.i..];
                let ch = remaining.chars().next().unwrap();
                let ch_len = ch.len_utf8();
                self.i += ch_len;
                if ch == quote {
                    return Ok(out);
                }
                if ch == '\\' && self.i < byte_len {
                    let remaining2 = &src[self.i..];
                    if let Some(esc) = remaining2.chars().next() {
                        self.i += esc.len_utf8();
                        let mapped = match esc {
                            'n' => '\n',
                            't' => '\t',
                            '"' => '"',
                            '\'' => '\'',
                            '\\' => '\\',
                            _ => esc,
                        };
                        out.push(mapped);
                    }
                } else {
                    out.push(ch);
                }
            } else {
                self.i += 1;
            }
        }
        Err("Unterminated string literal".to_string())
    }

    fn read_number(&mut self) -> String {
        let bytes = self.src.as_bytes();
        let start = self.i;
        while self.i < bytes.len() && (bytes[self.i] as char).is_ascii_digit() {
            self.i += 1;
        }
        if self.i < bytes.len() && bytes[self.i] as char == '.' {
            self.i += 1;
            while self.i < bytes.len() && (bytes[self.i] as char).is_ascii_digit() {
                self.i += 1;
            }
        }
        self.src[start..self.i].to_string()
    }

    fn read_ident(&mut self) -> String {
        let bytes = self.src.as_bytes();
        let start = self.i;
        while self.i < bytes.len() && is_ident_part(bytes[self.i] as char) {
            self.i += 1;
        }
        self.src[start..self.i].to_string()
    }
}

struct ExprParser {
    tokens: Vec<Token>,
    i: usize,
}

impl ExprParser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, i: 0 }
    }

    fn is_done(&self) -> bool {
        self.i >= self.tokens.len()
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.i)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.i).cloned();
        self.i += 1;
        t
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_or()
    }

    /// Parse an expression but STOP at a top-level `if` keyword (used for the
    /// iterable in `[x for x in xs if cond]`). Otherwise `parse_or` would treat
    /// the filter `if` as the start of a ternary and demand an `else`.
    fn parse_expr_stop_if(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_and()?;
        loop {
            if self.peek_ident("if") {
                break;
            }
            if self.match_ident("or") {
                let rhs = self.parse_and()?;
                node = Expr::Binary {
                    op: "or".to_string(),
                    lhs: Box::new(node),
                    rhs: Box::new(rhs),
                };
            } else {
                break;
            }
        }
        Ok(node)
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_and()?;
        loop {
            if self.match_ident("or") {
                let rhs = self.parse_and()?;
                node = Expr::Binary {
                    op: "or".to_string(),
                    lhs: Box::new(node),
                    rhs: Box::new(rhs),
                };
            } else if self.match_ident("if") {
                // Ternary: true_expr if cond else false_expr
                let cond = self.parse_or()?;
                if !self.match_ident("else") {
                    return Err("Expected 'else' in ternary expression".to_string());
                }
                let false_expr = self.parse_or()?;
                node = Expr::Ternary {
                    cond: Box::new(cond),
                    true_expr: Box::new(node),
                    false_expr: Box::new(false_expr),
                };
            } else {
                break;
            }
        }
        Ok(node)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_cmp()?;
        loop {
            if self.match_ident("and") {
                let rhs = self.parse_cmp()?;
                node = Expr::Binary {
                    op: "and".to_string(),
                    lhs: Box::new(node),
                    rhs: Box::new(rhs),
                };
            } else {
                break;
            }
        }
        Ok(node)
    }

    fn parse_cmp(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_bit_or()?;
        loop {
            let op = if self.peek_ident("not") && self.peek_ident_n(1, "in") {
                self.i += 2;
                "not in".to_string()
            } else if self.peek_ident("is") && self.peek_ident_n(1, "not") {
                self.i += 2;
                "is not".to_string()
            } else if self.match_ident("is") {
                "is".to_string()
            } else if self.match_ident("in") {
                "in".to_string()
            } else {
                match self.peek() {
                    Some(Token::Sym(s)) if ["==", "!=", ">", ">=", "<", "<="].contains(&s.as_str()) => {
                        let op = s.clone();
                        let _ = self.next();
                        op
                    }
                    _ => break,
                }
            };
            let rhs = self.parse_add()?;
            // Check if next token is another comparison (chained: a < b < c)
            let next_is_cmp = match self.peek() {
                Some(Token::Sym(s)) if ["==", "!=", ">", ">=", "<", "<="].contains(&s.as_str()) => true,
                _ => self.peek_ident("in") || self.peek_ident("is")
                    || (self.peek_ident("not") && self.peek_ident_n(1, "in")),
            };
            if next_is_cmp {
                // a < b < c  →  (a < b) AND (b < c)
                // Save (a < b) as partial, then continue with b and next operator
                let lhs_partial = Expr::Binary {
                    op: op.clone(),
                    lhs: Box::new(node.clone()),
                    rhs: Box::new(rhs.clone()),
                };
                // Parse remaining chain: b < c < d ...
                let mut chain_parts = vec![lhs_partial];
                let mut middle = rhs;
                loop {
                    let next_op = if self.peek_ident("not") && self.peek_ident_n(1, "in") {
                        self.i += 2;
                        "not in".to_string()
                    } else if self.peek_ident("is") && self.peek_ident_n(1, "not") {
                        self.i += 2;
                        "is not".to_string()
                    } else if self.match_ident("is") {
                        "is".to_string()
                    } else if self.match_ident("in") {
                        "in".to_string()
                    } else {
                        match self.peek() {
                            Some(Token::Sym(s)) if ["==", "!=", ">", ">=", "<", "<="].contains(&s.as_str()) => {
                                let op = s.clone();
                                let _ = self.next();
                                op
                            }
                            _ => break,
                        }
                    };
                    let next_rhs = self.parse_add()?;
                    chain_parts.push(Expr::Binary {
                        op: next_op.clone(),
                        lhs: Box::new(middle.clone()),
                        rhs: Box::new(next_rhs.clone()),
                    });
                    middle = next_rhs;
                    // Check for further chaining
                    let more = match self.peek() {
                        Some(Token::Sym(s)) if ["==", "!=", ">", ">=", "<", "<="].contains(&s.as_str()) => true,
                        _ => self.peek_ident("in") || self.peek_ident("is")
                            || (self.peek_ident("not") && self.peek_ident_n(1, "in")),
                    };
                    if !more { break; }
                }
                // Combine chain_parts with AND
                let mut combined = chain_parts.remove(0);
                for part in chain_parts {
                    combined = Expr::Binary {
                        op: "and".to_string(),
                        lhs: Box::new(combined),
                        rhs: Box::new(part),
                    };
                }
                return Ok(combined);
            }
            node = Expr::Binary {
                op,
                lhs: Box::new(node),
                rhs: Box::new(rhs),
            };
        }
        Ok(node)
    }

    fn parse_bit_or(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_bit_xor()?;
        loop {
            match self.peek() {
                Some(Token::Sym(s)) if s == "|" => {
                    let _ = self.next();
                    let rhs = self.parse_bit_xor()?;
                    node = Expr::Binary { op: "|".to_string(), lhs: Box::new(node), rhs: Box::new(rhs) };
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn parse_bit_xor(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_bit_and()?;
        loop {
            match self.peek() {
                Some(Token::Sym(s)) if s == "^" => {
                    let _ = self.next();
                    let rhs = self.parse_bit_and()?;
                    node = Expr::Binary { op: "^".to_string(), lhs: Box::new(node), rhs: Box::new(rhs) };
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn parse_bit_and(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_shift()?;
        loop {
            match self.peek() {
                Some(Token::Sym(s)) if s == "&" => {
                    let _ = self.next();
                    let rhs = self.parse_shift()?;
                    node = Expr::Binary { op: "&".to_string(), lhs: Box::new(node), rhs: Box::new(rhs) };
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn parse_shift(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_add()?;
        loop {
            match self.peek() {
                Some(Token::Sym(s)) if s == "<<" || s == ">>" => {
                    let op = s.clone();
                    let _ = self.next();
                    let rhs = self.parse_add()?;
                    node = Expr::Binary { op, lhs: Box::new(node), rhs: Box::new(rhs) };
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Some(Token::Sym(s)) if s == "+" || s == "-" => s.clone(),
                _ => break,
            };
            let _ = self.next();
            let rhs = self.parse_mul()?;
            node = Expr::Binary {
                op,
                lhs: Box::new(node),
                rhs: Box::new(rhs),
            };
        }
        Ok(node)
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Some(Token::Sym(s)) if s == "*" || s == "/" || s == "%" => s.clone(),
                _ => break,
            };
            let _ = self.next();
            let rhs = self.parse_unary()?;
            node = Expr::Binary {
                op,
                lhs: Box::new(node),
                rhs: Box::new(rhs),
            };
        }
        Ok(node)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if let Some(Token::Sym(s)) = self.peek() {
            if s == "+" || s == "-" || s == "~" {
                let op = s.clone();
                let _ = self.next();
                let rhs = self.parse_unary()?;
                return Ok(Expr::Unary {
                    op,
                    rhs: Box::new(rhs),
                });
            }
        }
        if self.match_ident("not") {
            let rhs = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: "not".to_string(),
                rhs: Box::new(rhs),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_primary()?;
        loop {
            if self.match_sym("(") {
                let mut args = vec![];
                if !self.match_sym(")") {
                    loop {
                        args.push(self.parse_expr()?);
                        if self.match_sym(",") {
                            continue;
                        }
                        self.expect_sym(")")?;
                        break;
                    }
                }

                let callee = match node {
                    Expr::Var(parts) => parts,
                    _ => return Err("Function call target must be an identifier".to_string()),
                };
                node = Expr::Call { callee, args };
                continue;
            }

            if self.match_sym("[") {
                if self.match_sym(":") {
                    let (end, step) = self.parse_slice_tail()?;
                    node = Expr::Slice {
                        target: Box::new(node),
                        start: None,
                        end: end.map(Box::new),
                        step: step.map(Box::new),
                    };
                    continue;
                }

                let first = self.parse_expr()?;
                if self.match_sym("]") {
                    node = Expr::Index {
                        target: Box::new(node),
                        index: Box::new(first),
                    };
                    continue;
                }

                if self.match_sym(":") {
                    let (end, step) = self.parse_slice_tail()?;
                    node = Expr::Slice {
                        target: Box::new(node),
                        start: Some(Box::new(first)),
                        end: end.map(Box::new),
                        step: step.map(Box::new),
                    };
                    continue;
                }

                return Err("Expected ']' or ':' in subscript".to_string());
            }
            break;
        }
        Ok(node)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        let tok = self
            .next()
            .ok_or_else(|| "Unexpected end of expression (expected a value)".to_string())?;
        match tok {
            Token::Number(n) => {
                if n.contains('.') {
                    n.parse::<f64>()
                        .map(Expr::Float)
                        .map_err(|_| format!("Invalid float literal '{}'", n))
                } else {
                    n.parse::<i64>()
                        .map(Expr::Int)
                        .map_err(|_| format!("Invalid integer literal '{}'", n))
                }
            }
            Token::String(s) => Ok(Expr::Str(s)),
            Token::Ident(name) => {
                if name == "fn" {
                    // Lambda: fn(param1, param2): expression  or  fn param: expression
                    let params: Vec<String>;
                    if self.match_sym("(") {
                        params = self.parse_param_list()?;
                        self.expect_sym(")")?;
                    } else {
                        // Single param: fn param: expression
                        let param = match self.next() {
                            Some(Token::Ident(p)) => p,
                            tok => return Err(format!("Expected parameter name in lambda, got {:?}", tok)),
                        };
                        params = vec![param];
                    }
                    // Expect colon before body
                    if !self.match_sym(":") {
                        return Err("Expected ':' after lambda params".to_string());
                    }
                    let body = self.parse_expr()?;
                    return Ok(Expr::Lambda {
                        params,
                        body: Box::new(body),
                    });
                }
                if name == "TRUE" || name == "YES" || name == "true" {
                    return Ok(Expr::Bool(true));
                }
                if name == "FALSE" || name == "NO" || name == "false" {
                    return Ok(Expr::Bool(false));
                }
                if name == "empty" || name == "null" {
                    return Ok(Expr::Empty);
                }

                let mut parts = vec![name];
                while self.match_sym(".") {
                    let next = self
                        .next()
                        .ok_or_else(|| "Expected identifier after '.'".to_string())?;
                    match next {
                        Token::Ident(s) => parts.push(s),
                        _ => return Err("Expected identifier after '.'".to_string()),
                    }
                }
                Ok(Expr::Var(parts))
            }
            Token::Sym(s) if s == "(" => {
                let inner = self.parse_expr()?;
                self.expect_sym(")")?;
                Ok(inner)
            }
            Token::Sym(s) if s == "[" => {
                if self.match_sym("]") {
                    return Ok(Expr::List(vec![]));
                }
                let first = self.parse_expr()?;

                // Check for list comprehension: [expr for item in iterable if cond]
                if self.match_ident("for") {
                    let item_name = match self.next() {
                        Some(Token::Ident(name)) => name,
                        tok => return Err(format!("Expected item variable in comprehension, got {:?}", tok)),
                    };
                    if !self.match_ident("in") {
                        return Err("Expected 'in' in comprehension".to_string());
                    }
                    let iterable = self.parse_expr_stop_if()?;
                    let condition = if self.match_ident("if") {
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    self.expect_sym("]")?;
                    return Ok(Expr::Comprehension {
                        kind: "list".to_string(),
                        result_expr: Box::new(first),
                        key_expr: None,
                        item_name,
                        iterable: Box::new(iterable),
                        condition: condition.map(Box::new),
                    });
                }

                // Normal list
                let mut items = vec![first];
                loop {
                    if self.match_sym(",") {
                        items.push(self.parse_expr()?);
                        continue;
                    }
                    self.expect_sym("]")?;
                    break;
                }
                Ok(Expr::List(items))
            }
            Token::Sym(s) if s == "{" => {
                if self.match_sym("}") {
                    return Ok(Expr::Dict(vec![]));
                }
                let first_key = self.parse_expr()?;

                // Check for dict comprehension: {key: value for item in iterable if cond}
                if self.peek_ident("for") {
                    // Need to have parsed key:value already
                    if !self.match_sym(":") {
                        return Err("Expected ':' in dict comprehension".to_string());
                    }
                    let first_val = self.parse_expr()?;
                    if !self.match_ident("for") {
                        return Err("Expected 'for' after key:value in dict comprehension".to_string());
                    }
                    let item_name = match self.next() {
                        Some(Token::Ident(name)) => name,
                        tok => return Err(format!("Expected item variable in comprehension, got {:?}", tok)),
                    };
                    if !self.match_ident("in") {
                        return Err("Expected 'in' in dict comprehension".to_string());
                    }
                    let iterable = self.parse_expr_stop_if()?;
                    let condition = if self.match_ident("if") {
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    self.expect_sym("}")?;
                    return Ok(Expr::Comprehension {
                        kind: "dict".to_string(),
                        result_expr: Box::new(first_val),
                        key_expr: Some(Box::new(first_key)),
                        item_name,
                        iterable: Box::new(iterable),
                        condition: condition.map(Box::new),
                    });
                }

                // Normal dict
                self.expect_sym(":")?;
                let first_val = self.parse_expr()?;
                let mut items = vec![(first_key, first_val)];
                loop {
                    if self.match_sym(",") {
                        let key = self.parse_expr()?;
                        self.expect_sym(":")?;
                        let value = self.parse_expr()?;
                        items.push((key, value));
                        continue;
                    }
                    self.expect_sym("}")?;
                    break;
                }
                Ok(Expr::Dict(items))
            }
            _ => Err(format!("Unexpected {} in expression", describe_token(&tok))),
        }
    }

    fn parse_param_list(&mut self) -> Result<Vec<String>, String> {
        let mut params = Vec::new();
        if self.peek_sym(")") {
            return Ok(params);
        }
        loop {
            match self.next() {
                Some(Token::Ident(name)) => params.push(name),
                tok => return Err(format!("Expected parameter name, got {:?}", tok)),
            }
            if self.match_sym(",") {
                continue;
            }
            break;
        }
        Ok(params)
    }

    fn match_sym(&mut self, expected: &str) -> bool {
        match self.peek() {
            Some(Token::Sym(s)) if s == expected => {
                self.i += 1;
                true
            }
            _ => false,
        }
    }

    fn match_ident(&mut self, expected: &str) -> bool {
        match self.peek() {
            Some(Token::Ident(s)) if s == expected => {
                self.i += 1;
                true
            }
            _ => false,
        }
    }

    fn expect_sym(&mut self, expected: &str) -> Result<(), String> {
        if self.match_sym(expected) {
            Ok(())
        } else {
            match self.peek() {
                Some(Token::Sym(found)) => Err(format!("Expected '{}' but found '{}'", expected, found)),
                Some(tok) => Err(format!("Expected '{}' but found {}", expected, describe_token(tok))),
                None => Err(format!("Expected '{}' but reached end of expression", expected)),
            }
        }
    }

    fn parse_slice_tail(&mut self) -> Result<(Option<Expr>, Option<Expr>), String> {
        let end = if self.peek_sym("]") || self.peek_sym(":") {
            None
        } else {
            Some(self.parse_expr()?)
        };

        if self.match_sym("]") {
            return Ok((end, None));
        }

        self.expect_sym(":")?;
        let step = if self.match_sym("]") {
            None
        } else {
            let value = self.parse_expr()?;
            self.expect_sym("]")?;
            Some(value)
        };

        Ok((end, step))
    }

    fn peek_ident(&self, expected: &str) -> bool {
        match self.peek() {
            Some(Token::Ident(s)) => s == expected,
            _ => false,
        }
    }

    fn peek_ident_n(&self, n: usize, expected: &str) -> bool {
        match self.tokens.get(self.i + n) {
            Some(Token::Ident(s)) => s == expected,
            _ => false,
        }
    }

    fn peek_sym(&self, expected: &str) -> bool {
        match self.peek() {
            Some(Token::Sym(s)) => s == expected,
            _ => false,
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_ident_part(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn preprocess(source: &str) -> Result<Vec<SourceLine>, String> {
    let mut out = vec![];
    let mut in_multi = false;

    for (idx, raw) in source.lines().enumerate() {
        let line_no = idx + 1;

        if in_multi {
            if raw.contains("#!*") {
                in_multi = false;
            }
            continue;
        }

        if raw.contains("#!*") {
            in_multi = true;
            continue;
        }

        let mut clean = strip_inline_comment(raw);
        clean = clean.replace('\t', "    ");
        if clean.trim().is_empty() {
            continue;
        }

        let indent = clean.len() - clean.trim_start_matches(' ').len();
        out.push(SourceLine {
            line_no,
            indent,
            text: clean.trim().to_string(),
        });
    }

    if in_multi {
        return Err("Unclosed multiline comment".to_string());
    }

    Ok(out)
}

fn strip_inline_comment(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    let mut in_str = false;
    let mut quote = '\0';
    let mut escape = false;

    while let Some(c) = chars.next() {
        if escape {
            out.push(c);
            escape = false;
            continue;
        }
        if c == '\\' {
            out.push(c);
            escape = true;
            continue;
        }

        if in_str {
            out.push(c);
            if c == quote {
                in_str = false;
            }
            continue;
        }

        if c == '"' || c == '\'' {
            in_str = true;
            quote = c;
            out.push(c);
            continue;
        }

        if c == '#' {
            if let Some('!') = chars.peek().copied() {
                break;
            }
        }

        out.push(c);
    }

    out
}

struct Parser {
    lines: Vec<SourceLine>,
    i: usize,
}

impl Parser {
    fn new(lines: Vec<SourceLine>) -> Self {
        Self { lines, i: 0 }
    }

    fn parse(&mut self) -> Result<Vec<Stmt>, String> {
        self.parse_block(0)
    }

    fn parse_block(&mut self, expected_indent: usize) -> Result<Vec<Stmt>, String> {
        let mut out = vec![];
        while let Some(line) = self.peek() {
            if line.indent < expected_indent {
                break;
            }
            if line.indent > expected_indent {
                return Err(format!(
                    "Unexpected indentation at line {}: '{}'",
                    line.line_no, line.text
                ));
            }
            out.push(self.parse_stmt(expected_indent)?);
        }
        Ok(out)
    }

    fn parse_stmt(&mut self, expected_indent: usize) -> Result<Stmt, String> {
        let line = self.consume()?;
        let text = line.text.as_str();

        if let Some(rest) = text.strip_prefix("say ") {
            return Ok(Stmt::Say {
                line: line.line_no,
                expr: rest.trim().to_string(),
            });
        }

        // Backward compat: Indent-1 say: removed in 1.2

        if let Some(rest) = text
            .strip_prefix("give ")
        {
            let expr = rest.trim().to_string();
            // If it looks like a call with space-separated args, convert to parenthesized form
            // But only if the expression doesn't already contain parentheses
            if !expr.contains('(') && !expr.contains(')') {
                if let Some((callee, args_text)) = expr.split_once(' ') {
                    if looks_like_callee(callee) && !is_keyword(callee)
                        && !contains_expr_operators(args_text)
                    {
                        let args: Vec<String> = parse_inline_args(args_text)
                            .into_iter()
                            .map(|a| match a {
                                ArgItem::Positional(e) => e,
                                ArgItem::Named { name, expr: e } => format!("{name}={e}"),
                                ArgItem::DefVar(_) => String::new(),
                            })
                            .collect();
                        let call_expr = format!("{}({})", callee, args.join(", "));
                        return Ok(Stmt::Give {
                            line: line.line_no,
                            expr: call_expr,
                        });
                    }
                }
            }
            return Ok(Stmt::Give {
                line: line.line_no,
                expr,
            });
        }

        if let Some(rest) = text
            .strip_prefix("return ")
            .or_else(|| text.strip_prefix("return:"))
        {
            return Ok(Stmt::Give {
                line: line.line_no,
                expr: rest.trim().to_string(),
            });
        }

        // Indent-2: set varname type — convert existing variable's type
        if let Some(rest) = text.strip_prefix("set ") {
            let rest = rest.trim();
            // Check if it looks like "set varname type" (not a function call with args)
            // Must have exactly 2 parts: name and type
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() == 2 && is_identifier(parts[0]) && is_identifier(parts[1]) {
                return Ok(Stmt::MakeType {
                    line: line.line_no,
                    target_type: parts[1].to_string(),
                    name: parts[0].to_string(),
                });
            }
            // Otherwise fall through — could be "set [1,2,3]" which is handled as BareExpr
        }

        // Indent-2: var name type = value
        if let Some(rest) = text.strip_prefix("var ") {
            let rest = rest.trim();
            if rest.is_empty() {
                return Err(format!(
                    "Variable declaration at line {} is missing a value. Use: var name type = value",
                    line.line_no
                ));
            }
            // Split: name type = value (type may be omitted for dynamic)
            if let Some((name_and_type, value_expr)) = rest.split_once(" = ") {
                let parts: Vec<&str> = name_and_type.split_whitespace().collect();
                if parts.is_empty() {
                    return Err(format!("Variable name missing at line {}", line.line_no));
                }
                let name = parts[0].to_string();
                let ty = if parts.len() >= 2 {
                    parts[1..].join(" ")
                } else {
                    // No explicit type — infer from value expression
                    infer_type_from_expr(value_expr).to_string()
                };

                // Check for ask expression
                if let Some(prompt) = value_expr.strip_prefix("ask ") {
                    let prompt = prompt.trim();

                    // Detect ask("type", "prompt") pattern: two quoted strings
                    // If the user wrote: ask "int" "How old are you? "
                    // the prompt would be: "int" "How old are you? "
                    // This is ambiguous in bare-call syntax — tell them to use parens.
                    if prompt.starts_with('"') || prompt.starts_with('\'') {
                        let quote = prompt.chars().next().unwrap();
                        if let Some(end) = find_string_end(prompt, quote) {
                            let first = &prompt[..=end];
                            let rest = prompt[end + 1..].trim();
                            if !rest.is_empty() {
                                // If rest looks like expression continuation (+ name + "?"),
                                // fall through to let the general expression evaluator handle it.
                                let is_expr_continuation = rest.starts_with('+')
                                    || rest.starts_with('-')
                                    || rest.starts_with('*')
                                    || rest.starts_with('/')
                                    || rest.starts_with('%')
                                    || rest.starts_with("and")
                                    || rest.starts_with("or")
                                    || rest.starts_with("==")
                                    || rest.starts_with("!=")
                                    || rest.starts_with(">=")
                                    || rest.starts_with("<=")
                                    || rest.starts_with('>')
                                    || rest.starts_with('<');
                                if !is_expr_continuation {
                                    // There's more after the first quoted string — looks like
                                    // ask "type" "prompt" which needs parentheses
                                    return Err(format!(
                                        "Line {}: ask with a type argument needs parentheses.\n\
                                         Use: var {} {} = ask(\"type\", \"Your prompt? \")\n\
                                         Example: var {} {} = ask(\"{}\", {})",
                                        line.line_no, name, ty, name, ty,
                                    first.trim_matches(quote),
                                    rest
                                ));
                                }
                            }
                        }
                    }

                    // Remove surrounding quotes if present (single prompt string)
                    let prompt_str = if (prompt.starts_with('"') && prompt.ends_with('"'))
                        || (prompt.starts_with('\'') && prompt.ends_with('\''))
                    {
                        prompt[1..prompt.len()-1].to_string()
                    } else {
                        prompt.to_string()
                    };
                    let ask_call = if ty.to_ascii_lowercase() == "dynamic" {
                        // No explicit type — use plain ask(prompt) which returns string
                        format!("ask(\"{}\")", prompt_str)
                    } else {
                        format!("ask(\"{}\", \"{}\")", ty.to_ascii_lowercase(), prompt_str)
                    };
                    return Ok(Stmt::DefVar {
                        line: line.line_no,
                        name,
                        ty: ty.to_ascii_lowercase(),
                        value: ValueSource::Expr(ask_call),
                    });
                }

                // Check if value is a space-separated function call: callee arg1 arg2 ...
                // Only treat as call if no expression operators are present
                if let Some((callee, args_text)) = value_expr.split_once(' ') {
                    if looks_like_callee(callee) && !is_keyword(callee)
                        && !contains_expr_operators(args_text)
                    {
                        let args = parse_inline_args(args_text);
                        return Ok(Stmt::DefVar {
                            line: line.line_no,
                            name,
                            ty: ty.to_ascii_lowercase(),
                            value: ValueSource::Call { callee: callee.to_string(), args },
                        });
                    }
                }

                // Check if value is a single identifier that looks like a zero-arg call
                if looks_like_callee(value_expr) && !is_keyword(value_expr)
                    && !is_literal(value_expr)
                {
                    return Ok(Stmt::DefVar {
                        line: line.line_no,
                        name,
                        ty: ty.to_ascii_lowercase(),
                        value: ValueSource::Call { callee: value_expr.to_string(), args: vec![] },
                    });
                }

                return Ok(Stmt::DefVar {
                    line: line.line_no,
                    name,
                    ty: ty.to_ascii_lowercase(),
                    value: ValueSource::Expr(value_expr.to_string()),
                });
            }

            // No = sign: treat as name only, type dynamic, value empty
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.is_empty() {
                return Err(format!("Variable name missing at line {}", line.line_no));
            }
            let name = parts[0].to_string();
            let ty = if parts.len() >= 2 {
                parts[1].to_string()
            } else {
                "dynamic".to_string()
            };
            return Ok(Stmt::DefVar {
                line: line.line_no,
                name,
                ty: ty.to_ascii_lowercase(),
                value: ValueSource::Expr("empty".to_string()),
            });
        }

        // Backward compat: Indent-1 def.var: — REMOVED in 1.2

        // Indent-2: fun name param1 param2 ...  (inline params)
        // Also: fun name  (with indented param lines)
        // Also: fun name param1 param2 as returnType
        // Also: fun name param = default_value (default params)
        // Also: async fun name ...  (calling it returns a future)
        let def_is_async;
        let rest_str;
        if let Some(r) = text
            .strip_prefix("async fun ")
            .or_else(|| text.strip_prefix("Async fun "))
        {
            def_is_async = true;
            rest_str = r.trim().to_string();
        } else if let Some(r) = text.strip_prefix("fun ").or_else(|| text.strip_prefix("Fun ")) {
            def_is_async = false;
            rest_str = r.trim().to_string();
        } else {
            def_is_async = false;
            rest_str = String::new();
        }
        if def_is_async || !rest_str.is_empty() {
            let rest = rest_str.as_str();
            if rest.is_empty() {
                return Err(format!("Function name missing at line {}", line.line_no));
            }
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let name = parts[0].to_string();
            let mut tail: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

            // Collapse `=` default values: "param", "=", "value" → param with default
            let mut i = 0;
            while i + 1 < tail.len() {
                if tail[i + 1] == "=" {
                    // Next token after = is the default value
                    let param_name = tail[i].clone();
                    let default_val = if i + 2 < tail.len() { tail[i + 2].clone() } else { "empty".to_string() };
                    tail[i] = format!("{}={}", param_name, default_val);
                    tail.remove(i + 1); // remove =
                    if i + 1 < tail.len() { tail.remove(i + 1); } // remove value
                }
                i += 1;
            }

            // Detect "as type" suffix for return type
            let (inline_params, return_type) = if let Some(as_pos) = tail.iter().position(|s| s == "as") {
                if as_pos + 1 < tail.len() {
                    let params = tail[..as_pos].to_vec();
                    let ret_ty = tail[as_pos + 1..].join(" ");
                    (params, Some(ret_ty))
                } else {
                    return Err(format!("Expected return type after 'as' at line {}", line.line_no));
                }
            } else {
                (tail, None)
            };

            // If there are inline params, body follows immediately
            if !inline_params.is_empty() {
                let params: Vec<FunctionParam> = inline_params
                    .into_iter()
                    .map(|n| {
                        if n.starts_with("...") {
                            FunctionParam { name: n[3..].to_string(), ty: None, default_value: None, is_varargs: true }
                        } else if let Some((pname, default)) = n.split_once('=') {
                            FunctionParam { name: pname.to_string(), ty: None, default_value: Some(default.to_string()), is_varargs: false }
                        } else {
                            FunctionParam { name: n, ty: None, default_value: None, is_varargs: false }
                        }
                    })
                    .collect();
                let body = self.parse_nested_block(line.indent)?;
                return Ok(Stmt::DefFun {
                    line: line.line_no,
                    name,
                    params,
                    return_type,
                    body,
                    is_generator: false,
                    is_async: def_is_async,
                });
            }

            // No inline params: check for indented parameter lines
            let child_indent = match self.peek() {
                Some(next) if next.indent > line.indent => next.indent,
                _ => {
                    // No indented block at all - function with no params and no body
                    return Ok(Stmt::DefFun {
                        line: line.line_no,
                        name,
                        params: vec![],
                        return_type,
                        body: vec![],
                        is_generator: false,
                        is_async: def_is_async,
                    });
                }
            };

            // Collect bare-identifier lines as parameters
            let mut params: Vec<FunctionParam> = Vec::new();
            while let Some(next) = self.peek() {
                if next.indent != child_indent {
                    break;
                }
                // A parameter line is a bare identifier (not a keyword, no spaces, not a literal)
                let is_param = is_identifier(&next.text)
                    && !is_keyword(&next.text)
                    && !is_literal(&next.text)
                    && !next.text.contains(' ');
                if !is_param {
                    break;
                }
                let param_line = self.consume()?;
                params.push(FunctionParam {
                    name: param_line.text.clone(),
                    ty: None,
                    default_value: None,
                    is_varargs: false,
                });
            }

            // Body is whatever remains at child_indent
            let mut body = Vec::new();
            while let Some(next) = self.peek() {
                if next.indent < child_indent {
                    break;
                }
                if next.indent > child_indent {
                    return Err(format!(
                        "Unexpected indentation at line {} in function body",
                        next.line_no
                    ));
                }
                body.push(self.parse_stmt(child_indent)?);
            }

            return Ok(Stmt::DefFun {
                line: line.line_no,
                name,
                params,
                return_type,
                body,
                is_generator: false,
                is_async: def_is_async,
            });
        }

        // ---- Indent-2: class Name with fields and methods ----
        if let Some(rest) = text.strip_prefix("class ") {
            let rest = rest.trim();
            let (name, parent) = if let Some((n, p)) = rest.split_once(" from ") {
                (n.trim().to_string(), Some(p.trim().to_string()))
            } else {
                (rest.to_string(), None)
            };
            if name.is_empty() || name.contains(' ') {
                return Err(format!("Class name missing or invalid at line {}", line.line_no));
            }
            if let Some(ref p) = parent {
                if p.is_empty() || p.contains(' ') {
                    return Err(format!("Parent class name invalid at line {}", line.line_no));
                }
            }
            let child_indent = match self.peek() {
                Some(next) if next.indent > line.indent => next.indent,
                _ => {
                    return Err(format!(
                        "Expected indented class body after 'class {}' at line {}",
                        name, line.line_no
                    ))
                }
            };

            let mut fields: Vec<ClassField> = Vec::new();
            let mut methods: Vec<Stmt> = Vec::new();

            while let Some(next) = self.peek() {
                if next.indent < child_indent {
                    break;
                }
                if next.indent > child_indent {
                    return Err(format!(
                        "Unexpected indentation at line {} in class body",
                        next.line_no
                    ));
                }

                let is_var = next.text.starts_with("var ");
                let is_fun = next.text.starts_with("fun ");
                let line_no = next.line_no;

                if is_var {
                    let stmt = self.parse_stmt(child_indent)?;
                    if let Stmt::DefVar { name: vname, ty, .. } = &stmt {
                        fields.push(ClassField {
                            name: vname.clone(),
                            _ty: ty.clone(),
                        });
                    } else {
                        return Err(format!(
                            "Expected var declaration at line {}", line_no
                        ));
                    }
                } else if is_fun {
                    let stmt = self.parse_stmt(child_indent)?;
                    methods.push(stmt);
                } else {
                    return Err(format!(
                        "Expected var or fun in class body at line {}",
                        line_no
                    ));
                }
            }

            return Ok(Stmt::DefClass {
                line: line.line_no,
                name,
                parent,
                fields,
                methods,
            });
        }

        // Indent-1 def.fun: — REMOVED in 1.2

        // Indent-2: if condition (no colon)
        if text.starts_with("if ") && !text.ends_with(':') {
            return self.parse_if_chain_no_colon(line);
        }

        // Backward compat: if condition:
        if text.starts_with("if ") && text.ends_with(':') {
            return self.parse_if_chain(line);
        }

        // Indent-2: match subject:
        if text.starts_with("match ") && text.ends_with(':') {
            let line2 = SourceLine {
                line_no: line.line_no,
                indent: line.indent,
                text: format!("Match {}", &text[6..]),
            };
            return self.parse_match_chain(line2);
        }

        // Backward compat: Match subject:
        if text.starts_with("Match ") && text.ends_with(':') {
            return self.parse_match_chain(line);
        }

        // Indent-2: do:
        if text == "do:" || text == "Do:" {
            return self.parse_do_chain(line);
        }

        // Async: loop:  — run the event loop until all futures complete
        if text == "loop:" || text == "Loop:" {
            let body = self.parse_nested_block(line.indent)?;
            return Ok(Stmt::Loop {
                line: line.line_no,
                body,
            });
        }

        // Async: await expr
        if text.starts_with("await ") || text.starts_with("Await ") {
            let expr = text[6..].trim().to_string();
            return Ok(Stmt::Await {
                line: line.line_no,
                expr,
            });
        }

        // Async: wait <future>  (int)  or  wait <seconds>  (float)
        if text.starts_with("wait ") || text.starts_with("Wait ") {
            let expr = text[5..].trim().to_string();
            return Ok(Stmt::Wait {
                line: line.line_no,
                expr,
            });
        }

        // Async: async with <future> as name:
        if text.starts_with("async with ") || text.starts_with("Async with ") {
            let rest = text[11..].trim().to_string(); // strip "async with "
            let rest = rest.trim_end_matches(':').trim();
            if let Some((expr, binding)) = rest.split_once(" as ") {
                let binding = binding.trim().to_string();
                let body = self.parse_nested_block(line.indent)?;
                return Ok(Stmt::AsyncWith {
                    line: line.line_no,
                    expr: expr.trim().to_string(),
                    binding,
                    body,
                });
            }
            return Err(format!("async with expects 'async with <future> as name:' at line {}", line.line_no));
        }

        // Indent-2: open "file" as handle:  or  open "file" for read as handle:
        // with "file" as handle:  (Python-style alias for the same context manager)
        if text.starts_with("open ") || text.starts_with("Open ")
            || text.starts_with("with ") || text.starts_with("With ")
        {
            return self.parse_open(line);
        }

        // Indent-2: repeat keyword + for alias
        if text.starts_with("repeat") || text.starts_with("Repeat") {
            return self.parse_repeat(line);
        }

        // Indent-2: for x in list:  (alias for repeat for x in list)
        if text.starts_with("for ") {
            let for_rest = &text[4..]; // strip "for "
            // Build a synthetic "repeat for ..." line and parse it
            let synth_line = SourceLine {
                line_no: line.line_no,
                indent: line.indent,
                text: format!("repeat for {}", for_rest),
            };
            return self.parse_repeat(synth_line);
        }

        // Indent-2: while cond:  (alias for repeat while cond)
        if text.starts_with("while ") {
            let while_rest = &text[6..]; // strip "while "
            let synth_line = SourceLine {
                line_no: line.line_no,
                indent: line.indent,
                text: format!("repeat while {}", while_rest),
            };
            return self.parse_repeat(synth_line);
        }

        if let Some(rest) = text.strip_prefix("flag:").or_else(|| text.strip_prefix("Flag:")) {
            return Ok(Stmt::Flag {
                line: line.line_no,
                expr: rest.trim().to_string(),
            });
        }

        // Indent-2: yield expression
        if let Some(rest) = text.strip_prefix("yield ").or_else(|| text.strip_prefix("Yield ")) {
            return Ok(Stmt::Yield {
                line: line.line_no,
                expr: rest.trim().to_string(),
            });
        }
        if text == "yield" || text == "Yield" {
            return Ok(Stmt::Yield {
                line: line.line_no,
                expr: "empty".to_string(),
            });
        }

        // Decorator: @name or @name(args) before fun/class
        if text.starts_with("@") && text.len() > 1 {
            let decorator_text = text[1..].to_string();
            // Parse the decorator name and optional args
            let (dec_name, dec_args) = if let Some((n, a)) = decorator_text.split_once("(") {
                let args_text = a.trim_end_matches(")");
                let args = if args_text.is_empty() { vec![] } else {
                    parse_inline_args(args_text)
                };
                (n.to_string(), args)
            } else {
                (decorator_text, vec![])
            };
            // Parse the next statement as the decorated target
            let target = self.parse_stmt(expected_indent)?;
            return Ok(Stmt::Decorator {
                line: line.line_no,
                name: dec_name,
                args: dec_args,
                target: Box::new(target),
            });
        }

        // Indent-2: lowercase control flow
        if text == "stop" || text == "STOP" {
            return Ok(Stmt::Stop { line: line.line_no });
        }
        if text.eq_ignore_ascii_case("break") {
            return Ok(Stmt::Stop { line: line.line_no });
        }
        if text == "next" || text == "NEXT" {
            return Ok(Stmt::Next { line: line.line_no });
        }
        if text.eq_ignore_ascii_case("continue") {
            return Ok(Stmt::Next { line: line.line_no });
        }
        if text == "reset" || text == "RESET" {
            return Ok(Stmt::Reset { line: line.line_no });
        }
        if text.eq_ignore_ascii_case("restart") {
            return Ok(Stmt::Reset { line: line.line_no });
        }

        // Indent-2: get/import keyword for imports
        if text.starts_with("get ") || text.starts_with("Get ") || text.starts_with("get:") || text.starts_with("Get:") {
            return parse_import(&line);
        }
        // import alias for get
        if text.starts_with("import ") {
            let import_rest = &text[7..]; // strip "import "
            let synth_line = SourceLine {
                line_no: line.line_no,
                indent: line.indent,
                text: format!("get {}", import_rest),
            };
            return parse_import(&synth_line);
        }

        if let Some((target, expr)) =
            parse_subscript_assignment(text).map_err(|e| format!("{} at line {}", e, line.line_no))?
        {
            let value = if expr.ends_with(';') {
                let call_line = SourceLine {
                    line_no: line.line_no,
                    indent: line.indent,
                    text: expr.clone(),
                };
                match self.parse_call_with_args(call_line, expected_indent)? {
                    Stmt::Call { callee, args, .. } => ValueSource::Call { callee, args },
                    _ => unreachable!("parse_call_with_args returned non-call statement"),
                }
            } else {
                ValueSource::Expr(expr)
            };

            return Ok(match target {
                SubscriptAssignTarget::Index { name, index_expr } => Stmt::AssignIndex {
                    line: line.line_no,
                    name,
                    index_expr,
                    value,
                },
                SubscriptAssignTarget::Slice {
                    name,
                    start_expr,
                    end_expr,
                    step_expr,
                } => Stmt::AssignSlice {
                    line: line.line_no,
                    name,
                    start_expr,
                    end_expr,
                    step_expr,
                    value,
                },
            });
        }

        // Compound assignment: x += expr, x -= expr, etc.
        if let Some((name, op, expr)) = parse_compound_assignment(text) {
            let value = if let Some((callee, args_text)) = expr.split_once(' ') {
                if looks_like_callee(callee) && !is_keyword(callee)
                    && !contains_expr_operators(args_text)
                {
                    let args = parse_inline_args(args_text);
                    ValueSource::Call { callee: callee.to_string(), args }
                } else {
                    ValueSource::Expr(expr)
                }
            } else {
                ValueSource::Expr(expr)
            };
            return Ok(Stmt::AssignOp {
                line: line.line_no,
                name,
                op,
                value,
            });
        }

        if let Some((name, expr)) = parse_assignment(text) {
            let value = if expr.ends_with(';') {
                let call_line = SourceLine {
                    line_no: line.line_no,
                    indent: line.indent,
                    text: expr.clone(),
                };
                match self.parse_call_with_args(call_line, expected_indent)? {
                    Stmt::Call { callee, args, .. } => ValueSource::Call { callee, args },
                    _ => unreachable!("parse_call_with_args returned non-call statement"),
                }
            } else if let Some((callee, args_text)) = expr.split_once(' ') {
                if looks_like_callee(callee) && !is_keyword(callee)
                    && !contains_expr_operators(args_text)
                {
                    let args = parse_inline_args(args_text);
                    ValueSource::Call { callee: callee.to_string(), args }
                } else {
                    ValueSource::Expr(expr)
                }
            } else {
                ValueSource::Expr(expr)
            };
            return Ok(Stmt::Assign {
                line: line.line_no,
                name,
                value,
            });
        }

        if text.ends_with(';') {
            return self.parse_call_with_args(line, expected_indent);
        }

        // Indent-2: inline space-separated calls like `greet "World"` or `math.pow 2 8`
        if let Some((callee, args_text)) = text.split_once(' ') {
            if looks_like_callee(callee) && !args_text.is_empty() && !is_keyword(callee)
                && !contains_expr_operators(args_text)
            {
                // Parse space-separated arguments
                let args: Vec<ArgItem> = parse_inline_args(args_text);
                return Ok(Stmt::Call {
                    line: line.line_no,
                    callee: callee.to_string(),
                    args,
                });
            }
        }

        if looks_like_callee(text) && !text.contains('(') {
            return Ok(Stmt::Call {
                line: line.line_no,
                callee: text.to_string(),
                args: vec![],
            });
        }

        // If it has parentheses, treat as an expression (like set(data), len(x))
        if text.contains('(') && text.ends_with(')') {
            return Ok(Stmt::BareExpr {
                line: line.line_no,
                expr: text.to_string(),
            });
        }

        Ok(Stmt::BareExpr {
            line: line.line_no,
            expr: text.to_string(),
        })
    }

    fn parse_nested_block(&mut self, header_indent: usize) -> Result<Vec<Stmt>, String> {
        let Some(next) = self.peek() else {
            return Ok(vec![]);
        };
        if next.indent <= header_indent {
            return Ok(vec![]);
        }
        self.parse_block(next.indent)
    }

    fn parse_if_chain(&mut self, first: SourceLine) -> Result<Stmt, String> {
        let mut branches: Vec<(Option<String>, Vec<Stmt>)> = vec![];
        let cond = first.text[2..first.text.len() - 1].trim().to_string();
        let body = self.parse_nested_block(first.indent)?;
        branches.push((Some(cond), body));

        while let Some(next) = self.peek() {
            if next.indent != first.indent {
                break;
            }
            if next.text.starts_with("or ") && next.text.ends_with(':') {
                let line = self.consume()?;
                let c = line.text[2..line.text.len() - 1].trim().to_string();
                let b = self.parse_nested_block(line.indent)?;
                branches.push((Some(c), b));
                continue;
            }
            if next.text == "otherwise:" {
                let line = self.consume()?;
                let b = self.parse_nested_block(line.indent)?;
                branches.push((None, b));
                break;
            }
            break;
        }

        Ok(Stmt::IfChain {
            line: first.line_no,
            branches,
        })
    }

    fn parse_if_chain_no_colon(&mut self, first: SourceLine) -> Result<Stmt, String> {
        // Indent-2: if condition (no trailing colon)
        let mut branches: Vec<(Option<String>, Vec<Stmt>)> = vec![];
        let cond = first.text[2..].trim().to_string();
        let body = self.parse_nested_block(first.indent)?;
        branches.push((Some(cond), body));

        while let Some(next) = self.peek() {
            if next.indent != first.indent {
                break;
            }
            if next.text.starts_with("or ") && !next.text.ends_with(':') {
                let line = self.consume()?;
                let c = line.text[2..].trim().to_string();
                let b = self.parse_nested_block(line.indent)?;
                branches.push((Some(c), b));
                continue;
            }
            // else if alias
            if next.text.starts_with("else if ") {
                let line = self.consume()?;
                let c = line.text[8..].trim().to_string();
                let b = self.parse_nested_block(line.indent)?;
                branches.push((Some(c), b));
                continue;
            }
            // Backward compat: or ... :
            if next.text.starts_with("or ") && next.text.ends_with(':') {
                let line = self.consume()?;
                let c = line.text[2..line.text.len() - 1].trim().to_string();
                let b = self.parse_nested_block(line.indent)?;
                branches.push((Some(c), b));
                continue;
            }
            if next.text == "otherwise" || next.text == "otherwise:" {
                let line = self.consume()?;
                let b = self.parse_nested_block(line.indent)?;
                branches.push((None, b));
                break;
            }
            break;
        }

        Ok(Stmt::IfChain {
            line: first.line_no,
            branches,
        })
    }

    fn parse_match_chain(&mut self, first: SourceLine) -> Result<Stmt, String> {
        let subject_expr = first.text[5..first.text.len() - 1].trim().to_string();
        if subject_expr.is_empty() {
            return Err(format!("Match expression missing at line {}", first.line_no));
        }

        let case_indent = match self.peek() {
            Some(next) if next.indent > first.indent => next.indent,
            Some(next) => {
                return Err(format!(
                    "Expected indented case block after Match at line {}, found '{}'",
                    next.line_no, next.text
                ))
            }
            None => {
                return Err(format!(
                    "Expected at least one case after Match at line {}",
                    first.line_no
                ))
            }
        };

        let mut branches: Vec<(String, Vec<Stmt>)> = vec![];
        let mut otherwise_body: Option<Vec<Stmt>> = None;

        while let Some(next) = self.peek() {
            if next.indent < case_indent {
                break;
            }
            if next.indent > case_indent {
                return Err(format!(
                    "Unexpected indentation at line {} inside Match block",
                    next.line_no
                ));
            }

            // Indent-2: case expr: or case expr (no colon for Indent-2 style)
            if (next.text.starts_with("case ") || next.text.starts_with("Case ")) && next.text.ends_with(':') {
                let line = self.consume()?;
                let case_expr = line.text[4..line.text.len() - 1].trim().to_string();
                if case_expr.is_empty() {
                    return Err(format!("case expression missing at line {}", line.line_no));
                }
                let body = self.parse_nested_block(line.indent)?;
                branches.push((case_expr, body));
                continue;
            }

            // Indent-2: otherwise (lowercase, no colon) or otherwise:
            if next.text == "otherwise" || next.text == "otherwise:" || next.text == "Otherwise:" {
                let line = self.consume()?;
                otherwise_body = Some(self.parse_nested_block(line.indent)?);
                break;
            }

            return Err(format!(
                "Expected 'case <expr>:' or 'otherwise:' in Match block at line {}",
                next.line_no
            ));
        }

        if branches.is_empty() {
            return Err(format!(
                "Match block requires at least one case at line {}",
                first.line_no
            ));
        }

        Ok(Stmt::Match {
            line: first.line_no,
            subject_expr,
            branches,
            otherwise_body,
        })
    }

    fn parse_do_chain(&mut self, first: SourceLine) -> Result<Stmt, String> {
        let do_body = self.parse_nested_block(first.indent)?;
        let mut catches: Vec<(Option<String>, Vec<Stmt>)> = vec![];
        let mut otherwise_body: Option<Vec<Stmt>> = None;
        let mut lastly_body: Option<Vec<Stmt>> = None;

        while let Some(next) = self.peek() {
            if next.indent != first.indent {
                break;
            }

            // Indent-2: catch (lowercase) or Catch (backward compat)
            if (next.text.starts_with("catch") || next.text.starts_with("Catch")) && next.text.ends_with(':') {
                let line = self.consume()?;
                let binding = parse_catch_binding(&line.text).map_err(|e| {
                    format!("{} at line {}", e, line.line_no)
                })?;
                let body = self.parse_nested_block(line.indent)?;
                catches.push((binding, body));
                continue;
            }

            // Indent-2: otherwise (lowercase) or Otherwise:
            if next.text == "otherwise" || next.text == "otherwise:" || next.text == "Otherwise:" {
                let line = self.consume()?;
                let body = self.parse_nested_block(line.indent)?;
                otherwise_body = Some(body);
                continue;
            }

            // Indent-2: lastly (lowercase) or Lastly:
            if next.text == "lastly" || next.text == "lastly:" || next.text == "Lastly:" {
                let line = self.consume()?;
                let body = self.parse_nested_block(line.indent)?;
                lastly_body = Some(body);
                continue;
            }

            break;
        }

        Ok(Stmt::DoChain {
            line: first.line_no,
            do_body,
            catches,
            otherwise_body,
            lastly_body,
        })
    }

    fn parse_open(&mut self, line: SourceLine) -> Result<Stmt, String> {
        // Syntax: open "file.txt" as handle:
        //         open "file.txt" for read as handle:
        //         open "file.txt" for write as handle:
        //         open "file.txt" for append as handle:
        let text = &line.text;
        let rest = text
            .strip_prefix("open ")
            .or_else(|| text.strip_prefix("Open "))
            .or_else(|| text.strip_prefix("with "))
            .or_else(|| text.strip_prefix("With "))
            .ok_or_else(|| format!("Invalid open syntax at line {}", line.line_no))?
            .trim();

        // Parse: [for mode] as binding
        let (path_expr, mode, binding) = if let Some((before_as, after_as)) = rest.split_once(" as ") {
            let binding = after_as.trim().trim_end_matches(':').trim();
            let binding = if binding.is_empty() { None } else { Some(binding.to_string()) };
            // Check if there's a "for mode" in the path expression
            if let Some((path, mode_str)) = before_as.split_once(" for ") {
                let mode = match mode_str.trim().to_lowercase().as_str() {
                    "read" | "r" => "read".to_string(),
                    "write" | "w" => "write".to_string(),
                    "append" | "a" => "append".to_string(),
                    other => return Err(format!("Unknown open mode '{}' at line {}. Use read, write, or append.", other, line.line_no)),
                };
                (path.trim().to_string(), mode, binding)
            } else {
                (before_as.trim().to_string(), "read".to_string(), binding)
            }
        } else {
            return Err(format!("Expected 'as' binding in open statement at line {}. Example: open \"file.txt\" as f:", line.line_no));
        };

        let body = self.parse_nested_block(line.indent)?;
        Ok(Stmt::Open {
            line: line.line_no,
            mode,
            path_expr,
            binding,
            body,
        })
    }

    fn parse_repeat(&mut self, line: SourceLine) -> Result<Stmt, String> {
        let body = self.parse_nested_block(line.indent)?;
        let text = &line.text;

        let mode = if text == "repeat" || text == "Repeat" || text == "Repeat:" || text == "repeat:" {
            RepeatMode::Infinite
        } else {
            let rest = text
                .strip_prefix("repeat ")
                .or_else(|| text.strip_prefix("Repeat "))
                .or_else(|| text.strip_prefix("repeat:"))
                .or_else(|| text.strip_prefix("Repeat:"))
                .unwrap_or("")
                .trim()
                .trim_end_matches(':')
                .trim()
                .to_string();
            if let Some(after) = rest.strip_prefix("for ") {
                if let Some((item, iter)) = after.split_once(" in ") {
                    RepeatMode::ForIn {
                        item_name: item.trim().to_string(),
                        iterable_expr: iter.trim().to_string(),
                    }
                } else {
                    RepeatMode::ForEach(after.trim().to_string())
                }
            } else if rest.contains(" in ") {
                // Indent-2: repeat item in list (without "for" keyword)
                if let Some((item, iter)) = rest.split_once(" in ") {
                    RepeatMode::ForIn {
                        item_name: item.trim().to_string(),
                        iterable_expr: iter.trim().to_string(),
                    }
                } else {
                    RepeatMode::Count(rest)
                }
            } else if let Some(after) = rest.strip_prefix("while ") {
                RepeatMode::While(after.trim().to_string())
            } else if let Some(after) = rest.strip_prefix("until ") {
                RepeatMode::Until(after.trim().to_string())
            } else {
                RepeatMode::Count(rest)
            }
        };

        Ok(Stmt::Repeat {
            line: line.line_no,
            mode,
            body,
        })
    }

    fn parse_call_with_args(&mut self, line: SourceLine, _expected_indent: usize) -> Result<Stmt, String> {
        let callee = line.text[..line.text.len() - 1].trim().to_string();
        if !looks_like_callee(&callee) {
            return Err(format!("Invalid function call at line {}", line.line_no));
        }

        let mut args: Vec<ArgItem> = vec![];
        let Some(next) = self.peek() else {
            return Ok(Stmt::Call {
                line: line.line_no,
                callee,
                args,
            });
        };
        if next.indent <= line.indent {
            return Ok(Stmt::Call {
                line: line.line_no,
                callee,
                args,
            });
        }

        let child_indent = next.indent;
        while let Some(cur) = self.peek() {
            if cur.indent < child_indent {
                break;
            }
            if cur.indent > child_indent {
                return Err(format!(
                    "Unexpected indentation in call arguments at line {}",
                    cur.line_no
                ));
            }

            if cur.text.starts_with("def.var:") {
                let stmt = self.parse_stmt(child_indent)?;
                args.push(ArgItem::DefVar(stmt));
                continue;
            }

            if let Some((name, expr)) = parse_assignment(&cur.text) {
                let _ = self.consume()?;
                args.push(ArgItem::Named { name, expr });
                continue;
            }

            let raw = self.consume()?.text;
            args.push(ArgItem::Positional(raw));
        }

        Ok(Stmt::Call {
            line: line.line_no,
            callee,
            args,
        })
    }

    fn peek(&self) -> Option<&SourceLine> {
        self.lines.get(self.i)
    }

    fn consume(&mut self) -> Result<SourceLine, String> {
        let line = self
            .lines
            .get(self.i)
            .cloned()
            .ok_or_else(|| "Unexpected end of file".to_string())?;
        self.i += 1;
        Ok(line)
    }
}

fn parse_import(line: &SourceLine) -> Result<Stmt, String> {
    let text = line.text.trim();

    // Strip get:/Get: or get /Get  prefix
    let rest = if let Some(r) = text.strip_prefix("get ") {
        r.trim().to_string()
    } else if let Some(r) = text.strip_prefix("Get ") {
        r.trim().to_string()
    } else if let Some(r) = text.strip_prefix("get:") {
        r.trim().to_string()
    } else if let Some(r) = text.strip_prefix("Get:") {
        r.trim().to_string()
    } else {
        return Err(format!("Invalid import syntax at line {}", line.line_no));
    };

    // Indent-2: get function from moduleName as alias
    // Also support old: function From: moduleName As: alias
    if let Some((symbol, after_from)) = rest.split_once(" from ")
        .or_else(|| rest.split_once(" From "))
        .or_else(|| rest.split_once(" From:"))
    {
        let symbol_name = symbol.trim().to_string();
        let (module_name, alias) = if let Some((m, a)) = after_from.split_once(" as ")
            .or_else(|| after_from.split_once(" As "))
            .or_else(|| after_from.split_once(" As:"))
        {
            (m.trim().to_string(), Some(a.trim().to_string()))
        } else {
            (after_from.trim().to_string(), None)
        };
        return Ok(Stmt::Import {
            line: line.line_no,
            module_name,
            symbol_name: Some(symbol_name),
            alias,
        });
    }

    // Indent-2: get moduleName as alias
    if let Some((module_name, alias)) = rest.split_once(" as ")
        .or_else(|| rest.split_once(" As "))
        .or_else(|| rest.split_once(" As:"))
    {
        return Ok(Stmt::Import {
            line: line.line_no,
            module_name: module_name.trim().to_string(),
            symbol_name: None,
            alias: Some(alias.trim().to_string()),
        });
    }

    Ok(Stmt::Import {
        line: line.line_no,
        module_name: rest.to_string(),
        symbol_name: None,
        alias: None,
    })
}

fn parse_catch_binding(header: &str) -> Result<Option<String>, String> {
    let raw = if let Some(r) = header.strip_prefix("catch") {
        r
    } else if let Some(r) = header.strip_prefix("Catch") {
        r
    } else {
        return Err("Catch header must start with 'catch' or 'Catch'".to_string());
    };
    let inner = raw.trim().trim_end_matches(':').trim();
    if inner.is_empty() {
        return Ok(None);
    }

    let name = if let Some(rest) = inner.strip_prefix("as ") {
        rest.trim()
    } else {
        inner
    };

    if !is_identifier(name) {
        return Err("Catch binding must be a valid identifier".to_string());
    }
    Ok(Some(name.to_string()))
}

fn looks_like_callee(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    text.split('.').all(is_identifier)
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "say" | "Say"
            | "give" | "Give"
            | "return" | "Return"
            | "var" | "Var"
            | "fun" | "Fun"
            | "if" | "If"
            | "or" | "Or"
            | "otherwise" | "Otherwise"
            | "repeat" | "Repeat" | "for" | "For"
            | "stop" | "STOP" | "break"
            | "next" | "NEXT" | "continue"
            | "get" | "Get" | "import" | "Import"
            | "reset" | "RESET" | "restart"
            | "match" | "Match"
            | "do" | "Do"
            | "catch" | "Catch"
            | "lastly" | "Lastly"
            | "makeType" | "maketype"
            | "set"
            | "flag" | "Flag"
            | "yield" | "Yield"
            | "open" | "Open"
            | "is"
            | "ask"
            | "and" | "not" | "in"
    )
}

fn parse_inline_args(args_text: &str) -> Vec<ArgItem> {
    let mut args = Vec::new();
    let mut remaining = args_text.trim();
    while !remaining.is_empty() {
        // Try to find the next argument boundary
        if remaining.starts_with('"') {
            // String literal argument
            if let Some(end) = find_string_end(remaining, '"') {
                let arg = remaining[..=end].to_string();
                args.push(ArgItem::Positional(arg));
                remaining = remaining[end + 1..].trim();
                continue;
            }
        }
        if remaining.starts_with('\'') {
            if let Some(end) = find_string_end(remaining, '\'') {
                let arg = remaining[..=end].to_string();
                args.push(ArgItem::Positional(arg));
                remaining = remaining[end + 1..].trim();
                continue;
            }
        }
        if remaining.starts_with('[') {
            // List/dict literal - find matching bracket
            if let Some(end) = find_matching_bracket(remaining, '[', ']') {
                let arg = remaining[..=end].to_string();
                args.push(ArgItem::Positional(arg));
                remaining = remaining[end + 1..].trim();
                continue;
            }
        }
        if remaining.starts_with('{') {
            if let Some(end) = find_matching_bracket(remaining, '{', '}') {
                let arg = remaining[..=end].to_string();
                args.push(ArgItem::Positional(arg));
                remaining = remaining[end + 1..].trim();
                continue;
            }
        }
        if remaining.starts_with('(') {
            if let Some(end) = find_matching_bracket(remaining, '(', ')') {
                let arg = remaining[..=end].to_string();
                args.push(ArgItem::Positional(arg));
                remaining = remaining[end + 1..].trim();
                continue;
            }
        }
        // Check for named argument: name is value
        if let Some((name, rest)) = remaining.split_once(" is ") {
            if is_identifier(name) && !name.is_empty() {
                // The value might be a simple word or complex expression
                let value_end = find_arg_end(rest);
                let value = rest[..value_end].trim().to_string();
                args.push(ArgItem::Named { name: name.to_string(), expr: value });
                remaining = rest[value_end..].trim();
                continue;
            }
        }
        // Simple word argument
        let end = find_word_end(remaining);
        let arg = remaining[..end].to_string();
        args.push(ArgItem::Positional(arg));
        remaining = remaining[end..].trim();
    }
    args
}

fn find_string_end(s: &str, quote: char) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 1; // skip opening quote
    while i < bytes.len() {
        if bytes[i] as char == '\\' {
            i += 2;
            continue;
        }
        if bytes[i] as char == quote {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_matching_bracket(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 1;
    let bytes = s.as_bytes();
    let mut i = 1; // skip opening bracket
    let mut in_str = false;
    let mut str_char = '\0';
    let mut escape = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if escape {
            escape = false;
            i += 1;
            continue;
        }
        if c == '\\' {
            escape = true;
            i += 1;
            continue;
        }
        if in_str {
            if c == str_char {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' {
            in_str = true;
            str_char = c;
            i += 1;
            continue;
        }
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn find_arg_end(s: &str) -> usize {
    // Find end of a simple argument (next space or end)
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] as char {
            ' ' | '\t' | '\n' => return i,
            '"' | '\'' => {
                // Skip over string
                let quote = bytes[i] as char;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] as char == '\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] as char == quote {
                        break;
                    }
                    i += 1;
                }
            }
            '[' => {
                if let Some(end) = find_matching_bracket(&s[i..], '[', ']') {
                    return i + end + 1;
                }
            }
            '{' => {
                if let Some(end) = find_matching_bracket(&s[i..], '{', '}') {
                    return i + end + 1;
                }
            }
            '(' => {
                if let Some(end) = find_matching_bracket(&s[i..], '(', ')') {
                    return i + end + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    s.len()
}

fn find_word_end(s: &str) -> usize {
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if (b as char) == ' ' || (b as char) == '\t' || (b as char) == '\n' {
            return i;
        }
    }
    s.len()
}

fn is_literal(s: &str) -> bool {
    // Check if string is a literal value (number, string, bool, list, dict)
    if s.is_empty() { return true; }
    if s == "empty" || s == "null" || s == "TRUE" || s == "FALSE" || s == "YES" || s == "NO" || s == "true" || s == "false" { return true; }
    if s.starts_with('"') || s.starts_with('\'') { return true; }
    if s.starts_with('[') || s.starts_with('{') { return true; }
    // Check if it's a number
    if s.parse::<f64>().is_ok() { return true; }
    false
}

fn contains_expr_operators(s: &str) -> bool {
    // Check if the text contains TOP-LEVEL expression operators (outside
    // strings AND outside brackets/parens) that indicate it's an expression,
    // not a function call. Operators nested inside ( ) [ ] { } are part of a
    // single parenthesized argument (e.g. foo "a" ("b" + x)) and do NOT make
    // the whole line an expression.
    let bytes = s.as_bytes();
    let mut in_str = false;
    let mut str_char = '\0';
    let mut escape = false;
    let mut depth = 0i32;
    for &b in bytes {
        let c = b as char;
        if escape { escape = false; continue; }
        if c == '\\' { escape = true; continue; }
        if in_str {
            if c == str_char { in_str = false; }
            continue;
        }
        if c == '"' || c == '\'' { in_str = true; str_char = c; continue; }
        if c == '(' || c == '[' || c == '{' { depth += 1; continue; }
        if c == ')' || c == ']' || c == '}' { depth -= 1; continue; }
        if depth > 0 { continue; }
        // Expression operators outside strings and brackets
        if matches!(c, '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '!') {
            return true;
        }
    }
    false
}

fn parse_assignment(text: &str) -> Option<(String, String)> {
    let (left, right) = text.split_once(" is ")?;
    let name = left.trim();
    if !looks_like_callee(name) {
        return None;
    }
    Some((name.to_string(), right.trim().to_string()))
}

fn parse_compound_assignment(text: &str) -> Option<(String, String, String)> {
    // Try each compound operator
    for op in &["+=", "-=", "*=", "/=", "%="] {
        if let Some((left, right)) = text.split_once(*op) {
            let name = left.trim().to_string();
            if looks_like_callee(&name) && !name.is_empty() && !right.trim().is_empty() {
                return Some((name, op.to_string(), right.trim().to_string()));
            }
        }
    }
    None
}

/// Infer a type string from a value expression at var declaration time
fn infer_type_from_expr(expr: &str) -> &str {
    let s = expr.trim();
    // Literal inference
    if s == "empty" || s == "null" { return "empty"; }
    if s == "TRUE" || s == "FALSE" || s == "YES" || s == "NO" || s == "true" || s == "false" { return "boolean"; }
    if s.starts_with('"') || s.starts_with('\'') { return "string"; }
    if s.starts_with('#') { return "color"; }
    if s.starts_with('[') { return "list"; }
    if s.starts_with('{') { return "dict"; }
    // Number inference
    if let Ok(_) = s.parse::<i64>() { return "int"; }
    if let Ok(_) = s.parse::<f64>() { return "float"; }
    // Function call inference heuristics
    if s.starts_with("ask(") { return "string"; }
    if s.starts_with("int(") { return "int"; }
    if s.starts_with("float(") { return "float"; }
    if s.starts_with("bool(") { return "boolean"; }
    if s.starts_with("string(") || s.starts_with("str(") { return "string"; }
    if s.starts_with("range(") { return "list"; }
    if s.starts_with("time_now") || s.starts_with("time_utc") { return "float"; }
    if s.starts_with("uuid") { return "string"; }
    // Default
    "dynamic"
}

fn parse_subscript_assignment(text: &str) -> Result<Option<(SubscriptAssignTarget, String)>, String> {
    let Some((left_raw, right_raw)) = text.split_once(" is ") else {
        return Ok(None);
    };

    let left = left_raw.trim();
    let right = right_raw.trim().to_string();

    if !left.ends_with(']') {
        return Ok(None);
    }

    let Some(open) = left.find('[') else {
        return Ok(None);
    };

    let name = left[..open].trim();
    if !is_identifier(name) {
        return Ok(None);
    }

    let inner = left[open + 1..left.len() - 1].trim();
    if inner.is_empty() {
        return Err("Indexed assignment requires an index expression".to_string());
    }

    let parts = split_top_level(inner, ':');
    if parts.len() == 1 {
        let index_expr = parts[0].trim();
        if index_expr.is_empty() {
            return Err("Indexed assignment requires an index expression".to_string());
        }
        return Ok(Some((
            SubscriptAssignTarget::Index {
                name: name.to_string(),
                index_expr: index_expr.to_string(),
            },
            right,
        )));
    }

    if parts.len() > 3 {
        return Err("Slice assignment supports start:end or start:end:step forms".to_string());
    }

    let to_opt = |part: &str| {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    let start_expr = to_opt(&parts[0]);
    let end_expr = to_opt(&parts[1]);
    let step_expr = if parts.len() == 3 {
        to_opt(&parts[2])
    } else {
        None
    };

    Ok(Some((
        SubscriptAssignTarget::Slice {
            name: name.to_string(),
            start_expr,
            end_expr,
            step_expr,
        },
        right,
    )))
}

fn split_top_level(text: &str, delimiter: char) -> Vec<String> {
    let mut out = vec![];
    let mut current = String::new();
    let mut in_str = false;
    let mut quote = '\0';
    let mut escape = false;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;

    for ch in text.chars() {
        if in_str {
            current.push(ch);
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == quote {
                in_str = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                in_str = true;
                quote = ch;
                current.push(ch);
            }
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                current.push(ch);
            }
            '[' => {
                bracket_depth += 1;
                current.push(ch);
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                current.push(ch);
            }
            '{' => {
                brace_depth += 1;
                current.push(ch);
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                current.push(ch);
            }
            _
                if ch == delimiter
                    && paren_depth == 0
                    && bracket_depth == 0
                    && brace_depth == 0 =>
            {
                out.push(current);
                current = String::new();
            }
            _ => current.push(ch),
        }
    }

    out.push(current);
    out
}

fn parse_function_signature(raw: &str) -> Result<(String, Vec<FunctionParam>), String> {
    let trimmed = raw
        .split_once("->")
        .map(|(left, _)| left.trim())
        .or_else(|| raw.split_once(" as ").map(|(left, _)| left.trim()))
        .unwrap_or_else(|| raw.trim());

    if trimmed.is_empty() {
        return Err("Function name missing".to_string());
    }

    let Some(paren_start) = trimmed.find('(') else {
        if !is_identifier(trimmed) {
            return Err(format!("Invalid function name '{}'", trimmed));
        }
        return Ok((trimmed.to_string(), vec![]));
    };

    if !trimmed.ends_with(')') {
        return Err("Function signature must end with ')'".to_string());
    }

    let name = trimmed[..paren_start].trim();
    if !is_identifier(name) {
        return Err(format!("Invalid function name '{}'", name));
    }

    let inner = &trimmed[paren_start + 1..trimmed.len() - 1];
    let inner = inner.trim();
    if inner.is_empty() {
        return Ok((name.to_string(), vec![]));
    }

    let mut params: Vec<FunctionParam> = Vec::new();
    for part in inner.split(',') {
        let p = part.trim();
        if p.is_empty() {
            return Err("Empty parameter in signature".to_string());
        }

        let (name, ty) = if let Some((n, t)) = p.split_once(':') {
            let n = n.trim();
            let t = t.trim();
            if !is_identifier(n) {
                return Err(format!("Invalid parameter '{}'", n));
            }
            if t.is_empty() {
                return Err(format!("Missing type for parameter '{}'", n));
            }
            (n.to_string(), Some(t.to_string()))
        } else {
            if !is_identifier(p) {
                return Err(format!("Invalid parameter '{}'", p));
            }
            (p.to_string(), None)
        };

        if params.iter().any(|x| x.name == name) {
            return Err(format!("Duplicate parameter '{}'", name));
        }
        let is_varargs = p.starts_with("...");
        let pname = if is_varargs { name.trim_start_matches("...").to_string() } else { name.clone() };
        params.push(FunctionParam { name: pname, ty, default_value: None, is_varargs });
    }

    Ok((name.to_string(), params))
}

fn parse_return_type(raw: &str) -> Option<String> {
    // Support both -> type and as type
    if let Some((_, right)) = raw.split_once("->") {
        let ty = right.trim();
        if ty.is_empty() { None } else { Some(ty.to_string()) }
    } else if let Some((_, right)) = raw.split_once(" as ") {
        let ty = right.trim();
        if ty.is_empty() { None } else { Some(ty.to_string()) }
    } else {
        None
    }
}

fn self_update() {
    let repo_url = "https://github.com/xytrolabs/indent.git";
    let tmp = std::env::temp_dir().join("indent-update");
    
    println!("⚡ Indent updater — fetching latest from GitHub...");
    
    // Clone or pull the repo
    let git_result = if tmp.join(".git").exists() {
        Command::new("git").args(&["-C", tmp.to_str().unwrap_or("/tmp/indent-update"), "pull", "--ff-only"])
            .output()
    } else {
        let _ = std::fs::remove_dir_all(&tmp);
        Command::new("git").args(&["clone", "--depth", "1", repo_url, tmp.to_str().unwrap_or("/tmp/indent-update")])
            .output()
    };
    
    match git_result {
        Ok(out) if out.status.success() => {},
        Ok(out) => {
            eprintln!("Indent: git failed — {}\n{}", 
                String::from_utf8_lossy(&out.stderr),
                "Make sure git is installed and you have internet access.");
            std::process::exit(1);
        },
        Err(e) => {
            eprintln!("Indent: git not found — install git to use auto-update.\n  {}", e);
            std::process::exit(1);
        }
    }
    
    println!("  ✓ Repository up to date");
    println!("  Building with cargo...");
    
    let build = Command::new("cargo")
        .args(&["build", "--release"])
        .current_dir(tmp.join("indent-native"))
        .output();
    
    match build {
        Ok(out) if out.status.success() => {},
        Ok(out) => {
            eprintln!("Indent: cargo build failed — {}\n{}",
                String::from_utf8_lossy(&out.stderr),
                "Make sure Rust is installed: https://rustup.rs");
            std::process::exit(1);
        },
        Err(e) => {
            eprintln!("Indent: cargo not found — install Rust: https://rustup.rs\n  {}", e);
            std::process::exit(1);
        }
    }
    
    println!("  ✓ Build succeeded");
    
    // Copy the binary over the current one
    let current = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("indent"));
    let new_bin = tmp.join("indent-native/target/release/indent");

    if new_bin.exists() {
        let backup = format!("{}.bak", current.display());
        std::fs::copy(&current, &backup).ok();

        // Overwriting a running executable in place fails on Linux with
        // "Text file busy". Stage the new binary in the same directory and
        // rename it over the current one — rename works while it is running.
        let current_dir = current.parent().unwrap_or(Path::new("."));
        let staged = current_dir.join(format!(".indent.new.{}", std::process::id()));
        if let Err(e) = std::fs::copy(&new_bin, &staged) {
            eprintln!("Indent: cannot stage new binary — try running with sudo.\n  {}", e);
            std::process::exit(1);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
        }

        match std::fs::rename(&staged, &current) {
            Ok(_) => {
                println!("  ✓ Updated to the latest Indent!");
                println!("  (Backup saved to {})", backup);
            },
            Err(e) => {
                let _ = std::fs::remove_file(&staged);
                eprintln!("Indent: cannot replace binary — try running with sudo.\n  {}", e);
                eprintln!("  New binary at: {}", new_bin.display());
                std::process::exit(1);
            }
        }
    } else {
        eprintln!("Indent: built binary not found at {}", new_bin.display());
        std::process::exit(1);
    }
}

fn usage() {
    eprintln!("Usage:");
    eprintln!("  indent [--debug] [--break N[,M...]] <file.ind>");
    eprintln!("  indent run [--debug] [--break N[,M...]] <file.ind>");
    eprintln!("  indent repl");
    eprintln!("  indent check <file-or-dir>");
    eprintln!("  indent test [path]");
    eprintln!("  indent lint <file.ind>");
    eprintln!("  indent fmt [--check] <file.ind>");
    eprintln!("  indent new <project-name-or-path>");
    eprintln!("  indent --update            Update to latest version");
    eprintln!("  indent --version");
}

fn check_single_file(file: &Path) -> Result<(), String> {
    let source = fs::read_to_string(file)
        .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;
    let lines = preprocess(&source)
        .map_err(|e| format_error_with_source(&source, &e))?;
    let mut parser = Parser::new(lines);
    parser
        .parse()
        .map_err(|e| format_error_with_source(&source, &e))?;
    Ok(())
}

fn run_check(path: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    collect_ath_files(path, &mut files)?;
    files.sort();

    if files.is_empty() {
        return Err(format!("No .ind files found under {}", path.display()));
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut failures = Vec::new();

    for file in &files {
        match check_single_file(file) {
            Ok(()) => {
                passed += 1;
                println!("CHECK OK {}", file.display());
            }
            Err(e) => {
                failed += 1;
                println!("CHECK FAIL {}", file.display());
                failures.push(format!("{}\n{}", file.display(), e));
            }
        }
    }

    println!("\nCheck summary: {} passed, {} failed", passed, failed);
    if failed > 0 {
        println!("\nFailures:");
        for f in failures {
            println!("\n---\n{f}");
        }
        return Err(format!("{} file(s) failed checks", failed));
    }

    Ok(())
}

fn run_new_project(target: &Path) -> Result<(), String> {
    if target.exists() {
        return Err(format!(
            "Target already exists: {}",
            target.display()
        ));
    }

    fs::create_dir_all(target)
        .map_err(|e| format!("Failed to create {}: {e}", target.display()))?;
    fs::create_dir_all(target.join("tests"))
        .map_err(|e| format!("Failed to create tests dir: {e}"))?;
    fs::create_dir_all(target.join(".vscode"))
        .map_err(|e| format!("Failed to create .vscode dir: {e}"))?;

    let project_name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("indent-project");

    let main_src = format!(
        "#! {project_name} entry script\n\n"
    ) + "var greeting string = \"Hello from Indent-2\"\nsay greeting\n\nfun add a b\n    give a + b\n\nvar result int = add 2 3\nsay \"2 + 3 = \" + result\n";

    let test_src = "assert_eq(2 + 2, 4, \"math should work\")\n";

    let readme = format!(
        "# {project_name}\n\n"
    ) + "## Run\n\n```bash\nindent main.ind\n```\n\n## Test\n\n```bash\nindent test tests\n```\n";

        let tasks_json = r#"{
    "version": "2.0.0",
    "tasks": [
        {
            "label": "Run Current Indent File",
            "type": "shell",
            "command": "indent",
            "args": ["${file}"],
            "group": "build",
            "presentation": { "reveal": "always", "panel": "shared", "clear": false },
            "problemMatcher": []
        },
        {
            "label": "Debug Current Indent File",
            "type": "shell",
            "command": "indent",
            "args": ["--debug", "${file}"],
            "presentation": { "reveal": "always", "panel": "shared", "clear": false },
            "problemMatcher": []
        }
    ]
}
"#;

        let launch_json = r#"{
    "version": "0.2.0",
    "configurations": [
        {
            "name": "Run Current Indent File",
            "type": "node-terminal",
            "request": "launch",
            "command": "indent ${file}",
            "cwd": "${workspaceFolder}"
        },
        {
            "name": "Debug Current Indent File",
            "type": "node-terminal",
            "request": "launch",
            "command": "indent --debug ${file}",
            "cwd": "${workspaceFolder}"
        }
    ]
}
"#;

    fs::write(target.join("main.ind"), main_src)
        .map_err(|e| format!("Failed to write main.ind: {e}"))?;
    fs::write(target.join("tests").join("smoke.ind"), test_src)
        .map_err(|e| format!("Failed to write tests/smoke.ind: {e}"))?;
    fs::write(target.join("README.md"), readme)
        .map_err(|e| format!("Failed to write README.md: {e}"))?;
    fs::write(target.join(".vscode").join("tasks.json"), tasks_json)
        .map_err(|e| format!("Failed to write .vscode/tasks.json: {e}"))?;
    fs::write(target.join(".vscode").join("launch.json"), launch_json)
        .map_err(|e| format!("Failed to write .vscode/launch.json: {e}"))?;

    println!("Created Indent project at {}", target.display());
    println!("Next steps:");
    println!("  cd {}", target.display());
    println!("  indent main.ind");
    println!("  indent test tests");
    println!("  F5 in VS Code for run/debug");

    Ok(())
}

fn collect_ath_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let is_lang_file = |p: &Path| {
        matches!(p.extension().and_then(|e| e.to_str()), Some("ath") | Some("ind"))
    };
    if path.is_file() {
        if is_lang_file(path) {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }

    let entries = fs::read_dir(path)
        .map_err(|e| format!("Failed to read directory {}: {e}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read dir entry: {e}"))?;
        let p = entry.path();
        if p.is_dir() {
            collect_ath_files(&p, out)?;
        } else if is_lang_file(&p) {
            out.push(p);
        }
    }
    Ok(())
}

fn run_test_suite(path: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    collect_ath_files(path, &mut files)?;
    files.sort();

    if files.is_empty() {
        println!("No .ind test files found under {}", path.display());
        return Ok(());
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut failures = Vec::new();

    for file in &files {
        let mut runtime = Runtime::new(
            file.parent()
                .unwrap_or(Path::new("."))
                .to_path_buf(),
        );
        match runtime.run_file(file) {
            Ok(()) => {
                passed += 1;
                println!("PASS {}", file.display());
            }
            Err(e) => {
                failed += 1;
                println!("FAIL {}", file.display());
                failures.push(format!("{}\n{}", file.display(), e));
            }
        }
    }

    println!("\nTest summary: {} passed, {} failed", passed, failed);
    if failed > 0 {
        println!("\nFailures:");
        for f in failures {
            println!("\n---\n{f}");
        }
        return Err(format!("{} test file(s) failed", failed));
    }
    Ok(())
}

fn run_repl() -> Result<(), String> {
    println!("Indent REPL");
    println!("Type :help for commands, :quit to exit.");

    let mut runtime = Runtime::new(
        env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    );

    loop {
        print!("indent> ");
        io::stdout()
            .flush()
            .map_err(|e| format!("REPL I/O error: {e}"))?;

        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .map_err(|e| format!("REPL I/O error: {e}"))?;

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == ":quit" || trimmed == ":q" {
            println!("bye");
            break;
        }
        if trimmed == ":help" {
            println!(":help               show help");
            println!(":quit               exit REPL");
            println!(":vars               list global variables");
            println!("Any other input is parsed and executed as Indent code.");
            continue;
        }
        if trimmed == ":vars" {
            let mut names = runtime.vars.keys().cloned().collect::<Vec<_>>();
            names.sort();
            for name in names {
                if let Some(v) = runtime.vars.get(&name) {
                    println!("{name} = {v}");
                }
            }
            continue;
        }

        let mut chunk = line.clone();
        if trimmed.ends_with(':') {
            println!("... enter indented block lines, finish with empty line");
            loop {
                print!("... ");
                io::stdout()
                    .flush()
                    .map_err(|e| format!("REPL I/O error: {e}"))?;
                let mut next = String::new();
                io::stdin()
                    .read_line(&mut next)
                    .map_err(|e| format!("REPL I/O error: {e}"))?;
                if next.trim().is_empty() {
                    break;
                }
                chunk.push_str(&next);
            }
        }

        if let Err(err) = runtime.run_source(&chunk) {
            println!("Indent error: {err}");
        }
    }

    Ok(())
}

fn run_lint(file: &Path) -> Result<(), String> {
    let source = fs::read_to_string(file)
        .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;

    let mut issues: Vec<String> = Vec::new();
    for (idx, raw) in source.lines().enumerate() {
        let line_no = idx + 1;
        if raw.contains('\t') {
            issues.push(format!("Line {line_no}: tab character found; use spaces"));
        }
        if raw.ends_with(' ') {
            issues.push(format!("Line {line_no}: trailing whitespace"));
        }
    }

    match preprocess(&source).and_then(|lines| Parser::new(lines).parse().map(|_| ())) {
        Ok(()) => {}
        Err(e) => issues.push(format!("Syntax: {e}")),
    }

    if issues.is_empty() {
        println!("Lint OK: {}", file.display());
        return Ok(());
    }

    println!("Lint issues in {}:", file.display());
    for issue in &issues {
        println!("- {issue}");
    }
    Err(format!("{} issue(s) found", issues.len()))
}

fn run_fmt(file: &Path, check: bool) -> Result<(), String> {
    let source = fs::read_to_string(file)
        .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;

    let mut out = String::new();
    for line in source.lines() {
        // Convert tabs to 4 spaces
        let normalized = line.replace('\t', "    ");
        // Normalize multiple spaces (but preserve string contents)
        let cleaned = normalize_spaces(&normalized);
        out.push_str(&cleaned);
        out.push('\n');
    }

    if check {
        if out == source {
            println!("Format OK: {}", file.display());
            return Ok(());
        }
        return Err(format!("Formatting differs: {}", file.display()));
    }

    if out != source {
        fs::write(file, out)
            .map_err(|e| format!("Failed to write {}: {e}", file.display()))?;
        println!("Formatted: {}", file.display());
    } else {
        println!("Already formatted: {}", file.display());
    }
    Ok(())
}

fn normalize_spaces(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    let mut str_char = '\0';

    // Preserve leading indentation
    while i < bytes.len() && (bytes[i] as char == ' ' || bytes[i] as char == '\t') {
        out.push(if bytes[i] as char == '\t' { ' ' } else { bytes[i] as char });
        i += 1;
    }

    while i < bytes.len() {
        let c = bytes[i] as char;

        // Preserve comments as-is
        if !in_str && c == '#' && i + 1 < bytes.len() && bytes[i + 1] as char == '!' {
            out.push_str(&line[i..]);
            break;
        }

        // Preserve strings as-is
        if !in_str && (c == '"' || c == '\'') {
            in_str = true;
            str_char = c;
            out.push(c);
            i += 1;
            continue;
        }
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < bytes.len() {
                i += 1;
                out.push(bytes[i] as char);
            } else if c == str_char {
                in_str = false;
            }
            i += 1;
            continue;
        }

        // Outside strings: collapse multiple spaces to single space
        if c == ' ' || c == '\t' {
            // Skip if previous char is already a space
            if out.ends_with(' ') {
                i += 1;
                continue;
            }
            // Peek ahead
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j] as char == ' ' || bytes[j] as char == '\t') {
                j += 1;
            }
            // Only add space if non-space content follows on this line
            if j < bytes.len() && bytes[j] as char != '\n' {
                out.push(' ');
            }
            i = j;
            continue;
        }

        out.push(c);
        i += 1;
    }

    // Trim trailing spaces
    while out.ends_with(' ') {
        out.pop();
    }

    out
}

fn touch_lines(stmt: &Stmt) {
    match stmt {
        Stmt::Say { line, .. }
        | Stmt::DefVar { line, .. }
        | Stmt::MakeType { line, .. }
        | Stmt::DefClass { line, .. }
        | Stmt::Assign { line, .. }
        | Stmt::AssignOp { line, .. }
        | Stmt::AssignIndex { line, .. }
        | Stmt::AssignSlice { line, .. }
        | Stmt::DefFun { line, .. }
        | Stmt::Give { line, .. }
        | Stmt::IfChain { line, .. }
        | Stmt::Match { line, .. }
        | Stmt::DoChain { line, .. }
        | Stmt::Repeat { line, .. }
        | Stmt::Stop { line }
        | Stmt::Next { line }
        | Stmt::Reset { line }
        | Stmt::Import { line, .. }
        | Stmt::Call { line, .. }
        | Stmt::BareExpr { line, .. }
        | Stmt::Flag { line, .. }
        | Stmt::Yield { line, .. }
        | Stmt::Decorator { line, .. }
        | Stmt::Open { line, .. }
        | Stmt::Loop { line, .. }
        | Stmt::Await { line, .. }
        | Stmt::Wait { line, .. }
        | Stmt::AsyncWith { line, .. } => {
            let _ = *line;
        }
    }
}

fn parse_project_env_line(line: &str) -> Option<(String, String)> {
    let mut raw = line.trim();
    if raw.is_empty() || raw.starts_with('#') {
        return None;
    }

    if let Some(rest) = raw.strip_prefix("export") {
        raw = rest.trim_start();
    }

    let (key_raw, value_raw) = raw.split_once('=')?;
    let key = key_raw.trim();
    if key.is_empty() {
        return None;
    }

    let mut value = value_raw.trim().to_string();
    if value.len() >= 2 {
        let first = value.as_bytes()[0] as char;
        let last = value.as_bytes()[value.len() - 1] as char;
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            value = value[1..value.len() - 1].to_string();
        }
    }

    Some((key.to_string(), value))
}

fn load_env_file_if_present(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    for line in content.lines() {
        if let Some((key, value)) = parse_project_env_line(line) {
            if env::var_os(&key).is_none() {
                // SAFETY: Runtime process environment mutation is intentional for script startup.
                unsafe {
                    env::set_var(key, value);
                }
            }
        }
    }

    Ok(true)
}

fn load_project_environment(base_dir: &Path) {
    let glo_path = base_dir.join(".glo");
    let env_path = base_dir.join(".env");

    match load_env_file_if_present(&glo_path) {
        Ok(true) => return,
        Ok(false) => {}
        Err(err) => {
            eprintln!("Indent warning: {err}");
            return;
        }
    }

    if let Err(err) = load_env_file_if_present(&env_path) {
        eprintln!("Indent warning: {err}");
    }
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    if args.len() < 2 {
        usage();
        std::process::exit(2);
    }

    if args[1] == "--version" || args[1] == "-V" || args[1] == "version" {
        println!("indent {}", INDENT_VERSION);
        return;
    }

    if args[1] == "--update" || args[1] == "update" {
        self_update();
        return;
    }

    if args[1] == "new" {
        if args.len() != 3 {
            usage();
            std::process::exit(2);
        }
        let target = PathBuf::from(&args[2]);
        let abs = if target.is_absolute() {
            target
        } else {
            env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(target)
        };
        if let Err(err) = run_new_project(&abs) {
            eprintln!("Indent new error: {err}");
            std::process::exit(1);
        }
        return;
    }

    if args[1] == "lint" {
        if args.len() != 3 {
            usage();
            std::process::exit(2);
        }
        let file = PathBuf::from(&args[2]);
        let abs = if file.is_absolute() {
            file
        } else {
            env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(file)
        };
        if let Err(err) = run_lint(&abs) {
            eprintln!("Indent lint error: {err}");
            std::process::exit(1);
        }
        return;
    }

    if args[1] == "repl" {
        if args.len() != 2 {
            usage();
            std::process::exit(2);
        }
        if let Err(err) = run_repl() {
            eprintln!("Indent REPL error: {err}");
            std::process::exit(1);
        }
        return;
    }

    if args[1] == "check" {
        let target = if args.len() == 2 {
            PathBuf::from(".")
        } else if args.len() == 3 {
            PathBuf::from(&args[2])
        } else {
            usage();
            std::process::exit(2);
        };
        let abs = if target.is_absolute() {
            target
        } else {
            env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(target)
        };
        if let Err(err) = run_check(&abs) {
            eprintln!("Indent check error: {err}");
            std::process::exit(1);
        }
        return;
    }

    if args[1] == "test" {
        let target = if args.len() == 2 {
            PathBuf::from("tests")
        } else if args.len() == 3 {
            PathBuf::from(&args[2])
        } else {
            usage();
            std::process::exit(2);
        };
        let abs = if target.is_absolute() {
            target
        } else {
            env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(target)
        };
        if let Err(err) = run_test_suite(&abs) {
            eprintln!("Indent test error: {err}");
            std::process::exit(1);
        }
        return;
    }

    if args[1] == "fmt" {
        if args.len() < 3 || args.len() > 4 {
            usage();
            std::process::exit(2);
        }
        let mut check = false;
        let file_arg = if args[2] == "--check" {
            check = true;
            if args.len() != 4 {
                usage();
                std::process::exit(2);
            }
            args[3].clone()
        } else {
            args[2].clone()
        };
        let file = PathBuf::from(file_arg);
        let abs = if file.is_absolute() {
            file
        } else {
            env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(file)
        };
        if let Err(err) = run_fmt(&abs, check) {
            eprintln!("Indent format error: {err}");
            std::process::exit(1);
        }
        return;
    }

    let mut debug = false;
    let mut breakpoints = HashSet::new();
    let mut file_arg: Option<String> = None;

    let mut i = if args[1] == "run" { 2 } else { 1 };
    if args[1] == "run" && args.len() < 3 {
        usage();
        std::process::exit(2);
    }
    while i < args.len() {
        let arg = &args[i];
        if arg == "--debug" {
            debug = true;
            i += 1;
            continue;
        }

        if arg == "--break" {
            if i + 1 >= args.len() {
                eprintln!("Missing value for --break");
                std::process::exit(2);
            }
            for chunk in args[i + 1].split(',') {
                let n = chunk.trim().parse::<usize>().unwrap_or(0);
                if n == 0 {
                    eprintln!("Invalid breakpoint line: {}", chunk.trim());
                    std::process::exit(2);
                }
                breakpoints.insert(n);
            }
            i += 2;
            continue;
        }

        if let Some(rest) = arg.strip_prefix("--break=") {
            for chunk in rest.split(',') {
                let n = chunk.trim().parse::<usize>().unwrap_or(0);
                if n == 0 {
                    eprintln!("Invalid breakpoint line: {}", chunk.trim());
                    std::process::exit(2);
                }
                breakpoints.insert(n);
            }
            i += 1;
            continue;
        }

        if arg == "--help" || arg == "-h" {
            usage();
            return;
        }

        if arg.starts_with('-') {
            eprintln!("Unknown option: {arg}");
            usage();
            std::process::exit(2);
        }

        if file_arg.is_some() {
            eprintln!("Only one input file is supported");
            usage();
            std::process::exit(2);
        }
        file_arg = Some(arg.clone());
        i += 1;
    }

    let Some(file_arg) = file_arg else {
        usage();
        std::process::exit(2);
    };

    let path = PathBuf::from(&file_arg);
    let abs = if path.is_absolute() {
        path
    } else {
        env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
    };

    if let Some(base_dir) = abs.parent() {
        // Run with the script's directory as the working directory so that
        // relative paths in the script (file ops, os_list_dir, subprocesses)
        // resolve relative to the script — the same behavior as running
        // `cd <script-dir> && indent bot.ind`. This makes
        // `indent run /abs/path/bot.ind` work from any terminal directory.
        let _ = env::set_current_dir(base_dir);
        load_project_environment(base_dir);
    }

    let mut runtime = Runtime::new(
        abs.parent()
            .unwrap_or(Path::new("."))
            .to_path_buf(),
    );

    if debug {
        if let Err(err) = runtime.enable_debugger(&abs, breakpoints) {
            let src = std::fs::read_to_string(&abs).unwrap_or_default();
            eprintln!("{}", format_error_with_source(&src, &err));
            std::process::exit(1);
        }
    }

    match runtime.run_file(&abs) {
        Ok(()) => {
            for f in runtime.funcs.values() {
                for s in &f.body {
                    touch_lines(s);
                }
            }
        }
        Err(err) => {
            if debug && err == DEBUGGER_STOP_MSG {
                println!("Indent debug session ended.");
                std::process::exit(0);
            }
            let src = std::fs::read_to_string(&abs).unwrap_or_default();
            eprintln!("{}", format_error_with_source(&src, &err));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_dir(prefix: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("{}_{}", prefix, now))
    }

    #[test]
    fn executes_repeat_and_assignment() {
        let dir = unique_dir("indent_test_repeat");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        let src = r#"
var total int = 0

repeat 5
    total is total + 1

if total == 5
    say "ok"
otherwise
    say "bad"
"#;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
        assert!(matches!(rt.vars.get("total"), Some(Value::Int(5))));
    }

    #[test]
    fn make_type_converts_existing_variable() {
        let src = r#"
var UInput int = 4
var converted string = string(UInput)
"#;

        let lines = preprocess(src).expect("preprocesses");
        let program = Parser::new(lines).parse().expect("program parses");
        let mut rt = Runtime::new(PathBuf::from("."));
        let mut ctx = ExecContext::new(&mut rt);
        exec_block(&program, &mut ctx).expect("program executes");

        assert!(matches!(ctx.rt.vars.get("converted"), Some(Value::Str(s)) if s == "4"));
    }
    #[test]
    fn imports_module_from_indent_path() {
        let root = unique_dir("indent_test_import");
        let main_dir = root.join("main");
        let pkg_dir = root.join("pkg");
        fs::create_dir_all(&main_dir).expect("create main dir");
        fs::create_dir_all(&pkg_dir).expect("create pkg dir");

        fs::write(
            pkg_dir.join("mathmod.ind"),
            "fun Double\n    give argument * 2\n",
        )
        .expect("write module");
        fs::write(
            main_dir.join("main.ind"),
            "get Double from mathmod as Twice\nvar r int = Twice 7\nsay r\n",
        )
        .expect("write main");

        unsafe {
            std::env::set_var("INDENT_PATH", pkg_dir.to_string_lossy().to_string());
        }
        let mut rt = Runtime::new(main_dir.clone());
        let result = rt.run_file(&main_dir.join("main.ind"));
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
    }

    #[test]
    fn supports_dict_literals_and_indexing() {
        let dir = unique_dir("indent_test_dict");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        let src = r#"
var bag dynamic = {"name": "Indent", "v": 1}
var nums dynamic = [10, 20, 30]
var title string = bag["name"]
var second int = nums[1]
var firstChar string = title[0]
"#;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
        assert!(matches!(rt.vars.get("title"), Some(Value::Str(s)) if s == "Indent"));
        assert!(matches!(rt.vars.get("second"), Some(Value::Int(20))));
        assert!(matches!(rt.vars.get("firstChar"), Some(Value::Str(s)) if s == "I"));
    }

    #[test]
    fn supports_named_function_parameters() {
        let dir = unique_dir("indent_test_params");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        let src = r#"
fun Add a b
    say a + b

Add 3 4

Add;
    b is 10
    a is 2
"#;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
        let params = rt
            .funcs
            .get("Add")
            .map(|f| f.params.clone())
            .unwrap_or_default();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "a");
        assert_eq!(params[1].name, "b");
    }

    #[test]
    fn supports_call_expressions_and_typed_signatures() {
        let dir = unique_dir("indent_test_call_expr");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        let src = r#"
fun Add a b as int
    give a + b

var sum int = Add 5 7
"#;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
        assert!(matches!(rt.vars.get("sum"), Some(Value::Int(12))));
    }

    #[test]
    fn rejects_wrong_typed_function_arguments() {
        let dir = unique_dir("indent_test_typed_fail");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        let src = r#"
def.fun: Add(a:int, b:int) -> int
    Give: a + b

def.var: sum
    int
    Add("x", 2)
"#;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_err(), "expected typed argument failure");
    }

    #[test]
    fn imports_dotted_module_paths() {
        let root = unique_dir("indent_test_dotted_imports");
        let std_dir = root.join("std");
        fs::create_dir_all(&std_dir).expect("create std dir");
        fs::write(
            std_dir.join("math.ind"),
            "fun Add a b\n    give a + b\n",
        )
        .expect("write module");

        let main_file = root.join("main.ind");
        fs::write(
            &main_file,
            "get Add from std.math as Plus\nvar r int = Plus 1 2\nsay r\n",
        )
        .expect("write main file");

        let mut rt = Runtime::new(root.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
    }

    #[test]
    fn supports_color_type_and_built_in_palette_variables() {
        let dir = unique_dir("indent_test_color_type");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        let src = r##"
var accent color = RED
var accentHex color = "#22c55e"
var accentBare color = "#3b82f6"
    "##;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
        assert!(matches!(rt.vars.get("accent"), Some(Value::Str(s)) if s == "#ff4d4d"));
        assert!(matches!(rt.vars.get("accentHex"), Some(Value::Str(s)) if s == "#22c55e"));
        assert!(matches!(rt.vars.get("accentBare"), Some(Value::Str(s)) if s == "#3b82f6"));
    }

    #[test]
    fn supports_do_catch_otherwise_lastly_and_flag() {
        let dir = unique_dir("indent_test_do_catch");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        let src = r#"
var state string = "start"

Do:
    flag: "boom"
Catch as err:
    state is "caught: " + err
Otherwise:
    state is "otherwise"
Lastly:
    state is state + " | lastly"
"#;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
        assert!(matches!(rt.vars.get("state"), Some(Value::Str(s)) if s.contains("caught: Line") && s.contains("lastly")));
    }

    #[test]
    fn supports_do_otherwise_when_no_error() {
        let dir = unique_dir("indent_test_do_otherwise");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        let src = r#"
var status string = "init"

Do:
    status is "ok"
Otherwise:
    status is status + "|otherwise"
Lastly:
    status is status + "|lastly"
"#;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
        assert!(matches!(rt.vars.get("status"), Some(Value::Str(s)) if s == "ok|otherwise|lastly"));
    }

    #[test]
    fn supports_say_semicolon_call_style() {
        let dir = unique_dir("indent_test_say_semicolon");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        let src = r#"
fun Pair a b as string
    give a + ":" + b

var p string = Pair "left" "right"
say p
"#;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
    }

    #[test]
    fn and_or_short_circuit() {
        let dir = unique_dir("indent_test_short_circuit");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        // RHS must NOT be evaluated when LHS already decides the result.
        // Here the second operand would error (key lookup on a dict that
        // lacks the key), so if `and` short-circuits it must not crash.
        let src = r#"
var e = {"type": "quit"}
var safe = "none"
if e["type"] == "key" and e["down"] == true
    safe is "key-down"
if e["type"] == "quit" or e["missing"] == true
    safe is "quit-or"
say safe
"#;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
        assert!(matches!(rt.vars.get("safe"), Some(Value::Str(s)) if s == "quit-or"));
    }

    #[test]
    fn sort_preserves_nested_pair_values() {
        let dir = unique_dir("indent_test_sort_pairs");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        // sort must compare nested list values element-wise (Python-style)
        // and keep the pairs intact instead of flattening to strings.
        let src = r#"
var scored = [[0.3, "low"], [0.9, "high"], [0.5, "mid"]]
var ranked = sort scored
if ranked[2][0] == 0.9
    say "ok"
otherwise
    say "bad"
"#;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
    }

    #[test]
    fn split_path_list_handles_windows_and_unix_separators() {
        // On Unix we split on ':'; on Windows on ';'. The helper must drop
        // empty entries and trim whitespace around each entry.
        #[cfg(not(target_os = "windows"))]
        let got = split_path_list("a:b:: c ");
        #[cfg(target_os = "windows")]
        let got = split_path_list("a;b;; c ");
        assert_eq!(got, vec!["a", "b", "c"]);
    }

    #[test]
    fn filtered_comprehension_and_sets() {
        let dir = unique_dir("indent_test_compr_set");
        fs::create_dir_all(&dir).expect("create temp dir");
        let main_file = dir.join("main.ind");
        // Filtered list comprehension must parse (previously mis-parsed as a
        // ternary because `if` was consumed by parse_or). `group` is the
        // builtin for unique ordered collections; `type_of` reports "group".
        let src = r#"
var nums = [1, 2, 3, 4, 5, 6]
var evens = [x for x in nums if x % 2 == 0]
if len(evens) == 3 and evens[0] == 2
    say "compr-ok"
var g = group([1, 2, 2, 3])
if len(g) == 3 and type_of(g) == "group"
    say "group-ok"
var s = set([1, 1, 2])
if len(s) == 2
    say "set-alias-ok"
"#;
        fs::write(&main_file, src).expect("write main file");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&main_file);
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
    }
}
