# Print specs

`kata print photo.jpg --spec spec.json [--mask m.png] [--background-spec bg.json] [--ground "#hex"]`
and the MCP tool `kata_print` take the same spec.

```json
{
  "mode": "tonal",
  "inks": ["#2A1308", "#A6391C", "#F4703A", "#FFF3DE"],
  "prep": {"gamma": 0.72, "clahe": 40},
  "smooth": 50,
  "hue_weight": 2.5,
  "despeckle": 1,
  "regions": [
    {"polygon": [[270,285],[465,285],[465,520],[270,520]], "forbid": ["#FFF3DE"], "replace_with": "#F4703A"}
  ],
  "line": {"ink": "#2A1308", "threshold": 40, "width": 7}
}
```

| Field | Meaning |
|---|---|
| `mode` | `tonal`: inks by lightness (2–5 inks). `spot`: nearest ink by colour (2–16 inks). |
| `inks` | Hex colours. Tonal sorts them dark to light itself. |
| `prep.gamma` | Below 1 lifts shadows. 0.7–0.75 for dark or backlit faces. |
| `prep.clahe` | Local contrast 0–100. |
| `smooth` | Edge-preserving smoothing 0–100. |
| `hue_weight` | Spot only: how much hue outweighs lightness. |
| `despeckle` | 0, 1 or 2: removes isolated specks of ink. |
| `regions` | Inside each polygon, swap `forbid` inks for `replace_with`. |
| `line` | A stroke plate in one ink: pencil, pen, wire. |

With `--mask`, only the subject is printed (the rest is transparent) and cut
points come from the subject alone. With `--background-spec` as well, the whole
scene is printed with that spec and the subject is printed on top with `spec`.

`--ground` reports any part of the subject printed within delta E 10 of the
poster's ground colour, the tone that will vanish.
