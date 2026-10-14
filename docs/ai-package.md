# ai.ind — AI Assistant Package (like Python's `openai` SDK)

An **OpenAI-native** client for Indent. Talks to *any* OpenAI-compatible API —
works with **real OpenAI** *and* a **local Ollama** server (which exposes the
same OpenAI API at `/v1`). Chat completions, embeddings, model listing, cosine
similarity, and semantic search — all in pure Indent, no Python needed.

> **🛡️ Robust (v1.4)**: never crashes, and reports errors the way the OpenAI
> Python SDK does. Every call classifies the failure into a *typed* error
> (`authentication_error`, `rate_limit_error`, `connection_error`, …), keeps the
> provider's message/code/param, captures the `x-request-id` header, and retries
> **only retryable** failures (connection errors, 408/409/429, 5xx) with
> exponential backoff + jitter — honouring `Retry-After`. Callers still get
> `empty`/`[]` so nothing crashes, but the full reason is available via
> `LastError()` / `ErrorType()` / `IsRateLimit()` / `GetRequestId()`.

> Install: `air install ai` — import with `get ai as AI` (namespace, `AI.Chat`)
> or per-function: `get Chat from ai`.

```indent
get ai as AI
#! Local Ollama (default, no key):
var reply = AI.Chat("qwen2.5:0.5b", [{"role":"user","content":"What is 2+2?"}])
say reply

#! Real OpenAI — just point base + key:
AI.SetBase("https://api.openai.com/v1")
AI.SetApiKey("sk-...")               #! from platform.openai.com
var gpt = AI.Chat("gpt-4o-mini", [{"role":"user","content":"hi"}])
```

Under the hood it uses Indent's native `http_post_json` / `http_get` builtins.

---

## Configuration

| Function | Params | Description |
|---|---|---|
| `SetBase` | `url` | API base URL (default `http://localhost:11434/v1`). Use `https://api.openai.com/v1` for real OpenAI. |
| `SetApiKey` | `key` | API key → sends `Authorization: Bearer <key>`. Empty = no auth (local Ollama). |
| `SetDefaultModel` | `name` | Default chat model (default `qwen2.5:0.5b`). |
| `SetDefaultEmbedModel` | `name` | Default embedding model (default `nomic-embed-text`). |
| `GetBase` | — | Return the current base URL. |
| `SetRetries(n)` / `SetMaxRetries(n)` | `int` | Max retries for *retryable* failures (default 2; 0 disables). |
| `SetTimeout(seconds)` / `GetTimeout()` | `number` | Per-request timeout (default 30s). |
| `GetLastError()` | — | Last error **message** (`""` = success). |
| `GetLastStatus()` | — | Last HTTP status (0 = no response). |
| `GetRequestId()` | — | `x-request-id` of the last call (support tickets). |
| `WasError()` | — | True if the most recent call failed. |
| `LastError()` | dict | Full structured error object (see below). |
| `ErrorType()` | string | Category word, e.g. `rate_limit_error`. |
| `IsAuthError()` / `IsPermissionError()` / `IsBadRequest()` / `IsNotFound()` | boolean | Typed checks |
| `IsRateLimit()` / `IsServerError()` / `IsConnectionError()` / `IsTimeout()` | boolean | Typed checks |
| `ErrorTypeFor(status)` / `IsRetryable(status)` | — | Pure helpers (no state) for classification |
| `LastUsage()` / `LastFinishReason()` | dict / string | Token usage & finish reason of the last chat call |

### Namespace calling style — `AI.<Fn> …`

Everything above can also be reached through an `AI` namespace object, which is
the long-standing convention in older code (and in the Angela bot):

```indent
get AI from ai

AI.SetBase "https://ai.xytro.site/v1"
AI.SetApiKey key
AI.SetDefaultModel "xael-turbo"

var reply string = AI.Chat model messages
if reply == ""
    var why string = AI.GetLastError      #! human-readable text
    say "AI failed: " + why
```

