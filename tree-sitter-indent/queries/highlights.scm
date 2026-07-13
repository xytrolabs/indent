; Tree-sitter queries for Aether — syntax highlighting
; These map tree-sitter capture names to TextMate-style scopes
; for use in CodeMirror 6, Neovim, Helix, etc.

;; ── Comments ──────────────────────────────────────────────────────

(comment) @comment

;; ── Keywords ──────────────────────────────────────────────────────

[
  "if" "otherwise" "or" "match" "case" "repeat" "until" "while"
  "for" "in" "do" "catch" "lastly" "stop" "next" "reset"
  "break" "continue" "restart" "give" "flag" "empty"
  "var" "fun" "say" "get" "from" "as" "is"
  "assert" "assert_eq" "and" "not"
] @keyword

;; ── Booleans ──────────────────────────────────────────────────────

[
  "true" "false" "yes" "no" "empty"
] @constant.builtin.boolean

;; ── Types ─────────────────────────────────────────────────────────

[
  "string" "int" "float" "boolean" "bool" "dynamic" "empty"
  "list" "dict" "any"
] @type.builtin

;; ── Strings ───────────────────────────────────────────────────────

(string) @string
(escape_sequence) @constant.character.escape

;; ── Numbers ───────────────────────────────────────────────────────

(number) @number

;; ── Color literals ────────────────────────────────────────────────

(color_literal) @constant.other.color

;; ── Function definitions ─────────────────────────────────────────

(function_definition name: (identifier) @function)

;; ── Function calls ────────────────────────────────────────────────

(call_expression callee: (identifier) @function.call)

;; ── Variables ────────────────────────────────────────────────────

(variable_declaration name: (identifier) @variable)

;; ── Parameters ────────────────────────────────────────────────────

(parameter_list (identifier) @variable.parameter)

;; ── Operators ─────────────────────────────────────────────────────

[
  "=" "==" "!=" "<" ">" "<=" ">=" "+" "-" "*" "/" "%" "^"
  "->" ":"
] @operator

;; ── Punctuation ───────────────────────────────────────────────────

[ "(" ")" "[" "]" "{" "}" "," "." ] @punctuation.delimiter

;; ── Imports ───────────────────────────────────────────────────────

(import_statement module: (identifier) @namespace)
(import_statement source: (identifier) @namespace)
(import_statement alias: (identifier) @namespace)

;; ── Classes ───────────────────────────────────────────────────────

(class_definition name: (identifier) @type)
