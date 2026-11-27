<p align="center"><img src="assets/kata-256.png" width="128" alt="Kata"></p>

# Kata

**Cut, print, compose and review: an on-device design toolkit for people and agents.**

Kata takes a photo and gives you a clean cutout of its subject, prints it in
your brand's inks like a screen print, lays it out as a poster from a JSON
scene, and reviews the result for the mistakes people notice: type running into
the picture, a logo that sinks into its background, faces printed black, an
elbow printed in the ground colour so it vanishes, a scanned paper texture
putting a fold line through everything.

It runs on your machine and nothing is uploaded. The same engine works three
ways: a command-line tool, a local web page, and an
[MCP](https://modelcontextprotocol.io) server. Claude, Cursor or any MCP client
can drive the whole workflow, and the design judgement ships with it as
[skills](skills/).

*Kata* is Swahili for "cut".

```
photo ─► cut out ─► touch up ─► print in brand inks ─► lay out ─► review ─► export
                     (undoable)   (tonal / spot / line)                (collisions, legibility,
                                                                        logo size, safe areas)
```

## Install

**Windows:** download `kata-<version>-setup.exe` from
[Releases](https://github.com/ianclemence/kata/releases) and run it. It installs
for you alone (no administrator password), into any folder you choose (`D:\Kata`
works), adds **Kata** to the Start menu, the desktop and your PATH, and offers
to download the cutout engine at the end.

**macOS and Linux:** download the archive for your platform, unpack it, and put
the folder on your `PATH`. Then fetch the cutout engine once (Kata checks the
download before using it):

```sh
kata models download birefnet-lite-fp32     # 214 MB, fast on any computer
kata models list                            # sharper options
kata status                                 # what Kata found
```


## Command line

```sh
# A cutout. In a busy street, tell it where the subject is.
kata cut matatu.jpg --box 100,615,500,345 -o matatu-cut.png --mask matatu-mask.png

# Print it in brand inks: the street quiet, the matatu loud.
kata print matatu.jpg --mask matatu-mask.png \
  --spec examples/print-subject.json --background-spec examples/print-street.json \
  --ground "#F9ECDD" -o matatu-print.png

# Lay out a poster and review it. Exits with code 1 if the review finds errors.
kata compose examples/scene.example.json -o poster.png --target ig-portrait

# Checks and helpers
kata inspect --mask matatu-mask.png
kata palette logo.png
kata texture -o paper.png --size 1080x1350
kata texture --check scanned-paper.png
kata export poster.png --targets ig-square,ig-story,x-post
```

The first cut of a photo runs the model. After that the mask comes from a cache
keyed by the image's bytes, the model and the hint, so iterating on a design is
instant.

## Agents (MCP)

```sh
claude mcp add kata -- kata mcp --allow "$PWD"
```

For Claude Desktop, Cursor and other clients, see [docs/mcp.md](docs/mcp.md).

There are 13 tools: `kata_status`, `kata_models_download`, `kata_segment`,
`kata_refine_mask`, `kata_cutout`, `kata_print`, `kata_palette`, `kata_brand`,
`kata_texture`, `kata_compose`, `kata_inspect`, `kata_view` and `kata_export`.

- Every output gets a short id (`art_3_cutout`) that later calls can use
  instead of a path.
- `kata_view` lets the agent zoom into any region, boost contrast to find
  hidden lines, or show transparency on a checkerboard. Looking closely is the
  step that matters most in design.
- The five skills are also served as resources, along with an example brand
  file and scene. There is one prompt, `poster`.

## Skills

[`skills/`](skills/) holds the workflow as
[Agent Skills](https://docs.claude.com/en/docs/agents-and-tools/agent-skills):

- **kata-poster**: from brief to posting plan. One concept per poster, copy in
  the audience's own words, layout, logo rules, series and cadence.
- **kata-cutout**: trust the model's mask, and edit only what is wrong.
- **kata-print**: choosing inks by role, tonal or spot, faces that stay readable.
- **kata-review**: every check, what it means, and what to look at yourself.
- **kata-brand**: building `brand.json` from an app's theme, a logo and past designs.

To use them without the MCP server, copy them into `.claude/skills/` (or your
agent's skills folder). Brand specifics live in a brand file, not in the
skills, so one Kata serves every project. See
[examples/brand.example.json](examples/brand.example.json).

## Documentation

- [docs/mcp.md](docs/mcp.md): connecting agents, the tools, files and safety
- [docs/scene.md](docs/scene.md): the poster scene format and the review
- [docs/mask-ops.md](docs/mask-ops.md): hints and mask edits
- [docs/print.md](docs/print.md): print specs
- [docs/building.md](docs/building.md): building Kata yourself

## Privacy and safety

- Images never leave your machine. The only network access is downloading a
  engine you ask for, and that download is checked before it is used.
- The MCP server reads and writes only inside its work folder and the folders
  you pass with `--allow`.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
