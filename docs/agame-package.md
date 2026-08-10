# agame.ind — Merged into InGame (compatibility shim)

> **⚠️ agame is now merged into [`ingame`](ingame-package.md)** (v2.0). The
> `agame` package is kept only as a **compatibility shim** that re-exports the
> merged functions so old `get X from agame` code keeps working.
>
> **New code should use `ingame`:** `get Clamp, Lerp, NewEntity, MoveInMap,
> DrawTilemap, ... from ingame`

---

## Migration

| Old (`agame`) | New (`ingame`) |
|---|---|
| `get Clamp from agame` | `get Clamp from ingame` |
| `get NewEntity from agame` | `get NewEntity from ingame` |
| `get Collides from agame` | `get Collides from ingame` |
| `get TileToWorld from agame` | `get TileToWorld from ingame` |
| ... | ... |

All agame functions still exist in `ingame` (unchanged), **plus** the v2.0
game-dev additions: `MakeTilemap`/`DrawTilemap`, `MoveInMap`, `SetCamera`,
`DrawSprite`, `DrawEllipse`, `DrawArc`, `StepPhysics`, `IsKeyDown`, and more.

```indent
#! OLD code — still works:
get Clamp from agame
get NewEntity from agame
var health = Clamp(150, 0, 100)   # → 100

#! NEW code — use ingame:
get Clamp from ingame
get NewEntity from ingame
get MoveInMap from ingame
```

---

## Still available via agame (re-exported from ingame)

| Function | Description |
|---|---|
| `Clamp(value, low, high)` | Clamp `value` into `[low, high]`. |
| `Lerp(a, b, t)` | Linear interpolation: `a + (b - a) * t`. |
| `Distance(x1, y1, x2, y2)` | Euclidean distance. |
| `Wrap(value, min, max)` | Wrap into `[min, max)`. |
| `NewEntity(x, y, w, h)` | Entity dict `{"x","y","w","h"}`. |
| `Move(entity, dx, dy)` | Return entity shifted by `(dx, dy)`. |
| `Collides(a, b)` | AABB overlap test. |
| `StepPhysics(entity, g)` | Apply velocity + gravity. |
| `MoveInMap(entity, dx, dy, map, tileSize, legend)` | AABB tile collision. |
| `TileToWorld(tileX, tileY, tileSize)` | Tile coords → pixels. |
| `WorldToTile(x, y, tileSize)` | Pixels → tile coords. |
| `MakeTilemap(rows, cols)` | Create an empty tilemap. |
| `SetTile(map, col, row, id)` | Set a tile. |
| `GetTile(map, col, row)` | Get a tile. |
| `IsSolidAt(map, col, row, legend)` | Tile solidity check. |
| `DrawTilemap(map, tileSize, legend, screenW, screenH)` | Camera-culled tile drawing. |

---

## Why the merge?

`agame` was "Aether Game"; `ingame` is "Indent Game". Both served the same
purpose — building games in Indent — so v2.0 unifies them into one package.
`air install ingame` now provides everything (math, entities, tiles, camera,
sprites, physics, rendering, input). See the
[`ingame` guide](ingame-package.md) for the full API and examples.
