# Kata over MCP

`kata mcp` speaks the Model Context Protocol over stdio (JSON-RPC 2.0, one
message per line). Protocol versions 2025-06-18, 2025-03-26 and 2024-11-05 are
accepted.

## Connect a client

Claude Code:

```sh
claude mcp add kata -- kata mcp --allow "$PWD"
```

Claude Desktop (`claude_desktop_config.json`), Cursor (`.cursor/mcp.json`) and
most other clients:

```json
{
  "mcpServers": {
    "kata": {
      "command": "kata",
      "args": ["mcp", "--allow", "/Users/you/Pictures", "--allow", "/Users/you/projects"],
      "env": { "KATA_MODEL": "birefnet-lite-fp32" }
    }
  }
}
```

On Windows use full paths, e.g. `"command": "C:\\tools\\kata\\kata.exe"`.

## Files and safety

- `--work <dir>`: where outputs go (default: Kata's data folder `/work`).
- `--allow <dir>`: a folder inputs may be read from and outputs written to.
  Repeatable. With none, the current directory is allowed.
- Every output gets an id, `art_<n>_<kind>`, valid for the session. Pass the id
  wherever a tool takes an image, mask or file.
- Pictures come back as previews (768 px); full-size files stay on disk.
  `kata_view` returns detail up to 1400 px for close inspection.
- Tool failures come back as results with `isError: true` and a message that
  says what to do, so the agent can recover.

## Tools

| Tool | Purpose |
|---|---|
| `kata_status` | Runtime, models, folders, targets. Call first. |
| `kata_models_download` | Download and verify a registry model. |
| `kata_segment` | Raw mask + cutout + leftover findings. `hint`: `box` or `points`. Cached. |
| `kata_refine_mask` | Ordered, explicit mask edits ([mask-ops.md](mask-ops.md)). |
| `kata_cutout` | Mask applied as alpha; optional trim. |
| `kata_print` | Tonal / spot print, prep, region rules, line plate, two-layer scene, ground check ([print.md](print.md)). |
| `kata_palette` | Extract a palette, or compare colours (contrast, delta E). |
| `kata_brand` | Validate and summarise a brand file. |
| `kata_texture` | Generate paper, or check a texture for creases. |
| `kata_compose` | Render a scene ([scene.md](scene.md)) and review it. |
| `kata_inspect` | Individual checks. |
| `kata_view` | Crop, zoom, boost, checkerboard. |
| `kata_export` | Platform sizes with crop report. |

## Resources and prompts

- `kata://skills/kata-poster`, `kata-cutout`, `kata-print`, `kata-review`, `kata-brand`
- `kata://examples/brand`, `kata://examples/scene`, `kata://targets`
- Prompt `poster` (arguments: `photo`, `brand`, `message`, `target`)

## A typical session

1. `kata_status` → download a model if needed.
2. Read `kata://skills/kata-poster`.
3. `kata_brand` with the project's brand file.
4. `kata_segment` the photo (box hint in busy scenes), `kata_view` the cutout.
5. `kata_print` with brand inks and the poster's `ground`.
6. `kata_compose` with the target; fix every error in the review.
7. `kata_view` the logo, headline edges and subject outline at zoom 2, and the
   whole poster with `boost`.
8. `kata_export` if other sizes are needed.
