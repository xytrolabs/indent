#!/usr/bin/env python3
"""Local fixture server for the Indent networking / AI parity tests.

Serves, on one port:

  * HTTP endpoints that exercise the semantics the `http_*` builtins must get
    right — methods, JSON round-trips, headers, auth, status codes, redirects,
    timeouts, gzip, big bodies, 204, and non-UTF-8 text.
  * A WebSocket echo endpoint (`/ws`) for `ws_connect`/`ws_send_text`/`ws_recv_text`.
  * An OpenAI-compatible mock (`/v1/chat/completions`, `/v1/embeddings`,
    `/v1/models`) so `std/ai.ind` can be tested end-to-end without a provider,
    including every error class the package claims to classify, retry-after
    handling, flakiness (fail N times then succeed) and a slow model for timeouts.

Everything is deterministic and offline, so it is safe for CI. Usage:

    python3 tools/mock_fixture_server.py [port]     # default 8799
"""
import asyncio
import gzip
import json
import os
import sys
import time

from aiohttp import web

FAIL_TIMES = {}          # model -> how many times it has been asked
DEFAULT_PORT = 8799


async def get_json(request):
    return web.json_response({
        "method": "GET",
        "path": request.path,
        "args": dict(request.query),
        "headers": {k.lower(): v for k, v in request.headers.items()},
    })


async def echo_json(request):
    raw = await request.text()
    try:
        parsed = json.loads(raw) if raw else None
    except json.JSONDecodeError as e:
        return web.json_response({"error": "invalid json", "detail": str(e)}, status=400)
    return web.json_response({
        "method": request.method,
        "content_type": request.headers.get("Content-Type", ""),
        "authorization": request.headers.get("Authorization", ""),
        "json": parsed,
        "bytes": len(raw),
    })


async def raw_echo(request):
    body = await request.read()
    return web.Response(body=body, content_type="application/octet-stream")


async def status(request):
    code = int(request.match_info["code"])
    payload = {"status": code, "path": request.path}
    if code == 429:
        return web.json_response(payload, status=429, headers={"Retry-After": "2"})
    return web.json_response(payload, status=code)


async def redirect(request):
    n = int(request.match_info.get("n", "1"))
    target = "/get" if n <= 1 else f"/redirect/{n - 1}"
    raise web.HTTPFound(target)


async def slow(request):
    ms = int(request.query.get("ms", "1500"))
    await asyncio.sleep(ms / 1000.0)
    return web.json_response({"slept_ms": ms})


async def large(request):
    n = int(request.query.get("n", "1000"))
    return web.json_response({"items": [{"i": i, "s": f"row-{i}"} for i in range(n)]})


async def gzipped(request):
    payload = json.dumps({"gzipped": True, "note": "x" * 200}).encode()
    return web.Response(
        body=gzip.compress(payload),
        content_type="application/json",
        headers={"Content-Encoding": "gzip"},
    )


async def latin1(request):
    # NB: pass the header explicitly — aiohttp rejects `content_type=` with a
    # charset of its own and answers 500.
    return web.Response(
        body="café naïve".encode("latin-1"),
        headers={"Content-Type": "text/plain; charset=iso-8859-1"},
    )


async def no_content(request):
    return web.Response(status=204)


async def ws_echo(request):
    ws = web.WebSocketResponse()
    await ws.prepare(request)
    await ws.send_str("hello")
    async for msg in ws:
        if msg.type == web.WSMsgType.TEXT:
            if msg.data == "close":
                await ws.close()
                break
            if msg.data == "silence":
                continue                      # for ws_recv_text_timeout
            await ws.send_str("echo:" + msg.data)
        elif msg.type == web.WSMsgType.ERROR:
            break
    return ws


# ── OpenAI-compatible mock ────────────────────────────────────────────────

def _completion(model, text="ok"):
    return {
        "id": "chatcmpl-mock-1",
        "object": "chat.completion",
        "created": int(time.time()),
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": "stop",
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7},
    }


