# Hints and mask edits

## Hints (`kata cut --box`, `--point`; MCP `hint`)

```json
{"kind": "box", "x": 100, "y": 615, "w": 500, "h": 345, "margin": 0.08}
{"kind": "points", "points": [[360, 720]]}
```

- **box**: the model only sees the box (plus margin). In a busy scene this is
  the difference between a clean subject and half the street.
- **points**: segment everything, keep only the pieces under the points.

## Edits (`kata cut --ops edits.json`; MCP `kata_refine_mask`)

A JSON array, applied in order. Drop an item and re-apply to undo it.

```json
[
  {"op": "drop_specks", "min_area": 400},
  {"op": "fill_holes", "max_area": 300},
  {"op": "remove", "polygon": [[460,1276],[495,1280],[480,1360],[432,1414]]},
  {"op": "matte", "radius": 8, "epsilon": 0.0001},
  {"op": "feather", "px": 1.5}
]
```

| op | Fields | Use |
|---|---|---|
| `keep_largest` | | One subject, stray pieces elsewhere |
| `keep_at` | `points` | Keep the pieces under these points |
| `drop_specks` | `min_area` | Remove solid pieces smaller than this |
| `fill_holes` | `max_area` | Fill small holes only; gaps between limbs stay open |
| `remove` | `polygon` | Make inside transparent (furniture, railings) |
| `add` | `polygon` | Make inside opaque |
| `grow` / `shrink` | `px` | Move the edge out / in |
| `feather` | `px` | Soften a scissor-cut edge |
| `harden` | `low`, `high` | Steepen a soft edge |
| `matte` | `radius`, `epsilon` | Edge follows the photo (hair, fur); needs the image |

The model's raw mask is usually right. Edit what a finding or your own look
shows is wrong, and nothing else.
