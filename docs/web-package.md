# The `web` package

> **Since:** Indent 2.1
>
> `web` is a small first-party package that turns the native
> [`http_serve`](builtins-reference.md) builtin into a dynamic web app
> toolkit — including the ability to **run Indent code that a browser POSTs
> to you**, and return its output as JSON. That's the missing piece behind a
> "Run" button on a page served by Indent.

## Loading

```indent
get RunCode from web
get Json from web
```

## What's inside

| Function | Purpose |
|----------|---------|
| `RunCode code` | Writes `code` to a temp file, runs it in a **fresh `indent` process**, removes the temp file, and returns `{ok, status, stdout, stderr}`. |
| `Html body` | Response dict → `200`, `text/html`. |
| `Json obj` | Response dict → `200`, `application/json` (JSON-encodes `obj`). |
| `Text body` | Response dict → `200`, `text/plain`. |
| `Send status body content_type` | Response dict with an explicit status and content type. |

`Html`, `Json`, `Text` and `Send` return the dict shape `http_serve`
already understands, so you can pass them straight back from your handler.

## A code-running server (the Run button)

```indent
get RunCode from web
get Json from web

fun handle req
    if req.path == "/run"
        var result = RunCode req.body
        give Json result
    give {"status": 404, "body": "not found", "content_type": "text/plain"}

http_serve handle 8080
```

Then a browser page (or `curl`) can POST Indent source and get JSON back:

```
curl -X POST --data-binary 'var x is 6
say "answer: " + string(x * 7)' http://127.0.0.1:8080/run
=> {"ok":true,"status":0,"stderr":"","stdout":"answer: 42\n"}
```

Each request gets its own stateless handler run; `RunCode` spins up an
isolated `indent` process per call, so misbehaving or crashing code can never
take down the server. A full, runnable example lives at
`examples/web_run.ind`.

## Notes & gotchas

- Each `http_serve` request is handled in a fresh runtime — module-level
  variables are **not** shared across requests.
- `RunCode` shells out to the `indent` binary, which must be on `PATH`.
- Keep handler logic small; block while the request is being answered.
