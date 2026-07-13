//! indent-lsp — Tower-LSP Language Server for Indent
//!
//! Provides IDE features directly from the Rust Indent compiler:
//!   - Semantic tokens (syntax highlighting driven by the AST)
//!   - Diagnostics (parse errors surfaced as you type)
//!   - Hover (type info, documentation)
//!   - Go-to-definition
//!   - Completions (keywords, builtins, variables in scope)
//!   - Document symbols (outline)
//!   - Folding ranges
//!
//! Architecture:
//!   ┌─────────────┐     JSON-RPC     ┌──────────────┐
//!   │  CodeMirror │ ◄──────────────► │  indent-lsp  │
//!   │  (Tauri UI) │    over stdio    │  (Rust bin)  │
//!   └─────────────┘                  └──────┬───────┘
//!                                           │
//!                                    ┌──────▼───────┐
//!                                    │  indent-core │
//!                                    │  (AST/types) │
//!                                    └──────────────┘

use indent_core::*;
use dashmap::DashMap;
use log::info;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

// ═══════════════════════════════════════════════════════════════════
// Backend state
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Default)]
struct TextDocument {
    uri: Url,
    content: String,
    language_id: String,
    version: i32,
}

/// Parsed document with AST and diagnostics.
#[derive(Debug)]
struct ParsedDocument {
    /// Raw source text
    source: String,
    /// Line offsets for position calculation
    line_offsets: Vec<usize>,
    /// Diagnostics from the last parse
    diagnostics: Vec<Diagnostic>,
    /// Collected document symbols
    symbols: Vec<DocumentSymbol>,
}

impl ParsedDocument {
    fn parse(source: &str) -> Self {
        let line_offsets: Vec<usize> = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(i, _)| i + 1))
            .collect();

        let mut diagnostics = Vec::new();
        let mut symbols = Vec::new();

        // Simple line-by-line AST extraction for IDE features.
        // Full deep parsing is in the indent CLI binary; here we do
        // lightweight structural analysis sufficient for LSP features.
        let lines: Vec<&str> = source.lines().collect();

        for (line_idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("#!") {
                continue;
            }

            let pos = Position::new(line_idx, line.len() - trimmed.len());

            // Detect function definitions
            if let Some(name) = trimmed.strip_prefix("fun ") {
                if let Some(first_space) = name.find(|c: char| c.is_whitespace() || c == '(')
                {
                    let func_name = &name[..first_space];
                    let end_pos = Position::new(
                        line_idx,
                        line.len() - trimmed.len() + trimmed.len(),
                    );
                    symbols.push(DocumentSymbol {
                        name: func_name.to_string(),
                        detail: Some("function".to_string()),
                        kind: SymbolKind::FUNCTION,
                        range: Range::new(pos.to_lsp(), end_pos.to_lsp()),
                        selection_range: Range::new(pos.to_lsp(), pos.to_lsp()),
                        children: None,
                        tags: None,
                        deprecated: None,
                    });
                }
            }

            // Detect variable declarations
            if trimmed.starts_with("var ") {
                let rest = &trimmed[4..];
                if let Some(first_space) = rest.find(|c: char| c.is_whitespace()) {
                    let var_name = &rest[..first_space];
                    let end_pos = Position::new(
                        line_idx,
                        line.len() - trimmed.len() + trimmed.len(),
                    );
                    symbols.push(DocumentSymbol {
                        name: var_name.to_string(),
                        detail: Some("variable".to_string()),
                        kind: SymbolKind::VARIABLE,
                        range: Range::new(pos.to_lsp(), end_pos.to_lsp()),
                        selection_range: Range::new(pos.to_lsp(), pos.to_lsp()),
                        children: None,
                        tags: None,
                        deprecated: None,
                    });
                }
            }

            // Detect class definitions
            if let Some(name) = trimmed.strip_prefix("class ") {
                if let Some(first_space) = name.find(char::is_whitespace) {
                    let class_name = &name[..first_space];
                    let end_pos = Position::new(
                        line_idx,
                        line.len() - trimmed.len() + trimmed.len(),
                    );
                    symbols.push(DocumentSymbol {
                        name: class_name.to_string(),
                        detail: Some("class".to_string()),
                        kind: SymbolKind::CLASS,
                        range: Range::new(pos.to_lsp(), end_pos.to_lsp()),
                        selection_range: Range::new(pos.to_lsp(), pos.to_lsp()),
                        children: None,
                        tags: None,
                        deprecated: None,
                    });
                }
            }
        }

        ParsedDocument {
            source: source.to_string(),
            line_offsets,
            diagnostics,
            symbols,
        }
    }

    fn offset_to_position(&self, offset: usize) -> Position {
        let line = self
            .line_offsets
            .partition_point(|&o| o <= offset)
            .saturating_sub(1);
        let col = offset - self.line_offsets.get(line).copied().unwrap_or(0);
        Position::new(line, col)
    }
}

