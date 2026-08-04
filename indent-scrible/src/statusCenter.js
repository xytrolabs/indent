const vscode = require('vscode');
const path = require('path');

/**
 * ScribleStatusCenter — Bottom panel showing model status, metrics,
 * and quick controls for the Scrible AI Agent.
 */

class ScribleStatusCenter {
    constructor(context) {
        this._context = context;
        this._view = null;
        this._webview = null;
    }

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
        this.refresh();
    }

    refresh() {
        if (!this._webview) return;

        const config = vscode.workspace.getConfiguration('indent-scrible');
        const model = config.get('model', 'indent-scrible:3b-q4');
        const actualModel = model === 'custom'
            ? config.get('customModel', 'indent-scrible:3b-q4')
            : model;
        const autocomplete = config.get('autocompleteEnabled', true);
        const endpoint = config.get('ollamaEndpoint', 'http://localhost:11434');

        this._webview.postMessage({
            type: 'refresh',
            model: actualModel,
            autocomplete: autocomplete,
            endpoint: endpoint,
        });
    }

    _getHtml() {
        return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${this._webview.cspSource} 'unsafe-inline'; script-src ${this._webview.cspSource} 'unsafe-inline';">
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            background: var(--vscode-panel-background, #1e1e2e);
            color: var(--vscode-panel-foreground, #cdd6f4);
            font-family: var(--vscode-font-family, 'Segoe UI', sans-serif);
            font-size: 12px;
            padding: 8px 12px;
        }
        .status-row {
            display: flex;
            align-items: center;
            gap: 16px;
            flex-wrap: wrap;
        }
        .status-item {
            display: flex;
            align-items: center;
            gap: 6px;
        }
        .status-label {
            color: var(--vscode-descriptionForeground, #6c7086);
            font-weight: 500;
        }
        .status-value {
            font-weight: 600;
        }
        .status-dot {
            width: 8px;
            height: 8px;
            border-radius: 50%;
            display: inline-block;
        }
        .dot-on { background: #a6e3a1; box-shadow: 0 0 6px #a6e3a1; }
        .dot-off { background: #f38ba8; }
        .dot-warn { background: #f9e2af; }
    </style>
</head>
<body>
    <div class="status-row">
        <div class="status-item">
            <span class="status-label">Model:</span>
            <span class="status-value" id="modelName">indent-scrible:3b-q4</span>
        </div>
        <div class="status-item">
            <span class="status-label">Autocomplete:</span>
            <span class="status-dot dot-on" id="autocompleteDot"></span>
            <span class="status-value" id="autocompleteStatus">ON</span>
        </div>
        <div class="status-item">
            <span class="status-label">Endpoint:</span>
            <span class="status-value" id="endpointUrl">localhost:11434</span>
        </div>
        <div class="status-item">
            <span class="status-label">Engine:</span>
            <span class="status-value">StarCoder2-3B Q4</span>
        </div>
    </div>
    <script>
        const vscode = acquireVsCodeApi();
        window.addEventListener('message', (event) => {
            const msg = event.data;
            if (msg.type === 'refresh') {
                document.getElementById('modelName').textContent = msg.model;
                document.getElementById('endpointUrl').textContent = msg.endpoint.replace('http://', '');
                const dot = document.getElementById('autocompleteDot');
                const status = document.getElementById('autocompleteStatus');
                if (msg.autocomplete) {
                    dot.className = 'status-dot dot-on';
                    status.textContent = 'ON';
                } else {
                    dot.className = 'status-dot dot-off';
                    status.textContent = 'OFF';
                }
            }
        });
    </script>
</body>
</html>`;
    }
}

module.exports = { ScribleStatusCenter };
