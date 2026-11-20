//! `kata`: a photo in, a transparent cutout and a screen-print sticker out.
//!
//! The work itself lives in `kata_core::pipeline`; this binary only parses
//! arguments, chooses a segmenter honestly, and writes files.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use kata_core::model::{default_for, model_path, ModelSpec};
use kata_core::pipeline;
use kata_core::segment::LumaSegmenter;

const USAGE: &str = "\
kata - remove a photo's background and turn it into a screen-print sticker

Usage:
  kata <input> [options]

Options:
  -o, --output <path>     Where to write the cutout PNG (default <stem>-cutout.png)
      --segmenter <kind>  auto (default), luma or onnx
      --model <path>      Override the ONNX model file path
      --sticker <path>    Also write a screen-print sticker PNG here
  -h, --help              Show this help
  -V, --version           Show the version
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    /// Use ONNX if this build has it; never pretend the lightness split is a model.
    Auto,
    /// The deliberate lightness split, silent.
    Luma,
    /// Require ONNX: an error if this build cannot run it.
    Onnx,
}

impl Choice {
    fn parse(text: &str) -> Result<Choice, String> {
        match text {
            "auto" => Ok(Choice::Auto),
            "luma" => Ok(Choice::Luma),
            "onnx" => Ok(Choice::Onnx),
            other => Err(format!(
                "`{other}` is not a segmenter; choose auto, luma or onnx"
            )),
        }
    }
}

struct Options {
    input: PathBuf,
    output: PathBuf,
    segmenter: Choice,
    model: Option<PathBuf>,
    sticker: Option<PathBuf>,
}

enum Action {
    Run(Options),
    Help,
    Version,
}

fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Ok(action) => match run(action) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("kata: {message}");
                ExitCode::from(2)
            }
        },
        Err(message) => {
            eprintln!("kata: {message}");
            ExitCode::from(2)
        }
    }
}

fn parse_args<I: Iterator<Item = String>>(args: I) -> Result<Action, String> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut segmenter = Choice::Auto;
    let mut model: Option<PathBuf> = None;
    let mut sticker: Option<PathBuf> = None;

    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            "-V" | "--version" => return Ok(Action::Version),
            "-o" | "--output" => output = Some(PathBuf::from(value(&mut args, &arg)?)),
            "--segmenter" => segmenter = Choice::parse(&value(&mut args, &arg)?)?,
            "--model" => model = Some(PathBuf::from(value(&mut args, &arg)?)),
            "--sticker" => sticker = Some(PathBuf::from(value(&mut args, &arg)?)),
            other if other.starts_with('-') => {
                return Err(format!("unknown option `{other}` (try --help)"));
            }
            other => {
                if input.is_some() {
                    return Err(format!(
                        "unexpected argument `{other}`; only one input image is accepted"
                    ));
                }
                input = Some(PathBuf::from(other));
            }
        }
    }

    let input = input.ok_or_else(|| "no input image given (try --help)".to_string())?;
    let output = output.unwrap_or_else(|| default_output(&input));
    Ok(Action::Run(Options {
        input,
        output,
        segmenter,
        model,
        sticker,
    }))
}

fn value<I: Iterator<Item = String>>(args: &mut I, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("`{option}` needs a value"))
}

/// `<stem>-cutout.png` next to the input.
fn default_output(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("image");
    let name = format!("{stem}-cutout.png");
    match input.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

fn run(action: Action) -> Result<(), String> {
    let options = match action {
        Action::Help => {
            print!("{USAGE}");
            return Ok(());
        }
        Action::Version => {
            println!("kata {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Action::Run(options) => options,
    };

    match options.segmenter {
        Choice::Onnx => {
            let (spec, path) = resolved_model(&options.model);
            return Err(onnx_unavailable(spec, &path));
        }
        Choice::Auto => {
            let (spec, path) = resolved_model(&options.model);
            if path.exists() {
                eprintln!(
                    "kata: found a model at {} but this build has no ONNX runtime \
                     (the `onnx` feature is off), so it cannot use it. Using the \
                     lightness fallback (--segmenter luma), which is not a matting model.",
                    path.display()
                );
            } else {
                eprintln!(
                    "kata: no model at {} and this build has no ONNX runtime, so the \
                     lightness fallback is being used (--segmenter luma). That fallback is \
                     not a matting model: it splits on lightness. To fix, put {} at that \
                     path or pass --model <path>.",
                    path.display(),
                    spec.file
                );
            }
        }
        Choice::Luma => {}
    }

    let bytes = std::fs::read(&options.input)
        .map_err(|error| format!("cannot read {}: {error}", options.input.display()))?;
    let segmenter = LumaSegmenter::default();

    let cutout = pipeline::cutout_png(&bytes, &segmenter).map_err(|error| error.to_string())?;
    write_and_report(&options.output, &cutout)?;

    if let Some(path) = &options.sticker {
        let sticker =
            pipeline::sticker_png(&bytes, &segmenter).map_err(|error| error.to_string())?;
        write_and_report(path, &sticker)?;
    }

    Ok(())
}

/// The model this build would use, and where it would look for it.
fn resolved_model(override_path: &Option<PathBuf>) -> (&'static ModelSpec, PathBuf) {
    let spec = default_for(false);
    let path = override_path.clone().unwrap_or_else(|| model_path(spec));
    (spec, path)
}

fn onnx_unavailable(spec: &ModelSpec, path: &Path) -> String {
    format!(
        "the ONNX segmenter is not available in this build: no ONNX runtime is compiled \
         in (the `onnx` feature is off).\n  model:    {} ({})\n  expected: {}\n  Put that \
         model file at the expected path and rebuild with the `onnx` feature, or rerun \
         with `--segmenter luma` to use the lightness fallback.",
        spec.name,
        spec.id,
        path.display()
    )
}

fn write_and_report(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
    }
    std::fs::write(path, bytes)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;

    let (width, height) = kata_core::decode_image(bytes)
        .map_err(|error| format!("the encoded image could not be read back: {error}"))?
        .dimensions();
    println!(
        "wrote {} ({} bytes, {}x{})",
        path.display(),
        bytes.len(),
        width,
        height
    );
    Ok(())
}
