# ingame.ind — PyGame-Style 2D Game Framework for Indent

**InGame** mirrors [PyGame's API](https://www.pygame.org/docs/) so you can write
games **entirely in Indent** — movement, physics, collision, scoring, and
rendering all live in Indent code. A native WebKitGTK canvas window
(`indent-ingame`) just draws the frames you build and reports input back.

> Install: `air install ingame` — import with `get ingame as IG` (namespace,
> `IG.DrawRect(...)`) or per-function: `get DrawRect from ingame`.

```indent
get Init from ingame
get SetMode from ingame
get DrawRect from ingame
get DrawCircle from ingame
get Flip from ingame
get GetEvents from ingame
get Quit from ingame

Init()
var win = SetMode(400, 400, "My Game")   #! display.set_mode → spawns window
repeat while running
    repeat e in GetEvents()              #! event.get
        if e["type"] == "quit"
            running is false
        if e["type"] == "keydown"
            #! e["key"] is e.g. "ArrowLeft"
    DrawRect 10 10 50 50 "#39d353"       #! draw.rect
    DrawCircle 200 200 20 "#f85149"      #! draw.circle
    Flip "#000000"                       #! display.flip — flush the frame
    Tick 60                              #! time.Clock.tick — ~60 fps
Quit()
```

See `examples/snake_game.ind` and `examples/breakout_game.ind` for complete,
playable games written 100% in Indent.

---

## How it works

- `SetMode` creates an IPC folder `/tmp/ingame-<uuid>/` and spawns the native
  `indent-ingame` window in the background. The window polls `frame.json`.
- Your loop calls `Draw*` to queue shapes, then `Flip(clearColor)` writes the
  whole frame (`{"clear", "shapes"}`) to `frame.json` and clears the queue.
- The window writes input to `events.txt`, `keys.txt`, and `mouse.txt`, which
  `GetEvents` / `GetKeys` / `GetMouse` read (and clear).

Requires the `indent-ingame` native helper (built by `install.sh`; needs
`gcc`, `gtk3`, `webkit2gtk`).

---

## API reference

### Setup & lifecycle

| Function | PyGame equivalent | Description |
|---|---|---|
| `Init()` | `pygame.init()` | Initialize (state is lazy; no-op). |
| `SetMode(w, h, title)` | `pygame.display.set_mode()` | Spawn the native window, prep IPC files, return the workdir path. |
| `SetCaption(title)` | `pygame.display.set_caption()` | Window title (title is already set at `SetMode`; kept for compatibility). |
| `Quit()` | `pygame.quit()` | Close the window and exit. |

### Drawing (queue shapes; flushed on `Flip`)

| Function | PyGame equivalent | Description |
|---|---|---|
| `DrawRect(x, y, w, h, color)` | `pygame.draw.rect()` | Rectangle. |
| `DrawCircle(cx, cy, r, color)` | `pygame.draw.circle()` | Circle. |
| `DrawLine(x1, y1, x2, y2, color, w)` | `pygame.draw.line()` | Line segment with width. |
| `DrawPolygon(points, color)` | `pygame.draw.polygon()` | Polygon; `points` = list of `[x, y]`. |
| `DrawText(x, y, text, color, size)` | `pygame.font` | Text. |
| `Flip(clear)` | `pygame.display.flip()` | Flush all queued shapes to the window and reset the queue. `clear` is the background color. |

### Input

| Function | PyGame equivalent | Description |
|---|---|---|
| `GetEvents()` | `pygame.event.get()` | Read **and clear** input events. Each event has a `"type"` — see below. |
| `GetKeys()` | `pygame.key.get_pressed()` | List of currently held key names. |
| `GetMouse()` | `pygame.mouse.get_pos()` | `[x, y]` cursor position. |

### Timing

| Function | PyGame equivalent | Description |
|---|---|---|
| `Tick(fps)` | `pygame.time.Clock.tick()` | Sleep to target frame rate. |

---

## Event types

`GetEvents()` returns a list of dicts; **every** event has a `"type"` key:

| Type | Extra keys | Meaning |
|---|---|---|
| `quit` | — | Window closed. |
| `keydown` | `key`, `down: true` | A key was pressed. |
| `keyup` | `key`, `down: false` | A key was released. |
| `mousemove` | `x`, `y` | Cursor moved. |
| `mousedown` | `x`, `y`, `button` | Mouse button pressed. |
| `mouseup` | `x`, `y`, `button` | Mouse button released. |

`key` is the key name, e.g. `"ArrowLeft"`, `"ArrowRight"`, `"ArrowUp"`,
`"ArrowDown"`, `" "`, `"Enter"`, letters, etc.

```indent
repeat e in GetEvents()
    if e["type"] == "quit"
        running is false
    if e["type"] == "keydown" and e["key"] == "ArrowLeft"
        px is px - 20
    if e["type"] == "mousedown"
        say "clicked at " + string(e["x"]) + "," + string(e["y"])
```

---

## Compatibility aliases

The older snake-era names still work (they map to the new API):

| Alias | Maps to |
|---|---|
| `Clear(color)` | reset the queue (draw nothing this frame) |
| `Rect(x,y,w,h,color)` | `DrawRect` |
| `Circle(cx,cy,r,color)` | `DrawCircle` |
| `Line(x1,y1,x2,y2,color,w)` | `DrawLine` |
| `Polygon(points,color)` | `DrawPolygon` |
| `Text(x,y,text,color,size)` | `DrawText` |
| `Present(clear)` | `Flip` |
| `Events()` | `GetEvents` |
| `Keys()` | `GetKeys` |
| `Mouse()` | `GetMouse` |

---

## Mini game skeleton

```indent
get Init from ingame
get SetMode from ingame
get DrawRect from ingame
get DrawCircle from ingame
get DrawText from ingame
get Flip from ingame
get GetEvents from ingame
get Quit from ingame
get Clamp from agame          #! optional: math helpers

Init()
SetMode(500, 400, "Paddle")
var px = 250
var running = true

repeat while running
    repeat e in GetEvents()
        if e["type"] == "quit"
            running is false
        if e["type"] == "keydown" and e["key"] == "ArrowLeft"
            px is px - 25
        if e["type"] == "keydown" and e["key"] == "ArrowRight"
            px is px + 25

    DrawRect(px, 380, 80, 10, "#58a6ff")     #! paddle
    DrawCircle(250, 200, 8, "#f85149")       #! ball
    DrawText(8, 12, "Score: 0", "#ffffff", 14)

    Flip("#0d1117")
    time_sleep 0.016
Quit()
```

> **Gotcha**: the IPC loop is file-based and runs the window as a separate
> process, so keep frames simple and call `Flip` once per loop iteration.
