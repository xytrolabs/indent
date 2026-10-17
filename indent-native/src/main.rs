use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::path::{Path, PathBuf};
use std::process;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tungstenite::{connect as ws_connect_blocking, Message as WsMessage};

// ── Hashing ─────────────────────────────────────────────────
//
// Every variable read, variable write, dict lookup and dict insert hashes a
// `String` key. std's default hasher is SipHash-1-3, chosen to be
// DoS-resistant: it costs roughly 20-25 ns for a short key, and the interpreter
// pays it several times per operation (a 2-argument call hashes six or more
// names). Measured against CPython, that single cost is a large part of why
// every operation sat at 3-8x — it is spread evenly across the whole language
// rather than concentrated in one slow builtin.
//
// FxHash is the hash used by rustc itself for its own interner: a rotate-xor-
// multiply over machine words, a few cycles for a short key and no quality
// claim beyond good distribution for strings. It is not collision-resistant,
// but nothing here needs that — these maps are keyed by names that come from
// the program being interpreted, not from an attacker's network input. (The
// hashes that do guard against hostile input, like `source_hash` for the AST
// cache, are separate and unchanged.)
//
// Implemented locally rather than pulling in the `rustc-hash` crate: the
// interpreter must build offline, and this is ~40 lines.
const FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default, Clone, Copy)]
struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline(always)]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(FX_SEED);
    }
}

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut b = bytes;
        while b.len() >= 8 {
            let (head, rest) = b.split_at(8);
            self.add(u64::from_le_bytes(head.try_into().unwrap()));
            b = rest;
        }
        if b.len() >= 4 {
            let (head, rest) = b.split_at(4);
            self.add(u32::from_le_bytes(head.try_into().unwrap()) as u64);
            b = rest;
        }
        for &byte in b {
            self.add(byte as u64);
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64)
    }
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(i as u64)
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64)
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i)
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64)
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

type FxBuildHasher = std::hash::BuildHasherDefault<FxHasher>;

/// Copy an ordinary (std-hashed) name→value map into a `Map`. Used where a
/// `Value::Dict` or object field set becomes a temporary runtime's variables.
/// Those paths already deep-copied the map, so this is the same cost plus a
/// cheaper hash for the lookups that follow.
#[inline]
fn to_map(src: &HashMap<String, Value>) -> Map<String, Value> {
    src.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// The same thing for a `SmallDict` source, which no longer has a `HashMap` to
/// clone from: the pairs are copied out one at a time, clones and all. The three
/// call sites are `dict with function value` / JS-style-object method dispatch,
/// so this is not on any hot path.
#[inline]
fn dict_to_map(src: &SmallDict) -> Map<String, Value> {
    let mut out = Map::default();
    for (k, v) in src {
        out.insert(k.clone(), v.clone());
    }
    out
}

/// A dictionary that does not pay for a hash table until it has enough keys.
///
/// A `HashMap` is 48 bytes when empty and allocates on its first insert, and each
/// entry costs a 24-byte `String`, a 56-byte `Value` and a control byte. A
/// three-key dict like `{"type": "quit", "down": true}` is therefore a hash table,
/// three `String`s and three entries — around 400 bytes and four allocations for
/// what is really a tagged record.
///
/// Most dicts in real scripts are small: config objects, event payloads, one JSON
/// row each. Up to `SMALL_DICT_MAX` keys live in one `Vec` of pairs and are found
/// by scanning, which for a handful of short keys is cheaper than hashing them —
/// no hash, no control bytes, one or two cache lines. Past the threshold the dict
/// promotes to the hash map, so a dict that really is a dictionary still behaves
/// like one.
///
/// Both representations are correct at any size, and the API mirrors `HashMap`
/// so the interpreter code reads the same either way.
#[derive(Debug, Clone, Default)]
enum SmallDict {
    #[default]
    Empty,
    Small(Vec<(String, Value)>),
    Large(HashMap<String, Value>),
}

/// How many keys a `SmallDict` keeps in a `Vec` before promoting to a hash map.
/// This is where the scan stops paying for itself, not a correctness boundary.
const SMALL_DICT_MAX: usize = 8;

impl SmallDict {
    fn new() -> Self {
        SmallDict::Empty
    }

    fn with_capacity(n: usize) -> Self {
        if n <= SMALL_DICT_MAX {
            SmallDict::Small(Vec::with_capacity(n))
        } else {
            SmallDict::Large(HashMap::with_capacity(n))
        }
    }

    fn len(&self) -> usize {
        match self {
            SmallDict::Empty => 0,
            SmallDict::Small(pairs) => pairs.len(),
            SmallDict::Large(map) => map.len(),
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            SmallDict::Empty => None,
            SmallDict::Small(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            SmallDict::Large(map) => map.get(key),
        }
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            SmallDict::Empty => None,
            SmallDict::Small(pairs) => pairs.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            SmallDict::Large(map) => map.get_mut(key),
        }
    }

    fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    fn insert(&mut self, key: String, value: Value) -> Option<Value> {
        match self {
            SmallDict::Empty => {
                let mut pairs = Vec::with_capacity(4);
                pairs.push((key, value));
                *self = SmallDict::Small(pairs);
                None
            }
            SmallDict::Small(pairs) => {
                if let Some(slot) = pairs.iter_mut().find(|(k, _)| *k == key) {
                    return Some(std::mem::replace(&mut slot.1, value));
                }
                if pairs.len() < SMALL_DICT_MAX {
                    pairs.push((key, value));
                    return None;
                }
                let mut map: HashMap<String, Value> = pairs.drain(..).collect();
                map.insert(key, value);
                *self = SmallDict::Large(map);
                None
            }
            SmallDict::Large(map) => map.insert(key, value),
        }
    }

    fn remove(&mut self, key: &str) -> Option<Value> {
        match self {
            SmallDict::Empty => None,
            // Order is not part of a dict's meaning, so the cheap removal is fine
            // — but a small dict keeps insertion order for printing, so removing
            // is the one place it can shuffle. Nothing depends on that order.
            SmallDict::Small(pairs) => {
                let at = pairs.iter().position(|(k, _)| k == key)?;
                Some(pairs.swap_remove(at).1)
            }
            SmallDict::Large(map) => map.remove(key),
        }
    }

    fn iter(&self) -> DictIter<'_> {
        match self {
            SmallDict::Empty => DictIter::Empty,
            SmallDict::Small(pairs) => DictIter::Small(pairs.iter()),
            SmallDict::Large(map) => DictIter::Large(map.iter()),
        }
    }

    fn keys(&self) -> impl Iterator<Item = &String> {
        self.iter().map(|(k, _)| k)
    }

    fn values(&self) -> impl Iterator<Item = &Value> {
        self.iter().map(|(_, v)| v)
    }

    /// Add every pair of `other`, overwriting what is already here. What
    /// `Value::Dict + Value::Dict` means.
    fn extend(&mut self, other: SmallDict) {
        match other {
            SmallDict::Empty => {}
            SmallDict::Small(pairs) => {
                for (k, v) in pairs {
                    self.insert(k, v);
                }
            }
            SmallDict::Large(map) => {
                for (k, v) in map {
                    self.insert(k, v);
                }
            }
        }
    }

    /// Keep only the pairs `f` accepts.
    fn retain<F: FnMut(&String, &Value) -> bool>(&mut self, mut f: F) {
        match self {
            SmallDict::Empty => {}
            SmallDict::Small(pairs) => pairs.retain(|(k, v)| f(k, v)),
            SmallDict::Large(map) => map.retain(|k, v| f(k, v)),
        }
    }
}

impl From<HashMap<String, Value>> for SmallDict {
    fn from(map: HashMap<String, Value>) -> Self {
        // A map that arrived as a hash map but is small still gets the scan: the
        // point is the representation, not how the dict was built.
        if map.len() <= SMALL_DICT_MAX {
            SmallDict::Small(map.into_iter().collect())
        } else {
            SmallDict::Large(map)
        }
    }
}

impl FromIterator<(String, Value)> for SmallDict {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(iter: I) -> Self {
        let mut out = SmallDict::new();
        for (k, v) in iter {
            out.insert(k, v);
        }
        out
    }
}

/// Iterator over a `SmallDict`'s pairs, whichever representation it is in. An
/// enum rather than a boxed trait object so that walking a dict — which happens
/// in every `for (k, v) in d` and every dump — allocates nothing.
enum DictIter<'a> {
    Empty,
    Small(std::slice::Iter<'a, (String, Value)>),
    Large(std::collections::hash_map::Iter<'a, String, Value>),
}

impl<'a> Iterator for DictIter<'a> {
    type Item = (&'a String, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            DictIter::Empty => None,
            DictIter::Small(pairs) => pairs.next().map(|(k, v)| (k, v)),
            DictIter::Large(map) => map.next(),
        }
    }
}

impl<'a> IntoIterator for &'a SmallDict {
    type Item = (&'a String, &'a Value);
    type IntoIter = DictIter<'a>;

    fn into_iter(self) -> DictIter<'a> {
        self.iter()
    }
}


/// The map type for anything keyed by a variable, field or property name — the
/// maps on the interpreter's hot path. See the note on `FxHasher` above.
///
/// Deliberately *not* used for `Value::Dict`: measured against CPython, dict
/// indexing was already the closest operation (~3.4-4.5x) because Python's dict
/// is also a hash table, while the worst offenders (`call_2arg` at 7.9x,
/// `list_append` at 7.2x, `list_index_read` at 6.8x) are all dominated by
/// `get_var`/`set_var` — one or more name lookups per operation, taken every
/// single time. Converting the variable maps attacks those directly; converting
/// `Dict` as well would mean threading a new type through ~40 dict-construction
/// sites for the operation that needed it least.
type Map<K, V> = HashMap<K, V, FxBuildHasher>;

const DEBUGGER_STOP_MSG: &str = "Execution stopped by debugger";

const INDENT_VERSION: &str = env!("CARGO_PKG_VERSION");

type WsClient = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

static WS_CLIENTS: LazyLock<Mutex<HashMap<i64, WsClient>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static WS_NEXT_ID: AtomicI64 = AtomicI64::new(1);
static RNG_STATE: AtomicI64 = AtomicI64::new(0x4d595df4d0f33173u64 as i64);
static SAFE_MODE: AtomicBool = AtomicBool::new(false);
/// Set by `--no-cache`: disables reading and writing the on-disk AST cache.
static AST_CACHE_OFF: AtomicBool = AtomicBool::new(false);
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

/// Stack sizes for background threads.
///
/// `std::thread::spawn` uses the platform default — 8 MB of *reserved* address
/// space on Linux — for every task, including one that only sleeps. A bot with a
/// few hundred live futures therefore reserves gigabytes of address space it
/// never touches. These are a deliberate compromise rather than the 256 KB a
/// minimum-size task could get away with: a user task can recurse arbitrarily
/// deeply, and a small stack would turn user recursion that works on the main
/// thread into a hard crash inside `spawn`. 2 MB keeps the reservation 4x
/// smaller while still leaving thousands of frames; the timer thread runs no user
/// code at all, so it can be tiny.
const USER_TASK_STACK: usize = 2 * 1024 * 1024;
const BUILTIN_TASK_STACK: usize = 512 * 1024;
const TIMER_TASK_STACK: usize = 64 * 1024;

/// Spawn a background thread with an explicit stack size.
fn spawn_task_thread<F>(
    name: &str,
    stack: usize,
    body: F,
) -> Result<std::thread::JoinHandle<()>, String>
where
    F: FnOnce() + Send + 'static,
{
    std::thread::Builder::new()
        .name(name.to_string())
        .stack_size(stack)
        .spawn(body)
        .map_err(|e| format!("could not start background task: {e}"))
}

/// How many finished tasks to keep around for `task_result`.
const TASK_HISTORY_MAX: usize = 256;

/// Live entries allowed in `WS_CLIENTS`.
///
/// The table is process-wide and used to be unbounded, so a client that kept
/// reconnecting left every dead socket and its buffers behind. A program that
/// genuinely needs more than this many *simultaneous* sockets is beyond what the
/// builtins are meant for; see `ws_close` for the explicit release path.
const WS_CLIENTS_MAX: usize = 256;

/// Distinct modules kept in `module_cache`.
///
/// The cache is keyed by canonical path and nothing ever evicted from it, so a
/// program that imports from disk repeatedly — a plugin loader, a test runner
/// walking a directory — kept one fully loaded module per distinct path for the
/// life of the process. See the insert in `load_module` for how it is bounded.
const MODULE_CACHE_MAX: usize = 500;

/// Record a task, evicting the oldest *completed* entries once the history gets
/// long.
///
/// Nothing removed entries from `TASKS` except an explicit `future_cancel`, and
/// `task_wait` only took the `JoinHandle`, so every `spawn`/`future`/`sleep`/
/// `*_async` leaked its entry (and the result `Value` inside it) for the life of
/// the program — a bot spawning 10 futures a second grows this map without bound.
/// Live tasks are never evicted; only finished ones, oldest first, and only past
/// the cap.
/// Track a background task so `task_status` and `wait` can find it.
///
/// The cap is on *finished* history: once the map is past `TASK_HISTORY_MAX` the
/// oldest completed entries are evicted, oldest id first. Live tasks are
/// deliberately never dropped — a program may legitimately hold more than
/// `TASK_HISTORY_MAX` running tasks, and evicting one would lose a handle the
/// program can still join. So a busy program's map can exceed the cap, and the
/// ceiling that actually bounds memory is the number of concurrent live tasks.
fn register_task(id: i64, state: TaskState) {
    let mut tasks = TASKS.lock().unwrap();
    tasks.insert(id, state);
    if tasks.len() > TASK_HISTORY_MAX {
        let mut finished: Vec<i64> = tasks
            .iter()
            .filter(|(_, t)| t.done.load(Ordering::SeqCst))
            .map(|(id, _)| *id)
            .collect();
        finished.sort_unstable();
        let excess = tasks.len() - TASK_HISTORY_MAX;
        for id in finished.into_iter().take(excess) {
            tasks.remove(&id);
        }
    }
}

#[derive(Debug, Clone)]
struct SourceLine {
    line_no: usize,
    indent: usize,
    text: String,
}

/// A parsed statement. `Serialize`/`Deserialize` exist so the whole parsed
/// program can be stored in the on-disk AST cache (`__incache__/*.inc`) and
/// reloaded without lexing or parsing again.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    AssignSlice(Box<AssignSliceStmt>),
    DefFun {
        line: usize,
        name: String,
        params: Vec<FunctionParam>,
        return_type: Option<String>,
        /// The body is behind an `Arc` because a `fun` statement is *executed*:
        /// `Stmt::DefFun` builds a `FunctionDef` whose body is the same statements,
        /// so an owned `Vec` meant a deep copy of the whole body per execution —
        /// and the parser was copying it a second time to keep a handle for
        /// `stmts_contain_yield`. Two `Arc` increments now replace both, and a
        /// body cloned from the AST cache (which deserialises it) is shared too.
        /// Serialised form is unchanged: `serde`'s `rc` feature writes the value.
        body: Arc<Vec<Stmt>>,
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
    DefClass(Box<DefClassStmt>),
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
    /// Launch: `launch <path>` — run another Indent file in this runtime.
    Launch {
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

/// The payload of a slice assignment, boxed out of `Stmt`.
///
/// `Stmt` was 152 bytes for *every* statement, and this variant was the reason:
/// three optional bound expressions plus a `ValueSource` is 152 bytes on its own.
/// Measured payloads (temporary layout probe, since removed): `AssignSlice` 152,
/// `DefClass` 112, `DoChain` 104, `DefVar`/`AssignOp`/`AssignIndex` 104, `DefFun`
/// 96 — and the shapes that appear most often, `Say` and `BareExpr`, only 32.
///
/// So the two variants above 104 are the two worth boxing: doing that takes
/// `Stmt` to 104 while adding an allocation only for slice assignments and class
/// definitions, both rare. Boxing further would mean boxing the common
/// assignment shapes, which trades an allocation per statement for 16-48 inline
/// bytes — and the allocation (pointer plus the allocator's header) costs more
/// than the inline bytes it saves.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AssignSliceStmt {
    line: usize,
    name: String,
    start_expr: Option<String>,
    end_expr: Option<String>,
    step_expr: Option<String>,
    value: ValueSource,
}

/// The payload of a class definition, boxed out of `Stmt` for the same reason as
/// `AssignSliceStmt`: two `Vec`s, two `String`s and a bool came to 112 bytes, and
/// a class is defined once per program while every statement paid that width.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DefClassStmt {
    line: usize,
    name: String,
    parent: Option<String>,
    fields: Vec<ClassField>,
    methods: Vec<Stmt>,
    is_dataclass: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum RepeatMode {
    Infinite,
    Count(String),
    ForEach(String),
    ForIn { item_name: String, iterable_expr: String },
    While(String),
    Until(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum ValueSource {
    Expr(String),
    Call { callee: String, args: Vec<ArgItem> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
enum ArgItem {
    Positional(String),
    Named { name: String, expr: String },
    /// Boxed: this variant embeds a `Stmt`, which made `ArgItem` 152 bytes — so
    /// *every* argument of *every* call in the AST cost 152 bytes (a four-argument
    /// call, 608). Inline-`var` arguments are rare, so the box is essentially never
    /// paid while the saving applies to every call argument there is.
    DefVar(Box<Stmt>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Shared with the `Stmt::DefFun` it was built from, so defining a function is
    /// a refcount bump rather than a copy of its body. `FunctionDef` is not
    /// serialised (it is built from the AST at run time), so this costs nothing
    /// at the cache boundary.
    body: Arc<Vec<Stmt>>,
    is_generator: bool,
    /// The body as instructions, compiled on the first call that needs it.
    ///
    /// `None` means "the VM cannot take this body" (or cannot pay for it), which is
    /// as important to remember as a program: without this, every call re-ran the
    /// whole compile — parse-cache lookups, slot allocation and instruction
    /// building — and a small body's own work is less than that. The body never
    /// changes, and neither does the answer, so one cell per function is enough.
    ///
    /// Nothing here is serialised (`FunctionDef` is built from `Stmt::DefFun` at
    /// run time), so this cannot leak into the AST cache.
    compiled: OnceLock<Option<Arc<VmProgram>>>,
}

// ---- Classes (OOP) ----
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClassField {
    name: String,
    _ty: String,
}

#[derive(Debug, Clone)]
struct ClassDef {
    name: String,
    parent: Option<String>,
    fields: Vec<ClassField>,
    methods: HashMap<String, Arc<FunctionDef>>,
    is_dataclass: bool,
}

/// Everything every instance of a class shares, built once per class definition:
/// the name, the merged field list in slot order, the name-to-slot index, and
/// the merged method table.
///
/// An instance used to carry a `String` for the class name, a `HashMap` of its
/// own for the fields — which stored every field *name* again, once per instance
/// — and an `Arc` to a method table. For a five-field, ten-method class that is
/// roughly 850 bytes of bookkeeping per object, nearly all of it duplicate. Now
/// an instance is a pointer to this and an array of values, about 150 bytes, and
/// a field read is an index rather than a hash lookup.
///
/// `field_index` is what makes the slot layout usable: the class statement
/// declares the fields, and a write to a name it does not declare is refused, so
/// there is no overflow bucket to fall back to.
#[derive(Debug)]
struct ClassMeta {
    name: String,
    /// Merged, parent fields first: this *is* the slot order.
    field_names: Vec<String>,
    field_index: Map<String, u32>,
    methods: HashMap<String, Arc<FunctionDef>>,
    is_dataclass: bool,
}

impl ClassMeta {
    /// Find a method by name, matching case-insensitively.
    ///
    /// The map lookup comes first. Call sites spell methods the way they were
    /// declared, and this used to be a scan of the whole method table on *every*
    /// object method call and on every special-method lookup — `say obj`, `+`,
    /// `==`, `string()`, `len()` and the VM's `PlaceMethodCall` guard. A call
    /// spelled differently still works, at the price of the linear fallback, which
    /// is what keeps Indent's case-insensitive dispatch intact.
    fn find_method(&self, name: &str) -> Option<&Arc<FunctionDef>> {
        if let Some(def) = self.methods.get(name) {
            return Some(def);
        }
        self.methods
            .iter()
            .find(|(declared, _)| declared.eq_ignore_ascii_case(name))
            .map(|(_, def)| def)
    }
}

/// A function body is immutable once parsed, so it is shared behind an `Arc`.
/// This matters enormously: `resolve_callable` clones the resolved callable on
/// **every call**, and a by-value `FunctionDef` would deep-copy the parameters
/// and the entire body AST each time. A 300-statement body used to cost ~16 µs
/// per call *even when none of it executed*; sharing makes it a refcount bump.
#[derive(Debug, Clone)]
enum Callable {
    Local(Arc<FunctionDef>),
    External { module: Arc<ModuleInstance>, name: String },
    Builtin(String),
}

#[derive(Debug, Clone)]
/// One name in an imported module's namespace.
///
/// A module used to keep three parallel maps — variables, functions, and the
/// callables it imported from other modules — which meant three bucket arrays,
/// and a function's name stored twice (once in `funcs`, once in `callables`) to
/// say the same thing. One map holds one bucket array and one key per name.
///
/// Precedence, which is the part that has to be stated rather than assumed,
/// because a single entry per name forces a choice the old three maps never had
/// to make. `Func` outranks `Imported` so that a module's own function wins over
/// an imported one of the same name (the old injection loop skipped exactly that
/// case), and both outrank `Value` so that `get X from Y` still resolves a name
/// that is a function rather than a variable. A module with a *variable* and a
/// *function* sharing a name loses the variable from its namespace; that is not
/// a shape any module in `std/` has, and it is the one case this fold changes.
enum ModuleEntry {
    Value(Value),
    Func(Arc<FunctionDef>),
    Imported(Callable),
}

#[derive(Debug, Clone)]
struct ModuleInstance {
    entries: Map<String, ModuleEntry>,
}

/// A list's elements, shared by every binding that holds them.
///
/// Reading a list variable used to deep-copy every element. `len(big)` on a
/// 20 000-element list cost 1.1 MB of copying *per call*, which made read-only work
/// on a big collection O(n) per use — `bench_container_reads` measured the compiled
/// path 1700x slower than the tree-walker on exactly that line, because the VM's
/// `LoadSlot` clones the value it reads. `for x in xs` cloned `xs` too.
///
/// The `Arc` turns a read into a refcount bump. Writes go through `Arc::make_mut`,
/// which copies only when the buffer really is shared: the pass-by-value model the
/// language already had, deferred to the first write instead of paid on every read.
/// `DerefMut` does that automatically, so a site that mutates a list it owns pays
/// nothing, and a site that mutates one it shares keeps its own copy — which is the
/// same result the old code got by cloning eagerly, only later.
#[derive(Debug, Clone, Default)]
struct SharedList(Arc<Vec<Value>>);

impl SharedList {
    /// The elements themselves, for a caller that needs to own them — taking them out
    /// of the `Arc` when nothing else holds it, and copying only when something does.
    fn into_vec(self) -> Vec<Value> {
        match Arc::try_unwrap(self.0) {
            Ok(vec) => vec,
            Err(shared) => (*shared).clone(),
        }
    }
}

impl From<Vec<Value>> for SharedList {
    fn from(vec: Vec<Value>) -> Self {
        Self(Arc::new(vec))
    }
}

impl std::ops::Deref for SharedList {
    type Target = Vec<Value>;
    fn deref(&self) -> &Vec<Value> {
        &self.0
    }
}

/// A class instance's field values, shared by every binding that holds the
/// instance.
///
/// The same change as `SharedList` and for the same reason, one level up: an
/// object's fields were a `Box<[Value]>`, so *reading* a variable that holds an
/// instance copied the whole field array — every field, and every `Value` in it.
/// Passing an object to a function, storing it in a list, or reading one inside a
/// loop therefore cost a copy of the instance per use, which is the eager half of
/// pass-by-value that `ListShared` already deferred.
///
/// Writes go through `Arc::make_mut`, so the first write after a second handle
/// exists pays one copy and repeated writes do not. That is exactly the behaviour
/// the old eager clone produced: a field written through an alias never reaches the
/// original. `tests/object_aliasing.ind` pins all seven shapes that has to hold for.
///
/// `Arc<Box<[Value]>>` rather than `Arc<[Value]>` because `Arc::try_unwrap` needs a
/// `Sized` payload — `Arc<[Value]>` cannot be taken apart in O(1), so the paths that
/// move the array out would copy it every time. It is the same reason `SharedStr`
/// holds an `Arc<String>` and not an `Arc<str>`.
#[derive(Debug, Clone)]
struct SharedFields(Arc<Box<[Value]>>);

impl From<Vec<Value>> for SharedFields {
    fn from(fields: Vec<Value>) -> Self {
        SharedFields(Arc::new(fields.into_boxed_slice()))
    }
}

impl From<Box<[Value]>> for SharedFields {
    fn from(fields: Box<[Value]>) -> Self {
        SharedFields(Arc::new(fields))
    }
}

impl std::ops::Deref for SharedFields {
    type Target = Box<[Value]>;
    fn deref(&self) -> &Box<[Value]> {
        &self.0
    }
}

impl std::ops::DerefMut for SharedFields {
    fn deref_mut(&mut self) -> &mut Box<[Value]> {
        Arc::make_mut(&mut self.0)
    }
}

/// A dictionary's entries, shared by every binding that holds them.
///
/// Exactly the same change as `SharedList`, and it was measurable for exactly the same
/// reason: `bench_container_reads` spent 0.0706 s on `300 x len(5k dict)` and again on
/// `300 x has_key(5k dict)`, both of which only need to *look* at the table.
#[derive(Debug, Clone, Default)]
struct SharedDict(Arc<SmallDict>);

impl SharedDict {
    /// The entries themselves, for a caller that needs to own them.
    fn into_inner(self) -> SmallDict {
        match Arc::try_unwrap(self.0) {
            Ok(dict) => dict,
            Err(shared) => (*shared).clone(),
        }
    }
}

impl From<SmallDict> for SharedDict {
    fn from(dict: SmallDict) -> Self {
        Self(Arc::new(dict))
    }
}

impl std::ops::Deref for SharedDict {
    type Target = SmallDict;
    fn deref(&self) -> &SmallDict {
        &self.0
    }
}

impl std::ops::DerefMut for SharedDict {
    fn deref_mut(&mut self) -> &mut SmallDict {
        Arc::make_mut(&mut self.0)
    }
}

/// Iterating a borrowed shared dict, delegating to the underlying type's own iterator
/// rather than restating its item type here.
impl<'a> IntoIterator for &'a SharedDict {
    type Item = <&'a SmallDict as IntoIterator>::Item;
    type IntoIter = <&'a SmallDict as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        (&*self.0).into_iter()
    }
}

impl std::ops::DerefMut for SharedList {
    fn deref_mut(&mut self) -> &mut Vec<Value> {
        Arc::make_mut(&mut self.0)
    }
}

/// Iterating a borrowed shared list is free — the elements are already in one buffer
/// and nothing is moved out, so this is the shape to prefer.
impl<'a> IntoIterator for &'a SharedList {
    type Item = &'a Value;
    type IntoIter = std::slice::Iter<'a, Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Iterating an owned one has to hand the elements out by value, so it costs a copy
/// when the buffer is still shared. That is the same result the old code got by
/// cloning eagerly; it is just no longer paid when the caller only reads.
impl IntoIterator for SharedList {
    type Item = Value;
    type IntoIter = std::vec::IntoIter<Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

/// A shared string.
///
/// `Value::Str` held an owned `String`, so *reading* a string variable copied its
/// whole buffer: 201 KB per read, and 5.6 copies' worth to pass one string to a
/// function (`benchmarks/bench_string_reads.ind`). It also meant the same bytes
/// were resident once per holder — 200 copies of one 200 KB string cost ~40 MB of
/// RSS (`benchmarks/string_memory.ind`), which is most of that process.
///
/// The payload is `Arc<String>` rather than `Arc<str>`, which costs one extra
/// pointer hop per read and buys two things that matter more here. First,
/// `Arc<str>` cannot be taken apart: `Arc::try_unwrap` needs `Sized`, `str` is not,
/// and the standard library offers no `From<Arc<str>> for String` — so there is no
/// way to get the buffer back out in O(1), and the in-place growth paths
/// (`s is s + x`) would copy the whole accumulated prefix on every step, which is
/// exactly the quadratic trap this codebase has removed three times already.
/// `String` is `Sized`, so `try_unwrap` is free while the string is unshared.
/// Second, it makes this type structurally identical to `SharedList` and
/// `SharedDict`, so the same `Deref`/`DerefMut`-to-`make_mut` shape applies and a
/// wrong usage is a compile error rather than a silent copy.
#[derive(Clone)]
struct SharedStr(Arc<String>);

impl SharedStr {
    /// Take the bytes out as an owned `String`, copying only if the buffer is
    /// shared. This is what keeps the in-place string paths O(1) per step: after
    /// the first growth the refcount is back to 1, so every later step unwraps
    /// without touching the bytes.
    fn into_string(self) -> String {
        match Arc::try_unwrap(self.0) {
            Ok(s) => s,
            Err(arc) => (*arc).clone(),
        }
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::ops::Deref for SharedStr {
    type Target = String;
    fn deref(&self) -> &String {
        &self.0
    }
}

/// Writing through a shared string is copy-on-write, exactly as reading is: the
/// first write after a second handle exists pays one copy, and repeated writes in
/// a loop do not, because the writer is then the only owner again.
impl std::ops::DerefMut for SharedStr {
    fn deref_mut(&mut self) -> &mut String {
        Arc::make_mut(&mut self.0)
    }
}

impl AsRef<str> for SharedStr {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl std::borrow::Borrow<str> for SharedStr {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

impl From<String> for SharedStr {
    fn from(s: String) -> Self {
        SharedStr(Arc::new(s))
    }
}

impl From<&str> for SharedStr {
    fn from(s: &str) -> Self {
        SharedStr(Arc::new(s.to_string()))
    }
}

impl fmt::Display for SharedStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl fmt::Debug for SharedStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl PartialEq for SharedStr {
    fn eq(&self, other: &Self) -> bool {
        // The bytes are the value; two handles on one buffer are equal either way,
        // but `ptr_eq` is the cheap way to say so and is what makes comparing a
        // variable with itself cost nothing.
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl Eq for SharedStr {}

impl PartialEq<str> for SharedStr {
    fn eq(&self, other: &str) -> bool {
        self.0.as_str() == other
    }
}

impl PartialEq<&str> for SharedStr {
    fn eq(&self, other: &&str) -> bool {
        self.0.as_str() == *other
    }
}

impl PartialEq<String> for SharedStr {
    fn eq(&self, other: &String) -> bool {
        self.0.as_str() == other.as_str()
    }
}

/// Iterating a string's characters, for the sites that do.

#[derive(Debug, Clone)]
enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    /// The shared string. Written as `Value::Str(s)` everywhere, through the
    /// constructor below: only *patterns* have to know the bytes are behind an
    /// `Arc`, and that split is what let this change be mechanical.
    StrShared(SharedStr),
    /// The shared list. Written as `Value::List(v)` everywhere, through the
    /// constructor below: only *patterns* have to know the elements are behind an
    /// `Arc`, and keeping the two apart is what let this change be mechanical.
    ListShared(SharedList),
    /// The shared set. Its storage is the same ordered, unique `Vec<Value>` a list
    /// uses, so it shares the same payload type rather than restating the
    /// `Arc`/`make_mut` machinery for the third time.
    SetShared(SharedList),
    /// The shared dictionary, for the same reason and by the same route as
    /// `ListShared`: `len(d)` or `has_key d k` on a 5 000-entry dict was copying the
    /// whole table on every call.
    DictShared(SharedDict),
    /// Class instance. The two parts are a pointer to the class's shared
    /// metadata and a slot-indexed array of the values.
    ///
    /// The fields used to be a `Box<HashMap<String, Value>>` keyed by name, which
    /// meant every instance held its own hash table *and* its own copy of every
    /// field name. Slots come from the class, so the instance holds values only:
    /// a field read is `class.field_index[name]` and then an index, and a
    /// five-field instance is five values instead of a table of five entries.
    ///
    /// The array itself is shared and copy-on-write, like `ListShared`: reading a
    /// variable holding an instance used to deep-copy every field, so passing an
    /// object to a function, storing one in a list, or reading one in a loop cost
    /// the whole field array each time. A read is now a refcount bump and the copy
    /// happens on the first *write* — the same pass-by-value result, paid later and
    /// only when something is actually written.
    Object {
        class: Arc<ClassMeta>,
        fields: SharedFields,
    },
    Func(String),
    Module(Arc<ModuleInstance>),
    Empty,
}

impl Value {
    /// Build a list from its elements.
    ///
    /// Named `List` because that is what every call site already said. The variant
    /// itself is `ListShared`, so a *pattern* has to be written differently, which is
    /// the point: a pattern has to deal with the shared representation, and a
    /// construction should not have to. Conversions that were mechanical — every
    /// `Value::List(v)` stayed as it was — are the ones the compiler cannot check for
    /// you, and this is one fewer of them.
    #[allow(non_snake_case)]
    fn List(elements: Vec<Value>) -> Value {
        Value::ListShared(elements.into())
    }

    /// Build a dictionary. Named `Dict` for the same reason as `List` above.
    #[allow(non_snake_case)]
    fn Dict(entries: SmallDict) -> Value {
        Value::DictShared(entries.into())
    }

    /// Build a set. Named `Set` for the same reason as `List` above.
    #[allow(non_snake_case)]
    fn Set(elements: Vec<Value>) -> Value {
        Value::SetShared(elements.into())
    }

    /// Build a string. Named `Str` for the same reason as `List` above.
    #[allow(non_snake_case)]
    fn Str(s: String) -> Value {
        Value::StrShared(s.into())
    }
}

/// The eleven named colors that `var c = RED` resolves to.
///
/// Built once and then *cloned* per runtime. It used to be rebuilt on every
/// `Runtime::new` — eleven keys and eleven values allocated each time, and the
/// test suite constructs a runtime per test file, so a 500-file suite built 11 000
/// strings to hold eleven colors. Now that `Value::Str` shares its bytes, a clone
/// shares every value and only the map keys are copied; the palette itself is the
/// same table `color_to_rgb` already carries, so nothing here is information the
/// program cannot already reach.
fn default_color_vars() -> Map<String, Value> {
    static PALETTE: LazyLock<Map<String, Value>> = LazyLock::new(build_color_vars);
    PALETTE.clone()
}

fn build_color_vars() -> Map<String, Value> {
    const PALETTE: [(&str, &str); 11] = [
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

    // Sized up front: eleven inserts into an empty map reallocated three times.
    let mut out = Map::with_capacity_and_hasher(PALETTE.len(), Default::default());
    for (name, hex) in PALETTE {
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
/// The named-colour table, `(name, hex)`. Looked up case-insensitively so that
/// parsing a name never has to lowercase the input first.
const NAMED_COLORS: [(&str, &str); 31] = [
    // base
    ("red", "ff4d4d"),
    ("green", "32c671"),
    ("blue", "3b82f6"),
    ("cyan", "06b6d4"),
    ("magenta", "d946ef"),
    ("yellow", "facc15"),
    ("orange", "fb923c"),
    ("purple", "8b5cf6"),
    ("pink", "ec4899"),
    ("black", "0f172a"),
    ("white", "f8fafc"),
    // extended palette
    ("gray", "9ca3af"),
    ("grey", "9ca3af"),
    ("silver", "c0c0c0"),
    ("maroon", "800000"),
    ("olive", "808000"),
    ("lime", "a3e635"),
    ("teal", "14b8a6"),
    ("navy", "1e3a8a"),
    ("fuchsia", "c026d3"),
    ("brown", "92400e"),
    ("gold", "fbbf24"),
    ("coral", "ff7f50"),
    ("salmon", "fa8072"),
    ("sky", "38bdf8"),
    ("indigo", "6366f1"),
    ("violet", "a78bfa"),
    ("amber", "f59e0b"),
    ("emerald", "10b981"),
    ("slate", "64748b"),
    ("zinc", "71717a"),
];

/// Look a colour name up without allocating. Returns the hex digits only.
fn named_color_hex(name: &str) -> Option<&'static str> {
    NAMED_COLORS
        .iter()
        .find(|(n, _)| name.eq_ignore_ascii_case(n))
        .map(|(_, hex)| *hex)
}

/// Hex digit to its value, accepting either case.
#[inline]
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Two hex digits at `i` as one byte, without slicing or allocating.
#[inline]
fn hex_pair(h: &[u8], i: usize) -> Option<u8> {
    Some(hex_nibble(*h.get(i)?)? * 16 + hex_nibble(*h.get(i + 1)?)?)
}

fn color_to_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();
    // Borrow the hex digits straight out of the input; a name yields a 'static
    // table entry. Either way nothing is copied or lowercased.
    let hex = match s.strip_prefix('#') {
        Some(h) => h,
        None => named_color_hex(s)?,
    };
    let h = hex.as_bytes();
    match h.len() {
        // #RGB / #RGBA — each digit is doubled
        3 | 4 => {
            let n = |i: usize| hex_nibble(h[i]).map(|d| d * 16 + d);
            Some((n(0)?, n(1)?, n(2)?))
        }
        // #RRGGBB / #RRGGBBAA (alpha ignored)
        6 | 8 => Some((hex_pair(h, 0)?, hex_pair(h, 2)?, hex_pair(h, 4)?)),
        _ => None,
    }
}

/// Build a single combined ANSI SGR sequence from an optional foreground,
/// optional background and a list of style codes. Returns plain `text` when no
/// codes apply (so `paint` can be used with all-empty options).
fn render_ansi(
    text: &str,
    fg: Option<(u8, u8, u8)>,
    bg: Option<(u8, u8, u8)>,
    styles: &[u8],
) -> String {
    let mut codes: Vec<String> = Vec::new();
    for s in styles {
        codes.push(s.to_string());
    }
    if let Some((r, g, b)) = fg {
        codes.push(format!("38;2;{r};{g};{b}"));
    }
    if let Some((r, g, b)) = bg {
        codes.push(format!("48;2;{r};{g};{b}"));
    }
    if codes.is_empty() {
        return text.to_string();
    }
    format!("\x1b[{}m{}\x1b[0m", codes.join(";"), text)
}

/// Map a style name to its ANSI SGR code.
fn style_code(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "bold" | "bright" => Some(1),
        "dim" | "faint" => Some(2),
        "italic" => Some(3),
        "underline" => Some(4),
        "blink" => Some(5),
        "reverse" | "invert" | "negative" => Some(7),
        "strikethrough" | "strike" | "crossed" => Some(9),
        _ => None,
    }
}

/// Linearly interpolate one channel.
fn lerp(a: u8, b: u8, t: f64) -> u8 {
    (a as f64 + (b as f64 - a as f64) * t).round() as u8
}

/// Color each character of `text` by interpolating `from` → `to`. `bg` selects
/// the 48;2 background plane instead of the 38;2 foreground plane.
fn gradient_text(text: &str, from: (u8, u8, u8), to: (u8, u8, u8), bg: bool) -> String {
    use std::fmt::Write as _;

    let plane = if bg { "48;2" } else { "38;2" };
    // Counting costs one pass but avoids materialising the whole string as a
    // Vec<char>; the escapes are then written straight into one pre-sized
    // buffer instead of building a String per character.
    let n = text.chars().count();
    if n == 0 {
        return String::new();
    }
    if n == 1 {
        let (r, g, b) = from;
        return format!("\x1b[{plane};{r};{g};{b}m{text}\x1b[0m");
    }
    let mut out = String::with_capacity(text.len() + n * 22);
    for (i, c) in text.chars().enumerate() {
        let t = i as f64 / (n - 1) as f64;
        let r = lerp(from.0, to.0, t);
        let g = lerp(from.1, to.1, t);
        let b = lerp(from.2, to.2, t);
        let _ = write!(out, "\x1b[{plane};{r};{g};{b}m{c}\x1b[0m");
    }
    out
}

/// Color each character of `text` cycling through `colors`. `bg` selects the
/// background plane.
fn multicolor_text(text: &str, colors: &[(u8, u8, u8)], bg: bool) -> String {
    use std::fmt::Write as _;

    if text.is_empty() || colors.is_empty() {
        return text.to_string();
    }
    let plane = if bg { "48;2" } else { "38;2" };
    let mut out = String::with_capacity(text.len() * 2 + text.chars().count() * 22);
    for (i, c) in text.chars().enumerate() {
        let (r, g, b) = colors[i % colors.len()];
        let _ = write!(out, "\x1b[{plane};{r};{g};{b}m{c}\x1b[0m");
    }
    out
}

/// Classic 7-color rainbow palette.
fn rainbow_palette() -> Vec<(u8, u8, u8)> {
    vec![
        (255, 77, 77),   // red
        (251, 146, 60),  // orange
        (250, 204, 21),  // yellow
        (50, 198, 113),  // green
        (6, 182, 212),   // cyan
        (59, 130, 246),  // blue
        (139, 92, 246),  // violet
    ]
}

fn rainbow_text(text: &str, bg: bool) -> String {
    multicolor_text(text, &rainbow_palette(), bg)
}

/// Write `n` as decimal digits, straight into the formatter.
///
/// `core::fmt`'s integer formatting routes through its generic machinery (width,
/// fill, precision, alignment flags) that this `Display` impl can never be asked
/// to honour, and every `say`, `string(n)` and string concatenation involving a
/// number goes through here. The digits themselves are a handful of integer ops.
#[inline]
fn write_int(f: &mut fmt::Formatter<'_>, n: i64) -> fmt::Result {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    // `unsigned_abs` so that i64::MIN does not overflow on negation.
    let mut u = n.unsigned_abs();
    loop {
        i -= 1;
        buf[i] = b'0' + (u % 10) as u8;
        u /= 10;
        if u == 0 {
            break;
        }
    }
    if n < 0 {
        i -= 1;
        buf[i] = b'-';
    }
    match std::str::from_utf8(&buf[i..]) {
        Ok(digits) => f.write_str(digits),
        // Unreachable: only ASCII digits and '-' are ever written above.
        Err(_) => Err(fmt::Error),
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(v) => write_int(f, *v),
            Value::Float(v) => {
                if (v.fract()).abs() < f64::EPSILON {
                    write!(f, "{:.1}", v)
                } else {
                    write!(f, "{v}")
                }
            }
            Value::Bool(v) => write!(f, "{}", if *v { "TRUE" } else { "FALSE" }),
            Value::StrShared(v) => write!(f, "{v}"),
            Value::ListShared(items) => {
                let joined = items
                    .iter()
                    .map(|x| format!("{x}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "[{joined}]")
            }
            Value::SetShared(items) => {
                let joined = items
                    .iter()
                    .map(|x| format!("{x}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{{{joined}}}")
            }
            Value::DictShared(items) => {
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
            Value::Object { class, fields } => {
                // Declaration order, which is the slot order too.
                let joined = class
                    .field_names
                    .iter()
                    .zip(fields.iter())
                    .map(|(name, value)| format!("{}: {}", name, value))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{}({joined})", class.name)
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
            Value::StrShared(v) => !v.is_empty(),
            Value::ListShared(v) => !v.is_empty(),
            Value::SetShared(v) => !v.is_empty(),
            Value::DictShared(v) => !v.is_empty(),
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
            Value::StrShared(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::StrShared(_) => "string",
            Value::ListShared(_) => "list",
            Value::SetShared(_) => "group",
            Value::DictShared(_) => "dict",
            Value::Func(_) => "function",
            Value::Module(_) => "module",
            Value::Object { .. } => "object",
            Value::Empty => "empty",
        }
    }
}

// Instantiate a class: create an Object with fields initialized from positional args.
// If the class has a parent, parent fields come first, then child fields.
fn instantiate_class(class_def: &ClassDef, args: &[Value], rt: &mut Runtime) -> Value {
    let class = rt.class_meta(class_def);
    // One slot per merged field, parent fields first, so a positional argument is
    // simply its index — the merged list is what `class_meta` computed.
    let mut fields: Vec<Value> = vec![Value::Empty; class.field_names.len()];
    for (slot, value) in args.iter().enumerate() {
        if slot < fields.len() {
            fields[slot] = value.clone();
        }
    }
    Value::Object {
        class,
        fields: fields.into_boxed_slice().into(),
    }
}

/// Recursively collect all field *names* from a class and its ancestors (parent
/// first). The names are the slot layout of every instance of the class.
fn collect_field_names(class_def: &ClassDef, classes: &Map<String, ClassDef>) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(ref parent_name) = class_def.parent {
        if let Some(parent_def) = classes.get(parent_name) {
            names = collect_field_names(parent_def, classes);
        }
    }
    for field in &class_def.fields {
        if !names.iter().any(|n| n == &field.name) {
            names.push(field.name.clone());
        }
    }
    names
}

/// Recursively collect all methods from a class and its ancestors (parent first, child overrides)
fn collect_all_methods(class_def: &ClassDef, classes: &Map<String, ClassDef>) -> HashMap<String, Arc<FunctionDef>> {
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
        Value::StrShared(v) => v.is_empty(),
        Value::ListShared(v) => v.is_empty(),
        Value::SetShared(v) => v.is_empty(),
        Value::DictShared(v) => v.is_empty(),
        _ => false,
    }
}

/// Whether a single statement (recursively) contains a `yield`, marking the
/// enclosing function as a generator.
fn stmt_contain_yield(s: &Stmt) -> bool {
    match s {
        Stmt::Yield { .. } => true,
        Stmt::IfChain { branches, .. } => branches.iter().any(|(_, b)| stmts_contain_yield(b)),
        Stmt::Match { branches, otherwise_body, .. } => {
            branches.iter().any(|(_, b)| stmts_contain_yield(b))
                || otherwise_body.as_ref().map_or(false, |b| stmts_contain_yield(b))
        }
        Stmt::DoChain { do_body, catches, otherwise_body, lastly_body, .. } => {
            stmts_contain_yield(do_body)
                || catches.iter().any(|(_, b)| stmts_contain_yield(b))
                || otherwise_body.as_ref().map_or(false, |b| stmts_contain_yield(b))
                || lastly_body.as_ref().map_or(false, |b| stmts_contain_yield(b))
        }
        Stmt::Repeat { body, .. } => stmts_contain_yield(body),
        Stmt::Loop { body, .. } => stmts_contain_yield(body),
        Stmt::AsyncWith { body, .. } => stmts_contain_yield(body),
        Stmt::Open { body, .. } => stmts_contain_yield(body),
        Stmt::Decorator { target, .. } => stmt_contain_yield(target),
        _ => false,
    }
}

fn stmts_contain_yield(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_contain_yield)
}

/// Whether a statement holds a loop, at any depth.
///
/// Used to decide whether a function body is worth compiling. Measured, a body
/// whose work is a loop runs several times faster as instructions, while a body of
/// a couple of straight-line statements pays the slot setup, the compile and the
/// call's variable hand-over for less work than the tree-walker does — `give x + x`
/// came out 1.6x *slower*. Only `Repeat` and the `if` chains that can contain one
/// are searched: anything else is either refused by the compiler or too rare to
/// matter.
fn stmt_contain_loop(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Repeat { .. } => true,
        Stmt::IfChain { branches, .. } => {
            branches.iter().any(|(_, body)| stmts_contain_loop(body))
        }
        _ => false,
    }
}

fn stmts_contain_loop(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_contain_loop)
}

/// Convert an *owned* iterable into its items without copying it again.
///
/// `iterable_items` takes a reference and clones the whole list/set, and loops
/// call it on a value they already own — so every `repeat x in xs` copied the
/// collection twice per execution (once reading the variable, once here). The
/// loop now consumes the value it evaluated, which is the same snapshot with one
/// fewer O(n) copy.
fn iterable_items_owned(value: Value) -> Option<Vec<Value>> {
    iterable_items_owned_typed(value, true)
}

/// `iterable_items_owned`, with the accepted set chosen by the source form.
///
/// A `repeat for` takes a generator; a comprehension takes only a list or a set, and
/// reports it with its own message. Sharing one lookup would have let a comprehension
/// quietly succeed where the tree-walker raises.
fn iterable_items_owned_typed(value: Value, generators: bool) -> Option<Vec<Value>> {
    match value {
        // `into_vec` moves the buffer out when nothing else holds it, so a loop over
        // a list the caller owns still does not copy — but a shared one is read, not
        // moved away from, and that is the one case that copies.
        Value::ListShared(v) => Some(v.into_vec()),
        Value::SetShared(v) => Some(v.into_vec()),
        Value::DictShared(mut m)
            if generators && m.get("__generator__").map_or(false, |v| v.to_bool()) =>
        {
            match m.remove("__gen_values__") {
                Some(Value::ListShared(v)) => Some(v.into_vec()),
                _ => Some(Vec::new()),
            }
        }
        _ => None,
    }
}

/// Convert an iterable (list, set, or generator) into its items.
fn iterable_items(value: &Value) -> Option<Vec<Value>> {
    match value {
        // A read of an iterable is an owned snapshot, so it copies the elements —
        // which is exactly what the caller asked for.
        Value::ListShared(v) => Some(v.to_vec()),
        Value::SetShared(v) => Some(v.to_vec()),
        Value::DictShared(m) if m.get("__generator__").map_or(false, |v| v.to_bool()) => {
            match m.get("__gen_values__") {
                Some(Value::ListShared(v)) => Some(v.to_vec()),
                _ => Some(Vec::new()),
            }
        }
        _ => None,
    }
}

/// Helper to resolve simple identifiers or dotted chains like `user.name`
/// from the execution context.
fn resolve_context_value(name: &str, ctx: &ExecContext<'_>) -> Option<Value> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.contains('.') {
        return ctx.get_var(trimmed);
    }
    let parts: Vec<&str> = trimmed.split('.').collect();
    let mut cur = ctx.get_var(parts[0])?;
    for p in &parts[1..] {
        match cur {
            Value::DictShared(ref map) => {
                cur = map.get(*p)?.clone();
            }
            Value::Object { ref class, ref fields } => {
                let slot = *class.field_index.get(*p)? as usize;
                cur = fields[slot].clone();
            }
            Value::Module(ref m) => {
                // Only a variable is a value here. A function of this name used to
                // miss the `vars` lookup and fail the interpolation, and it still
                // does — the entry type just says so explicitly now.
                cur = match m.entries.get(*p)? {
                    ModuleEntry::Value(v) => v.clone(),
                    _ => return None,
                };
            }
            _ => return None,
        }
    }
    Some(cur)
}

/// Interpolate placeholders (%var%, ${var}, or {var}) in a string with values
/// from the execution context. Supports dotted names (e.g. `%user.name%`).
fn interpolate_string(s: &str, ctx: &ExecContext<'_>) -> String {
    // Most strings a program prints have nothing to interpolate, and every `say`
    // of a literal goes through here: copy once instead of walking the text a
    // byte at a time.
    if !s.as_bytes().iter().any(|b| *b == b'%' || *b == b'$' || *b == b'{') {
        return s.to_string();
    }
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
                if let Some(val) = resolve_context_value(var_name, ctx) {
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
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let start = i + 2;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'}' {
                j += 1;
            }
            if j < bytes.len() && j > start {
                let var_name = &s[start..j];
                if let Some(val) = resolve_context_value(var_name, ctx) {
                    out.push_str(&val.to_string());
                } else {
                    out.push_str("${");
                    out.push_str(var_name);
                    out.push('}');
                }
                i = j + 1;
                continue;
            }
        }
        if bytes[i] == b'{' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'}' {
                j += 1;
            }
            if j < bytes.len() && j > start {
                let var_name = &s[start..j];
                // Only treat as variable placeholder if it looks like an identifier or dotted chain
                if var_name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                    if let Some(val) = resolve_context_value(var_name, ctx) {
                        out.push_str(&val.to_string());
                        i = j + 1;
                        continue;
                    }
                }
            }
        }
        // Ordinary text: copy the run in a single slice push.  This used to push
        // `bytes[i] as char` one byte at a time, which (a) paid a bounds check
        // and a char encode per byte and (b) mangled any multi-byte UTF-8
        // character into its separate Latin-1 stand-ins ("é" came out as "Ã©").
        let run_start = i;
        while i < bytes.len() && bytes[i] != b'%' && bytes[i] != b'$' && bytes[i] != b'{' {
            i += 1;
        }
        if i > run_start {
            out.push_str(&s[run_start..i]);
        } else {
            // A lone '%', '$' or '{' that did not open a placeholder: keep it.
            // (These three are ASCII, so the byte is a char boundary.)
            out.push_str(&s[i..i + 1]);
            i += 1;
        }
    }
    out
}

fn to_i64_value(value: &Value) -> Result<i64, String> {
    match value {
        Value::Int(v) => Ok(*v),
        Value::Float(v) => Ok(*v as i64),
        Value::Bool(v) => Ok(if *v { 1 } else { 0 }),
        Value::StrShared(s) => match s.trim().parse::<i64>() {
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
        Value::StrShared(s) => match s.trim().parse::<f64>() {
            Ok(v) => Ok(v),
            Err(_) => Err(format!("Cannot convert string \"{}\" to float", s)),
        },
        _ => Err(format!("Cannot convert {} to float", value.type_name())),
    }
}

struct Runtime {
    module_dir: PathBuf,
    vars: Map<String, Value>,
    /// The program's functions, callables, classes and async-name set, shared
    /// copy-on-write.
    ///
    /// These are the tables a spawned task needs, and spawning used to *clone all
    /// four* into the new runtime: four hash maps rebuilt per task, sized by the
    /// program rather than by the task. Measured on this machine, a spawn+wait cost
    /// 26 µs with an empty table and 138 µs with 800 functions — 0.14 µs per entry,
    /// five times the cost of starting the thread, for tables the task mostly never
    /// reads.
    ///
    /// Sharing them behind `Arc` makes the hand-over four refcount bumps. Writes go
    /// through `Arc::make_mut`, so the first `fun`/`class` statement after a task
    /// exists copies the table once — and the task keeps the snapshot it was given,
    /// which is exactly what cloning gave it before.
    funcs: Arc<Map<String, Arc<FunctionDef>>>,
    callables: Arc<Map<String, Callable>>,
    modules: Map<String, Arc<ModuleInstance>>,
    classes: Arc<Map<String, ClassDef>>,
    module_cache: Arc<Mutex<HashMap<PathBuf, Arc<ModuleInstance>>>>,
    /// Canonical paths currently being loaded — used to detect circular imports
    /// (A imports B imports A) and report a clean error instead of recursing
    /// until the stack overflows.
    module_loading: Arc<Mutex<HashSet<PathBuf>>>,
    /// Parsed-expression cache. Every `Stmt` stores its expressions as *text*
    /// and `eval_expr` used to lex + parse that text on **every single
    /// evaluation** — so a loop body re-parsed its own source thousands of
    /// times a second. Parsing is context-free, so the AST is cached here and
    /// reused. Shared across module runtimes (like `module_cache`), so a
    /// std-library expression is parsed once per process.
    expr_cache: Arc<RwLock<Map<String, Arc<Expr>>>>,
    /// Merged metadata per class name — the field slots and the method table —
    /// shared by every instance of that class (see `Runtime::class_meta`). Each
    /// entry records the class generation it was built for, so a later `class`
    /// statement invalidates it.
    class_meta_cache: HashMap<String, (u64, Arc<ClassMeta>)>,
    /// Bumped by every `class` statement. `class_meta_cache` entries are only
    /// valid while this matches the value they were built with.
    class_gen: u64,
    /// Direct-mapped cache of recently evaluated expressions.
    ///
    /// `eval_expr` receives raw *text*, so on every evaluation it has to work
    /// out what that text is: a bare variable name, a literal, or something to
    /// parse — two scans, then the parse cache's lock and a hash of the text.
    /// A loop body repeats the same handful of texts, so caching the
    /// *classification* per text removes all of that from the inner loop.
    ///
    /// A previous version of this was one entry, justified by the idea that a
    /// bigger table "would cost more to search than the compare it saves". That
    /// had it backwards: a loop body with more than one statement *alternates*
    /// its expression texts, so the single entry was replaced before it could
    /// ever be reused and the memo missed on nearly every evaluation. Keying by
    /// text instead of by recency is what makes it work.
    ///
    /// It lives on the runtime rather than on `ExecContext` because it is keyed
    /// purely by text: the same text classifies the same way in every frame, and
    /// a context is created for every method call, every spawned task and every
    /// served HTTP request. Per-context, that was a fresh ~3 KB table of 32
    /// `Option<ExprMemoSlot>`s (each holding a `String`) allocated and dropped
    /// each time, and method bodies never got to reuse a single hit.
    expr_memo: Vec<Option<ExprMemoSlot>>,
    debugger: Option<Debugger>,
    async_funcs: Arc<HashSet<String>>,
}

struct Debugger {
    stepping: bool,
    breakpoints: HashSet<usize>,
    source_lines: Vec<String>,
}

impl Runtime {
    /// A temporary runtime for one method body, sharing the caller's caches from
    /// the start.
    ///
    /// This used to be `bare()`: it allocated three cache `Arc`s and a fresh
    /// 32-slot expression memo, and every caller then handed the runtime the
    /// caller's own handles through `lend_tables_to` — so all four allocations were
    /// discarded before the body ran, on every method call. The memo was the
    /// expensive one: 32 slots of `Option<ExprMemoSlot>`, each slot an enum plus a
    /// `String`.
    ///
    /// The handles now come from the caller up front, and the memo starts *empty*
    /// because the lender moves its own in. That is why `memo_get`/`memo_put`
    /// tolerate a short vector: until the lend happens there is no memo, and a
    /// missing memo only means the slow path runs.
    fn bare_sharing(module_dir: PathBuf, from: &Runtime) -> Self {
        Self {
            module_dir,
            vars: Map::default(),
            funcs: Arc::default(),
            callables: Arc::default(),
            modules: Map::default(),
            classes: Arc::default(),
            module_cache: Arc::clone(&from.module_cache),
            module_loading: Arc::clone(&from.module_loading),
            expr_cache: Arc::clone(&from.expr_cache),
            class_meta_cache: HashMap::new(),
            class_gen: 0,
            expr_memo: Vec::new(),
            debugger: None,
            async_funcs: Arc::default(),
        }
    }

    /// Hand a temporary runtime the tables and caches it must share with the
    /// caller, then give them back afterwards.
    ///
    /// Dispatch used to `clone()` the class, function, callable and module
    /// tables on **every** method call — O(program) per call, which dominates
    /// OOP-heavy code — and to give the temporary runtime a fresh, empty
    /// `expr_cache`, so every expression in a method body was re-parsed on every
    /// call. Moving the tables and sharing the caches makes both O(1).
    ///
    /// Nothing can observe the move: the body runs to completion before the
    /// tables are put back, `ctx` is not usable during that window, and a panic
    /// aborts the process rather than unwinding out of it.
    fn lend_tables_to(&mut self, temp: &mut Runtime) {
        temp.classes = std::mem::take(&mut self.classes);
        temp.funcs = std::mem::take(&mut self.funcs);
        temp.callables = std::mem::take(&mut self.callables);
        temp.modules = std::mem::take(&mut self.modules);
        // The class-method table cache goes along with the classes it describes,
        // so a method body that instantiates objects does not rebuild them.
        temp.class_meta_cache = std::mem::take(&mut self.class_meta_cache);
        temp.class_gen = self.class_gen;
        // The expression memo is text-keyed and frame-independent, so the method
        // body shares the caller's — its expressions then hit on the second call
        // instead of every call starting from an empty table.
        temp.expr_memo = std::mem::take(&mut self.expr_memo);
        temp.module_dir = self.module_dir.clone();
        temp.expr_cache = Arc::clone(&self.expr_cache);
        temp.module_cache = Arc::clone(&self.module_cache);
        temp.module_loading = Arc::clone(&self.module_loading);
    }

    fn reclaim_tables_from(&mut self, temp: Runtime) {
        self.classes = temp.classes;
        self.funcs = temp.funcs;
        self.callables = temp.callables;
        self.modules = temp.modules;
        self.class_meta_cache = temp.class_meta_cache;
        // A method body may define a class, which invalidates tables built before
        // it; taking the newer generation is the safe direction (a stale entry
        // just gets rebuilt, where a too-new one would be trusted).
        self.class_gen = temp.class_gen;
        self.expr_memo = temp.expr_memo;
    }

    /// Merged metadata for a class: its own fields and methods plus every
    /// ancestor's, compiled into slot order, child overriding parent. Shared by
    /// every instance of the class.
    ///
    /// This used to be rebuilt per *instance*, which meant a fresh `HashMap`, one
    /// `String` key per field and per method, and one `Arc` clone per method, for
    /// every object constructed. `class_gen` is bumped by every `class`
    /// statement, so redefining a class (or a parent, which children pick up
    /// through the merge) invalidates the cached entry exactly as rebuilding
    /// per instance did.
    fn class_meta(&mut self, class_def: &ClassDef) -> Arc<ClassMeta> {
        if let Some((generation, meta)) = self.class_meta_cache.get(&class_def.name) {
            if *generation == self.class_gen {
                return Arc::clone(meta);
            }
        }

        // Parent fields first, so a positional argument is its slot index.
        let mut field_names: Vec<String> = Vec::new();
        if let Some(ref parent_name) = class_def.parent {
            if let Some(parent_def) = self.classes.get(parent_name) {
                field_names = collect_field_names(parent_def, &self.classes);
            }
        }
        for field in &class_def.fields {
            // A child redeclaring a parent's field keeps the parent's slot: the
            // old code wrote both names into one map and let the later write win,
            // which shifted no positional argument either.
            if !field_names.iter().any(|n| n == &field.name) {
                field_names.push(field.name.clone());
            }
        }

        let mut field_index = Map::default();
        for (slot, name) in field_names.iter().enumerate() {
            field_index.insert(name.clone(), slot as u32);
        }

        let mut methods = HashMap::new();
        if let Some(ref parent_name) = class_def.parent {
            if let Some(parent_def) = self.classes.get(parent_name) {
                methods.extend(collect_all_methods(parent_def, &self.classes));
            }
        }
        methods.extend(class_def.methods.clone());

        let meta = Arc::new(ClassMeta {
            name: class_def.name.clone(),
            field_names,
            field_index,
            methods,
            is_dataclass: class_def.is_dataclass,
        });
        self.class_meta_cache
            .insert(class_def.name.clone(), (self.class_gen, Arc::clone(&meta)));
        meta
    }

    fn new(module_dir: PathBuf) -> Self {
        Self {
            module_dir,
            vars: default_color_vars(),
            funcs: Arc::default(),
            callables: Arc::default(),
            modules: Map::default(),
            classes: Arc::default(),
            module_cache: Arc::new(Mutex::new(HashMap::new())),
            module_loading: Arc::new(Mutex::new(HashSet::new())),
            expr_cache: Arc::new(RwLock::new(Map::default())),
            class_meta_cache: HashMap::new(),
            class_gen: 0,
            expr_memo: (0..EXPR_MEMO_SLOTS).map(|_| None).collect(),
            debugger: None,
            async_funcs: Arc::default(),
        }
    }

    fn with_cache(module_dir: PathBuf, cache: Arc<Mutex<HashMap<PathBuf, Arc<ModuleInstance>>>>) -> Self {
        Self {
            module_dir,
            vars: default_color_vars(),
            funcs: Arc::default(),
            callables: Arc::default(),
            modules: Map::default(),
            classes: Arc::default(),
            module_cache: cache,
            module_loading: Arc::new(Mutex::new(HashSet::new())),
            expr_cache: Arc::new(RwLock::new(Map::default())),
            class_meta_cache: HashMap::new(),
            class_gen: 0,
            expr_memo: (0..EXPR_MEMO_SLOTS).map(|_| None).collect(),
            debugger: None,
            async_funcs: Arc::default(),
        }
    }

    /// Like `with_cache`, but also shares the in-progress import set so that
    /// circular imports spanning more than one hop are detected.
    fn with_cache_and_loading(
        module_dir: PathBuf,
        cache: Arc<Mutex<HashMap<PathBuf, Arc<ModuleInstance>>>>,
        loading: Arc<Mutex<HashSet<PathBuf>>>,
        exprs: Arc<RwLock<Map<String, Arc<Expr>>>>,
    ) -> Self {
        Self {
            module_dir,
            vars: default_color_vars(),
            funcs: Arc::default(),
            callables: Arc::default(),
            modules: Map::default(),
            classes: Arc::default(),
            module_cache: cache,
            module_loading: loading,
            expr_cache: exprs,
            class_meta_cache: HashMap::new(),
            class_gen: 0,
            expr_memo: (0..EXPR_MEMO_SLOTS).map(|_| None).collect(),
            debugger: None,
            async_funcs: Arc::default(),
        }
    }

    /// Parse `expr` once and remember the AST. Parsing is context-free, so the
    /// result is valid for every later evaluation of the same text.
    fn cached_expr(&self, expr: &str) -> Result<Arc<Expr>, String> {
        // `RwLock`, not `Mutex`: this is a read-mostly cache and `spawn` /
        // `parallel` / `coop` run code on other threads, all sharing this one
        // map. With a `Mutex` every expression evaluation on every thread
        // serialised against every other thread, even though the lookup is a
        // pure read after the first parse.
        if let Some(hit) = self.expr_cache.read().unwrap().get(expr) {
            return Ok(hit.clone());
        }
        let parsed = Arc::new(parse_expr_ast(expr)?);
        let mut cache = self.expr_cache.write().unwrap();
        // Expression text is usually a fixed set taken from the program, but a
        // long-running service can build it at runtime (templates, filters), and
        // this map is shared for the life of the process. Past the cap we stop
        // adding new entries rather than evicting: evicting would let a program
        // near the cap re-parse everything on every pass, which is far worse than
        // a few uncached texts. Existing entries keep serving hits.
        if cache.len() < EXPR_CACHE_MAX {
            cache.insert(expr.to_string(), parsed.clone());
        }
        Ok(parsed)
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
        self.module_dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();

        // Fast path: load the pre-parsed program from `__incache__` so a re-run
        // skips reading, pre-processing, lexing and parsing entirely.
        if let Some(program) = load_cached_program(file) {
            let mut ctx = ExecContext::new(self);
            return exec_block(&program, &mut ctx).map(|_| ());
        }

        let source = fs::read_to_string(file)
            .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;
        let program = parse_source(&source)?;
        store_cached_program(file, &source, &program);
        let mut ctx = ExecContext::new(self);
        exec_block(&program, &mut ctx).map(|_| ())
    }

    fn run_source(&mut self, source: &str) -> Result<(), String> {
        let program = parse_source(source)?;
        let mut ctx = ExecContext::new(self);
        exec_block(&program, &mut ctx).map(|_| ())
    }
}

// =============================================================================
// On-disk AST cache — `__incache__/<module>.inc` (the `.inc` files)
// =============================================================================
//
// Indent's own `__pycache__`: the first time a file is run its parsed program is
// written next to it as `<name>.inc` inside a `__incache__/` directory, keyed by
// the source size, modification time and a hash of the source text — plus the
// build that wrote it (see `AST_CACHE_BUILD`). The next run deserialises that
// program instead of reading, pre-processing, lexing and parsing the source — a
// large module goes from milliseconds to microseconds.
//
// The cache is always optional: any failure to read, validate or write it simply
// falls back to parsing, and `INDENT_NO_AST_CACHE=1` disables it entirely. Cache
// files are never treated as source (every file enumerator only accepts
// `.ind`/`.ath`).

/// Bumped whenever the grammar, `Stmt`/`Expr` layout or the parse pipeline
/// changes, so old cache files are ignored instead of misread. (Python calls
/// this the magic number.)
///
/// 3: one-line `fun f p = <default>` signatures — a default is no longer read as a
///    single whitespace token, so `= "Not Found"` parses as one default instead
///    of producing a parameter named `Found"`. An AST cached by format 2 is not
///    merely stale, it is wrong, so it must not be reused even within a version.
/// `3` -> `4`: `Stmt::AssignSlice` and `Stmt::DefClass` became tuple variants
/// holding a `Box`, and `ArgItem::DefVar` gained a box. That changes the
/// serialised shape of those nodes, so a cache written by an older build of the
/// same version would deserialise into a *different* program rather than fail —
/// exactly the hazard the build check exists for, but the build string (2.4.0) is
/// unchanged here, so the format number is what has to move.
const AST_CACHE_FORMAT: u32 = 4;
const AST_CACHE_MAGIC: &[u8; 8] = b"INDENTC2";
const AST_CACHE_DIR: &str = "__incache__";

/// The interpreter build that wrote a cache file — the other half of the cache
/// key.
///
/// `bincode` is not self-describing, so an `Stmt`/`Expr` layout change can
/// deserialize **successfully into a wrong program** instead of failing. A cache
/// written by one build must therefore never be read by another. Keying on the
/// package version makes that automatic, rather than relying on somebody
/// remembering to bump `AST_CACHE_FORMAT` by hand for the release. The cost is
/// one re-parse of each source per upgrade, which is also what a first run does.
///
/// Deliberately *not* keyed on the binary's own mtime/size: that would also
/// separate two development builds of the same unreleased version, but it would
/// invalidate every cache on every rebuild (and on every reinstall of the same
/// release). During development on an unreleased version, bump
/// `AST_CACHE_FORMAT` after an AST change or run `--clear-cache`.
const AST_CACHE_BUILD: &str = env!("CARGO_PKG_VERSION");

/// Bytes in a cache header that do not depend on the length of the build string:
/// `magic | format | build length | size | mtime | hash | payload length`.
const AST_CACHE_HEADER_FIXED: usize = 8 + 4 + 2 + 8 + 8 + 8 + 8;

fn ast_cache_enabled() -> bool {
    if AST_CACHE_OFF.load(Ordering::Relaxed) {
        return false;
    }
    match env::var("INDENT_NO_AST_CACHE") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            v.is_empty() || v == "0" || v == "false" || v == "no"
        }
        Err(_) => true,
    }
}

/// Path of the cache file for a given source file: `<dir>/__incache__/<name>.inc`.
fn cache_path_for(file: &Path) -> Option<PathBuf> {
    let dir = file.parent()?;
    let name = file.file_name()?.to_string_lossy().to_string();
    Some(dir.join(AST_CACHE_DIR).join(format!("{name}.inc")))
}

/// FNV-1a. Chosen because it is fast (no SipHash overhead) and dependency-free;
/// this only guards against a stale cache, not against tampering.
fn source_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Parse a program from source text, sharing the `run_source` pipeline.
fn parse_source(source: &str) -> Result<Vec<Stmt>, String> {
    // Errors are propagated raw here and formatted ONCE by the top-level
    // handler (format_error_with_source), which re-reads the source file and
    // locates the line. Formatting earlier would double-render.
    let lines = preprocess(source)?;
    let mut parser = Parser::new(lines);
    parser.parse()
}

/// Try to load a pre-parsed program from the cache.
/// Returns `None` on any miss, mismatch or corruption.
fn load_cached_program(file: &Path) -> Option<Vec<Stmt>> {
    if !ast_cache_enabled() {
        return None;
    }
    let cache_path = cache_path_for(file)?;
    let source = fs::read(file).ok()?;
    let meta = fs::metadata(file).ok()?;
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let data = fs::read(&cache_path).ok()?;

    // Header: magic | format | build len | build | source size | mtime | hash | payload len
    if data.len() < AST_CACHE_HEADER_FIXED || &data[0..8] != AST_CACHE_MAGIC {
        return None;
    }
    let format = u32::from_le_bytes(data[8..12].try_into().ok()?);
    if format != AST_CACHE_FORMAT {
        return None;
    }
    let build_len = u16::from_le_bytes(data[12..14].try_into().ok()?) as usize;
    let build_end = 14 + build_len;
    // Written by a different Indent build: a miss, never a misread.
    if data.len() < build_end + 32 || &data[14..build_end] != AST_CACHE_BUILD.as_bytes() {
        return None;
    }
    let fixed = &data[build_end..];
    let size = u64::from_le_bytes(fixed[0..8].try_into().ok()?);
    let mtime = u64::from_le_bytes(fixed[8..16].try_into().ok()?);
    let hash = u64::from_le_bytes(fixed[16..24].try_into().ok()?);
    let payload_len = u64::from_le_bytes(fixed[24..32].try_into().ok()?);

    let header = build_end + 32;
    if size != source.len() as u64 || mtime != mtime_ns || hash != source_hash(&source) {
        return None;
    }
    if payload_len != (data.len() - header) as u64 {
        return None;
    }
    bincode::deserialize(&data[header..]).ok()
}

/// Write the parsed program to the cache. Every failure is ignored — the cache
/// is an optimisation, never a requirement (read-only installs, full disks, …).
fn store_cached_program(file: &Path, source: &str, program: &[Stmt]) {
    if !ast_cache_enabled() {
        return;
    }
    let Some(cache_path) = cache_path_for(file) else {
        return;
    };
    let Ok(payload) = bincode::serialize(program) else {
        return;
    };
    let Some(dir) = cache_path.parent() else { return };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let mtime_ns = fs::metadata(file)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    let mut out = Vec::with_capacity(AST_CACHE_HEADER_FIXED + AST_CACHE_BUILD.len() + payload.len());
    out.extend_from_slice(AST_CACHE_MAGIC);
    out.extend_from_slice(&AST_CACHE_FORMAT.to_le_bytes());
    out.extend_from_slice(&(AST_CACHE_BUILD.len() as u16).to_le_bytes());
    out.extend_from_slice(AST_CACHE_BUILD.as_bytes());
    out.extend_from_slice(&(source.len() as u64).to_le_bytes());
    out.extend_from_slice(&mtime_ns.to_le_bytes());
    out.extend_from_slice(&source_hash(source.as_bytes()).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&payload);

    // Write to a unique temp file and rename, so a concurrent reader never sees
    // a half-written cache and two writers cannot interleave.
    let tmp = cache_path.with_extension(format!("inc.tmp{}", std::process::id()));
    if fs::write(&tmp, &out).is_err() {
        let _ = fs::remove_file(&tmp);
        return;
    }
    if fs::rename(&tmp, &cache_path).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

/// Remove every `__incache__` directory under `dir` (used by `--no-cache`-style
/// housekeeping and `indent cache clear`). Returns how many files were removed.
fn clear_ast_cache(dir: &Path, removed: &mut usize) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some(AST_CACHE_DIR) {
                if let Ok(inner) = fs::read_dir(&path) {
                    for f in inner.flatten() {
                        if fs::remove_file(f.path()).is_ok() {
                            *removed += 1;
                        }
                    }
                }
                let _ = fs::remove_dir(&path);
            } else {
                clear_ast_cache(&path, removed);
            }
        }
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

    let name = error_code_to_name(code);
    // Display-only: collapse repeated "error[EXXX]: " prefixes (the runtime
    // sometimes nests them). The raw `err` is left untouched so `error_type` /
    // `error_message` still parse it correctly.
    let mut err_display = err.trim().to_string();
    loop {
        let t = err_display.trim_start();
        if let Some(rest) = t.strip_prefix("error[") {
            if let Some(end) = rest.find(']') {
                let after = rest[end + 1..].trim_start();
                err_display = after.trim_start_matches(": ").trim().to_string();
                continue;
            }
        }
        break;
    }

    let mut out = format!(
        "\x1b[90m── \x1b[0m\x1b[1;31m{}\x1b[0m\x1b[90m ──────────────────────\x1b[0m\n  {}\n",
        name, err_display
    );

    if let Some(ln) = line_no {
        if ln > 0 && ln <= lines.len() {
            let source_line = lines[ln - 1];
            let trimmed = source_line.trim_start();
            let indent = source_line.len() - trimmed.len();

            // Show the source line
            out.push_str(&format!("  \x1b[1;34m{}\x1b[0m {} \x1b[2m{}\x1b[0m\n", arrow_style_right(ln, lines.len()), lines[ln - 1], ""));

            // Underline with carets — point at the approximate error position
            // (first non-space token, or the middle for short lines).
            let caret_pos = if trimmed.is_empty() { 0 } else { (trimmed.len().min(2)).max(trimmed.len() / 2) };
            let spaces = " ".repeat(indent + caret_pos + 2); // +2 for the arrow column
            out.push_str(&format!("  \x1b[1;34m{}\x1b[0m \x1b[1;31m{}^\x1b[0m\n", arrow_style_empty(), spaces));

            // Show surrounding context
            let start = ln.saturating_sub(2).max(1);
            let end = (ln + 1).min(lines.len());
            if start < ln {
                out.push_str(&format!("  \x1b[1;34m{}\x1b[0m \x1b[90m(see lines {}-{})\x1b[0m\n", arrow_style_empty(), start, end));
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

    out.push_str("\x1b[90m────────────────────────────────────\x1b[0m\n");

    out
}

fn arrow_style_right(line: usize, total: usize) -> String {
    let digits = total.to_string().len();
    format!("\x1b[1;34m-->\x1b[0m {:>width$} |", line, width = digits)
}

fn arrow_style_empty() -> String {
    "   |".to_string()
}

/// One call frame: named locals plus the positional arguments of the call that
/// owns it.
///
/// The positional arguments are kept as a plain list and bound *lazily* — a read
/// of `argument` / `argument2` … resolves against this list instead of the frame
/// having to materialise a `String` key and a map entry for every parameter of
/// every call. Most functions never touch the aliases, so this removes several
/// allocations per call.
#[derive(Default, Clone)]
struct Frame {
    vars: Map<String, Value>,
    args: Vec<Value>,
}

/// `argument` → 0, `argument2` → 1, `argument3` → 2 … (the aliases the runtime
/// has always bound). Returns `None` for any other name.
fn positional_alias_index(name: &str) -> Option<usize> {
    let rest = name.strip_prefix("argument")?;
    if rest.is_empty() {
        return Some(0);
    }
    match rest.parse::<usize>() {
        Ok(n) if n >= 2 => Some(n - 1),
        _ => None,
    }
}

/// How `eval_expr` classified an expression the last time it saw that text.
#[derive(Debug, Clone)]
enum ExprKind {
    /// A bare identifier: the text names a variable. Only the *shape* is
    /// cached — the value is still read from the scope on every evaluation.
    Ident,
    /// A literal, whose value is fixed by the text, so it is stored ready-made.
    Literal(Value),
    /// Anything else, parsed once.
    Ast(Arc<Expr>),
}

#[derive(Debug, Clone)]
struct ExprMemoSlot {
    text: String,
    kind: ExprKind,
}

/// Upper bound on parsed expressions kept in `Runtime::expr_cache` (see
/// `Runtime::cached_expr`). Expression text is normally a fixed set taken from the
/// program, but a long-running service can generate it at runtime, so this keeps
/// that growth bounded rather than unbounded.
const EXPR_CACHE_MAX: usize = 20_000;

/// Slots in `Runtime::expr_memo`. A power of two so the index is a mask.
///
/// Measured: building this same code with the constant set to 1 (which restores
/// by-recency behaviour) and running the two binaries interleaved, best of 5,
/// `--no-cache`, gave 32 slots 5% faster on bench_hot_paths and 7% faster on
/// bench_calls (2% on bench_deep_calls). So keying by text really does beat
/// keeping the most recent expression, which is what the previous one-entry memo
/// did. 32 covers a loop body with a dozen or more distinct expressions; it costs
/// one small `Vec` of `Option`s per execution context.
const EXPR_MEMO_SLOTS: usize = 32;

/// Cheap index for the expression memo. Deliberately not a full hash: hashing
/// the whole text would cost about as much as the cache lookup saves. Length
/// plus the first and last byte separates the expressions that share a loop
/// body, and a collision is harmless — every hit is still verified by comparing
/// the text, so a collision only costs a missed hit.
#[inline]
fn expr_memo_index(expr: &str) -> usize {
    let bytes = expr.as_bytes();
    let mut h = bytes.len().wrapping_mul(31);
    if let Some(&first) = bytes.first() {
        h = h.wrapping_add(first as usize);
    }
    if let Some(&last) = bytes.last() {
        h = h.wrapping_add((last as usize).wrapping_mul(7));
    }
    h & (EXPR_MEMO_SLOTS - 1)
}

struct ExecContext<'a> {
    rt: &'a mut Runtime,
    frames: Vec<Frame>,
    /// Recycled call frames. A call binds its parameters into a fresh
    /// `HashMap` and collects its positional arguments into a fresh `Vec`, so
    /// every call with arguments allocated twice. Returning the frame to this
    /// pool instead (cleared) keeps the allocations for the next call. Purely
    /// an allocation optimisation: contents are always cleared before reuse, so
    /// a recycled frame can never leak a variable or a positional argument.
    frame_pool: Vec<Frame>,
    collect_yields: bool,
    yielded_values: Vec<Value>,
    /// The modules whose top level the running code belongs to, innermost last.
    ///
    /// Calling a function out of a module used to *inject* that module's whole
    /// symbol table into the caller's runtime for the duration of the call — one
    /// hash insert per exported name, then one lookup and restore per name
    /// afterwards. Measured with a module exporting 41 functions: **6 170 ns per
    /// module call against 326 ns for the same function defined locally**, 19x, and
    /// the cost grew with the module's size rather than with the call.
    ///
    /// A function now runs *in its module's scope*: lookups that miss the frames
    /// and the caller's globals consult these modules innermost-first. Same
    /// precedence as the injection it replaces — a module's own functions shadow
    /// the caller's, while an existing global variable still shadows a module
    /// variable — but nothing is copied, inserted or restored.
    module_scope: Vec<Arc<ModuleInstance>>,
}

impl<'a> ExecContext<'a> {
    /// Frames kept for reuse. Deep recursion is bounded so a pathological call
    /// depth does not retain an unbounded amount of memory.
    const FRAME_POOL_MAX: usize = 64;

    fn new(rt: &'a mut Runtime) -> Self {
        Self {
            rt,
            frames: vec![],
            frame_pool: vec![],
            collect_yields: false,
            yielded_values: vec![],
            module_scope: Vec::new(),
        }
    }

    /// Look up `expr` in the runtime's memo, returning a clone of its cached
    /// classification. A collision returns `None` (the text did not match),
    /// which just means the slow path runs.
    #[inline]
    fn memo_get(&self, expr: &str) -> Option<ExprKind> {
        // `get`, not indexing: a temporary runtime has no slots until the caller's
        // memo is lent to it, and a miss is always safe here.
        let slot = self.rt.expr_memo.get(expr_memo_index(expr))?.as_ref()?;
        if slot.text == expr {
            Some(slot.kind.clone())
        } else {
            None
        }
    }

    /// Remember how `expr` was classified, replacing whatever held its slot.
    #[inline]
    fn memo_put(&mut self, expr: &str, kind: ExprKind) {
        let Some(slot) = self.rt.expr_memo.get_mut(expr_memo_index(expr)) else {
            return;
        };
        match slot {
            Some(existing) if existing.text == expr => existing.kind = kind,
            other => {
                *other = Some(ExprMemoSlot {
                    text: expr.to_string(),
                    kind,
                })
            }
        }
    }

    fn push_frame(&mut self) {
        let mut frame = self.frame_pool.pop().unwrap_or_default();
        frame.vars.clear();
        frame.args.clear();
        self.frames.push(frame);
    }

    fn pop_frame(&mut self) {
        if let Some(mut frame) = self.frames.pop() {
            frame.vars.clear();
            frame.args.clear();
            // `clear()` keeps the bucket array, so a frame that once belonged to
            // a 200-local function would hold its slots for the life of the pool
            // (64 frames). Give the memory back when a frame is wildly oversized
            // for reuse — `shrink_to` rather than `shrink_to_fit` keeps a sensible
            // working size, so the next small function does not realloc straight
            // back into it. Measured cost of these two checks: below the noise
            // floor (call_2arg ratio 1.033 without them, 1.039 with them, on a
            // machine whose per-op noise is ~4%).
            if frame.vars.capacity() > 32 {
                frame.vars.shrink_to(32);
            }
            if frame.args.capacity() > 16 {
                frame.args.shrink_to(16);
            }
            if self.frame_pool.len() < Self::FRAME_POOL_MAX {
                self.frame_pool.push(frame);
            }
        }
    }

    fn define_local(&mut self, name: &str, value: Value) {
        if let Some(frame) = self.frames.last_mut() {
            match frame.vars.get_mut(name) {
                Some(slot) => *slot = value,
                None => {
                    frame.vars.insert(name.to_string(), value);
                }
            }
        } else {
            match self.rt.vars.get_mut(name) {
                Some(slot) => *slot = value,
                None => {
                    self.rt.vars.insert(name.to_string(), value);
                }
            }
        }
    }

    fn set_var(&mut self, name: &str, value: Value) {
        // Update in place when the variable already exists: rebuilding the key
        // String on every assignment (i.e. on every loop iteration) was pure
        // overhead — `i is i + 1` allocated a String every time.
        for frame in self.frames.iter_mut().rev() {
            if let Some(slot) = frame.vars.get_mut(name) {
                *slot = value;
                return;
            }
        }
        match self.rt.vars.get_mut(name) {
            Some(slot) => *slot = value,
            None => {
                self.rt.vars.insert(name.to_string(), value);
            }
        }
    }

    /// Does a variable exist without cloning its value?
    fn has_var(&self, name: &str) -> bool {
        for frame in self.frames.iter().rev() {
            if frame.vars.contains_key(name) {
                return true;
            }
        }
        self.rt.vars.contains_key(name) || self.module_var(name).is_some()
    }

    fn get_var(&self, name: &str) -> Option<Value> {
        // Only names that could be positional aliases pay for the extra check.
        let alias = positional_alias_index(name);
        for frame in self.frames.iter().rev() {
            if let Some(idx) = alias {
                if let Some(v) = frame.args.get(idx) {
                    return Some(v.clone());
                }
            }
            if let Some(v) = frame.vars.get(name) {
                return Some(v.clone());
            }
        }
        match self.rt.vars.get(name) {
            Some(v) => Some(v.clone()),
            // The modules the running code belongs to, innermost first. The
            // caller's own globals were already consulted, which is the precedence
            // the old injected-symbols scheme produced with its `contains_key` guard.
            None => self.module_var(name).cloned(),
        }
    }

    /// A module-scope variable, innermost module first. Values are *borrowed* from
    /// the module, never copied into the runtime.
    fn module_var(&self, name: &str) -> Option<&Value> {
        for module in self.module_scope.iter().rev() {
            if let Some(ModuleEntry::Value(v)) = module.entries.get(name) {
                return Some(v);
            }
        }
        None
    }

    /// Borrow a variable's value, for read-only fast paths that must not pay the
    /// O(n) clone `get_var` performs for containers.
    fn get_var_ref(&self, name: &str) -> Option<&Value> {
        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.vars.get(name) {
                return Some(v);
            }
        }
        self.rt.vars.get(name).or_else(|| self.module_var(name))
    }

    /// Remove and return a variable's value (allowing a move instead of a
    /// copy). Mirrors `set_var`'s scope search: innermost frame first.
    fn take_var(&mut self, name: &str) -> Option<Value> {
        for frame in self.frames.iter_mut().rev() {
            if let Some(v) = frame.vars.remove(name) {
                return Some(v);
            }
        }
        self.rt.vars.remove(name).or_else(|| self.module_var(name).cloned())
    }

    /// Move a variable's value out and leave `Empty` in its place, keeping the
    /// name where it was — unlike `take_var`, which drops the binding and would
    /// let a later `set_var` re-create it in an inner frame, changing which
    /// variable shadows which. This is the move half of the hand-over a compiled
    /// body does around user code: a slot holding a list costs the same as one
    /// holding an int instead of a deep copy per call.
    fn swap_var(&mut self, name: &str) -> Option<Value> {
        for frame in self.frames.iter_mut().rev() {
            if let Some(slot) = frame.vars.get_mut(name) {
                return Some(std::mem::replace(slot, Value::Empty));
            }
        }
        match self.rt.vars.get_mut(name) {
            Some(slot) => Some(std::mem::replace(slot, Value::Empty)),
            // A module-level variable that the caller does not have a copy of.
            // `std::mem::replace` has nothing to replace, so the value is taken by
            // clone — which is what the old scheme did too: the module's symbol was
            // *copied* into the caller's runtime for the duration of the call, so a
            // write to it never reached the module and did not survive the call.
            None => self.module_var(name).cloned(),
        }
    }

    fn get_module(&self, name: &str) -> Option<Arc<ModuleInstance>> {
        if let Some(m) = self.rt.modules.get(name) {
            return Some(m.clone());
        }
        // A bare `get mod` binds the module in `rt.modules` *and* as a
        // `Value::Module` variable. Only the variable survives when a module is
        // snapshotted into a ModuleInstance, and invoke_external_function
        // re-injects those vars into the caller's runtime. Without this
        // fallback, a function exported by one module cannot reach its own
        // `get`-ed imports when it is called from another file.
        match self.get_var(name) {
            Some(Value::Module(m)) => Some(m),
            _ => None,
        }
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
                // In a generator body, record the yielded value and keep
                // executing so every `yield` is collected. Otherwise propagate.
                if ctx.collect_yields {
                    ctx.yielded_values.push(v);
                    continue;
                }
                return Ok(Control::Yield(v));
            }
            _ => return Ok(control),
        }
    }
    Ok(Control::None)
}

/// Peephole optimisation for the documented list-building idiom:
///
/// ```indent
/// var l dynamic = []
/// repeat 1000
///     l is append l item      #! also `push`, and `l is extend l more`
/// ```
///
/// `append`/`extend`/`push` are *functional* builtins — they clone the list and
/// return a new one — which makes this loop copy the whole list on every
/// iteration, i.e. **O(n²)**. Measured: 20 000 appends took ~10 s.
///
/// When the assignment target is exactly the list being appended to, the list
/// can be *moved* out of the variable, mutated, and moved back. That computes
/// the same result as the copying path — arguments are passed by value, so no
/// other variable can be sharing this value — but costs O(1) instead of O(n).
///
/// Returns `None` when the statement is not that shape, in which case the
/// caller falls back to the normal evaluation path.
fn try_inplace_list_append(
    name: &str,
    value: &ValueSource,
    ctx: &mut ExecContext<'_>,
) -> Option<Result<Control, String>> {
    let (callee, args) = match value {
        ValueSource::Call { callee, args } => (callee.as_str(), args),
        _ => return None,
    };
    if !matches!(callee, "append" | "push" | "extend") {
        return None;
    }
    if args.len() != 2 {
        return None;
    }
    // Only a plain variable target can be moved out of the scope chain.
    if name.is_empty() || name.contains('.') || name.contains('[') {
        return None;
    }
    let first = match &args[0] {
        ArgItem::Positional(expr) => expr.trim(),
        _ => return None,
    };
    if first != name {
        return None;
    }
    let second = match &args[1] {
        ArgItem::Positional(expr) => expr,
        _ => return None,
    };

    // Evaluate the appended value FIRST: it may read `name` itself.
    let item = match eval_expr(second, ctx) {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };

    match ctx.take_var(name) {
        Some(Value::ListShared(mut list)) => {
            match item {
                Value::ListShared(extra) if callee == "extend" => list.extend(extra),
                other if callee == "extend" => {
                    return Some(Err(format!(
                        "extend expects a list, got {}",
                        other.type_name()
                    )))
                }
                other => list.push(other),
            }
            ctx.set_var(name, Value::ListShared(list));
            Some(Ok(Control::None))
        }
        // Not a list (or not a variable): reproduce the builtin's own error.
        Some(other) => Some(Err(format!(
            "{callee} expects a list, got {}",
            other.type_name()
        ))),
        // Undefined variable: let the normal path produce the usual error.
        None => None,
    }
}

/// `s is s + x` — the idiomatic way to build a string in a loop.
///
/// Without this, every step goes through `eval_binary`'s `(Str, y)` arm, which
/// is `format!("{}{}", s, y)`: a brand-new `String` holding everything
/// accumulated so far. Building an n-character string therefore copied O(n²)
/// bytes. Appending into the existing buffer makes it O(n).
///
/// The match is deliberately conservative, and anything it is not certain about
/// falls through to the normal path, which is always correct:
///   * only `name + <rest>` with a real separator follows the name, so a
///     variable called `s` is never matched against the expression `sum + 1`;
///   * only when the variable *already holds a string*, and that is checked
///     before the right-hand side is evaluated, so an expression with side
///     effects can never be evaluated twice.
fn try_inplace_string_concat(
    name: &str,
    value: &ValueSource,
    ctx: &mut ExecContext<'_>,
) -> Option<Result<Control, String>> {
    if name.is_empty() || name.contains('.') || name.contains('[') || !is_bare_identifier(name) {
        return None;
    }
    let text = match value {
        ValueSource::Expr(text) => text.as_str(),
        _ => return None,
    };
    let rhs_text = match text
        .strip_prefix(name)
        .and_then(|rest| rest.strip_prefix(" + "))
    {
        Some(rhs) if !rhs.is_empty() => rhs,
        _ => return None,
    };

    if !matches!(ctx.get_var_ref(name), Some(Value::StrShared(_))) {
        return None;
    }

    // The right-hand side may read `name` itself (`s is s + s`), so it has to be
    // evaluated while the old value is still in place.
    let rhs = match eval_expr(rhs_text, ctx) {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };

    match ctx.take_var(name) {
        Some(Value::StrShared(mut s)) => {
            use std::fmt::Write as _;
            // `write!` appends the `Display` form straight into the buffer, so
            // the output is byte-for-byte what `format!("{}{}", s, rhs)`
            // produced, without the intermediate allocation (and without
            // copying the accumulated prefix a second time).
            //
            // Writing *through* the `Arc` (`DerefMut` -> `make_mut`) is what keeps
            // this O(1) per step without rebuilding the buffer: the variable was
            // just taken out of its frame, so this is the only handle, `make_mut`
            // sees a refcount of 1 and hands back the same `String`. Taking the
            // bytes out with `into_string()` and wrapping them in a fresh `Arc`
            // instead allocated a header on every step. Writing into a `String`
            // cannot fail, so the result is ignored.
            let _ = write!(&mut *s, "{}", rhs);
            ctx.set_var(name, Value::StrShared(s));
            Some(Ok(Control::None))
        }
        // The right-hand side reassigned the variable to something that is not a
        // string. Put it back and let the normal path decide, exactly as if
        // this fast path had not been offered at all.
        Some(other) => {
            ctx.set_var(name, other);
            None
        }
        None => None,
    }
}

/// `xs is xs + ys` for lists and dicts — the same trap `s is s + x` had.
///
/// `eval_binary`'s `(List, List)` arm is `x.extend(y)` on a *clone* of the left
/// operand, so growing a list one element at a time (`xs is xs + [x]`, which
/// `benchmarks/bench_append_scaling.ind` measures as `build2`) copied everything
/// accumulated so far on every step: O(n²) to build an n-element list. Merging a
/// dict in a loop had the identical shape.
///
/// Like the string version this match is deliberately narrow — exactly
/// `name + <rest>` after a complete identifier, and only when the variable
/// already holds a list or a dict. Unlike the string version it cannot tell from
/// the left-hand side alone whether the *right* operand will be the matching
/// kind, so instead of returning `None` (which would make the caller evaluate
/// the right-hand side a second time, running its side effects twice) it hands
/// both operands it already holds to `eval_binary`. That keeps every other
/// combination — the `Object.add` dispatch, `list + string`, and the type error
/// — decided in exactly one place.
fn try_inplace_container_concat(
    name: &str,
    value: &ValueSource,
    ctx: &mut ExecContext<'_>,
) -> Option<Result<Control, String>> {
    if name.is_empty() || name.contains('.') || name.contains('[') || !is_bare_identifier(name) {
        return None;
    }
    let text = match value {
        ValueSource::Expr(text) => text.as_str(),
        _ => return None,
    };
    let rhs_text = match text
        .strip_prefix(name)
        .and_then(|rest| rest.strip_prefix(" + "))
    {
        Some(rhs) if !rhs.is_empty() => rhs,
        _ => return None,
    };
    // One lookup rejects the overwhelming majority of assignments here.
    if !matches!(ctx.get_var_ref(name), Some(Value::ListShared(_) | Value::DictShared(_))) {
        return None;
    }

    // The right-hand side may read `name` (`xs is xs + xs`), so it has to be
    // evaluated while the old value is still in place.
    let rhs = match eval_expr(rhs_text, ctx) {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };
    let lhs = ctx.take_var(name)?;

    let out = match (lhs, rhs) {
        (Value::ListShared(mut l), Value::ListShared(r)) => {
            l.extend(r);
            Value::ListShared(l)
        }
        (Value::DictShared(mut d), Value::DictShared(r)) => {
            d.extend(r.into_inner());
            Value::DictShared(d)
        }
        (lhs, rhs) => match eval_binary("+", lhs, rhs, ctx) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        },
    };
    ctx.set_var(name, out);
    Some(Ok(Control::None))
}

/// Append `item` to `list`, applying the same `append`/`push`/`extend` rule the
/// builtins use. Shared so the plain-variable and member-target fast paths
/// cannot drift apart.
fn append_items_into(list: &mut Vec<Value>, item: Value, callee: &str) -> Result<(), String> {
    match item {
        Value::ListShared(extra) if callee == "extend" => {
            list.extend(extra);
            Ok(())
        }
        other if callee == "extend" => {
            Err(format!("extend expects a list, got {}", other.type_name()))
        }
        other => {
            list.push(other);
            Ok(())
        }
    }
}

/// `holder.xs is append holder.xs v` — growing a list kept inside a dict or an
/// object (`holder.xs`, `self.items`, JSON-ish state).
///
/// The member form does not reach the plain-variable path above because its
/// matchers reject names containing '.', so every step deep-copied the root
/// container *and* the list inside it — twice on the read and once more on the
/// write. Measured `benchmarks/bench_member_append.ind`: 6967 → 50778 ns/step as
/// n went 500 → 4000, which is the same quadratic shape as `xs is xs + [x]`.
///
/// The root is *moved* out and put back rather than cloned, and the member is
/// moved out of it, mutated, and moved back. Every path below either rebuilds
/// the root or puts it back unchanged before returning, so the variable can
/// never be lost even if an assumption is wrong.
fn try_inplace_member_append(
    name: &str,
    value: &ValueSource,
    ctx: &mut ExecContext<'_>,
) -> Option<Result<Control, String>> {
    let (callee, args) = match value {
        ValueSource::Call { callee, args } => (callee.as_str(), args),
        _ => return None,
    };
    if !matches!(callee, "append" | "push" | "extend") || args.len() != 2 {
        return None;
    }
    // Exactly `root.field`, both plain identifiers.
    let (root, field) = match name.split_once('.') {
        Some((r, f)) if !f.contains('.') && is_bare_identifier(r) && is_bare_identifier(f) => (r, f),
        _ => return None,
    };
    match &args[0] {
        ArgItem::Positional(expr) if expr.trim() == name => {}
        _ => return None,
    }
    let item_expr = match &args[1] {
        ArgItem::Positional(expr) => expr,
        _ => return None,
    };

    // Confirm the member really is a list before evaluating the value, so an
    // expression with side effects can never be evaluated twice.
    let holds_list = match ctx.get_var_ref(root) {
        Some(Value::DictShared(m)) => matches!(m.get(field), Some(Value::ListShared(_))),
        Some(Value::Object { class, fields }) => class
            .field_index
            .get(field)
            .is_some_and(|slot| matches!(fields[*slot as usize], Value::ListShared(_))),
        _ => false,
    };
    if !holds_list {
        return None;
    }

    let item = match eval_expr(item_expr, ctx) {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };
    let root_val = ctx.take_var(root)?;

    let updated = match root_val {
        Value::DictShared(mut m) => {
            let mut list = match m.remove(field) {
                Some(Value::ListShared(l)) => l,
                Some(other) => {
                    m.insert(field.to_string(), other);
                    ctx.set_var(root, Value::DictShared(m));
                    return None;
                }
                None => {
                    ctx.set_var(root, Value::DictShared(m));
                    return None;
                }
            };
            if let Err(e) = append_items_into(&mut list, item, callee) {
                m.insert(field.to_string(), Value::ListShared(list));
                ctx.set_var(root, Value::DictShared(m));
                return Some(Err(e));
            }
            m.insert(field.to_string(), Value::ListShared(list));
            Value::DictShared(m)
        }
        Value::Object { class, mut fields } => {
            // The slot is where the list already is, so this appends into the
            // instance's own value: no remove-and-put-back, and nothing to
            // restore when the item is rejected.
            let slot = match class.field_index.get(field) {
                Some(slot) => *slot as usize,
                None => {
                    ctx.set_var(root, Value::Object { class, fields });
                    return None;
                }
            };
            if let Value::ListShared(list) = &mut fields[slot] {
                if let Err(e) = append_items_into(list, item, callee) {
                    ctx.set_var(root, Value::Object { class, fields });
                    return Some(Err(e));
                }
            }
            Value::Object { class, fields }
        }
        other => {
            ctx.set_var(root, other);
            return None;
        }
    };
    ctx.set_var(root, updated);
    Some(Ok(Control::None))
}

/// `holder.xs is holder.xs + ys` — the concat spelling of the member case above,
/// and equally quadratic (measured 4174 → 37228 ns/step over n=500 → 4000).
///
/// Only list-plus-list is done in place, because that is the arm of `eval_binary`
/// whose cost is a copy of the left operand (`x.extend(y)`). The right-hand side
/// is evaluated first and the container is only taken out of the variable once it
/// is known to be a list, so the "not a list" case needs no restoration and can
/// simply fall through to the normal path.
fn try_inplace_member_concat(
    name: &str,
    value: &ValueSource,
    ctx: &mut ExecContext<'_>,
) -> Option<Result<Control, String>> {
    let (root, field) = match name.split_once('.') {
        Some((r, f)) if !f.contains('.') && is_bare_identifier(r) && is_bare_identifier(f) => (r, f),
        _ => return None,
    };
    let text = match value {
        ValueSource::Expr(text) => text.as_str(),
        _ => return None,
    };
    let rhs_text = match text
        .strip_prefix(name)
        .and_then(|rest| rest.strip_prefix(" + "))
    {
        Some(rhs) if !rhs.is_empty() => rhs,
        _ => return None,
    };
    let holds_list = match ctx.get_var_ref(root) {
        Some(Value::DictShared(m)) => matches!(m.get(field), Some(Value::ListShared(_))),
        Some(Value::Object { class, fields }) => class
            .field_index
            .get(field)
            .is_some_and(|slot| matches!(fields[*slot as usize], Value::ListShared(_))),
        _ => false,
    };
    if !holds_list {
        return None;
    }

    // Evaluate before taking anything out. If this is not list+list the ordinary
    // path handles it (and the right-hand side is evaluated a second time only in
    // that case, which `try_inplace_string_concat` already documents).
    let rhs = match eval_expr(rhs_text, ctx) {
        Ok(v) => v,
        Err(e) => return Some(Err(e)),
    };
    let extra = match rhs {
        Value::ListShared(r) => r,
        _ => return None,
    };

    let root_val = ctx.take_var(root)?;
    let updated = match root_val {
        Value::DictShared(mut m) => {
            let mut list = match m.remove(field) {
                Some(Value::ListShared(l)) => l,
                Some(other) => {
                    m.insert(field.to_string(), other);
                    ctx.set_var(root, Value::DictShared(m));
                    return None;
                }
                None => {
                    ctx.set_var(root, Value::DictShared(m));
                    return None;
                }
            };
            list.extend(extra);
            m.insert(field.to_string(), Value::ListShared(list));
            Value::DictShared(m)
        }
        Value::Object { class, mut fields } => {
            let slot = match class.field_index.get(field) {
                Some(slot) => *slot as usize,
                None => {
                    ctx.set_var(root, Value::Object { class, fields });
                    return None;
                }
            };
            if let Value::ListShared(list) = &mut fields[slot] {
                list.extend(extra);
            }
            Value::Object { class, fields }
        }
        other => {
            ctx.set_var(root, other);
            return None;
        }
    };
    ctx.set_var(root, updated);
    Some(Ok(Control::None))
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
    // `stmt_line` is a match over every `Stmt` variant, and `exec_stmt` is the
    // hottest function in the interpreter — running it unconditionally charged
    // every statement of every program for a line number that only the debugger
    // ever reads. `maybe_debug` returns immediately when no debugger is
    // attached, so skipping the call entirely is exactly equivalent.
    if ctx.rt.debugger.is_some() {
        maybe_debug(stmt_line(stmt), ctx)?;
    }

    match stmt {
        Stmt::Say { expr, line } => {
            let mut v = eval_expr(expr, ctx).map_err(|e| format!("Line {}: {}", line, e))?;
            // Object `to_string` special method (natural name) — use it when printing.
            if let Some(r) = call_object_method_value(&v, "to_string", &[], ctx) {
                v = r?;
            }
            let interpolated = match v {
                Value::StrShared(ref s) => Value::Str(interpolate_string(s, ctx)),
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
        Stmt::DefClass(data) => {
            let DefClassStmt {
                name,
                parent,
                fields,
                methods,
                is_dataclass,
                ..
            } = &**data;
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
                        Arc::new(FunctionDef {
                            params: params.clone(),
                            return_type: None,
                            body: body.clone(),
                            is_generator: stmts_contain_yield(body),
                            compiled: OnceLock::new(),
                        }),
                    );
                }
            }
            Arc::make_mut(&mut ctx.rt.classes).insert(
                name.clone(),
                ClassDef {
                    name: name.clone(),
                    parent: parent.clone(),
                    fields: fields.clone(),
                    methods: method_map,
                    is_dataclass: *is_dataclass,
                },
            );
            // Invalidate the per-class method-table cache: an existing class may
            // have been redefined, or a child may now see this as its parent.
            ctx.rt.class_gen += 1;
            Ok(Control::None)
        }
        Stmt::Assign { name, value, .. } => {
            if let Some(result) = try_inplace_list_append(name, value, ctx) {
                return result;
            }
            if let Some(result) = try_inplace_member_append(name, value, ctx) {
                return result;
            }
            if let Some(result) = try_inplace_member_concat(name, value, ctx) {
                return result;
            }
            if let Some(result) = try_inplace_string_concat(name, value, ctx) {
                return result;
            }
            if let Some(result) = try_inplace_container_concat(name, value, ctx) {
                return result;
            }
            let v = eval_value_source(value, ctx)?;
            assign_var_chain(name, v, ctx)?;
            Ok(Control::None)
        }
        Stmt::AssignOp { line, name, op, value } => {
            // `xs += [x]` had the same quadratic shape as `xs is xs + [x]`: the
            // old path called `get_var` (cloning the whole list) and then
            // `eval_binary`, whose `(List, List)` arm extended *another* copy,
            // so two O(n) copies happened per step. Decide the shape before
            // evaluating the right-hand side so it is still evaluated once.
            let in_place_container = op == "+="
                && !name.contains('.')
                && is_bare_identifier(name)
                && matches!(ctx.get_var_ref(name), Some(Value::ListShared(_) | Value::DictShared(_)));
            let rhs = eval_value_source(value, ctx)?;
            if in_place_container {
                if let Some(container) = ctx.take_var(name) {
                    let out = match (container, rhs) {
                        (Value::ListShared(mut l), Value::ListShared(r)) => {
                            l.extend(r);
                            Value::ListShared(l)
                        }
                        (Value::DictShared(mut d), Value::DictShared(r)) => {
                            d.extend(r.into_inner());
                            Value::DictShared(d)
                        }
                        (container, rhs) => eval_binary("+", container, rhs, ctx)
                            .map_err(|e| format!("Line {}: {}", line, e))?,
                    };
                    ctx.set_var(name, out);
                    return Ok(Control::None);
                }
            }
            let lhs = ctx.get_var(name)
                .ok_or_else(|| format!("Line {}: variable '{}' is not defined", line, name))?;
            let result = eval_binary(&op[..op.len()-1], lhs, rhs, ctx)
                .map_err(|e| format!("Line {}: {}", line, e))?;
            assign_var_chain(name, result, ctx).map_err(|e| format!("Line {}: {}", line, e))?;
            Ok(Control::None)
        }
        Stmt::AssignIndex {
            line,
            name,
            index_expr,
            value,
        } => {
            let replacement = eval_value_source(value, ctx)?;

            // Fast path: `d["k"] is v` where `d` is a plain variable. Moving the
            // container out of the variable and back avoids cloning it, which
            // made repeatedly filling a dict (or list) O(n²) — same class of
            // problem as the old `append` copy.
            if is_bare_identifier(name) {
                if !ctx.has_var(name) {
                    return Err(format!("Line {}: variable '{}' is not defined", line, name));
                }
                let index = eval_expr(index_expr, ctx)?;
                if let Some(target) = ctx.take_var(name) {
                    let updated = assign_index_value(target, index, replacement)
                        .map_err(|e| format!("Line {}: {}", line, e))?;
                    ctx.set_var(name, updated);
                    return Ok(Control::None);
                }
                // Should be unreachable (checked above); fall through defensively.
                return Err(format!("Line {}: variable '{}' is not defined", line, name));
            }

            let target = ctx
                .get_var(name)
                .ok_or_else(|| format!("Line {}: variable '{}' is not defined", line, name))?;
            let index = eval_expr(index_expr, ctx)?;
            let updated = assign_index_value(target, index, replacement)?;
            ctx.set_var(name, updated);
            Ok(Control::None)
        }
        Stmt::AssignSlice(data) => {
            let AssignSliceStmt {
                line,
                name,
                start_expr,
                end_expr,
                step_expr,
                value,
            } = &**data;
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
            let f = Arc::new(FunctionDef {
                params: params.clone(),
                return_type: return_type.clone(),
                body: body.clone(),
                is_generator: *is_generator,
                compiled: OnceLock::new(),
            });
            Arc::make_mut(&mut ctx.rt.funcs).insert(name.clone(), f.clone());
            Arc::make_mut(&mut ctx.rt.callables).insert(name.clone(), Callable::Local(f));
            if *is_async {
                Arc::make_mut(&mut ctx.rt.async_funcs).insert(name.clone());
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
        Stmt::Launch { expr, line } => {
            // launch <path> — run another Indent file in this runtime.
            let path_val = eval_expr(expr, ctx)?;
            let raw = path_val.to_string();
            let mut path = PathBuf::from(&raw);
            // A relative launch path is resolved against the *module's* own
            // directory first (so a package can launch a sibling file), then
            // through the regular module search path. This makes packages
            // self-contained instead of depending on the process cwd.
            let module_dir = ctx.rt.module_dir.clone();
            if !path.is_absolute() && !path.exists() {
                let sibling = module_dir.join(&path);
                if sibling.exists() {
                    path = sibling;
                } else {
                    let as_module = raw
                        .trim_end_matches(".ind")
                        .trim_end_matches(".glo")
                        .replace('/', ".");
                    if let Some(found) = find_module_file(&module_dir, &as_module) {
                        path = found;
                    }
                }
            }
            ctx.rt
                .run_file(&path)
                .map_err(|e| format!("Line {line}: launch: {e}"))?;
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
        // These two carry their payload in a box, so they cannot join the `|`
        // chain below, which binds `line` out of each pattern directly.
        Stmt::DefClass(d) => d.line,
        Stmt::AssignSlice(d) => d.line,
        Stmt::Say { line, .. }
        | Stmt::DefVar { line, .. }
        | Stmt::MakeType { line, .. }
        | Stmt::Assign { line, .. }
        | Stmt::AssignOp { line, .. }
        | Stmt::AssignIndex { line, .. }
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
        | Stmt::Launch { line, .. }
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

/// Runs a loop with ONE frame for the whole loop rather than one per iteration.
///
/// `run_loop_iteration` used to push and pop a frame per iteration, which meant
/// re-inserting the `Reps` and item-variable keys every time round: two
/// `String` allocations, two hash inserts and their deallocations on every
/// iteration of every loop in every program, plus a frame push/pop. Binding
/// them once for the loop's duration lets `define_local` update those entries in
/// place, because the keys stay alive.
///
/// Loop bodies are still scoped — the frame is popped when the loop exits, on
/// every path including an error or an early `give` — so a `var` declared
/// inside a loop still cannot be seen after it. The one observable difference is
/// that such a variable now survives from one iteration to the next within the
/// same loop, which is the behaviour most languages have anyway.
// ── Stage 1 bytecode VM: `repeat N` loop bodies ─────────────
//
// The tree-walker's loop cost is not the recursion. It is the per-statement text
// path: every statement goes through `exec_stmt`'s match, and every expression
// through `eval_expr`'s text classification, the memo compare and `eval_ast`. A
// probe that added an expression VM *on top of* that path measured 8.7-12.6%
// SLOWER, because the text path still ran underneath it.
//
// So the VM owns the whole body here: a `Count` loop compiles its body once into
// a flat instruction array, and one loop executes it. Nothing per-statement
// remains. The compiler refuses anything this stage does not implement — control
// flow, calls, index/member access, `say`, typed declarations, interpolated
// strings — and `vm_compile_body` returning false puts the whole loop back on the
// tree-walker, unchanged.
/// Stage 1 is on by default; `INDENT_VM=0` turns it off.
///
/// Enabled because it is strictly better on the macro battery (geomean 0.937,
/// nothing worse) and differentially correct: all 58 programs in the suite give
/// identical results with it on and off. The compiler's refusals keep the exposed
/// surface narrow — a body it cannot express falls back to the tree-walker,
/// unchanged — so `INDENT_VM=0` is the escape hatch if any program behaves
/// differently.
static INDENT_VM: LazyLock<bool> =
    LazyLock::new(|| std::env::var("INDENT_VM").map(|v| v != "0").unwrap_or(true));

#[inline]
fn vm_enabled() -> bool {
    *INDENT_VM
}

/// Maximum operand-stack depth. Deeper bodies stay in the tree-walker.
const VM_STACK: usize = 16;

/// A binary operator, resolved once when the body is compiled.
///
/// The VM used to carry the operator as a `String` per instruction and hand it to
/// `eval_binary`, which opened with a match over `&str` literals — a length
/// compare plus a `memcmp` for each arm it rejects, on every `<`, `+` and `==` a
/// compiled loop evaluates. The set of operators is fixed by the parser, so the
/// name can be turned into a discriminant at compile time (a body that somehow
/// carries an unknown operator is simply not compiled) and the run-time decision
/// is a jump table.
///
/// `eval_binary` stays the single source of truth for *what an operator means*:
/// `as_str` gives the name back, so anything the fast path below does not cover —
/// mixed types, string and container arms, `Object` operator methods, every error
/// message — is decided in exactly one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    NotIn,
    Is,
    IsNot,
    And,
    Or,
    BitOr,
    BitAnd,
    BitXor,
    Shl,
    Shr,
}

impl BinaryOp {
    /// The operator a name denotes. `None` for anything the parser cannot emit,
    /// which is what keeps a hand-written or future operator on the tree-walker.
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "+" => BinaryOp::Add,
            "-" => BinaryOp::Sub,
            "*" => BinaryOp::Mul,
            "/" => BinaryOp::Div,
            "%" => BinaryOp::Rem,
            "**" => BinaryOp::Pow,
            "==" => BinaryOp::Eq,
            "!=" => BinaryOp::Ne,
            "<" => BinaryOp::Lt,
            "<=" => BinaryOp::Le,
            ">" => BinaryOp::Gt,
            ">=" => BinaryOp::Ge,
            "in" => BinaryOp::In,
            "not in" => BinaryOp::NotIn,
            "is" => BinaryOp::Is,
            "is not" => BinaryOp::IsNot,
            "and" => BinaryOp::And,
            "or" => BinaryOp::Or,
            "|" => BinaryOp::BitOr,
            "&" => BinaryOp::BitAnd,
            "^" => BinaryOp::BitXor,
            "<<" => BinaryOp::Shl,
            ">>" => BinaryOp::Shr,
            _ => return None,
        })
    }

    fn as_str(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::Pow => "**",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::In => "in",
            BinaryOp::NotIn => "not in",
            BinaryOp::Is => "is",
            BinaryOp::IsNot => "is not",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
            BinaryOp::BitOr => "|",
            BinaryOp::BitAnd => "&",
            BinaryOp::BitXor => "^",
            BinaryOp::Shl => "<<",
            BinaryOp::Shr => ">>",
        }
    }

    /// The answer for two integer operands, when it is exactly what `eval_binary`
    /// would return from integer arithmetic alone.
    ///
    /// `None` means "ask `eval_binary`": either the operation is not one of these,
    /// or the operands are outside the range where the integer and floating-point
    /// paths agree — `7 / 2` must stay `3.5`, `1 / 0` must stay infinity, and a
    /// product past `2^26` must go through the float path that `eval_binary` uses
    /// for it. Every guard here mirrors a guard in `eval_binary`; the two are
    /// tested against each other by the suite, which runs in both engines.
    ///
    /// `inline(always)` because it is called from the two hottest instruction arms
    /// (`Bin` and `AssignOpSlot`), and left to its own judgement the compiler
    /// stopped inlining it once there were two call sites — which cost every
    /// arithmetic section about 7% (measured, best of fifteen) for a call that the
    /// whole point of this function is to avoid.
    #[inline(always)]
    fn int_fast(self, x: i64, y: i64) -> Option<Value> {
        let exact = x.unsigned_abs() <= EXACT_INT_MAG && y.unsigned_abs() <= EXACT_INT_MAG;
        match self {
            BinaryOp::Add => Some(Value::Int(x.wrapping_add(y))),
            BinaryOp::Sub if exact => Some(Value::Int(x.wrapping_sub(y))),
            BinaryOp::Mul
                if x.unsigned_abs() <= EXACT_INT_MUL_MAG && y.unsigned_abs() <= EXACT_INT_MUL_MAG =>
            {
                Some(Value::Int(x.wrapping_mul(y)))
            }
            // Only an exact division is an int; `7 / 2` and `1 / 0` belong to the
            // float path.
            BinaryOp::Div if exact && y != 0 && x % y == 0 => Some(Value::Int(x / y)),
            // A zero divisor is NaN on the float path, not an error, so it falls
            // through like the rest.
            BinaryOp::Rem if exact && y != 0 => Some(Value::Int(x % y)),
            BinaryOp::Eq => Some(Value::Bool(x == y)),
            BinaryOp::Ne => Some(Value::Bool(x != y)),
            BinaryOp::Lt if exact => Some(Value::Bool(x < y)),
            BinaryOp::Le if exact => Some(Value::Bool(x <= y)),
            BinaryOp::Gt if exact => Some(Value::Bool(x > y)),
            BinaryOp::Ge if exact => Some(Value::Bool(x >= y)),
            // Bitwise operators are integer-only in `eval_binary` too, so these
            // are the whole arm rather than a fast path into it.
            BinaryOp::BitOr => Some(Value::Int(x | y)),
            BinaryOp::BitAnd => Some(Value::Int(x & y)),
            BinaryOp::BitXor => Some(Value::Int(x ^ y)),
            // A shift count outside the type is a panic in Rust and a plain wrong
            // answer after wrapping, so only sane counts take this path.
            BinaryOp::Shl if (0..64).contains(&y) => Some(Value::Int(x << y)),
            BinaryOp::Shr if (0..64).contains(&y) => Some(Value::Int(x >> y)),
            _ => None,
        }
    }
}

/// A unary operator, resolved once when the body is compiled. Same reasoning as
/// `BinaryOp`; `apply_unary` still decides what an operator means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnaryOp {
    Neg,
    Pos,
    Not,
    BitNot,
}

impl UnaryOp {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "-" => UnaryOp::Neg,
            "+" => UnaryOp::Pos,
            "not" => UnaryOp::Not,
            "~" => UnaryOp::BitNot,
            _ => return None,
        })
    }

    fn as_str(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Pos => "+",
            UnaryOp::Not => "not",
            UnaryOp::BitNot => "~",
        }
    }
}

/// One instruction of a compiled program.
#[derive(Debug, Clone)]
enum VmInstr {
    /// Push a value fixed at compile time.
    Const(Value),
    /// Push the value in frame slot `n`.
    LoadSlot(u16),
    /// Pop a value into frame slot `n`.
    StoreSlot(u16),
    /// Pop two operands and push `eval_binary(op, a, b)`.
    Bin(BinaryOp),
    /// Pop and discard an expression statement's result.
    Pop,
    /// Pop a condition and jump to the given instruction when it is falsy.
    JumpIfFalse(usize),
    /// Pop a condition and jump to the given instruction when it is truthy.
    JumpIfTrue(usize),
    /// Jump to the given instruction.
    Jump(usize),
    /// Error out when a loop counter has passed the tree-walker's safety limit.
    CheckLimit { slot: u16, msg: String },
    /// Add one to a loop counter.
    BumpSlot(u16),
    /// Push a frame for a nested loop's own bindings.
    PushFrame,
    /// Pop it again, on the loop's every exit path.
    PopFrame,
    /// Define `name` in the current frame with the value in `slot`.
    SetLocal { slot: u16, name: String },
    /// Pop a `repeat` count, check it is a number, and store it as an integer.
    CheckCount { slot: u16 },
    /// Pop an iterable, materialise it for a nested `for`, or fail with `msg`.
    /// `generators` is what the source accepts: a `repeat for` takes a generator,
    /// a comprehension does not, and the two report it with different messages.
    IterPrepare {
        items: u16,
        index: u16,
        msg: String,
        generators: bool,
    },
    /// Pop the value a `give` produced and leave the stream with it. The value goes
    /// into `value` and `flag` records that a `give` ran, so the runner can tell a
    /// returned `empty` from a body that fell off its end.
    Return { value: u16, flag: u16 },
    /// Run a `do:` block: its body, then — when that failed — its `catch` body with
    /// the message bound. Both bodies are sub-programs of `VmProgram::subs`.
    TryBlock {
        do_sub: u16,
        catch_sub: u16,
        bind_slot: Option<u16>,
        bind_name: String,
    },
    /// The end of a function body, reached only when no `give` ran.
    EndBody { declared: bool },
    /// Pop `argc` arguments, call `callee` with them, push the result.
    Call { callee: String, argc: u16 },
    /// Fetch the next item of a materialised iterable: bind it, advance the
    /// position, publish the index as `Reps`, or jump to `end` when exhausted.
    IterNext {
        items: u16,
        index: u16,
        item: u16,
        reps: u16,
        end: usize,
    },
    /// Pop a value and append it to frame slot `slot` in place, under `callee`'s
    /// rule (`append`/`push`/`extend`.
    AppendSlot { slot: u16, callee: String },
    /// Pop a value and concatenate it onto frame slot `slot` in place.
    ConcatSlot(u16),
    /// Pop a value and combine it into frame slot `slot`: `x += value`.
    ///
    /// `Stmt::AssignOp` — `i += 1`, `total *= 2`, `s += chunk` — is one of the
    /// commonest statements there is, and it was not one the compiler accepted, so a
    /// body containing it went back to the tree-walker in its entirety: a `while`
    /// loop whose counter is bumped with `+=` got no compiled form at all, however
    /// simple the rest of it was. That was the reason `bench_string_ops` and
    /// `bench_string_build` never appeared to benefit from any VM work.
    ///
    /// It is its own instruction rather than `LoadSlot` + `Bin` + `StoreSlot` for two
    /// reasons: the operand order (`Bin` pops the left operand first, but the value
    /// is what the source evaluates first) and, for `+=`, the same in-place growth
    /// `ConcatSlot` does — `s += x` in a loop must not copy the accumulated string
    /// per iteration.
    ///
    /// `line` is carried so a failure reports exactly what `exec_stmt` reports.
    AssignOpSlot {
        slot: u16,
        op: BinaryOp,
        line: usize,
    },
    /// Pop one operand and push the result of a unary operator.
    Unary(UnaryOp),
    /// Pop one value and push its truth as a boolean.
    ToBool,
    /// Peek at the top of the stack: jump when it is falsy, leaving it there, so
    /// `and` can keep its left operand until the right one is known.
    JumpIfFalseKeep(usize),
    /// The same for a truthy condition, which is what `or` needs.
    JumpIfTrueKeep(usize),
    /// Pop `n` values and push them as a list, in order.
    MakeList(u16),
    /// Pop `2n` values (key, value, key, value) and push them as a dictionary.
    MakeDict(u16),
    /// Push a literal that contains `%`, interpolated.
    Interpolate(String),
    /// Pop an index and a container, push the element.
    Index,
    /// Pop only the index and read the element out of frame slot `slot`.
    ///
    /// The container is borrowed rather than copied, which is the difference
    /// between O(1) and O(n) per read: `xs[i]` in a loop used to clone the whole
    /// list to hand back one element.
    IndexSlot(u16),
    /// Pop step, end, start and container, and push the slice.
    Slice,
    /// Pop only step, end and start, and slice frame slot `slot` in place.
    SliceSlot(u16),
    /// Pop a container and push one of its members.
    MemberGet(String),
    /// Push a member of frame slot `slot`, without copying the container.
    MemberSlot { slot: u16, field: String },
    /// Pop `argc` arguments and a receiver, and push the method's result.
    MethodCall { method: String, argc: u16 },
    /// The same, but the receiver is a frame slot's variable: the instance is
    /// moved out of it, run against, and put back, so a method that assigns a
    /// field reaches the instance the variable holds.
    PlaceMethodCall { slot: u16, method: String, argc: u16 },
    /// Pop a value and an index and store it in frame slot `slot`, in place:
    /// `xs[i] is v` and `d["k"] is v`.
    IndexStoreSlot { slot: u16 },
    /// Pop a value, coerce it to the declared type, and make it frame slot
    /// `slot`: `var x ty = value` inside a body.
    DefVarSlot {
        slot: u16,
        line: usize,
        name: String,
        ty: String,
    },
    /// `stop`: leave the innermost compiled loop.
    ///
    /// A loop whose stream carries its own jumps is left by jumping to `target`.
    /// A `repeat N` body is not: the runner runs the stream once per pass, so a
    /// jump out of it would only end *that* pass and the next one would start
    /// anyway. The flag is raised for the runner to read instead, and the jump
    /// still happens so the rest of this pass is skipped.
    StopLoop { flag: u16, target: usize },
    /// `next`: jump to `target`, the innermost loop's continue point — the counter
    /// bump for `repeat N` and `while`/`until`, the `IterNext` for a `for`.
    NextLoop { target: usize },
    /// `reset`: put the innermost loop's counter back to zero and jump to
    /// `target`, its start, so the loop begins again without a bump.
    ResetLoop { slot: u16, target: usize },
    /// A declaration whose value the parser could not tell from a bare name.
    ///
    /// `var best = dir` and `var best = dir()` are the same source to the parser,
    /// and `eval_value_source` answers both the same way: it calls the name, and
    /// when that fails it reads the name as a variable instead. Which of the two it
    /// is depends on the frames — a name can be a function in one scope and a
    /// variable in another — so the choice cannot be made here. This instruction
    /// keeps it, and pushes whichever of the two the tree-walker would have got.
    ///
    /// `slot` is where the name lives as a variable, and the value is read from
    /// there *before* the call is attempted: a failed call does not leave the frames
    /// as it found them, and the fallback must not be able to see that. A dotted
    /// name — `msg.id` — is a member read when it is not a call, so `field` is read
    /// off the base value, and `slot` is the *base* variable's.
    CallOrRead {
        name: String,
        slot: u16,
        field: Option<String>,
    },
}

/// A compiled loop, as `vm_compile_stmts` sees it while it builds the body.
///
/// `stop`, `next` and `reset` name the **innermost** loop, so one of these has to
/// be pushed before a body is compiled and popped after it. Two things are not
/// known until the body is built — where the loop is left, and where it continues
/// — so the emitted instructions carry placeholders that `vm_close_loop` fills in,
/// exactly as the loop's own jumps do.
#[derive(Debug)]
struct VmLoop {
    /// Where the loop re-enters: the counter bump for `repeat N` and
    /// `while`/`until`, the `IterNext` for a `for`. `reset` jumps here.
    start: usize,
    /// The counter slot, which `reset` writes back to zero.
    reps: u16,
    /// The flag `stop` raises. Only a runner-driven loop reads it; a stream's
    /// `stop` leaves through its exit jump and the flag is simply never consulted.
    stop_flag: u16,
    /// Set when something outside the body owns the counter — the runner of a
    /// `repeat N`, or `IterNext`. Such a loop cannot honour `reset`, because the
    /// counter is written again before the body sees it.
    counter_owned: bool,
    /// `stop` sites still waiting for the loop's exit.
    stop_jumps: Vec<usize>,
    /// `next` sites still waiting for the loop's continue point.
    next_jumps: Vec<usize>,
}

/// A compiled loop body: instructions plus the names of the frame slots they
/// address.
///
/// Slots are the point of this stage. A compiled loop used to reach back into the
/// interpreter for every variable: `get_var`/`set_var` search the frame stack
/// innermost-first and hash a `String` on every access, once per iteration. Now
/// the body's variables are read into a `Vec<Value>` on entry and written back on
/// exit, and the instruction stream only indexes that vector.
#[derive(Debug)]
struct VmProgram {
    instrs: Vec<VmInstr>,
    slots: Vec<String>,
    /// Slot holding the loop counter, when the body reads `Reps`.
    reps_slot: Option<u16>,
    /// Slot holding the `repeat <Item> in` binding. Like `Reps` it belongs to the
    /// loop: it is not required to exist beforehand and does not outlive the loop.
    item_slot: Option<u16>,
    /// Slot the iterable is materialised into, for the `for` forms.
    items_slot: Option<u16>,
    /// Slots the loop owns: every `Reps` and every `for` item binding, at every
    /// nesting level, plus every name a body declares with `var`. They are not
    /// required to exist beforehand, which is what `vm_names_bound` needs to know;
    /// they *are* flushed, so a callee can name them, and they are never read back
    /// from the frames — the tree-walker's loop frames are popped too.
    owned: Vec<u16>,
    /// The `Reps` slot of each loop level currently being compiled, innermost last.
    /// An inner level shadows the outer one, which is what makes `Reps` inside a
    /// nested body mean the inner counter.
    reps_stack: Vec<u16>,
    /// Whether `give` may compile. Only a function body can return; a loop the VM
    /// runs on its own has to hand `Control` values back to `exec_repeat` instead.
    allow_return: bool,
    /// Scratch slots for a function body: the value `give` produced, and a flag that
    /// records whether one ran at all.
    return_slot: Option<(u16, u16)>,
    /// Bodies of a `do:` block, as programs of their own.
    ///
    /// Running each one separately is what keeps `catch` out of the instruction
    /// loop: the block's error is caught at the boundary, so no instruction needs to
    /// know that handlers exist. It also means the block's bodies reach the loop's
    /// variables through the frames, exactly as a call does.
    subs: Vec<VmProgram>,
    /// The loops being compiled, innermost last. Empty outside a body, which is
    /// what makes a `stop` with no loop to stop in a refusal rather than a jump to
    /// nowhere.
    loops: Vec<VmLoop>,
    /// Set only for a runner-driven loop: the flag its `stop` raises, which
    /// `vm_run` reads after each pass.
    stop_flag: Option<u16>,
    /// Every `Reps` slot, at every nesting level: the values the instruction stream
    /// rewrites on its own. The hand-over publishes them so a callee can read the
    /// counter, but `vm_pull_slots` must not read them back — see there.
    counters: Vec<u16>,
    /// Per-slot "is a loop counter" flags, built once on first use.
    ///
    /// `vm_pull_slots` runs at every call boundary and has to skip the counters, so it
    /// used to do `counters.contains(&slot)` for every slot. `counters` is at most the
    /// loop-nesting depth, so that was a handful of comparisons — small, but it is
    /// inside the hottest boundary there is, and it is trivially removable: the roles
    /// cannot change after compilation, so they are computed once and indexed.
    counter_flags: OnceLock<Vec<bool>>,
}

impl VmProgram {
    /// Is slot `i` a loop counter? Computed once, then O(1).
    ///
    /// The roles cannot change after compilation, so this is a one-time pass over the
    /// slots rather than a scan of `counters` per slot at every call boundary.
    #[inline]
    fn is_counter(&self, i: usize) -> bool {
        let flags = self.counter_flags.get_or_init(|| {
            let mut flags = vec![false; self.slots.len()];
            for counter in &self.counters {
                if let Some(flag) = flags.get_mut(*counter as usize) {
                    *flag = true;
                }
            }
            flags
        });
        flags.get(i).copied().unwrap_or(false)
    }

    /// An empty program. The slots, the `owned` set and the level stack are filled
    /// in by whichever compiler builds it.
    fn new() -> Self {
        Self {
            instrs: Vec::new(),
            slots: Vec::new(),
            reps_slot: None,
            item_slot: None,
            items_slot: None,
            owned: Vec::new(),
            reps_stack: Vec::new(),
            allow_return: false,
            return_slot: None,
            subs: Vec::new(),
            loops: Vec::new(),
            stop_flag: None,
            counters: Vec::new(),
            counter_flags: OnceLock::new(),
        }
    }
}

/// Fill in the targets of a finished loop's `stop` and `next` sites.
///
/// `exit` is where the loop is left: the `PopFrame` of a stream that pushed one —
/// so the frame goes even when a `stop` left the body early — or the end of the
/// stream when the `exec_repeat` around the loop owns the frame. `cont` is where
/// `next` resumes the loop.
fn vm_close_loop(prog: &mut VmProgram, exit: usize, cont: usize) {
    let Some(open) = prog.loops.pop() else {
        return;
    };
    for i in open.stop_jumps {
        if let VmInstr::StopLoop { target, .. } = &mut prog.instrs[i] {
            *target = exit;
        }
    }
    for i in open.next_jumps {
        if let VmInstr::NextLoop { target } = &mut prog.instrs[i] {
            *target = cont;
        }
    }
}

/// Slot index for `name`, adding it if this body has not used it yet.
fn vm_slot(slots: &mut Vec<String>, name: &str) -> u16 {
    if let Some(i) = slots.iter().position(|s| s == name) {
        return i as u16;
    }
    slots.push(name.to_string());
    (slots.len() - 1) as u16
}

/// A slot the compiler owns rather than the program: the `for` list and its
/// position. It has no name, so it is never looked up in — or written back to —
/// the frames, and `vm_names_bound` does not require it to exist.
fn vm_scratch(slots: &mut Vec<String>) -> u16 {
    slots.push(String::new());
    (slots.len() - 1) as u16
}

/// A slot for `name` that shadows any slot already carrying that name.
///
/// Nested loops each need their own `Reps`, and both must keep the name `Reps` so
/// the frames — and therefore a callee — see the counter under the name the
/// tree-walker uses. The flush walks slots in order, so the innermost level wins.
fn vm_slot_shadow(slots: &mut Vec<String>, name: &str) -> u16 {
    slots.push(name.to_string());
    (slots.len() - 1) as u16
}

/// Whether `callee` may be compiled into a `Call` instruction in an expression.
///
/// A compiled call hands its arguments to `invoke_callable_values`, which is the
/// expression engine's own dispatcher, so nothing about dispatch has to be
/// restated here. Safe mode is the exception: `exec_call_inner` enforces it and
/// the expression dispatcher does not, so rather than duplicate the deny-list
/// the whole body is left to the tree-walker while safe mode is on.
fn vm_call_compilable(callee: &[String]) -> bool {
    callee.len() == 1 && !safe_mode()
}

/// Whether a *statement* call — `f(x)` on its own line, or `y is f(x)` — may be
/// compiled.
///
/// These go through `exec_call_inner`, not the expression dispatcher, so the two
/// differences have to be ruled out: the builtins it intercepts before falling
/// through (`call_func` and friends are not reachable from the expression
/// dispatcher), and its order — it instantiates a class *before* it looks for a
/// function of that name, while the expression dispatcher does the reverse. A
/// name that is already a class is therefore left alone.
fn vm_stmt_call_ok(callee: &str, args: &[ArgItem], rt: &Runtime) -> bool {
    if safe_mode() {
        return false;
    }
    if matches!(callee, "call_func" | "call_func_lenient" | "dispatch") {
        return false;
    }
    if rt.classes.contains_key(callee) {
        return false;
    }
    args.iter()
        .all(|a| matches!(a, ArgItem::Positional(_)))
}

/// Compile a call's positional arguments, left to right.
fn vm_compile_args(args: &[ArgItem], prog: &mut VmProgram, rt: &Runtime) -> bool {
    for arg in args {
        match arg {
            ArgItem::Positional(text) => {
                if !vm_compile_text(text, prog, rt) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// Compile a statement's value, which the parser gives either as an expression or
/// as a call it could not tell from a bare name.
///
/// A space-call — `len text` — is a call with arguments, and `vm_stmt_call_ok` is
/// the same gate a statement call takes. A bare name — `dir` — is the ambiguous
/// form: `eval_value_source` calls the name when it can and reads it as a variable
/// when the call fails, so it becomes a `CallOrRead` and the choice stays at run
/// time. That covers every shape `Stmt::Assign` already took; the compiler used to
/// take only the expression one, which left `var x = y` and `var n = len text` —
/// and so their whole enclosing body — on the tree-walker.
fn vm_compile_value(value: &ValueSource, prog: &mut VmProgram, rt: &Runtime) -> bool {
    match value {
        ValueSource::Expr(text) => vm_compile_text(text, prog, rt),
        ValueSource::Call { callee, args } if args.is_empty() => {
            // The ambiguous form: `var best = dir`, `x is y`, and a member read like
            // `msg.id`, none of which the parser can tell from a zero-argument call.
            // A dotted one keeps the member read as its fallback — a zero-argument
            // *call* on one would have been written `msg.id()`, which arrives as an
            // expression — and a deeper chain is left to the tree-walker.
            let (base, field) = match callee.split_once('.') {
                Some((base, field)) if !field.contains('.') => (base, Some(field.to_string())),
                Some(_) => return false,
                None => (callee.as_str(), None),
            };
            if !is_bare_identifier(base) {
                return false;
            }
            // The name is registered as a slot so the hand-over keeps it current —
            // that is where the instruction reads the variable from.
            let slot = vm_slot(&mut prog.slots, base);
            prog.owned.push(slot);
            prog.instrs.push(VmInstr::CallOrRead {
                name: callee.clone(),
                slot,
                field,
            });
            true
        }
        ValueSource::Call { callee, args } if vm_stmt_call_ok(callee, args, rt) => {
            if !vm_compile_args(args, prog, rt) {
                return false;
            }
            prog.instrs.push(VmInstr::Call {
                callee: callee.clone(),
                argc: args.len() as u16,
            });
            true
        }
        _ => false,
    }
}

/// The frame slot to read `target` from *in place*, when it is a bare variable.
///
/// `None` means the caller has to evaluate it as a value instead. Reading a
/// container out of its slot rather than pushing a copy is what keeps `xs[i]`,
/// `xs.length`, `s[1:3]` and `p.method()` O(1) in a compiled body rather than
/// O(size of the container) — the same split the tree-walker makes with
/// `get_var_ref` and `resolve_index_ref`.
fn vm_slot_target(target: &Expr, prog: &mut VmProgram) -> Option<u16> {
    match target {
        Expr::Var(names) if names.len() == 1 => {
            // `argument`/`argument2` resolve from `frame.args`, not `frame.vars`.
            if positional_alias_index(&names[0]).is_some() {
                return None;
            }
            Some(match prog.reps_stack.last() {
                Some(inner) if names[0] == "Reps" => *inner,
                _ => vm_slot(&mut prog.slots, &names[0]),
            })
        }
        _ => None,
    }
}

/// Compile an expression, postfix. Returns false for anything unsupported, in
/// which case the caller must discard `out`.
///
/// Every expression form is compiled except dict comprehensions and lambdas: the
/// dict form needs a way to store a key and a value, and a lambda carries its body
/// as an AST the runtime would have to keep. Everything else — indexing,
/// slicing, members, method calls, interpolation, list and dict literals,
/// ternaries, unary operators and short-circuit `and`/`or` — used to send the
/// whole enclosing body back to the tree-walker, which meant a single method call
/// anywhere in a loop cost the loop its compilation.
fn vm_compile_expr(e: &Expr, prog: &mut VmProgram) -> bool {
    match e {
        Expr::Int(v) => prog.instrs.push(VmInstr::Const(Value::Int(*v))),
        Expr::Float(v) => prog.instrs.push(VmInstr::Const(Value::Float(*v))),
        Expr::Bool(v) => prog.instrs.push(VmInstr::Const(Value::Bool(*v))),
        Expr::Empty => prog.instrs.push(VmInstr::Const(Value::Empty)),
        Expr::Str(v) => {
            // `eval_ast` interpolates only when the literal contains `%`; a
            // literal without one is a constant, and one with one needs the
            // runtime to read the variables it names.
            if v.contains('%') {
                prog.instrs.push(VmInstr::Interpolate(v.clone()));
            } else {
                prog.instrs.push(VmInstr::Const(Value::Str(v.clone())));
            }
        }
        Expr::Var(names) if names.len() == 1 => match vm_slot_target(e, prog) {
            Some(slot) => prog.instrs.push(VmInstr::LoadSlot(slot)),
            None => return false,
        },
        // A two-part name — `d.key`, `obj.field`, `mod.func` — is the shape the
        // parser gives a dotted read, and it is common enough in a loop body that
        // refusing it cost the whole body its compilation. The root is read from
        // its slot and one member is taken, which is exactly what
        // `resolve_var_chain` does for the same two parts.
        Expr::Var(names) if names.len() == 2 => {
            let root = Expr::Var(vec![names[0].clone()]);
            match vm_slot_target(&root, prog) {
                Some(slot) => prog.instrs.push(VmInstr::MemberSlot {
                    slot,
                    field: names[1].clone(),
                }),
                None => return false,
            }
        }
        // Deeper chains stay in the tree-walker.
        Expr::Var(_) => return false,
        Expr::Unary { op, rhs } => {
            if !vm_compile_expr(rhs, prog) {
                return false;
            }
            match UnaryOp::from_name(op) {
                Some(op) => prog.instrs.push(VmInstr::Unary(op)),
                None => return false,
            }
        }
        Expr::Binary { op, lhs, rhs } if op == "and" || op == "or" => {
            // Short-circuit, as `eval_ast` does: the right operand is only
            // evaluated when the left one does not decide the answer. Compiling
            // these into `Bin` would evaluate both sides, which is a different
            // program — a guard like `has_key(d, "k") and d["k"] == 1` would fail
            // on the right operand instead of short-circuiting past it.
            if !vm_compile_expr(lhs, prog) {
                return false;
            }
            let keep_jump = prog.instrs.len();
            prog.instrs.push(if op == "and" {
                VmInstr::JumpIfFalseKeep(0)
            } else {
                VmInstr::JumpIfTrueKeep(0)
            });
            if !vm_compile_expr(rhs, prog) {
                return false;
            }
            prog.instrs.push(VmInstr::ToBool);
            let exit_jump = prog.instrs.len();
            prog.instrs.push(VmInstr::Jump(0));
            // The left operand decided it: drop it and push the answer.
            let decided = prog.instrs.len();
            prog.instrs.push(VmInstr::Pop);
            prog.instrs.push(VmInstr::Const(Value::Bool(op == "or")));
            let end = prog.instrs.len();
            match &mut prog.instrs[keep_jump] {
                VmInstr::JumpIfFalseKeep(target) | VmInstr::JumpIfTrueKeep(target) => {
                    *target = decided;
                }
                _ => return false,
            }
            match &mut prog.instrs[exit_jump] {
                VmInstr::Jump(target) => *target = end,
                _ => return false,
            }
        }
        Expr::Binary { op, lhs, rhs } => {
            if !vm_compile_expr(lhs, prog) {
                return false;
            }
            if !vm_compile_expr(rhs, prog) {
                return false;
            }
            // Resolved here, once, rather than matched as text on every pass.
            match BinaryOp::from_name(op) {
                Some(op) => prog.instrs.push(VmInstr::Bin(op)),
                None => return false,
            }
        }
        Expr::Ternary {
            cond,
            true_expr,
            false_expr,
        } => {
            if !vm_compile_expr(cond, prog) {
                return false;
            }
            let to_false = prog.instrs.len();
            prog.instrs.push(VmInstr::JumpIfFalse(0));
            if !vm_compile_expr(true_expr, prog) {
                return false;
            }
            let past = prog.instrs.len();
            prog.instrs.push(VmInstr::Jump(0));
            let on_false = prog.instrs.len();
            if !vm_compile_expr(false_expr, prog) {
                return false;
            }
            let end = prog.instrs.len();
            match &mut prog.instrs[to_false] {
                VmInstr::JumpIfFalse(target) => *target = on_false,
                _ => return false,
            }
            match &mut prog.instrs[past] {
                VmInstr::Jump(target) => *target = end,
                _ => return false,
            }
        }
        Expr::List(items) => {
            for item in items {
                if !vm_compile_expr(item, prog) {
                    return false;
                }
            }
            prog.instrs.push(VmInstr::MakeList(items.len() as u16));
        }
        Expr::Dict(pairs) => {
            for (key, value) in pairs {
                if !vm_compile_expr(key, prog) {
                    return false;
                }
                if !vm_compile_expr(value, prog) {
                    return false;
                }
            }
            prog.instrs.push(VmInstr::MakeDict(pairs.len() as u16));
        }
        Expr::Index { target, index } => match vm_slot_target(target, prog) {
            Some(slot) => {
                if !vm_compile_expr(index, prog) {
                    return false;
                }
                prog.instrs.push(VmInstr::IndexSlot(slot));
            }
            None => {
                if !vm_compile_expr(target, prog) || !vm_compile_expr(index, prog) {
                    return false;
                }
                prog.instrs.push(VmInstr::Index);
            }
        },
        Expr::Slice {
            target,
            start,
            end,
            step,
        } => {
            let slot = vm_slot_target(target, prog);
            if slot.is_none() && !vm_compile_expr(target, prog) {
                return false;
            }
            // A missing bound is the `Empty` the tree-walker's slice parser
            // accepts for "no bound".
            for bound in [start, end, step] {
                match bound {
                    Some(expr) => {
                        if !vm_compile_expr(expr, prog) {
                            return false;
                        }
                    }
                    None => prog.instrs.push(VmInstr::Const(Value::Empty)),
                }
            }
            match slot {
                Some(slot) => prog.instrs.push(VmInstr::SliceSlot(slot)),
                None => prog.instrs.push(VmInstr::Slice),
            }
        }
        Expr::Member { target, field } => match vm_slot_target(target, prog) {
            Some(slot) => prog.instrs.push(VmInstr::MemberSlot {
                slot,
                field: field.clone(),
            }),
            None => {
                if !vm_compile_expr(target, prog) {
                    return false;
                }
                prog.instrs.push(VmInstr::MemberGet(field.clone()));
            }
        },
        Expr::MethodCall {
            target,
            method,
            args,
        } => {
            let slot = vm_slot_target(target, prog);
            if slot.is_none() && !vm_compile_expr(target, prog) {
                return false;
            }
            for arg in args {
                if !vm_compile_expr(arg, prog) {
                    return false;
                }
            }
            let argc = args.len() as u16;
            match slot {
                Some(slot) => prog.instrs.push(VmInstr::PlaceMethodCall {
                    slot,
                    method: method.clone(),
                    argc,
                }),
                None => prog.instrs.push(VmInstr::MethodCall {
                    method: method.clone(),
                    argc,
                }),
            }
        }
        // Arguments are pushed left to right, so the instruction pops `argc`
        // values and passes them on in source order.
        Expr::Call { callee, args } if vm_call_compilable(callee) => {
            for arg in args {
                if !vm_compile_expr(arg, prog) {
                    return false;
                }
            }
            prog.instrs.push(VmInstr::Call {
                callee: callee[0].clone(),
                argc: args.len() as u16,
            });
        }
        // A list comprehension: the first expression form that iterates, lowered
        // onto the pair a `for` already uses, with a slot holding the result.
        //
        //   out = []
        //   <iterable>              (pushed, then materialised by IterPrepare)
        //   IterPrepare
        //   PushFrame
        //   item <- its old value saved, then bound in the frame
        // start:
        //   IterNext                (exit when exhausted)
        //   <condition>? JumpIfFalse -> join
        //   <result>
        //   AppendSlot(out)
        // join: Jump(start)
        // exit: PopFrame
        //   item <- its old value put back
        //   LoadSlot(out)
        //
        // The item is the delicate part, exactly as it is for a `for`: a read
        // resolves to the first slot carrying a name, so the item shares whatever
        // slot the name already had and shadows it for precisely as long as the
        // comprehension runs. `Reps` gets a scratch slot instead — the tree-walker's
        // comprehension publishes no counter, so a read of `Reps` inside one means an
        // enclosing loop's, which is what the slot it resolves to holds.
        //
        // Only the list form is compiled; a dict comprehension needs a way to store
        // a key and a value, which is a different accumulation.
        Expr::Comprehension {
            kind,
            result_expr,
            item_name,
            iterable,
            condition,
            ..
        } if kind.as_str() == "list" => {
            let out = vm_scratch(&mut prog.slots);
            let items = vm_scratch(&mut prog.slots);
            let index = vm_scratch(&mut prog.slots);
            let reps = vm_scratch(&mut prog.slots);
            let item = vm_slot(&mut prog.slots, item_name);
            let saved = vm_scratch(&mut prog.slots);
            prog.owned.push(item);
            prog.instrs.push(VmInstr::MakeList(0));
            prog.instrs.push(VmInstr::StoreSlot(out));
            if !vm_compile_expr(iterable, prog) {
                return false;
            }
            prog.instrs.push(VmInstr::IterPrepare {
                items,
                index,
                msg: "Comprehension iterable must be a list or set".to_string(),
                generators: false,
            });
            prog.instrs.push(VmInstr::PushFrame);
            prog.instrs.push(VmInstr::LoadSlot(item));
            prog.instrs.push(VmInstr::StoreSlot(saved));
            prog.instrs.push(VmInstr::SetLocal {
                slot: item,
                name: item_name.clone(),
            });
            let start = prog.instrs.len();
            prog.instrs.push(VmInstr::IterNext {
                items,
                index,
                item,
                reps,
                end: usize::MAX,
            });
            let mut skipped: Vec<usize> = Vec::new();
            if let Some(cond) = condition {
                if !vm_compile_expr(cond, prog) {
                    return false;
                }
                prog.instrs.push(VmInstr::JumpIfFalse(usize::MAX));
                skipped.push(prog.instrs.len() - 1);
            }
            if !vm_compile_expr(result_expr, prog) {
                return false;
            }
            prog.instrs.push(VmInstr::AppendSlot {
                slot: out,
                callee: "append".to_string(),
            });
            let join = prog.instrs.len();
            prog.instrs.push(VmInstr::Jump(start));
            let exit = prog.instrs.len();
            prog.instrs.push(VmInstr::PopFrame);
            prog.instrs.push(VmInstr::LoadSlot(saved));
            prog.instrs.push(VmInstr::StoreSlot(item));
            prog.instrs.push(VmInstr::LoadSlot(out));
            if let VmInstr::IterNext { end, .. } = &mut prog.instrs[start] {
                *end = exit;
            }
            for j in skipped {
                if let VmInstr::JumpIfFalse(target) = &mut prog.instrs[j] {
                    *target = join;
                }
            }
        }
        _ => return false,
    }
    true
}

/// `INDENT_VM_TRACE=1`: report which statement refused, one at a time.
///
/// A refusal is silent by design — the body goes back to the tree-walker, which is
/// always correct — so this is the only way to see how much of a program the
/// compiler is actually taking. Without it, a body that quietly stopped compiling
/// (a new statement form, an expression it cannot lower) looks exactly like one
/// that never did, and the first symptom is a measurement that no longer improves.
///
/// Each statement is retried on its own against a fresh program, which is what
/// makes the report name a statement rather than the whole body. The probe is given
/// a loop it is `inside`, so a `stop`, `next` or `reset` is judged the way the body
/// judged it instead of being reported for having no loop to name.
fn vm_trace_refusal(body: &[Stmt], rt: &Runtime) {
    if std::env::var("INDENT_VM_TRACE").is_err() {
        return;
    }
    for stmt in body {
        let mut probe = VmProgram::new();
        // Judged the way a function body judges it: `give` is legal in one, and a
        // `stop`, `next` or `reset` is legal inside a loop, so the probe is given
        // both. Otherwise the report would name statements the body accepted, and
        // the refusals that actually cost compilation would be lost among them.
        probe.allow_return = true;
        let value = vm_scratch(&mut probe.slots);
        let flag = vm_scratch(&mut probe.slots);
        probe.return_slot = Some((value, flag));
        probe.loops.push(VmLoop {
            start: 0,
            reps: 0,
            stop_flag: 0,
            counter_owned: false,
            stop_jumps: Vec::new(),
            next_jumps: Vec::new(),
        });
        if !vm_compile_stmts(std::slice::from_ref(stmt), &mut probe, rt) {
            eprintln!("[vm] refused statement: {stmt:?}");
        }
    }
}

/// Compile a loop body's statements. Only straight-line statements are accepted:
/// a plain-name assignment, or a bare expression. Anything that can produce
/// control flow (`stop`, `next`, `give`, nested loops, `if`) is refused, so a
/// compiled body always runs to completion and yields `Control::None`.
fn vm_compile_body(body: &[Stmt], rt: &Runtime) -> Option<VmProgram> {
    let mut prog = VmProgram::new();
    // The counter is allocated up front rather than discovered in the body: the
    // runner publishes it either way, and a nested loop's shadow slot of the same
    // name can then never be mistaken for this one.
    let reps = vm_slot(&mut prog.slots, "Reps");
    prog.reps_slot = Some(reps);
    prog.owned.push(reps);
    prog.counters.push(reps);
    prog.reps_stack.push(reps);
    // The runner owns the count and the stream is entered wherever the body
    // starts, so the only label a `stop` needs is the end of the instructions and
    // the flag is what actually stops the loop. `next` is the same place: the
    // runner's own increment is the loop's continue point.
    let stop_flag = vm_scratch(&mut prog.slots);
    prog.stop_flag = Some(stop_flag);
    prog.loops.push(VmLoop {
        start: 0,
        reps,
        stop_flag,
        counter_owned: true,
        stop_jumps: Vec::new(),
        next_jumps: Vec::new(),
    });
    let compiled = vm_compile_stmts(body, &mut prog, rt);
    prog.reps_stack.pop();
    let exit = prog.instrs.len();
    vm_close_loop(&mut prog, exit, exit);
    // `INDENT_VM_TRACE=1` reports which statement refused. A refusal is silent
    // by design — the body goes back to the tree-walker — so this is the only
    // way to see how much of a program the compiler is actually accepting.
    if !compiled {
        vm_trace_refusal(body, rt);
        return None;
    }
    if !vm_verify_stack(&prog) {
        return None;
    }
    Some(prog)
}

/// The shape `try_inplace_list_append` takes: `name is append name x`.
///
/// Reproducing it matters more than it looks: `exec_stmt` grows the list in
/// place for exactly this shape, so a compiled loop that ran the builtin instead
/// would copy the whole list on every iteration and build an n-element list in
/// O(n²) — slower than the tree-walker it replaced.
fn vm_append_self(name: &str, callee: &str, args: &[ArgItem]) -> bool {
    matches!(callee, "append" | "push" | "extend")
        && args.len() == 2
        && matches!(&args[0], ArgItem::Positional(t) if t.trim() == name)
        && matches!(&args[1], ArgItem::Positional(_))
}

/// The shape `try_inplace_string_concat` and `try_inplace_container_concat` take:
/// `name is name + <rest>`. Returns `<rest>`, the text the compiler must compile.
fn vm_concat_self<'a>(name: &str, text: &'a str) -> Option<&'a str> {
    if !is_bare_identifier(name) {
        return None;
    }
    match text
        .strip_prefix(name)
        .and_then(|rest| rest.strip_prefix(" + "))
    {
        Some(rhs) if !rhs.is_empty() => Some(rhs),
        _ => None,
    }
}

/// Compile a `while`/`until` loop.
///
/// Unlike a `repeat N` body this stream carries its own jumps and counter, so the
/// condition is compiled exactly where the tree-walker evaluates it — inside the
/// loop, before the body — and the runner just executes the stream once.
fn vm_compile_while(cond: &str, until: bool, body: &[Stmt], rt: &Runtime) -> Option<VmProgram> {
    // In the tree-walker `Reps` does not exist until the body runs, so a condition
    // that reads it sees an enclosing loop's counter, or the undefined-variable
    // error. Binding the slot before the first condition would answer differently,
    // so such a loop is left to the tree-walker.
    if vm_mentions_reps(cond) {
        return None;
    }
    let mut prog = VmProgram::new();
    let reps = vm_slot(&mut prog.slots, "Reps");
    prog.reps_slot = Some(reps);
    prog.owned.push(reps);
    prog.counters.push(reps);
    prog.instrs.push(VmInstr::Const(Value::Int(0)));
    prog.instrs.push(VmInstr::StoreSlot(reps));
    let start = prog.instrs.len();
    prog.instrs.push(VmInstr::CheckLimit {
        slot: reps,
        msg: if until {
            "Repeat until loop exceeded safety limit".to_string()
        } else {
            "Repeat while loop exceeded safety limit".to_string()
        },
    });
    if !vm_compile_text(cond, &mut prog, rt) {
        return None;
    }
    // `until` leaves the loop when the condition first holds; `while` when it
    // first fails.
    let exit_jump = prog.instrs.len();
    prog.instrs.push(VmInstr::JumpIfFalse(usize::MAX));
    // A `stop` here has to leave the stream, which ends it; a `next` has to reach
    // the `BumpSlot` below, so that the pass still counts.
    let stop_flag = vm_scratch(&mut prog.slots);
    prog.loops.push(VmLoop {
        start,
        reps,
        stop_flag,
        counter_owned: false,
        stop_jumps: Vec::new(),
        next_jumps: Vec::new(),
    });
    prog.reps_stack.push(reps);
    let compiled = vm_compile_stmts(body, &mut prog, rt);
    prog.reps_stack.pop();
    let cont = prog.instrs.len();
    prog.instrs.push(VmInstr::BumpSlot(reps));
    prog.instrs.push(VmInstr::Jump(start));
    let end = prog.instrs.len();
    vm_close_loop(&mut prog, end, cont);
    if !compiled {
        vm_trace_refusal(body, rt);
        return None;
    }
    prog.instrs[exit_jump] = if until {
        VmInstr::JumpIfTrue(end)
    } else {
        VmInstr::JumpIfFalse(end)
    };
    if !vm_verify_stack(&prog) {
        return None;
    }
    Some(prog)
}

/// Compile a `repeat <Item> in <iterable>` / `repeat for <iterable>` loop.
///
/// The iterable is materialised by the caller — once, before the loop, exactly as
/// the tree-walker does — and handed to the runner, which drops it into a scratch
/// slot. The stream then fetches one item per pass:
///
///     position = 0
///   start:
///     IterNext            -> item, position += 1, Reps = position, or jump to exit
///     <body>
///     Jump(start)
///   exit:
///
/// `Reps` is the item's index, and the item binding is loop-owned: neither has to
/// exist beforehand, and neither survives the loop (the tree-walker's loop frame
/// is popped too).
fn vm_compile_for(item_name: &str, body: &[Stmt], rt: &Runtime) -> Option<VmProgram> {
    let mut prog = VmProgram::new();
    let reps = vm_slot(&mut prog.slots, "Reps");
    prog.reps_slot = Some(reps);
    prog.owned.push(reps);
    prog.counters.push(reps);
    let item = vm_slot(&mut prog.slots, item_name);
    prog.item_slot = Some(item);
    prog.owned.push(item);
    let items = vm_scratch(&mut prog.slots);
    prog.items_slot = Some(items);
    let index = vm_scratch(&mut prog.slots);
    prog.instrs.push(VmInstr::Const(Value::Int(0)));
    prog.instrs.push(VmInstr::StoreSlot(index));
    let start = prog.instrs.len();
    prog.instrs.push(VmInstr::IterNext {
        items,
        index,
        item,
        reps,
        end: usize::MAX,
    });
    prog.reps_stack.push(reps);
    let compiled = vm_compile_stmts(body, &mut prog, rt);
    prog.reps_stack.pop();
    if !compiled {
        vm_trace_refusal(body, rt);
        return None;
    }
    prog.instrs.push(VmInstr::Jump(start));
    let end = prog.instrs.len();
    if let VmInstr::IterNext { end: target, .. } = &mut prog.instrs[start] {
        *target = end;
    }
    if !vm_verify_stack(&prog) {
        return None;
    }
    Some(prog)
}

/// Compile a loop that sits inside a body which is itself compiled.
///
/// The nested loop is a stream of its own, embedded in the enclosing one. It
/// pushes a frame first — the tree-walker's `exec_repeat` pushes one per loop
/// *execution*, so a callee reached from the inner body must find the inner
/// `Reps`, and a `for` item, in a frame of that loop's own — binds them there once,
/// and jumps back to its own start. Every exit goes through the `PopFrame`, and
/// `vm_run` pops anything the stream left behind if an instruction failed.
///
/// Shape, for `repeat N`:
///
///     [count]  CheckCount(count)      count evaluated once, as the tree-walker does
///     PushFrame
///     Reps = 0;  SetLocal Reps
///   start:
///     LoadSlot(reps) LoadSlot(count) Bin("<") JumpIfFalse(exit)
///     <body>
///     BumpSlot(reps)
///     Jump(start)
///   exit:
///     PopFrame
///
/// A nested `while`/`until` swaps the count test for `CheckLimit` plus the
/// condition, and a nested `for` for `IterPrepare` plus `IterNext`.
fn vm_compile_nested(mode: &RepeatMode, body: &[Stmt], prog: &mut VmProgram, rt: &Runtime) -> bool {
    // A nested loop's counter shadows the enclosing level's, and both keep the name
    // `Reps` so the frames — and a callee reading it — stay faithful.
    let reps = vm_slot_shadow(&mut prog.slots, "Reps");
    prog.owned.push(reps);
    prog.counters.push(reps);
    // The loop's own `stop` flag. A nested loop that is a stream leaves through
    // the `PopFrame` at its exit, so the flag is never read — but `stop` still
    // needs something to raise, and one scratch slot per loop level is cheaper
    // than making the instruction shape conditional.
    let stop_flag = vm_scratch(&mut prog.slots);

    let count = match mode {
        RepeatMode::Count(text) => {
            let slot = vm_scratch(&mut prog.slots);
            if !vm_compile_text(text, prog, rt) {
                return false;
            }
            // The tree-walker's own check, so a non-numeric count reports the same
            // thing and a fractional or negative one truncates the same way.
            prog.instrs.push(VmInstr::CheckCount { slot });
            Some(slot)
        }
        _ => None,
    };

    // The `for` forms: the expression, the binding it creates, and the message the
    // tree-walker reports when the value is not iterable.
    let for_binding = match mode {
        RepeatMode::ForEach(expr) => Some((
            expr.as_str(),
            "Item",
            "Repeat for expects a list, set, or generator",
        )),
        RepeatMode::ForIn {
            iterable_expr,
            item_name,
        } => Some((
            iterable_expr.as_str(),
            item_name.as_str(),
            "Repeat for <Item> in expects a list, set, or generator",
        )),
        _ => None,
    };
    let mut for_slots = None;
    // Scratch slot holding what the item name meant before the loop bound it.
    let mut item_saved = None;
    if let Some((text, name, msg)) = for_binding {
        let items = vm_scratch(&mut prog.slots);
        let index = vm_scratch(&mut prog.slots);
        // One slot per name, not one per loop.
        //
        // `vm_slot_target` resolves a name to the *first* slot carrying it, so a
        // second loop over the same item name — `repeat k in xs` twice in one body,
        // which is how every dict- or list-copying helper in the standard library is
        // written — has to share the first loop's slot. A slot of its own would be
        // invisible to its own body, which would then read the first loop's
        // leftovers: `Merge` in `std/json` returned a dict missing every key of its
        // second argument that way.
        //
        // Sharing is only sound because the value is saved before the loop binds the
        // item and put back when the loop ends. That is what makes the item shadow a
        // name the body already had — a variable, or an enclosing loop's item — for
        // exactly as long as the loop runs.
        let existed = prog.slots.iter().any(|s| s == name);
        let item = vm_slot(&mut prog.slots, name);
        let saved = vm_scratch(&mut prog.slots);
        // Only a name the body did not already have needs excusing from the "must
        // exist beforehand" check.
        if !existed {
            prog.owned.push(item);
        }
        // Evaluated once, before the frame is pushed, exactly where the
        // tree-walker evaluates it.
        if !vm_compile_text(text, prog, rt) {
            return false;
        }
        prog.instrs.push(VmInstr::IterPrepare {
            items,
            index,
            msg: msg.to_string(),
            generators: true,
        });
        for_slots = Some((items, index, item, name));
        item_saved = Some(saved);
    }

    prog.instrs.push(VmInstr::PushFrame);
    // The item name's old value, read before anything binds it.
    if let Some(saved) = item_saved {
        let item = for_slots.unwrap_or((reps, reps, reps, "Item")).2;
        prog.instrs.push(VmInstr::LoadSlot(item));
        prog.instrs.push(VmInstr::StoreSlot(saved));
    }
    prog.instrs.push(VmInstr::Const(Value::Int(0)));
    prog.instrs.push(VmInstr::StoreSlot(reps));
    prog.instrs.push(VmInstr::SetLocal {
        slot: reps,
        name: "Reps".to_string(),
    });
    // The `for` item is bound in the loop's own frame for the same reason as
    // `Reps`: it must not outlive the loop, and a callee reached from the body has
    // to find it. `IterNext` then keeps the slot — and the hand-over keeps the
    // frame — up to date on every pass.
    if let Some((_, _, item, name)) = for_slots {
        prog.instrs.push(VmInstr::SetLocal {
            slot: item,
            name: name.to_string(),
        });
    }

    let start = prog.instrs.len();
    let exit: usize;
    match mode {
        RepeatMode::Count(_) => {
            let count = count.unwrap_or(reps);
            prog.instrs.push(VmInstr::LoadSlot(reps));
            prog.instrs.push(VmInstr::LoadSlot(count));
            prog.instrs.push(VmInstr::Bin(BinaryOp::Lt));
            exit = prog.instrs.len();
            prog.instrs.push(VmInstr::JumpIfFalse(usize::MAX));
        }
        RepeatMode::While(cond) | RepeatMode::Until(cond) => {
            let until = matches!(mode, RepeatMode::Until(_));
            prog.instrs.push(VmInstr::CheckLimit {
                slot: reps,
                msg: if until {
                    "Repeat until loop exceeded safety limit".to_string()
                } else {
                    "Repeat while loop exceeded safety limit".to_string()
                },
            });
            if !vm_compile_text(cond, prog, rt) {
                return false;
            }
            exit = prog.instrs.len();
            prog.instrs.push(if until {
                VmInstr::JumpIfTrue(usize::MAX)
            } else {
                VmInstr::JumpIfFalse(usize::MAX)
            });
        }
        RepeatMode::ForEach(_) | RepeatMode::ForIn { .. } => {
            let (items, index, item, _) = for_slots.unwrap_or((reps, reps, reps, "Item"));
            exit = prog.instrs.len();
            prog.instrs.push(VmInstr::IterNext {
                items,
                index,
                item,
                reps,
                end: usize::MAX,
            });
        }
        // `repeat` with no bound only ends through `stop`, which a compiled body
        // does not support.
        RepeatMode::Infinite => return false,
    }

    prog.reps_stack.push(reps);
    prog.loops.push(VmLoop {
        start,
        reps,
        stop_flag,
        // A `for` advances the position itself, and `IterNext` republishes `Reps`
        // from it, so a `reset` could not survive to the next pass.
        counter_owned: matches!(mode, RepeatMode::ForEach(_) | RepeatMode::ForIn { .. }),
        stop_jumps: Vec::new(),
        next_jumps: Vec::new(),
    });
    let compiled = vm_compile_stmts(body, prog, rt);
    prog.reps_stack.pop();
    if !compiled {
        vm_trace_refusal(body, rt);
        prog.loops.pop();
        return false;
    }
    let mut cont = start;
    if !matches!(mode, RepeatMode::ForEach(_) | RepeatMode::ForIn { .. }) {
        cont = prog.instrs.len();
        prog.instrs.push(VmInstr::BumpSlot(reps));
    }
    prog.instrs.push(VmInstr::Jump(start));
    let end = prog.instrs.len();
    prog.instrs.push(VmInstr::PopFrame);
    // Every exit — falling out of the `for`, the condition failing, or a `stop`
    // jumping here — goes through the `PopFrame`, so putting the saved values back
    // here covers them all.
    if let Some(saved) = item_saved {
        let item = for_slots.unwrap_or((reps, reps, reps, "Item")).2;
        prog.instrs.push(VmInstr::LoadSlot(saved));
        prog.instrs.push(VmInstr::StoreSlot(item));
    }
    // The inner level's `Reps` slot must stop winning too. `vm_slot_target` already
    // resolves `Reps` to the innermost live level, so reads were right; it is the
    // hand-over that publishes slots in order, which would otherwise keep handing a
    // callee the *inner* counter after the inner loop is over.
    if let Some(outer) = prog.reps_stack.last().copied() {
        prog.instrs.push(VmInstr::LoadSlot(outer));
        prog.instrs.push(VmInstr::StoreSlot(reps));
    }
    vm_close_loop(prog, end, cont);
    match &mut prog.instrs[exit] {
        VmInstr::JumpIfFalse(t) | VmInstr::JumpIfTrue(t) => *t = end,
        VmInstr::IterNext { end: t, .. } => *t = end,
        _ => {}
    }
    true
}

/// Compile a statement block as a program of its own — the bodies of a `do:` block.
///
/// Deliberately not a loop: `Reps` inside one names the enclosing loop's counter
/// through the frame, which the hand-over keeps current, rather than a counter of
/// its own.
fn vm_compile_sub(body: &[Stmt], allow_return: bool, rt: &Runtime) -> Option<VmProgram> {
    let mut prog = VmProgram::new();
    prog.allow_return = allow_return;
    if allow_return {
        // A `give` in a block leaves the program the way one outside it does, so
        // the block needs somewhere to put the value for the caller to propagate.
        let value = vm_scratch(&mut prog.slots);
        let flag = vm_scratch(&mut prog.slots);
        prog.return_slot = Some((value, flag));
    }
    if !vm_compile_stmts(body, &mut prog, rt) || !vm_verify_stack(&prog) {
        return None;
    }
    Some(prog)
}

/// Compile a function body.
///
/// The same statements a loop body takes, plus `give` — which is a `Return` here,
/// so a `give` inside a loop in the body is the same instruction. Everything else
/// about a call is untouched: the frame, the parameters, the arity and default
/// handling and the return-type coercion all stay in `invoke_function`, which only
/// swaps how the body runs.
fn vm_compile_function(body: &[Stmt], declared_return: bool, rt: &Runtime) -> Option<VmProgram> {
    if !stmts_contain_loop(body) {
        return None;
    }
    let mut prog = VmProgram::new();
    prog.allow_return = true;
    let value = vm_scratch(&mut prog.slots);
    let flag = vm_scratch(&mut prog.slots);
    prog.return_slot = Some((value, flag));
    if !vm_compile_stmts(body, &mut prog, rt) {
        vm_trace_refusal(body, rt);
        return None;
    }
    prog.instrs.push(VmInstr::EndBody {
        declared: declared_return,
    });
    if !vm_verify_stack(&prog) {
        return None;
    }
    Some(prog)
}

/// Run a compiled function body.
///
/// `give` is `Control::Return` and falling off the end is `Control::None` — the two
/// outcomes `exec_block` reports — so `invoke_function` keeps deciding what they
/// mean (an implicit `empty`, or the error for a body that declares a return type).
fn vm_run_body(prog: &VmProgram, ctx: &mut ExecContext<'_>) -> Result<Control, String> {
    Ok(match vm_run(None, None, prog, ctx)? {
        Some(v) => Control::Return(v),
        None => Control::None,
    })
}

/// Whether an expression's text mentions the bare name `Reps`.
///
/// A word scan rather than an AST walk: a false positive only means the loop is
/// left to the tree-walker, which is always correct.
fn vm_mentions_reps(text: &str) -> bool {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|word| word == "Reps")
}

/// Compile one expression from its text: the same parse cache `eval_expr` uses,
/// so preprocessing and parsing cannot diverge between the two engines.
///
/// The operand-stack bound is checked on the emitted program instead of here —
/// see `vm_verify_stack` for why that has to be the source of truth.
fn vm_compile_text(text: &str, prog: &mut VmProgram, rt: &Runtime) -> bool {
    let Ok(ast) = rt.cached_expr(text) else {
        if std::env::var("INDENT_VM_TRACE").is_ok() {
            eprintln!("[vm] unparsable expression: {text}");
        }
        return false;
    };
    if !vm_compile_expr(&ast, prog) {
        if std::env::var("INDENT_VM_TRACE").is_ok() {
            eprintln!("[vm] refused expression: {text}");
        }
        return false;
    }
    true
}

/// Compile a statement list, recursing into `if` branches.
///
/// An `if` becomes: each conditional branch emits its condition, a `JumpIfFalse`
/// to the next branch, its body, then a `Jump` past every remaining branch; an
/// `otherwise` branch emits its body unconditionally; and the false-targets of the
/// last conditional fall past the whole chain. That is first-true-wins, exactly as
/// `exec_stmt`'s `IfChain` arm reads. Branch bodies are compiled recursively, so
/// nested `if`s work; anything else in a body (including `give`, `stop` and nested
/// loops) refuses the whole loop, because those produce `Control` and a compiled
/// body must yield `Control::None`.
fn vm_compile_stmts(stmts: &[Stmt], prog: &mut VmProgram, rt: &Runtime) -> bool {
    for stmt in stmts {
        match stmt {
            Stmt::Assign { name, value, .. } => {
                if name.contains('.') || positional_alias_index(name).is_some() {
                    return false;
                }
                // The two assignments `exec_stmt` grows in place get instructions
                // that mutate the slot directly, for the same reason: the general
                // path copies the container or the accumulated string on every
                // iteration.
                if let ValueSource::Call { callee, args } = value {
                    if vm_append_self(name, callee, args) {
                        if !vm_compile_args(&args[1..], prog, rt) {
                            return false;
                        }
                        let s = vm_slot(&mut prog.slots, name);
                        prog.instrs.push(VmInstr::AppendSlot {
                            slot: s,
                            callee: callee.clone(),
                        });
                        continue;
                    }
                }
                if let ValueSource::Expr(text) = value {
                    if let Some(rest) = vm_concat_self(name, text) {
                        if !vm_compile_text(rest, prog, rt) {
                            return false;
                        }
                        let s = vm_slot(&mut prog.slots, name);
                        prog.instrs.push(VmInstr::ConcatSlot(s));
                        continue;
                    }
                }
                // Every value shape, the same ones a declaration takes — including
                // the bare name, whose call-or-read choice `CallOrRead` keeps at run
                // time. This used to compile only a call with explicit arguments,
                // which sent `x is y` and `x is append ys z`… to the tree-walker.
                if !vm_compile_value(value, prog, rt) {
                    return false;
                }
                let s = vm_slot(&mut prog.slots, name);
                prog.instrs.push(VmInstr::StoreSlot(s));
            }
            // `xs[i] is v` / `d["k"] is v` with a plain name: the container is
            // mutated where it lives. A target that is not a bare name is left to
            // the tree-walker.
            Stmt::AssignIndex {
                name,
                index_expr,
                value,
                ..
            } if is_bare_identifier(name) => {
                if !vm_compile_value(value, prog, rt) {
                    return false;
                }
                if !vm_compile_text(index_expr, prog, rt) {
                    return false;
                }
                let slot = vm_slot(&mut prog.slots, name);
                prog.instrs.push(VmInstr::IndexStoreSlot { slot });
            }
            Stmt::AssignIndex { .. } => return false,
            // `x += y`. The operator is the compound form minus its `=`, which is
            // exactly how `exec_stmt` reads it; an operator this stage does not know
            // refuses the body, as everywhere else.
            Stmt::AssignOp {
                line,
                name,
                op,
                value,
            } if is_bare_identifier(name) && positional_alias_index(name).is_none() => {
                let Some(op) = op.strip_suffix('=').and_then(BinaryOp::from_name) else {
                    return false;
                };
                // The value is evaluated before the variable is read, which is what
                // `exec_stmt` does — and what makes `x += x` well defined.
                if !vm_compile_value(value, prog, rt) {
                    return false;
                }
                let slot = vm_slot(&mut prog.slots, name);
                prog.instrs.push(VmInstr::AssignOpSlot {
                    slot,
                    op,
                    line: *line,
                });
            }
            Stmt::AssignOp { .. } => return false,
            // `var x ty = value`. At the top of a body there is no frame to pop and
            // nothing to shadow, so this is an ordinary rebinding of the slot the
            // name already has — exactly what `define_local` on the function frame
            // does, which is why a declaration may take over a parameter's name.
            //
            // Inside a loop the binding is local to the loop, so it gets a slot of
            // its own instead, which must not be read back and does not have to
            // exist beforehand. That is only sound while the name is not already a
            // slot here: a read resolves to the *first* slot carrying a name, so a
            // declaration that shadowed one would be invisible to its own body —
            // `var t` in a second loop of the same body would keep reading the first
            // loop's `t`. Such a body goes back to the tree-walker, which is always
            // right, rather than compiling into something that reads the wrong slot.
            Stmt::DefVar {
                line,
                name,
                ty,
                value,
            } if is_bare_identifier(name) => {
                // A value reaches these two either as an expression or as a call the
                // parser could not tell from one, and both shapes compile the same
                // way a plain assignment compiles them: a space-call — `len text` —
                // is an ordinary call, and a bare name is the form
                // `eval_value_source` reads as a variable when the call fails, so it
                // gets an instruction that keeps that choice at run time. Compiling
                // only the expression shape left the commonest statement in the
                // language out of every compiled body.
                if !vm_compile_value(value, prog, rt) {
                    return false;
                }
                let slot = if prog.loops.is_empty() {
                    vm_slot(&mut prog.slots, name)
                } else {
                    if prog.slots.iter().any(|s| s == name) {
                        return false;
                    }
                    vm_slot_shadow(&mut prog.slots, name)
                };
                prog.owned.push(slot);
                prog.instrs.push(VmInstr::DefVarSlot {
                    slot,
                    line: *line,
                    name: name.clone(),
                    ty: ty.clone(),
                });
            }
            Stmt::DefVar { .. } => return false,
            Stmt::BareExpr { expr, .. } => {
                if !vm_compile_text(expr, prog, rt) {
                    return false;
                }
                prog.instrs.push(VmInstr::Pop);
            }
            // A loop inside a compiled body: its own stream, its own counter, and
            // its own frame for the bindings a callee can name.
            Stmt::Repeat { mode, body, .. } => {
                if !vm_compile_nested(mode, body, prog, rt) {
                    return false;
                }
            }
            // `do:` with a single `catch as name:` — the shape the corpus uses. Each
            // body becomes a program of its own, run at the boundary, so the error
            // path of no single instruction has to know about handlers.
            //
            // A block with `otherwise`, `lastly` or several `catch` clauses is left
            // to the tree-walker for now: those change what happens to a control
            // value that came out of a body, and getting that wrong silently is
            // worse than not compiling it.
            Stmt::DoChain {
                do_body,
                catches,
                otherwise_body,
                lastly_body,
                ..
            } if catches.len() == 1 && otherwise_body.is_none() && lastly_body.is_none() => {
                let Some(do_prog) = vm_compile_sub(do_body, prog.allow_return, rt) else {
                    return false;
                };
                let (binding, catch_body) = &catches[0];
                let mut catch_prog = VmProgram::new();
                catch_prog.allow_return = prog.allow_return;
                if prog.allow_return {
                    let value = vm_scratch(&mut catch_prog.slots);
                    let flag = vm_scratch(&mut catch_prog.slots);
                    catch_prog.return_slot = Some((value, flag));
                }
                // The binding is the block's, not the caller's: it need not exist
                // beforehand and dies with the frame the block pushes.
                let bind_slot = binding.as_ref().map(|name| {
                    let slot = vm_slot(&mut catch_prog.slots, name);
                    catch_prog.owned.push(slot);
                    slot
                });
                if !vm_compile_stmts(catch_body, &mut catch_prog, rt) {
                    return false;
                }
                let do_sub = prog.subs.len() as u16;
                prog.subs.push(do_prog);
                let catch_sub = prog.subs.len() as u16;
                prog.subs.push(catch_prog);
                prog.instrs.push(VmInstr::TryBlock {
                    do_sub,
                    catch_sub,
                    bind_slot,
                    bind_name: binding.clone().unwrap_or_default(),
                });
            }
            // `give`, which only a function body can do. It is a `Return` here, so a
            // `give` inside a loop in the body is the same instruction, and the
            // frames those loops pushed are unwound by the runner.
            Stmt::Give { expr, .. } if prog.allow_return => {
                let (value, flag) = match prog.return_slot {
                    Some(slots) => slots,
                    None => return false,
                };
                if !vm_compile_text(expr, prog, rt) {
                    return false;
                }
                prog.instrs.push(VmInstr::Return { value, flag });
            }
            // A call standing on its own line: the result is discarded, as
            // `exec_stmt` does.
            Stmt::Call { callee, args, .. } if vm_stmt_call_ok(callee, args, rt) => {
                if !vm_compile_args(args, prog, rt) {
                    return false;
                }
                prog.instrs.push(VmInstr::Call {
                    callee: callee.clone(),
                    argc: args.len() as u16,
                });
                prog.instrs.push(VmInstr::Pop);
            }
            // `stop`, `next` and `reset`, which name the innermost loop being
            // compiled. With no loop open they belong to something outside this
            // program — a function body's `stop`, or a loop the block is inside —
            // and the whole body goes back to the tree-walker.
            Stmt::Stop { .. } if !prog.loops.is_empty() => {
                let flag = prog.loops.last().map(|l| l.stop_flag).unwrap_or(0);
                prog.instrs.push(VmInstr::StopLoop {
                    flag,
                    target: usize::MAX,
                });
                let at = prog.instrs.len() - 1;
                if let Some(open) = prog.loops.last_mut() {
                    open.stop_jumps.push(at);
                }
            }
            Stmt::Next { .. } if !prog.loops.is_empty() => {
                prog.instrs.push(VmInstr::NextLoop { target: usize::MAX });
                let at = prog.instrs.len() - 1;
                if let Some(open) = prog.loops.last_mut() {
                    open.next_jumps.push(at);
                }
            }
            Stmt::Reset { .. }
                if prog
                    .loops
                    .last()
                    .is_some_and(|l| !l.counter_owned) =>
            {
                let (slot, target) = match prog.loops.last() {
                    Some(open) => (open.reps, open.start),
                    None => return false,
                };
                prog.instrs.push(VmInstr::ResetLoop { slot, target });
            }
            Stmt::IfChain { branches, .. } => {
                // Indices of `JumpIfFalse`s still waiting for their target: the
                // next branch's first instruction.
                let mut pending_false: Vec<usize> = Vec::new();
                // Indices of the `Jump`s that skip the rest of the chain.
                let mut end_jumps: Vec<usize> = Vec::new();
                for (cond, body) in branches {
                    for j in pending_false.drain(..) {
                        prog.instrs[j] = VmInstr::JumpIfFalse(prog.instrs.len());
                    }
                    if let Some(c) = cond {
                        if !vm_compile_text(c, prog, rt) {
                            return false;
                        }
                        prog.instrs.push(VmInstr::JumpIfFalse(usize::MAX));
                        pending_false.push(prog.instrs.len() - 1);
                    }
                    if !vm_compile_stmts(body, prog, rt) {
                        return false;
                    }
                    if cond.is_some() {
                        prog.instrs.push(VmInstr::Jump(usize::MAX));
                        end_jumps.push(prog.instrs.len() - 1);
                    }
                }
                // A chain whose last branch is conditional falls past the end.
                for j in pending_false.drain(..) {
                    prog.instrs[j] = VmInstr::JumpIfFalse(prog.instrs.len());
                }
                for j in end_jumps {
                    prog.instrs[j] = VmInstr::Jump(prog.instrs.len());
                }
            }
            _ => return false,
        }
    }
    true
}

/// Check every name the body reads before running it. A name that is unbound is
/// the tree-walker's business (it owns the error message), and a body that
/// assigns a name it later reads must also be left to it, so this is done once up
/// front rather than mid-loop where a bail-out could leave half a loop's
/// assignments behind. `Reps` is excluded: the loop binds it itself.
fn vm_names_bound(prog: &VmProgram, ctx: &ExecContext<'_>) -> bool {
    // A guarded block's bodies are programs too, and their names have to exist for
    // the same reason — checked here, once, rather than mid-block.
    prog.subs.iter().all(|sub| vm_names_bound(sub, ctx))
        && prog.slots.iter().enumerate().all(|(i, name)| {
            let i = i as u16;
            // `Reps` and the `for` item are bound by the loop, at every level; a
            // scratch slot is the compiler's own and names nothing.
            name.is_empty() || prog.owned.contains(&i) || ctx.has_var(name)
        })
}

/// Whether reaching `callee` can run Indent code against the current frames.
///
/// Only three things can: a user function or an imported module function (which
/// `resolve_callable` reports), a class constructor, and an object method. Every
/// builtin — `append`, `len`, `string`, `dict_set`, … — runs in Rust and cannot
/// name a frame variable, so it needs no hand-over at all. That matters because
/// the hand-over copies each mirrored value: skipping it keeps a loop that calls
/// a builtin on a container from copying the whole container twice per call.
fn vm_call_runs_user_code(callee: &str, args: &[Value], ctx: &ExecContext<'_>) -> bool {
    if ctx.rt.classes.contains_key(callee) {
        return true;
    }
    // A dotted callee — `msg.id()` — is a method on a module, an object or an
    // instance, so it runs Indent code however it resolves.
    if callee.contains('.') {
        return true;
    }
    if args.iter().any(|a| matches!(a, Value::Object { .. })) {
        return true;
    }
    matches!(
        resolve_callable(callee, ctx),
        Ok(Callable::Local(_)) | Ok(Callable::External { .. })
    )
}

/// `name is name + <rhs>` against `values[idx]`, in place where the tree-walker
/// is in place.
///
/// Growing a string or a list in a loop is how accumulation is spelled in this
/// language, and `eval_binary` copies its left operand — so a compiled loop that
/// used it would copy everything accumulated so far on every pass. The string, list
/// and dict arms grow the slot's own buffer instead, and the numeric arms answer
/// without the string match and the two type-name formattings `eval_binary` opens
/// with; anything else falls back to `eval_binary` with the value moved out, which
/// is exactly what the ordinary path decides.
///
/// The numeric arms are the hop an accumulator actually takes: `total is total + x`
/// compiles to this instruction whatever the operand types turn out to be, so a loop
/// counter or a running sum arrives here on every pass and not at `Bin`. They are
/// `+`'s own numeric arms, mixed cases included, so nothing is decided differently.
///
/// The value is taken out first — a move, not a copy — because a `&mut` borrow of
/// the slot cannot be held while the result is written back into it. Moving a
/// `Value` out and back keeps the `String` or `Vec` it owns, so the arms that grow
/// a buffer still grow that buffer.
fn vm_concat_slot(
    values: &mut [Value],
    idx: usize,
    rhs: Value,
    ctx: &mut ExecContext<'_>,
) -> Result<(), String> {
    use std::fmt::Write as _;
    let lhs = std::mem::replace(&mut values[idx], Value::Empty);
    match (lhs, rhs) {
        (Value::StrShared(mut buf), rhs) => {
            // Appending through `Display` is byte-for-byte what
            // `format!("{}{}", buf, rhs)` produced, without the second copy of
            // the accumulated prefix. Writing through the `Arc` keeps the buffer
            // already owned (`make_mut` is a no-op at refcount 1) instead of
            // allocating a fresh header per step; writing into a `String` cannot
            // fail, so the result is ignored.
            let _ = write!(&mut *buf, "{}", rhs);
            values[idx] = Value::StrShared(buf);
            Ok(())
        }
        (Value::ListShared(mut list), Value::ListShared(extra)) => {
            list.extend(extra);
            values[idx] = Value::ListShared(list);
            Ok(())
        }
        (Value::DictShared(mut dict), Value::DictShared(extra)) => {
            dict.extend(extra.into_inner());
            values[idx] = Value::DictShared(dict);
            Ok(())
        }
        (Value::Int(x), Value::Int(y)) => {
            values[idx] = Value::Int(x + y);
            Ok(())
        }
        (Value::Float(x), Value::Float(y)) => {
            values[idx] = Value::Float(x + y);
            Ok(())
        }
        (Value::Int(x), Value::Float(y)) => {
            values[idx] = Value::Float(x as f64 + y);
            Ok(())
        }
        (Value::Float(x), Value::Int(y)) => {
            values[idx] = Value::Float(x + y as f64);
            Ok(())
        }
        (lhs, rhs) => {
            values[idx] = eval_binary("+", lhs, rhs, ctx)?;
            Ok(())
        }
    }
}

/// Move the slot mirrors into the frame stack.
///
/// A compiled body keeps its variables in `values` and only writes them back when
/// the loop ends, so without this a callee would read the values from *before* the
/// loop and, worse, a pull-back afterwards would undo the iterations that already
/// ran. The counter is handed over too, because a callee can read `Reps`.
///
/// The hand-over is a move, not a copy: a body holding a large list used to pay a
/// deep copy of it into the frame and another one out of the frame on *every* call
/// it made, which turned any loop with a list and a call into quadratic work. The
/// moved-out slots hold `Empty` until `vm_pull_slots` runs, and no user code can
/// observe that: the frames, which are what user code reads, are filled before the
/// boundary and emptied only after it.
fn vm_flush_slots(prog: &VmProgram, values: &mut [Value], ctx: &mut ExecContext<'_>) {
    // Slot order matters: when loops are nested, two slots are named `Reps` and
    // `set_var` resolves to the innermost frame holding that name, so writing the
    // outer level first leaves the inner counter — the one a callee should see —
    // in place.
    for (i, name) in prog.slots.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        ctx.set_var(name, std::mem::replace(&mut values[i], Value::Empty));
    }
}

/// Re-read the slot mirrors from the frame stack after user code has run.
///
/// A call — or an object operator method — assigns variables the ordinary way,
/// through frames, and those writes have to come back into the loop's cache or
/// the body would keep reading the values it had before the call. Paired with
/// `vm_flush_slots`, this keeps the two engines' view identical across a call.
///
/// Every named slot comes back **except the loop counters**. A counter is the one
/// value the compiled stream owns and rewrites itself, and the tree-walker keeps
/// its own in a Rust local that it re-publishes every pass, so a callee's write to
/// `Reps` means nothing there and must mean nothing here.
///
/// It used to skip every `owned` slot too, on the reasoning that the loop's
/// bindings are not the callee's to replace. That was wrong for everything `owned`
/// covers *besides* the counters: the flush moves a slot's value out and leaves
/// `Empty` behind, so skipping the pull left a `var` declared inside a loop — or a
/// `for` item, or the `catch` binding — `Empty` for the rest of the pass. A body
/// that declared a variable and then touched it after a call read `Empty`:
/// `c is c + 1` after `SetTile` in `std/ingame` raised "cannot add empty and int".
fn vm_pull_slots(prog: &VmProgram, values: &mut [Value], ctx: &mut ExecContext<'_>) {
    for (i, name) in prog.slots.iter().enumerate() {
        // A scratch slot names nothing, and a counter is the stream's own.
        if name.is_empty() || prog.is_counter(i) {
            continue;
        }
        // `get_var` consults the positional aliases before the frame's variables,
        // so a slot named `argument` keeps taking the copying read; every other
        // name moves the value straight back into the slot.
        if positional_alias_index(name).is_some() {
            values[i] = ctx.get_var(name).unwrap_or(Value::Empty);
        } else if let Some(value) = ctx.swap_var(name) {
            values[i] = value;
        }
    }
}

/// The ceiling the tree-walker puts on loops whose exit it cannot predict.
const LOOP_MAX_ITERS: usize = 100_000;

/// Run a compiled `repeat N` body in the frame `exec_repeat` already pushed.
/// The runner supplies the iterations and writes `Reps` before each of them.
fn vm_run_loop(n: usize, prog: &VmProgram, ctx: &mut ExecContext<'_>) -> Result<(), String> {
    vm_run(Some(n), None, prog, ctx).map(|_| ())
}

/// Run a compiled loop whose instruction stream carries its own jumps and counter,
/// so it runs exactly once — `while` and `until`.
fn vm_run_stream(prog: &VmProgram, ctx: &mut ExecContext<'_>) -> Result<(), String> {
    vm_run(None, None, prog, ctx).map(|_| ())
}

/// Run a compiled `for` loop over an already-materialised iterable.
fn vm_run_for(
    items: Vec<Value>,
    prog: &VmProgram,
    ctx: &mut ExecContext<'_>,
) -> Result<(), String> {
    let preload = prog.items_slot.map(|s| (s, Value::List(items)));
    vm_run(None, preload, prog, ctx).map(|_| ())
}

fn vm_run(
    iterations: Option<usize>,
    preload: Option<(u16, Value)>,
    prog: &VmProgram,
    ctx: &mut ExecContext<'_>,
) -> Result<Option<Value>, String> {
    // A nested loop's stream may have pushed a frame that an error skipped the
    // `PopFrame` for, so drop back to the depth the loop started at.
    let depth = ctx.frames.len();
    while ctx.frames.len() > depth {
        ctx.pop_frame();
    }
    // Read the body's variables once. Slots are then just a `Vec` index, so the
    // loop body never hashes a name again.
    let mut values: Vec<Value> = prog
        .slots
        .iter()
        .map(|name| ctx.get_var(name).unwrap_or(Value::Empty))
        .collect();
    // `Reps` belongs to the loop frame in the tree-walker (`define_local`), so
    // create it there too: otherwise the write-back below would treat it as a new
    // name and insert it into the globals. The `for` item is bound the same way.
    if prog.reps_slot.is_some() {
        ctx.define_local("Reps", Value::Empty);
    }
    if let Some(s) = prog.item_slot {
        let name = prog.slots[s as usize].clone();
        ctx.define_local(&name, Value::Empty);
    }
    if let Some((slot, value)) = preload {
        values[slot as usize] = value;
    }
    // A stream counts for itself, through `BumpSlot` and `CheckLimit`.
    let reps_slot = if iterations.is_some() {
        prog.reps_slot
    } else {
        None
    };
    // Only a runner-driven loop has a flag for `stop` to raise.
    let stop_flag = if iterations.is_some() {
        prog.stop_flag
    } else {
        None
    };
    let mut stack: [Value; VM_STACK] = std::array::from_fn(|_| Value::Empty);
    let outcome = (|| -> Result<(), String> {
        for reps in 0..iterations.unwrap_or(1) {
            if let Some(s) = reps_slot {
                values[s as usize] = Value::Int(reps as i64);
            }
            let mut sp = 0usize;
            let mut ip = 0usize;
            while ip < prog.instrs.len() {
                match &prog.instrs[ip] {
                    VmInstr::Const(v) => {
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = v.clone();
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::LoadSlot(s) => {
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? =
                            values[*s as usize].clone();
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::StoreSlot(s) => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        values[*s as usize] = stack[sp].clone();
                        ip += 1;
                    }
                    VmInstr::Bin(op) => {
                        sp = sp.checked_sub(2).ok_or_else(vm_underflow)?;
                        let a = stack[sp].clone();
                        let b = stack[sp + 1].clone();
                        // Two ints: the whole operator is decided here, with no
                        // string match, no operand copy for a method lookup and no
                        // possibility of user code running.
                        if let (Value::Int(x), Value::Int(y)) = (&a, &b) {
                            if let Some(v) = op.int_fast(*x, *y) {
                                *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = v;
                                sp += 1;
                                ip += 1;
                                continue;
                            }
                        }
                        // Only an Object operand can reach user code (through one
                        // of its operator methods).
                        let object_operand =
                            matches!(a, Value::Object { .. }) || matches!(b, Value::Object { .. });
                        if object_operand {
                            vm_flush_slots(prog, &mut values, ctx);
                        }
                        let result = eval_binary(op.as_str(), a, b, ctx);
                        if object_operand {
                            vm_pull_slots(prog, &mut values, ctx);
                        }
                        let v = result?;
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = v;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::Unary(op) => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let v = stack[sp].clone();
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? =
                            apply_unary(op.as_str(), v)?;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::ToBool => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let truth = stack[sp].to_bool();
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = Value::Bool(truth);
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::JumpIfFalseKeep(target) => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let jump = !stack[sp].to_bool();
                        sp += 1;
                        ip = if jump { *target } else { ip + 1 };
                    }
                    VmInstr::JumpIfTrueKeep(target) => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let jump = stack[sp].to_bool();
                        sp += 1;
                        ip = if jump { *target } else { ip + 1 };
                    }
                    VmInstr::MakeList(n) => {
                        let n = *n as usize;
                        sp = sp.checked_sub(n).ok_or_else(vm_underflow)?;
                        let items: Vec<Value> = stack[sp..sp + n].to_vec();
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = Value::List(items);
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::MakeDict(n) => {
                        let n = *n as usize;
                        sp = sp.checked_sub(2 * n).ok_or_else(vm_underflow)?;
                        let mut out = SmallDict::new();
                        for pair in 0..n {
                            let key = stack[sp + pair * 2].clone();
                            let value = stack[sp + pair * 2 + 1].clone();
                            let key = match key {
                                Value::StrShared(s) => s.into_string(),
                                // The tree-walker's message for the same mistake.
                                _ => return Err("Dictionary keys must evaluate to string values".to_string()),
                            };
                            out.insert(key, value);
                        }
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = Value::Dict(out);
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::Interpolate(text) => {
                        // Interpolation reads variables, so it has to see them the
                        // way the frames do.
                        vm_flush_slots(prog, &mut values, ctx);
                        let rendered = interpolate_string(text, ctx);
                        vm_pull_slots(prog, &mut values, ctx);
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = Value::Str(rendered);
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::Index => {
                        sp = sp.checked_sub(2).ok_or_else(vm_underflow)?;
                        let target = stack[sp].clone();
                        let index = stack[sp + 1].clone();
                        // Only an Object can define `get_item`, and that is user
                        // code; everything else is a plain read.
                        let result = if matches!(target, Value::Object { .. }) {
                            vm_flush_slots(prog, &mut values, ctx);
                            let out = match call_object_method_value(
                                &target,
                                "get_item",
                                std::slice::from_ref(&index),
                                ctx,
                            ) {
                                Some(r) => r,
                                None => resolve_index(target, index),
                            };
                            vm_pull_slots(prog, &mut values, ctx);
                            out
                        } else {
                            resolve_index(target, index)
                        };
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = result?;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::IndexSlot(slot) => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let index = stack[sp].clone();
                        let is_object = matches!(values[*slot as usize], Value::Object { .. });
                        let result = if is_object {
                            let target = values[*slot as usize].clone();
                            vm_flush_slots(prog, &mut values, ctx);
                            let out = match call_object_method_value(
                                &target,
                                "get_item",
                                std::slice::from_ref(&index),
                                ctx,
                            ) {
                                Some(r) => r,
                                None => resolve_index(target, index),
                            };
                            vm_pull_slots(prog, &mut values, ctx);
                            out
                        } else {
                            resolve_index_ref(&values[*slot as usize], index)
                        };
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = result?;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::Slice => {
                        sp = sp.checked_sub(4).ok_or_else(vm_underflow)?;
                        let target = stack[sp].clone();
                        let start = parse_optional_slice_bound(&stack[sp + 1], "slice start")?;
                        let end = parse_optional_slice_bound(&stack[sp + 2], "slice end")?;
                        let step = parse_slice_step_value(&stack[sp + 3])?;
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? =
                            slice_builtin(&target, start, end, step)?;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::SliceSlot(slot) => {
                        sp = sp.checked_sub(3).ok_or_else(vm_underflow)?;
                        let start = parse_optional_slice_bound(&stack[sp], "slice start")?;
                        let end = parse_optional_slice_bound(&stack[sp + 1], "slice end")?;
                        let step = parse_slice_step_value(&stack[sp + 2])?;
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? =
                            slice_builtin(&values[*slot as usize], start, end, step)?;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::MemberGet(field) => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let target = stack[sp].clone();
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? =
                            resolve_member_access_ref(&target, field)?;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::MemberSlot { slot, field } => {
                        let value = resolve_member_access_ref(&values[*slot as usize], field)?;
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = value;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::MethodCall { method, argc } => {
                        let n = *argc as usize;
                        sp = sp.checked_sub(n + 1).ok_or_else(vm_underflow)?;
                        let receiver = stack[sp].clone();
                        // The operand stack range *is* the argument slice: no vector
                        // and no clones. `receiver` is cloned because the callee can
                        // write it back to the slot this statement overwrites.
                        let args = &stack[sp + 1..sp + 1 + n];
                        vm_flush_slots(prog, &mut values, ctx);
                        let result = invoke_method_on_value(receiver, method, args, ctx);
                        vm_pull_slots(prog, &mut values, ctx);
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = result?;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::PlaceMethodCall { slot, method, argc } => {
                        let n = *argc as usize;
                        sp = sp.checked_sub(n).ok_or_else(vm_underflow)?;
                        // Same as `MethodCall`: the stack range is the argument slice.
                        let args = &stack[sp..sp + n];
                        let name = prog.slots[*slot as usize].clone();
                        // A receiver that is a variable tries the place dispatch
                        // first — that is what lets a method assign its own field
                        // — and the value path only when the name is not a class
                        // method (a function-valued field, or a builtin method).
                        // The check reads the *slot*: between boundaries a frame's
                        // binding for a mirrored variable is `Empty` (the
                        // hand-over moves the value out and leaves the slot as the
                        // authority), so asking the frame here would answer "not a
                        // class method" and quietly drop the field write.
                        let is_class_method = matches!(
                            &values[*slot as usize],
                            Value::Object { class, .. }
                                if class.find_method(method).is_some()
                        );
                        let by_value = if is_class_method {
                            None
                        } else {
                            Some(values[*slot as usize].clone())
                        };
                        vm_flush_slots(prog, &mut values, ctx);
                        let result = match by_value {
                            Some(receiver) => invoke_method_on_value(receiver, method, args, ctx),
                            None => invoke_method_on_place(&name, method, args, ctx)
                                .unwrap_or(Ok(Value::Empty)),
                        };
                        vm_pull_slots(prog, &mut values, ctx);
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = result?;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::IndexStoreSlot { slot } => {
                        sp = sp.checked_sub(2).ok_or_else(vm_underflow)?;
                        let value = stack[sp].clone();
                        let index = stack[sp + 1].clone();
                        // The rules and the messages of the tree-walker's indexed
                        // assignment, done where the container lives: filling a
                        // list element by element must not copy the list per write.
                        match &mut values[*slot as usize] {
                            Value::ListShared(items) => {
                                let raw = match index {
                                    Value::Int(i) => i,
                                    _ => {
                                        return Err(format!(
                                            "List index must be an int, got {}: {}",
                                            index.type_name(),
                                            index
                                        ))
                                    }
                                };
                                let idx = normalize_index(items.len(), raw)
                                    .ok_or_else(|| format!("List index out of range: {raw}"))?;
                                items[idx] = value;
                            }
                            Value::DictShared(map) => {
                                let key = match index {
                                    Value::StrShared(s) => s.to_string(),
                                    _ => {
                                        return Err(format!(
                                            "Dictionary key must be a string, got {}: {}",
                                            index.type_name(),
                                            index
                                        ))
                                    }
                                };
                                map.insert(key, value);
                            }
                            _ => {
                                return Err(
                                    "Indexed assignment is only supported for list and dictionary values"
                                        .to_string(),
                                )
                            }
                        }
                        ip += 1;
                    }
                    VmInstr::DefVarSlot {
                        slot,
                        line,
                        name,
                        ty,
                    } => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let v = stack[sp].clone();
                        // The same coercion the tree-walker's `DefVar` applies.
                        values[*slot as usize] = coerce_type(*line, name, ty, v)?;
                        ip += 1;
                    }
                    VmInstr::Call { callee, argc } => {
                        let n = *argc as usize;
                        sp = sp.checked_sub(n).ok_or_else(vm_underflow)?;
                        // The operand stack range *is* the argument slice: no vector
                        // and no clone per call. It borrows `stack` only until the
                        // call returns, and the push below overwrites the range that
                        // held the arguments, so the order here matters.
                        let args = &stack[sp..sp + n];
                        // Hand the loop's variables over before user code can look
                        // at them, and read its writes back afterwards — including
                        // when it fails, so the write-back below cannot put a value
                        // the callee replaced back into the frame.
                        let user_code = vm_call_runs_user_code(callee, args, ctx);
                        if user_code {
                            vm_flush_slots(prog, &mut values, ctx);
                        }
                        let result = invoke_callable_values(callee, args, ctx);
                        if user_code {
                            vm_pull_slots(prog, &mut values, ctx);
                        }
                        let v = result?;
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = v;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::CallOrRead { name, slot, field } => {
                        // The value the name has as a *variable*, taken before the
                        // call is attempted. A failed `exec_call` does not leave the
                        // frames as it found them — the hand-over then reads the
                        // emptied binding back into the slot — so the fallback has to
                        // be a value that was captured first. Reading the slot is also
                        // the right place for it: slots, not frames, are the body's
                        // store between boundaries.
                        let as_variable = values[*slot as usize].clone();
                        // The hand-over is only for a call that can run Indent code: a
                        // builtin cannot read a frame, and a variable is not going to
                        // be called at all. A dotted name is given it too — if it is a
                        // call it reaches a method.
                        let user_code = field.is_some() || vm_call_runs_user_code(name, &[], ctx);
                        if user_code {
                            vm_flush_slots(prog, &mut values, ctx);
                        }
                        let called = exec_call(name, &[], ctx);
                        if user_code {
                            vm_pull_slots(prog, &mut values, ctx);
                        }
                        let v = match called {
                            Ok(v) => v,
                            // The read `eval_value_source` falls back to. A dotted
                            // source is a member of the value the base had, not a frame
                            // lookup: `msg.id` names a key of `msg`.
                            Err(_) => match field {
                                Some(field) => resolve_member_access_ref(&as_variable, field)?,
                                // A name that is neither has to fail the way it fails
                                // in the tree-walker, message included.
                                None => {
                                    if matches!(as_variable, Value::Empty)
                                        && ctx.get_var(name).is_none()
                                    {
                                        return Err(format!("Unknown identifier '{name}'"));
                                    }
                                    as_variable
                                }
                            },
                        };
                        *stack.get_mut(sp).ok_or_else(|| vm_overflow())? = v;
                        sp += 1;
                        ip += 1;
                    }
                    VmInstr::IterNext {
                        items,
                        index,
                        item,
                        reps,
                        end,
                    } => {
                        let at = match &values[*index as usize] {
                            Value::Int(i) => *i as usize,
                            _ => 0,
                        };
                        let next = match &values[*items as usize] {
                            Value::ListShared(list) => list.get(at).cloned(),
                            _ => None,
                        };
                        match next {
                            Some(v) => {
                                values[*item as usize] = v;
                                values[*index as usize] = Value::Int(at as i64 + 1);
                                values[*reps as usize] = Value::Int(at as i64);
                                ip += 1;
                            }
                            // The iterable is exhausted: leave the loop.
                            None => ip = *end,
                        }
                    }
                    VmInstr::AppendSlot { slot, callee } => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let item = stack[sp].clone();
                        let idx = *slot as usize;
                        match &mut values[idx] {
                            Value::ListShared(list) => append_items_into(list, item, callee)?,
                            // Not a list: `try_inplace_list_append` reports the
                            // error the builtin itself would have produced.
                            other => {
                                return Err(format!(
                                    "{callee} expects a list, got {}",
                                    other.type_name()
                                ))
                            }
                        }
                        ip += 1;
                    }
                    VmInstr::ConcatSlot(slot) => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let rhs = stack[sp].clone();
                        let idx = *slot as usize;
                        // Either operand being an `Object` means an operator method
                        // can run — user code, which needs the hand-over a call gets
                        // and whose *result is the new value of the variable*.
                        if matches!(values[idx], Value::Object { .. })
                            || matches!(rhs, Value::Object { .. })
                        {
                            // The operand is cloned rather than moved out of the slot:
                            // the method has to see the variable as it stands, and on
                            // error the variable has to keep its old value exactly as
                            // the tree-walker leaves it (the write-back at the end of
                            // `vm_run` would otherwise store `Empty`).
                            let lhs = values[idx].clone();
                            vm_flush_slots(prog, &mut values, ctx);
                            let result = eval_binary("+", lhs, rhs, ctx);
                            vm_pull_slots(prog, &mut values, ctx);
                            // `x is x + y` assigns: the operator's result wins over
                            // anything the method did to the name itself, which is what
                            // `exec_stmt`'s assignment does afterwards too.
                            values[idx] = result?;
                        } else {
                            vm_concat_slot(&mut values, idx, rhs, ctx)?;
                        }
                        ip += 1;
                    }
                    VmInstr::AssignOpSlot { slot, op, line } => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let rhs = stack[sp].clone();
                        let idx = *slot as usize;
                        // Two ints: answered here, exactly as `Bin` answers them, so a
                        // compiled `total -= n` does not pay for a call into
                        // `eval_binary` that the same operation spelled `x is x - n`
                        // does not. Everything else — strings, containers, operator
                        // methods, every error — is out of line, which keeps this arm
                        // (and therefore the dispatch loop it lives in) small.
                        if let (Value::Int(x), Value::Int(y)) = (&values[idx], &rhs) {
                            if let Some(v) = op.int_fast(*x, *y) {
                                values[idx] = v;
                                ip += 1;
                                continue;
                            }
                        }
                        vm_assign_slot(prog, &mut values, idx, *op, rhs, *line, ctx)?;
                        ip += 1;
                    }
                    VmInstr::Pop => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        ip += 1;
                    }
                    VmInstr::PushFrame => {
                        ctx.push_frame();
                        ip += 1;
                    }
                    VmInstr::PopFrame => {
                        ctx.pop_frame();
                        ip += 1;
                    }
                    VmInstr::SetLocal { slot, name } => {
                        let v = values[*slot as usize].clone();
                        ctx.define_local(name, v);
                        ip += 1;
                    }
                    VmInstr::CheckCount { slot } => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let v = stack[sp].clone();
                        let n = v.as_number().ok_or_else(|| {
                            format!(
                                "Repeat count must be a number, got {}: {}",
                                v.type_name(),
                                v
                            )
                        })?;
                        // The tree-walker truncates with `as_number()? as usize`,
                        // which saturates, so a negative count is no iterations.
                        values[*slot as usize] = Value::Int((n as usize).min(i64::MAX as usize) as i64);
                        ip += 1;
                    }
                    VmInstr::IterPrepare {
                        items,
                        index,
                        msg,
                        generators,
                    } => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        let v = stack[sp].clone();
                        let list =
                            iterable_items_owned_typed(v, *generators).ok_or_else(|| msg.clone())?;
                        values[*items as usize] = Value::List(list);
                        values[*index as usize] = Value::Int(0);
                        ip += 1;
                    }
                    VmInstr::Return { value, flag } => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        values[*value as usize] = stack[sp].clone();
                        values[*flag as usize] = Value::Bool(true);
                        // Only a function body emits this, and its stream runs exactly
                        // one pass, so leaving the instruction loop is leaving the
                        // program — and it skips the `EndBody` below the last
                        // instruction, which is how a body that returned avoids the
                        // "declares a return type" error. The frames its loops pushed
                        // are unwound after the run.
                        ip = prog.instrs.len();
                    }
                    VmInstr::EndBody { declared } => {
                        // Reached only when no `give` ran.
                        if *declared {
                            return Err(
                                "Function declares a return type but no Give statement returned a value"
                                    .to_string(),
                            );
                        }
                        break;
                    }
                    VmInstr::TryBlock {
                        do_sub,
                        catch_sub,
                        bind_slot,
                        bind_name,
                    } => {
                        // The bodies are programs of their own, so the loop's variables
                        // are handed over exactly as they are around a call — before
                        // the body, and read back after it, whichever way it ended.
                        vm_flush_slots(prog, &mut values, ctx);
                        let outcome = vm_run(None, None, &prog.subs[*do_sub as usize], ctx);
                        vm_pull_slots(prog, &mut values, ctx);
                        let returned = match outcome {
                            Ok(value) => value,
                            Err(message) => {
                                // `catch as name:` runs in a frame of its own, with the
                                // message bound there — the tree-walker's shape. The
                                // binding is defined before the body so its own slot for
                                // the name reads it out of that frame.
                                ctx.push_frame();
                                if bind_slot.is_some() {
                                    ctx.define_local(bind_name, Value::Str(message));
                                }
                                // The body that failed may have written variables; the
                                // catch body reads them through the frames.
                                vm_flush_slots(prog, &mut values, ctx);
                                let caught = vm_run(None, None, &prog.subs[*catch_sub as usize], ctx);
                                vm_pull_slots(prog, &mut values, ctx);
                                ctx.pop_frame();
                                caught?
                            }
                        };
                        match returned {
                            // A `give` inside the block leaves the program the same way
                            // one outside it does.
                            Some(v) => {
                                if let Some((value, flag)) = prog.return_slot {
                                    values[value as usize] = v;
                                    values[flag as usize] = Value::Bool(true);
                                }
                                ip = prog.instrs.len();
                            }
                            None => ip += 1,
                        }
                    }
                    VmInstr::JumpIfFalse(target) => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        ip = if stack[sp].to_bool() { ip + 1 } else { *target };
                    }
                    VmInstr::JumpIfTrue(target) => {
                        sp = sp.checked_sub(1).ok_or_else(vm_underflow)?;
                        ip = if stack[sp].to_bool() { *target } else { ip + 1 };
                    }
                    VmInstr::Jump(target) => {
                        ip = *target;
                    }
                    VmInstr::StopLoop { flag, target } => {
                        // The jump skips the rest of this pass; the flag tells the
                        // runner there is nothing left to run passes for.
                        values[*flag as usize] = Value::Bool(true);
                        ip = *target;
                    }
                    VmInstr::NextLoop { target } => {
                        ip = *target;
                    }
                    VmInstr::ResetLoop { slot, target } => {
                        values[*slot as usize] = Value::Int(0);
                        ip = *target;
                    }
                    VmInstr::CheckLimit { slot, msg } => {
                        let count = match &values[*slot as usize] {
                            Value::Int(i) => *i,
                            _ => 0,
                        };
                        if count > LOOP_MAX_ITERS as i64 {
                            return Err(msg.clone());
                        }
                        ip += 1;
                    }
                    VmInstr::BumpSlot(slot) => {
                        let idx = *slot as usize;
                        let next = match &values[idx] {
                            Value::Int(i) => i + 1,
                            _ => 1,
                        };
                        values[idx] = Value::Int(next);
                        ip += 1;
                    }
                }
            }
            // A `stop` in a body the runner drives cannot leave by a jump — the
            // runner would simply start the next pass — so the flag it raised is
            // read here. A stream's `stop` jumps to the loop's exit instead, and
            // `stop_flag` is `None` for one.
            if let Some(f) = stop_flag {
                if values[f as usize].to_bool() {
                    break;
                }
            }
        }
        Ok(())
    })();
    // Write back even when the loop failed part-way: the tree-walker applies each
    // assignment as it goes, so iterations that completed must keep their effect.
    // `set_var` is the same call a plain assignment makes, so scope resolution
    // (nearest frame, else globals) is identical. Scratch slots name nothing and
    // the loop's own bindings (`Reps`, the `for` item) die with the loop frame, so
    // neither is written anywhere.
    for (i, name) in prog.slots.iter().enumerate() {
        if name.is_empty() || prog.owned.contains(&(i as u16)) {
            continue;
        }
        ctx.set_var(name, values[i].clone());
    }
    outcome?;
    Ok(match prog.return_slot {
        Some((value, flag)) if values[flag as usize].to_bool() => {
            Some(values[value as usize].clone())
        }
        _ => None,
    })
}

/// `x += value` for everything the integer fast path does not answer: strings,
/// lists and dicts grow in place, an `Object` operand runs an operator method, and
/// anything else goes through `eval_binary`.
///
/// Deliberately out of line. The instruction arm that calls it is in the dispatch
/// loop, and every byte of that loop is paid for by every instruction of every
/// compiled body; the integer case — which is the one that has to be fast — is
/// decided in the arm itself, before this is called.
fn vm_assign_slot(
    prog: &VmProgram,
    values: &mut [Value],
    idx: usize,
    op: BinaryOp,
    rhs: Value,
    line: usize,
    ctx: &mut ExecContext<'_>,
) -> Result<(), String> {
    let prefixed = |e: String| format!("Line {line}: {e}");
    if matches!(values[idx], Value::Object { .. }) || matches!(rhs, Value::Object { .. }) {
        // An operator method is user code: it sees the variable as it stands, and its
        // result becomes the variable's value. The operand is cloned rather than
        // moved out of the slot so that a failing method leaves the variable exactly
        // as the tree-walker leaves it.
        let lhs = values[idx].clone();
        vm_flush_slots(prog, values, ctx);
        let result = eval_binary(op.as_str(), lhs, rhs, ctx);
        vm_pull_slots(prog, values, ctx);
        values[idx] = result.map_err(prefixed)?;
        return Ok(());
    }
    if op == BinaryOp::Add {
        // `+=` on a string, list or dict grows the slot's own buffer, exactly as the
        // tree-walker's in-place arm does.
        return vm_concat_slot(values, idx, rhs, ctx).map_err(prefixed);
    }
    // Everything else has no in-place form; the value is moved out (a move, so a
    // container is not copied) and put back.
    let lhs = std::mem::replace(&mut values[idx], Value::Empty);
    values[idx] = eval_binary(op.as_str(), lhs, rhs, ctx).map_err(prefixed)?;
    Ok(())
}

fn vm_overflow() -> String {
    // Unreachable for a body that compiled: `vm_verify_stack` checks the emitted
    // program's operand stack before it runs.
    "internal error: VM operand stack overflow".to_string()
}

fn vm_underflow() -> String {
    "internal error: VM operand stack underflow".to_string()
}

/// The operand-stack effect of one instruction: (consumed, produced).
///
/// Every variant is listed, including the ones with no effect, so that adding an
/// instruction is a compile error until its effect is written down here.
fn vm_instr_stack_effect(instr: &VmInstr) -> (usize, usize) {
    match instr {
        VmInstr::Const(_)
        | VmInstr::LoadSlot(_)
        | VmInstr::Interpolate(_)
        | VmInstr::CallOrRead { .. }
        | VmInstr::MemberSlot { .. } => (0, 1),
        VmInstr::StoreSlot(_)
        | VmInstr::Pop
        | VmInstr::Bin(_)
        | VmInstr::JumpIfFalse(_)
        | VmInstr::JumpIfTrue(_)
        | VmInstr::AppendSlot { .. }
        | VmInstr::ConcatSlot(_)
        | VmInstr::AssignOpSlot { .. }
        | VmInstr::CheckCount { .. }
        | VmInstr::IterPrepare { .. }
        | VmInstr::DefVarSlot { .. }
        | VmInstr::Return { .. } => (1, 0),
        VmInstr::IndexStoreSlot { .. } => (2, 0),
        VmInstr::Unary(_) | VmInstr::ToBool | VmInstr::MemberGet(_) | VmInstr::IndexSlot(_) => (1, 1),
        VmInstr::Jump(_) | VmInstr::JumpIfFalseKeep(_) | VmInstr::JumpIfTrueKeep(_) => (0, 0),
        VmInstr::MakeList(n) => (*n as usize, 1),
        VmInstr::MakeDict(n) => (*n as usize * 2, 1),
        VmInstr::Index => (2, 1),
        VmInstr::Slice => (4, 1),
        VmInstr::SliceSlot(_) => (3, 1),
        VmInstr::MethodCall { argc, .. } => (*argc as usize + 1, 1),
        VmInstr::PlaceMethodCall { argc, .. } => (*argc as usize, 1),
        VmInstr::Call { argc, .. } => (*argc as usize, 1),
        VmInstr::CheckLimit { .. }
        | VmInstr::BumpSlot(_)
        | VmInstr::PushFrame
        | VmInstr::PopFrame
        | VmInstr::SetLocal { .. }
        | VmInstr::IterNext { .. }
        | VmInstr::EndBody { .. }
        | VmInstr::StopLoop { .. }
        | VmInstr::NextLoop { .. }
        | VmInstr::ResetLoop { .. }
        | VmInstr::TryBlock { .. } => (0, 0),
    }
}

/// Check that a compiled program's operand stack stays inside `VM_STACK` on every
/// path, and is empty when it ends.
///
/// This walks the **emitted instructions**, not the expression AST. A bound kept
/// as a walk over the AST has to be updated by hand for every new node *and* for
/// every path that emits instructions without going through the expression
/// compiler — and the paths that were missed (statement calls, the append and
/// concat argument lists) let a body through the gate that then overflowed at run
/// time. Worse, the same shape of gate refused the index, slice, member and
/// method forms outright when it was not extended with them, so a whole stage of
/// expression support compiled and silently never ran. The instruction stream
/// cannot be bypassed: whatever emits it, this checks it.
///
/// A refusal sends the body to the tree-walker, which is always correct.
fn vm_verify_stack(prog: &VmProgram) -> bool {
    // The depth each instruction is entered with, once some path has reached it.
    let mut entry: Vec<Option<usize>> = vec![None; prog.instrs.len()];
    let mut work: Vec<(usize, usize)> = vec![(0, 0)];

    while let Some((mut ip, mut depth)) = work.pop() {
        while ip < prog.instrs.len() {
            match entry[ip] {
                // Seen before by another path: the two must agree, or this
                // instruction would read an operand one path never pushed.
                Some(seen) if seen != depth => return false,
                Some(_) => break,
                None => entry[ip] = Some(depth),
            }

            let (consumed, produced) = vm_instr_stack_effect(&prog.instrs[ip]);
            if depth < consumed {
                return false;
            }
            depth = depth - consumed + produced;
            if depth > VM_STACK {
                return false;
            }

            match &prog.instrs[ip] {
                // A conditional's target continues from the same depth.
                VmInstr::Jump(target)
                | VmInstr::JumpIfFalse(target)
                | VmInstr::JumpIfTrue(target)
                | VmInstr::JumpIfFalseKeep(target)
                | VmInstr::JumpIfTrueKeep(target) => work.push((*target, depth)),
                VmInstr::IterNext { end, .. } => work.push((*end, depth)),
                // A `stop`, `next` or `reset` continues from the same depth too:
                // all three are jumps that happen to name a loop rather than a
                // condition.
                VmInstr::StopLoop { target, .. }
                | VmInstr::NextLoop { target }
                | VmInstr::ResetLoop { target, .. } => work.push((*target, depth)),
                // A `give` leaves the stream outright, so what follows in the same
                // block is unreachable rather than a merge point.
                VmInstr::Return { .. } => break,
                _ => {}
            }
            ip += 1;
        }
    }
    true
}

fn exec_repeat(mode: &RepeatMode, body: &[Stmt], ctx: &mut ExecContext<'_>) -> Result<Control, String> {
    ctx.push_frame();
    let out = exec_repeat_framed(mode, body, ctx);
    ctx.pop_frame();
    out
}

fn exec_repeat_framed(mode: &RepeatMode, body: &[Stmt], ctx: &mut ExecContext<'_>) -> Result<Control, String> {
    let max_iters = LOOP_MAX_ITERS;

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
            // Stage 1/2: if the body compiles and every name it reads is bound,
            // run the whole loop as instructions over slot indices. `None` from
            // any of these puts the loop back on the tree-walker below.
            if vm_enabled() {
                if let Some(prog) = vm_compile_body(body, ctx.rt) {
                    if vm_names_bound(&prog, ctx) {
                        vm_run_loop(n, &prog, ctx)?;
                        return Ok(Control::None);
                    }
                }
            }
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
            if vm_enabled() {
                if let Some(prog) = vm_compile_for("Item", body, ctx.rt) {
                    if vm_names_bound(&prog, ctx) {
                        // The iterable is evaluated once, before the loop, in both
                        // engines — and materialised here rather than left to the
                        // tree-walker, so a side effect in the expression cannot
                        // happen twice.
                        let iterable = eval_expr(expr, ctx)?;
                        let items = iterable_items_owned(iterable).ok_or_else(|| {
                            "Repeat for expects a list, set, or generator".to_string()
                        })?;
                        vm_run_for(items, &prog, ctx)?;
                        return Ok(Control::None);
                    }
                }
            }
            let iterable = eval_expr(expr, ctx)?;
            let items = match iterable_items_owned(iterable) {
                Some(v) => v,
                None => return Err("Repeat for expects a list, set, or generator".to_string()),
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
            if vm_enabled() {
                if let Some(prog) = vm_compile_for(item_name, body, ctx.rt) {
                    if vm_names_bound(&prog, ctx) {
                        let iterable = eval_expr(iterable_expr, ctx)?;
                        let items = iterable_items_owned(iterable).ok_or_else(|| {
                            "Repeat for <Item> in expects a list, set, or generator".to_string()
                        })?;
                        vm_run_for(items, &prog, ctx)?;
                        return Ok(Control::None);
                    }
                }
            }
            let iterable = eval_expr(iterable_expr, ctx)?;
            let items = match iterable_items_owned(iterable) {
                Some(v) => v,
                None => return Err("Repeat for <Item> in expects a list, set, or generator".to_string()),
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
            if vm_enabled() {
                if let Some(prog) = vm_compile_while(cond_expr, false, body, ctx.rt) {
                    if vm_names_bound(&prog, ctx) {
                        vm_run_stream(&prog, ctx)?;
                        return Ok(Control::None);
                    }
                }
            }
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
            if vm_enabled() {
                if let Some(prog) = vm_compile_while(cond_expr, true, body, ctx.rt) {
                    if vm_names_bound(&prog, ctx) {
                        vm_run_stream(&prog, ctx)?;
                        return Ok(Control::None);
                    }
                }
            }
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

/// Runs one pass of a loop body. The loop's frame is pushed once by
/// `exec_repeat`, so binding `Reps` and the item variable updates the existing
/// entries in place instead of allocating a fresh key on every iteration.
fn run_loop_iteration(
    body: &[Stmt],
    ctx: &mut ExecContext<'_>,
    reps: usize,
    item: Option<(&str, Value)>,
) -> Result<Control, String> {
    ctx.define_local("Reps", Value::Int(reps as i64));
    if let Some((name, value)) = item {
        ctx.define_local(name, value);
    }
    exec_block(body, ctx)
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
        Arc::make_mut(&mut ctx.rt.callables)
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
            // Also expose the module under its own name, so a qualified call
            // (`mod.Fn ...`) keeps working next to the imported symbol, and so
            // namespace objects holding `Value::Func` members can resolve back
            // to the module that defined them.
            if !ctx.rt.modules.contains_key(module_name) {
                ctx.rt
                    .modules
                    .insert(module_name.to_string(), module.clone());
            }
            match module.entries.get(symbol) {
                Some(ModuleEntry::Func(_)) => {
                    let callable = Callable::External {
                        module: module.clone(),
                        name: symbol.to_string(),
                    };
                Arc::make_mut(&mut ctx.rt.callables).insert(bind, callable);
                }
                Some(ModuleEntry::Value(v)) => {
                    ctx.rt.vars.insert(bind, v.clone());
                }
                _ => {
                    return Err(format!(
                        "Module '{}' does not export '{}'",
                        module_name, symbol
                    ));
                }
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

    // Circular-import guard: if this exact file is already being loaded further
    // up the stack, report a clean error instead of recursing until the stack
    // overflows (Python raises ImportError for this).
    {
        let mut loading = ctx.rt.module_loading.lock().unwrap();
        if loading.contains(&canonical) {
            let name = canonical
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| module_name.to_string());
            return Err(format!(
                "Circular import detected: '{name}' imports itself (directly or through another module). Break the cycle by moving the shared code into a third module."
            ));
        }
        loading.insert(canonical.clone());
    }

    let mut module_runtime = Runtime::with_cache_and_loading(
        canonical
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf(),
        ctx.rt.module_cache.clone(),
        ctx.rt.module_loading.clone(),
        ctx.rt.expr_cache.clone(),
    );
    let run_result = module_runtime.run_file(&canonical);

    // Always clear the in-progress marker, even on failure, so a later import
    // can retry rather than being permanently blocked.
    ctx.rt.module_loading.lock().unwrap().remove(&canonical);
    run_result?;

    let mut module_runtime = module_runtime;
    let mut entries = Map::with_capacity_and_hasher(
        module_runtime.vars.len() + module_runtime.funcs.len() + module_runtime.callables.len(),
        Default::default(),
    );
    // Insertion order sets the precedence documented on `ModuleEntry`: variables
    // first, then imports, then the module's own functions last so they win.
    for (name, value) in module_runtime.vars.drain() {
        entries.insert(name, ModuleEntry::Value(value));
    }
    for (name, callable) in Arc::make_mut(&mut module_runtime.callables).drain() {
        entries.insert(name, ModuleEntry::Imported(callable));
    }
    for (name, f) in Arc::make_mut(&mut module_runtime.funcs).drain() {
        entries.insert(name, ModuleEntry::Func(f));
    }
    let module = Arc::new(ModuleInstance { entries });

    {
        let mut cache = ctx.rt.module_cache.lock().unwrap();
        // Drop the whole cache on overflow rather than trimming it: a `HashMap`
        // has no notion of "oldest", and a cleared cache costs a reload, never
        // correctness. A program that reaches 500 distinct modules is either
        // generating them or running a long-lived plugin host, and in both cases
        // a bounded cache that occasionally reloads beats unbounded growth.
        if cache.len() >= MODULE_CACHE_MAX {
            cache.clear();
        }
        cache.insert(canonical, module.clone());
    }
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
    if safe_mode() {
        let lc = callee.to_ascii_lowercase();
        let dangerous = lc.starts_with("py.")
            || lc.starts_with("os_") || lc.starts_with("http") || lc.starts_with("ws_")
            || lc.starts_with("python_") || lc.starts_with("file_") || lc.starts_with("sqlite")
            || lc.starts_with("csv_") || lc.starts_with("future_") || lc.starts_with("task_")
            || lc.starts_with("process_") || lc.starts_with("zip_")
            || matches!(lc.as_str(), "spawn" | "parallel" | "coop" | "run_file"
                | "http_serve" | "http_serve_dir" | "ask" | "gui_show_html" | "sleep"
                | "glob" | "walk" | "process_exit");
        if dangerous {
            return Err(format!("'{callee}' is not allowed in safe mode"));
        }
    }
    // Sized from the argument list: `vec![]` grows 1 -> 4 -> 8, so a 5-argument
    // call reallocated twice. `with_capacity(0)` still does not allocate, so the
    // no-argument case is unchanged.
    let mut positional: Vec<Value> = Vec::with_capacity(args.len());
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
        return Ok(instantiate_class(&class_def, &positional, ctx.rt));
    }

    // Object `len` special method (natural name).
    if callee == "len" && positional.len() == 1 {
        if let Some(r) = call_object_method_value(&positional[0], "len", &[], ctx) {
            return r;
        }
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

    // Lenient dispatcher — like call_func, but tolerant of the callee's arity.
    // Surplus positional args are dropped and missing ones are filled with
    // `empty` (params that declare a default keep their default). Generic event
    // dispatchers use this so a handler may be written with 1, 2 or 3 params.
    if callee == "call_func_lenient" || callee == "dispatch" {
        if positional.is_empty() {
            return Err("call_func_lenient expects at least 1 argument".to_string());
        }
        let func_name = positional[0].to_string();
        let mut func_args: Vec<Value> = positional[1..].to_vec();

        if named.is_empty() {
            if let Some(result) = invoke_builtin(&func_name, &func_args) {
                return result;
            }
        }

        let callable = resolve_callable(&func_name, ctx)?;
        // Declared positional parameters of the target, when knowable.
        let params: Option<Vec<FunctionParam>> = match &callable {
            Callable::Local(f) => Some(f.params.clone()),
            Callable::External { module, name } => match module.entries.get(name) {
                Some(ModuleEntry::Func(f)) => Some(f.params.clone()),
                _ => None,
            },
            Callable::Builtin(_) => None,
        };
        if let Some(params) = params {
            let is_varargs = params.iter().any(|p| p.is_varargs);
            if !is_varargs {
                if func_args.len() > params.len() {
                    func_args.truncate(params.len());
                } else if func_args.len() < params.len() {
                    // Pad only up to the first param that has a default — that
                    // param (and everything after it) can supply its own value.
                    let mut i = func_args.len();
                    while i < params.len() && params[i].default_value.is_none() {
                        func_args.push(Value::Empty);
                        i += 1;
                    }
                }
            }
        }
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

    // Handle http_serve — dynamic web server that dispatches each request to a
    // handler function.
    if callee == "http_serve" {
        if positional.len() != 2 {
            return Err("http_serve expects exactly 2 arguments: http_serve(handler, port)".to_string());
        }
        let handler = positional[0].to_string();
        let port = match &positional[1] {
            Value::Int(p) if *p > 0 && *p < 65536 => *p as u16,
            _ => return Err("http_serve: port must be an integer in 1..65535".to_string()),
        };
        return http_serve_builtin(&handler, port, ctx);
    }

    // Handle interpolate builtin — evaluate %var%, ${var}, or {var} in string.
    if callee == "interpolate" {
        if positional.is_empty() {
            return Err("interpolate expects at least 1 argument: interpolate(template, [vars])".to_string());
        }
        if positional.len() == 1 || (positional.len() == 2 && matches!(positional[1], Value::Empty)) {
            let template = positional[0].to_string();
            return Ok(Value::Str(interpolate_string(&template, ctx)));
        }
    }

    // User-defined and imported functions take precedence over builtins
    // (Python-style shadowing), so std library functions like `Upper` or
    // `Append` are not hidden by case-insensitive builtin lookup.
    if let Ok(callable) = resolve_callable(callee, ctx) {
        // An `async fun` call returns a future: schedule it on a background thread.
        if ctx.rt.async_funcs.contains(callee) && matches!(callable, Callable::Local(_)) {
            let task_args: Vec<Value> = positional.to_vec();
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
        Value::ListShared(_) => match method {
            "append" | "push" => Some("append"),
            "extend" | "concat" => Some("extend"),
            "insert" => Some("insert"),
            "unshift" => Some("unshift"),
            "pop" => Some("pop"),
            "shift" => Some("shift"),
            "remove" => Some("remove"),
            "index" | "indexof" => Some("index"),
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
            "contains" | "has" | "includes" => Some("contains"),
            "find" => Some("find"),
            "len" => Some("len"),
            "slice" => Some("slice"),
            "join" => Some("join"),
            "flatten" | "flat" => Some("flatten"),
            "unique" => Some("unique"),
            "chunk" => Some("chunk"),
            "first" => Some("first"),
            "last" => Some("last"),
            _ => None,
        },
        Value::SetShared(_) => match method {
            "add" => Some("set_add"),
            "remove" => Some("set_remove"),
            "contains" | "has" | "includes" => Some("set_contains"),
            "union" => Some("set_union"),
            "intersection" => Some("set_intersection"),
            "difference" => Some("set_difference"),
            "len" => Some("len"),
            "copy" => Some("copy"),
            "clear" => Some("clear"),
            _ => None,
        },
        Value::StrShared(_) => match method {
            "upper" => Some("upper"),
            "lower" => Some("lower"),
            "trim" | "strip" => Some("trim"),
            "lstrip" | "trimstart" | "trim_start" => Some("lstrip"),
            "rstrip" | "trimend" | "trim_end" => Some("rstrip"),
            "capitalize" => Some("capitalize"),
            "title" => Some("title"),
            "swapcase" => Some("swapcase"),
            "replace" => Some("replace"),
            "split" => Some("split"),
            "splitlines" => Some("str_splitlines"),
            "starts_with" | "startswith" => Some("starts_with"),
            "ends_with" | "endswith" => Some("ends_with"),
            "contains" | "has" | "includes" => Some("contains"),
            "find" => Some("find"),
            "index" | "indexof" => Some("index"),
            "copy" => Some("copy"),
            "clear" => Some("clear"),
            "count" => Some("count"),
            "len" => Some("len"),
            "reverse" => Some("reverse"),
            "slice" => Some("slice"),
            "pad_left" | "padleft" | "padstart" => Some("pad_left"),
            "pad_right" | "padright" | "padend" => Some("pad_right"),
            "repeat" => Some("repeat_str"),
            "zfill" => Some("str_zfill"),
            "removeprefix" | "remove_prefix" => Some("str_removeprefix"),
            "removesuffix" | "remove_suffix" => Some("str_removesuffix"),
            "partition" => Some("str_partition"),
            "join" => Some("join"),
            "char_at" | "charat" => Some("char_at"),
            "char_code" | "charcode" | "charcodeat" => Some("ord"),
            _ => None,
        },
        Value::DictShared(_) => match method {
            "keys" => Some("keys"),
            "values" => Some("values"),
            "items" | "entries" => Some("items"),
            "get" => Some("dict_get"),
            "set" => Some("dict_set"),
            "update" => Some("dict_update"),
            "remove" | "pop" => Some("dict_remove"),
            "copy" => Some("copy"),
            "clear" => Some("clear"),
            "has_key" | "contains_key" | "has" => Some("has_key"),
            "contains" | "includes" => Some("contains"),
            "len" => Some("len"),
            _ => None,
        },
        _ => None,
    }
}

fn invoke_object_method(
    method_def: &FunctionDef,
    class: &ClassMeta,
    fields: &[Value],
    args: &[Value],
    ctx: &mut ExecContext<'_>,
) -> Result<(Value, Box<[Value]>), String> {
    // A method body runs in its own runtime so that it sees its fields as
    // globals and does *not* see the caller's locals.
    //
    // Building that runtime used to deep-copy the whole program's class,
    // function, callable and module tables on every call, and to give it an
    // empty `expr_cache` so every expression in the body was re-parsed on every
    // call. Both are now O(1): the tables are lent and reclaimed, and the caches
    // are shared.
    let mut temp_rt = Runtime::bare_sharing(PathBuf::from("."), ctx.rt);
    for (slot, name) in class.field_names.iter().enumerate() {
        temp_rt.vars.insert(name.clone(), fields[slot].clone());
    }
    ctx.rt.lend_tables_to(&mut temp_rt);

    let out = {
        let mut temp_ctx = ExecContext::new(&mut temp_rt);
        temp_ctx.push_frame();

        for (i, param) in method_def.params.iter().enumerate() {
            let val = args.get(i).cloned().unwrap_or(Value::Empty);
            temp_ctx.define_local(&param.name, val);
        }

        match exec_block(&method_def.body, &mut temp_ctx) {
            Ok(Control::Return(v)) => Ok(v),
            Ok(Control::Yield(v)) => Ok(Value::List(vec![v])),
            Ok(Control::None) => Ok(Value::Empty),
            Ok(_) => Err("STOP/NEXT/RESET cannot be used in a method".to_string()),
            Err(e) => Err(e),
        }
    };

    // The body reads and writes its fields as globals of that runtime, so the
    // fields as they stand afterwards are what the caller has to put back on the
    // instance. They are read back in slot order by *moving* them out of the
    // runtime, which is about to be dropped anyway — no clone per field and no
    // map built per call. A name the method invented is not a field and is left
    // behind, which is the rule a write from outside follows too.
    let after: Box<[Value]> = class
        .field_names
        .iter()
        .map(|name| temp_rt.vars.remove(name).unwrap_or(Value::Empty))
        .collect();

    ctx.rt.reclaim_tables_from(temp_rt);
    out.map(|value| (value, after))
}

/// Call an object's method by its natural-language name, if defined.
/// Returns `Some(result)` when `receiver` is an Object that defines `method`
/// (matched case-insensitively), otherwise `None` so callers can fall back to
/// default behavior. Used for special methods like `to_string`, `equals`, `add`.
fn call_object_method_value(
    receiver: &Value,
    method: &str,
    args: &[Value],
    ctx: &mut ExecContext<'_>,
) -> Option<Result<Value, String>> {
    if let Value::Object { class, fields } = receiver {
        // Special methods are looked up on every `say obj`, `+`, `==`, `string()`
        // and `len()` of an object, so this takes the class's map rather than
        // scanning it; a differently-cased call falls back inside `find_method`.
        if let Some(mdef) = class.find_method(method) {
            return Some(
                invoke_object_method(mdef, class, fields, args, ctx).map(|(value, _)| value),
            );
        }
        // dataclass auto-generated defaults (when no user method is defined).
        if class.is_dataclass {
            if method.eq_ignore_ascii_case("to_string") {
                // Declaration order, which is slot order: the fields print the
                // way they were written, and the way a positional argument
                // fills them. This used to sort the names, because a hash map
                // has no order to rely on.
                let parts: Vec<String> = class
                    .field_names
                    .iter()
                    .zip(fields.iter())
                    .map(|(name, value)| format!("{}: {}", name, value))
                    .collect();
                return Some(Ok(Value::Str(format!(
                    "{}({})",
                    class.name,
                    parts.join(", ")
                ))));
            }
            if method.eq_ignore_ascii_case("equals") {
                if let Some(other) = args.first() {
                    if let Value::Object {
                        class: class2,
                        fields: f2,
                    } = other
                    {
                        // Two instances of the same class have the same slots,
                        // so the fields compare position for position.
                        if class2.name != class.name || fields.len() != f2.len() {
                            return Some(Ok(Value::Bool(false)));
                        }
                        return Some(Ok(Value::Bool(
                            fields
                                .iter()
                                .zip(f2.iter())
                                .all(|(a, b)| eq_values(a, b)),
                        )));
                    }
                    return Some(Ok(Value::Bool(false)));
                }
            }
        }
    }
    None
}

fn invoke_object_method_call(
    callee: &str,
    positional: &[Value],
    named: &HashMap<String, Value>,
    ctx: &mut ExecContext<'_>,
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

    // A module is not an object: `mod.func(...)` must go through module
    // dispatch in resolve_callable, never through builtin method mapping.
    if matches!(receiver, Value::Module(_)) {
        return None;
    }

    if !named.is_empty() {
        return Some(Err(format!(
            "Object method call '{}' does not support named arguments",
            callee
        )));
    }

    // The receiver is a variable, so a method that assigns a field can write it
    // back onto the instance the variable holds. Tried before the value path,
    // which cannot: it holds a copy.
    if let Some(result) = invoke_method_on_place(receiver_name, method_name, positional, ctx) {
        return Some(result);
    }

    Some(invoke_method_on_value(receiver, method_name, positional, ctx))
}

/// Run a class method for a receiver that is a *variable*, and put the instance
/// back where it came from.
///
/// A method body sees its fields as globals of a runtime of its own, so a field
/// it assigns — `fun bump` whose body is `x is x + 1` — changed that throwaway
/// runtime and nothing else, and `p.bump()` looked like it did nothing at all.
/// The receiver is moved out of the variable, run against, and moved back, so
/// those writes reach the instance the variable holds. Moving rather than
/// copying is also why this does not pay the instance clone that reading the
/// receiver as a value would.
///
/// `None` means the receiver is not an instance with a class method of that
/// name, so the caller's other dispatch — a function-valued field, a module, a
/// builtin method — is still to be tried, and the receiver is exactly where it
/// was.
fn invoke_method_on_place(
    receiver_name: &str,
    method: &str,
    args: &[Value],
    ctx: &mut ExecContext<'_>,
) -> Option<Result<Value, String>> {
    // Look the method up on the instance as it stands, before taking it apart.
    // `find_method` matches case-insensitively itself, so there is no folded copy
    // to build here.
    let method_def = {
        let Value::Object { class, .. } = ctx.get_var_ref(receiver_name)? else {
            return None;
        };
        // `find_method` hits the map for the declared spelling and only scans for
        // a differently-cased call, which is what this loop used to do every time.
        Arc::clone(class.find_method(method)?)
    };

    let Some(Value::Object { class, fields }) = ctx.take_var(receiver_name) else {
        return None;
    };

    let (out, after) = match invoke_object_method(&method_def, &class, &fields, args, ctx) {
        Ok(pair) => pair,
        // The body failed: the instance goes back as it was, because a method
        // that raised cannot be said to have assigned anything.
        Err(e) => {
            ctx.set_var(receiver_name, Value::Object { class, fields });
            return Some(Err(e));
        }
    };

    ctx.set_var(
        receiver_name,
        Value::Object {
            class,
            fields: after.into(),
        },
    );
    Some(Ok(out))
}

fn invoke_method_on_value(
    target_val: Value,
    method: &str,
    args: &[Value],
    ctx: &mut ExecContext<'_>,
) -> Result<Value, String> {
    // Dispatch on the method name case-insensitively, but call sites essentially
    // always spell it canonically (lowercase), so only fold when we must: this
    // used to allocate a folded copy on every method call.
    let owned_lc;
    let method_lc: &str = if method.bytes().any(|b| b.is_ascii_uppercase()) {
        owned_lc = method.to_ascii_lowercase();
        &owned_lc
    } else {
        method
    };

    // 1. Handle Object method dispatch (classes)
    if let Value::Object {
        ref class,
        ref fields,
    } = target_val
    {
        // `find_method` for the same reason as the place path: an exact map hit for
        // the declared spelling, and a case-insensitive match otherwise. This used
        // to be `methods.get(method_lc)`, which could only ever find a method whose
        // declared name was already lowercase. The folded name above is still needed
        // below, because `map_object_method_builtin` takes the canonical form.
        if let Some(method_def) = class.find_method(method) {
            return invoke_object_method(method_def, class, fields, args, ctx).map(|(value, _)| value);
        }
        // A function-valued field behaves as a method of the instance.
        let from_field = class
            .field_index
            .get(method)
            .and_then(|slot| match &fields[*slot as usize] {
                Value::Func(name) => Some(name.clone()),
                _ => None,
            });
        if let Some(fn_name) = from_field {
            let exprs: Vec<Expr> = args.iter().map(|v| match v {
                Value::Int(i) => Expr::Int(*i),
                Value::Float(f) => Expr::Float(*f),
                Value::Bool(b) => Expr::Bool(*b),
                Value::StrShared(s) => Expr::Str(s.to_string()),
                Value::Empty => Expr::Empty,
                _ => Expr::Str(v.to_string()),
            }).collect();
            let mut temp_rt = Runtime::bare_sharing(PathBuf::from("."), ctx.rt);
            for (slot, name) in class.field_names.iter().enumerate() {
                temp_rt.vars.insert(name.clone(), fields[slot].clone());
            }
            ctx.rt.lend_tables_to(&mut temp_rt);
            let out = invoke_callable_expr(&fn_name, &exprs, &mut ExecContext::new(&mut temp_rt));
            ctx.rt.reclaim_tables_from(temp_rt);
            return out;
        }
        return Err(format!(
            "Object of class '{}' has no method '{method}'",
            class.name
        ));
    }

    // 2. Dict with function property (JS-style objects)
    if let Value::DictShared(ref map) = target_val {
        if let Some(Value::Func(fn_name)) = map.get(method) {
            let exprs: Vec<Expr> = args.iter().map(|v| match v {
                Value::Int(i) => Expr::Int(*i),
                Value::Float(f) => Expr::Float(*f),
                Value::Bool(b) => Expr::Bool(*b),
                Value::StrShared(s) => Expr::Str(s.to_string()),
                Value::Empty => Expr::Empty,
                _ => Expr::Str(v.to_string()),
            }).collect();
            let mut temp_rt = Runtime::bare_sharing(PathBuf::from("."), ctx.rt);
            temp_rt.vars = dict_to_map(map);
            ctx.rt.lend_tables_to(&mut temp_rt);
            let out = invoke_callable_expr(fn_name, &exprs, &mut ExecContext::new(&mut temp_rt));
            ctx.rt.reclaim_tables_from(temp_rt);
            return out;
        }
    }

    // 2b. Module function invocation
    if let Value::Module(ref m) = target_val {
        if let Some(ModuleEntry::Func(func_def)) = m.entries.get(method) {
            let mut temp_rt = Runtime::bare_sharing(PathBuf::from("."), ctx.rt);
            // The module's variables seed the temporary runtime; a function or an
            // import sharing a name is not one, and the old `vars` map did not
            // hold it either.
            temp_rt.vars = m
                .entries
                .iter()
                .filter_map(|(k, e)| match e {
                    ModuleEntry::Value(v) => Some((k.clone(), v.clone())),
                    _ => None,
                })
                .collect();
            ctx.rt.lend_tables_to(&mut temp_rt);
            let out = invoke_function(
                func_def,
                args,
                &HashMap::new(),
                &mut ExecContext::new(&mut temp_rt),
            );
            ctx.rt.reclaim_tables_from(temp_rt);
            return out;
        }
        return Err(format!("Module has no function '{method}'"));
    }

    // 3. Built-in mapped methods on List, Str, Dict, Set
    let builtin_name = match map_object_method_builtin(&target_val, method_lc) {
        Some(name) => name,
        None => {
            return Err(format!(
                "Unsupported method '{}' for {} value",
                method, target_val.type_name()
            ));
        }
    };

    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(target_val);
    call_args.extend(args.iter().cloned());

    match invoke_builtin(builtin_name, &call_args) {
        Some(result) => result,
        None => Err(format!(
            "Internal error: mapped method '{}' is not implemented",
            builtin_name
        )),
    }
}

/// Apply a unary operator, with the messages the tree-walker produces.
///
/// Shared with `eval_ast` so the compiled and tree-walking engines cannot drift:
/// an unsupported operator or a wrong operand type has to report the same thing
/// whichever engine ran the body.
fn apply_unary(op: &str, v: Value) -> Result<Value, String> {
    match op {
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

/// Read a member without consuming the container. Everything the by-value
/// version does except the copy of the container itself, so a compiled
/// `xs.length` in a loop does not clone the list to count it.
fn resolve_member_access_ref(target: &Value, field: &str) -> Result<Value, String> {
    match target {
        Value::DictShared(map) => {
            if let Some(v) = map.get(field) {
                Ok(v.clone())
            } else if field == "length" || field == "len" || field == "size" {
                Ok(Value::Int(map.len() as i64))
            } else {
                Err(format!("Dictionary has no key '{field}'"))
            }
        }
        Value::Object { class, fields } => {
            if let Some(slot) = class.field_index.get(field) {
                Ok(fields[*slot as usize].clone())
            } else {
                Err(format!("Object has no field '{field}'"))
            }
        }
        Value::Module(m) => {
            // A variable is read; an own function's name is handed back as a
            // `Value::Func`, which is what the old `funcs` check produced. An
            // imported name is neither — it used to fall through to the error.
            match m.entries.get(field) {
                Some(ModuleEntry::Value(v)) => Ok(v.clone()),
                Some(ModuleEntry::Func(_)) => Ok(Value::Func(field.to_string())),
                _ => Err(format!("Module has no attribute '{field}'")),
            }
        }
        Value::ListShared(items) => {
            if field == "length" || field == "len" {
                Ok(Value::Int(items.len() as i64))
            } else if field == "first" {
                items.first().cloned().ok_or_else(|| "List is empty".to_string())
            } else if field == "last" {
                items.last().cloned().ok_or_else(|| "List is empty".to_string())
            } else {
                Err(format!("List has no property '{field}'"))
            }
        }
        Value::StrShared(s) => {
            if field == "length" || field == "len" {
                Ok(Value::Int(s.chars().count() as i64))
            } else {
                Err(format!("String has no property '{field}'"))
            }
        }
        Value::SetShared(s) => {
            if field == "length" || field == "len" || field == "size" {
                Ok(Value::Int(s.len() as i64))
            } else {
                Err(format!("Set has no property '{field}'"))
            }
        }
        _ => Err(format!("Cannot access '.{field}' on {} value", target.type_name())),
    }
}

fn resolve_member_access(target_val: Value, field: &str, _ctx: &mut ExecContext<'_>) -> Result<Value, String> {
    resolve_member_access_ref(&target_val, field)
}

fn invoke_callable_value(
    callee_val: &Value,
    args: &[Value],
    ctx: &mut ExecContext<'_>,
) -> Result<Value, String> {
    match callee_val {
        Value::Func(name) => {
            let exprs: Vec<Expr> = args.iter().map(|v| match v {
                Value::Int(i) => Expr::Int(*i),
                Value::Float(f) => Expr::Float(*f),
                Value::Bool(b) => Expr::Bool(*b),
                Value::StrShared(s) => Expr::Str(s.to_string()),
                Value::Empty => Expr::Empty,
                _ => Expr::Str(v.to_string()),
            }).collect();
            invoke_callable_expr(name, &exprs, ctx)
        }
        _ => Err(format!("Value of type {} is not callable", callee_val.type_name())),
    }
}

fn resolve_callable(callee: &str, ctx: &ExecContext<'_>) -> Result<Callable, String> {
    if let Some((left, right)) = callee.split_once('.') {
        let module = ctx
            .get_module(left)
            .ok_or_else(|| format!("Undefined module '{}'", left))?;
        if matches!(module.entries.get(right), Some(ModuleEntry::Func(_))) {
            return Ok(Callable::External {
                module,
                name: right.to_string(),
            });
        }
        return Err(format!("Module '{}' has no function '{}'", left, right));
    }

    // The module whose top level is running comes first: its own functions and its
    // own imports shadow the caller's, which is what the injected symbol table used
    // to do — and it is why a module function can call its siblings at all.
    for module in ctx.module_scope.iter().rev() {
        match module.entries.get(callee) {
            Some(ModuleEntry::Func(_)) => {
                return Ok(Callable::External {
                    module: Arc::clone(module),
                    name: callee.to_string(),
                })
            }
            Some(ModuleEntry::Imported(callable)) => return Ok(callable.clone()),
            _ => {}
        }
    }

    if let Some(callable) = ctx.rt.callables.get(callee) {
        return Ok(callable.clone());
    }

    // Fallback: a plain name that no local/imported symbol provides may still be
    // exported by an imported module. This is what makes namespace objects
    // work — e.g. `get AI from ai` where `ai.ind` exposes
    // `var AI dynamic = {"SetBase": SetBase, ...}` and the caller writes
    // `AI.SetBase url`. Module order is sorted so resolution is deterministic.
    let mut module_names: Vec<&String> = ctx.rt.modules.keys().collect();
    module_names.sort();
    for name in module_names {
        if let Some(module) = ctx.rt.modules.get(name) {
            if matches!(module.entries.get(callee), Some(ModuleEntry::Func(_))) {
                return Ok(Callable::External {
                    module: module.clone(),
                    name: callee.to_string(),
                });
            }
        }
    }

    Err(format!("Undefined function '{}'", callee))
}

fn invoke_external_function(
    module: Arc<ModuleInstance>,
    name: &str,
    positional: &[Value],
    named: &HashMap<String, Value>,
    ctx: &mut ExecContext<'_>,
) -> Result<Value, String> {
    let f = match module.entries.get(name) {
        Some(ModuleEntry::Func(f)) => Arc::clone(f),
        _ => return Err(format!("Module function '{}' not found", name)),
    };

    // The body runs in its module's scope, which is what used to be achieved by
    // injecting the module's whole symbol table into the caller's runtime and
    // removing it again afterwards: one hash insert per exported name, per call.
    // A scope stack entry is a refcount bump, and lookups consult the module only
    // when they have already missed the frames and the caller's globals — the same
    // precedence the injection produced.
    ctx.module_scope.push(module);
    let result = invoke_function(&f, positional, named, ctx);
    ctx.module_scope.pop();
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
            let expected: Vec<&str> = f.params.iter().map(|p| p.name.as_str()).collect();
            return Err(format!(
                "Too many positional arguments: expected at most {}, got {} (parameters: {})",
                f.params.len(),
                positional.len(),
                expected.join(", ")
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
            let raw_value = if !named.is_empty() {
                if let Some(v) = named.get(&param.name) {
                    v.clone()
                } else if let Some(v) = positional.get(idx) {
                    v.clone()
                } else if let Some(default_expr) = &param.default_value {
                    eval_expr(default_expr, ctx).unwrap_or(Value::Empty)
                } else {
                    ctx.pop_frame();
                    return Err(format!("Missing argument for parameter '{}'", param.name));
                }
            } else if let Some(v) = positional.get(idx) {
                // Fast path: the overwhelmingly common case — no named args, so
                // skip the hash lookup entirely.
                v.clone()
            } else if let Some(default_expr) = &param.default_value {
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

    // Bind the positional arguments once, as a list. `argument` / `argument2` …
    // are resolved from this list on demand (see `get_var`), which avoids a
    // String allocation and a map insert per argument per call. The list comes
    // from the recycled frame, so this normally allocates nothing.
    if let Some(frame) = ctx.frames.last_mut() {
        frame.args.clear();
        frame
            .args
            .extend(positional.iter().cloned());
    }

    for (k, v) in named {
        ctx.define_local(k, v.clone());
    }

    let output = if f.is_generator {
        // Generator: run the body once, collecting every `yield` into a list,
        // then wrap them in a Generator value that `for` can iterate.
        ctx.collect_yields = true;
        ctx.yielded_values = Vec::new();
        let _ = exec_block(&f.body, ctx)?;
        ctx.collect_yields = false;
        let values = std::mem::take(&mut ctx.yielded_values);
        let mut g = HashMap::new();
        g.insert("__generator__".to_string(), Value::Bool(true));
        g.insert("__gen_values__".to_string(), Value::List(values));
        Ok(Value::Dict(g.into()))
    } else {
        match resumed_body(f, ctx)? {
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

/// Run a function body, compiled when the VM can take it and on the tree-walker
/// otherwise.
///
/// The choice is invisible to the caller: the body sees the frame `invoke_function`
/// already built, and reports the same `Control` either way. A body the VM cannot
/// compile — one with `var`, `do:`/`catch`, `yield`, `stop`/`next`, or a statement
/// it has no instruction for — simply runs as it did before.
fn resumed_body(
    f: &FunctionDef,
    ctx: &mut ExecContext<'_>,
) -> Result<Control, String> {
    if vm_enabled() {
        let compiled = f.compiled.get_or_init(|| {
            vm_compile_function(&f.body, f.return_type.is_some(), ctx.rt).map(Arc::new)
        });
        if let Some(prog) = compiled {
            // Binding is per call: the body's names must exist *now*.
            if vm_names_bound(prog, ctx) {
                return vm_run_body(prog, ctx);
            }
        }
    }
    exec_block(&f.body, ctx)
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
    invoke_callable_values(callee, &positional, ctx)
}

/// Invoke `callee` with already-evaluated positional arguments.
///
/// Takes a **slice**, not a `Vec`: the VM's call instruction used to build a fresh
/// vector out of its operand stack for every call, which is an allocation plus an
/// argument clone per call on the hottest path there is. Everything here reads the
/// arguments; the two places that need to own them (the `spawn`/`future` thread
/// boundary) copy explicitly, which is where a copy belongs.
///
/// Split out of `invoke_callable_expr` so a compiled expression reaches exactly
/// the same dispatch — the same prefixes, the same shadowing order, the same
/// error messages — without re-walking its argument expressions. Both engines
/// therefore agree on what a call means by construction.
fn invoke_callable_values(
    callee: &str,
    positional: &[Value],
    ctx: &mut ExecContext<'_>,
) -> Result<Value, String> {
    // Object `len` special method (natural name).
    if callee == "len" && positional.len() == 1 {
        if let Some(r) = call_object_method_value(&positional[0], "len", &[], ctx) {
            return r;
        }
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

    // Handle http_serve — dynamic web server that dispatches each request to a
    // handler function.
    if callee == "http_serve" {
        if positional.len() != 2 {
            return Err("http_serve expects exactly 2 arguments: http_serve(handler, port)".to_string());
        }
        let handler = positional[0].to_string();
        let port = match &positional[1] {
            Value::Int(p) if *p > 0 && *p < 65536 => *p as u16,
            _ => return Err("http_serve: port must be an integer in 1..65535".to_string()),
        };
        return http_serve_builtin(&handler, port, ctx);
    }

    // Handle interpolate builtin — evaluate %var%, ${var}, or {var} in string.
    if callee == "interpolate" {
        if positional.is_empty() {
            return Err("interpolate expects at least 1 argument: interpolate(template, [vars])".to_string());
        }
        if positional.len() == 1 || (positional.len() == 2 && matches!(positional[1], Value::Empty)) {
            let template = positional[0].to_string();
            return Ok(Value::Str(interpolate_string(&template, ctx)));
        }
    }

    // User-defined and imported functions take precedence over builtins
    // (Python-style shadowing) — must match exec_call_inner so expression-level
    // calls to module functions (e.g. `Count(items)` inside a module) are not
    // shadowed by case-insensitive builtin lookup.
    if let Ok(callable) = resolve_callable(callee, ctx) {
        // An `async fun` call returns a future: schedule it on a background thread.
        if ctx.rt.async_funcs.contains(callee) && matches!(callable, Callable::Local(_)) {
            let task_args: Vec<Value> = positional.to_vec();
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
        return Ok(instantiate_class(&class_def, &positional, ctx.rt));
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
        (Value::StrShared(x), Value::StrShared(y)) => x.cmp(y),
        (Value::ListShared(x), Value::ListShared(y)) => {
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
    "capitalize", "char_at", "char_code", "chr", "clamp", "clear", "coalesce", "colored", "contains", "coop", "copy", "count",
    "bg", "bg_gradient", "bg_multicolor", "bg_rainbow", "fg", "gradient", "multicolor",
    "paint", "rainbow", "style",
    "counter", "csv_read", "csv_write", "dec", "default", "dict_get", "dict_items",
    "dict_remove", "dict_set", "dict_update", "div_int", "ends_with", "entries", "enumerate",
    "err", "error_message", "error_type", "extend", "false", "file_append_text",
    "file_read_text", "file_sha256", "file_size", "file_write_text", "filter", "find",
    "flat", "float", "float_or", "format", "from_char_code", "future", "future_cancel", "future_done",
    "future_result", "future_wait_for", "gather", "glob", "group", "gui_show_html",
    "gzip_compress", "gzip_decompress", "hash_sha256", "has_key", "http_delete",
    "http_delete_async", "http_get", "http_get_async", "http_patch_json",
    "http_post_json", "http_post_json_async", "http_put_json", "http_put_json_async",
    "http_serve_dir", "http_serve", "http_request", "http_set_timeout", "http_timeout", "inc", "index", "insert", "int", "int_or", "interpolate",
    "is_err", "is_even", "is_finite", "is_missing", "is_nan", "is_odd", "is_ok", "items", "join",
    "json_dumps", "json_loads", "json_parse", "json_stringify",
    "keys", "len", "log", "lower", "lstrip", "map", "chain", "flatten", "chunk",
    "product", "permutations", "combinations", "accumulate", "cycle", "repeat_item",
    "takewhile", "dropwhile", "unique", "partition", "group_by", "max_key", "min_key",
    "reduce", "zip_longest", "pairwise", "filterfalse", "compress", "starmap", "first", "last",
    "math_abs", "math_acos", "math_asin", "math_atan", "math_atan2", "math_ceil",
    "math_clamp", "math_cos", "math_degrees", "math_e", "math_exp", "math_factorial", "math_floor",
    "math_gcd", "math_hypot", "math_lcm", "math_log", "math_log10", "math_log2",
    "math_pi", "math_pow", "math_radians", "math_round", "math_sign", "math_sin", "math_sqrt",
    "math_tan", "math_tau", "math_trunc", "max", "min", "mod_int", "mul_int", "ok", "ord", "os_chdir",
    "os_copy", "os_copy_tree", "os_environ", "os_exists", "os_getcwd", "os_getenv",
    "os_is_dir", "os_is_file", "os_list_dir", "os_mkdir", "os_move", "os_remove",
    "os_rename", "os_run", "os_run_ok", "os_setenv", "os_system", "os_which", "pad_left", "pad_right",
    "parallel", "path_abs", "path_basename", "path_dirname", "path_expand",
    "path_ext", "path_join", "path_norm", "path_stem", "pop", "print",
    "process_exit", "python_available", "python_eval", "python_eval_json",
    "python_exec", "python_run_file", "random_choice", "random_float", "random_int",
    "random_randint", "random_sample", "random_seed", "random_shuffle", "random_uniform",
    "range", "regex_findall", "regex_match",
    "regex_replace", "regex_search", "regex_split", "remove", "repeat", "repeat_str",
    "replace", "reverse", "rstrip", "run_file", "say", "set", "set_add", "set_contains",
    "set_difference", "set_intersection", "set_remove", "set_union", "sformat",
    "shift", "sign", "sleep", "slice", "sort", "spawn", "split", "sqlite_exec", "sqlite_query",
    "sqlite_query_one", "starts_with", "str", "str_center", "string", "str_ljust",
    "str_partition", "str_removeprefix", "str_removesuffix", "str_rjust",
    "str_splitlines", "str_zfill", "sub_int", "sum", "swapcase", "sys_arch",
    "sys_argv", "sys_executable", "sys_platform", "sys_version", "task_done",
    "task_result", "task_wait", "task_wait_all", "task_wait_timeout", "time_format",
    "time_now", "time_parse", "time_perf_counter", "time_sleep", "time_utc",
    "title", "to_list", "toml_dumps", "toml_loads", "trim", "true", "trunc", "try", "type_of", "typeof",
    "unshift", "is_list", "is_dict", "is_string", "is_number", "is_int", "is_float", "is_bool", "is_group",
    "is_generator", "unwrap", "upper", "uuid", "values", "walk", "ws_close", "ws_connect",
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

/// Dispatch for the color subsystem (Indent 2.0): foreground/background,
/// styles, gradients, multi-color and rainbow text.
fn color_builtin(name: &str, positional: &[Value]) -> Result<Value, String> {
    let resolve = |v: &Value| -> Result<(u8, u8, u8), String> {
        let s = v.to_string();
        color_to_rgb(&s).ok_or_else(|| format!("unknown color '{s}'"))
    };
    let is_bg = name.starts_with("bg_");
    match name {
        "colored" | "colorize" | "fg" => {
            if positional.len() != 2 {
                return Err(format!("{name} expects exactly 2 arguments: {name}(text, color)"));
            }
            let text = positional[0].to_string();
            let (r, g, b) = resolve(&positional[1])?;
            Ok(Value::Str(render_ansi(&text, Some((r, g, b)), None, &[])))
        }
        "bg" => {
            if positional.len() != 2 {
                return Err("bg expects exactly 2 arguments: bg(text, color)".to_string());
            }
            let text = positional[0].to_string();
            let (r, g, b) = resolve(&positional[1])?;
            Ok(Value::Str(render_ansi(&text, None, Some((r, g, b)), &[])))
        }
        "style" => {
            if positional.len() < 2 {
                return Err("style expects at least 2 arguments: style(text, style, ...)".to_string());
            }
            let text = positional[0].to_string();
            let mut codes: Vec<u8> = Vec::new();
            for arg in &positional[1..] {
                match arg {
                    Value::StrShared(s) => {
                        let c = style_code(s)
                            .ok_or_else(|| format!("style: unknown style '{s}'"))?;
                        codes.push(c);
                    }
                    Value::ListShared(items) => {
                        for it in items {
                            if let Value::StrShared(s) = it {
                                let c = style_code(s)
                                    .ok_or_else(|| format!("style: unknown style '{s}'"))?;
                                codes.push(c);
                            } else {
                                return Err("style: list entries must be style names".to_string());
                            }
                        }
                    }
                    _ => return Err("style: arguments must be style names".to_string()),
                }
            }
            Ok(Value::Str(render_ansi(&text, None, None, &codes)))
        }
        "gradient" | "bg_gradient" => {
            if positional.len() != 3 {
                return Err(format!("{name} expects exactly 3 arguments: {name}(text, from, to)"));
            }
            let text = positional[0].to_string();
            let from = resolve(&positional[1])?;
            let to = resolve(&positional[2])?;
            Ok(Value::Str(gradient_text(&text, from, to, is_bg)))
        }
        "multicolor" | "bg_multicolor" => {
            if positional.len() < 2 {
                return Err(format!("{name} expects at least 2 arguments: {name}(text, color, ...)"));
            }
            let text = positional[0].to_string();
            let mut colors: Vec<(u8, u8, u8)> = Vec::new();
            for arg in &positional[1..] {
                match arg {
                    Value::ListShared(items) => {
                        for it in items {
                            colors.push(resolve(it)?);
                        }
                    }
                    _ => colors.push(resolve(arg)?),
                }
            }
            if colors.is_empty() {
                return Err(format!("{name}: at least one color is required"));
            }
            Ok(Value::Str(multicolor_text(&text, &colors, is_bg)))
        }
        "rainbow" | "bg_rainbow" => {
            if positional.len() != 1 {
                return Err(format!("{name} expects exactly 1 argument: {name}(text)"));
            }
            let text = positional[0].to_string();
            Ok(Value::Str(rainbow_text(&text, is_bg)))
        }
        "paint" => {
            if positional.len() != 4 {
                return Err("paint expects exactly 4 arguments: paint(text, fg, bg, style) — pass \"\" to skip fg/bg/style".to_string());
            }
            let text = positional[0].to_string();
            let fg_s = positional[1].to_string();
            let bg_s = positional[2].to_string();
            let mut fg = None;
            let mut bgr = None;
            if !fg_s.is_empty() {
                fg = Some(color_to_rgb(&fg_s).ok_or_else(|| format!("unknown color '{fg_s}'"))?);
            }
            if !bg_s.is_empty() {
                bgr = Some(color_to_rgb(&bg_s).ok_or_else(|| format!("unknown color '{bg_s}'"))?);
            }
            let mut styles: Vec<u8> = Vec::new();
            match &positional[3] {
                Value::StrShared(s) => {
                    if !s.is_empty() {
                        styles.push(style_code(s).ok_or_else(|| format!("style: unknown style '{s}'"))?);
                    }
                }
                Value::ListShared(items) => {
                    for it in items {
                        if let Value::StrShared(s) = it {
                            if !s.is_empty() {
                                styles.push(style_code(s).ok_or_else(|| format!("style: unknown style '{s}'"))?);
                            }
                        } else {
                            return Err("paint: style list entries must be style names".to_string());
                        }
                    }
                }
                _ => {}
            }
            Ok(Value::Str(render_ansi(&text, fg, bgr, &styles)))
        }
        _ => Err(format!("Unknown color builtin '{name}'")),
    }
}

/// Category for a builtin name, used to organize the dict returned by
/// `builtins()`. Falls back to "misc".
fn builtin_category(name: &str) -> &'static str {
    if name.starts_with("math_") { return "math"; }
    if name.starts_with("str_") { return "string"; }
    if name.starts_with("path_") { return "path"; }
    if name.starts_with("os_") || name.starts_with("file_") { return "os/file"; }
    if name.starts_with("http_") || name.starts_with("ws_") { return "http/net"; }
    if name.starts_with("json_") || name.starts_with("toml_") || name.starts_with("yaml_")
        || name.starts_with("sqlite_") || name.starts_with("csv_") { return "data"; }
    if name.starts_with("regex_") { return "text"; }
    if name.starts_with("time_") || name.starts_with("random_") { return "time/random"; }
    if name.starts_with("sys_") { return "system"; }
    if name.starts_with("task_") || name.starts_with("future_") || name.starts_with("async_")
        || matches!(name, "spawn" | "parallel" | "future" | "gather" | "sleep" | "coop" | "wait" | "await") {
        return "async";
    }
    if name.starts_with("error_") { return "errors"; }
    if name.starts_with("python_") { return "interop"; }
    if name.starts_with("base64_") || name.starts_with("hash_") || name == "uuid" { return "crypto"; }
    if name.starts_with("set_") || name == "set" || name == "group" { return "group"; }
    if name.starts_with("dict_") || matches!(name, "keys" | "values" | "items" | "entries" | "has_key") { return "dict"; }
    if matches!(name, "ok" | "err" | "is_ok" | "is_err" | "unwrap" | "try") { return "result"; }
    if matches!(name, "is_list" | "is_dict" | "is_string" | "is_number" | "is_int"
        | "is_float" | "is_bool" | "is_group" | "is_generator" | "is_nan" | "is_finite" | "type_of" | "typeof" | "type_name") {
        return "types";
    }

    match name {
        "upper" | "lower" | "trim" | "lstrip" | "rstrip" | "capitalize" | "title"
        | "swapcase" | "replace" | "split" | "join" | "starts_with" | "ends_with"
        | "contains" | "find" | "slice" | "len" | "reverse" | "format" | "sformat"
        | "repeat_str" | "repeat" | "pad_left" | "pad_right" | "string"
        | "char_at" | "char_code" | "from_char_code" | "chr" | "ord" | "interpolate" => "string",
        "append" | "extend" | "insert" | "pop" | "remove" | "sort" | "enumerate"
        | "zip" | "range" | "count" | "index" | "sum" | "min" | "max" | "any" | "all"
        | "filter" | "map" | "copy" | "clear" | "chain" | "flatten" | "flat" | "chunk"
        | "product" | "permutations" | "combinations" | "accumulate" | "cycle"
        | "repeat_item" | "takewhile" | "dropwhile" | "unique" | "partition" | "group_by"
        | "max_key" | "min_key" | "reduce" | "zip_longest" | "pairwise"
        | "filterfalse" | "compress" | "starmap" | "first" | "last" | "to_list"
        | "shift" | "unshift" => "list",
        "abs" | "int" | "float" | "int_or" | "float_or" | "between_int" | "is_even"
        | "is_odd" | "inc" | "dec" | "add_int" | "sub_int" | "mul_int" | "div_int"
        | "mod_int" | "bool" | "boolean" | "clamp" | "math_clamp" | "sign" | "trunc" | "counter" => "math",
        "say" | "print" | "ask" | "log" | "colored" | "colorize" | "fg" | "bg"
        | "style" | "gradient" | "bg_gradient" | "multicolor" | "bg_multicolor"
        | "rainbow" | "bg_rainbow" | "paint" => "io",
        "assert" | "assert_eq" | "builtins" | "is_missing" | "default" | "coalesce"
        | "process_exit" | "run_file" | "launch" => "misc",
        _ => "misc",
    }
}

fn safe_mode() -> bool {
    SAFE_MODE.load(Ordering::Relaxed)
}

/// Default-deny allowlist for `--safe`: only pure / computational builtins.
/// Anything not listed here is blocked in safe mode.
fn is_safe_builtin(name: &str) -> bool {
    if name.starts_with("math_") || name.starts_with("regex_") || name.starts_with("random_")
        || name.starts_with("str_") || name.starts_with("path_") || name.starts_with("is_")
        || name.starts_with("set_")
    {
        return true;
    }
    matches!(
        name,
        "abs" | "add_int" | "all" | "any" | "append" | "assert" | "assert_eq"
        | "between_int" | "bool" | "boolean" | "builtins" | "capitalize" | "clamp"
        | "char_at" | "char_code" | "chr" | "clear" | "coalesce" | "colored" | "contains" | "copy" | "count" | "counter"
        | "bg" | "bg_gradient" | "bg_multicolor" | "bg_rainbow" | "fg" | "gradient"
        | "multicolor" | "paint" | "rainbow" | "style" | "dec" | "default" | "dict_get"
        | "dict_items" | "dict_remove" | "dict_set" | "dict_update" | "div_int"
        | "ends_with" | "entries" | "enumerate" | "extend" | "false" | "filter" | "find" | "flat" | "float"
        | "float_or" | "format" | "from_char_code" | "group" | "gzip_compress" | "gzip_decompress"
        | "hash_sha256" | "has_key" | "inc" | "index" | "insert" | "int" | "int_or" | "interpolate"
        | "items" | "join" | "json_dumps" | "json_loads" | "json_parse" | "json_stringify" | "keys" | "len" | "lower"
        | "lstrip" | "map" | "chain" | "flatten" | "chunk" | "product" | "permutations"
        | "combinations" | "accumulate" | "cycle" | "repeat_item" | "takewhile"
        | "dropwhile" | "unique" | "partition" | "group_by" | "max_key" | "min_key"
        | "reduce" | "zip_longest" | "pairwise" | "filterfalse" | "compress" | "starmap"
        | "first" | "last" | "max" | "min" | "mod_int" | "mul_int" | "ok" | "ord" | "pad_left"
        | "pad_right" | "pop" | "print" | "range" | "remove" | "repeat" | "repeat_str" | "replace"
        | "reverse" | "rstrip" | "say" | "set" | "sformat" | "shift" | "sign" | "slice" | "sort" | "split"
        | "starts_with" | "str" | "string" | "sub_int" | "sum" | "swapcase" | "title"
        | "to_list" | "toml_dumps" | "toml_loads" | "trim" | "true" | "trunc" | "type_of"
        | "typeof" | "unshift" | "upper" | "values" | "uuid" | "yaml_dumps" | "yaml_loads"
        | "time_format" | "time_parse" | "time_now" | "time_perf_counter" | "time_utc"
        | "sys_arch" | "sys_platform" | "sys_version"
    )
}

fn invoke_builtin(callee: &str, positional: &[Value]) -> Option<Result<Value, String>> {
    // Builtin names are all lowercase and lookup is case-insensitive, but every
    // call site in practice spells them canonically -- so borrow the caller's
    // string and only fold a copy when the case actually differs. The
    // unconditional `to_ascii_lowercase()` here was a heap allocation on every
    // single builtin call.
    let owned;
    let builtin: &str = if callee.bytes().any(|b| b.is_ascii_uppercase()) {
        owned = callee.to_ascii_lowercase();
        &owned
    } else {
        callee
    };
    if safe_mode() && !is_safe_builtin(builtin) {
        return Some(Err(format!("'{builtin}' is not allowed in safe mode")));
    }
    match builtin {
        "builtins" => {
            if !positional.is_empty() {
                return Some(Err("builtins takes no arguments".to_string()));
            }
            // Return an organized dict: category -> [builtin names].
            let mut groups: std::collections::BTreeMap<String, Vec<Value>> = Default::default();
            for n in INDENT_BUILTINS {
                groups
                    .entry(builtin_category(n).to_string())
                    .or_default()
                    .push(Value::Str(n.to_string()));
            }
            let mut out = HashMap::new();
            for (cat, names) in groups {
                out.insert(cat, Value::List(names));
            }
            Some(Ok(Value::Dict(out.into())))
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
        "colored" | "colorize" | "fg" | "bg" | "style" | "gradient"
        | "bg_gradient" | "multicolor" | "bg_multicolor" | "rainbow"
        | "bg_rainbow" | "paint" => Some(color_builtin(builtin, positional)),
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
                Value::StrShared(s) => Value::Int(s.chars().count() as i64),
                Value::ListShared(v) => Value::Int(v.len() as i64),
                Value::SetShared(v) => Value::Int(v.len() as i64),
                Value::DictShared(v) => Value::Int(v.len() as i64),
                _ => return Some(Err(format!("len expects a string, list, set, or dictionary, got {}", positional[0].type_name()))),
            };
            Some(Ok(out))
        }
        // `group` is THE canonical way to build a unique, ordered collection
        // (type_of reports "group"). `set([...])` below is only a deprecated
        // alias kept so older scripts keep working — prefer `group [...]`.
        "group" => {
            if positional.is_empty() || positional.len() > 1 {
                return Some(Err("group expects exactly 1 argument: a list of values".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v.to_vec(),
                Value::SetShared(v) => v.to_vec(),
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
        // Deprecated alias of `group`. `set` is normally the *type-conversion*
        // keyword (`set x string`); use `group([...])` to build a collection.
        "set" => {
            if positional.is_empty() || positional.len() > 1 {
                return Some(Err("set (deprecated alias of group) expects exactly 1 argument: a list of values".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v.to_vec(),
                Value::SetShared(v) => v.to_vec(),  // set of set = dedup again
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
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err("join expects 1 or 2 arguments: join(list, [separator]) or join(separator, list)".to_string()));
            }
            let (items, sep) = if positional.len() == 1 {
                match &positional[0] {
                    Value::ListShared(v) => (v.clone(), String::new()),
                    _ => return Some(Err(format!("join expects a list, got {}", positional[0].type_name()))),
                }
            } else {
                match (&positional[0], &positional[1]) {
                    (Value::ListShared(v), sep_val) => (v.clone(), sep_val.to_string()),
                    (sep_val, Value::ListShared(v)) => (v.clone(), sep_val.to_string()),
                    _ => return Some(Err(format!("join expects a list and a separator, got {} and {}", positional[0].type_name(), positional[1].type_name()))),
                }
            };
            let out = items.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(&sep);
            Some(Ok(Value::Str(out)))
        }
        "chr" | "from_char_code" => {
            if positional.len() != 1 {
                return Some(Err("chr expects exactly 1 argument (code)".to_string()));
            }
            let code = match positional[0].as_number() {
                Some(n) => n as u32,
                None => return Some(Err("chr code must be integer".to_string())),
            };
            match char::from_u32(code) {
                Some(c) => Some(Ok(Value::Str(c.to_string()))),
                None => Some(Err(format!("chr invalid Unicode code point: {code}"))),
            }
        }
        "ord" | "char_code" => {
            if positional.len() != 1 {
                return Some(Err("ord expects exactly 1 argument (string)".to_string()));
            }
            let s = positional[0].to_string();
            match s.chars().next() {
                Some(c) => Some(Ok(Value::Int(c as u32 as i64))),
                None => Some(Err("ord expects non-empty string".to_string())),
            }
        }
        "char_at" => {
            if positional.len() != 2 {
                return Some(Err("char_at expects exactly 2 arguments (string, index)".to_string()));
            }
            let s = positional[0].to_string();
            let idx = match &positional[1] {
                Value::Int(i) => *i,
                _ => return Some(Err("char_at index must be int".to_string())),
            };
            let count = s.chars().count();
            if let Some(norm) = normalize_index(count, idx) {
                let ch = s.chars().nth(norm).unwrap_or('\0');
                Some(Ok(Value::Str(ch.to_string())))
            } else {
                Some(Ok(Value::Str(String::new())))
            }
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
                Value::DictShared(v) => v,
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
                Value::DictShared(v) => v,
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
                Value::DictShared(v) => v,
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
                Value::ListShared(v) => v.clone(),
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
                Some(Ok(Value::ListShared(sorted)))
            }
        }
        "reverse" => {
            if positional.len() != 1 {
                return Some(Err("reverse expects exactly 1 argument (list or string)".to_string()));
            }
            match &positional[0] {
                Value::ListShared(v) => {
                    let mut out = v.to_vec();
                    out.reverse();
                    Some(Ok(Value::List(out)))
                }
                Value::StrShared(s) => {
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
                Value::ListShared(v) => v,
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
                Value::ListShared(v) => v,
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

            let all_strings = items.iter().all(|v| matches!(v, Value::StrShared(_)));
            if all_strings {
                let mut best = match &items[0] {
                    Value::StrShared(s) => s.to_string(),
                    _ => String::new(),
                };
                for item in items.iter().skip(1) {
                    if let Value::StrShared(s) = item {
                        if s.as_str() < best.as_str() {
                            best = s.to_string();
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
                Value::ListShared(v) => v,
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

            let all_strings = items.iter().all(|v| matches!(v, Value::StrShared(_)));
            if all_strings {
                let mut best = match &items[0] {
                    Value::StrShared(s) => s.to_string(),
                    _ => String::new(),
                };
                for item in items.iter().skip(1) {
                    if let Value::StrShared(s) = item {
                        if s.as_str() > best.as_str() {
                            best = s.to_string();
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
                Value::ListShared(v) => v,
                _ => return Some(Err(format!("any expects a list, got {}", positional[0].type_name()))),
            };
            Some(Ok(Value::Bool(items.iter().any(|v| v.to_bool()))))
        }
        "all" => {
            if positional.len() != 1 {
                return Some(Err("all expects exactly 1 argument (list)".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v,
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
                Value::ListShared(items) => {
                    let count = items
                        .iter()
                        .filter(|v| eq_values(v, &positional[1]))
                        .count() as i64;
                    Some(Ok(Value::Int(count)))
                }
                Value::StrShared(text) => {
                    let needle = match &positional[1] {
                        Value::StrShared(s) => s,
                        _ => {
                            return Some(Err(
                                "count on string expects string value".to_string(),
                            ))
                        }
                    };
                    if needle.is_empty() {
                        Some(Ok(Value::Int(text.chars().count() as i64 + 1)))
                    } else {
                        Some(Ok(Value::Int(text.matches(needle.as_str()).count() as i64)))
                    }
                }
                Value::DictShared(map) => {
                    let key = match &positional[1] {
                        Value::StrShared(s) => s,
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
                Value::ListShared(v) => v.to_vec(),
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
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err(format!("extend expects a list, got {}", positional[0].type_name()))),
            };
            let extra = match &positional[1] {
                Value::ListShared(v) => v.to_vec(),
                _ => return Some(Err(format!("extend expects a list, got {}", positional[1].type_name()))),
            };
            out.extend(extra.clone());
            Some(Ok(Value::ListShared(out)))
        }
        "insert" => {
            if positional.len() != 3 {
                return Some(Err(
                    "insert expects exactly 3 arguments: insert(list, index, value)".to_string(),
                ));
            }

            let mut out = match &positional[0] {
                Value::ListShared(v) => v.clone(),
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
            Some(Ok(Value::ListShared(out)))
        }
        "pop" => {
            if positional.is_empty() || positional.len() > 2 {
                return Some(Err(
                    "pop expects 1 or 2 arguments: pop(list, [index])".to_string(),
                ));
            }

            let mut out = match &positional[0] {
                Value::ListShared(v) => v.clone(),
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
            result.insert("list".to_string(), Value::ListShared(out));
            Some(Ok(Value::Dict(result.into())))
        }
        "unshift" => {
            if positional.len() != 2 {
                return Some(Err("unshift expects exactly 2 arguments: unshift(list, item)".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v,
                _ => return Some(Err(format!("unshift expects a list, got {}", positional[0].type_name()))),
            };
            let mut out = Vec::with_capacity(items.len() + 1);
            out.push(positional[1].clone());
            out.extend(items.iter().cloned());
            Some(Ok(Value::List(out)))
        }
        "shift" => {
            if positional.len() != 1 {
                return Some(Err("shift expects exactly 1 argument: shift(list)".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v,
                _ => return Some(Err(format!("shift expects a list, got {}", positional[0].type_name()))),
            };
            if items.is_empty() {
                Some(Ok(Value::List(Vec::new())))
            } else {
                Some(Ok(Value::List(items[1..].to_vec())))
            }
        }
        "remove" => {
            if positional.len() != 2 {
                return Some(Err(
                    "remove expects exactly 2 arguments: remove(list, value)".to_string(),
                ));
            }

            let mut out = match &positional[0] {
                Value::ListShared(v) => v.to_vec(),
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
                Value::ListShared(v) => v,
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
                Value::ListShared(v) => v,
                _ => return Some(Err(format!("zip expects a list, got {}", positional[0].type_name()))),
            };
            let right = match &positional[1] {
                Value::ListShared(v) => v,
                _ => return Some(Err("zip expects second argument to be list".to_string())),
            };

            let size = left.len().min(right.len());
            let mut out = Vec::with_capacity(size);
            for i in 0..size {
                out.push(Value::List(vec![left[i].clone(), right[i].clone()]));
            }
            Some(Ok(Value::List(out)))
        }
        "items" | "dict_items" | "entries" => {
            if positional.len() != 1 {
                return Some(Err("items expects exactly 1 argument (dictionary)".to_string()));
            }
            let map = match &positional[0] {
                Value::DictShared(v) => v,
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
                Value::DictShared(v) => v,
                _ => return Some(Err(format!("dict_get expects a dictionary, got {}", positional[0].type_name()))),
            };
            let key = match &positional[1] {
                Value::StrShared(v) => v,
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
                Value::DictShared(v) => v.clone(),
                _ => return Some(Err(format!("dict_set expects a dictionary, got {}", positional[0].type_name()))),
            };
            let key = match &positional[1] {
                Value::StrShared(v) => v.to_string(),
                _ => return Some(Err("dict_set key must be string".to_string())),
            };

            map.insert(key, positional[2].clone());
            Some(Ok(Value::DictShared(map)))
        }
        "dict_update" => {
            if positional.len() != 2 {
                return Some(Err(
                    "dict_update expects exactly 2 arguments: dict_update(dict, updates)"
                        .to_string(),
                ));
            }

            let mut map = match &positional[0] {
                Value::DictShared(v) => v.clone(),
                _ => {
                    return Some(Err(format!(
                        "dict_update expects a dictionary, got {}", positional[0].type_name()
                    )))
                }
            };
            let updates = match &positional[1] {
                Value::DictShared(v) => v,
                _ => {
                    return Some(Err(
                        "dict_update expects second argument to be dictionary".to_string(),
                    ))
                }
            };

            for (key, value) in updates {
                map.insert(key.clone(), value.clone());
            }

            Some(Ok(Value::DictShared(map)))
        }
        "dict_remove" => {
            if positional.len() != 2 {
                return Some(Err(
                    "dict_remove expects exactly 2 arguments: dict_remove(dict, key)".to_string(),
                ));
            }
            let mut map = match &positional[0] {
                Value::DictShared(v) => v.clone(),
                _ => return Some(Err(format!("dict_remove expects a dictionary, got {}", positional[0].type_name()))),
            };
            let key = match &positional[1] {
                Value::StrShared(v) => v,
                _ => return Some(Err("dict_remove key must be string".to_string())),
            };

            map.remove(key);
            Some(Ok(Value::DictShared(map)))
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
                Value::ListShared(_) => Some(Ok(Value::List(vec![]))),
                Value::SetShared(_) => Some(Ok(Value::Set(vec![]))),
                Value::DictShared(_) => Some(Ok(Value::Dict(HashMap::new().into()))),
                Value::StrShared(_) => Some(Ok(Value::Str(String::new()))),
                _ => Some(Err(format!("clear expects a string, list, set, or dictionary, got {}", positional[0].type_name()))),
            }
        }
        "set_add" => {
            if positional.len() != 2 {
                return Some(Err("set.add expects exactly 1 argument: the element to add".to_string()));
            }
            match &positional[0] {
                Value::SetShared(items) => {
                    let mut new_items = items.clone();
                    let key = format!("{}", positional[1]);
                    if !items.iter().any(|v| format!("{}", v) == key) {
                        new_items.push(positional[1].clone());
                    }
                    Some(Ok(Value::SetShared(new_items)))
                }
                _ => Some(Err(format!("add expects a set, got {}", positional[0].type_name()))),
            }
        }
        "set_remove" => {
            if positional.len() != 2 {
                return Some(Err("set.remove expects exactly 1 argument: the element to remove".to_string()));
            }
            match &positional[0] {
                Value::SetShared(items) => {
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
                Value::SetShared(items) => {
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
            let a = match &positional[0] { Value::SetShared(v) => v.clone(), _ => return Some(Err(format!("{} expects a set, got {}", callee, positional[0].type_name()))) };
            let b = match &positional[1] { Value::SetShared(v) => v.clone(), _ => return Some(Err(format!("{} expects a set, got {}", callee, positional[1].type_name()))) };
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
        "clamp" | "math_clamp" => {
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
                Value::ListShared(items) => {
                    let idx = items
                        .iter()
                        .position(|v| eq_values(v, &positional[1]))
                        .map(|v| v as i64)
                        .unwrap_or(-1);
                    Some(Ok(Value::Int(idx)))
                }
                Value::StrShared(text) => {
                    let needle = match &positional[1] {
                        Value::StrShared(s) => s,
                        _ => {
                            return Some(Err(
                                "index on string expects string value".to_string(),
                            ))
                        }
                    };

                    if needle.is_empty() {
                        return Some(Ok(Value::Int(0)));
                    }

                    if let Some(byte_index) = text.find(needle.as_str()) {
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
            Some(Ok(Value::Dict(map.into())))
        }
        "err" => {
            if positional.len() != 1 {
                return Some(Err("err expects exactly 1 argument: err(message)".to_string()));
            }
            let mut map = HashMap::new();
            map.insert("ok".to_string(), Value::Bool(false));
            map.insert("error".to_string(), positional[0].clone());
            Some(Ok(Value::Dict(map.into())))
        }
        "is_ok" => {
            if positional.len() != 1 {
                return Some(Err("is_ok expects exactly 1 argument".to_string()));
            }
            match &positional[0] {
                Value::DictShared(m) => Some(Ok(Value::Bool(m.get("ok").map(|v| v.to_bool()).unwrap_or(false)))),
                _ => Some(Ok(Value::Bool(false))),
            }
        }
        "is_err" => {
            if positional.len() != 1 {
                return Some(Err("is_err expects exactly 1 argument".to_string()));
            }
            match &positional[0] {
                Value::DictShared(m) => Some(Ok(Value::Bool(!m.get("ok").map(|v| v.to_bool()).unwrap_or(true)))),
                _ => Some(Ok(Value::Bool(true))),
            }
        }
        "unwrap" => {
            if positional.len() < 1 || positional.len() > 2 {
                return Some(Err("unwrap expects 1 or 2 arguments: unwrap(result, [fallback])".to_string()));
            }
            match &positional[0] {
                Value::DictShared(m) => {
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
            Some(Ok(Value::Dict(map.into())))
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
                Value::StrShared(_) => "string",
                Value::ListShared(_) => "list",
                Value::SetShared(_) => "group",
                Value::DictShared(_) => "dict",
                Value::Func(_) => "function",
                Value::Object { .. } => "object",
                Value::Module(_) => "module",
                Value::Empty => "empty",
            };
            Some(Ok(Value::Str(ty.to_string())))
        }
        "is_list" | "is_dict" | "is_string" | "is_number" | "is_int" | "is_float"
        | "is_bool" | "is_group" | "is_generator" => {
            if positional.len() != 1 {
                return Some(Err(format!("{} expects exactly 1 argument", callee)));
            }
            let matches = match callee {
                "is_list" => matches!(positional[0], Value::ListShared(_)),
                "is_dict" => matches!(positional[0], Value::DictShared(_)),
                "is_string" => matches!(positional[0], Value::StrShared(_)),
                "is_int" => matches!(positional[0], Value::Int(_)),
                "is_float" => matches!(positional[0], Value::Float(_)),
                "is_bool" => matches!(positional[0], Value::Bool(_)),
                "is_group" => matches!(positional[0], Value::SetShared(_)),
                "is_generator" => matches!(&positional[0], Value::DictShared(m) if m.get("__generator__").map_or(false, |v| v.to_bool())),
                "is_number" => matches!(positional[0], Value::Int(_) | Value::Float(_)),
                _ => false,
            };
            Some(Ok(Value::Bool(matches)))
        }
        "to_list" => {
            if positional.len() != 1 {
                return Some(Err("to_list expects exactly 1 argument".to_string()));
            }
            match iterable_items(&positional[0]) {
                Some(v) => Some(Ok(Value::List(v))),
                None => Some(Err(format!(
                    "to_list expects a list, set, or generator, got {}",
                    positional[0].type_name()
                ))),
            }
        }
        "is_nan" => {
            if positional.len() != 1 {
                return Some(Err("is_nan expects exactly 1 argument".to_string()));
            }
            let is_n = match &positional[0] {
                Value::Float(f) => f.is_nan(),
                _ => false,
            };
            Some(Ok(Value::Bool(is_n)))
        }
        "is_finite" => {
            if positional.len() != 1 {
                return Some(Err("is_finite expects exactly 1 argument".to_string()));
            }
            let is_f = match &positional[0] {
                Value::Int(_) => true,
                Value::Float(f) => f.is_finite(),
                _ => false,
            };
            Some(Ok(Value::Bool(is_f)))
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
        "http_request" => {
            // http_request(method, url, [payload], [authorization], [headers])
            if positional.len() < 2 || positional.len() > 5 {
                return Some(Err(
                    "http_request expects 2-5 arguments: http_request(method, url, [payload], [authorization], [headers])"
                        .to_string(),
                ));
            }
            let method = positional[0].to_string();
            let url = positional[1].to_string();
            let payload = positional.get(2).cloned().unwrap_or(Value::Empty);
            let auth = positional.get(3).map(|v| v.to_string()).unwrap_or_default();
            let headers = positional.get(4).cloned().unwrap_or(Value::Empty);
            Some(http_request_builtin(&method, &url, &payload, &auth, &headers))
        }
        // ── Async I/O — run an HTTP builtin on a background thread, return a future id ──
        "http_set_timeout" => {
            if positional.len() != 1 {
                return Some(Err("http_set_timeout expects exactly 1 argument (seconds)".to_string()));
            }
            let secs = match positional[0].as_number() {
                Some(n) if n >= 0.0 => n,
                _ => return Some(Err("http_set_timeout seconds must be a non-negative number".to_string())),
            };
            HTTP_TIMEOUT_MS.store((secs * 1000.0) as u64, Ordering::Relaxed);
            Some(Ok(Value::Empty))
        }
        "http_timeout" => {
            if !positional.is_empty() {
                return Some(Err("http_timeout takes no arguments".to_string()));
            }
            Some(Ok(Value::Float(http_timeout().as_secs_f64())))
        }
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
        "json_dumps" | "json_stringify" => {
            if positional.len() != 1 {
                return Some(Err("json_dumps expects exactly 1 argument".to_string()));
            }
            let json_payload = value_to_json(&positional[0]);
            match serde_json::to_string(&json_payload) {
                Ok(text) => Some(Ok(Value::Str(text))),
                Err(e) => Some(Err(format!("json_dumps failed: {e}"))),
            }
        }
        "json_loads" | "json_parse" => {
            if positional.len() != 1 {
                return Some(Err("json_loads expects exactly 1 argument".to_string()));
            }
            let text = positional[0].to_string();
            match serde_json::from_str::<JsonValue>(&text) {
                Ok(parsed) => Some(Ok(json_to_value(parsed))),
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
        "os_run_ok" => {
            if positional.len() != 1 {
                return Some(Err("os_run_ok expects exactly 1 argument (command)".to_string()));
            }
            Some(os_run_ok_builtin(&positional[0].to_string()))
        }
        "os_which" => {
            if positional.len() != 1 {
                return Some(Err("os_which expects exactly 1 argument (command)".to_string()));
            }
            Some(os_which_builtin(&positional[0].to_string()))
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
                Value::StrShared(s) => match s.trim().parse::<i64>() {
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
        "random_randint" => {
            if positional.len() != 2 {
                return Some(Err("random_randint expects exactly 2 arguments (a, b)".to_string()));
            }
            let a = match &positional[0] {
                Value::Int(v) => *v,
                _ => return Some(Err("random_randint expects integer arguments".to_string())),
            };
            let b = match &positional[1] {
                Value::Int(v) => *v,
                _ => return Some(Err("random_randint expects integer arguments".to_string())),
            };
            Some(random_int_builtin(a, b))
        }
        "random_uniform" => {
            if positional.len() != 2 {
                return Some(Err("random_uniform expects exactly 2 arguments (a, b)".to_string()));
            }
            let a = match value_to_f64("random_uniform", &positional[0]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let b = match value_to_f64("random_uniform", &positional[1]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            Some(Ok(Value::Float(lo + (hi - lo) * random_next_f64())))
        }
        "random_sample" => {
            if positional.len() != 2 {
                return Some(Err("random_sample expects exactly 2 arguments (list, k)".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("random_sample expects a list".to_string())),
            };
            let k = match &positional[1] {
                Value::Int(v) if *v >= 0 => *v as usize,
                _ => return Some(Err("random_sample: k must be a non-negative integer".to_string())),
            };
            if k > items.len() {
                return Some(Err("random_sample: k cannot exceed list length".to_string()));
            }
            let shuffled = match random_shuffle_builtin(&Value::ListShared(items)) {
                Ok(Value::ListShared(v)) => v.into_vec(),
                Ok(_) => return Some(Err("random_sample internal error".to_string())),
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(Value::List(shuffled[..k].to_vec())))
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
        "math_trunc" | "trunc" => {
            if positional.len() != 1 {
                return Some(Err("trunc expects exactly 1 argument".to_string()));
            }
            match positional[0].as_number() {
                Some(n) => Some(Ok(Value::Int(n.trunc() as i64))),
                None => Some(Err("trunc expects a number".to_string())),
            }
        }
        "math_sign" | "sign" => {
            if positional.len() != 1 {
                return Some(Err("sign expects exactly 1 argument".to_string()));
            }
            match positional[0].as_number() {
                Some(n) => {
                    let s = if n > 0.0 { 1 } else if n < 0.0 { -1 } else { 0 };
                    Some(Ok(Value::Int(s)))
                }
                None => Some(Err("sign expects a number".to_string())),
            }
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
        "math_pi" => {
            if !positional.is_empty() {
                return Some(Err("math_pi expects no arguments".to_string()));
            }
            Some(Ok(Value::Float(std::f64::consts::PI)))
        }
        "math_e" => {
            if !positional.is_empty() {
                return Some(Err("math_e expects no arguments".to_string()));
            }
            Some(Ok(Value::Float(std::f64::consts::E)))
        }
        "math_tau" => {
            if !positional.is_empty() {
                return Some(Err("math_tau expects no arguments".to_string()));
            }
            Some(Ok(Value::Float(std::f64::consts::TAU)))
        }
        "math_factorial" => {
            if positional.len() != 1 {
                return Some(Err("math_factorial expects exactly 1 argument".to_string()));
            }
            match &positional[0] {
                Value::Int(v) if *v >= 0 => {
                    let mut r: i64 = 1;
                    for i in 2..=*v {
                        r *= i;
                    }
                    Some(Ok(Value::Int(r)))
                }
                _ => Some(Err("math_factorial expects a non-negative integer".to_string())),
            }
        }
        "math_gcd" => {
            let mut nums: Vec<i64> = Vec::new();
            for p in positional {
                match p {
                    Value::Int(v) => nums.push(*v),
                    Value::ListShared(items) => {
                        for it in items {
                            match it {
                                Value::Int(v) => nums.push(*v),
                                _ => return Some(Err("math_gcd expects integers".to_string())),
                            }
                        }
                    }
                    _ => return Some(Err("math_gcd expects integers".to_string())),
                }
            }
            let mut g: i64 = 0;
            for n in nums {
                g = gcd_i64(g, n);
            }
            Some(Ok(Value::Int(g)))
        }
        "math_lcm" => {
            let mut nums: Vec<i64> = Vec::new();
            for p in positional {
                match p {
                    Value::Int(v) => nums.push(*v),
                    Value::ListShared(items) => {
                        for it in items {
                            match it {
                                Value::Int(v) => nums.push(*v),
                                _ => return Some(Err("math_lcm expects integers".to_string())),
                            }
                        }
                    }
                    _ => return Some(Err("math_lcm expects integers".to_string())),
                }
            }
            let mut l: i64 = 1;
            for n in nums {
                if n == 0 {
                    return Some(Ok(Value::Int(0)));
                }
                l = (l / gcd_i64(l, n)) * n;
            }
            Some(Ok(Value::Int(l)))
        }
        "math_hypot" => {
            if positional.len() != 2 {
                return Some(Err("math_hypot expects exactly 2 arguments".to_string()));
            }
            Some(math_binary_float_builtin(&positional[0], &positional[1], |a, b| a.hypot(b)))
        }
        "math_log2" => {
            if positional.len() != 1 {
                return Some(Err("math_log2 expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.log2()))
        }
        "math_degrees" => {
            if positional.len() != 1 {
                return Some(Err("math_degrees expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.to_degrees()))
        }
        "math_radians" => {
            if positional.len() != 1 {
                return Some(Err("math_radians expects exactly 1 argument".to_string()));
            }
            Some(math_unary_float_builtin(&positional[0], |v| v.to_radians()))
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
                        Some(Ok(Value::Dict(map.into())))
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
        "repeat_str" | "repeat" => {
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
        "interpolate" => {
            if positional.is_empty() {
                return Some(Err("interpolate expects at least 1 argument: interpolate(template, [vars])".to_string()));
            }
            let mut template = positional[0].to_string();
            if positional.len() > 1 {
                if let Value::DictShared(ref map) = positional[1] {
                    for (k, v) in map {
                        template = template.replace(&format!("%{}%", k), &v.to_string());
                        template = template.replace(&format!("${{{}}}", k), &v.to_string());
                        template = template.replace(&format!("{{{}}}", k), &v.to_string());
                    }
                }
            }
            Some(Ok(Value::Str(template)))
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
            let items = match &positional[0] { Value::ListShared(v) => v.clone(), _ => return Some(Err("map expects a list".to_string())) };
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
            let items = match &positional[0] { Value::ListShared(v) => v.clone(), _ => return Some(Err("filter expects a list".to_string())) };
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
        // ── itertools / functools / collections (Indent 2.0) ──
        "chain" => {
            if positional.is_empty() {
                return Some(Err("chain expects at least 1 argument (list, ...)".to_string()));
            }
            let mut out = Vec::new();
            for p in positional {
                match p {
                    Value::ListShared(v) => out.extend(v.clone()),
                    _ => return Some(Err("chain: all arguments must be lists".to_string())),
                }
            }
            Some(Ok(Value::List(out)))
        }
        "flatten" | "flat" => {
            if positional.len() != 1 {
                return Some(Err("flatten expects exactly 1 argument (list)".to_string()));
            }
            fn flatten_rec(v: &Value, out: &mut Vec<Value>) {
                match v {
                    Value::ListShared(items) => {
                        for it in items {
                            flatten_rec(it, out);
                        }
                    }
                    other => out.push(other.clone()),
                }
            }
            let mut out = Vec::new();
            flatten_rec(&positional[0], &mut out);
            Some(Ok(Value::List(out)))
        }
        "chunk" => {
            if positional.len() != 2 {
                return Some(Err("chunk expects exactly 2 arguments: chunk(list, n)".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("chunk expects a list".to_string())),
            };
            let n = match &positional[1] {
                Value::Int(v) if *v > 0 => *v as usize,
                _ => return Some(Err("chunk: size must be a positive integer".to_string())),
            };
            let mut out = Vec::new();
            for c in items.chunks(n) {
                out.push(Value::List(c.to_vec()));
            }
            Some(Ok(Value::List(out)))
        }
        "product" => {
            let mut lists: Vec<Vec<Value>> = Vec::new();
            for p in positional {
                match p {
                    Value::ListShared(v) => lists.push(v.to_vec()),
                    _ => return Some(Err("product: all arguments must be lists".to_string())),
                }
            }
            let mut result: Vec<Vec<Value>> = vec![Vec::new()];
            for l in &lists {
                let mut next = Vec::new();
                for partial in &result {
                    for item in l {
                        let mut p = partial.clone();
                        p.push(item.clone());
                        next.push(p);
                    }
                }
                result = next;
            }
            Some(Ok(Value::List(result.into_iter().map(Value::List).collect())))
        }
        "permutations" => {
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("permutations expects a list".to_string())),
            };
            let k = if positional.len() == 2 {
                match &positional[1] {
                    Value::Int(v) if *v >= 0 => *v as usize,
                    _ => return Some(Err("permutations: r must be a non-negative integer".to_string())),
                }
            } else {
                items.len()
            };
            fn permute(items: &[Value], k: usize, prefix: &mut Vec<Value>, used: &mut Vec<bool>, out: &mut Vec<Value>) {
                if prefix.len() == k {
                    out.push(Value::List(prefix.clone()));
                    return;
                }
                for i in 0..items.len() {
                    if used[i] {
                        continue;
                    }
                    used[i] = true;
                    prefix.push(items[i].clone());
                    permute(items, k, prefix, used, out);
                    prefix.pop();
                    used[i] = false;
                }
            }
            let mut out = Vec::new();
            let mut prefix = Vec::new();
            let mut used = vec![false; items.len()];
            permute(&items, k, &mut prefix, &mut used, &mut out);
            Some(Ok(Value::List(out)))
        }
        "combinations" => {
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("combinations expects a list".to_string())),
            };
            let k = match &positional[1] {
                Value::Int(v) if *v >= 0 => *v as usize,
                _ => return Some(Err("combinations: r must be a non-negative integer".to_string())),
            };
            fn combine(items: &[Value], start: usize, k: usize, prefix: &mut Vec<Value>, out: &mut Vec<Value>) {
                if prefix.len() == k {
                    out.push(Value::List(prefix.clone()));
                    return;
                }
                for i in start..items.len() {
                    if items.len() - i < k - prefix.len() {
                        break;
                    }
                    prefix.push(items[i].clone());
                    combine(items, i + 1, k, prefix, out);
                    prefix.pop();
                }
            }
            let mut out = Vec::new();
            let mut prefix = Vec::new();
            combine(&items, 0, k, &mut prefix, &mut out);
            Some(Ok(Value::List(out)))
        }
        "accumulate" => {
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("accumulate expects a list".to_string())),
            };
            if items.is_empty() {
                return Some(Ok(Value::List(Vec::new())));
            }
            if items.iter().all(|v| matches!(v, Value::Int(_))) {
                let mut acc: i64 = 0;
                let mut out = Vec::new();
                for it in &items {
                    if let Value::Int(v) = it {
                        acc += v;
                    }
                    out.push(Value::Int(acc));
                }
                Some(Ok(Value::List(out)))
            } else {
                let mut acc: f64 = 0.0;
                let mut out = Vec::new();
                for it in &items {
                    acc += match value_to_f64("accumulate", it) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };
                    out.push(Value::Float(acc));
                }
                Some(Ok(Value::List(out)))
            }
        }
        "cycle" => {
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("cycle expects a list".to_string())),
            };
            let n = match &positional[1] {
                Value::Int(v) if *v >= 0 => *v as usize,
                _ => return Some(Err("cycle: n must be a non-negative integer".to_string())),
            };
            let mut out = Vec::new();
            for _ in 0..n {
                out.extend(items.clone());
            }
            Some(Ok(Value::List(out)))
        }
        "repeat_item" => {
            if positional.len() != 2 {
                return Some(Err("repeat_item expects exactly 2 arguments: repeat_item(item, n)".to_string()));
            }
            let n = match &positional[1] {
                Value::Int(v) if *v >= 0 => *v as usize,
                _ => return Some(Err("repeat: n must be a non-negative integer".to_string())),
            };
            let item = positional[0].clone();
            Some(Ok(Value::List(vec![item; n])))
        }
        "takewhile" => {
            if positional.len() != 2 {
                return Some(Err("takewhile expects exactly 2 arguments: takewhile(predicate, list)".to_string()));
            }
            let pred = positional[0].to_string();
            let items = match &positional[1] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("takewhile expects a list".to_string())),
            };
            let mut out = Vec::new();
            for it in &items {
                match apply_builtin1(&pred, it) {
                    Ok(r) if r.to_bool() => out.push(it.clone()),
                    Ok(_) => break,
                    Err(e) => return Some(Err(e)),
                }
            }
            Some(Ok(Value::List(out)))
        }
        "dropwhile" => {
            if positional.len() != 2 {
                return Some(Err("dropwhile expects exactly 2 arguments: dropwhile(predicate, list)".to_string()));
            }
            let pred = positional[0].to_string();
            let items = match &positional[1] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("dropwhile expects a list".to_string())),
            };
            let mut out = Vec::new();
            let mut dropping = true;
            for it in &items {
                if dropping {
                    match apply_builtin1(&pred, it) {
                        Ok(r) if r.to_bool() => continue,
                        Ok(_) => dropping = false,
                        Err(e) => return Some(Err(e)),
                    }
                }
                out.push(it.clone());
            }
            Some(Ok(Value::List(out)))
        }
        "unique" => {
            if positional.len() != 1 {
                return Some(Err("unique expects exactly 1 argument (list)".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("unique expects a list".to_string())),
            };
            let mut out: Vec<Value> = Vec::new();
            for it in &items {
                if !out.iter().any(|x| eq_values(x, it)) {
                    out.push(it.clone());
                }
            }
            Some(Ok(Value::List(out)))
        }
        "partition" => {
            if positional.len() != 2 {
                return Some(Err("partition expects exactly 2 arguments: partition(list, predicate)".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("partition expects a list".to_string())),
            };
            let pred = positional[1].to_string();
            let mut yes = Vec::new();
            let mut no = Vec::new();
            for it in &items {
                match apply_builtin1(&pred, it) {
                    Ok(r) if r.to_bool() => yes.push(it.clone()),
                    Ok(_) => no.push(it.clone()),
                    Err(e) => return Some(Err(e)),
                }
            }
            Some(Ok(Value::List(vec![Value::List(yes), Value::List(no)])))
        }
        "group_by" => {
            if positional.len() != 2 {
                return Some(Err("group_by expects exactly 2 arguments: group_by(list, keyfn)".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("group_by expects a list".to_string())),
            };
            let keyfn = positional[1].to_string();
            let mut groups: HashMap<String, Vec<Value>> = HashMap::new();
            for it in &items {
                let key = match apply_builtin1(&keyfn, it) {
                    Ok(k) => k.to_string(),
                    Err(e) => return Some(Err(e)),
                };
                groups.entry(key).or_default().push(it.clone());
            }
            let mut out = HashMap::new();
            for (k, v) in groups {
                out.insert(k, Value::List(v));
            }
            Some(Ok(Value::Dict(out.into())))
        }
        "max_key" | "min_key" => {
            if positional.len() != 2 {
                return Some(Err(format!("{callee} expects exactly 2 arguments: {callee}(list, keyfn)")));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err(format!("{callee} expects a list"))),
            };
            if items.is_empty() {
                return Some(Err(format!("{callee}: cannot operate on an empty list")));
            }
            let keyfn = positional[1].to_string();
            let want_max = callee == "max_key";
            let mut best = items[0].clone();
            let mut best_key = match apply_builtin1(&keyfn, &best) {
                Ok(k) => k,
                Err(e) => return Some(Err(e)),
            };
            for it in &items[1..] {
                let k = match apply_builtin1(&keyfn, it) {
                    Ok(k) => k,
                    Err(e) => return Some(Err(e)),
                };
                let ord = compare_values(&k, &best_key);
                if (want_max && ord == std::cmp::Ordering::Greater)
                    || (!want_max && ord == std::cmp::Ordering::Less)
                {
                    best = it.clone();
                    best_key = k;
                }
            }
            Some(Ok(best))
        }
        "reduce" => {
            if positional.len() < 2 || positional.len() > 3 {
                return Some(Err("reduce expects 2 or 3 arguments: reduce(fn, list, initial?)".to_string()));
            }
            let fn_name = positional[0].to_string();
            let items = match &positional[1] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("reduce expects a list".to_string())),
            };
            let mut acc: Value;
            let mut start_idx = 0usize;
            if positional.len() == 3 {
                acc = positional[2].clone();
            } else {
                if items.is_empty() {
                    return Some(Err("reduce: empty list with no initial value".to_string()));
                }
                acc = items[0].clone();
                start_idx = 1;
            }
            for it in &items[start_idx..] {
                let args = vec![acc, it.clone()];
                match invoke_builtin(&fn_name, &args) {
                    Some(Ok(v)) => acc = v,
                    Some(Err(e)) => return Some(Err(e)),
                    None => return Some(Err(format!("reduce: function '{}' not found", fn_name))),
                }
            }
            Some(Ok(acc))
        }
        "zip_longest" => {
            if positional.len() < 2 || positional.len() > 3 {
                return Some(Err("zip_longest expects 2 or 3 arguments: zip_longest(a, b, fill?)".to_string()));
            }
            let a = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("zip_longest expects lists".to_string())),
            };
            let b = match &positional[1] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("zip_longest expects lists".to_string())),
            };
            let fill = positional.get(2).cloned().unwrap_or(Value::Empty);
            let n = a.len().max(b.len());
            let mut out = Vec::new();
            for i in 0..n {
                let x = a.get(i).cloned().unwrap_or_else(|| fill.clone());
                let y = b.get(i).cloned().unwrap_or_else(|| fill.clone());
                out.push(Value::List(vec![x, y]));
            }
            Some(Ok(Value::List(out)))
        }
        "pairwise" => {
            if positional.len() != 1 {
                return Some(Err("pairwise expects exactly 1 argument (list)".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("pairwise expects a list".to_string())),
            };
            Some(Ok(pairwise_builtin(&items)))
        }
        "filterfalse" => {
            if positional.len() != 2 {
                return Some(Err("filterfalse expects exactly 2 arguments: filterfalse(predicate, list)".to_string()));
            }
            let pred = positional[0].to_string();
            let items = match &positional[1] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("filterfalse expects a list".to_string())),
            };
            let mut out = Vec::new();
            for it in &items {
                match apply_builtin1(&pred, it) {
                    Ok(r) if !r.to_bool() => out.push(it.clone()),
                    Ok(_) => {}
                    Err(e) => return Some(Err(e)),
                }
            }
            Some(Ok(Value::List(out)))
        }
        "compress" => {
            if positional.len() != 2 {
                return Some(Err("compress expects exactly 2 arguments: compress(list, selectors)".to_string()));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("compress expects a list".to_string())),
            };
            let selectors = match &positional[1] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("compress: selectors must be a list".to_string())),
            };
            let mut out = Vec::new();
            for (i, it) in items.iter().enumerate() {
                if i < selectors.len() && selectors[i].to_bool() {
                    out.push(it.clone());
                }
            }
            Some(Ok(Value::List(out)))
        }
        "starmap" => {
            if positional.len() != 2 {
                return Some(Err("starmap expects exactly 2 arguments: starmap(fn, list_of_lists)".to_string()));
            }
            let fn_name = positional[0].to_string();
            let rows = match &positional[1] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err("starmap expects a list".to_string())),
            };
            let mut out = Vec::new();
            for row in &rows {
                let args = match row {
                    Value::ListShared(v) => v.clone(),
                    _ => return Some(Err("starmap: each element must be a list of args".to_string())),
                };
                match invoke_builtin(&fn_name, &args) {
                    Some(Ok(v)) => out.push(v),
                    Some(Err(e)) => return Some(Err(e)),
                    None => return Some(Err(format!("starmap: function '{}' not found", fn_name))),
                }
            }
            Some(Ok(Value::List(out)))
        }
        "first" | "last" => {
            if positional.len() != 2 {
                return Some(Err(format!("{callee} expects exactly 2 arguments: {callee}(list, n)")));
            }
            let items = match &positional[0] {
                Value::ListShared(v) => v.clone(),
                _ => return Some(Err(format!("{callee} expects a list"))),
            };
            let n = match &positional[1] {
                Value::Int(v) if *v >= 0 => *v as usize,
                _ => return Some(Err(format!("{callee}: n must be a non-negative integer"))),
            };
            Some(Ok(head_tail_builtin(&items, n, callee == "last")))
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
    Ok(Value::Dict(out.into()))
}

/// Return `true` if `command` exits with status 0 (shutil-style quick check).
fn os_run_ok_builtin(command: &str) -> Result<Value, String> {
    #[cfg(target_os = "windows")]
    let status = Command::new("cmd").args(["/C", command]).status();
    #[cfg(not(target_os = "windows"))]
    let status = Command::new("sh").args(["-c", command]).status();
    let status = status.map_err(|e| format!("os_run_ok failed: {e}"))?;
    Ok(Value::Bool(status.success()))
}

/// Locate an executable on PATH (shutil.which equivalent).
fn os_which_builtin(name: &str) -> Result<Value, String> {
    if name.contains('/') || name.contains('\\') {
        let p = Path::new(name);
        if p.is_file() {
            return Ok(Value::Str(name.to_string()));
        }
        return Ok(Value::Empty);
    }
    let path_var = std::env::var("PATH").unwrap_or_default();
    for dir in path_var.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = format!("{}/{}", dir.trim_end_matches('/'), name);
        let p = Path::new(&candidate);
        if p.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = p.metadata() {
                    if meta.permissions().mode() & 0o111 != 0 {
                        return Ok(Value::Str(candidate));
                    }
                }
            }
            #[cfg(not(unix))]
            {
                return Ok(Value::Str(candidate));
            }
        }
    }
    Ok(Value::Empty)
}

/// Pair adjacent elements: pairwise([1,2,3]) → [[1,2],[2,3]].
fn pairwise_builtin(items: &[Value]) -> Value {
    let mut out = Vec::new();
    for w in items.windows(2) {
        out.push(Value::List(vec![w[0].clone(), w[1].clone()]));
    }
    Value::List(out)
}

/// first(list, n) / last(list, n).
fn head_tail_builtin(items: &[Value], n: usize, from_end: bool) -> Value {
    if items.is_empty() || n == 0 {
        return Value::List(Vec::new());
    }
    let k = n.min(items.len());
    let slice = if from_end {
        &items[items.len() - k..]
    } else {
        &items[..k]
    };
    Value::List(slice.to_vec())
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
    Ok(Value::Dict(out.into()))
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
    Ok(json_to_value(parsed))
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
    Ok(Value::Dict(out.into()))
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
    let kwargs_json = serde_json::to_string(&value_to_json(&Value::Dict(named_map.into())))
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
        // `value` borrows `envelope`, which the other branch still needs.
        Ok(json_to_value(value.clone()))
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
    Value::Dict(out.into())
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
        Value::ListShared(rows) => rows,
        _ => return Err("csv_write expects a list of rows (list of lists)".to_string()),
    };
    let mut content = String::new();
    for row in rows {
        let row_items = match row {
            Value::ListShared(items) => items.clone(),
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
        Value::ListShared(v) => v,
        _ => return Err("counter expects a list".to_string()),
    };
    let mut out = HashMap::new();
    for it in items {
        let e = out.entry(it.to_string()).or_insert(Value::Int(0));
        if let Value::Int(n) = e {
            *n += 1;
        }
    }
    Ok(Value::Dict(out.into()))
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
            Value::Dict(m.into())
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
        Value::StrShared(s) => Some(toml::Value::String(s.to_string())),
        Value::ListShared(items) => Some(toml::Value::Array(
            items.iter().filter_map(value_to_toml).collect(),
        )),
        Value::SetShared(items) => Some(toml::Value::Array(
            items.iter().filter_map(value_to_toml).collect(),
        )),
        Value::DictShared(d) => {
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
    Ok(json_to_value(v))
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
    let handle = spawn_task_thread("indent-task", USER_TASK_STACK, move || {
        let mut rt = Runtime::new(module_dir);
        rt.funcs = funcs;
        rt.callables = callables;
        rt.classes = classes;
        rt.async_funcs = async_funcs;

        let out = {
            let mut ctx2 = ExecContext::new(&mut rt);
            // A spawned task needs no symbol injection any more: the module whose
            // function it runs is pushed onto `invoke_external_function`'s scope
            // stack, so its siblings and its module-level variables resolve without
            // being copied into this runtime first.
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
    })?;

    register_task(
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
        Value::ListShared(v) => v,
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
        Value::ListShared(v) => v,
        _ => return Err("parallel expects a list of argument-lists".to_string()),
    };
    let mut ids = Vec::new();
    for item in lists {
        let args = match item {
            Value::ListShared(a) => a.clone(),
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
        Value::StrShared(s) => s
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
            Value::ListShared(l) => {
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
    let handle = spawn_task_thread("indent-sleep", TIMER_TASK_STACK, move || {
        std::thread::sleep(Duration::from_secs_f64(seconds.max(0.0)));
        *result2.lock().unwrap() = Some(Value::Empty);
        done2.store(true, Ordering::SeqCst);
    })?;
    register_task(
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
    let handle = spawn_task_thread("indent-async", BUILTIN_TASK_STACK, move || {
        let out = invoke_builtin(&builtin_name, &args).unwrap_or_else(|| {
            Err(format!("async {}: no such builtin", builtin_name))
        });
        let val = out.unwrap_or(Value::Empty);
        *result2.lock().unwrap() = Some(val);
        done2.store(true, Ordering::SeqCst);
    })?;
    register_task(
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
    body: Arc<Vec<Stmt>>,
    ip: usize,
    locals: Map<String, Value>,
    result: Value,
    done: bool,
    awaiting: Option<i64>,
}

impl Coro {
    fn new(body: Arc<Vec<Stmt>>) -> Self {
        Self {
            body,
            ip: 0,
            locals: Map::default(),
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
        Value::ListShared(v) => v,
        _ => return Err("coop expects a list of [fn_name, [args]]".to_string()),
    };

    let mut coros: Vec<Coro> = Vec::new();
    for item in items {
        let pair = match item {
            Value::ListShared(p) => p,
            _ => return Err("coop expects each item to be [fn_name, [args]]".to_string()),
        };
        let name = pair[0].to_string();
        let args: Vec<Value> = match pair.get(1) {
            Some(Value::ListShared(a)) => a.to_vec(),
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
                        ctx.frames.push(Frame { vars: c.locals.clone(), args: vec![] });
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
                        ctx.frames.push(Frame { vars: c.locals.clone(), args: vec![] });
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
                        ctx.frames.push(Frame { vars: c.locals.clone(), args: vec![] });
                        let control = exec_stmt(&stmt, ctx);
                        c.locals = ctx.frames.pop().unwrap().vars;
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
    let fmt_str = format
        .map(|v| v.to_string())
        .unwrap_or_else(|| "%Y-%m-%d %H:%M:%S".to_string());

    let secs = ts_secs.floor() as i64;
    let d = UNIX_EPOCH + std::time::Duration::new(secs as u64, 0);
    let total_secs = d.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;

    // Break the epoch second count into calendar fields (UTC), using the
    // civil-from-days algorithm (accurate for the full i64 range, incl. leap years).
    let days = total_secs.div_euclid(86400);
    let tod = total_secs.rem_euclid(86400);
    let hours = tod / 3600;
    let minutes = (tod % 3600) / 60;
    let seconds = tod % 60;

    let (year, month, day) = civil_from_days(days);

    // Day of year (1-based) for %j.
    let jan1_days = days_from_civil(year, 1, 1);
    let day_of_year = days - jan1_days + 1;

    // Weekday: 1970-01-01 was a Thursday (index 4 with Sunday = 0).
    let weekday = (days.rem_euclid(7) + 4).rem_euclid(7);

    const MONTHS: [&str; 12] = [
        "January", "February", "March", "April", "May", "June", "July", "August",
        "September", "October", "November", "December",
    ];
    const DAYS: [&str; 7] = [
        "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
    ];

    let hour12 = if hours % 12 == 0 { 12 } else { hours % 12 };
    let ampm = if hours < 12 { "AM" } else { "PM" };

    let mut out = String::with_capacity(fmt_str.len() + 8);
    let bytes = fmt_str.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 1 < bytes.len() {
            let spec = bytes[i + 1] as char;
            i += 2;
            match spec {
                'Y' => out.push_str(&format!("{:04}", year)),
                'y' => out.push_str(&format!("{:02}", year.rem_euclid(100))),
                'm' => out.push_str(&format!("{:02}", month)),
                'd' => out.push_str(&format!("{:02}", day)),
                'e' => out.push_str(&format!("{:2}", day)),
                'H' => out.push_str(&format!("{:02}", hours)),
                'I' => out.push_str(&format!("{:02}", hour12)),
                'M' => out.push_str(&format!("{:02}", minutes)),
                'S' => out.push_str(&format!("{:02}", seconds)),
                'p' => out.push_str(ampm),
                'P' => out.push_str(&ampm.to_lowercase()),
                'j' => out.push_str(&format!("{:03}", day_of_year)),
                'w' => out.push_str(&format!("{}", weekday)),
                'A' => out.push_str(DAYS[weekday as usize]),
                'a' => out.push_str(&DAYS[weekday as usize][..3]),
                'B' => out.push_str(MONTHS[(month - 1) as usize]),
                'b' | 'h' => out.push_str(&MONTHS[(month - 1) as usize][..3]),
                'F' => out.push_str(&format!("{:04}-{:02}-{:02}", year, month, day)),
                'T' => out.push_str(&format!("{:02}:{:02}:{:02}", hours, minutes, seconds)),
                'R' => out.push_str(&format!("{:02}:{:02}", hours, minutes)),
                'D' => out.push_str(&format!("{:02}/{:02}/{:02}", month, day, year.rem_euclid(100))),
                's' => out.push_str(&format!("{}", total_secs)),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '%' => out.push('%'),
                other => {
                    out.push('%');
                    out.push(other);
                }
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(Value::Str(out))
}

/// Days since 1970-01-01 for a civil date (proleptic Gregorian), via Hinnant's algorithm.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Civil (year, month, day) from days since 1970-01-01 (Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
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
            let days = days_from_civil(y, m as u32, d as u32);
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
        Value::ListShared(items) => {
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
        Value::ListShared(items) => {
            let mut out = items.clone();
            if out.len() > 1 {
                for i in (1..out.len()).rev() {
                    let j = (random_next_u64() % ((i + 1) as u64)) as usize;
                    out.swap(i, j);
                }
            }
            Ok(Value::ListShared(out))
        }
        _ => Err(format!("random_shuffle expects a list, got {}", value.type_name())),
    }
}

fn value_to_f64(name: &str, value: &Value) -> Result<f64, String> {
    value
        .as_number()
        .ok_or_else(|| format!("{name} expects numeric arguments"))
}

/// Call a builtin with a single argument. Used by the higher-order list
/// builtins (takewhile/dropwhile/partition/group_by/max_key/min_key). Like the
/// existing `map`/`filter`, these reach native builtins by name.
fn apply_builtin1(fn_name: &str, arg: &Value) -> Result<Value, String> {
    match invoke_builtin(fn_name, &[arg.clone()]) {
        Some(Ok(v)) => Ok(v),
        Some(Err(e)) => Err(e),
        None => Err(format!("function '{}' not found", fn_name)),
    }
}

fn gcd_i64(mut a: i64, mut b: i64) -> i64 {
    if a < 0 {
        a = -a;
    }
    if b < 0 {
        b = -b;
    }
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
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

/// Convert a parsed JSON tree into an Indent value, **consuming** the tree.
///
/// Taking `JsonValue` by value matters a lot: this runs once per node of every
/// `json_loads`/`json_parse`, and the previous `&JsonValue` version cloned every
/// string, every object key and every array element, then let each `HashMap`
/// grow by rehashing from empty. Object keys and strings are now moved, and the
/// containers are reserved up front. Decoding a JSON response is on the hot path
/// of every API call a script makes, so the allocation count here is a
/// first-order cost.
fn json_to_value(value: JsonValue) -> Value {
    match value {
        JsonValue::Null => Value::Empty,
        JsonValue::Bool(v) => Value::Bool(v),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Empty
            }
        }
        JsonValue::String(s) => Value::Str(s),
        JsonValue::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            out.extend(items.into_iter().map(json_to_value));
            Value::List(out)
        }
        JsonValue::Object(map) => {
            let mut out = HashMap::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k, json_to_value(v));
            }
            Value::Dict(out.into())
        }
    }
}

fn parse_socket_id(value: &Value) -> Result<i64, String> {
    match value {
        Value::Int(v) => Ok(*v),
        Value::StrShared(s) => s
            .trim()
            .parse::<i64>()
            .map_err(|_| "socket_id must be an int".to_string()),
        // `ws_connect` returns {"ok": true, "socket_id": N}, so accept that dict
        // directly: `ws_send_text (ws_connect url) "hi"` is the form the guide
        // documents, and requiring every caller to pull out ["socket_id"] first is
        // the sort of thing nobody discovers from the source.
        Value::DictShared(map) => match map.get("socket_id") {
            Some(Value::Int(v)) => Ok(*v),
            Some(Value::StrShared(s)) => s
                .trim()
                .parse::<i64>()
                .map_err(|_| "socket_id must be an int".to_string()),
            _ => Err("socket_id must be an int, or the dict returned by ws_connect".to_string()),
        },
        _ => Err("socket_id must be an int, or the dict returned by ws_connect".to_string()),
    }
}

/// Open sockets are resources, not values: a script that reconnects on network
/// errors without ever calling `ws_close` would otherwise accumulate them (and
/// their buffers) for the life of the process. Past this many, connecting closes
/// the oldest socket instead.
const WS_CLIENT_MAX: usize = 256;

fn ws_connect_builtin(url: &str) -> Result<Value, String> {
    let (socket, _) = ws_connect_blocking(url).map_err(|e| format!("ws_connect failed: {e}"))?;

    let socket_id = WS_NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let mut sockets = WS_CLIENTS
        .lock()
        .map_err(|_| "ws_connect failed: socket registry lock poisoned".to_string())?;
    // Ids increase monotonically, so the lowest id is the oldest connection.
    if sockets.len() >= WS_CLIENT_MAX {
        if let Some(oldest) = sockets.keys().min().copied() {
            sockets.remove(&oldest);
        }
    }
    // Bound the socket table. Nothing but an explicit `ws_close` ever removed an
    // entry, so a client that errors out and reconnects — the ordinary shape of a
    // bot on a flaky network — accumulated dead sockets and their buffers for the
    // life of the process. Socket ids increase monotonically, so the smallest key
    // is the oldest connection; dropping the entry drops the socket with it.
    if sockets.len() >= WS_CLIENTS_MAX && !sockets.contains_key(&socket_id) {
        if let Some(oldest) = sockets.keys().min().copied() {
            sockets.remove(&oldest);
        }
    }
    sockets.insert(socket_id, socket);

    let mut out = HashMap::new();
    out.insert("ok".to_string(), Value::Bool(true));
    out.insert("socket_id".to_string(), Value::Int(socket_id));
    Ok(Value::Dict(out.into()))
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
    Ok(Value::Dict(out.into()))
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
                return Ok(Value::Dict(out.into()));
            }
            WsMessage::Binary(data) => {
                let mut out = HashMap::new();
                out.insert("ok".to_string(), Value::Bool(true));
                out.insert("socket_id".to_string(), Value::Int(socket_id));
                out.insert(
                    "text".to_string(),
                    Value::Str(String::from_utf8_lossy(&data).to_string()),
                );
                return Ok(Value::Dict(out.into()));
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
                return Ok(Value::Dict(out.into()));
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
                return Ok(Value::Dict(out.into()));
            }
            Err(tungstenite::Error::ConnectionClosed)
            | Err(tungstenite::Error::AlreadyClosed) => {
                let mut out = HashMap::new();
                out.insert("ok".to_string(), Value::Bool(false));
                out.insert("socket_id".to_string(), Value::Int(socket_id));
                out.insert("closed".to_string(), Value::Bool(true));
                return Ok(Value::Dict(out.into()));
            }
            Err(err) => {
                let msg = err.to_string().to_lowercase();
                if msg.contains("closed") || msg.contains("connection reset") || msg.contains("broken pipe") {
                    let mut out = HashMap::new();
                    out.insert("ok".to_string(), Value::Bool(false));
                    out.insert("socket_id".to_string(), Value::Int(socket_id));
                    out.insert("closed".to_string(), Value::Bool(true));
                    return Ok(Value::Dict(out.into()));
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
                return Ok(Value::Dict(out.into()));
            }
            WsMessage::Binary(data) => {
                let mut out = HashMap::new();
                out.insert("ok".to_string(), Value::Bool(true));
                out.insert("socket_id".to_string(), Value::Int(socket_id));
                out.insert(
                    "text".to_string(),
                    Value::Str(String::from_utf8_lossy(&data).to_string()),
                );
                return Ok(Value::Dict(out.into()));
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
                return Ok(Value::Dict(out.into()));
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
    Ok(Value::Dict(out.into()))
}

fn value_to_json(value: &Value) -> JsonValue {
    match value {
        Value::Int(v) => JsonValue::from(*v),
        Value::Float(v) => JsonValue::from(*v),
        Value::Bool(v) => JsonValue::from(*v),
        Value::StrShared(v) => JsonValue::String(v.to_string()),
        Value::ListShared(items) => JsonValue::Array(items.iter().map(value_to_json).collect()),
        Value::SetShared(items) => JsonValue::Array(items.iter().map(value_to_json).collect()),
        Value::DictShared(items) => {
            let mut out = JsonMap::new();
            for (k, v) in items {
                out.insert(k.clone(), value_to_json(v));
            }
            JsonValue::Object(out)
        }
        Value::Func(name) => JsonValue::String(name.clone()),
        Value::Module(_) | Value::Empty => JsonValue::Null,
        Value::Object { class, fields } => {
            let mut out = JsonMap::new();
            out.insert("__class__".to_string(), JsonValue::from(class.name.clone()));
            // Declaration order, which is slot order: the keys come out in a
            // fixed order rather than whatever a hash map happened to have.
            for (name, value) in class.field_names.iter().zip(fields.iter()) {
                out.insert(name.clone(), value_to_json(value));
            }
            JsonValue::Object(out)
        },
    }
}

/// Shared HTTP client — created once and reused for every request. Reusing the
/// client keeps the connection pool (and TLS sessions) alive, so successive
/// calls to the same host skip the TCP + TLS handshake. This is the single
/// biggest HTTP performance win for a script that polls the same host.
static HTTP_CLIENT: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    reqwest::blocking::Client::builder()
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
});

/// Per-request timeout in milliseconds. 0 = use the default (30s). Settable at
/// runtime with the `http_set_timeout(seconds)` builtin.
static HTTP_TIMEOUT_MS: AtomicU64 = AtomicU64::new(0);

fn http_timeout() -> Duration {
    let ms = HTTP_TIMEOUT_MS.load(Ordering::Relaxed);
    Duration::from_millis(if ms == 0 { 30_000 } else { ms })
}

fn http_response_value(status: i64, body: String, headers: HashMap<String, Value>) -> Value {
    let mut out = HashMap::with_capacity(4);
    out.insert("ok".to_string(), Value::Bool((200..300).contains(&status)));
    out.insert("status".to_string(), Value::Int(status));
    out.insert("body".to_string(), Value::Str(body));
    out.insert("headers".to_string(), Value::Dict(headers.into()));
    Value::Dict(out.into())
}

/// Collect response headers into an Indent dict (lowercased names → values).
/// Reserved up front: this runs on every single request, and an empty `HashMap`
/// rehashes its way up to the real size.
fn http_headers_map(resp: &reqwest::blocking::Response) -> HashMap<String, Value> {
    let mut out = HashMap::with_capacity(resp.headers().len());
    for (k, v) in resp.headers().iter() {
        if let Ok(s) = v.to_str() {
            // Header names are already lowercase in HTTP/2 and usually in HTTP/1.1,
            // so only pay for the rewrite when there is actually an uppercase byte.
            let name = k.as_str();
            let key = if name.bytes().any(|b| b.is_ascii_uppercase()) {
                name.to_ascii_lowercase()
            } else {
                name.to_string()
            };
            out.insert(key, Value::Str(s.to_string()));
        }
    }
    out
}

fn http_get_builtin(url: &str, authorization: &str) -> Result<Value, String> {
    let mut req = HTTP_CLIENT.get(url).timeout(http_timeout());
    if !authorization.trim().is_empty() {
        req = req.header(reqwest::header::AUTHORIZATION, authorization.trim());
    }

    let resp = req.send().map_err(|e| format!("http_get request failed: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let headers = http_headers_map(&resp);
    let body = resp
        .text()
        .map_err(|e| format!("http_get failed to read response body: {e}"))?;
    Ok(http_response_value(status, body, headers))
}

fn http_post_json_builtin(url: &str, payload: Value, authorization: &str) -> Result<Value, String> {
    let json_payload = value_to_json(&payload);

    let mut req = HTTP_CLIENT
        .post(url)
        .timeout(http_timeout())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&json_payload);

    if !authorization.trim().is_empty() {
        req = req.header(reqwest::header::AUTHORIZATION, authorization.trim());
    }

    let resp = req
        .send()
        .map_err(|e| format!("http_post_json request failed: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let headers = http_headers_map(&resp);
    let body = resp
        .text()
        .map_err(|e| format!("http_post_json failed to read response body: {e}"))?;
    Ok(http_response_value(status, body, headers))
}

fn http_put_json_builtin(url: &str, payload: Value, authorization: &str) -> Result<Value, String> {
    let json_payload = value_to_json(&payload);

    let mut req = HTTP_CLIENT
        .put(url)
        .timeout(http_timeout())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&json_payload);

    if !authorization.trim().is_empty() {
        req = req.header(reqwest::header::AUTHORIZATION, authorization.trim());
    }

    let resp = req
        .send()
        .map_err(|e| format!("http_put_json request failed: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let headers = http_headers_map(&resp);
    let body = resp
        .text()
        .map_err(|e| format!("http_put_json failed to read response body: {e}"))?;
    Ok(http_response_value(status, body, headers))
}

fn http_patch_json_builtin(url: &str, payload: Value, authorization: &str) -> Result<Value, String> {
    let json_payload = value_to_json(&payload);

    let mut req = HTTP_CLIENT
        .patch(url)
        .timeout(http_timeout())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&json_payload);

    if !authorization.trim().is_empty() {
        req = req.header(reqwest::header::AUTHORIZATION, authorization.trim());
    }

    let resp = req
        .send()
        .map_err(|e| format!("http_patch_json request failed: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let headers = http_headers_map(&resp);
    let body = resp
        .text()
        .map_err(|e| format!("http_patch_json failed to read response body: {e}"))?;
    Ok(http_response_value(status, body, headers))
}

fn http_delete_builtin(url: &str, authorization: &str) -> Result<Value, String> {
    let mut req = HTTP_CLIENT.delete(url).timeout(http_timeout());

    if !authorization.trim().is_empty() {
        req = req.header(reqwest::header::AUTHORIZATION, authorization.trim());
    }

    let resp = req
        .send()
        .map_err(|e| format!("http_delete request failed: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let headers = http_headers_map(&resp);
    let body = resp
        .text()
        .map_err(|e| format!("http_delete failed to read response body: {e}"))?;
    Ok(http_response_value(status, body, headers))
}

/// Generic request: any HTTP method, optional JSON body, optional auth header,
/// and optional extra headers. Powers rate-limited/audited API clients.
fn http_request_builtin(
    method: &str,
    url: &str,
    payload: &Value,
    authorization: &str,
    extra_headers: &Value,
) -> Result<Value, String> {
    let m = method.trim().to_ascii_uppercase();
    let reqwest_method = reqwest::Method::from_bytes(m.as_bytes())
        .map_err(|_| format!("http_request: invalid method '{method}'"))?;
    let mut req = HTTP_CLIENT.request(reqwest_method, url).timeout(http_timeout());

    if !authorization.trim().is_empty() {
        req = req.header(reqwest::header::AUTHORIZATION, authorization.trim());
    }

    if let Value::DictShared(map) = extra_headers {
        for (k, v) in map {
            req = req.header(k.as_str(), v.to_string());
        }
    }

    if !matches!(payload, Value::Empty) {
        let json_payload = value_to_json(payload);
        req = req.header(reqwest::header::CONTENT_TYPE, "application/json").json(&json_payload);
    }

    let resp = req
        .send()
        .map_err(|e| format!("http_request request failed: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let headers = http_headers_map(&resp);
    let body = resp
        .text()
        .map_err(|e| format!("http_request failed to read response body: {e}"))?;
    Ok(http_response_value(status, body, headers))
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

/// Build a `{status, body, content_type}` response dict.
fn dict_response(status: i64, body: &str, ct: &str) -> Value {
    let mut m = HashMap::new();
    m.insert("status".to_string(), Value::Int(status));
    m.insert("body".to_string(), Value::Str(body.to_string()));
    m.insert("content_type".to_string(), Value::Str(ct.to_string()));
    Value::Dict(m.into())
}

fn http_status_text(code: i64) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'%' && i + 2 < bytes.len() {
            let h = |b: u8| -> Option<u8> {
                match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    b'A'..=b'F' => Some(b - b'A' + 10),
                    _ => None,
                }
            };
            if let (Some(hi), Some(lo)) = (h(bytes[i + 1]), h(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        if c == b'+' {
            out.push(b' ');
        } else {
            out.push(c);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Parse a raw HTTP request into a request dict:
/// `{method, path, query: {..}, headers: {..}, body}`.
fn parse_http_request(raw: &str) -> Value {
    let mut lines = raw.lines();
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query_str) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.clone(), String::new()),
    };

    let mut headers: HashMap<String, Value> = HashMap::new();
    let mut body = String::new();
    let mut in_body = false;
    for l in lines {
        if !in_body {
            if l.trim().is_empty() {
                in_body = true;
                continue;
            }
            if let Some((k, v)) = l.split_once(':') {
                headers.insert(k.trim().to_lowercase(), Value::Str(v.trim().to_string()));
            }
        } else {
            body.push_str(l);
            body.push('\n');
        }
    }

    let mut query: HashMap<String, Value> = HashMap::new();
    for pair in query_str.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (url_decode(k), url_decode(v)),
            None => (url_decode(pair), String::new()),
        };
        query.insert(k, Value::Str(v));
    }

    let mut m = HashMap::new();
    m.insert("method".to_string(), Value::Str(method));
    m.insert("path".to_string(), Value::Str(path));
    m.insert("query".to_string(), Value::Dict(query.into()));
    m.insert("headers".to_string(), Value::Dict(headers.into()));
    m.insert("body".to_string(), Value::Str(body.trim_end().to_string()));
    Value::Dict(m.into())
}

/// Write an HTTP response. `resp` is either a string (→ 200 text/html body) or
/// a dict with optional `status` / `body` / `content_type` / `headers`.
fn write_http_response(stream: &mut TcpStream, resp: &Value) -> Result<(), String> {
    let mut status = 200i64;
    let mut body = String::new();
    let mut content_type = "text/html; charset=utf-8".to_string();
    let mut extra: Vec<(String, String)> = Vec::new();
    match resp {
        Value::StrShared(s) => body = s.to_string(),
        Value::DictShared(m) => {
            if let Some(Value::Int(s)) = m.get("status") {
                status = *s;
            }
            if let Some(v) = m.get("body") {
                body = v.to_string();
            }
            if let Some(Value::StrShared(ct)) = m.get("content_type") {
                content_type = ct.to_string();
            }
            if let Some(Value::DictShared(h)) = m.get("headers") {
                for (k, v) in h {
                    extra.push((k.clone(), v.to_string()));
                }
            }
        }
        _ => {}
    }
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        status,
        http_status_text(status),
        content_type,
        body.len()
    );
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    use std::io::Write;
    stream
        .write_all(head.as_bytes())
        .map_err(|e| format!("failed to write response head: {e}"))?;
    stream
        .write_all(body.as_bytes())
        .map_err(|e| format!("failed to write response body: {e}"))
}

/// Dynamic HTTP server: for every request, build a request dict and call the
/// Indent `handler` function with it. The handler returns a response (string or
/// dict). Handlers run in a fresh scope (stateless per request).
fn http_serve_builtin(
    handler: &str,
    port: u16,
    ctx: &mut ExecContext<'_>,
) -> Result<Value, String> {
    let bind_addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&bind_addr)
        .map_err(|e| format!("http_serve: cannot bind to {bind_addr}: {e}"))?;
    eprintln!("Indent HTTP server listening on http://{bind_addr}");

    let callable = resolve_callable(handler, ctx)
        .map_err(|e| format!("http_serve: handler '{handler}' not found: {e}"))?;
    let module_dir = ctx.rt.module_dir.clone();
    let funcs = ctx.rt.funcs.clone();
    let callables = ctx.rt.callables.clone();
    let classes = ctx.rt.classes.clone();
    let async_funcs = ctx.rt.async_funcs.clone();

    use std::io::Read;
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let mut buf = [0u8; 65536];
                let n = match stream.read(&mut buf) {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                if n == 0 {
                    continue;
                }
                let raw = String::from_utf8_lossy(&buf[..n]).to_string();
                let req = parse_http_request(&raw);
                let result = {
                    let mut rt = Runtime::new(module_dir.clone());
                    rt.funcs = funcs.clone();
                    rt.callables = callables.clone();
                    rt.classes = classes.clone();
                    rt.async_funcs = async_funcs.clone();
                    let mut ctx2 = ExecContext::new(&mut rt);
                    ctx2.push_frame();
                    match &callable {
                        Callable::Local(f) => {
                            invoke_function(f, &[req.clone()], &HashMap::new(), &mut ctx2)
                        }
                        Callable::Builtin(name) => invoke_builtin(name, &[req.clone()])
                            .unwrap_or_else(|| {
                                Err(format!("http_serve: unknown builtin '{name}'"))
                            }),
                        Callable::External { module, name } => invoke_external_function(
                            module.clone(),
                            name,
                            &[req.clone()],
                            &HashMap::new(),
                            &mut ctx2,
                        ),
                    }
                };
                match result {
                    Ok(resp) => {
                        let _ = write_http_response(&mut stream, &resp);
                    }
                    Err(e) => {
                        let err = format!("Internal Server Error\n\n{e}");
                        let resp = dict_response(500, &err, "text/plain");
                        let _ = write_http_response(&mut stream, &resp);
                    }
                }
            }
            Err(_) => {}
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
    // Type names are lowercase in virtually every program, and this runs for
    // every `var` declaration — including one inside a loop body, so once per
    // iteration — and for every typed parameter of every call. `to_lowercase`
    // allocates a `String`, so only pay for it when there is actually an
    // uppercase byte to fold.
    let lowered;
    let ty = if ty.bytes().any(|b| b.is_ascii_uppercase()) {
        lowered = ty.to_lowercase();
        lowered.as_str()
    } else {
        ty
    };
    match ty {
        "string" => match value {
            Value::StrShared(_) => Ok(value),
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
            Value::ListShared(_) => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' expects list")),
        },
        "dictionary" | "dict" => match value {
            Value::DictShared(_) => Ok(value),
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
            Value::StrShared(s) if is_valid_color_literal(&s) => Ok(Value::StrShared(s)),
            _ => Err(format!("Line {line}: variable '{name}' expects color")),
        },
        "dynamic" => Ok(value),
        _ => Err(format!("Unknown type '{ty}' for '{name}'")),
    }
}

fn convert_type(line: usize, name: &str, ty: &str, value: Value) -> Result<Value, String> {
    // See `coerce_type`: avoid the allocating `to_lowercase` for the normal
    // already-lowercase type name.
    let lowered;
    let ty = if ty.bytes().any(|b| b.is_ascii_uppercase()) {
        lowered = ty.to_lowercase();
        lowered.as_str()
    } else {
        ty
    };
    match ty {
        "string" => Ok(Value::Str(value.to_string())),
        "int" => match value {
            Value::Int(_) => Ok(value),
            Value::Float(v) if v.fract().abs() < f64::EPSILON => Ok(Value::Int(v as i64)),
            Value::StrShared(text) => text.trim().parse::<i64>().map(Value::Int).map_err(|_| {
                format!("Line {line}: variable '{name}' cannot be converted to int")
            }),
            _ => Err(format!("Line {line}: variable '{name}' cannot be converted to int")),
        },
        "float" => match value {
            Value::Float(_) => Ok(value),
            Value::Int(v) => Ok(Value::Float(v as f64)),
            Value::StrShared(text) => text.trim().parse::<f64>().map(Value::Float).map_err(|_| {
                format!("Line {line}: variable '{name}' cannot be converted to float")
            }),
            _ => Err(format!("Line {line}: variable '{name}' cannot be converted to float")),
        },
        "boolean" => match value {
            Value::Bool(_) => Ok(value),
            Value::Int(v) => Ok(Value::Bool(v != 0)),
            Value::Float(v) => Ok(Value::Bool(v != 0.0)),
            Value::StrShared(text) => {
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
            Value::ListShared(_) => Ok(value),
            _ => Err(format!("Line {line}: variable '{name}' cannot be converted to list")),
        },
        "group" | "set" => match value {
            Value::SetShared(_) => Ok(value),
            Value::ListShared(v) => {
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
            Value::DictShared(_) => Ok(value),
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
            Value::StrShared(text) if is_valid_color_literal(&text) => Ok(Value::StrShared(text)),
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
    Member { target: Box<Expr>, field: String },
    MethodCall { target: Box<Expr>, method: String, args: Vec<Expr> },
}

// Preprocess an expression string: convert space-separated function calls
// like "len text" into parenthesized form "len(text)" so they work in
// all expression contexts (say, if, is-assign, etc.).
fn preprocess_expr_calls(expr: &str) -> String {
    // Only convert if no parentheses are already present (avoid double-processing)
    if expr.contains('(') || expr.contains(')') {
        return expr.to_string();
    }
    // Handle a leading `not ` so `not fn args...` (e.g. `not has_key d k`)
    // parses correctly — previously the leftover args caused a syntax error.
    if let Some(rest) = expr.strip_prefix("not ") {
        let inner = preprocess_expr_calls(rest.trim());
        return format!("not {}", inner);
    }
    // Check if it looks like a simple space-separated call: callee arg1 arg2 ...
    if let Some((callee, args_text)) = expr.split_once(' ') {
        if looks_like_callee(callee)
            && !is_structural_keyword(callee)
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

/// Preprocess (space-call → parenthesized call) + lex + parse an expression
/// string into an AST. Pure: the result depends only on the text.
fn parse_expr_ast(expr: &str) -> Result<Expr, String> {
    let processed = preprocess_expr_calls(expr);
    let mut lexer = Lexer::new(&processed);
    let tokens = lexer.tokenize()?;
    let mut parser = ExprParser::new(tokens);
    let ast = parser.parse_expr()?;
    if !parser.is_done() {
        return Err(format!("Unexpected token in expression '{}'", expr));
    }
    Ok(ast)
}

/// Cheap classification of the shapes that account for most evaluations, so the
/// common case never touches the parse cache (or its lock) at all.
fn fast_literal(expr: &str) -> Option<Value> {
    let bytes = expr.as_bytes();
    let first = *bytes.first()?;

    // Bare identifier: `i`, `total`, `user_name` → a plain variable read.
    if first.is_ascii_alphabetic() || first == b'_' {
        if bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            return None; // handled by the caller (needs runtime lookup)
        }
    }

    // Integer literal.
    if first.is_ascii_digit() || (first == b'-' && bytes.len() > 1 && bytes[1].is_ascii_digit()) {
        if let Ok(n) = expr.parse::<i64>() {
            return Some(Value::Int(n));
        }
    }

    // Double-quoted string with no escapes and no interpolation: the inner text
    // is already the literal value.
    if first == b'"' && bytes.len() >= 2 && bytes[bytes.len() - 1] == b'"' {
        let inner = &expr[1..expr.len() - 1];
        if !inner.contains('"') && !inner.contains('\\') && !inner.contains('%') {
            return Some(Value::Str(inner.to_string()));
        }
    }

    None
}

/// True when `expr` is a bare identifier (letters/digits/underscore).
fn is_bare_identifier(expr: &str) -> bool {
    let bytes = expr.as_bytes();
    match bytes.first() {
        Some(b) if b.is_ascii_alphabetic() || *b == b'_' => {}
        _ => return false,
    }
    bytes.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

fn eval_expr(expr: &str, ctx: &mut ExecContext<'_>) -> Result<Value, String> {
    // Memo first: everything below this point is per-evaluation work on the
    // expression *text*, and a loop repeats the same handful of texts.
    match ctx.memo_get(expr) {
        Some(ExprKind::Literal(v)) => return Ok(v),
        Some(ExprKind::Ast(ast)) => return eval_ast(&ast, ctx),
        Some(ExprKind::Ident) => {
            // Only the shape is cached, never the value, so this still reads the
            // current scope (and still falls through if it is not a variable).
            if let Some(v) = ctx.get_var(expr) {
                return Ok(v);
            }
        }
        None => {}
    }

    // Fast path 1: bare identifier → direct variable read, no parse at all.
    if is_bare_identifier(expr) {
        ctx.memo_put(expr, ExprKind::Ident);
        if let Some(v) = ctx.get_var(expr) {
            return Ok(v);
        }
        // Not a variable: fall through so the normal path reports it (or it
        // resolves as a zero-arg function call).
    }

    // Fast path 2: plain literals.
    if let Some(v) = fast_literal(expr) {
        ctx.memo_put(expr, ExprKind::Literal(v.clone()));
        return Ok(v);
    }

    // Otherwise use the shared parse cache. Parsing used to happen here on
    // *every* evaluation; now it happens once per distinct expression string.
    let ast = ctx.rt.cached_expr(expr)?;
    ctx.memo_put(expr, ExprKind::Ast(ast.clone()));
    eval_ast(&ast, ctx)
}

fn eval_ast(expr: &Expr, ctx: &mut ExecContext<'_>) -> Result<Value, String> {
    match expr {
        Expr::Int(v) => Ok(Value::Int(*v)),
        Expr::Float(v) => Ok(Value::Float(*v)),
        Expr::Bool(v) => Ok(Value::Bool(*v)),
        Expr::Str(v) => {
            if v.contains('%') {
                Ok(Value::Str(interpolate_string(v, ctx)))
            } else {
                Ok(Value::Str(v.clone()))
            }
        }
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
                    Value::StrShared(s) => s.into_string(),
                    _ => return Err("Dictionary keys must evaluate to string values".to_string()),
                };
                let value = eval_ast(v_expr, ctx)?;
                out.insert(key, value);
            }
            Ok(Value::Dict(out.into()))
        }
        Expr::Var(parts) => resolve_var_chain(parts, ctx),
        Expr::Index { target, index } => {
            // Fast path: `container[i]` where the container is a bare variable.
            //
            // Reading a variable clones its value, so indexing a 20 000-item
            // list cloned the whole list to hand back one element — O(n) per
            // access, O(n²) for a loop over the list. Borrow the container and
            // clone only the element. Objects are excluded because their
            // `get_item` method needs a mutable context, and positional aliases
            // are excluded because `get_var` reads those from the frame's
            // argument list, not from its variables.
            if let Expr::Var(parts) = target.as_ref() {
                let borrowable = parts.len() == 1
                    && borrowable_var(&parts[0])
                    && matches!(
                        ctx.get_var_ref(&parts[0]),
                        Some(Value::ListShared(_) | Value::StrShared(_) | Value::DictShared(_))
                    );
                if borrowable {
                    let index_val = eval_ast(index, ctx)?;
                    if let Some(base) = ctx.get_var_ref(&parts[0]) {
                        return resolve_index_ref(base, index_val);
                    }
                }
            }
            let target_val = eval_ast(target, ctx)?;
            let index_val = eval_ast(index, ctx)?;
            // Object `get_item` special method (natural name). Only an Object can
            // define it, so test the discriminant before handing over the index
            // (passing it eagerly cloned the index for every plain `xs[i]`).
            if matches!(&target_val, Value::Object { .. }) {
                if let Some(r) = call_object_method_value(
                    &target_val,
                    "get_item",
                    std::slice::from_ref(&index_val),
                    ctx,
                ) {
                    return r;
                }
            }
            resolve_index(target_val, index_val)
        }
        Expr::Slice {
            target,
            start,
            end,
            step,
        } => {
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

            // Borrowing fast path: slicing a bare container variable cloned the
            // whole container only to build the slice from it. The slice itself
            // is still a fresh value, so this is purely the clone that goes away.
            if let Expr::Var(parts) = target.as_ref() {
                let borrowable = parts.len() == 1
                    && borrowable_var(&parts[0])
                    && matches!(
                        ctx.get_var_ref(&parts[0]),
                        Some(Value::ListShared(_) | Value::StrShared(_))
                    );
                if borrowable {
                    if let Some(base) = ctx.get_var_ref(&parts[0]) {
                        return slice_builtin(base, start_bound, end_bound, step_value);
                    }
                }
            }

            let target_val = eval_ast(target, ctx)?;
            slice_builtin(&target_val, start_bound, end_bound, step_value)
        }
        Expr::Call { callee, args } => {
            // Fast path: `len <bare variable>`.
            //
            // Arguments are passed by value, so `len(bigList)` used to clone the
            // whole container just to read its length — 127 µs for a 20 000-item
            // list, and `len` is everywhere. Counting it in place is O(1). Only
            // taken when `len` has not been redefined, so shadowing still wins,
            // and the semantics match the builtin exactly (chars for a string).
            if callee.len() == 1 && callee[0] == "len" && args.len() == 1 {
                if let Expr::Var(parts) = &args[0] {
                    if parts.len() == 1
                        && !ctx.rt.callables.contains_key("len")
                        && !ctx.rt.funcs.contains_key("len")
                    {
                        if let Some(v) = ctx.get_var_ref(&parts[0]) {
                            match v {
                                Value::StrShared(s) => {
                                    return Ok(Value::Int(s.chars().count() as i64))
                                }
                                Value::ListShared(v) => return Ok(Value::Int(v.len() as i64)),
                                Value::SetShared(v) => return Ok(Value::Int(v.len() as i64)),
                                Value::DictShared(v) => return Ok(Value::Int(v.len() as i64)),
                                _ => {} // other types: normal path reports the error
                            }
                        }
                    }
                }
            }
            // Fast path: `has_key <bare variable> <key>` — same reasoning as
            // `len`: the dictionary was cloned (117 µs for 5 000 entries) purely
            // to run one lookup. Only taken when arg 0 really is a dictionary, so
            // the builtin still reports every error message itself.
            if callee.len() == 1 && callee[0] == "has_key" && args.len() == 2 {
                if !ctx.rt.callables.contains_key("has_key")
                    && !ctx.rt.funcs.contains_key("has_key")
                {
                    if let Expr::Var(parts) = &args[0] {
                        if parts.len() == 1
                            && matches!(ctx.get_var_ref(&parts[0]), Some(Value::DictShared(_)))
                        {
                            let key_val = eval_ast(&args[1], ctx)?;
                            if let Some(Value::DictShared(map)) = ctx.get_var_ref(&parts[0]) {
                                return Ok(Value::Bool(map.contains_key(&key_val.to_string())));
                            }
                        }
                    }
                }
            }
            let name = callee.join(".");
            invoke_callable_expr(&name, args, ctx)
        }
        Expr::Unary { op, rhs } => {
            let v = eval_ast(rhs, ctx)?;
            apply_unary(op, v)
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
            eval_binary(op, a, b, ctx)
        }
        Expr::Comprehension { kind, result_expr, key_expr, item_name, iterable, condition } => {
            let iter = eval_ast(iterable, ctx)?;
            let items = match iter {
                Value::ListShared(v) => v.into_vec(),
                Value::SetShared(v) => v.into_vec(),
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
                    Ok(Value::Dict(out.into()))
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
            Ok(Value::Dict(lambda_obj.into()))
        }
        Expr::Ternary { cond, true_expr, false_expr } => {
            let cond_val = eval_ast(cond, ctx)?;
            if cond_val.to_bool() {
                eval_ast(true_expr, ctx)
            } else {
                eval_ast(false_expr, ctx)
            }
        }
        Expr::Member { target, field } => {
            let target_val = eval_ast(target, ctx)?;
            resolve_member_access(target_val, field, ctx)
        }
        Expr::MethodCall { target, method, args } => {
            let target_val = eval_ast(target, ctx)?;
            let mut arg_vals = Vec::with_capacity(args.len());
            for a in args {
                arg_vals.push(eval_ast(a, ctx)?);
            }
            // A bare variable receiver is a place, so a method that assigns a
            // field changes the instance the variable holds. Anything else
            // (`a.b.m()`, `xs[0].m()`) stays a value, and its fields stop at the
            // method.
            if let Expr::Var(parts) = &**target {
                if parts.len() == 1 && is_bare_identifier(&parts[0]) {
                    if let Some(result) =
                        invoke_method_on_place(&parts[0], method, &arg_vals, ctx)
                    {
                        return result;
                    }
                }
            }
            invoke_method_on_value(target_val, method, &arg_vals, ctx)
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
        // The module scope counts too: inside a module, one of its own functions is
        // a name that can be passed around, and it used to be visible here only
        // because the whole table was injected into the runtime.
        if ctx.rt.callables.contains_key(first)
            || ctx.rt.funcs.contains_key(first)
            || ctx
                .module_scope
                .iter()
                .any(|m| m.entries.get(first).is_some_and(|e| !matches!(e, ModuleEntry::Value(_))))
        {
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
                match m.entries.get(p) {
                    Some(ModuleEntry::Value(v)) => cur = v.clone(),
                    Some(ModuleEntry::Func(_)) => {
                        return Err(format!("Module '{}' has function '{}' — use 'get {} from {}' to import it", module_name, p, p, module_name));
                    }
                    _ => {
                        return Err(format!("Module '{}' has no attribute '{}'", module_name, p));
                    }
                }
            }
            Value::DictShared(ref map) => {
                if let Some(v) = map.get(p) {
                    cur = v.clone();
                } else {
                    return Err(format!("Dictionary has no key '{p}'"));
                }
            }
            Value::Object { ref class, ref fields } => {
                if let Some(slot) = class.field_index.get(p) {
                    cur = fields[*slot as usize].clone();
                } else {
                    return Err(format!("Object has no field '{p}'"));
                }
            }
            // Everything else is a member of a value, and the rules for that live
            // in one place — `resolve_member_access_ref` — so a dotted name and an
            // `Expr::Member` cannot disagree about what `.length` or `.first`
            // means. Before this, the list, string and set arms there were
            // unreachable from a dotted name, so `xs.length` on a list failed
            // while the same field on the same container worked after a
            // non-name target such as a call or a literal.
            _ => cur = resolve_member_access_ref(&cur, p)?,
        }
    }
    Ok(cur)
}

/// Assign a value to a dotted variable chain like `bot.commands`.
/// For simple names (no dots), works like ctx.set_var.
/// For dotted chains, resolves the parent dict and sets the key on it.
fn assign_var_chain(name: &str, value: Value, ctx: &mut ExecContext<'_>) -> Result<(), String> {
    // Plain names (no dots) are the overwhelming majority, and this runs for
    // every assignment — including every iteration of `i is i + 1` in a loop.
    // `split('.').collect()` allocated a `Vec` for each one, only to find it had
    // a single element and take the trivial branch.
    if !name.contains('.') {
        ctx.set_var(name, value);
        return Ok(());
    }
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() == 1 {
        ctx.set_var(name, value);
        return Ok(());
    }

    // 2-part case: bot.commands — get bot dict, set commands key, write back
    let root_name = parts[0];
    let field = parts[1];
    match ctx.get_var(root_name) {
        Some(Value::DictShared(mut map)) => {
            map.insert(field.to_string(), value);
            ctx.set_var(root_name, Value::DictShared(map));
            Ok(())
        }
        // An instance. The field has to be one the class declared, which is the
        // rule reads already follow, so an unknown one is an error rather than a
        // silent drop. Before this arm existed the write went nowhere at all:
        // `p.x is 9` left `p.x` at its old value and only the read afterwards
        // complained, which made a declared field look read-only.
        Some(Value::Object { class, mut fields }) => {
            match class.field_index.get(field) {
                Some(slot) => {
                    fields[*slot as usize] = value;
                    ctx.set_var(root_name, Value::Object { class, fields });
                    Ok(())
                }
                None => Err(format!(
                    "Object of class '{}' has no field '{field}'",
                    class.name
                )),
            }
        }
        // Anything else keeps the old behaviour of leaving the chain alone.
        _ => Ok(()),
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
        Value::ListShared(items) => {
            let indices = build_slice_indices(items.len(), start, end, step)?;
            let mut out = Vec::with_capacity(indices.len());
            for idx in indices {
                if let Some(value) = items.get(idx) {
                    out.push(value.clone());
                }
            }
            Ok(Value::List(out))
        }
        Value::StrShared(text) => {
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
        Value::ListShared(items) => {
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
        Value::StrShared(s) => {
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
        Value::DictShared(map) => {
            let key = match index {
                Value::StrShared(s) => s,
                _ => return Err(format!("Dictionary key must be a string, got {}: {}", index.type_name(), index)),
            };
            map.get(&key)
                .cloned()
                .ok_or_else(|| format!("Dictionary key not found: {key}"))
        }
        _ => Err(format!("Indexing is not supported for {} values", target.type_name())),
    }
}

/// Borrowing counterpart of `resolve_index`, for callers that already hold the
/// container (see the `Expr::Index` fast path). The messages and the accepted
/// types are identical to `resolve_index` — only the selected element is
/// cloned instead of the whole container.
fn resolve_index_ref(target: &Value, index: Value) -> Result<Value, String> {
    match target {
        Value::ListShared(items) => {
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
        Value::StrShared(s) => {
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
        Value::DictShared(map) => {
            let key = match index {
                Value::StrShared(s) => s,
                _ => return Err(format!("Dictionary key must be a string, got {}: {}", index.type_name(), index)),
            };
            map.get(&key)
                .cloned()
                .ok_or_else(|| format!("Dictionary key not found: {key}"))
        }
        _ => Err(format!("Indexing is not supported for {} values", target.type_name())),
    }
}

/// Can `name` be read through `get_var_ref` with exactly the same result that
/// `get_var` would produce? False for the positional aliases
/// (`argument`, `argument2`, …), because those are resolved from the frame's
/// argument list and would otherwise be shadowed by same-named variables.
fn borrowable_var(name: &str) -> bool {
    positional_alias_index(name).is_none()
}

fn assign_index_value(target: Value, index: Value, replacement: Value) -> Result<Value, String> {
    match target {
        Value::ListShared(mut items) => {
            let raw = match index {
                Value::Int(i) => i,
                _ => return Err(format!("List index must be an int, got {}: {}", index.type_name(), index)),
            };
            let idx = normalize_index(items.len(), raw)
                .ok_or_else(|| format!("List index out of range: {raw}"))?;
            items[idx] = replacement;
            Ok(Value::ListShared(items))
        }
        Value::DictShared(mut map) => {
            let key = match index {
                Value::StrShared(s) => s.into_string(),
                _ => return Err(format!("Dictionary key must be a string, got {}: {}", index.type_name(), index)),
            };
            map.insert(key, replacement);
            Ok(Value::DictShared(map))
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
        Value::ListShared(items) => items,
        _ => return Err(format!("Slice assignment expects a list, got {}", replacement.type_name())),
    };

    match target {
        Value::ListShared(mut items) => {
            if step == 1 {
                let (start_idx, end_idx) = normalize_slice_window_step_one(items.len(), start, end);
                items.splice(start_idx..end_idx, replacement_items);
                return Ok(Value::ListShared(items));
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
            Ok(Value::ListShared(items))
        }
        _ => Err("Slice assignment is only supported for list values".to_string()),
    }
}

fn eval_binary(op: &str, a: Value, b: Value, ctx: &mut ExecContext<'_>) -> Result<Value, String> {
    // Object special-method dispatch using natural-language names. If either
    // operand is an Object that defines the matching method, call it and use
    // its result instead of the built-in behavior.
    //
    // The `matches!` guards matter a lot: only a `Value::Object` can have these
    // methods, but passing the other operand as an argument used to clone it
    // *eagerly* — so every `+`, `==` and `in` deep-copied both operands (a fresh
    // String or a whole list/dict) purely to discover there was no method to
    // call. A discriminant test replaces those copies.
    match op {
        "+" => {
            if matches!(&a, Value::Object { .. }) {
                if let Some(r) = call_object_method_value(&a, "add", std::slice::from_ref(&b), ctx) {
                    return r;
                }
            }
            if matches!(&b, Value::Object { .. }) {
                if let Some(r) = call_object_method_value(&b, "add", std::slice::from_ref(&a), ctx) {
                    return r;
                }
            }
        }
        "-" => {
            if matches!(&a, Value::Object { .. }) {
                if let Some(r) =
                    call_object_method_value(&a, "subtract", std::slice::from_ref(&b), ctx)
                {
                    return r;
                }
            }
        }
        "*" => {
            if matches!(&a, Value::Object { .. }) {
                if let Some(r) =
                    call_object_method_value(&a, "multiply", std::slice::from_ref(&b), ctx)
                {
                    return r;
                }
            }
        }
        "/" => {
            if matches!(&a, Value::Object { .. }) {
                if let Some(r) =
                    call_object_method_value(&a, "divide", std::slice::from_ref(&b), ctx)
                {
                    return r;
                }
            }
        }
        "==" => {
            if matches!(&a, Value::Object { .. }) {
                if let Some(r) =
                    call_object_method_value(&a, "equals", std::slice::from_ref(&b), ctx)
                {
                    return r.map(|v| Value::Bool(v.to_bool()));
                }
            }
            if matches!(&b, Value::Object { .. }) {
                if let Some(r) =
                    call_object_method_value(&b, "equals", std::slice::from_ref(&a), ctx)
                {
                    return r.map(|v| Value::Bool(v.to_bool()));
                }
            }
        }
        "!=" => {
            if matches!(&a, Value::Object { .. }) {
                if let Some(r) =
                    call_object_method_value(&a, "equals", std::slice::from_ref(&b), ctx)
                {
                    return r.map(|v| Value::Bool(!v.to_bool()));
                }
            }
            if matches!(&b, Value::Object { .. }) {
                if let Some(r) =
                    call_object_method_value(&b, "equals", std::slice::from_ref(&a), ctx)
                {
                    return r.map(|v| Value::Bool(!v.to_bool()));
                }
            }
        }
        "in" => {
            if matches!(&b, Value::Object { .. }) {
                if let Some(r) =
                    call_object_method_value(&b, "contains", std::slice::from_ref(&a), ctx)
                {
                    return r.map(|v| Value::Bool(v.to_bool()));
                }
            }
        }
        "not in" => {
            if matches!(&b, Value::Object { .. }) {
                if let Some(r) =
                    call_object_method_value(&b, "contains", std::slice::from_ref(&a), ctx)
                {
                    return r.map(|v| Value::Bool(!v.to_bool()));
                }
            }
        }
        _ => {}
    }

    match op {
        "+" => {
            let a_type = a.type_name();
            let b_type = b.type_name();
            match (a, b) {
            // `wrapping_add`, like every other integer arm here (`-` already used
            // `wrapping_sub`): overflow wraps in a release build anyway, and saying
            // so keeps a debug build from panicking where a release build does not
            // — and keeps the VM's integer fast path bit-identical to this one.
            (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x.wrapping_add(y))),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x + y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(x as f64 + y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x + y as f64)),
            (Value::StrShared(x), y) => Ok(Value::Str(format!("{}{}", x, y))),
            (x, Value::StrShared(y)) => Ok(Value::Str(format!("{}{}", x, y))),
            (Value::ListShared(mut x), Value::ListShared(y)) => {
                // `extend` grows the left operand, so a shared buffer copies here —
                // which is what `+` on two lists has to do: it must not change the
                // list it was given.
                x.extend(y);
                Ok(Value::ListShared(x))
            }
            (Value::SetShared(mut x), Value::SetShared(y)) => {
                // Union: add y elements not already in x
                for item in y {
                    let key = format!("{}", item);
                    if !x.iter().any(|v| format!("{}", v) == key) {
                        x.push(item);
                    }
                }
                Ok(Value::SetShared(x))
            }
            (Value::DictShared(mut x), Value::DictShared(y)) => {
                x.extend(y.into_inner());
                Ok(Value::DictShared(x))
            }
            _ => Err(format!("Cannot add {} and {} with '+'", a_type, b_type)),
            }
        },
        "-" => match exact_int_pair(&a, &b, EXACT_INT_MAG) {
            Some((x, y)) => Ok(Value::Int(x.wrapping_sub(y))),
            None => numeric_bin(a, b, |x, y| x - y),
        },
        "*" => {
            match (&a, &b) {
                (Value::StrShared(s), Value::Int(n)) | (Value::Int(n), Value::StrShared(s)) => {
                    let count = if *n < 0 { 0 } else { *n as usize };
                    Ok(Value::Str(s.repeat(count)))
                }
                (Value::ListShared(l), Value::Int(n)) | (Value::Int(n), Value::ListShared(l)) => {
                    let count = if *n < 0 { 0 } else { *n as usize };
                    let mut out = Vec::with_capacity(l.len() * count);
                    for _ in 0..count {
                        out.extend(l.iter().cloned());
                    }
                    Ok(Value::List(out))
                }
                (Value::Int(x), Value::Int(y))
                    if x.unsigned_abs() <= EXACT_INT_MUL_MAG
                        && y.unsigned_abs() <= EXACT_INT_MUL_MAG =>
                {
                    Ok(Value::Int(x.wrapping_mul(*y)))
                }
                _ => numeric_bin(a, b, |x, y| x * y),
            }
        },
        "**" => match (&a, &b) {
            (Value::Int(x), Value::Int(y)) if *y >= 0 && *y <= 62 => {
                if let Some(res) = x.checked_pow(*y as u32) {
                    Ok(Value::Int(res))
                } else {
                    Ok(Value::Float((*x as f64).powf(*y as f64)))
                }
            }
            _ => {
                let x = a.as_number().ok_or_else(|| format!("Expected number for '**', got {}", a.type_name()))?;
                let y = b.as_number().ok_or_else(|| format!("Expected number for '**', got {}", b.type_name()))?;
                Ok(Value::Float(x.powf(y)))
            }
        },
        "/" => match exact_int_pair(&a, &b, EXACT_INT_MAG) {
            // Only exact integer divisions: `7 / 2` is 3.5, and the float path is
            // what decides that, so a non-exact quotient must fall through.
            Some((x, y)) if y != 0 && x % y == 0 => Ok(Value::Int(x / y)),
            _ => numeric_bin(a, b, |x, y| x / y),
        },
        "%" => match exact_int_pair(&a, &b, EXACT_INT_MAG) {
            // `fmod` agrees with `%` for integer operands (both truncate toward
            // zero), but a zero divisor gives NaN on the float path — fall through.
            Some((x, y)) if y != 0 => Ok(Value::Int(x % y)),
            _ => numeric_bin(a, b, |x, y| x % y),
        },
        "==" => Ok(Value::Bool(eq_values(&a, &b))),
        "!=" => Ok(Value::Bool(!eq_values(&a, &b))),
        ">" => match exact_int_pair(&a, &b, EXACT_INT_MAG) {
            Some((x, y)) => Ok(Value::Bool(x > y)),
            None => cmp_bin(a, b, |x, y| x > y),
        },
        ">=" => match exact_int_pair(&a, &b, EXACT_INT_MAG) {
            Some((x, y)) => Ok(Value::Bool(x >= y)),
            None => cmp_bin(a, b, |x, y| x >= y),
        },
        "<" => match exact_int_pair(&a, &b, EXACT_INT_MAG) {
            Some((x, y)) => Ok(Value::Bool(x < y)),
            None => cmp_bin(a, b, |x, y| x < y),
        },
        "<=" => match exact_int_pair(&a, &b, EXACT_INT_MAG) {
            Some((x, y)) => Ok(Value::Bool(x <= y)),
            None => cmp_bin(a, b, |x, y| x <= y),
        },
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
        Value::ListShared(items) => Ok(items.iter().any(|v| eq_values(v, item))),
        Value::SetShared(items) => Ok(items.iter().any(|v| eq_values(v, item))),
        Value::DictShared(map) => {
            let key = match item {
                Value::StrShared(s) => s,
                _ => return Err(format!("'in' with dict expects a string key, got {}: {}", item.type_name(), item)),
            };
            Ok(map.contains_key(key))
        }
        Value::StrShared(text) => {
            let needle = match item {
                Value::StrShared(s) => s,
                _ => return Err(format!("'in' with string expects a string needle, got {}: {}", item.type_name(), item)),
            };
            Ok(text.contains(needle.as_str()))
        }
        _ => Err(format!("'in' expects a list, set, dict, or string container, got {}", container.type_name())),
    }
}

/// Largest operand magnitude for which `f64` arithmetic on two integers is
/// exactly equal to `i64` arithmetic on them, so an integer fast path cannot
/// change a result (`2^52 + 2^52` is still exactly representable).
const EXACT_INT_MAG: u64 = 1 << 52;
/// For `*` the *product* has to stay exactly representable, hence the smaller bound.
const EXACT_INT_MUL_MAG: u64 = 1 << 26;

/// The `(i64, i64)` operands of an arithmetic binary op, when both are ints and
/// both are small enough that `f64` arithmetic agrees with `i64` arithmetic.
///
/// `-`, `*`, `/`, `%` and the comparisons all funnel through `numeric_bin` /
/// `cmp_bin`, which convert both operands to `f64`, apply the operation and
/// convert the result back — a `cvtsi2sd`/`mulsd`/`cvttsd2si` round trip for what
/// is a one-instruction integer operation, on *every* `-`, `*`, `/`, `%` and `<`
/// in every program. Returning `None` for anything outside the exact range keeps
/// the floating-point semantics (including `7 / 2 == 3.5` and `1 / 0 == inf`)
/// untouched.
#[inline]
fn exact_int_pair(a: &Value, b: &Value, mag: u64) -> Option<(i64, i64)> {
    if let (Value::Int(x), Value::Int(y)) = (a, b) {
        if x.unsigned_abs() <= mag && y.unsigned_abs() <= mag {
            return Some((*x, *y));
        }
    }
    None
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
        (Value::StrShared(x), Value::StrShared(y)) => x == y,
        (Value::ListShared(x), Value::ListShared(y)) => {
            x.len() == y.len()
                && x
                    .iter()
                    .zip(y.iter())
                    .all(|(left, right)| eq_values(left, right))
        }
        (Value::SetShared(x), Value::SetShared(y)) => {
            x.len() == y.len()
                && x.iter().all(|item| {
                    let key = format!("{}", item);
                    y.iter().any(|other| format!("{}", other) == key && eq_values(item, other))
                })
        }
        (Value::DictShared(x), Value::DictShared(y)) => {
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
        (Value::StrShared(x), Value::StrShared(y)) => x == y,
        (Value::Func(x), Value::Func(y)) => x == y,
        (Value::Module(x), Value::Module(y)) => Arc::ptr_eq(x, y),
        (Value::Empty, Value::Empty) => true,
        (Value::Object { class: c1, .. }, Value::Object { class: c2, .. }) => {
            // Object identity: same class AND same field values
            c1.name == c2.name && eq_values(a, b)
        }
        (Value::ListShared(_), Value::ListShared(_)) | (Value::DictShared(_), Value::DictShared(_)) => {
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
                if ["==", "!=", ">=", "<=", "<<", ">>", "**"].contains(&sym) {
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
        if bytes[self.i] == b'0' && self.i + 1 < bytes.len() {
            let next_c = bytes[self.i + 1] as char;
            if next_c == 'x' || next_c == 'X' {
                self.i += 2;
                while self.i < bytes.len() && (bytes[self.i] as char).is_ascii_hexdigit() {
                    self.i += 1;
                }
                let hex_str = &self.src[start + 2..self.i];
                if let Ok(val) = i64::from_str_radix(hex_str, 16) {
                    return val.to_string();
                }
            } else if next_c == 'b' || next_c == 'B' {
                self.i += 2;
                while self.i < bytes.len() && (bytes[self.i] == b'0' || bytes[self.i] == b'1') {
                    self.i += 1;
                }
                let bin_str = &self.src[start + 2..self.i];
                if let Ok(val) = i64::from_str_radix(bin_str, 2) {
                    return val.to_string();
                }
            } else if next_c == 'o' || next_c == 'O' {
                self.i += 2;
                while self.i < bytes.len() && (bytes[self.i] >= b'0' && bytes[self.i] <= b'7') {
                    self.i += 1;
                }
                let oct_str = &self.src[start + 2..self.i];
                if let Ok(val) = i64::from_str_radix(oct_str, 8) {
                    return val.to_string();
                }
            }
        }
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
        let mut node = self.parse_power()?;
        loop {
            let op = match self.peek() {
                Some(Token::Sym(s)) if s == "*" || s == "/" || s == "%" => s.clone(),
                _ => break,
            };
            let _ = self.next();
            let rhs = self.parse_power()?;
            node = Expr::Binary {
                op,
                lhs: Box::new(node),
                rhs: Box::new(rhs),
            };
        }
        Ok(node)
    }

    fn parse_power(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Some(Token::Sym(s)) if s == "**" => s.clone(),
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
            if self.match_sym(".") {
                let field = match self.next() {
                    Some(Token::Ident(name)) => name,
                    tok => return Err(format!("Expected member name after '.', got {:?}", tok)),
                };
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
                    node = Expr::MethodCall {
                        target: Box::new(node),
                        method: field,
                        args,
                    };
                } else {
                    node = Expr::Member {
                        target: Box::new(node),
                        field,
                    };
                }
                continue;
            }

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

                match node {
                    Expr::Var(parts) => {
                        node = Expr::Call { callee: parts, args };
                    }
                    Expr::Member { target, field } => {
                        node = Expr::MethodCall {
                            target,
                            method: field,
                            args,
                        };
                    }
                    other => {
                        return Err(format!("Expression is not callable: {:?}", other));
                    }
                }
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
                    // If this dot is followed by an identifier AND a '(', it is a method call,
                    // so stop here and let parse_postfix handle it as a method call on this object/chain!
                    if self.peek_sym_n(1, "(") {
                        self.i -= 1; // unconsume the '.'
                        break;
                    }
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
                if !self.match_sym(")") {
                    if let Expr::Var(callee) = inner {
                        let mut args = Vec::new();
                        while !self.match_sym(")") {
                            let arg = self.parse_add()?;
                            args.push(arg);
                            if self.match_sym(",") {
                                continue;
                            }
                        }
                        return Ok(Expr::Call { callee, args });
                    }
                    self.expect_sym(")")?;
                }
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

    fn peek_sym_n(&self, n: usize, expected: &str) -> bool {
        match self.tokens.get(self.i + n) {
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

        let clean = strip_inline_comment(raw);
        // Tab expansion only when there is a tab: `str::replace` allocates a fresh
        // String even when there is nothing to replace, on every line of every file
        // that is parsed.
        let clean: std::borrow::Cow<'_, str> = if clean.contains('\t') {
            std::borrow::Cow::Owned(clean.replace('\t', "    "))
        } else {
            std::borrow::Cow::Borrowed(clean)
        };
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

/// The line with an inline `#!` comment removed — borrowed when there is nothing
/// to cut, which is the common case.
///
/// This runs for every line of every file that is parsed, and it used to build a
/// fresh `String` for each one whether or not the line had a comment. Finding the
/// cut point first is a scan with no allocation, so a line without a comment costs
/// nothing at all; the state machine is the same one, it just indexes instead of
/// appending.
fn strip_inline_comment(raw: &str) -> &str {
    let mut in_str = false;
    let mut quote = '\0';
    let mut escape = false;

    for (i, c) in raw.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' {
            escape = true;
            continue;
        }
        if in_str {
            if c == quote {
                in_str = false;
            }
            continue;
        }
        if c == '"' || c == '\'' {
            in_str = true;
            quote = c;
            continue;
        }
        // A `#` that is not the start of `#!` is an ordinary character, exactly as
        // the character-at-a-time version treated it.
        if c == '#' && raw[i + 1..].starts_with('!') {
            return &raw[..i];
        }
    }

    raw
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
            // Split: name type = value (or `is` — canonical for declarations)
            if let Some((name_and_type, value_expr)) = rest
                .split_once(" = ")
                .or_else(|| rest.split_once(" is "))
            {
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
            // Each spec is either `param` or `param=<default expression>`, where the
            // default may contain spaces (`= "Not Found"`, `= [1, 2]`).
            let mut tail: Vec<String> = split_inline_param_specs(rest);
            if !tail.is_empty() {
                tail.remove(0); // the name itself
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
                let is_generator = stmts_contain_yield(&body);
                return Ok(Stmt::DefFun {
                    line: line.line_no,
                    name,
                    params,
                    return_type,
                    body: body.into(),
                    is_generator,
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
                        body: Arc::new(Vec::new()),
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

            let is_generator = stmts_contain_yield(&body);
            return Ok(Stmt::DefFun {
                line: line.line_no,
                name,
                params,
                return_type,
                body: body.into(),
                is_generator,
                is_async: def_is_async,
            });
        }

        // ---- Indent-2: class / dataclass Name with fields and methods ----
        if let Some(rest) = text
            .strip_prefix("class ")
            .or_else(|| text.strip_prefix("dataclass "))
        {
            let is_dataclass = text.starts_with("dataclass ");
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

            return Ok(Stmt::DefClass(Box::new(DefClassStmt {
                line: line.line_no,
                name,
                parent,
                fields,
                methods,
                is_dataclass,
            })));
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

        // Launch: launch <path>  — run another Indent file in this runtime
        if text.starts_with("launch ") || text.starts_with("Launch ") {
            let expr = text[7..].trim().to_string();
            return Ok(Stmt::Launch {
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

        // `flag` raises an error. Accept both the space form (`flag "msg"`),
        // like `say`, and the colon form (`flag: "msg"`), for consistency.
        if let Some(rest) = text
            .strip_prefix("flag:")
            .or_else(|| text.strip_prefix("Flag:"))
            .or_else(|| text.strip_prefix("flag "))
            .or_else(|| text.strip_prefix("Flag "))
        {
            return Ok(Stmt::Flag {
                line: line.line_no,
                expr: rest.trim().to_string(),
            });
        }
        if text == "flag" || text == "Flag" {
            return Ok(Stmt::Flag {
                line: line.line_no,
                expr: "empty".to_string(),
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

        // Indent-2: get/import keyword for imports.
        // Only treat as an import when a module identifier follows — so a
        // user function named `Get` called as `Get a b` falls through to call
        // dispatch instead of erroring with "cannot import module 'a b'".
        if starts_like_import(text) {
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
                } => Stmt::AssignSlice(Box::new(AssignSliceStmt {
                    line: line.line_no,
                    name,
                    start_expr,
                    end_expr,
                    step_expr,
                    value,
                })),
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
            if looks_like_callee(callee) && !args_text.is_empty() && !is_structural_keyword(callee)
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
                args.push(ArgItem::DefVar(Box::new(stmt)));
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

/// Keywords that must never be treated as a call target when an *expression*
/// is preprocessed into space-separated-call form (lowercase structural forms
/// only). Capitalized identifiers such as `Get`, `Ask`, or `Set` are NOT
/// structural, so a user function named `Get` can be called as `Get a b` —
/// which previously made helpers such as `Get`/`GetUser` unreachable without
/// parentheses.
fn is_structural_keyword(s: &str) -> bool {
    matches!(
        s,
        "say" | "give" | "return" | "var" | "fun" | "if" | "or" | "otherwise"
            | "repeat" | "for" | "while" | "until" | "stop" | "break" | "next"
            | "continue" | "get" | "import" | "reset" | "restart" | "match"
            | "case" | "do" | "catch" | "lastly" | "maketype" | "set" | "flag"
            | "yield" | "open" | "is" | "ask" | "and" | "not" | "in" | "else"
            | "from" | "as" | "launch" | "wait" | "async" | "await" | "class"
            | "with"
    )
}

/// True when a statement is an import (`get X from Y`, `get X`, `import X`, …)
/// rather than a call to a user function named `Get`. An import must be
/// followed by a valid module identifier.
fn starts_like_import(text: &str) -> bool {
    let rest = if let Some(r) = text.strip_prefix("get ") {
        r
    } else if let Some(r) = text.strip_prefix("Get ") {
        r
    } else if let Some(r) = text.strip_prefix("get:") {
        r
    } else if let Some(r) = text.strip_prefix("Get:") {
        r
    } else {
        return false;
    };
    match rest.trim().split_whitespace().next() {
        Some(first) => is_identifier(first),
        None => false,
    }
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
    let (left, right) = text
        .split_once(" is ")
        .or_else(|| text.split_once(" = "))?;
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
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) { return "string"; }
    if s.starts_with('#') { return "color"; }
    if s.starts_with('[') && s.ends_with(']') { return "list"; }
    if s.starts_with('{') && s.ends_with('}') { return "dict"; }
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
    let (left_raw, right_raw) =
        match text.split_once(" is ").or_else(|| text.split_once(" = ")) {
            Some(x) => x,
            None => return Ok(None),
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

/// Split a one-line `fun` signature tail into parameter specs, keeping
/// `name = default` groups intact even when the default contains spaces.
///
/// The default runs up to the next `name =` group, a trailing `as <type>`, or the
/// end of the signature. So `fun NotFound msg = "Not Found"` gives **one**
/// parameter with the default `"Not Found"`, while `fun f a = 1 b = 2` still gives
/// two. (Taking a single whitespace-delimited token as the default — the previous
/// behaviour — turned `= "Not Found"` into a parameter literally named `Found"`,
/// which then failed at every call with "Missing argument for parameter".)
fn split_inline_param_specs(rest: &str) -> Vec<String> {
    let bytes = rest.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        let name_end = i;
        let name = rest[name_start..name_end].to_string();

        // Optional `= default`.
        let mut j = name_end;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        // `==` is an operator, not a default marker.
        let is_default = j < bytes.len() && bytes[j] == b'=' && bytes.get(j + 1) != Some(&b'=');
        if is_default {
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let end = scan_default_end(rest, j);
            out.push(format!("{}={}", name, rest[j..end].trim_end()));
            i = end;
        } else {
            out.push(name);
            i = name_end;
        }
    }
    out
}

/// End offset of a default value starting at `start`: the position of the next
/// `name =` group, a trailing `as` clause, or the end of the string. Quoted text
/// is skipped, so spaces and even `=` inside a string literal are preserved.
fn scan_default_end(rest: &str, start: usize) -> usize {
    let bytes = rest.as_bytes();
    let mut i = start;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' {
            quote = Some(c);
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= bytes.len() {
                return i;
            }
            // A trailing `as <type>` belongs to the signature, not the value.
            if rest[j..] == *"as" || rest[j..].starts_with("as ") {
                return i;
            }
            // `ident [`ws`] =` starts the next defaulted parameter.
            let ident_start = j;
            while j < bytes.len() && !bytes[j].is_ascii_whitespace() && bytes[j] != b'=' {
                j += 1;
            }
            if j > ident_start {
                let mut k = j;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b'=' && bytes.get(k + 1) != Some(&b'=') {
                    return i;
                }
            }
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    bytes.len()
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

// ---- Release channels ----
//
// Two channels share one release pipeline. A stable release is tagged `v2.4.0`;
// an experimental one is `v2.4.1-exp.1` and is published as a GitHub *prerelease*,
// so `releases/latest` keeps pointing at stable and an existing install is never
// moved onto an experimental build by accident. Which channel this installation
// is on lives in a file beside the binary rather than being compiled in, because
// the same binary can be installed either way — and a missing file means stable,
// which is what every install predating channels is.

const CHANNEL_STABLE: &str = "stable";
const CHANNEL_EXP: &str = "exp";

/// `~/.local/share/indent`, honouring `INDENT_HOME`.
fn indent_home_dir() -> PathBuf {
    if let Some(home) = env::var_os("INDENT_HOME") {
        return PathBuf::from(home);
    }
    let base = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".local").join("share").join("indent")
}

fn channel_file() -> PathBuf {
    indent_home_dir().join("channel")
}

/// The channel a `channel` file's contents name. Anything else — including no
/// file at all — means stable, which is what every install predating channels is.
fn parse_channel(contents: Option<&str>) -> String {
    match contents {
        Some(s) if s.trim().eq_ignore_ascii_case(CHANNEL_EXP) => CHANNEL_EXP.to_string(),
        _ => CHANNEL_STABLE.to_string(),
    }
}

/// The channel this installation is on.
fn read_channel() -> String {
    parse_channel(fs::read_to_string(channel_file()).ok().as_deref())
}

fn write_channel(channel: &str) -> Result<(), String> {
    let path = channel_file();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    fs::write(&path, format!("{channel}\n"))
        .map_err(|e| format!("cannot record the channel in {}: {e}", path.display()))
}

/// Is this an experimental (prerelease) tag? The convention the workflow relies
/// on: `v2.4.1-exp.1` carries a hyphen after the version, `v2.4.0` does not.
fn is_exp_tag(tag: &str) -> bool {
    tag.trim_start_matches('v').contains('-')
}

/// Pick a release tag out of a GitHub tags response for one channel.
///
/// Pure, so the rule is testable without a network. The response is newest-first.
/// The channel test is the part that matters: choosing purely on "starts with a
/// digit" was enough while there was one channel, and with two it would hand a
/// stable install the newest `-exp` tag.
fn pick_release_tag(body: &str, channel: &str) -> Result<String, String> {
    let json: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| format!("failed to parse tags response: {e}"))?;
    let tags = json
        .as_array()
        .ok_or_else(|| "GitHub tags response was not a list".to_string())?;
    let want_exp = channel == CHANNEL_EXP;
    tags.iter()
        .filter_map(|t| t.get("name").and_then(|v| v.as_str()))
        .find(|name| {
            let n = name.trim_start_matches('v');
            let is_version =
                !n.is_empty() && n.chars().next().map_or(false, |c| c.is_ascii_digit());
            is_version && is_exp_tag(name) == want_exp
        })
        .map(|s| s.to_string())
        .ok_or_else(|| {
            if want_exp {
                "no experimental build has been published yet".to_string()
            } else {
                "no version tags found on GitHub".to_string()
            }
        })
}

/// The digest recorded for `asset` in a `SHA256SUMS.txt` body
/// (`<64 hex digits>  <file name>`, with an optional leading `*`).
fn parse_checksum_for(sums: &str, asset: &str) -> Option<String> {
    for line in sums.lines() {
        let mut parts = line.split_whitespace();
        let (Some(digest), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        // `sha256sum` marks binary mode with a leading `*`, and the release
        // workflow records every name as `./indent-...` because it hashes the
        // output of `find . -maxdepth 1`. Neither belongs in the comparison.
        let name = name.trim_start_matches('*');
        let name = name.strip_prefix("./").unwrap_or(name);
        if name == asset && digest.len() == 64 {
            return Some(digest.to_string());
        }
    }
    None
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Check a downloaded archive against the release's `SHA256SUMS.txt`.
///
/// A mismatch is a hard error: the file exists to catch a truncated or
/// substituted download, and "close enough" is not a thing here. A release that
/// publishes no `SHA256SUMS.txt` only warns — the oldest releases predate it, and
/// refusing to install those would be worse than trusting them.
fn verify_release_checksum(
    client: &reqwest::blocking::Client,
    tag: &str,
    asset: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let url = format!(
        "https://github.com/xytrolabs/indent/releases/download/{tag}/SHA256SUMS.txt"
    );
    let resp = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, "indent")
        .send()
        .map_err(|e| format!("could not fetch SHA256SUMS.txt: {e}"))?;
    if resp.status().as_u16() != 200 {
        eprintln!("  ⚠ {tag} publishes no SHA256SUMS.txt — install continues unverified");
        return Ok(());
    }
    let body = resp
        .text()
        .map_err(|e| format!("failed to read SHA256SUMS.txt: {e}"))?;
    let expected = parse_checksum_for(&body, asset)
        .ok_or_else(|| format!("SHA256SUMS.txt for {tag} has no entry for {asset}"))?;
    let actual = sha256_hex(bytes);
    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(format!(
            "checksum mismatch for {asset}\n  expected {expected}\n  got      {actual}\n  \
             Nothing was installed."
        ));
    }
    println!("  ✓ Checksum verified ({asset})");
    Ok(())
}

/// Report a channel switch's failure the way the rest of the CLI does, and exit
/// non-zero so a script can tell that nothing was changed.
fn exit_on_switch_result(result: Result<(), String>) {
    if let Err(e) = result {
        eprintln!("  ✗ {e}");
        std::process::exit(1);
    }
}

/// `indent --switch-to-exp` / `indent --switch-to-stable`.
///
/// The same machinery as `--update`, with two differences: the release is chosen
/// by channel, and the choice is recorded so `--version` and `--channel` can
/// report it. The previous binary is kept as `<path>.bak` by `install_over_current`.
fn switch_channel(channel: &str) -> Result<(), String> {
    let target = indent_target_triple().ok_or_else(|| {
        "no prebuilt Indent is published for this platform/architecture".to_string()
    })?;
    let client = reqwest::blocking::Client::new();
    println!("⚡ Indent channel: {channel}");

    let query = if channel == CHANNEL_EXP { CHANNEL_EXP } else { "latest" };
    let tag = resolve_release_tag(&client, query)?;
    let plain = tag.trim_start_matches('v').to_string();

    if plain == INDENT_VERSION && read_channel() == channel {
        println!("  ✓ Already on {channel} (Indent {plain}).");
        return Ok(());
    }

    println!("  Downloading Indent {plain} for {target} ...");
    let tmp = std::env::temp_dir().join(format!(
        "indent-switch-{channel}-{}",
        std::process::id()
    ));
    download_release_binary(&client, &tag, &target, &tmp)?;
    let installed = install_over_current(&tmp)?;
    let _ = std::fs::remove_file(&tmp);
    // Record the channel *after* the install, because the file has to describe the
    // binary that is really in place. If it cannot be written, say so loudly rather
    // than returning a bare error: the binary has already been replaced, and a
    // stale record would make `--version` lie and `--update` follow the wrong
    // channel from here on.
    if let Err(e) = write_channel(channel) {
        return Err(format!(
            "Indent {plain} ({channel}) was installed, but the channel record could not \
             be written: {e}\n  Run `indent --switch-to-{channel}` again once the install \
             directory is writable."
        ));
    }

    println!("  ✓ Now running Indent {plain} ({channel})");
    println!("    {}", installed.display());
    if channel == CHANNEL_EXP {
        println!("  Experimental builds are unstable by definition —");
        println!("  `indent --switch-to-stable` returns to the stable channel.");
    }
    Ok(())
}

/// `indent --update`: prefer the prebuilt release binary (fast, no Rust needed);
/// fall back to a source build only if no prebuilt release is available.
fn self_update() {
    println!("⚡ Indent updater");
    match self_update_from_binary() {
        Ok(()) => return,
        Err(e) => {
            eprintln!("  ⚠ {e}");
            eprintln!("  Falling back to a source build (this can take a while)...");
        }
    }
    // The source fallback has to land on the same channel, or `--update` on an
    // experimental install would build the default branch and quietly change
    // channels. The tag is resolved again here because the binary path may have
    // failed before it got as far as resolving one.
    let channel = read_channel();
    let query = if channel == CHANNEL_EXP { CHANNEL_EXP } else { "latest" };
    let tag = resolve_release_tag(&reqwest::blocking::Client::new(), query).ok();
    if channel == CHANNEL_EXP && tag.is_none() {
        eprintln!("  ⚠ No experimental tag could be resolved, and building the default");
        eprintln!("    branch would move this install off the experimental channel.");
        eprintln!("    Run `indent --switch-to-stable` first if that is what you want.");
        return;
    }
    self_update_from_source(tag.as_deref());
}

/// Download the latest release's prebuilt binary and swap it in.
fn self_update_from_binary() -> Result<(), String> {
    let target = indent_target_triple().ok_or_else(|| {
        "no prebuilt Indent is published for this platform/architecture".to_string()
    })?;
    let client = reqwest::blocking::Client::new();
    // Follow the channel this install is on. Resolving "latest" always meant
    // *stable*, so `indent --update` on an experimental build moved the user back
    // onto stable without saying so — the opposite of what `--switch-to-exp`
    // promised and of what the flag docs claimed.
    let channel = read_channel();
    let query = if channel == CHANNEL_EXP { CHANNEL_EXP } else { "latest" };
    println!("  Checking for the latest {channel} release ...");
    let tag = resolve_release_tag(&client, query)?;
    let plain = tag.trim_start_matches('v').to_string();
    if plain == INDENT_VERSION {
        println!("  ✓ Already up to date (Indent {plain}, {channel}).");
        return Ok(());
    }
    println!("  Downloading Indent {tag} ({target}) ...");
    let tmp = std::env::temp_dir().join(format!("indent-update-{}", std::process::id()));
    download_release_binary(&client, &tag, &target, &tmp)?;

    let current = install_over_current(&tmp)?;
    let _ = std::fs::remove_file(&tmp);
    println!("  ✓ Updated to Indent {plain} ({channel})!");
    println!("  (Backup saved to {}.bak)", current.display());
    Ok(())
}

/// Put `source` in place of the running `indent`, keeping one backup.
///
/// Overwriting a running executable in place fails on Linux ("Text file busy"),
/// so the new binary is staged in the *same* directory — a rename within one
/// filesystem is atomic — and then renamed over the target.
fn install_over_current(source: &std::path::Path) -> Result<PathBuf, String> {
    let current = std::env::current_exe()
        .map_err(|e| format!("cannot locate the running indent binary: {e}"))?;
    // `current_exe` gives the resolved path, so on a symlinked install this
    // replaces the target rather than the link. That is what the current layout
    // wants; a layout that pointed a symlink at a versioned directory would need
    // the link rewritten instead.
    let dir = current.parent().unwrap_or(Path::new("."));
    let staged = dir.join(format!(".indent.new.{}", std::process::id()));
    std::fs::copy(source, &staged)
        .map_err(|e| format!("cannot stage the new binary in {}: {e}", dir.display()))?;
    // Only now that the new binary is staged — a `.bak` sitting beside a binary
    // that was never replaced is just confusing.
    let backup = format!("{}.bak", current.display());
    std::fs::copy(&current, &backup).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    }

    match std::fs::rename(&staged, &current) {
        Ok(_) => Ok(current),
        Err(e) => {
            let _ = std::fs::remove_file(&staged);
            Err(format!(
                "cannot replace the binary — check that {} is writable: {e}",
                current.display()
            ))
        }
    }
}

/// Slow fallback: clone the repo and build with cargo.
fn self_update_from_source(tag: Option<&str>) {
    let repo_url = "https://github.com/xytrolabs/indent.git";
    let tmp = std::env::temp_dir().join("indent-update");
    
    println!("⚡ Indent updater — fetching latest from GitHub...");
    
    // Always start from a clean, fresh shallow clone. Re-using a leftover
    // working copy (git pull) fails whenever it has local changes (e.g. a
    // dirty Cargo.lock), so we simply discard it and re-clone each run.
    let _ = std::fs::remove_dir_all(&tmp);
    let tmp_str = tmp.to_str().unwrap_or("/tmp/indent-update");
    let mut clone_args = vec!["clone", "--depth", "1"];
    if let Some(t) = tag {
        // Shallow-cloning the tag is what keeps an experimental install on its
        // channel; without it the fallback built the default branch.
        clone_args.extend_from_slice(&["--branch", t]);
    }
    clone_args.extend_from_slice(&[repo_url, tmp_str]);
    let git_result = Command::new("git").args(&clone_args).output();
    
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
    eprintln!("Indent {} — a language for learning and building", INDENT_VERSION);
    eprintln!();
    eprintln!("Usage:  indent [--debug] [--break N[,M...]] [--no-cache] <file.ind>");
    eprintln!("        indent <command> [options] [args]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  run <file.ind>            Run a program (also: indent <file.ind>)");
    eprintln!("  repl                      Interactive REPL");
    eprintln!("  check <file-or-dir>       Parse/check syntax (.ind files)");
    eprintln!("  test [path]               Run tests");
    eprintln!("  lint <file.ind>           Lint a file");
    eprintln!("  fmt [--check] <file.ind>  Auto-format a file");
    eprintln!("  new <name-or-path>        Scaffold a new project");
    eprintln!("  nest ...                  Project-local environments (venv-like)");
    eprintln!();
    eprintln!("Nest subcommands:");
    eprintln!("  indent nest init [dir]       Create a .nest/ in [dir] (default .)");
    eprintln!("  indent nest install [ver]    Pin an Indent release into this nest (e.g. '2.1.0')");
    eprintln!("  indent nest use [ver]        Alias of install");
    eprintln!("  indent nest version          Show this nest's pinned Indent version");
    eprintln!("  indent nest list             List packages in this nest");
    eprintln!("  indent nest path             Print this nest's path");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --watch <file.ind>        Hot reload: re-run on every save");
    eprintln!("  --safe <file.ind>         Run in a default-deny sandbox (no file/OS/net/Python)");
    eprintln!("  --debug [--break N[,M]]   Debug with breakpoints");
    eprintln!("  --no-cache                Ignore the __incache__ parsed-program cache");
    eprintln!("  --clear-cache             Delete __incache__ directories in this project");
    eprintln!("  --update                  Update to the latest version of your channel");
    eprintln!("  --channel                 Show which release channel this install is on");
    eprintln!("  --switch-to-exp           Switch to the experimental (unstable) channel");
    eprintln!("  --switch-to-stable        Switch back to the stable channel");
    eprintln!();
    eprintln!("  A channel belongs to an INDENT_HOME. Inside an active `nest` — which sets");
    eprintln!("  INDENT_HOME — the channel and the replaced binary are the nest's, not the");
    eprintln!("  global install's, which is what makes `nest install` per-project.");
    eprintln!("  --version, -V             Print version");
    eprintln!("  --help, -h                Show this help");
    eprintln!();
    eprintln!("Packages: air install <pkg>   |   Docs: https://github.com/xytrolabs/indent");
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
    let original_dir = env::current_dir().ok();

    for file in &files {
        let parent = file.parent().unwrap_or(Path::new("."));
        let _ = env::set_current_dir(parent);
        let mut runtime = Runtime::new(parent.to_path_buf());
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

    if let Some(dir) = original_dir {
        let _ = env::set_current_dir(dir);
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

fn file_mtime_ns(path: &std::path::Path) -> Option<u128> {
    std::fs::metadata(path).ok()?.modified().ok()?
        .duration_since(std::time::UNIX_EPOCH).ok()
        .map(|d| d.as_nanos())
}

/// Walk up from `start` looking for a project-local `.nest` directory.
fn find_nest(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut cur = Some(start.to_path_buf());
    while let Some(d) = cur {
        let nest = d.join(".nest");
        if nest.is_dir() {
            return Some(nest);
        }
        cur = d.parent().map(|p| p.to_path_buf());
    }
    None
}

/// Hot reload: run the file, then re-run whenever it changes on disk.
/// Because Indent is a tree-walker there is no compile step, so this is a
/// near-instant restart (edit a script while a window/server is open).
fn watch_and_run(abs: &std::path::Path, debug: bool) {
    let base = abs.parent().unwrap_or(std::path::Path::new("."));
    let mut last: Option<u128> = None;
    loop {
        let m = file_mtime_ns(abs);
        if m != last {
            last = m;
            println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("  indent --watch: {}", abs.display());
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            let mut runtime = Runtime::new(base.to_path_buf());
            match runtime.run_file(abs) {
                Ok(()) => {}
                Err(err) => {
                    if debug && err == DEBUGGER_STOP_MSG {
                        continue;
                    }
                    let src = std::fs::read_to_string(abs).unwrap_or_default();
                    eprintln!("{}", format_error_with_source(&src, &err));
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
}

fn run_new_nest(base: &Path) -> Result<(), String> {
    fs::create_dir_all(base)
        .map_err(|e| format!("Failed to create {}: {e}", base.display()))?;
    let nest = base.join(".nest");
    if nest.exists() {
        return Err(format!("A nest already exists here: {}", nest.display()));
    }
    for sub in ["air-packages", "bin", "lib"] {
        fs::create_dir_all(nest.join(sub))
            .map_err(|e| format!("Failed to create {sub}: {e}"))?;
    }
    let nest_str = nest.to_string_lossy().to_string();
    let name = base
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
        .to_string();

    // Bash activation (POSIX-ish).
    let mut activate = String::from("#!/usr/bin/env bash\n");
    activate.push_str(&format!("# Indent nest \"{}\" - activate: source .nest/activate\n", name));
    activate.push_str("export INDENT_NEST=\"");
    activate.push_str(&nest_str);
    activate.push_str("\"\n");
    activate.push_str("export INDENT_HOME=\"$INDENT_NEST\"\n");
    activate.push_str("export INDENT_PATH=\"$INDENT_NEST/air-packages:$INDENT_NEST/lib${INDENT_PATH:+:$INDENT_PATH}\"\n");
    activate.push_str("export PATH=\"$INDENT_NEST/bin${PATH:+:$PATH}\"\n");
    activate.push_str("echo \"activated Indent nest: $INDENT_NEST\"\n");
    activate.push_str("deactivate() {\n");
    activate.push_str("  unset INDENT_NEST INDENT_HOME\n");
    activate.push_str("  unset -f deactivate\n");
    activate.push_str("  echo \"deactivated Indent nest\"\n");
    activate.push_str("}\n");

    // PowerShell activation.
    let mut ps1 = String::new();
    ps1.push_str(&format!("# Indent nest \"{}\" - activate: . .nest/Activate.ps1\n", name));
    ps1.push_str("$env:INDENT_NEST=\"");
    ps1.push_str(&nest_str);
    ps1.push_str("\"\n");
    ps1.push_str("$env:INDENT_HOME=$env:INDENT_NEST\n");
    ps1.push_str("$env:INDENT_PATH=\"$env:INDENT_NEST/air-packages;$env:INDENT_NEST/lib;$env:INDENT_PATH\"\n");
    ps1.push_str("$env:Path=\"$env:INDENT_NEST/bin;$env:Path\"\n");
    ps1.push_str("Write-Host \"activated Indent nest: $env:INDENT_NEST\"\n");

    fs::write(nest.join("activate"), activate)
        .map_err(|e| format!("Failed to write activate: {e}"))?;
    fs::write(nest.join("Activate.ps1"), ps1)
        .map_err(|e| format!("Failed to write Activate.ps1: {e}"))?;
    fs::write(
        nest.join("deps.txt"),
        "# Indent nest dependencies - one package per line, e.g.:\n# colors\n",
    )
    .map_err(|e| format!("Failed to write deps.txt: {e}"))?;
    fs::write(
        nest.join("README.txt"),
        format!(
            "Indent nest \"{name}\"\n\nA project-local environment for isolated Indent packages\n(like Python's venv). With this nest active, `air install <pkg>`\ninstalls into air-packages/ here instead of your global install, and\nimports resolve from this nest first.\n\nActivate:   source .nest/activate      (PowerShell: . .nest/Activate.ps1)\n"
        ),
    )
    .map_err(|e| format!("Failed to write README: {e}"))?;

    println!("✓ Created Indent nest at {}", nest.display());
    println!("  Activate:   source {}/activate", nest.display());
    println!("  Then:       air install <package>   (installs into this nest)");
    println!("             indent nest install <version>   (pins the interpreter)");
    Ok(())
}

// =============================================================================
// Nest interpreter version pinning — `indent nest install <version>`
//
// A nest can pin its own Indent interpreter version, like a Python venv built
// from a chosen python. We download the *prebuilt* binary that the release CI
// publishes for the given tag (`indent-<tag>-<target>.tar.gz` / `.zip`) and
// drop it into `.nest/bin/`. Because activating a nest prepends `.nest/bin` to
// PATH, `indent` then resolves to the pinned version inside the nest.
// =============================================================================

/// Map the current platform/arch onto the release-asset target triple.
fn indent_target_triple() -> Option<String> {
    let (os, arch) = (env::consts::OS, env::consts::ARCH);
    let t = match (os, arch) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => return None,
    };
    Some(t.to_string())
}

/// Turn a user-supplied version into a release tag ("2.2.0" -> "v2.2.0").
fn normalize_version_tag(v: &str) -> String {
    let t = v.trim();
    if t.starts_with('v') || t.starts_with('V') {
        format!("v{}", t.trim_start_matches(['v', 'V']))
    } else {
        format!("v{t}")
    }
}

/// Resolve "latest", "exp", or an explicit version to a concrete release tag.
fn resolve_release_tag(
    client: &reqwest::blocking::Client,
    version: &str,
) -> Result<String, String> {
    let want_exp = version.eq_ignore_ascii_case(CHANNEL_EXP);
    if version.eq_ignore_ascii_case("latest") || want_exp {
        // Use the git-tags endpoint (newest first) rather than releases/latest,
        // which only works once a formal release has been published. A tag is
        // enough to reach its CI-built assets under releases/download/<tag>/.
        let url = "https://api.github.com/repos/xytrolabs/indent/tags?per_page=50";
        let resp = client
            .get(url)
            .header(reqwest::header::USER_AGENT, "indent-nest")
            .send()
            .map_err(|e| format!("could not reach GitHub to find the latest version: {e}"))?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .map_err(|e| format!("failed to read tags response: {e}"))?;
        if status != 200 {
            return Err(format!(
                "GitHub returned {status} while finding the latest version: {body}"
            ));
        }
        // The channel decides which tags are eligible. Choosing purely on "looks
        // like a version" was enough while there was one channel; with two, it
        // would hand a stable install the newest `-exp` tag.
        pick_release_tag(
            &body,
            if want_exp { CHANNEL_EXP } else { CHANNEL_STABLE },
        )
    } else {
        Ok(normalize_version_tag(version))
    }
}

/// Download the prebuilt `indent` binary for a release `tag`/`target` and write
/// it to `dest` (a file path). Used by both `--update` and `nest install`.
fn download_release_binary(
    client: &reqwest::blocking::Client,
    tag: &str,
    target: &str,
    dest: &std::path::Path,
) -> Result<(), String> {
    let is_windows = target.ends_with("windows-msvc");
    let bin_name = if is_windows { "indent.exe" } else { "indent" };
    let ext = if is_windows { "zip" } else { "tar.gz" };
    let asset = format!("indent-{tag}-{target}.{ext}");
    let url = format!(
        "https://github.com/xytrolabs/indent/releases/download/{tag}/{asset}"
    );

    let resp = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, "indent")
        .send()
        .map_err(|e| format!("download request failed: {e}"))?;
    let status = resp.status().as_u16();
    if status == 404 {
        return Err(format!(
            "no prebuilt Indent {tag} for {target} is published yet"
        ));
    }
    if status != 200 {
        return Err(format!("download failed with HTTP {status}"));
    }
    let bytes = resp
        .bytes()
        .map_err(|e| format!("failed to read downloaded binary: {e}"))?;

    // Before anything is unpacked or installed: the archive has to be the one the
    // release published. On failure the caller never reaches its own
    // `remove_file`, so clean up here rather than leave a bad archive behind.
    if let Err(e) = verify_release_checksum(client, tag, &asset, bytes.as_ref()) {
        let _ = std::fs::remove_file(dest);
        return Err(e);
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }

    let mut found = false;
    if is_windows {
        // .zip — pull out just indent.exe.
        let reader = std::io::Cursor::new(bytes.as_ref().to_vec());
        let mut archive = zip::ZipArchive::new(reader)
            .map_err(|e| format!("failed to read zip archive: {e}"))?;
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| format!("failed to read zip entry: {e}"))?;
            if entry.name().ends_with(bin_name) {
                use std::io::Read;
                let mut buf = Vec::new();
                entry
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("failed to extract {bin_name}: {e}"))?;
                std::fs::write(dest, &buf)
                    .map_err(|e| format!("failed to write {}: {e}", dest.display()))?;
                found = true;
                break;
            }
        }
    } else {
        // .tar.gz — pull out just the `indent` binary.
        use flate2::read::GzDecoder;
        use std::io::Read;
        let gz = GzDecoder::new(bytes.as_ref());
        let mut archive = tar::Archive::new(gz);
        let entries = archive
            .entries()
            .map_err(|e| format!("failed to read tar archive: {e}"))?;
        for entry in entries {
            let mut entry = entry.map_err(|e| format!("failed to read tar entry: {e}"))?;
            let path = entry
                .path()
                .map_err(|e| format!("failed to read tar entry path: {e}"))?
                .into_owned();
            if path.file_name().and_then(|s| s.to_str()) == Some(bin_name) {
                let mut buf = Vec::new();
                entry
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("failed to extract {bin_name}: {e}"))?;
                std::fs::write(dest, &buf)
                    .map_err(|e| format!("failed to write {}: {e}", dest.display()))?;
                found = true;
                break;
            }
        }
    }

    if !found {
        return Err(format!(
            "the {tag} release archive did not contain a {bin_name} binary"
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// Download a release tag's prebuilt Indent binary into `nest/bin` and record
/// the pinned version in `.nest/indent-version`.
fn nest_install_version(nest: &std::path::Path, version: &str) -> Result<(), String> {
    let target = indent_target_triple().ok_or_else(|| {
        "no prebuilt Indent binary is published for this platform/architecture".to_string()
    })?;
    let client = reqwest::blocking::Client::new();

    println!("  Resolving Indent version ...");
    let tag = resolve_release_tag(&client, version)?;
    let plain = tag.trim_start_matches('v').to_string();

    let is_windows = target.ends_with("windows-msvc");
    let bin_name = if is_windows { "indent.exe" } else { "indent" };
    let dest = nest.join("bin").join(bin_name);

    println!("  Downloading Indent {tag} for {target} ...");
    download_release_binary(&client, &tag, &target, &dest)?;

    std::fs::write(nest.join("indent-version"), format!("{plain}\n"))
        .map_err(|e| format!("failed to record pinned version: {e}"))?;

    println!("✓ Installed Indent {plain} into this nest");
    println!("  Binary: {}", dest.display());
    println!("  Pin:    written to {}/indent-version", nest.display());
    println!("  Use:    source {}/activate  (then `indent --version`)", nest.display());
    Ok(())
}

/// Read the version pinned into a nest, or "" if none.
fn nest_pinned_version(nest: &std::path::Path) -> String {
    std::fs::read_to_string(nest.join("indent-version"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    if args.len() < 2 {
        usage();
        std::process::exit(2);
    }

    if args[1] == "--version" || args[1] == "-V" || args[1] == "version" {
        // Once there are two channels, which one you are running is part of the
        // answer to "what version is this".
        println!("indent {} ({})", INDENT_VERSION, read_channel());
        return;
    }

    if args[1] == "--update" || args[1] == "update" {
        self_update();
        return;
    }

    // Channel switching. The choice is recorded beside the binary, so `--version`
    // and `--channel` report it from then on, and `--update` keeps following the
    // channel you are on.
    if args[1] == "--switch-to-exp" || args[1] == "--switch-to-experimental" {
        exit_on_switch_result(switch_channel(CHANNEL_EXP));
        return;
    }

    if args[1] == "--switch-to-stable" {
        exit_on_switch_result(switch_channel(CHANNEL_STABLE));
        return;
    }

    if args[1] == "--channel" {
        let channel = read_channel();
        println!("channel: {channel}   (indent {INDENT_VERSION})");
        if channel == CHANNEL_EXP {
            println!(
                "Experimental builds are unstable by definition; \
                 `indent --switch-to-stable` returns to the stable channel."
            );
        } else {
            println!(
                "`indent --switch-to-exp` opts in to the experimental channel; \
                 `--switch-to-stable` undoes it."
            );
        }
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

    if args[1] == "nest" {
        let sub = args.get(2).map(|s| s.as_str()).unwrap_or("");
        match sub {
            "init" => {
                let target = if args.len() >= 4 {
                    PathBuf::from(&args[3])
                } else {
                    PathBuf::from(".")
                };
                let abs = if target.is_absolute() {
                    target
                } else {
                    env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(target)
                };
                if let Err(err) = run_new_nest(&abs) {
                    eprintln!("Indent nest error: {err}");
                    std::process::exit(1);
                }
            }
            "install" | "use" => {
                let base = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let nest = match find_nest(&base) {
                    Some(n) => n,
                    None => {
                        eprintln!("No .nest found in this directory (or any parent).\n  Create one first with: indent nest init");
                        std::process::exit(1);
                    }
                };
                let version = args.get(3).map(|s| s.as_str()).unwrap_or("latest");
                if let Err(err) = nest_install_version(&nest, version) {
                    eprintln!("Indent nest error: {err}");
                    std::process::exit(1);
                }
            }
            "version" => {
                let base = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                if let Some(nest) = find_nest(&base) {
                    let pinned = nest_pinned_version(&nest);
                    let has_bin = nest.join("bin/indent").is_file()
                        || nest.join("bin/indent.exe").is_file();
                    if pinned.is_empty() && !has_bin {
                        println!("This nest does not pin an Indent version (using your global `indent`).\n  Pin one with: indent nest install <version>");
                    } else {
                        let label = if pinned.is_empty() {
                            "(unknown)".to_string()
                        } else {
                            pinned.clone()
                        };
                        println!("Indent {label} (pinned in {})", nest.display());
                        if !has_bin {
                            println!("  (note: version recorded but no binary present in bin/)");
                        }
                    }
                } else {
                    eprintln!("No .nest found in this directory (or any parent).");
                    std::process::exit(1);
                }
            }
            "list" | "ls" => {
                let base = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                if let Some(nest) = find_nest(&base) {
                    let ap = nest.join("air-packages");
                    match std::fs::read_dir(&ap) {
                        Ok(entries) => {
                            let mut names: Vec<String> = entries
                                .filter_map(|e| e.ok())
                                .map(|e| e.file_name().to_string_lossy().to_string())
                                .filter(|n| n.ends_with(".ind"))
                                .collect();
                            names.sort();
                            if names.is_empty() {
                                println!("(this nest has no packages installed)");
                            }
                            for n in names {
                                println!("{}", n);
                            }
                        }
                        Err(_) => println!("(this nest has no packages installed)"),
                    }
                } else {
                    eprintln!("No .nest found in this directory (or any parent).");
                    std::process::exit(1);
                }
            }
            "path" => {
                let base = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                if let Some(nest) = find_nest(&base) {
                    println!("{}", nest.display());
                } else {
                    eprintln!("No .nest found in this directory (or any parent).");
                    std::process::exit(1);
                }
            }
            _ => {
                eprintln!("Usage: indent nest <init|install|use|version|list|path> [args]");
                eprintln!("  indent nest init [dir]                 create a .nest (venv-like)");
                eprintln!("  indent nest install [version]          pin a specific Indent into this nest");
                eprintln!("  indent nest use [version]              alias of install");
                eprintln!("  indent nest version                    show this nest's pinned Indent");
                eprintln!("  indent nest list|ls                    list packages in this nest");
                eprintln!("  indent nest path                       print this nest's path");
                eprintln!();
                eprintln!("  version is a release tag: 'latest' (default) or e.g. '2.1.0' / 'v2.1.0'");
                std::process::exit(2);
            }
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
    let mut watch = false;
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

        if arg == "--safe" {
            SAFE_MODE.store(true, Ordering::Relaxed);
            i += 1;
            continue;
        }

        if arg == "--watch" {
            watch = true;
            i += 1;
            continue;
        }

        // Ignore `__incache__` for this run (read and write).
        if arg == "--no-cache" {
            AST_CACHE_OFF.store(true, Ordering::Relaxed);
            i += 1;
            continue;
        }

        // Delete every `__incache__` directory below the current directory.
        if arg == "--clear-cache" {
            let mut removed = 0usize;
            clear_ast_cache(Path::new("."), &mut removed);
            println!("Cleared {removed} cached program file(s).");
            std::process::exit(0);
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
        // `cd <script-dir> && indent <script>.ind`. This makes
        // `indent run /abs/path/<script>.ind` work from any terminal directory.
        let _ = env::set_current_dir(base_dir);
        load_project_environment(base_dir);
    }

    if let Some(base_dir) = abs.parent() {
        if let Some(nest) = find_nest(base_dir) {
            let extra = format!(
                "{}:{}",
                nest.join("air-packages").display(),
                nest.join("lib").display()
            );
            let existing = env::var("INDENT_PATH").unwrap_or_default();
            let merged = if existing.is_empty() {
                extra
            } else {
                format!("{extra}:{existing}")
            };
            // SAFETY: single-threaded startup before any interpreter work.
            unsafe { env::set_var("INDENT_PATH", merged); }
        }
    }

    if watch {
        watch_and_run(&abs, debug);
        return;
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
        // A `touch_lines` walk over every function body used to run here. Every
        // one of its arms was `let _ = line`, so it had no effect anyone could
        // observe — it only made the AST look like it was being read.
        Ok(()) => {}
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

    fn cache_file_for(src: &Path) -> PathBuf {
        src.parent()
            .unwrap()
            .join(AST_CACHE_DIR)
            .join(format!(
                "{}.inc",
                src.file_name().unwrap().to_string_lossy()
            ))
    }

    /// `AST_CACHE_OFF` is process-global, so the cache tests must not interleave
    /// (Rust runs tests in parallel by default). Every cache test takes this lock.
    fn cache_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn ast_cache_round_trips_a_program() {
        let _guard = cache_test_lock();
        let dir = unique_dir("indent_test_ast_cache");
        fs::create_dir_all(&dir).expect("create temp dir");
        let src = dir.join("mod_a.ind");
        let source = "fun Double\n    give 2\n";
        fs::write(&src, source).expect("write source");

        // First load is a miss (nothing cached yet).
        assert!(load_cached_program(&src).is_none());

        let program = parse_source(source).expect("parse");
        store_cached_program(&src, source, &program);

        let cache = cache_file_for(&src);
        assert!(cache.exists(), "cache file was not written");

        let loaded = load_cached_program(&src).expect("cache hit");
        assert_eq!(
            format!("{program:?}").len(),
            format!("{loaded:?}").len(),
            "cached program differs from the freshly parsed one"
        );
    }

    #[test]
    fn ast_cache_invalidates_when_the_source_changes() {
        let _guard = cache_test_lock();
        let dir = unique_dir("indent_test_ast_cache_invalidate");
        fs::create_dir_all(&dir).expect("create temp dir");
        let src = dir.join("mod_b.ind");
        let source = "fun One\n    give 1\n";
        fs::write(&src, source).expect("write source");
        let program = parse_source(source).expect("parse");
        store_cached_program(&src, source, &program);
        assert!(load_cached_program(&src).is_some(), "expected a cache hit");

        // Same length, different text — the source hash must catch this even if
        // the mtime granularity is coarse.
        let edited = "fun One\n    give 2\n";
        assert_eq!(edited.len(), source.len());
        fs::write(&src, edited).expect("rewrite source");
        assert!(
            load_cached_program(&src).is_none(),
            "stale cache must be rejected after an edit"
        );
    }

    #[test]
    fn ast_cache_rejects_a_corrupt_or_truncated_file() {
        let _guard = cache_test_lock();
        let dir = unique_dir("indent_test_ast_cache_corrupt");
        fs::create_dir_all(&dir).expect("create temp dir");
        let src = dir.join("mod_c.ind");
        let source = "fun Three\n    give 3\n";
        fs::write(&src, source).expect("write source");
        let program = parse_source(source).expect("parse");
        store_cached_program(&src, source, &program);

        let cache = cache_file_for(&src);
        // Truncate the payload: must be treated as a miss, never a panic.
        let bytes = fs::read(&cache).expect("read cache");
        fs::write(&cache, &bytes[..bytes.len() / 2]).expect("truncate cache");
        assert!(load_cached_program(&src).is_none());

        // Garbage with a valid header size.
        fs::write(&cache, b"not a cache file at all........").expect("garbage");
        assert!(load_cached_program(&src).is_none());

        // A different cache format version must be rejected too.
        let mut bad = Vec::new();
        bad.extend_from_slice(AST_CACHE_MAGIC);
        bad.extend_from_slice(&(AST_CACHE_FORMAT + 999).to_le_bytes());
        bad.extend_from_slice(&0u16.to_le_bytes()); // build length
        bad.extend_from_slice(&[0u8; 32]); // size, mtime, hash, payload length
        assert_eq!(bad.len(), AST_CACHE_HEADER_FIXED);
        fs::write(&cache, &bad).expect("bad version");
        assert!(load_cached_program(&src).is_none());
    }

    #[test]
    fn ast_cache_is_never_shared_between_builds() {
        // bincode is not self-describing, so a cache written by a different
        // Indent build could deserialize into a WRONG program instead of failing.
        // Everything here is valid — source size, mtime, hash and payload are the
        // real ones — so the build string is the only thing that can reject it.
        let _guard = cache_test_lock();
        let dir = unique_dir("indent_test_ast_cache_build");
        fs::create_dir_all(&dir).expect("create temp dir");
        let src = dir.join("mod_e.ind");
        let source = "fun Five\n    give 5\n";
        fs::write(&src, source).expect("write source");
        let program = parse_source(source).expect("parse");
        store_cached_program(&src, source, &program);
        assert!(load_cached_program(&src).is_some(), "expected a cache hit");

        let cache = cache_file_for(&src);
        let mut bytes = fs::read(&cache).expect("read cache");
        let build_len = AST_CACHE_BUILD.len();
        // Same length, different bytes: no other header field moves.
        let foreign = "9".repeat(build_len);
        assert!(foreign.as_bytes() != AST_CACHE_BUILD.as_bytes());
        assert_eq!(&bytes[14..14 + build_len], AST_CACHE_BUILD.as_bytes());
        bytes[14..14 + build_len].copy_from_slice(foreign.as_bytes());
        fs::write(&cache, &bytes).expect("write foreign cache");
        assert!(
            load_cached_program(&src).is_none(),
            "a cache written by another build must be a miss, not a misread"
        );
    }

    #[test]
    fn ast_cache_handles_a_missing_source() {
        let _guard = cache_test_lock();
        // A cache must never be used when the source file itself is gone.
        let dir = unique_dir("indent_test_ast_cache_missing");
        fs::create_dir_all(&dir).expect("create temp dir");
        let src = dir.join("mod_d.ind");
        let source = "fun Four\n    give 4\n";
        fs::write(&src, source).expect("write source");
        let program = parse_source(source).expect("parse");
        store_cached_program(&src, source, &program);
        assert!(load_cached_program(&src).is_some());

        fs::remove_file(&src).expect("remove source");
        assert!(
            load_cached_program(&src).is_none(),
            "must not serve a cache for a deleted source"
        );
    }

    #[test]
    fn ast_cache_can_be_disabled() {
        let _guard = cache_test_lock();
        let dir = unique_dir("indent_test_ast_cache_disabled");
        fs::create_dir_all(&dir).expect("create temp dir");
        let src = dir.join("mod_e.ind");
        let source = "fun Five\n    give 5\n";
        fs::write(&src, source).expect("write source");
        let program = parse_source(source).expect("parse");

        AST_CACHE_OFF.store(true, Ordering::Relaxed);
        store_cached_program(&src, source, &program);
        assert!(!cache_file_for(&src).exists(), "disabled cache wrote a file");
        assert!(load_cached_program(&src).is_none());
        AST_CACHE_OFF.store(false, Ordering::Relaxed);
    }

    #[test]
    fn cached_program_executes_identically() {
        let _guard = cache_test_lock();
        // End-to-end: run a program that imports a module, then run it again
        // from a fresh runtime — the second run loads the module from its cache.
        // The program *calls* the cached function, so a cache that round-trips
        // incorrectly would fail here rather than silently "pass".
        let dir = unique_dir("indent_test_ast_cache_exec");
        fs::create_dir_all(&dir).expect("create temp dir");
        let helper = dir.join("helper_mod.ind");
        fs::write(&helper, "fun Triple\n    give 3\n").expect("write helper");
        let main = dir.join("main.ind");
        fs::write(
            &main,
            "get Triple from helper_mod\nvar t dynamic = Triple()\n",
        )
        .expect("write main");

        let mut rt = Runtime::new(dir.clone());
        rt.run_file(&main).expect("first run");
        assert!(
            cache_file_for(&helper).exists(),
            "helper module cache missing"
        );
        assert!(
            load_cached_program(&helper).is_some(),
            "helper module cache is not loadable"
        );

        // Fresh runtime, warm cache.
        let mut rt2 = Runtime::new(dir.clone());
        let result = rt2.run_file(&main);
        assert!(result.is_ok(), "cached run failed: {:?}", result.err());
        assert!(
            rt2.callables.contains_key("Triple"),
            "function from a cached module was not imported"
        );
    }

    /// Perf guardrail: `Value` is moved constantly — every variable read, every
    /// argument, every list element and every map entry. It was 128 bytes until
    /// the `Object` maps were boxed, then 56 until the string payload became an
    /// `Arc` (which dropped the largest variant from 24 bytes to 8 and the whole
    /// enum to 32); anything that pushes it back up is a silent tax on the whole
    /// interpreter, so fail loudly instead.
    ///
    /// It is 24 now: `Object`'s field array became a shared `Arc<Box<[Value]>>`, so
    /// the widest payload is 16 bytes and the discriminant fits in the niche
    /// `String` and `Arc` already leave. The bound is 32 rather than 24 so the next
    /// change that needs a byte or two is not blocked — but not 40, because going
    /// back up from 24 is a tax on every read, argument and element, and that is
    /// exactly what the last three builds of this series spent their time removing.
    #[test]
    fn value_size_guardrail() {
        let value = std::mem::size_of::<Value>();
        println!(
            "SIZEOF Value={} Stmt={} Expr={} Callable={} FunctionDef={} Frame={}",
            value,
            std::mem::size_of::<Stmt>(),
            std::mem::size_of::<Expr>(),
            std::mem::size_of::<Callable>(),
            std::mem::size_of::<FunctionDef>(),
            std::mem::size_of::<Frame>(),
        );
        assert!(
            value <= 32,
            "Value grew to {value} bytes (was 24). Box or share large variants — \
             every variable read, argument and list element moves this."
        );

        // `Stmt` is the other width every statement in every program pays for, and
        // it was 152 bytes: `AssignSlice` (three optional bound expressions plus a
        // `ValueSource`) was 152 on its own and `DefClass` 112, while the shapes
        // that actually dominate — `Say`, `BareExpr` — are 32. Boxing just those
        // two takes it to 112, and allocates only for a slice assignment or a
        // class definition. `ArgItem` was 152 for the same reason (it embedded a
        // whole `Stmt`), so *every argument of every call* cost 152 bytes of AST;
        // boxing that payload takes it to 48.
        let stmt = std::mem::size_of::<Stmt>();
        let arg = std::mem::size_of::<ArgItem>();
        assert!(
            stmt <= 120,
            "Stmt grew to {stmt} bytes (was 112). One variant wider than the common \
             shapes makes every statement in every program that wide — box its payload."
        );
        assert!(
            arg <= 56,
            "ArgItem grew to {arg} bytes (was 48). It sits in a Vec, once per \
             argument of every call, so this is paid per argument."
        );
    }

    /// The rule that decides which release a channel follows. The interesting
    /// case is the middle one: with an experimental tag newest, a stable install
    /// must skip it — picking on "looks like a version" alone would not.
    #[test]
    fn release_tag_picking_respects_the_channel() {
        let tags = r#"[
            {"name": "v2.4.1-exp.2"},
            {"name": "v2.4.1-exp.1"},
            {"name": "v2.4.0"},
            {"name": "v2.3.0"}
        ]"#;
        assert_eq!(pick_release_tag(tags, "stable").unwrap(), "v2.4.0");
        assert_eq!(pick_release_tag(tags, "exp").unwrap(), "v2.4.1-exp.2");

        // No experimental tag published yet is an error for exp, not a
        // silent fallback onto stable.
        let stable_only = r#"[{"name": "v2.4.0"}]"#;
        assert_eq!(pick_release_tag(stable_only, "stable").unwrap(), "v2.4.0");
        assert!(pick_release_tag(stable_only, "exp").is_err());

        // A non-version tag is never a candidate, on either channel.
        let junk = r#"[{"name": "nightly"}, {"name": "v2.4.0"}]"#;
        assert_eq!(pick_release_tag(junk, "stable").unwrap(), "v2.4.0");
    }

    #[test]
    fn experimental_tags_are_recognised() {
        assert!(is_exp_tag("v2.4.1-exp.1"));
        assert!(is_exp_tag("2.4.1-rc.1"));
        assert!(!is_exp_tag("v2.4.0"));
        assert!(!is_exp_tag("v2.4.10"));
    }

    #[test]
    fn channel_file_contents_are_parsed_leniently() {
        assert_eq!(parse_channel(Some("exp\n")), "exp");
        assert_eq!(parse_channel(Some("  EXP  ")), "exp");
        assert_eq!(parse_channel(Some("stable")), "stable");
        // No file, or a file we do not understand, is stable rather than exp:
        // the failure mode has to be "did not opt in".
        assert_eq!(parse_channel(None), "stable");
        assert_eq!(parse_channel(Some("")), "stable");
        assert_eq!(parse_channel(Some("nightly\n")), "stable");
    }

    #[test]
    fn sha256sums_lines_are_read() {
        let sums = "\
             aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  other.tar.gz\n\
             bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb *indent-v2.4.0-x86_64-unknown-linux-gnu.tar.gz\n";
        assert_eq!(
            parse_checksum_for(sums, "indent-v2.4.0-x86_64-unknown-linux-gnu.tar.gz").as_deref(),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
        // A name that is not listed, and a digest of the wrong length, are both
        // "no answer" rather than a wrong answer.
        assert!(parse_checksum_for(sums, "indent-v2.4.0-aarch64-apple-darwin.tar.gz").is_none());
        assert!(parse_checksum_for("deadbeef  x.tar.gz", "x.tar.gz").is_none());

        // Published sums files record names as `./indent-...`, because the release
        // workflow hashes `find . -maxdepth 1`. Real releases have failed on this,
        // so it gets its own assertion.
        let published = "\
             cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc  ./indent-v2.3.0-x86_64-unknown-linux-gnu.tar.gz\n";
        assert_eq!(
            parse_checksum_for(published, "indent-v2.3.0-x86_64-unknown-linux-gnu.tar.gz")
                .as_deref(),
            Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc")
        );
    }

    #[test]
    fn sha256_hex_matches_a_known_vector() {
        // Empty input is the one hash everyone can check by hand.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
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

        assert!(matches!(ctx.rt.vars.get("converted"), Some(Value::StrShared(s)) if s == "4"));
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
    fn imported_module_functions_can_use_their_own_imports() {
        // Regression: a function exported by one module must be able to call a
        // module that *it* imported with a bare `get`, even when invoked from a
        // file that never imports that inner module.
        //
        // A bare `get mod` binds the module in `rt.modules` (used by call
        // dispatch) and as a `Value::Module` variable (used by member access),
        // but a ModuleInstance only snapshots variables. So `lib.Go()` running
        // in main's runtime lost the `helper` module-table entry and fell
        // through to builtin method mapping, reporting
        // "Unsupported method 'Hello' for receiver 'helper'".
        let dir = unique_dir("indent_test_transitive_import");
        fs::create_dir_all(&dir).expect("create temp dir");

        fs::write(dir.join("helper.ind"), "fun Hello\n    give 42\n").expect("write helper");
        fs::write(
            dir.join("lib.ind"),
            "get helper\n\nfun Go\n    var v int = helper.Hello()\n    give v\n",
        )
        .expect("write lib");
        fs::write(
            dir.join("main.ind"),
            "get lib\n\nvar got int = lib.Go()\n",
        )
        .expect("write main");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&dir.join("main.ind"));
        assert!(result.is_ok(), "runtime failed: {:?}", result.err());
        assert!(matches!(rt.vars.get("got"), Some(Value::Int(42))));
    }

    #[test]
    fn module_called_as_object_reports_missing_function() {
        // A genuine typo must not be reported as an "Unsupported method".
        let dir = unique_dir("indent_test_module_typo");
        fs::create_dir_all(&dir).expect("create temp dir");

        fs::write(dir.join("helper.ind"), "fun Hello\n    give 42\n").expect("write helper");
        fs::write(
            dir.join("lib.ind"),
            "get helper\n\nfun Go\n    helper.Nonexistent()\n",
        )
        .expect("write lib");
        fs::write(dir.join("main.ind"), "get lib\n\nlib.Go()\n").expect("write main");

        let mut rt = Runtime::new(dir.clone());
        let err = rt
            .run_file(&dir.join("main.ind"))
            .expect_err("expected a dispatch error");
        assert!(
            err.contains("has no function"),
            "expected a missing-function error, got: {err}"
        );
    }

    #[test]
    fn circular_import_reports_clean_error() {
        // Regression: a module importing itself (directly or through another
        // module) used to recurse until the process aborted with a stack
        // overflow. It must now surface a normal, catchable import error.
        let dir = unique_dir("indent_test_circular_import");
        fs::create_dir_all(&dir).expect("create temp dir");

        fs::write(
            dir.join("ping_mod.ind"),
            "get Pong from pong_mod\n\nfun Ping\n    give Pong()\n",
        )
        .expect("write ping_mod");
        fs::write(
            dir.join("pong_mod.ind"),
            "get Ping from ping_mod\n\nfun Pong\n    give Ping()\n",
        )
        .expect("write pong_mod");
        fs::write(dir.join("main.ind"), "get Ping from ping_mod\n").expect("write main");

        let mut rt = Runtime::new(dir.clone());
        let err = rt
            .run_file(&dir.join("main.ind"))
            .expect_err("expected an import error");
        assert!(
            err.contains("Circular import"),
            "expected a circular-import error, got: {err}"
        );
    }

    #[test]
    fn self_import_reports_clean_error() {
        let dir = unique_dir("indent_test_self_import");
        fs::create_dir_all(&dir).expect("create temp dir");

        fs::write(
            dir.join("loop.ind"),
            "get Hello from loop\n\nfun Hello\n    give 1\n",
        )
        .expect("write loop");
        fs::write(dir.join("main.ind"), "get Hello from loop\n").expect("write main");

        let mut rt = Runtime::new(dir.clone());
        let err = rt
            .run_file(&dir.join("main.ind"))
            .expect_err("expected an import error");
        assert!(
            err.contains("Circular import"),
            "expected a circular-import error, got: {err}"
        );
    }

    #[test]
    fn launch_resolves_relative_to_the_module_directory() {
        // A package must be able to `launch` a sibling file regardless of the
        // process working directory.
        let dir = unique_dir("indent_test_launch_sibling");
        let pkg = dir.join("pkg");
        fs::create_dir_all(&pkg).expect("create pkg dir");

        fs::write(
            pkg.join("part_two.ind"),
            "fun Extra\n    give 41\n",
        )
        .expect("write part_two");
        fs::write(
            pkg.join("entry.ind"),
            "launch \"part_two.ind\"\n\nfun Total\n    give Extra() + 1\n",
        )
        .expect("write entry");
        fs::write(dir.join("main.ind"), "get Total from pkg.entry\nsay Total()\n")
            .expect("write main");

        let mut rt = Runtime::new(dir.clone());
        let result = rt.run_file(&dir.join("main.ind"));
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
        assert!(matches!(rt.vars.get("title"), Some(Value::StrShared(s)) if s == "Indent"));
        assert!(matches!(rt.vars.get("second"), Some(Value::Int(20))));
        assert!(matches!(rt.vars.get("firstChar"), Some(Value::StrShared(s)) if s == "I"));
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
        assert!(matches!(rt.vars.get("accent"), Some(Value::StrShared(s)) if s == "#ff4d4d"));
        assert!(matches!(rt.vars.get("accentHex"), Some(Value::StrShared(s)) if s == "#22c55e"));
        assert!(matches!(rt.vars.get("accentBare"), Some(Value::StrShared(s)) if s == "#3b82f6"));
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
        assert!(matches!(rt.vars.get("state"), Some(Value::StrShared(s)) if s.contains("caught: Line") && s.contains("lastly")));
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
        assert!(matches!(rt.vars.get("status"), Some(Value::StrShared(s)) if s == "ok|otherwise|lastly"));
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
        assert!(matches!(rt.vars.get("safe"), Some(Value::StrShared(s)) if s == "quit-or"));
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

    #[test]
    fn color_names_parse_case_insensitively() {
        // The table lookup replaced a `to_ascii_lowercase()` + `String` copy,
        // so the case-insensitivity it relied on has to be an explicit
        // property of the replacement.
        for name in ["red", "RED", "Red", "rEd"] {
            assert_eq!(color_to_rgb(name), Some((0xff, 0x4d, 0x4d)), "{name}");
        }
        assert_eq!(color_to_rgb(" gray "), Some((0x9c, 0xa3, 0xaf)));
        assert_eq!(color_to_rgb("Grey"), color_to_rgb("grey"));
        assert_eq!(color_to_rgb("zinc"), Some((0x71, 0x71, 0x7a)));
        assert_eq!(color_to_rgb("notacolor"), None);
    }

    #[test]
    fn color_hex_forms_parse() {
        // #RGB / #RGBA double each digit; #RRGGBB / #RRGGBBAA read pairs and
        // ignore the alpha byte.
        assert_eq!(color_to_rgb("#f00"), Some((0xff, 0x00, 0x00)));
        assert_eq!(color_to_rgb("#f00f"), Some((0xff, 0x00, 0x00)));
        assert_eq!(color_to_rgb("#ff0000"), Some((0xff, 0x00, 0x00)));
        assert_eq!(color_to_rgb("#ff0000ff"), Some((0xff, 0x00, 0x00)));
        assert_eq!(color_to_rgb("#FF0000"), Some((0xff, 0x00, 0x00)));
        assert_eq!(color_to_rgb("#3b82f6"), Some((0x3b, 0x82, 0xf6)));
        // Odd lengths and non-hex digits are rejected, and a short string must
        // not panic on the doubling path.
        assert_eq!(color_to_rgb("#ff"), None);
        assert_eq!(color_to_rgb("#fffff"), None);
        assert_eq!(color_to_rgb("#gggggg"), None);
        assert_eq!(color_to_rgb(""), None);
        assert_eq!(color_to_rgb("#"), None);
    }

    #[test]
    fn gradient_and_rainbow_render_the_same_bytes() {
        // One escape per character, endpoints included, and the single-char
        // and empty cases stay special.
        let g = gradient_text("ab", (0, 0, 0), (255, 255, 255), false);
        assert_eq!(g, "\x1b[38;2;0;0;0ma\x1b[0m\x1b[38;2;255;255;255mb\x1b[0m");
        assert_eq!(
            gradient_text("x", (1, 2, 3), (9, 9, 9), true),
            "\x1b[48;2;1;2;3mx\x1b[0m"
        );
        assert_eq!(gradient_text("", (0, 0, 0), (1, 1, 1), false), "");

        let palette = rainbow_palette();
        let m = multicolor_text("abcd", &palette, false);
        let expected: String = "abcd"
            .chars()
            .zip(palette.iter())
            .map(|(c, (r, g, b))| format!("\x1b[38;2;{r};{g};{b}m{c}\x1b[0m"))
            .collect();
        assert_eq!(m, expected);
        assert_eq!(multicolor_text("", &palette, false), "");
        assert_eq!(multicolor_text("abc", &[], false), "abc");
        // Non-ASCII characters are one escape each, not one per byte.
        assert_eq!(
            multicolor_text("é🚀", &palette[..1], false),
            "\x1b[38;2;255;77;77mé\x1b[0m\x1b[38;2;255;77;77m🚀\x1b[0m"
        );
    }
}
