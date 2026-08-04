//! Indent Forge IDE — Tauri 2.0 main process
//!
//! This is the Rust backend for the Indent Forge IDE. It handles:
//!   - File system operations (read/write/list)
//!   - Indent LSP process management (spawns indent-lsp on demand)
//!   - Scrible AI agent (Ollama HTTP bridge for completions and chat)
//!   - Indent runtime execution (spawns indent CLI)
//!
//! Architecture:
//!   ┌────────────────────────────────────────────────────┐
//!   │  WebView (CodeMirror 6 + HTML/CSS/JS)              │
//!   │  ↔ Tauri IPC (JSON commands)                       │
//!   ├────────────────────────────────────────────────────┤
//!   │  Rust Backend (this file)                          │
//!   │  ├── File ops                                     │
//!   │  ├── indent-lsp manager                           │
//!   │  ├── Scrible AI (Ollama HTTP)                     │
//!   │  └── indent runtime (CLI spawn)                   │
//!   └────────────────────────────────────────────────────┘

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::State;

// ═══════════════════════════════════════════════════════════════════
// State
// ═══════════════════════════════════════════════════════════════════

struct TerminalState {
    stdin: Mutex<Option<std::process::ChildStdin>>,
    output_buf: Arc<Mutex<String>>,
    process: Mutex<Option<Child>>,
}

struct ForgeState {
    indent_process: Mutex<Option<Child>>,
    lsp_process: Mutex<Option<Child>>,
    terminal: TerminalState,
}

// ═══════════════════════════════════════════════════════════════════
// IPC Commands
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Deserialize)]
struct FileEntry {
    name: String,
    is_directory: bool,
    path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileResult {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entries: Option<Vec<FileEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ScribleQuery {
    endpoint: String,
    model: String,
    prompt: String,
    #[serde(default)]
    temperature: f64,
    #[serde(default = "default_max_tokens")]
    max_tokens: u32,
}

fn default_max_tokens() -> u32 {
    256
}

#[derive(Debug, Serialize)]
struct ScribleResult {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ScribleChatQuery {
    endpoint: String,
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(default)]
    temperature: f64,
    #[serde(default = "default_max_tokens")]
    max_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

// ── Integrated Terminal ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ShellCommand {
    command: String,
}

#[derive(Debug, Serialize, Clone)]
struct TerminalResult {
    success: bool,
    output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tauri::command]
fn run_shell(state: State<ForgeState>, cmd: ShellCommand) -> TerminalResult {
    // Kill any previous process
    if let Ok(mut proc) = state.terminal.process.lock() {
        if let Some(ref mut child) = *proc {
            let _ = child.kill();
            let _ = child.wait();
        }
        *proc = None;
    }
    if let Ok(mut stdin_guard) = state.terminal.stdin.lock() {
        *stdin_guard = None;
    }
    // Clear output buffer
    if let Ok(mut buf) = state.terminal.output_buf.lock() {
        buf.clear();
    }

    // Spawn with script for PTY (so interactive programs like sudo work)
    let escaped = cmd.command.replace('\'', "'\\''");
    let shell_cmd = format!("script -q -c '{}' /dev/null", escaped);

    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(&shell_cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return TerminalResult { success: false, output: String::new(), error: Some(format!("Failed: {}", e)) },
    };

    let stdin = child.stdin.take();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    if let Ok(mut guard) = state.terminal.stdin.lock() {
        *guard = stdin;
    }

    let output_buf = state.terminal.output_buf.clone();

    // Background thread: read stdout+stderr into shared buffer
    std::thread::spawn(move || {
        let mut combined = stdout.chain(stderr);
        let mut buf = [0u8; 1024];
        loop {
            match combined.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut ob) = output_buf.lock() {
                        ob.push_str(&String::from_utf8_lossy(&buf[..n]));
                    }
                }
                Err(_) => break,
            }
        }
        let _ = child.wait();
    });

    TerminalResult {
        success: true,
        output: format!("$ {}\n", cmd.command),
        error: None,
    }
}

#[derive(Debug, Deserialize)]
struct TerminalInput {
    input: String,
}

