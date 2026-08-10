# agame.ind — Starter 2D Game Helper API for Indent

A small, dependency-free helper library for 2D games written in Indent: math
helpers, simple game entities, AABB collision, and tile↔world conversion.
Pairs well with the [`ingame`](ingame-package.md) rendering framework.

> Install: `air install agame` — import with `get X from agame`.
> All functions are pure/stateless (entities are plain dicts) — no global state,
> nothing to initialize.

```indent
get Clamp from agame
get Lerp from agame
get Collides from agame
get NewEntity from agame

var player = NewEntity(10, 20, 32, 32)
var wall   = NewEntity(50, 50, 16, 16)
if Collides(player, wall)
    say "hit!"
```

---

## Math helpers

| Function | Params | Returns | Description |
|---|---|---|---|
| `Clamp` | `value, low, high` | number | Clamp `value` into `[low, high]`. |
| `Lerp` | `a, b, t` | number | Linear interpolation: `a + (b - a) * t` (t in 0..1). |
| `Distance` | `x1, y1, x2, y2` | number | Euclidean distance between two points. |
| `Wrap` | `value, min, max` | number | Wrap `value` into `[min, max)` (modular, supports negatives). |

```indent
var health = Clamp(150, 0, 100)     # → 100
var mid    = Lerp(0, 10, 0.5)       # → 5
var d      = Distance(0, 0, 3, 4)   # → 5
var angle  = Wrap(370, 0, 360)      # → 10
```

---

## Entities

Entities are simple dicts: `{"x", "y", "w", "h"}`.

| Function | Params | Returns | Description |
|---|---|---|---|
| `NewEntity` | `x, y, w, h` | entity dict | Create `{"x","y","w","h"}`. |
| `Move` | `entity, dx, dy` | entity | Return the entity shifted by `(dx, dy)` — **reassign** the result. |
| `Collides` | `a, b` | bool | AABB overlap test between two entities. |

```indent
var box = NewEntity(0, 0, 10, 10)
box is Move(box, 5, 5)              # now at (5,5)
var other = NewEntity(8, 8, 4, 4)
say Collides(box, other)            # → TRUE (overlap)
```

> `Collides` uses axis-aligned bounding boxes: overlap requires both
> `a.x < b.x + b.w and a.x + a.w > b.x` and the same for `y`.

---

## Tile math

For tile-based games (grid ↔ pixel conversion).

| Function | Params | Returns | Description |
|---|---|---|---|
| `TileToWorld` | `tileX, tileY, tileSize` | `{"x","y"}` | Tile coordinates → pixel position. |
| `WorldToTile` | `x, y, tileSize` | `{"x","y"}` | Pixel position → tile coordinates (integer division). |

```indent
var pos = TileToWorld(3, 2, 32)     # → {"x":96, "y":64}
var tile = WorldToTile(100, 70, 32) # → {"x":3, "y":2}
```

---

## Full example

```indent
get Clamp from agame
get Collides from agame
get NewEntity from agame
get Move from agame
get TileToWorld from agame

var tileSize = 32
var player = NewEntity(0, 0, 30, 30)
var wall   = NewEntity(TileToWorld(2, 0, tileSize).x, 0, 32, 32)

#! arrow-key movement clamped to a 10x8-tile world
player is Move(player, 1, 0)
player.x is Clamp(player.x, 0, 10 * tileSize - 30)

if Collides(player, wall)
    say "blocked by wall"
```

> **Note**: Indent passes arguments by value — `Move` returns the moved entity,
> so always reassign: `player is Move(player, dx, dy)`.
