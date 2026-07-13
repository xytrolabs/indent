//! indent-core — Shared AST types, spans, and language primitives
//!
//! This crate is the single source of truth for Indent's data structures.
//! It is imported by:
//!   - `indent-cli`  (the compiler/runtime binary)
//!   - `indent-lsp`  (the Language Server Protocol server)
//!   - `indent-forge` (the Tauri IDE)
//!
//! Architecture note: The Indent compiler currently runs top-to-bottom
//! rather than using incremental/query-based compilation (e.g. salsa).
//! The AST types here are designed to support both current batch
//! compilation and future incremental migration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

// ═══════════════════════════════════════════════════════════════════
// Source positions
// ═══════════════════════════════════════════════════════════════════

/// 0-indexed position in a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,   // 0-indexed
    pub column: usize, // 0-indexed byte offset within line
}

impl Position {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    /// Convert to LSP Position (0-indexed, as LSP uses)
    pub fn to_lsp(&self) -> lsp_types::Position {
        lsp_types::Position::new(self.line as u32, self.column as u32)
    }
}

/// Span covering a range in source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

impl Span {
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn to_lsp_range(&self) -> lsp_types::Range {
        lsp_types::Range::new(self.start.to_lsp(), self.end.to_lsp())
    }

    /// Merge two spans into one covering both.
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: if self.start < other.start {
                self.start
            } else {
                other.start
            },
            end: if self.end > other.end {
                self.end
            } else {
                other.end
            },
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Source line
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLine {
    pub line_no: usize,
    pub indent: usize,
    pub text: String,
}

// ═══════════════════════════════════════════════════════════════════
// AST — Statements
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    Say {
        span: Span,
        expr: String,
    },
    DefVar {
        span: Span,
        name: String,
        ty: String,
        value: ValueSource,
    },
    Assign {
        span: Span,
        name: String,
        value: ValueSource,
    },
    AssignIndex {
        span: Span,
        name: String,
        index_expr: String,
        value: ValueSource,
    },
    AssignSlice {
        span: Span,
        name: String,
        start_expr: Option<String>,
        end_expr: Option<String>,
        step_expr: Option<String>,
        value: ValueSource,
    },
    DefFun {
        span: Span,
        name: String,
        params: Vec<FunctionParam>,
        return_type: Option<String>,
        body: Vec<Stmt>,
    },
    Give {
        span: Span,
        expr: String,
    },
    IfChain {
        span: Span,
        branches: Vec<(Option<String>, Vec<Stmt>)>,
    },
    Match {
        span: Span,
        subject_expr: String,
        branches: Vec<(String, Vec<Stmt>)>,
        otherwise_body: Option<Vec<Stmt>>,
    },
    DoChain {
        span: Span,
        do_body: Vec<Stmt>,
        catches: Vec<(Option<String>, Vec<Stmt>)>,
        otherwise_body: Option<Vec<Stmt>>,
        lastly_body: Option<Vec<Stmt>>,
    },
    Repeat {
        span: Span,
        mode: RepeatMode,
        body: Vec<Stmt>,
    },
    Stop {
        span: Span,
    },
    Next {
        span: Span,
    },
    Reset {
        span: Span,
    },
    Import {
        span: Span,
        module_name: String,
        symbol_name: Option<String>,
        alias: Option<String>,
    },
    Call {
        span: Span,
        callee: String,
        args: Vec<ArgItem>,
    },
    BareExpr {
        span: Span,
        expr: String,
    },
    MakeType {
        span: Span,
        target_type: String,
        name: String,
    },
    DefClass {
        span: Span,
        name: String,
        fields: Vec<ClassField>,
        methods: Vec<Stmt>,
    },
    Flag {
        span: Span,
        expr: String,
    },
}