// ═══════════════════════════════════════════════════════════════════
// Language Server
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug)]
struct IndentLspBackend {
    client: Client,
    documents: Arc<DashMap<Url, TextDocument>>,
    parsed: Arc<DashMap<Url, ParsedDocument>>,
    analysis_cache: Arc<RwLock<HashMap<Url, Vec<Diagnostic>>>>,
}

impl IndentLspBackend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(DashMap::new()),
            parsed: Arc::new(DashMap::new()),
            analysis_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Parse and cache a document.
    async fn cache_document(&self, uri: &Url, source: &str) {
        let parsed = ParsedDocument::parse(source);
        self.parsed.insert(uri.clone(), parsed);
    }

    /// Semantic token types and modifiers for Indent.
    fn semantic_token_legend() -> SemanticTokensLegend {
        SemanticTokensLegend {
            token_types: vec![
                SemanticTokenType::KEYWORD,
                SemanticTokenType::FUNCTION,
                SemanticTokenType::VARIABLE,
                SemanticTokenType::STRING,
                SemanticTokenType::NUMBER,
                SemanticTokenType::COMMENT,
                SemanticTokenType::TYPE,
                SemanticTokenType::OPERATOR,
            ],
            token_modifiers: vec![
                SemanticTokenModifier::DEFINITION,
                SemanticTokenModifier::DECLARATION,
            ],
        }
    }

    /// Build completion items for Indent.
    fn build_completions(&self, prefix: &str, pos: Position) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        // Keywords
        for kw in KEYWORDS {
            if kw.starts_with(prefix) || prefix.is_empty() {
                items.push(CompletionItem {
                    label: kw.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some("Indent keyword".to_string()),
                    insert_text: Some(kw.to_string()),
                    sort_text: Some(format!("0_{}", kw)),
                    ..Default::default()
                });
            }
        }

        // Built-in types
        for ty in BUILTIN_TYPES {
            if ty.starts_with(prefix) || prefix.is_empty() {
                items.push(CompletionItem {
                    label: ty.to_string(),
                    kind: Some(CompletionItemKind::TYPE_PARAMETER),
                    detail: Some("Indent built-in type".to_string()),
                    insert_text: Some(ty.to_string()),
                    sort_text: Some(format!("1_{}", ty)),
                    ..Default::default()
                });
            }
        }

        // Built-in functions
        for (name, params, ret) in builtin_functions() {
            if name.starts_with(prefix) || prefix.is_empty() {
                let detail = format!("({}) -> {}", params, ret);
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(detail),
                    insert_text: Some(format!("{}($1)", name)),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    sort_text: Some(format!("2_{}", name)),
                    ..Default::default()
                });
            }
        }

        // Snippets
        items.push(CompletionItem {
            label: "fun — function definition".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text: Some("fun ${1:name}\n    ${2:body}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some("3_fun".to_string()),
            ..Default::default()
        });

        items.push(CompletionItem {
            label: "if — conditional".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text: Some("if ${1:condition}\n    ${2:body}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some("3_if".to_string()),
            ..Default::default()
        });

        items
    }
}

// ═══════════════════════════════════════════════════════════════════
// Tower-LSP trait implementation
// ═══════════════════════════════════════════════════════════════════