#[tauri::command]
fn terminal_write(state: State<ForgeState>, ti: TerminalInput) -> TerminalResult {
    if let Ok(mut guard) = state.terminal.stdin.lock() {
        if let Some(ref mut stdin) = *guard {
            let _ = stdin.write_all(ti.input.as_bytes());
            let _ = stdin.write_all(b"\n");
            let _ = stdin.flush();
            return TerminalResult { success: true, output: String::new(), error: None };
        }
    }
    TerminalResult { success: false, output: String::new(), error: Some("No running process".into()) }
}

#[tauri::command]
fn terminal_read(state: State<ForgeState>) -> TerminalResult {
    if let Ok(mut buf) = state.terminal.output_buf.lock() {
        let out = buf.clone();
        buf.clear();
        TerminalResult { success: true, output: out, error: None }
    } else {
        TerminalResult { success: false, output: String::new(), error: Some("Lock failed".into()) }
    }
}

#[tauri::command]
fn stop_terminal(state: State<ForgeState>) -> TerminalResult {
    if let Ok(mut proc) = state.terminal.process.lock() {
        if let Some(ref mut child) = *proc {
            let _ = child.kill();
            let _ = child.wait();
        }
        *proc = None;
    }
    if let Ok(mut guard) = state.terminal.stdin.lock() {
        *guard = None;
    }
    TerminalResult { success: true, output: "\n── Process terminated ──\n".into(), error: None }
}

// ── File Operations ────────────────────────────────────────────────