impl Stmt {
    /// Get the span of this statement.
    pub fn span(&self) -> Span {
        match self {
            Stmt::Say { span, .. }
            | Stmt::DefVar { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::AssignIndex { span, .. }
            | Stmt::AssignSlice { span, .. }
            | Stmt::DefFun { span, .. }
            | Stmt::Give { span, .. }
            | Stmt::IfChain { span, .. }
            | Stmt::Match { span, .. }
            | Stmt::DoChain { span, .. }
            | Stmt::Repeat { span, .. }
            | Stmt::Stop { span }
            | Stmt::Next { span }
            | Stmt::Reset { span }
            | Stmt::Import { span, .. }
            | Stmt::Call { span, .. }
            | Stmt::BareExpr { span, .. }
            | Stmt::MakeType { span, .. }
            | Stmt::DefClass { span, .. }
            | Stmt::Flag { span, .. } => *span,
        }
    }

    /// Human-readable description of this statement kind.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Stmt::Say { .. } => "say",
            Stmt::DefVar { .. } => "variable definition",
            Stmt::Assign { .. } => "assignment",
            Stmt::AssignIndex { .. } => "index assignment",
            Stmt::AssignSlice { .. } => "slice assignment",
            Stmt::DefFun { .. } => "function definition",
            Stmt::Give { .. } => "return",
            Stmt::IfChain { .. } => "if-chain",
            Stmt::Match { .. } => "match",
            Stmt::DoChain { .. } => "do-catch",
            Stmt::Repeat { .. } => "loop",
            Stmt::Stop { .. } => "stop",
            Stmt::Next { .. } => "next",
            Stmt::Reset { .. } => "reset",
            Stmt::Import { .. } => "import",
            Stmt::Call { .. } => "function call",
            Stmt::BareExpr { .. } => "expression",
            Stmt::MakeType { .. } => "type construction",
            Stmt::DefClass { .. } => "class definition",
            Stmt::Flag { .. } => "flag",
        }
    }

    /// If this statement defines a name, return it.
    pub fn defined_name(&self) -> Option<&str> {
        match self {
            Stmt::DefVar { name, .. } => Some(name),
            Stmt::DefFun { name, .. } => Some(name),
            Stmt::DefClass { name, .. } => Some(name),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Repeat modes
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepeatMode {
    Infinite,
    Count(String),
    ForEach(String),
    ForIn {
        item_name: String,
        iterable_expr: String,
    },
    While(String),
    Until(String),
}

// ═══════════════════════════════════════════════════════════════════
// Value sources
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValueSource {
    Expr(String),
    Call {
        callee: String,
        args: Vec<ArgItem>,
    },
}

// ═══════════════════════════════════════════════════════════════════
// Function params
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionParam {
    pub name: String,
    pub ty: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════
// Argument items (for calls)
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArgItem {
    Positional(String),
    Named { name: String, expr: String },
}

// ═══════════════════════════════════════════════════════════════════
// Class definitions
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassField {
    pub name: String,
    pub ty: String,
}

// ═══════════════════════════════════════════════════════════════════
// Runtime values (shared for LSP hover/completion)
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    List(Vec<RuntimeValue>),
    Dict(HashMap<String, RuntimeValue>),
    Object {
        class_name: String,
        fields: HashMap<String, RuntimeValue>,
    },
    Func(String),
    Module(String),
    Empty,
}

impl fmt::Display for RuntimeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeValue::Int(v) => write!(f, "{v}"),
            RuntimeValue::Float(v) => write!(f, "{v}"),
            RuntimeValue::Bool(v) => write!(f, "{}", if *v { "true" } else { "false" }),
            RuntimeValue::Str(v) => write!(f, "\"{v}\""),
            RuntimeValue::List(items) => {
                let joined = items
                    .iter()
                    .map(|x| format!("{x}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "[{joined}]")
            }
            RuntimeValue::Dict(_) => write!(f, "{{...}}"),
            RuntimeValue::Object { class_name, .. } => write!(f, "<{class_name}>"),
            RuntimeValue::Func(name) => write!(f, "<function {name}>"),
            RuntimeValue::Module(name) => write!(f, "<module {name}>"),
            RuntimeValue::Empty => write!(f, "empty"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Built-in type information for IDE tooling
// ═══════════════════════════════════════════════════════════════════

/// Known built-in functions with their signatures.
pub fn builtin_functions() -> Vec<(&'static str, &'static str, &'static str)> {
    // (name, params, return_type)
    vec![
        ("len", "value: any", "int"),
        ("typeOf", "value: any", "string"),
        ("ask", "prompt: string", "dynamic"),
        ("say", "message: any", "empty"),
        ("append", "list: list, item: any", "list"),
        ("join", "list: list, separator: string", "string"),
        ("split", "text: string, delimiter: string", "list"),
        ("replace", "text: string, old: string, new: string", "string"),
        ("keys", "dict: dict", "list"),
        ("values", "dict: dict", "list"),
        ("items", "dict: dict", "list"),
        ("range", "start: int, end: int", "list"),
        ("parse", "text: string", "dynamic"),
        ("format", "value: any, spec: string", "string"),
        ("now", "", "string"),
        ("sleep", "ms: int", "empty"),
        ("read", "path: string", "string"),
        ("write", "path: string, content: string", "empty"),
        ("exists", "path: string", "boolean"),
        ("remove", "path: string", "empty"),
        ("copy", "src: string, dest: string", "empty"),
        ("sort", "list: list", "list"),
        ("map", "list: list, func: string", "list"),
        ("filter", "list: list, func: string", "list"),
        ("reduce", "list: list, func: string, init: any", "any"),
        ("trim", "text: string", "string"),
        ("upper", "text: string", "string"),
        ("lower", "text: string", "string"),
        ("assert_eq", "left: any, right: any, message: string", "empty"),
        ("assert", "condition: boolean, message: string", "empty"),
    ]
}

/// Indent keywords.
pub const KEYWORDS: &[&str] = &[
    "if", "otherwise", "or", "match", "case", "repeat", "until", "while",
    "for", "in", "do", "catch", "lastly", "stop", "next", "reset",
    "break", "continue", "restart", "give", "flag", "empty",
    "var", "fun", "say", "get", "from", "as", "is",
    "assert", "assert_eq", "and", "not", "true", "false", "yes", "no",
];

/// Indent built-in types.
pub const BUILTIN_TYPES: &[&str] = &[
    "string", "int", "float", "boolean", "bool", "dynamic", "empty",
    "list", "dict", "any",
];
