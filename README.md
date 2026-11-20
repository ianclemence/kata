# Kata

Kata takes a photo's background away and writes the cutout as a PNG with a real
transparent channel. On request it also renders the cutout as a screen-print
sticker: flat ink colours, a paper-coloured border and an outline so it still
reads when printed. `kata-core` does the work; the `kata` CLI and a small local
web app both call it. Nothing leaves your computer.

## Requirements

Rust 1.82 or newer (`rust-version = "1.82"` in the workspace); no model ships.

## CLI

Build with `npm run build:cli` (or `cargo build --package kata-cli --release`);
the binary is `target/release/kata` (`kata.exe` on Windows).

```
kata <input> [options]

  -o, --output <path>     Where to write the cutout PNG (default <stem>-cutout.png)
      --segmenter <kind>  auto (default), luma or onnx
      --model <path>      Override the ONNX model file path
      --sticker <path>    Also write a screen-print sticker PNG here
  -h, --help              Show this help
  -V, --version           Show the version
```

Copy-pasteable example:

```sh
cargo run --release --package kata-cli -- photo.jpg -o photo-cutout.png --sticker photo-sticker.png
```

`auto` uses ONNX if this build can run it and otherwise falls back to the
lightness split, saying so on stderr. `luma` chooses the lightness split silently.
`onnx` refuses to guess: it exits with code 2 and prints the model it wanted and
the path it looked for, so a missing runtime is never mistaken for a result.

The cutout is a PNG; the CLI prints `wrote <path> (<n> bytes, <w>x<h>)`, reading
the file back for those dimensions.

## Browser UI

Start the local server with `npm run web` (that runs
`cargo run --package kata-web --release`). It prints
`Kata web UI on http://127.0.0.1:8765` and listens on loopback only. Open that
address. Pass `--port <PORT>` or set `KATA_WEB_PORT` to use another port.

The page has:

- **Dropzone / file picker** — PNG, JPEG, WebP, BMP or TIFF up to 64 MB, picked
  or dropped. The cutout is requested as soon as a file is chosen.
- **Use sample image** — fetches a synthetic 480×360 picture from the server so
  you can try the pipeline without a file of your own.
- **Screen-print sticker** — checkbox. On: requests the sticker and re-renders
  the current photo; off: the plain cutout.
- **Result panel** — before and after on a checkerboard, plus PNG size, pixel
  dimensions and fully transparent pixel count, measured from the bytes the
  browser received.
- **Save the PNG** — downloads the output as `kata-cutout.png` or
  `kata-sticker.png`.

Core errors come back as text and are shown above the result; a malformed image
gives a readable message rather than a blank page.

## Tests and release build

```sh
npm run test          # cargo test --workspace
npm run build         # cargo build --workspace --release
```

The same by cargo directly:

```sh
cargo test --workspace
cargo build --workspace --release
```

Also: `npm run check`, `npm run fmt`, `npm run fmt:check`, `npm run clippy`. Every
package script is a cargo command; none changes the machine.

## What is not done in this build

- **No ONNX inference is compiled in.** The `onnx` feature is off by default, and
  the `ort` dependency it would pull in is configured with `download-binaries`,
  which fetches runtime binaries at build time. This build has no matting runtime.
- **No model ships.** The registry in `kata-core/src/model.rs` records the BiRefNet
  files, sizes and SHA-256 checksums, but the files are not in the tree.
- **The default path is the lightness split, not matting.** `--segmenter auto`
  (the default) and `luma` split the image on lightness and keep the darker
  subject. That is a threshold: it can clip a light subject, keep a dark
  background, and gives no soft hair edges. The CLI and the page both say so.
- **`--segmenter onnx` cannot succeed here.** It reports the missing runtime and
  expected model path instead of panicking; `--model` only changes where that path
  is looked up.
- **There is no WebAssembly build.** The browser UI is a local native server, not
  code compiled to wasm; the page talks to it over HTTP on 127.0.0.1.