`AI` is a plain dict of function references — there is **one** implementation,
two calling styles. `AI.GetLastError` returns the error **string** (that is what
legacy code expects); use `AI.LastError` for the structured dict and
`AI.LastErrorText` for the text form explicitly.


### Typed errors (like `openai.AuthenticationError`, `openai.RateLimitError`, …)

`LastError()` returns a dict:

```indent
{"ok": false, "type": "authentication_error", "status": 401,
 "code": "invalid_api_key", "message": "Invalid API key", "param": "",
 "request_id": "req_abc123", "body": "…", "provider": "openai"}
```

Branch on the failure instead of string-matching:

```indent
var reply = AI.Chat(model, msgs)
if AI.WasError()
    if AI.IsRateLimit()
        say "slow down"
    or AI.IsAuthError()
        say "bad key: " + AI.GetLastError()
    or AI.IsConnectionError()
        say "network down"
    otherwise
        say AI.ErrorType() + ": " + AI.GetLastError() + " (" + AI.GetRequestId() + ")"
```

The retry policy matches the OpenAI SDK: **connection errors, 408, 409, 429, and
5xx** are retried (with `Retry-After` respected and exponential backoff + jitter);
4xx client errors are returned immediately.

---

## Core API

| Function | OpenAI SDK equivalent | Description |
|---|---|---|
| `Chat(model, messages)` | `client.chat.completions.create()` | Chat completion; returns assistant reply text |
| `ChatFull(model, messages)` | `client.chat.completions.create()` | Returns the full **choice** dict (`message`, `finish_reason`, …) |
| `ChatWithSystem(model, system, user)` | — | System + user prompt in one call |
| `ChatWithOptions(model, messages, options)` | — | Any kwargs: `temperature`, `max_tokens`, `top_p`, `seed`, `json`, `tools`, … |
| `ChatWithSystem(model, systemPrompt, userPrompt)` | — | Chat with system instruction and user prompt directly in one call. |
| `ChatWithOptions(model, messages, options)` | — | Chat with custom parameters (`temperature`, `max_tokens`, `top_p`, `json`, `seed`). |
| `Ask(model, prompt)` | `client.chat.completions.create()` | Single-prompt completion; returns generated text. |
| `AskJson(model, prompt)` | — | Structured JSON completion: requests and parses response into an Indent dictionary. |
| `NewConversation([systemPrompt])` | — | Initialize conversation state dict. |
| `Converse(conv, model, userText)` | — | Multi-turn chat turn: returns `{"reply": text, "conv": updatedConv}`. |
| `NewVectorStore()` | — | In-memory semantic vector store for instant RAG. |
| `VectorStoreAdd(store, id, text[, metadata])` | — | Embed text and add item to vector store; returns updated store. |
| `VectorStoreSearch(store, query[, topK])` | — | Semantic search across store items; returns top ranked matches. |
| `Embed(model, text)` | `client.embeddings.create()` | Single text → embedding vector (list of floats). |
| `EmbedMany(model, texts)` | `client.embeddings.create()` | Batch: list of texts → list of vectors. |
| `Models()` | `client.models.list()` | List model IDs from the server. |
| `Similarity(a, b)` | — | Cosine similarity between two embeddings (0..1). |
| `Search(query, docs)` | — | Semantic search: rank `docs` by similarity to `query`, best-first. |

---

## Examples

### Chat with a system prompt (multi-turn)

A system prompt is just a `system`-role message, exactly like the Python SDK:

```indent
get ai as AI
var history = [{"role":"system","content":"You are a terse assistant."}]
history is history + [{"role":"user","content":"What is the capital of France?"}]
var answer = AI.Chat("qwen2.5:0.5b", history)
say answer
```

You can also skip the message plumbing entirely:

```indent
get ChatWithSystem from ai
var answer = ChatWithSystem("qwen2.5:0.5b", "You are terse.", "Capital of France?")
```

### High-level helpers (one call, no prompt plumbing)

