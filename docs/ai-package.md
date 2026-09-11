# ai.ind — AI Assistant Package (like Python's `openai` SDK)

An **OpenAI-native** client for Indent. Talks to *any* OpenAI-compatible API —
works with **real OpenAI** *and* a **local Ollama** server (which exposes the
same OpenAI API at `/v1`). Chat completions, embeddings, model listing, cosine
similarity, and semantic search — all in pure Indent, no Python needed.

> **🛡️ Robust (v1.2)**: never crashes on transient server errors. Every call
> checks the HTTP response, parses JSON defensively, guards indexing, retries
> with backoff, and returns `empty`/`[]` gracefully on failure instead of
> throwing. Inspect failures with `GetLastError()` / `GetLastStatus()` /
> `WasError()`, or configure retries with `SetRetries(n)`.
>
> **🧠 System prompts (v1.3)**: set a default system prompt once with
> `SetSystemPrompt("...")` and it is automatically prepended to every `Chat`/
> `Ask`. Or pass one per call with `ChatSystem` / `AskSystem`. Build a system
> message with `System("...")`.

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
| `SetSystemPrompt` | `text` | Default system prompt, auto-prepended to every `Chat`/`Ask` (empty = none). |
| `GetSystemPrompt` | — | Return the current default system prompt. |
| `ClearSystemPrompt` | — | Remove the default system prompt. |
| `GetBase` | — | Return the current base URL. |
| `SetRetries(n)` | `int` | Retry a failed request up to `n` times with backoff (default 2; 0 disables). |
| `GetLastError()` | — | Last error message (`""` = success). |
| `GetLastStatus()` | — | Last HTTP status (0 = no response). |
| `WasError()` | — | True if the most recent call failed. |

---

## Core API

| Function | OpenAI SDK equivalent | Description |
|---|---|---|
| `Chat(model, messages)` | `client.chat.completions.create()` | Chat completion; `messages` = list of `{"role","content"}`; returns assistant reply text. |
| `Ask(model, prompt)` | `client.chat.completions.create()` | Single-prompt completion; returns generated text. |
| `ChatSystem(model, system, messages)` | `client.chat.completions.create()` | Chat with an explicit one-off system prompt (overrides the default). |
| `AskSystem(model, system, prompt)` | `client.chat.completions.create()` | Single-prompt completion with an explicit system prompt. |
| `System(text)` | — | Build a `{"role":"system","content":text}` message dict. |
| `Embed(model, text)` | `client.embeddings.create()` | Single text → embedding vector (list of floats). |
| `EmbedMany(model, texts)` | `client.embeddings.create()` | Batch: list of texts → list of vectors. |
| `Models()` | `client.models.list()` | List model IDs from the server. |
| `Similarity(a, b)` | — | Cosine similarity between two embeddings (0..1). |
| `Search(query, docs)` | — | Semantic search: rank `docs` by similarity to `query`, best-first. |

---

## Examples

### Chat (multi-turn)

```indent
get ai as AI
var history = [{"role":"system","content":"You are a terse assistant."}]
history is history + [{"role":"user","content":"What is the capital of France?"}]
var answer = AI.Chat("qwen2.5:0.5b", history)
say answer
```

### System prompts

```indent
get ai as AI

#! Set a default — applies to every Chat/Ask from now on:
AI.SetSystemPrompt("You are a terse assistant. Answer in one sentence.")
say AI.Ask("qwen2.5:0.5b", "What is the capital of France?")

#! Or use one just for a single call (overrides the default):
var msgs = [{"role":"user","content":"Explain recursion."}]
say AI.ChatSystem("qwen2.5:0.5b", "Explain like I'm five.", msgs)

#! Build a system message yourself (e.g. to keep a persistent history):
var history = [AI.System("You are a pirate.")]
history is history + [{"role":"user","content":"Ahoy?"}]
say AI.Chat("qwen2.5:0.5b", history)

#! Turn the default off again:
AI.ClearSystemPrompt()
```

The default system prompt is prepended only when the `messages` list does not
already begin with a `system` message — so a system message you pass yourself
always wins (no duplicates).

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
  request fails. Check `WasError()` / `GetLastError()` to see what happened, or
  rely on the automatic retry (`SetRetries n`) to ride out transient blips
  (rate limits, model reloads, connection resets).

```indent
get ai as AI
var reply = AI.Chat("xael-nano", [{"role":"user","content":"hi"}])
if AI.WasError()
    say "AI call failed: " + AI.GetLastError() + " (status " + string(AI.GetLastStatus()) + ")"
otherwise
    say reply
```

- See also: `examples/ai_openai_api.ind`, `examples/ai_pkg.ind`, and the
  low-level `http_get` / `http_post_json` / `http_put_json` / `http_patch_json`
  / `http_delete` builtins for calling any REST API directly.
