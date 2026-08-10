# ingame.ind — PyGame-Style 2D Game Framework for Indent

**InGame** ("Indent Game") mirrors [PyGame's API](https://www.pygame.org/docs/)
so you can write games **entirely in Indent** — movement, physics, collision,
AI, tilemaps, and rendering all live in Indent code. A native WebKitGTK canvas
window (`indent-ingame`) just draws the frames you build and reports input back.

> Install: `air install ingame` — import with `get ingame as IG` (namespace,
> `IG.DrawRect(...)`) or per-function: `get DrawRect from ingame`.

## 🆕 v2.0 — agame merged in + game-dev APIs

v2.0 **merges the old `agame` package** ("Aether Game") into InGame and adds
APIs for building RPGs, tile worlds, and action games:

- **Math/entities/tiles from agame**: `Clamp`, `Lerp`, `Distance`, `Wrap`,
  `NewEntity`, `Move`, `Collides`, `TileToWorld`, `WorldToTile`
- **Camera**: `SetCamera`, `GetCamera`, `ScreenX`, `ScreenY` — scroll a world
  larger than the screen (camera-follow)
- **Tilemap**: `MakeTilemap`, `SetTile`, `GetTile`, `IsSolidAt`, `DrawTilemap`
  — grid worlds with viewport culling (only visible tiles are drawn)
- **Sprites**: `DrawSprite` — render emoji/glyph sprites (zero asset files)
- **New shapes**: `DrawEllipse`, `DrawArc`, `DrawRectRot` (rotated rects)
- **Physics**: `StepPhysics`, `MoveInMap` — AABB tile collision with
  per-axis resolution (returns `hitX`/`hitY` flags)
- **Input helper**: `IsKeyDown`

**`agame` still works** (it now re-exports from ingame), but new code should
`get X from ingame`.

```indent
get Init from ingame
get SetMode from ingame
get DrawTilemap from ingame
get DrawSprite from ingame
get NewEntity from ingame
get MoveInMap from ingame
get SetCamera from ingame
get Flip from ingame
get GetEvents from ingame
get Quit from ingame

Init()
var win = SetMode(720, 480, "My Game")
repeat while running
    repeat e in GetEvents()
        if e["type"] == "quit"
            running is false
    DrawTilemap world 24 legend 720 480   #! draw visible tiles (camera-aware)
    DrawSprite px py 24 24 "🧙"           #! player sprite
    Flip "#000000"
    Tick 60
Quit()
```

See `examples/rpg_demo.ind` for a complete tilemap RPG, and
`examples/snake_game.ind` / `examples/breakout_game.ind` for classic games.

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
| `SetCaption(title)` | `pygame.display.set_caption()` | Window title (title is already set at `SetMode`). |
| `Quit()` | `pygame.quit()` | Close the window and exit. |

### Drawing (queued; flushed on `Flip`)

| Function | PyGame equivalent | Description |
|---|---|---|
| `DrawRect(x, y, w, h, color)` | `pygame.draw.rect()` | Rectangle. |
| `DrawRectRot(x, y, w, h, color, rot)` | `pygame.transform.rotate()` | Rectangle rotated `rot` degrees (clockwise, around center). |
| `DrawCircle(cx, cy, r, color)` | `pygame.draw.circle()` | Circle. |
| `DrawEllipse(cx, cy, rx, ry, color)` | `pygame.draw.ellipse()` | Ellipse with horizontal/vertical radii. |
| `DrawArc(cx, cy, r, a1, a2, color)` | `pygame.draw.arc()` | Pie slice from `a1°` to `a2°` (filled). Great for health/cooldown wheels. |
| `DrawLine(x1, y1, x2, y2, color, w)` | `pygame.draw.line()` | Line segment with width. |
| `DrawPolygon(points, color)` | `pygame.draw.polygon()` | Polygon; `points` = list of `[x, y]`. |
| `DrawSprite(x, y, w, h, glyph)` | `pygame.image` / text | Emoji/glyph sprite centered in a box — sprites without asset files. |
| `DrawText(x, y, text, color, size)` | `pygame.font` | Text. |
| `Flip(clear)` | `pygame.display.flip()` | Flush all queued shapes to the window and reset. `clear` = background color. |

### Camera (world scrolling)

| Function | Description |
|---|---|
| `SetCamera(x, y)` | Move the camera so world `(x, y)` appears at screen `(0,0)`. |
| `GetCamera()` | `[x, y]` current camera position. |
| `ScreenX(wx)` / `ScreenY(wy)` | World coords → screen coords (subtract camera). |

```indent
#! camera follows the player, clamped to the world
var camX = player.x - W/2
var camY = player.y - H/2
camX is Clamp(camX, 0, COLS*TILE - W)
camY is Clamp(camY, 0, ROWS*TILE - H)
SetCamera(camX, camY)
```

### Tilemaps

| Function | Description |
|---|---|
| `MakeTilemap(rows, cols)` | Empty map: list of rows of `0` (empty). |
| `TileWidth(map)` / `TileHeight(map)` | Map dimensions. |
| `SetTile(map, col, row, id)` | Set a tile (returns the map — reassign). |
| `GetTile(map, col, row)` | Get a tile id (`0` out of bounds). |
| `IsSolidAt(map, col, row, legend)` | True if that tile is marked solid. |
| `DrawTilemap(map, tileSize, legend, screenW, screenH)` | Draw visible tiles (camera-culled). |
| `TileToWorld(tileX, tileY, tileSize)` | Tile coords → pixel `{"x","y"}`. |
| `WorldToTile(x, y, tileSize)` | Pixel → tile `{"x","y"}`. |

**Legend** (string keys, e.g. `"1"`, `"2"`):
```indent
var legend = {}
legend["1"] is {"c": "2d6a2d"}                  #! grass tile (walkable color)
legend["2"] is {"solid": true, "s": "🌲"}        #! solid emoji tile (tree)
legend["3"] is {"solid": true, "s": "🪨"}        #! solid emoji tile (rock)
legend["4"] is {"s": "🌼"}                       #! walkable emoji tile (flower)
```
- `"c"` = hex color (drawn as a rect)
- `"s"` = emoji/glyph (drawn as a sprite)
- `"solid": true` makes the tile block movement (everything else is walkable)

`DrawTilemap` only draws tiles within the camera viewport, so huge worlds stay
fast.

### Entities & physics

| Function | Description |
|---|---|
| `NewEntity(x, y, w, h)` | Entity dict `{"x","y","w","h"}` (add `"vx"`, `"vy"` for physics). |
| `Move(entity, dx, dy)` | Return entity shifted by `(dx, dy)` — reassign. |
| `Collides(a, b)` | AABB overlap test. |
| `StepPhysics(entity, g)` | Apply `vx`/`vy`, add gravity `g` to `vy` — returns entity. |
| `MoveInMap(entity, dx, dy, map, tileSize, legend)` | Move with AABB collision against the tilemap. Returns entity with `hitX`/`hitY` flags. |

```indent
#! move + collide (per-axis), returns the moved entity
var m = MoveInMap(player, dx * speed, dy * speed, world, 24, legend)
player is m
if m.hitX == true
    say "blocked on X axis!"
```

`MoveInMap` resolves X and Y separately (so you can wall-slide), and sets
`hitX` / `hitY` booleans on the returned entity.

### Input

| Function | PyGame equivalent | Description |
|---|---|---|
| `GetEvents()` | `pygame.event.get()` | Read **and clear** input events (normalized `"type"`). |
| `GetKeys()` | `pygame.key.get_pressed()` | List of currently held key names. |
| `IsKeyDown(key)` | — | Convenience: is a key currently held? |
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

Key names: `"ArrowLeft"`, `"ArrowRight"`, `"ArrowUp"`, `"ArrowDown"`, `" "`,
`"Enter"`, `"Escape"`, `"w"`, `"a"`, `"s"`, `"d"`, etc.

---

## Compatibility aliases

The older snake-era names still work (they map to the new API):

| Alias | Maps to |
|---|---|
| `Clear(color)` | reset the queue |
| `Rect` / `Circle` / `Line` / `Polygon` / `Text` | `DrawRect` / `DrawCircle` / `DrawLine` / `DrawPolygon` / `DrawText` |
| `Sprite` / `Ellipse` | `DrawSprite` / `DrawEllipse` |
| `Present(clear)` | `Flip` |
| `Events()` / `Keys()` / `Mouse()` | `GetEvents` / `GetKeys` / `GetMouse` |

---

## Mini game skeleton (tilemap RPG)

```indent
get Init, SetMode, DrawTilemap, DrawSprite, DrawText, Flip, GetEvents, Quit from ingame
get NewEntity, MoveInMap, SetCamera, Clamp, MakeTilemap, SetTile from ingame

Init()
SetMode(720, 480, "Adventure")

var legend = {}
legend["1"] is {"c": "2d6a2d"}
legend["2"] is {"solid": true, "s": "🌲"}
var world = MakeTilemap(40, 60)
#! ... fill the world with SetTile ...

var player = NewEntity(122, 122, 20, 20)
var running = true
repeat while running
    repeat e in GetEvents()
        if e["type"] == "quit"
            running is false
    var dx = 0
    var dy = 0
    if IsKeyDown("d") or IsKeyDown("ArrowRight")
        dx is 1
    if IsKeyDown("a") or IsKeyDown("ArrowLeft")
        dx is -1
    if IsKeyDown("s") or IsKeyDown("ArrowDown")
        dy is 1
    if IsKeyDown("w") or IsKeyDown("ArrowUp")
        dy is -1
    player is MoveInMap(player, dx * 2, dy * 2, world, 24, legend)
    SetCamera(Clamp(player.x - 360, 0, 60*24 - 720), Clamp(player.y - 240, 0, 40*24 - 480))
    DrawTilemap(world, 24, legend, 720, 480)
    DrawSprite(player.x - 2, player.y - 2, 24, 24, "🧙")
    DrawText(10, 12, "Adventure", "#ffffff", 14)
    Flip("#10231a")
    Tick(60)
Quit()
```

> **Gotcha**: the IPC loop is file-based and runs the window as a separate
> process. Keep frames reasonably small and call `Flip` once per loop iteration.
> For huge tilemaps, `DrawTilemap` already culls to the visible viewport.
