# ai.ind — AI Assistant Package (like Python's `openai` SDK)

An **OpenAI-native** client for Indent. Talks to *any* OpenAI-compatible API —
works with **real OpenAI** *and* a **local Ollama** server (which exposes the
same OpenAI API at `/v1`). Chat completions, embeddings, model listing, cosine
similarity, and semantic search — all in pure Indent, no Python needed.

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

---

## Core API

| Function | OpenAI SDK equivalent | Description |
|---|---|---|
| `Chat(model, messages)` | `client.chat.completions.create()` | Chat completion; `messages` = list of `{"role","content"}`; returns assistant reply text. |
| `Ask(model, prompt)` | `client.chat.completions.create()` | Single-prompt completion; returns generated text. |
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

### Embeddings + similarity

```indent
get ai as AI
var v1 = AI.Embed("nomic-embed-text", "I love programming")
var v2 = AI.Embed("nomic-embed-text", "Coding is fun")
var v3 = AI.Embed("nomic-embed-text", "I enjoy pizza")
say AI.Similarity(v1, v2)   # high (0.7+)
say AI.Similarity(v1, v3)   # lower (0.5)
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
- **Errors**: on a bad request the underlying HTTP call still returns
  `{"ok","status","body"}`; parse the JSON body for the API error message.
- See also: `examples/ai_openai_api.ind`, `examples/ai_pkg.ind`, and the
  low-level `http_get` / `http_post_json` / `http_put_json` / `http_patch_json`
  / `http_delete` builtins for calling any REST API directly.
