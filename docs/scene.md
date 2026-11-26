# Scenes

A scene is a JSON description of a poster. `kata compose scene.json -o out.png`
renders it; the MCP tool `kata_compose` takes the same object.

```json
{
  "width": 1080,
  "height": 1350,
  "background": "#F85647",
  "fonts": ["fonts/Anton-Regular.ttf"],
  "base_dir": ".",
  "layers": [ ... ],
  "finish": { "paper": 0.8, "seed": 7, "halftone": 0.1, "halftone_cell": 6.6 }
}
```

- `fonts`: TTF/OTF files to load; system fonts are loaded too. Refer to fonts by
  family name in text layers.
- `base_dir`: relative paths resolve against it (the CLI defaults it to the
  scene file's folder). In MCP, `src` may also be an artifact id.
- Layers draw in order; later layers are on top. Give each an `id`: the review
  names elements by id.
- `finish.paper` (0–1) overlays generated, crease-free paper; `finish.halftone`
  (0–1) multiplies a 15° dot screen.

## Layers

| `type` | Fields |
|---|---|
| `rect` | `x y w h fill`, `radius`, `opacity` |
| `circle` | `cx cy r`, `fill`, `stroke`, `stroke_width`, `dash` ("2 12" for a dotted ring) |
| `image` | `src x y w`, `h` (default keeps aspect), `opacity`, `clip_circle`, `rotate`, `blend` ("multiply", "overlay"…) |
| `logo` | `src x y w`, `color` (recolour the logo's alpha to one ink: use it on light or saturated grounds) |
| `text` | `text` ("\n" between lines), `x y font size color`, `weight`, `anchor` (start/middle/end), `letter_spacing`, `line_height` (× size, default 0.92), `uppercase`, `rotate`, `opacity`, `vertical` (one glyph per line, for spines and obi strips) |
| `burst` | `cx cy size fill`, `points` (18), `inner` (0.8), `text`, `color`, `font`, `rotate` |
| `svg` | `markup`: any SVG fragment (patterns, borders, rules) |

Text `y` is the baseline of the first line.

## The review

`kata compose` prints the review and the box of every element, saves a 2×
crop per finding next to the output (`out.review/`), and exits 1 if there is
an error. Checks:

- `text_collision`: type over the opaque pixels of a cutout, logo or sticker.
  Type over an opaque full-bleed photo is overprinting and is judged by
  legibility instead.
- `text_overlap`: text boxes overlapping each other.
- `legibility`: WCAG contrast of each text and logo against the colour actually
  behind it (3:1 for display type and logos, 4.5:1 for small text).
- `logo_size`: with `--brand`, logo width outside the brand's range.
- `safe_area`, `target_size`: with `--target`.
