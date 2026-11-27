# Building Kata

## From source

Needs Rust 1.85 or newer.

```sh
cargo build --release -p kata-cli -p kata-web
python scripts/fetch-onnxruntime.py target/release   # ONNX Runtime beside the binary
```


## How it's put together

| Crate | What it is |
|---|---|
| `kata-core` | The algorithms: masks and edits, guided-filter matting, Lab colour, tonal and spot printing, line plates, paper, crease detection, inspections, export targets. No I/O; builds for WebAssembly. |
| `kata-engine` | ONNX Runtime (loaded at run time), checked model downloads, the mask cache, scene composition with resvg, and the review. |
| `kata-mcp` | The MCP server: tools, resources, prompts, path sandbox and artifact ids. |
| `kata-cli` | The `kata` binary. |
| `kata-web` | A local web page for quick cutouts, served on loopback only. |

Kata looks for ONNX Runtime in this order: `KATA_ORT_DYLIB`, `ORT_DYLIB_PATH`,
next to the `kata` binary, a `lib/` folder beside it, then Kata's data folder.
`kata status` prints every path it tried.

