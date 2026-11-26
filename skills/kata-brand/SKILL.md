---
name: kata-brand
description: Build a Kata brand.json (inks with roles, fonts, logos and size rules, voice) from an app's theme, a logo and existing designs. Use when starting design work for a new project or when colours are being picked at random.
---

# A brand file Kata can use

Kata keeps everything brand-specific in one JSON file so the same tools and
skills work for any project. Build it from what already exists; do not invent.

## Where the colours come from

1. The product's own theme (CSS variables, a `theme.ts`, design tokens):
   the canvas, text, action colour.
2. The logo: `kata_palette` with `image` set to the transparent logo.
3. Designs that already worked (covers, ads): `kata_palette` on them.

Merge near-duplicates (delta E under 6, check with `kata_palette hex`).

## Give every ink a role

- `ground`: page backgrounds.
- `ink`: body text and the darkest print ink (a deep shade of the brand
  colour often beats black).
- `paper`: highlights, light type on dark grounds, outlines.
- `accent`: headline colours and print layers.
- `action`: the one call-to-action colour. Used once per design.

Run `kata_brand` on the file: it warns when an ink sits too close to the
ground.

## Example

See `examples/brand.example.json`. Fields:

```json
{
  "name": "Acme",
  "inks": [{"name": "cream", "hex": "#F9ECDD", "role": "paper"}],
  "fonts": {"files": {"Anton": "fonts/Anton-Regular.ttf"}, "display": "Anton", "body": "Sora", "mono": "IBM Plex Mono"},
  "logos": {"full": "logo.png", "mono_above_luma": 140, "min_width": 0.10, "max_width": 0.22},
  "voice": ["Speak like friends planning a weekend.", "Swahili and Sheng where people really use them."],
  "website": "example.com"
}
```

`logos.full` must have real transparency. Kata makes one-colour versions from
its alpha when the ground is light (luma above `mono_above_luma`).
