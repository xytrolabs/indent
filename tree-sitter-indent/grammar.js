/// Tree-sitter grammar for the Indent programming language.
///
/// Indent is a clean, indentation-sensitive, expression-oriented language.
/// Key syntax features:
///   - Comments: #! (single-line), #!* ... #!* (block)
///   - Functions: fun name params \n indented_body
///   - Variables: var name type = value
///   - Flow: if/otherwise, match/case, repeat/until, do/catch
///   - Imports: get Module, get Symbol from Module
///   - Hex colors: #rgb, #rrggbb, #rrggbbaa
///
/// This grammar supports both the parser and queries for:
///   - Syntax highlighting
///   - Code folding
///   - Indentation
///   - Symbol extraction (for LSP)

module.exports = grammar({
  name: "indent",

  extras: ($) => [$.comment, /[\s\n]/],

  // Indentation-sensitive: we track indent levels externally
  // via the external scanner. The grammar defines the structure;
  // the scanner handles INDENT/DEDENT tokens.

  externals: ($) => [
    $.indent,
    $.dedent,
    $.newline,
  ],

  precedences: ($) => [
    [$.call_expression, $.member_expression],
  ],

  rules: {
    source_file: ($) =>
      repeat(choice($.statement, $.blank_line)),

    blank_line: ($) => seq($.newline),

    // ═══════════════════════════════════════════════════════════
    // Statements
    // ═══════════════════════════════════════════════════════════

    statement: ($) =>
      choice(
        $.say_statement,
        $.variable_declaration,
        $.assignment,
        $.function_definition,
        $.return_statement,
        $.if_statement,
        $.match_statement,
        $.do_catch_statement,
        $.repeat_statement,
        $.stop_statement,
        $.next_statement,
        $.reset_statement,
        $.import_statement,
        $.expression_statement,
        $.make_type_statement,
        $.class_definition,
        $.flag_statement,
      ),

    // ── say ──────────────────────────────────────────────────

    say_statement: ($) =>
      seq("say", $._expression),

    // ── var ──────────────────────────────────────────────────

    variable_declaration: ($) =>
      seq(
        "var",
        field("name", $.identifier),
        field("type", $._type_annotation),
        "=",
        field("value", $._expression),
      ),

    _type_annotation: ($) =>
      choice(
        "string", "int", "float", "boolean", "bool",
        "dynamic", "empty", "list", "dict", "any",
      ),

    // ── is assignment ───────────────────────────────────────

    assignment: ($) =>
      prec.left(seq(
        field("name", $.identifier),
        choice("is", "="),
        field("value", $._expression),
      )),

    // ── fun ─────────────────────────────────────────────────

    function_definition: ($) =>
      seq(
        "fun",
        field("name", $.identifier),
        field("parameters", $.parameter_list),
        optional(seq("->", field("return_type", $._type_annotation))),
        $.indent,
        repeat($.statement),
        $.dedent,
      ),

    parameter_list: ($) =>
      repeat1(seq(
        field("param", $.identifier),
        optional(field("param_type", $._type_annotation)),
      )),

    // ── give ────────────────────────────────────────────────

    return_statement: ($) =>
      seq("give", $._expression),

    // ── if / otherwise / or ─────────────────────────────────

    if_statement: ($) =>
      seq(
        "if",
        field("condition", $._expression),
        $.indent,
        field("consequence", repeat($.statement)),
        $.dedent,
        repeat(seq(
          choice("or", "otherwise"),
          optional(field("alt_condition", $._expression)),
          $.indent,
          field("alt_body", repeat($.statement)),
          $.dedent,
        )),
      ),

    // ── match / case ────────────────────────────────────────

    match_statement: ($) =>
      seq(
        "match",
        field("subject", $._expression),
        ":",
        $.indent,
        repeat1(seq(
          "case",
          field("pattern", $._expression),
          ":",
          $.indent,
          field("case_body", repeat($.statement)),
          $.dedent,
        )),
        optional(seq(
          "otherwise",
          ":",
          $.indent,
          field("otherwise_body", repeat($.statement)),
          $.dedent,
        )),
        $.dedent,
      ),

    // ── do / catch ──────────────────────────────────────────

    do_catch_statement: ($) =>
      seq(
        "do",
        ":",
        $.indent,
        field("do_body", repeat($.statement)),
        $.dedent,
        repeat(seq(
          "catch",
          optional(seq("as", field("error_var", $.identifier))),
          ":",
          $.indent,
          field("catch_body", repeat($.statement)),
          $.dedent,
        )),
        optional(seq(
          "otherwise",
          ":",
          $.indent,
          field("otherwise_body", repeat($.statement)),
          $.dedent,
        )),
        optional(seq(
          "lastly",
          ":",
          $.indent,
          field("lastly_body", repeat($.statement)),
          $.dedent,
        )),
      ),

    // ── repeat ──────────────────────────────────────────────

    repeat_statement: ($) =>
      seq(
        "repeat",
        optional(field("mode", choice(
          alias($.identifier, $.count),
          seq(field("item", $.identifier), "in", field("iterable", $._expression)),
          seq("while", field("while_cond", $._expression)),
          seq("until", field("until_cond", $._expression)),
        ))),
        $.indent,
        field("body", repeat($.statement)),
        $.dedent,
      ),

    // ── stop / next / reset ─────────────────────────────────

    stop_statement: ($) => "stop",
    next_statement: ($) => "next",
    reset_statement: ($) => "reset",

    // ── get (import) ────────────────────────────────────────

    import_statement: ($) =>
      seq(
        "get",
        field("module", $.identifier),
        optional(seq("from", field("source", $.identifier))),
        optional(seq("as", field("alias", $.identifier))),
      ),

    // ── expression statement ────────────────────────────────

    expression_statement: ($) => $._expression,

    // ── MakeType ────────────────────────────────────────────

    make_type_statement: ($) =>
      seq(
        field("type", $._type_annotation),
        field("name", $.identifier),
      ),

    // ── class ───────────────────────────────────────────────

    class_definition: ($) =>
      seq(
        "class",
        field("name", $.identifier),
        $.indent,
        repeat(choice(
          seq(field("field_name", $.identifier), ":", field("field_type", $._type_annotation)),
          $.function_definition,
        )),
        $.dedent,
      ),

    // ── flag ────────────────────────────────────────────────

    flag_statement: ($) =>
      seq("flag", $._expression),

    // ═══════════════════════════════════════════════════════════
    // Expressions (Pratt parser style)
    // ═══════════════════════════════════════════════════════════

    _expression: ($) =>
      choice(
        $.binary_expression,
        $.unary_expression,
        $.call_expression,
        $.member_expression,
        $.index_expression,
        $.primary_expression,
      ),

    primary_expression: ($) =>
      choice(
        $.string,
        $.number,
        $.color_literal,
        $.boolean,
        $.identifier,
        $.list_literal,
        $.dict_literal,
        seq("(", $._expression, ")"),
      ),

    // ── Literals ────────────────────────────────────────────

    string: ($) =>
      choice(
        seq('"', repeat(choice(/[^"\\]/, $.escape_sequence)), '"'),
      ),

    escape_sequence: ($) =>
      token.immediate(seq("\\", /[nrt\\"0]/)),

    number: ($) =>
      token(choice(
        /\d+\.\d+(?:[eE][+-]?\d+)?/,
        /\d+/,
      )),

    color_literal: ($) =>
      token(/#[0-9a-fA-F]{3,8}/),

    boolean: ($) =>
      choice("true", "false", "yes", "no", "empty"),

    identifier: ($) =>
      /[a-zA-Z_][a-zA-Z0-9_]*/,

    // ── List / Dict literals ────────────────────────────────

    list_literal: ($) =>
      seq("[", comma_sep($, $._expression), "]"),

    dict_literal: ($) =>
      seq("{", comma_sep($, $.dict_entry), "}"),

    dict_entry: ($) =>
      seq($.string, ":", $._expression),

    // ── Call ────────────────────────────────────────────────

    call_expression: ($) =>
      prec.left(seq(
        field("callee", $.identifier),
        "(",
        comma_sep($, choice(
          $._expression,
          seq(field("arg_name", $.identifier), "=", field("arg_value", $._expression)),
        )),
        ")",
      )),

    // ── Member access ───────────────────────────────────────

    member_expression: ($) =>
      prec.left(seq(
        field("object", $._expression),
        ".",
        field("member", $.identifier),
      )),

    // ── Index access ────────────────────────────────────────

    index_expression: ($) =>
      prec.left(seq(
        field("object", $._expression),
        "[",
        field("index", $._expression),
        optional(seq(":", field("end", optional($._expression)))),
        optional(seq(":", field("step", $._expression))),
        "]",
      )),

    // ── Binary ──────────────────────────────────────────────

    binary_expression: ($) =>
      choice(
        prec.left(1, seq($._expression, choice("or", "and"), $._expression)),
        prec.left(2, seq($._expression, choice("==", "!=", "<=", ">=", "<", ">"), $._expression)),
        prec.left(3, seq($._expression, choice("+", "-"), $._expression)),
        prec.left(4, seq($._expression, choice("*", "/", "%", "^"), $._expression)),
      ),

    // ── Unary ───────────────────────────────────────────────

    unary_expression: ($) =>
      seq(choice("-", "not"), $._expression),

    // ═══════════════════════════════════════════════════════════
    // Comments
    // ═══════════════════════════════════════════════════════════

    comment: ($) =>
      choice(
        seq("#!", /.*/),
        seq("#!*", /[\s\S]*?/, "#!*"),
      ),
  },
});

// ── Helpers ────────────────────────────────────────────────────────

function comma_sep($, rule) {
  return optional(seq(rule, repeat(seq(",", rule)), optional(",")));
}
