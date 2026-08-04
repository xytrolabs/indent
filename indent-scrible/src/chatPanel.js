const vscode = require('vscode');
const path = require('path');
const http = require('http');

/**
 * ScribleChatPanel — Right-side AI chat panel (webview).
 *
 * Provides a conversational interface for the Indent coding agent.
 * Users can ask questions, request code generation, get explanations,
 * and receive code fixes — all through natural language.
 */

class ScribleChatPanel {
    constructor(context) {
        this._context = context;
        this._view = null;
        this._webview = null;
        this._messageQueue = [];
        this._isReady = false;
    }

    /**
     * Called by VS Code when the webview view is created.
     */
    resolveWebviewView(webviewView, context, token) {
        this._view = webviewView;
        this._webview = webviewView.webview;

        webviewView.webview.options = {
            enableScripts: true,
            localResourceRoots: [
                vscode.Uri.file(path.join(this._context.extensionPath, 'webview')),
            ],
        };

        webviewView.webview.html = this._getHtml();

        // Handle messages from the webview
        webviewView.webview.onDidReceiveMessage(
            async (message) => {
                switch (message.type) {
                    case 'ready':
                        this._isReady = true;
                        this._flushQueue();
                        break;
                    case 'send':
                        await this._handleChatMessage(message.text);
                        break;
                    case 'insertCode':
                        await this._insertCodeIntoEditor(message.code);
                        break;
                    case 'copyCode':
                        await vscode.env.clipboard.writeText(message.code);
                        vscode.window.showInformationMessage('Code copied to clipboard!');
                        break;
                }
            },
            undefined,
            this._context.subscriptions
        );

        // Send the current model info
        this._sendStatus();
    }

    /**
     * Programmatically send a message (from commands).
     */
    sendMessage(text) {
        if (this._isReady && this._webview) {
            this._webview.postMessage({ type: 'userMessage', text });
        } else {
            this._messageQueue.push(text);
        }
        // Ensure the view is visible
        if (this._view) {
            this._view.show(true);
        }
    }

    /**
     * Flush queued messages once the webview is ready.
     */
    _flushQueue() {
        while (this._messageQueue.length > 0) {
            const text = this._messageQueue.shift();
            this._webview.postMessage({ type: 'userMessage', text });
        }
        this._sendStatus();
    }

    /**
     * Send current configuration status to the webview.
     */
    _sendStatus() {
        if (!this._webview || !this._isReady) return;
        const config = vscode.workspace.getConfiguration('indent-scrible');
        this._webview.postMessage({
            type: 'status',
            model: this._getModelName(config),
            autocomplete: config.get('autocompleteEnabled', true),
            endpoint: config.get('ollamaEndpoint', 'http://localhost:11434'),
        });
    }

    _getModelName(config) {
        return config.get('chatModel', 'scrible-chat');
    }