ERROR_MODELS = {
    "err-401": (401, "invalid_api_key", "Incorrect API key provided"),
    "err-403": (403, "permission_denied", "You do not have access to this model"),
    "err-400": (400, "invalid_request_error", "Bad request payload"),
    "err-404": (404, "model_not_found", "The model does not exist"),
    "err-429": (429, "rate_limit_exceeded", "Rate limit reached"),
    "err-500": (500, "server_error", "The server had an error"),
}


async def chat_completions(request):
    body = await request.json()
    model = body.get("model", "mock-model")
    messages = body.get("messages", [])
    prompt = ""
    if messages:
        last = messages[-1]
        prompt = last.get("content", "") if isinstance(last, dict) else str(last)

    if model == "err-429-retry":
        # Succeeds on the 3rd attempt, to exercise retry+backoff.
        FAIL_TIMES[model] = FAIL_TIMES.get(model, 0) + 1
        if FAIL_TIMES[model] < 3:
            return web.json_response(
                {"error": {"message": "slow down", "type": "rate_limit_exceeded"}},
                status=429, headers={"Retry-After": "1"})
        return web.json_response(_completion(model, "recovered"))

    if model.startswith("err-") and model in ERROR_MODELS:
        code, etype, msg = ERROR_MODELS[model]
        return web.json_response({"error": {"message": msg, "type": etype, "code": etype}}, status=code)

    if model == "err-conn-drop":
        # Abort without a response, so the client sees a connection error.
        request.transport.abort()
        return web.Response(status=500)

    if model == "slow-model":
        await asyncio.sleep(2.5)
        return web.json_response(_completion(model, "late"))

    echo = f"echo:{prompt}" if prompt else "ok"
    return web.json_response(_completion(model, echo))


async def embeddings(request):
    body = await request.json()
    model = body.get("model", "mock-embed")
    inp = body.get("input", "")
    items = inp if isinstance(inp, list) else [inp]
    if model in ERROR_MODELS:
        code, etype, msg = ERROR_MODELS[model]
        return web.json_response({"error": {"message": msg, "type": etype}}, status=code)
    data = [{"object": "embedding", "index": i, "embedding": [float(len(str(t))), 0.5, -1.25]}
            for i, t in enumerate(items)]
    return web.json_response({
        "object": "list",
        "data": data,
        "model": model,
        "usage": {"prompt_tokens": 3, "total_tokens": 3},
    })


async def models(request):
    if request.headers.get("Authorization", "").strip() == "Bearer bad":
        return web.json_response({"error": {"message": "bad key", "type": "invalid_api_key"}}, status=401)
    return web.json_response({"object": "list", "data": [
        {"id": "mock-model", "object": "model"},
        {"id": "mock-embed", "object": "model"},
    ]})


def main(port):
    app = web.Application()
    app.add_routes([
        web.get("/get", get_json),
        web.post("/post", echo_json),
        web.put("/put", echo_json),
        web.patch("/patch", echo_json),
        web.delete("/delete", echo_json),
        web.post("/raw", raw_echo),
        web.get("/status/{code}", status),
        web.get("/redirect/{n}", redirect),
        web.get("/slow", slow),
        web.get("/large", large),
        web.get("/gzip", gzipped),
        web.get("/latin1", latin1),
        web.get("/no-content", no_content),
        web.get("/ws", ws_echo),
        web.post("/v1/chat/completions", chat_completions),
        web.post("/v1/embeddings", embeddings),
        web.get("/v1/models", models),
        web.get("/health", lambda r: web.json_response({"ok": True})),
    ])
    print(f"fixture listening on http://127.0.0.1:{port}", flush=True)
    web.run_app(app, host="127.0.0.1", port=port, print=None, access_log=None)


if __name__ == "__main__":
    main(int(sys.argv[1]) if len(sys.argv) > 1 else int(os.environ.get("FIXTURE_PORT", DEFAULT_PORT)))