These use the **default chat model** (`SetDefaultModel`), so you rarely type a
model name:

| Function | Description |
|---|---|
| `ChatDefault(messages)` | Chat using the default model |
| `AskDefault(prompt)` | Single-prompt using the default model |
| `ChatWithSystem(model, system, user)` | System + user prompt in one call |
| `ChatWithOptions(model, messages, options)` | Full control: `temperature`, `max_tokens`, `top_p`, `seed`, `json` |
| `AskJson(model, prompt)` | Ask for and parse a JSON object → dict |
| `Summarize(text[, n])` | Bullet-point summary (default 3 points) |
| `Translate(text, language)` | Translate text into a target language |
| `Classify(text, labels)` | Return the single best matching label |
| `Extract(text, fields)` | Pull structured fields → dict |

```indent
get Summarize, Translate, Classify, Extract from ai

say Summarize(article, 5)
say Translate("Good morning", "Japanese")
var cat = Classify("My order never arrived", ["billing", "shipping", "other"])
var fields = Extract("Ada, 36, London", ["name", "age", "city"])
```

### Conversations & agents (stateful)

```indent
get NewConversation, Converse from ai
var conv = NewConversation("You are a helpful coding assistant.")
var turn = Converse(conv, "qwen2.5:0.5b", "Write a haiku about Rust.")
say turn.reply
conv = turn.conv          #! carry the updated history forward
```

### Vector store + RAG (zero dependencies)

```indent
get NewVectorStore, VectorStoreAdd, RagAnswer from ai
var store = NewVectorStore()
store = VectorStoreAdd(store, "doc1", "Indent is a simple scripting language.")
store = VectorStoreAdd(store, "doc2", "Python is a general-purpose language.")
say RagAnswer(store, "What is Indent?", 2)
```

### Embeddings + similarity

```indent
get ai as AI
var v1 = AI.Embed("nomic-embed-text", "I love programming")
var v2 = AI.Embed("nomic-embed-text", "Coding is fun")
var v3 = AI.Embed("nomic-embed-text", "I enjoy pizza")
say AI.Similarity(v1, v2)   #! high (0.7+)
say AI.Similarity(v1, v3)   #! lower (0.5)
```

### Semantic search

```indent
get ai as AI
var docs = []
docs is docs + ["Python is a programming language"]
docs is docs + ["Rust is a systems language"]
docs is docs + ["Cats are small pets"]
var ranked = AI.Search("programming languages", docs)
repeat pair in ranked
    say string(pair[0]) + "  " + string(pair[1])   #! similarity + doc
```

---

## Notes

- **Local Ollama**: models like `qwen2.5:0.5b` (chat) and `nomic-embed-text`
  (768-dim embeddings) work out of the box, no key needed.
- **Real OpenAI**: `SetBase("https://api.openai.com/v1")` + `SetApiKey("sk-...")`.
  Model names like `gpt-4o-mini`, `text-embedding-3-small`.
- **Errors never crash the program.** `Chat`/`Ask` return `empty`, `Embed`/
  `EmbedMany`/`Models`/`Search` return `[]`, and `Similarity` returns `0` when a
  request fails. Check `WasError()` and read the typed error (`LastError()`,
  `ErrorType()`, `GetRequestId()`) — and let the automatic retry (`SetRetries n`)
  ride out transient blips (rate limits, model reloads, connection resets).

```indent
get ai as AI
var reply = AI.Chat("xael-nano", [{"role":"user","content":"hi"}])
if AI.WasError()
    say "AI call failed [" + AI.ErrorType() + "]: " + AI.GetLastError()
    if AI.GetRequestId() != ""
        say "  request id: " + AI.GetRequestId()
otherwise
    say reply
```

- See also: `examples/ai_openai_api.ind`, `examples/ai_pkg.ind`, and the
  low-level `http_get` / `http_post_json` / `http_put_json` / `http_patch_json`
  / `http_delete` builtins for calling any REST API directly.