    /**
     * Handle a chat message from the user.
     */
    async _handleChatMessage(userText) {
        const config = vscode.workspace.getConfiguration('indent-scrible');
        const modelName = this._getModelName(config);
        const ollamaEndpoint = config.get('ollamaEndpoint', 'http://localhost:11434');

        // Get current editor context for better responses
        const contextCode = this._getEditorContext();

        const systemPrompt = `You are Scrible, an AI coding assistant specialized in the Indent programming language, created by Xytro Labs.

Indent is a clean, expression-oriented language with the following key rules:

SYNTAX RULES (CRITICAL):
- Comments use #! (NOT # alone — # is a hex color prefix like #ff4d4d)
- Function calls: ALWAYS use parenthesized syntax func(args) — it works everywhere
- Space-separated func arg only works in give, var, and standalone calls
- Functions: fun name params\\n    body
- Variables: var name type = value
- Conditionals: if cond\\n    body\\notherwise\\n    body
- Loops: repeat count\\n    body  OR  repeat item in list\\n    body
- Return: give value
- Import: get Module  OR  get Symbol from Module
- Types: string, int, float, boolean, dynamic, empty
- Match: match subject:\\n    case value:\\n        body
- Dicts/lists are immutable — reassign after modification

${contextCode ? `Current editor context:\\n\`\`\`indent\\n${contextCode}\\n\`\`\`\\n` : ''}
Respond concisely with correct Indent code. Always use parenthesized syntax for function calls.`;

        // Show a typing indicator
        this._webview.postMessage({ type: 'typing', active: true });

        try {
            const response = await this._queryOllamaChat(
                ollamaEndpoint,
                modelName,
                systemPrompt,
                userText,
                config
            );

            this._webview.postMessage({
                type: 'response',
                text: response || '_(No response from model. Is Ollama running?)_',
            });
        } catch (err) {
            this._webview.postMessage({
                type: 'response',
                text: `**Error:** ${err.message}\\n\\n> Make sure Ollama is running and the model is installed.\\n> Run: \`ollama pull ${modelName}\``,
            });
        } finally {
            this._webview.postMessage({ type: 'typing', active: false });
        }
    }

    /**
     * Get the current editor's code for context.
     */
    _getEditorContext() {
        const editor = vscode.window.activeTextEditor;
        if (!editor || editor.document.languageId !== 'indent') return '';

        const selection = editor.selection;
        if (!selection.isEmpty) {
            return editor.document.getText(selection);
        }

        // Return up to 200 lines of context around cursor
        const doc = editor.document;
        const cursorLine = selection.active.line;
        const startLine = Math.max(0, cursorLine - 100);
        const endLine = Math.min(doc.lineCount - 1, cursorLine + 100);
        return doc.getText(new vscode.Range(startLine, 0, endLine, doc.lineAt(endLine).text.length));
    }

    /**
     * Query Ollama chat API.
     */
    async _queryOllamaChat(endpoint, model, system, userMessage, config) {
        const maxTokens = config.get('maxTokens', 512);
        const temperature = config.get('temperature', 0.3);

        const requestBody = JSON.stringify({
            model: model,
            messages: [
                { role: 'system', content: system },
                { role: 'user', content: userMessage },
            ],
            stream: false,
            options: {
                num_predict: maxTokens,
                temperature: temperature,
                top_p: 0.95,
                top_k: 40,
            },
        });

        return new Promise((resolve, reject) => {
            const url = new URL('/api/chat', endpoint);
            const req = http.request(
                {
                    hostname: url.hostname,
                    port: url.port || 11434,
                    path: url.pathname,
                    method: 'POST',
                    headers: {
                        'Content-Type': 'application/json',
                        'Content-Length': Buffer.byteLength(requestBody),
                    },
                    timeout: 30000,
                },
                (res) => {
                    let data = '';
                    res.on('data', (chunk) => (data += chunk));
                    res.on('end', () => {
                        try {
                            const result = JSON.parse(data);
                            resolve(result.message?.content || '');
                        } catch (e) {
                            reject(new Error(`Failed to parse response: ${e.message}`));
                        }
                    });
                }
            );

            req.on('error', reject);
            req.on('timeout', () => {
                req.destroy();
                reject(new Error('Request timed out (30s)'));
            });

            req.write(requestBody);
            req.end();
        });
    }

    /**
     * Insert code from chat into the active editor.
     */
    async _insertCodeIntoEditor(code) {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            vscode.window.showWarningMessage('No active editor to insert code into.');
            return;
        }

        // Extract code from markdown code blocks if present
        const codeBlockMatch = code.match(/```(?:indent)?\s*\n?([\s\S]*?)\n?```/);
        const cleanCode = codeBlockMatch ? codeBlockMatch[1].trim() : code.trim();

        await editor.edit((editBuilder) => {
            editBuilder.insert(editor.selection.active, '\n' + cleanCode + '\n');
        });

        vscode.window.showInformationMessage('Code inserted from Scrible!');
    }

    /**
     * Get the HTML for the webview.
     */
    _getHtml() {
        const webviewDir = vscode.Uri.file(
            path.join(this._context.extensionPath, 'webview')
        );
        const webviewUri = this._webview.asWebviewUri(webviewDir);

        return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${this._webview.cspSource} 'unsafe-inline'; script-src ${this._webview.cspSource} 'unsafe-inline'; img-src ${this._webview.cspSource} data:;">
    <link rel="stylesheet" href="${webviewUri}/chat.css">
    <title>Scrible AI Agent</title>
</head>
<body>
    <div class="scrible-container">
        <!-- Header -->
        <div class="scrible-header">
            <div class="scrible-logo">
                <span class="scrible-icon">$(robot)</span>
                <span class="scrible-title">Scrible</span>
            </div>
            <div class="scrible-model-badge" id="modelBadge">scrible-chat (Phi-3 v3 · HF)</div>
        </div>

        <!-- Messages area -->
        <div class="scrible-messages" id="messages">
            <div class="scrible-welcome">
                <div class="welcome-icon">$(robot)</div>
                <h3>Scrible AI Agent</h3>
                <p>I'm your Indent coding assistant. I can help you write, explain, and fix Indent code.</p>
                <p class="welcome-hint">User can use our preinstalled pretrained model, or Ollama model.</p>
                <div class="quick-actions">
                    <button class="quick-btn" data-action="Explain this Indent code">$(book) Explain Code</button>
                    <button class="quick-btn" data-action="Write an Indent function that">$(code) Write Code</button>
                    <button class="quick-btn" data-action="Fix this Indent code:">$(wrench) Fix Code</button>
                    <button class="quick-btn" data-action="How do I use the Indent language to">$(question) How To</button>
                </div>
            </div>
        </div>

        <!-- Typing indicator -->
        <div class="scrible-typing" id="typingIndicator" style="display:none;">
            <span class="typing-dot"></span>
            <span class="typing-dot"></span>
            <span class="typing-dot"></span>
            <span>Scrible is thinking...</span>
        </div>

        <!-- Input area -->
        <div class="scrible-input-area">
            <div class="scrible-input-wrapper">
                <textarea
                    id="chatInput"
                    class="scrible-input"
                    placeholder="Ask Scrible about your Indent code..."
                    rows="2"
                ></textarea>
                <button class="scrible-send-btn" id="sendBtn" title="Send message">
                    $(send)
                </button>
            </div>
            <div class="scrible-input-hint">
                <span>$(info)</span> Scrible uses StarCoder2-3B Q4 fine-tuned on Indent
            </div>
        </div>
    </div>

    <script src="${webviewUri}/chat.js"></script>
</body>
</html>`;
    }
}

module.exports = { ScribleChatPanel };
