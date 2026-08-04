/* ── Scrible Chat Panel — Client Logic ──────────────────────────── */

(function () {
    const vscode = acquireVsCodeApi();

    // ── DOM elements ──────────────────────────────────────────────
    const messagesEl = document.getElementById('messages');
    const chatInput = document.getElementById('chatInput');
    const sendBtn = document.getElementById('sendBtn');
    const typingEl = document.getElementById('typingIndicator');
    const modelBadge = document.getElementById('modelBadge');

    // ── State ─────────────────────────────────────────────────────
    let isWaiting = false;

    // ── Send message ──────────────────────────────────────────────
    function sendMessage(text) {
        if (!text || !text.trim()) return;
        if (isWaiting) return;

        const trimmed = text.trim();
        chatInput.value = '';
        chatInput.style.height = 'auto';

        // Add user bubble
        addMessage('user', trimmed);
        isWaiting = true;

        // Notify extension
        vscode.postMessage({ type: 'send', text: trimmed });

        // Scroll to bottom
        scrollToBottom();
    }

    // ── Add a message bubble ──────────────────────────────────────
    function addMessage(role, text) {
        // Remove welcome message if present
        const welcome = messagesEl.querySelector('.scrible-welcome');
        if (welcome) {
            welcome.remove();
        }

        const msgDiv = document.createElement('div');
        msgDiv.className = `chat-message ${role}`;

        const label = document.createElement('span');
        label.className = 'chat-label';
        label.textContent = role === 'user' ? 'You' : 'Scrible';

        const bubble = document.createElement('div');
        bubble.className = 'chat-bubble';

        // Render markdown-like content
        bubble.innerHTML = renderContent(text);

        // Add code action buttons for bot messages with code blocks
        if (role === 'bot') {
            const codeBlocks = bubble.querySelectorAll('pre code');
            if (codeBlocks.length > 0) {
                const actions = document.createElement('div');
                actions.className = 'code-actions';

                const insertBtn = document.createElement('button');
                insertBtn.className = 'code-action-btn';
                insertBtn.textContent = '$(insert) Insert';
                insertBtn.addEventListener('click', () => {
                    const code = codeBlocks[0].textContent || '';
                    vscode.postMessage({ type: 'insertCode', code });
                });

                const copyBtn = document.createElement('button');
                copyBtn.className = 'code-action-btn';
                copyBtn.textContent = '$(copy) Copy';
                copyBtn.addEventListener('click', () => {
                    const code = codeBlocks[0].textContent || '';
                    vscode.postMessage({ type: 'copyCode', code });
                });

                actions.appendChild(insertBtn);
                actions.appendChild(copyBtn);
                bubble.appendChild(actions);
            }
        }

        msgDiv.appendChild(label);
        msgDiv.appendChild(bubble);
        messagesEl.appendChild(msgDiv);

        scrollToBottom();
    }

    // ── Render markdown-like content ──────────────────────────────
    function renderContent(text) {
        // Escape HTML first
        let html = escapeHtml(text);

        // Code blocks: ```indent ... ```
        html = html.replace(/```(\w*)\s*\n?([\s\S]*?)```/g, (_, lang, code) => {
            const langLabel = lang ? ` <small>(${lang})</small>` : '';
            return `<pre><code>${code.trim()}${langLabel}</code></pre>`;
        });

        // Inline code: `code`
        html = html.replace(/`([^`]+)`/g, '<code>$1</code>');

        // Bold: **text**
        html = html.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');

        // Italic: *text*
        html = html.replace(/\*([^*]+)\*/g, '<em>$1</em>');

        // Inline links: [text](url)
        html = html.replace(
            /\[([^\]]+)\]\(([^)]+)\)/g,
            '<a href="$2" title="$2">$1</a>'
        );

        // Line breaks
        html = html.replace(/\n/g, '<br>');

        return html;
    }

    function escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    // ── Scroll to bottom ──────────────────────────────────────────
    function scrollToBottom() {
        messagesEl.scrollTop = messagesEl.scrollHeight;
    }

    // ── Auto-resize textarea ──────────────────────────────────────
    chatInput.addEventListener('input', () => {
        chatInput.style.height = 'auto';
        chatInput.style.height = Math.min(chatInput.scrollHeight, 120) + 'px';
    });

    // ── Enter to send, Shift+Enter for newline ────────────────────
    chatInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            sendMessage(chatInput.value);
        }
    });

    // ── Send button ───────────────────────────────────────────────
    sendBtn.addEventListener('click', () => {
        sendMessage(chatInput.value);
    });

    // ── Quick action buttons ──────────────────────────────────────
    document.addEventListener('click', (e) => {
        const btn = e.target.closest('.quick-btn');
        if (!btn) return;

        const action = btn.dataset.action;
        if (action) {
            // Get selected code from the editor if available
            chatInput.value = action;
            sendMessage(action);
        }
    });

    // ── Handle messages from extension ────────────────────────────
    window.addEventListener('message', (event) => {
        const msg = event.data;

        switch (msg.type) {
            case 'userMessage':
                // Programmatic message from a command
                sendMessage(msg.text);
                break;

            case 'response':
                addMessage('bot', msg.text);
                isWaiting = false;
                break;

            case 'typing':
                typingEl.style.display = msg.active ? 'flex' : 'none';
                if (msg.active) scrollToBottom();
                break;

            case 'status':
                modelBadge.textContent = msg.model;
                break;
        }
    });

    // ── Notify extension that webview is ready ────────────────────
    vscode.postMessage({ type: 'ready' });

    console.log('[Scrible Webview] Ready.');
})();