#[tower_lsp::async_trait]
impl LanguageServer for IndentLspBackend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        info!("indent-lsp initializing...");

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        "(".to_string(),
                        " ".to_string(),
                    ]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
                        SemanticTokensRegistrationOptions {
                            text_document_registration_options: {
                                TextDocumentRegistrationOptions {
                                    document_selector: Some(vec![Filter {
                                        language: Some("indent".to_string()),
                                        scheme: Some("file".to_string()),
                                        pattern: None,
                                    }]),
                                }
                            },
                            semantic_tokens_options: SemanticTokensOptions {
                                legend: Self::semantic_token_legend(),
                                range: Some(false),
                                full: Some(SemanticTokensFullOptions::Bool(true)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    ),
                ),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "indent-lsp".to_string(),
                version: Some("2.2.0".to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("indent-lsp initialized.");
        self.client
            .log_message(MessageType::INFO, "Indent LSP ready.")
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        info!("indent-lsp shutting down.");
        Ok(())
    }

    // ── Document sync ───────────────────────────────────────────

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let source = params.text_document.text.clone();

        self.documents.insert(
            uri.clone(),
            TextDocument {
                uri: uri.clone(),
                content: source.clone(),
                language_id: params.text_document.language_id,
                version: params.text_document.version,
            },
        );

        self.cache_document(&uri, &source).await;

        // Publish initial diagnostics
        if let Some(parsed) = self.parsed.get(&uri) {
            self.client
                .publish_diagnostics(uri.clone(), parsed.diagnostics.clone(), None)
                .await;
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();

        if let Some(mut doc) = self.documents.get_mut(&uri) {
            // Apply changes incrementally (simplified: full replace)
            for change in params.content_changes {
                if let Some(range) = change.range {
                    // Apply range-based edit
                    // (simplified — full reparse)
                }
                doc.content = change.text.clone();
                doc.version = params.text_document.version;
            }

            let source = doc.content.clone();
            drop(doc);
            self.cache_document(&uri, &source).await;

            if let Some(parsed) = self.parsed.get(&uri) {
                self.client
                    .publish_diagnostics(uri.clone(), parsed.diagnostics.clone(), None)
                    .await;
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri);
        self.parsed.remove(&params.text_document.uri);
    }

    // ── Completion ──────────────────────────────────────────────

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        // Determine prefix
        let prefix = if let Some(doc) = self.documents.get(&uri) {
            let line = doc
                .content
                .lines()
                .nth(pos.line as usize)
                .unwrap_or("");
            let col = pos.character as usize;
            let before = &line[..col.min(line.len())];
            before
                .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("")
        } else {
            ""
        };

        let items = self.build_completions(
            prefix,
            Position::new(pos.line as usize, pos.character as usize),
        );

        Ok(Some(CompletionResponse::Array(items)))
    }

    // ── Hover ───────────────────────────────────────────────────

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        if let Some(doc) = self.documents.get(&uri) {
            let line = doc
                .content
                .lines()
                .nth(pos.line as usize)
                .unwrap_or("");
            let word = extract_word_at(line, pos.character as usize);

            if let Some(word) = word {
                // Check builtins
                for (name, params, ret) in builtin_functions() {
                    if name == word {
                        return Ok(Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: format!(
                                    "**`{}({})` → `{}`**\n\nIndent built-in function.",
                                    name, params, ret
                                ),
                            }),
                            range: None,
                        }));
                    }
                }

                // Check keywords
                if KEYWORDS.contains(&word.as_str()) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: format!(
                                "**`{}`**\n\nIndent keyword.",
                                word
                            ),
                        }),
                        range: None,
                    }));
                }

                // Check types
                if BUILTIN_TYPES.contains(&word.as_str()) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: format!(
                                "**`{}`**\n\nIndent built-in type.",
                                word
                            ),
                        }),
                        range: None,
                    }));
                }
            }
        }

        Ok(None)
    }

    // ── Go-to-definition ────────────────────────────────────────

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        // For now, returns None — full def-finding needs AST traversal
        // from the compiler, which we'll wire in once the parser is
        // extracted to indent-core.
        Ok(None)
    }

    // ── Document Symbols ────────────────────────────────────────

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;

        if let Some(parsed) = self.parsed.get(&uri) {
            let symbols: Vec<DocumentSymbol> = parsed.symbols.clone();
            if symbols.is_empty() {
                return Ok(None);
            }
            return Ok(Some(DocumentSymbolResponse::Nested(symbols)));
        }

        Ok(None)
    }

    // ── Semantic Tokens ─────────────────────────────────────────

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> LspResult<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;

        if let Some(doc) = self.documents.get(&uri) {
            let source = &doc.content;
            let mut data = Vec::new();
            let mut prev_line = 0u32;
            let mut prev_col = 0u32;

            for (line_idx, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with("#!") {
                    continue;
                }

                let col = (line.len() - trimmed.len()) as u32;

                // Send tokens: (delta_line, delta_col, length, type_idx, modifier_bits)
                let push_token = |data: &mut Vec<u32>,
                                  line: u32,
                                  col: u32,
                                  len: u32,
                                  type_idx: u32,
                                  mods: u32| {
                    let d_line = line - prev_line;
                    let d_col = if d_line == 0 {
                        col - prev_col
                    } else {
                        col
                    };
                    prev_line = line;
                    prev_col = col;
                    data.extend([d_line, d_col, len, type_idx, mods]);
                };

                let words: Vec<&str> = trimmed.split_whitespace().collect();
                for word in &words {
                    let clean: String = word
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if clean.is_empty() {
                        continue;
                    }

                    let offset_in_line = trimmed.find(&clean).unwrap_or(0);
                    let abs_col = col + offset_in_line as u32;
                    let len = clean.len() as u32;

                    if KEYWORDS.contains(&clean.as_str()) {
                        push_token(&mut data, line_idx as u32, abs_col, len, 0, 0);
                    } else if BUILTIN_TYPES.contains(&clean.as_str()) {
                        push_token(&mut data, line_idx as u32, abs_col, len, 6, 0);
                    } else if builtin_functions()
                        .iter()
                        .any(|(n, _, _)| n == &clean.as_str())
                    {
                        push_token(&mut data, line_idx as u32, abs_col, len, 1, 0);
                    }
                }
            }

            if data.is_empty() {
                return Ok(None);
            }

            return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data,
            })));
        }

        Ok(None)
    }

    // ── Folding Ranges ──────────────────────────────────────────

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> LspResult<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri;
        let mut ranges = Vec::new();

        if let Some(doc) = self.documents.get(&uri) {
            let mut indent_stack: Vec<(u32, u32)> = Vec::new(); // (indent, start_line)

            for (line_idx, line) in doc.content.lines().enumerate() {
                let indent = line.len() - line.trim_start().len();
                let trimmed = line.trim();

                if trimmed.is_empty() || trimmed.starts_with("#!") {
                    continue;
                }

                // Pop stack — dedent
                while let Some(&(stack_indent, start)) = indent_stack.last() {
                    if indent <= stack_indent as usize {
                        let fold = FoldingRange {
                            start_line: start,
                            end_line: (line_idx as u32).saturating_sub(1),
                            kind: Some(FoldingRangeKind::Region),
                            collapsed_text: None,
                        };
                        if fold.end_line > fold.start_line {
                            ranges.push(fold);
                        }
                        indent_stack.pop();
                    } else {
                        break;
                    }
                }

                // Push new indent level on block-starting constructs
                if trimmed.starts_with("fun ")
                    || trimmed.starts_with("if ")
                    || trimmed.starts_with("match ")
                    || trimmed.starts_with("repeat ")
                    || trimmed.starts_with("do:")
                    || trimmed.starts_with("class ")
                {
                    indent_stack.push((indent as u32, line_idx as u32));
                }
            }

            // Close remaining folds
            let last_line = doc.content.lines().count().saturating_sub(1) as u32;
            for (_, start) in indent_stack {
                let fold = FoldingRange {
                    start_line: start,
                    end_line: last_line,
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: None,
                };
                if fold.end_line > fold.start_line {
                    ranges.push(fold);
                }
            }
        }

        if ranges.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ranges))
        }
    }

    // ── Formatting ──────────────────────────────────────────────

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        // Delegate to `indent fmt` via CLI — for now, return identity
        Ok(None)
    }
}

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

fn extract_word_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let col = col.min(chars.len());

    // Find start of word
    let mut start = col;
    while start > 0 && chars[start - 1].is_alphanumeric() {
        start -= 1;
    }

    // Find end of word
    let mut end = col;
    while end < chars.len() && chars[end].is_alphanumeric() {
        end += 1;
    }

    if start < end {
        Some(chars[start..end].iter().collect())
    } else {
        None
    }
}

// ═══════════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("indent-lsp v{} starting on stdio...", env!("CARGO_PKG_VERSION"));

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(IndentLspBackend::new);

    Server::new(stdin, stdout, socket).serve(service).await;
}