#[tauri::command]
fn read_file(path: String) -> FileResult {
    match fs::read_to_string(&path) {
        Ok(content) => FileResult {
            success: true,
            content: Some(content),
            entries: None,
            error: None,
        },
        Err(e) => FileResult {
            success: false,
            content: None,
            entries: None,
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
fn write_file(path: String, content: String) -> FileResult {
    // Ensure parent directory exists
    if let Some(parent) = PathBuf::from(&path).parent() {
        let _ = fs::create_dir_all(parent);
    }

    match fs::write(&path, &content) {
        Ok(_) => FileResult {
            success: true,
            content: None,
            entries: None,
            error: None,
        },
        Err(e) => FileResult {
            success: false,
            content: None,
            entries: None,
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
fn list_dir(path: String) -> FileResult {
    match fs::read_dir(&path) {
        Ok(entries) => {
            let mut file_entries = Vec::new();
            for entry in entries.flatten() {
                let file_type = entry.file_type().ok();
                file_entries.push(FileEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    is_directory: file_type.map(|t| t.is_dir()).unwrap_or(false),
                    path: entry.path().to_string_lossy().to_string(),
                });
            }
            // Sort: directories first, then alphabetically
            file_entries.sort_by(|a, b| {
                b.is_directory
                    .cmp(&a.is_directory)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            FileResult {
                success: true,
                content: None,
                entries: Some(file_entries),
                error: None,
            }
        }
        Err(e) => FileResult {
            success: false,
            content: None,
            entries: None,
            error: Some(e.to_string()),
        },
    }
}

// ── Indent Runtime ─────────────────────────────────────────────────

#[tauri::command]
fn run_indent(path: String, debug: bool, state: State<ForgeState>) -> FileResult {
    if let Ok(mut proc) = state.indent_process.lock() {
        if let Some(ref mut child) = *proc { let _ = child.kill(); }
    }
    // Detect available terminal emulator
    let terminals = [
        ("ghostty",       &["-e"][..]),
        ("kitty",         &["--"][..]),
        ("alacritty",     &["-e"][..]),
        ("konsole",       &["-e"][..]),
        ("xfce4-terminal",&["-e"][..]),
        ("gnome-terminal",&["--"][..]),
        ("xterm",         &["-e"][..]),
        ("foot",          &["--"][..]),
        ("lxterminal",    &["-e"][..]),
    ];
    let term = std::env::var("TERMINAL").ok()
        .or_else(|| terminals.iter().find(|(name,_)| which::which(name).is_ok()).map(|(n,_)| n.to_string()))
        .unwrap_or_else(|| "xterm".into());

    let (cmd_name, extra_args) = terminals.iter()
        .find(|(name,_)| name == &term.as_str())
        .map(|(_,args)| (term.as_str(), *args))
        .unwrap_or_else(|| ("xterm", &["-e"][..]));

    // Wrap in sh -c so &&, echo, read work with every terminal emulator
    let shell_cmd = format!(
        "indent {} && echo && echo '── Press Enter to close ──' && read",
        if debug { format!("--debug \"{}\"", path) } else { format!("\"{}\"", path) }
    );

    let mut cmd = std::process::Command::new(cmd_name);
    cmd.args(extra_args).arg("sh").arg("-c").arg(&shell_cmd);
    let status = cmd.spawn();

    match status {
        Ok(child) => {
            if let Ok(mut proc) = state.indent_process.lock() { *proc = Some(child); }
            FileResult { success: true, content: Some(format!("Running in {}...", cmd_name).into()), entries: None, error: None }
        }
        Err(e) => FileResult {
            success: false, content: None, entries: None,
            error: Some(format!("Could not open terminal: {}. Install xterm or set TERMINAL env var.", e)),
        },
    }
}

#[tauri::command]
fn stop_execution(state: State<ForgeState>) -> FileResult {
    if let Ok(mut proc) = state.indent_process.lock() {
        if let Some(ref mut child) = *proc {
            let _ = child.kill();
            *proc = None;
        }
    }
    FileResult {
        success: true,
        content: None,
        entries: None,
        error: None,
    }
}

// ── Scrible AI (Ollama bridge) ─────────────────────────────────────

#[tauri::command]
async fn scrible_complete(query: ScribleQuery) -> ScribleResult {
    let client = reqwest::Client::new();
    let url = format!("{}/api/generate", query.endpoint);

    // FIM format for StarCoder2
    let fim_prefix = "<fim_prefix>";
    let _fim_suffix = "<fim_suffix>";
    let fim_middle = "<fim_middle>";

    // If the prompt contains FIM markers, use as-is; otherwise wrap
    let prompt = if query.prompt.contains(fim_prefix) {
        query.prompt
    } else {
        format!("{}{}{}{}", fim_prefix, query.prompt, fim_middle, "")
    };

    let body = serde_json::json!({
        "model": query.model,
        "prompt": prompt,
        "stream": false,
        "options": {
            "num_predict": query.max_tokens,
            "temperature": query.temperature,
            "top_p": 0.95,
            "stop": ["<|endoftext|>", "<fim_prefix>", "<fim_suffix>", "\n\n\n"]
        }
    });

    match client.post(&url).json(&body).timeout(std::time::Duration::from_secs(10)).send().await {
        Ok(resp) => {
            match resp.json::<serde_json::Value>().await {
                Ok(json) => {
                    let text = json["response"].as_str().unwrap_or("").to_string();
                    ScribleResult {
                        success: true,
                        response: Some(text),
                        error: None,
                    }
                }
                Err(e) => ScribleResult {
                    success: false,
                    response: None,
                    error: Some(format!("Parse error: {}", e)),
                },
            }
        }
        Err(e) => ScribleResult {
            success: false,
            response: None,
            error: Some(format!("Ollama unreachable: {}", e)),
        },
    }
}

#[tauri::command]
async fn scrible_chat(query: ScribleChatQuery) -> ScribleResult {
    let client = reqwest::Client::new();
    let url = format!("{}/api/chat", query.endpoint);

    let body = serde_json::json!({
        "model": query.model,
        "messages": query.messages,
        "stream": false,
        "options": {
            "num_predict": query.max_tokens,
            "temperature": query.temperature,
            "top_p": 0.95,
        }
    });

    match client.post(&url).json(&body).timeout(std::time::Duration::from_secs(30)).send().await {
        Ok(resp) => {
            match resp.json::<serde_json::Value>().await {
                Ok(json) => {
                    let content = json["message"]["content"]
                        .as_str()
                        .unwrap_or("_(No response)_")
                        .to_string();
                    ScribleResult {
                        success: true,
                        response: Some(content),
                        error: None,
                    }
                }
                Err(e) => ScribleResult {
                    success: false,
                    response: None,
                    error: Some(format!("Parse error: {}", e)),
                },
            }
        }
        Err(e) => ScribleResult {
            success: false,
            response: None,
            error: Some(format!("Ollama unreachable: {}", e)),
        },
    }
}

// ── LSP Lifecycle ──────────────────────────────────────────────────

#[tauri::command]
fn start_lsp(state: State<ForgeState>) -> FileResult {
    // Check if already running
    if let Ok(proc) = state.lsp_process.lock() {
        if proc.is_some() {
            return FileResult {
                success: true,
                content: Some("LSP already running".to_string()),
                entries: None,
                error: None,
            };
        }
    }

    // Try to spawn indent-lsp
    match Command::new("indent-lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => {
            if let Ok(mut proc) = state.lsp_process.lock() {
                *proc = Some(child);
            }
            FileResult {
                success: true,
                content: Some("LSP started".to_string()),
                entries: None,
                error: None,
            }
        }
        Err(e) => FileResult {
            success: false,
            content: None,
            entries: None,
            error: Some(format!("Failed to start indent-lsp: {}. Is it installed?", e)),
        },
    }
}

#[tauri::command]
fn stop_lsp(state: State<ForgeState>) -> FileResult {
    if let Ok(mut proc) = state.lsp_process.lock() {
        if let Some(ref mut child) = *proc {
            let _ = child.kill();
            *proc = None;
        }
    }
    FileResult {
        success: true,
        content: None,
        entries: None,
        error: None,
    }
}

// ═══════════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════════

fn main() {
    // Fix blank/grey screen on Wayland + webkit2gtk GPU rendering
    std::env::set_var("GDK_BACKEND", "x11");
    std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|_app| {
            // Auto-start Ollama in the background if not already running
            std::thread::spawn(|| {
                let status = std::process::Command::new("ollama")
                    .arg("serve")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                match status {
                    Ok(_) => println!("[Forge] Ollama service started"),
                    Err(e) => eprintln!("[Forge] Could not start Ollama: {}. Is it installed?", e),
                }
            });

            // Auto-create models from HuggingFace if not already installed
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(3)); // wait for ollama serve
                let models = vec![
                    ("scrible-chat", "https://huggingface.co/xytrolabs/scrible-chat/resolve/main/scrible-chat.gguf", "0.2", "<|end|>"),
                    ("scrible-inline", "https://huggingface.co/xytrolabs/scrible-inline/resolve/main/scrible-inline.gguf", "0.1", "<|endoftext|>"),
                ];
                for (name, url, temp, stop) in models {
                    // Check if model already exists
                    let list = std::process::Command::new("ollama")
                        .arg("list")
                        .output();
                    if let Ok(out) = list {
                        let list_str = String::from_utf8_lossy(&out.stdout);
                        if list_str.contains(name) {
                            println!("[Forge] Model '{}' already installed", name);
                            continue;
                        }
                    }
                    // Create model from HF
                    let modelfile = format!(
                        "FROM {}\nPARAMETER temperature {}\nPARAMETER stop \"{}\"\n",
                        url, temp, stop
                    );
                    let result = std::process::Command::new("ollama")
                        .arg("create")
                        .arg(name)
                        .arg("-f")
                        .arg("-")
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::piped())
                        .spawn();
                    match result {
                        Ok(mut child) => {
                            if let Some(stdin) = child.stdin.as_mut() {
                                use std::io::Write;
                                let _ = stdin.write_all(modelfile.as_bytes());
                            }
                            let output = child.wait_with_output();
                            match output {
                                Ok(o) if o.status.success() => {
                                    println!("[Forge] Model '{}' installed successfully", name);
                                }
                                Ok(o) => {
                                    let stderr = String::from_utf8_lossy(&o.stderr);
                                    eprintln!("[Forge] Failed to install model '{}': {}", name, stderr.trim());
                                }
                                Err(e) => {
                                    eprintln!("[Forge] Model '{}' install error: {}", name, e);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[Forge] Could not run ollama create for '{}': {}", name, e);
                        }
                    }
                }
            });

            Ok(())
        })
        .manage(ForgeState {
            indent_process: Mutex::new(None),
            lsp_process: Mutex::new(None),
            terminal: TerminalState {
                stdin: Mutex::new(None),
                output_buf: Arc::new(Mutex::new(String::new())),
                process: Mutex::new(None),
            },
        })
        .invoke_handler(tauri::generate_handler![
            read_file,
            write_file,
            list_dir,
            run_indent,
            stop_execution,
            run_shell,
            terminal_write,
            terminal_read,
            stop_terminal,
            scrible_complete,
            scrible_chat,
            start_lsp,
            stop_lsp,
        ])
        .run(tauri::generate_context!())
        .expect("Failed to launch Indent Forge");
}
