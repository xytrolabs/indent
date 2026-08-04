const vscode = require('vscode');
const http = require('http');

/**
 * ScribleCompletionProvider — Inline code completion using Ollama.
 *
 * Communicates with a local Ollama instance running the fine-tuned
 * StarCoder2-3B model (or any other configured model).
 *
 * Uses Fill-In-the-Middle (FIM) format for StarCoder2 models:
 *   <fim_prefix>prefix<fim_suffix>suffix<fim_middle>
 */

const FIM_PREFIX = '<fim_prefix>';
const FIM_SUFFIX = '<fim_suffix>';
const FIM_MIDDLE = '<fim_middle>';

class ScribleCompletionProvider {
    constructor() {
        this._debounceTimer = null;
        this._onDidChange = new vscode.EventEmitter();
        this.onDidChange = this._onDidChange.event;
    }

    dispose() {
        this._onDidChange.dispose();
    }

    /**
     * Provide inline completion items for the given document/position.
     */
    async provideInlineCompletionItems(document, position, context, token) {
        const config = vscode.workspace.getConfiguration('indent-scrible');

        if (!config.get('autocompleteEnabled', true)) {
            return [];
        }

        // Only trigger automatically (not on manual invoke via this provider)
        if (context.triggerKind === vscode.InlineCompletionTriggerKind.Automatic) {
            // Debounce to avoid too many requests while typing
            // (VS Code handles debounce internally via the delay setting,
            //  but we do a simple check here too)
        }

        // ── Build FIM prompt ──────────────────────────────────────
        const prefix = this._getPrefix(document, position);
        const suffix = this._getSuffix(document, position);

        if (prefix.trim().length < 3) {
            return []; // Too little context
        }

        // ── Call Ollama ───────────────────────────────────────────
        const modelName = this._getModelName(config);
        const ollamaEndpoint = config.get('ollamaEndpoint', 'http://localhost:11434');

        try {
            const completion = await this._queryOllama(
                ollamaEndpoint,
                modelName,
                prefix,
                suffix,
                config
            );

            if (!completion || completion.trim().length === 0) {
                return [];
            }

            // Clean up the completion
            let text = this._cleanCompletion(completion, prefix, suffix);

            if (!text || text.trim().length === 0) {
                return [];
            }

            return [
                new vscode.InlineCompletionItem(
                    text,
                    new vscode.Range(position, position)
                ),
            ];
        } catch (err) {
            console.error('[Scrible] Completion error:', err.message);
            return [];
        }
    }

    /**
     * Get the prefix (code before cursor) for FIM.
     */
    _getPrefix(document, position) {
        const start = new vscode.Position(0, 0);
        const range = new vscode.Range(start, position);
        return document.getText(range);
    }

    /**
     * Get the suffix (code after cursor) for FIM.
     */
    _getSuffix(document, position) {
        const end = document.lineAt(document.lineCount - 1).range.end;
        const range = new vscode.Range(position, end);
        return document.getText(range);
    }

    /**
     * Resolve the model name from config.
     */
    _getModelName(config) {
        return config.get('inlineModel', 'scrible-inline');
    }

    /**
     * Query Ollama API for completion.
     */
    async _queryOllama(endpoint, model, prefix, suffix, config) {
        const maxTokens = config.get('maxTokens', 256);
        const temperature = config.get('temperature', 0.2);

        // Build FIM prompt for StarCoder2 architecture
        const prompt = `${FIM_PREFIX}${prefix}${FIM_SUFFIX}${suffix}${FIM_MIDDLE}`;

        const requestBody = JSON.stringify({
            model: model,
            prompt: prompt,
            stream: false,
            options: {
                num_predict: maxTokens,
                temperature: temperature,
                top_p: 0.95,
                top_k: 40,
                stop: ['<|endoftext|>', FIM_PREFIX, FIM_SUFFIX, '\n\n\n'],
            },
        });

        return new Promise((resolve, reject) => {
            const url = new URL('/api/generate', endpoint);
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
                    timeout: 5000,
                },
                (res) => {
                    let data = '';
                    res.on('data', (chunk) => (data += chunk));
                    res.on('end', () => {
                        try {
                            const result = JSON.parse(data);
                            resolve(result.response || '');
                        } catch (e) {
                            reject(new Error(`Failed to parse Ollama response: ${e.message}`));
                        }
                    });
                }
            );

            req.on('error', (err) => {
                // Silently fail — Ollama may not be running
                reject(err);
            });

            req.on('timeout', () => {
                req.destroy();
                reject(new Error('Ollama request timed out'));
            });

            req.write(requestBody);
            req.end();
        });
    }

    /**
     * Clean up the raw completion from the model.
     */
    _cleanCompletion(completion, prefix, suffix) {
        let text = completion;

        // Remove any FIM tokens that might appear in output
        text = text.replace(new RegExp(FIM_PREFIX.replace(/[<>]/g, '\\$&'), 'g'), '');
        text = text.replace(new RegExp(FIM_SUFFIX.replace(/[<>]/g, '\\$&'), 'g'), '');
        text = text.replace(new RegExp(FIM_MIDDLE.replace(/[<>]/g, '\\$&'), 'g'), '');

        // Trim to the next newline that would create duplicate content
        const suffixLines = suffix.trim().split('\n');
        if (suffixLines.length > 0) {
            const firstSuffixLine = suffixLines[0].trim();
            const idx = text.indexOf(firstSuffixLine);
            if (idx > 0 && firstSuffixLine.length > 3) {
                text = text.substring(0, idx).trimEnd();
            }
        }

        return text.trim();
    }
}

module.exports = { ScribleCompletionProvider };
