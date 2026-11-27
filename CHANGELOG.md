# Changelog

All notable changes to Kata. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-11-27

### Added

- A Windows installer, `kata-<version>-setup.exe`: installs for you alone into
  any folder (for example `D:\Kata`), adds Kata to the Start menu, the desktop
  and PATH, and offers to download the cutout engine at the end.
- `kata-app`, the windowed launcher the shortcuts open, so no console window
  appears.
- `kata-web --open` opens the page, reusing one already running.

### Changed

- A redesigned page: drop or paste a photo, preview the cutout on different
  backgrounds, switch between cutout and sticker, download.

## [0.1.0] - 2026-11-27

The first public release: from a photo to a reviewed poster, for people and agents.

### Added

- **Real segmentation.** BiRefNet through ONNX Runtime, loaded at run time, so
  one binary works with the runtime shipped beside it. fp16 and fp32 models,
  logit or probability outputs.
- **Hints.** A box around the subject (the model sees only that region) or
  points on it (keep only those pieces).
- **Mask cache.** Masks are cached by image bytes, model and hint; repeat cuts
  are instant.
- **Model downloads.** Resumable, size- and SHA-256-checked, MIT-licensed
  models only. Any BiRefNet-style `.onnx` file also works.
- **Mask edits.** `keep_largest`, `keep_at`, `drop_specks`, `fill_holes` (small
  holes only), `remove` / `add` polygons, `grow`, `shrink`, `feather`,
  `harden`, and photo-guided `matte` for hair.
- **Printing.** Tonal and spot (Lab, hue-weighted) modes, shadow-lifting prep,
  region rules (no paper ink on faces), a line plate for drawings, and
  two-layer scenes (a quiet background, a loud subject).
- **Brands.** `brand.json` with inks by role, fonts, logos with size rules and
  voice notes; palette extraction from logos and photos.
- **Paper.** Procedural, crease-free paper, and a detector for fold lines in
  scanned textures.
- **Composition.** JSON scenes rendered with resvg: rectangles, circles, images,
  one-colour logos, text (including vertical), star bursts, raw SVG, paper and
  halftone finish.
- **Review.** Type running into art, overlapping type, legibility against what
  is really behind each element, logo size, platform safe areas, ink against
  ground, mask islands and slivers, dark faces, texture creases. Each finding
  has a box and a saved crop.
- **Export targets.** Instagram square, 4:5 and story; TikTok; X; Open Graph; A4.
- **MCP server** (`kata mcp`): 13 tools, the skills as resources, a `poster`
  prompt, a path sandbox, and artifact ids.
- **Skills.** `kata-poster`, `kata-cutout`, `kata-print`, `kata-review` and
  `kata-brand`.
- **CLI subcommands.** `cut`, `print`, `compose`, `inspect`, `palette`,
  `texture`, `export`, `models`, `status` and `mcp`. `kata <photo>` still cuts.
- **Releases.** Windows x64, Linux x64 and arm64, and macOS arm64, each with
  ONNX Runtime 1.32.0 bundled.

### Changed

- Saved PNGs keep the full canvas; trimming is opt-in (`--trim`), so masks,
  prints and cutouts stay aligned with their photo.
- The local web UI uses the real model when one is installed.

[0.1.1]: https://github.com/ianclemence/kata/releases/tag/v0.1.1
[0.1.0]: https://github.com/ianclemence/kata/releases/tag/v0.1.0
