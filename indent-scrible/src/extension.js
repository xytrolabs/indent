const vscode = require('vscode');
const path = require('path');
const { ScribleCompletionProvider } = require('./completion');
const { ScribleChatPanel } = require('./chatPanel');
const { ScribleStatusCenter } = require('./statusCenter');

/**
 * Scrible — AI Coding Agent for the Indent Language
 *
 * Architecture:
 *   ┌─────────────────────────────────────────────────┐
 *   │  TOOLBAR (VS Code native)                       │
 *   ├────────┬──────────────────────┬─────────────────┤
 *   │ FILES  │  Active File         │  AI Coding      │
 *   │ (VS    │  (Text Editor)       │  Agent — Scrible│
 *   │  Code  │                      │  (Webview)      │
 *   │ native)│                      │                 │
 *   ├────────┴──────────────────────┴─────────────────┤
 *   │  Status Center (Panel)                          │
 *   └─────────────────────────────────────────────────┘
 */

let completionProvider;
let chatPanel;
let statusCenter;

/**
 * Activate the Scrible extension.
 */
function activate(context) {
    console.log('[Scrible] Activating Indent AI Coding Agent...');

    // ── Status bar indicator ──────────────────────────────────────
    const statusBarItem = vscode.window.createStatusBarItem(
        vscode.StatusBarAlignment.Right,
        100
    );
    statusBarItem.text = '$(robot) Scrible';
    statusBarItem.tooltip = 'Scrible AI Agent — Click for status';
    statusBarItem.command = 'indent-scrible.openStatusCenter';
    statusBarItem.show();
    context.subscriptions.push(statusBarItem);

    // ── Inline completion provider ────────────────────────────────
    completionProvider = new ScribleCompletionProvider();
    const completionDisposable = vscode.languages.registerInlineCompletionItemProvider(
        { language: 'indent' },
        completionProvider
    );
    context.subscriptions.push(completionDisposable);

    // ── Chat panel (sidebar webview) ──────────────────────────────
    chatPanel = new ScribleChatPanel(context);
    const chatViewDisposable = vscode.window.registerWebviewViewProvider(
        'indent-scrible.chatPanel',
        chatPanel,
        { webviewOptions: { retainContextWhenHidden: true } }
    );
    context.subscriptions.push(chatViewDisposable);

    // ── Status center (bottom panel webview) ──────────────────────
    statusCenter = new ScribleStatusCenter(context);
    const statusViewDisposable = vscode.window.registerWebviewViewProvider(
        'indent-scrible.statusCenter',
        statusCenter,
        { webviewOptions: { retainContextWhenHidden: true } }
    );
    context.subscriptions.push(statusViewDisposable);

    // ── Commands ──────────────────────────────────────────────────

    // Start Chat — focuses the Scrible sidebar
    context.subscriptions.push(
        vscode.commands.registerCommand('indent-scrible.startChat', () => {
            vscode.commands.executeCommand(
                'workbench.view.extension.indent-scrible-sidebar'
            );
        })
    );

    // Explain Code — sends selected text or full file to chat
    context.subscriptions.push(
        vscode.commands.registerCommand('indent-scrible.explainCode', async () => {
            const code = getSelectedOrAllCode();
            if (!code) {
                vscode.window.showWarningMessage('No Indent code to explain.');
                return;
            }
            chatPanel.sendMessage(`Explain what this Indent code does in detail:\n\`\`\`indent\n${code}\n\`\`\``);
            vscode.commands.executeCommand('workbench.view.extension.indent-scrible-sidebar');
        })
    );

    // Fix Code
    context.subscriptions.push(
        vscode.commands.registerCommand('indent-scrible.fixCode', async () => {
            const code = getSelectedOrAllCode();
            if (!code) {
                vscode.window.showWarningMessage('No Indent code to fix.');
                return;
            }
            chatPanel.sendMessage(
                `Find and fix any bugs or issues in this Indent code. Remember: #! is for comments, # alone is a hex color prefix. Use parenthesized function call syntax:\n\`\`\`indent\n${code}\n\`\`\``
            );
            vscode.commands.executeCommand('workbench.view.extension.indent-scrible-sidebar');
        })
    );

    // Generate Docstring
    context.subscriptions.push(
        vscode.commands.registerCommand('indent-scrible.generateDocstring', async () => {
            const code = getSelectedOrAllCode();
            if (!code) {
                vscode.window.showWarningMessage('No Indent code selected.');
                return;
            }
            chatPanel.sendMessage(
                `Generate a comprehensive #!* docstring comment for this Indent function:\n\`\`\`indent\n${code}\n\`\`\``
            );
            vscode.commands.executeCommand('workbench.view.extension.indent-scrible-sidebar');
        })
    );

    // Switch Model
    context.subscriptions.push(
        vscode.commands.registerCommand('indent-scrible.switchModel', async () => {
            const config = vscode.workspace.getConfiguration('indent-scrible');
            const current = config.get('chatModel', 'scrible-chat');

            const models = [
                { label: '$(hubot) Scrible Chat (Phi-3 v3 · HF)', model: 'scrible-chat', description: 'Primary chat model from HuggingFace' },
                { label: '$(zap) Scrible Inline (FIM · HF)', model: 'scrible-inline', description: 'Fill-in-the-middle completions' },
            ];
                { label: '$(robot) Indent Scrible 3B (pretrained)', value: 'indent-scrible:3b-q4', description: 'StarCoder2-3B fine-tuned on Indent code' },
                { label: '$(hubot) StarCoder2 3B (base)', value: 'starcoder2:3b', description: 'Base StarCoder2-3B model via Ollama' },
                { label: '$(beaker) DeepSeek Coder 1.3B', value: 'deepseek-coder:1.3b', description: 'Lightweight alternative via Ollama' },
                { label: '$(code) CodeLlama 7B', value: 'codellama:7b', description: 'Larger model via Ollama (needs more VRAM)' },
                { label: '$(gear) Custom Ollama Model...', value: 'custom', description: 'Use any Ollama model' },
            ];

            const picked = await vscode.window.showQuickPick(models, {
                placeHolder: `Current: ${current}`,
                title: 'Scrible: Switch AI Model',
            });

            if (picked) {
                if (picked.value === 'custom') {
                    const custom = await vscode.window.showInputBox({
                        prompt: 'Enter Ollama model name',
                        placeHolder: 'e.g., my-custom-model:latest',
                    });
                    if (custom) {
                        await config.update('customModel', custom, true);
                        await config.update('model', 'custom', true);
                    }
                } else {
                    await config.update('model', picked.value, true);
                }
                vscode.window.showInformationMessage(
                    `Scrible: Switched to ${picked.label}`
                );
                statusCenter.refresh();
            }
        })
    );

    // Toggle Autocomplete
    context.subscriptions.push(
        vscode.commands.registerCommand('indent-scrible.toggleAutocomplete', async () => {
            const config = vscode.workspace.getConfiguration('indent-scrible');
            const current = config.get('autocompleteEnabled', true);
            await config.update('autocompleteEnabled', !current, true);
            vscode.window.showInformationMessage(
                `Scrible autocomplete: ${!current ? 'ON' : 'OFF'}`
            );
        })
    );

    // Open Status Center
    context.subscriptions.push(
        vscode.commands.registerCommand('indent-scrible.openStatusCenter', () => {
            vscode.commands.executeCommand(
                'workbench.panel.indent-scrible-status.focus'
            );
        })
    );

    // ── Listen for config changes ─────────────────────────────────
    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration((e) => {
            if (e.affectsConfiguration('indent-scrible')) {
                statusCenter.refresh();
            }
        })
    );

    console.log('[Scrible] Indent AI Coding Agent ready.');
    vscode.window.showInformationMessage(
        '$(robot) Scrible AI Agent ready for Indent. Use $(comment-discussion) to chat.'
    );
}

/**
 * Get currently selected code, or all code in the active editor.
 */
function getSelectedOrAllCode() {
    const editor = vscode.window.activeTextEditor;
    if (!editor) return null;
    if (editor.document.languageId !== 'indent') return null;

    const selection = editor.selection;
    if (!selection.isEmpty) {
        return editor.document.getText(selection);
    }
    return editor.document.getText();
}

/**
 * Deactivate the extension.
 */
function deactivate() {
    if (completionProvider) {
        completionProvider.dispose();
    }
    console.log('[Scrible] Deactivated.');
}

module.exports = { activate, deactivate };
