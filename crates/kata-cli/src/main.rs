//! `kata`: cut subjects out of photos, print them in brand inks, compose and
//! review posters, and serve all of it to agents over MCP.
//!
//! The work lives in `kata-core` (algorithms) and `kata-engine` (models,
//! runtime, composition). This binary parses arguments and writes files.

use kata_core::brand::{extract_palette, Brand};
use kata_core::color::Rgba;
use kata_core::inspect;
use kata_core::mask::Mask;
use kata_core::ops::{self, Hint, MaskOp};
use kata_core::print::{self, PrintSpec};
use kata_core::style::StickerStyle;
use kata_core::{model, sticker, targets, texture};
use kata_engine::compose::{self, Scene};
use kata_engine::cut::{self, CutOptions};
use kata_engine::{models, review, runtime};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
kata - cut, print, compose and review: an on-device design toolkit for people and agents

Usage:
  kata <image> [cut options]            Cut out the subject (same as `kata cut`)
  kata cut <image> [options]            Transparent cutout, mask and sticker
  kata print <image> --spec <json> [options]
  kata compose <scene.json> -o <png> [--target <id>] [--brand <json>]
  kata inspect [--mask <png>] [--layer <png> --ground <hex>] [--texture <png>]
  kata palette <image> [--count <n>]
  kata texture -o <png> [--size <w>x<h>] [--seed <n>]   |   kata texture --check <png>
  kata export <image> --targets <id,id> [-d <dir>]
  kata models [list | download <id>]
  kata status
  kata mcp [--work <dir>] [--allow <dir>]...   Serve the tools over MCP (stdio)

Cut options:
  -o, --output <path>     Cutout PNG (default <stem>-cutout.png)
      --mask <path>       Also write the mask as a greyscale PNG
      --sticker <path>    Also write a screen-print sticker PNG
      --segmenter <kind>  auto (default), onnx or luma
      --model <id|path>   Registry id or a .onnx file (default: KATA_MODEL, then any downloaded model)
      --box <x,y,w,h>     The subject is inside this box (busy scenes)
      --point <x,y>       Keep only the piece under this point (repeatable)
      --ops <json>        Mask edits to apply, a JSON array (see docs/mask-ops.md)
      --trim              Crop transparent margins
      --fresh             Ignore the mask cache

Print options:
      --spec <json>             Print spec file (mode, inks, prep, regions, line)
      --mask <png>              Print only the subject
      --background-spec <json>  With --mask: print the scene quietly behind the subject
      --ground <hex>            Report parts of the subject printed in the ground colour
  -o, --output <path>

  -h, --help     Show this help
  -V, --version  Show the version

Environment: KATA_MODEL (model id or path), KATA_ORT_DYLIB (ONNX Runtime library), KATA_HOME (data folder).
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("kata: {message}");
            ExitCode::from(2)
        }
    }
}

/// A tiny argument reader: flags with values, repeatable flags, positionals.
struct Args {
    items: Vec<String>,
}

impl Args {
    fn new(items: &[String]) -> Args {
        Args {
            items: items.to_vec(),
        }
    }

    fn flag(&mut self, names: &[&str]) -> bool {
        if let Some(i) = self.items.iter().position(|a| names.contains(&a.as_str())) {
            self.items.remove(i);
            true
        } else {
            false
        }
    }

    fn value(&mut self, names: &[&str]) -> Result<Option<String>, String> {
        let Some(i) = self.items.iter().position(|a| names.contains(&a.as_str())) else {
            return Ok(None);
        };
        if i + 1 >= self.items.len() {
            return Err(format!("`{}` needs a value", self.items[i]));
        }
        let v = self.items.remove(i + 1);
        self.items.remove(i);
        Ok(Some(v))
    }

    fn values(&mut self, names: &[&str]) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        while let Some(v) = self.value(names)? {
            out.push(v);
        }
        Ok(out)
    }

    /// The remaining positionals; any leftover `-x` is an error.
    fn positionals(self, expected: usize) -> Result<Vec<String>, String> {
        if let Some(unknown) = self
            .items
            .iter()
            .find(|a| a.starts_with('-') && a.len() > 1)
        {
            return Err(format!("unknown option `{unknown}` (try --help)"));
        }
        if self.items.len() > expected {
            return Err(format!("unexpected argument `{}`", self.items[expected]));
        }
        Ok(self.items)
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{path} is not valid: {e}"))
}

fn save(image: &image::RgbaImage, path: &Path) -> Result<(), String> {
    let bytes = kata_engine::save_png(image, path).map_err(|e| e.to_string())?;
    let (w, h) = image.dimensions();
    println!("wrote {} ({bytes} bytes, {w}x{h})", path.display());
    Ok(())
}

fn load_mask(path: &str) -> Result<Mask, String> {
    let gray = image::open(path)
        .map_err(|e| format!("cannot open mask {path}: {e}"))?
        .to_luma8();
    let (w, h) = gray.dimensions();
    Ok(Mask {
        width: w,
        height: h,
        data: gray.into_raw(),
    })
}

fn numbers(text: &str, count: usize, what: &str) -> Result<Vec<u32>, String> {
    let parts: Vec<u32> = text
        .split(',')
        .map(|p| p.trim().parse::<u32>())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("{what} must be {count} whole numbers separated by commas"))?;
    if parts.len() != count {
        return Err(format!(
            "{what} must be {count} numbers, got {}",
            parts.len()
        ));
    }
    Ok(parts)
}

fn print_findings(findings: &[inspect::Finding]) -> ExitCode {
    let review = review::Review::new(findings.to_vec());
    println!(
        "{}",
        serde_json::to_string_pretty(&review).unwrap_or_default()
    );
    if review.errors > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn run(all: &[String]) -> Result<ExitCode, String> {
    let Some(first) = all.first() else {
        print!("{USAGE}");
        return Ok(ExitCode::from(2));
    };
    let rest = &all[1..];
    match first.as_str() {
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        "-V" | "--version" => {
            println!("kata {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        "cut" => cut_command(Args::new(rest)),
        "print" => print_command(Args::new(rest)),
        "compose" => compose_command(Args::new(rest)),
        "inspect" => inspect_command(Args::new(rest)),
        "palette" => palette_command(Args::new(rest)),
        "texture" => texture_command(Args::new(rest)),
        "export" => export_command(Args::new(rest)),
        "models" => models_command(rest),
        "status" => status_command(),
        "mcp" => mcp_command(Args::new(rest)),
        _ => cut_command(Args::new(all)),
    }
}

#[derive(PartialEq)]
enum Choice {
    Auto,
    Onnx,
    Luma,
}

fn cut_command(mut a: Args) -> Result<ExitCode, String> {
    if a.flag(&["-h", "--help"]) {
        print!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    let output = a.value(&["-o", "--output"])?;
    let mask_out = a.value(&["--mask"])?;
    let sticker_out = a.value(&["--sticker"])?;
    let segmenter = match a.value(&["--segmenter"])?.as_deref() {
        None | Some("auto") => Choice::Auto,
        Some("onnx") => Choice::Onnx,
        Some("luma") => Choice::Luma,
        Some(other) => {
            return Err(format!(
                "`{other}` is not a segmenter; choose auto, onnx or luma"
            ))
        }
    };
    let model_choice = a.value(&["--model"])?;
    let hint_box = a.value(&["--box"])?;
    let points = a.values(&["--point"])?;
    let ops_file = a.value(&["--ops"])?;
    let trim = a.flag(&["--trim"]);
    let fresh = a.flag(&["--fresh"]);
    let positionals = a.positionals(1)?;
    let input = PathBuf::from(
        positionals
            .first()
            .ok_or("no input image given (try --help)")?,
    );
    let output = output.map(PathBuf::from).unwrap_or_else(|| {
        let stem = input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("image");
        input.with_file_name(format!("{stem}-cutout.png"))
    });

    let hint = if let Some(b) = hint_box {
        let n = numbers(&b, 4, "--box")?;
        Hint::Box {
            x: n[0],
            y: n[1],
            w: n[2],
            h: n[3],
            margin: 0.08,
        }
    } else if !points.is_empty() {
        Hint::Points {
            points: points
                .iter()
                .map(|p| numbers(p, 2, "--point").map(|n| [n[0], n[1]]))
                .collect::<Result<_, _>>()?,
        }
    } else {
        Hint::None
    };

    // Decide honestly what will do the segmenting.
    let luma = match segmenter {
        Choice::Luma => true,
        Choice::Onnx => {
            let wanted = model_choice.clone().unwrap_or_else(|| {
                model::model_path(model::default_for(false))
                    .display()
                    .to_string()
            });
            let resolved = models::resolve(model_choice.as_deref()).map_err(|e| {
                format!("{e}\n  wanted: {wanted}\n  Or rerun with `--segmenter luma` to use the lightness split, which is not a matting model.")
            })?;
            runtime::environment().map_err(|e| {
                format!(
                    "{e}\n  model: {}\n  Or rerun with `--segmenter luma`.",
                    resolved.path.display()
                )
            })?;
            false
        }
        Choice::Auto => match (
            models::resolve(model_choice.as_deref()),
            runtime::environment(),
        ) {
            (Ok(_), Ok(_)) => false,
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("kata: {e}\nkata: using the lightness split (--segmenter luma) instead. It is not a matting model: expect rough edges.");
                true
            }
        },
    };

    let (img, hash) = kata_engine::open_image(&input).map_err(|e| e.to_string())?;
    let options = CutOptions {
        model: model_choice,
        hint,
        fresh,
        luma,
    };
    let (mut mask, rep) = cut::segment(&img, &hash, &options).map_err(|e| e.to_string())?;
    eprintln!(
        "kata: {} in {:.1}s{} ({:.0}% of the image is subject)",
        rep.model,
        rep.seconds,
        if rep.cached { " (cached)" } else { "" },
        rep.coverage * 100.0
    );
    if let Some(path) = ops_file {
        let ops_list: Vec<MaskOp> = read_json(&path)?;
        mask = ops::apply(&mask, Some(&img), &ops_list).map_err(|e| e.to_string())?;
    }
    for finding in inspect::mask_leftovers(&mask) {
        eprintln!(
            "kata: {:?} {}: {} {:?}",
            finding.severity,
            finding.check,
            finding.message,
            finding.bbox.unwrap_or_default()
        );
    }
    let mut out = cut::cutout(&img, &mask);
    if trim {
        out = cut::trim(&out, 4);
    }
    save(&out, &output)?;
    if let Some(path) = mask_out {
        let gray = image::GrayImage::from_raw(mask.width, mask.height, mask.data.clone())
            .ok_or("mask size")?;
        gray.save(&path).map_err(|e| e.to_string())?;
        println!("wrote {path} (mask)");
    }
    if let Some(path) = sticker_out {
        let st = sticker::render_sticker(
            &img,
            &mask,
            &kata_core::mask::MaskParams::default(),
            &StickerStyle::default(),
        )
        .map_err(|e| e.to_string())?;
        save(&st, Path::new(&path))?;
    }
    Ok(ExitCode::SUCCESS)
}

fn print_command(mut a: Args) -> Result<ExitCode, String> {
    let spec_path = a.value(&["--spec"])?.ok_or("--spec <json> is required")?;
    let mask = a.value(&["--mask"])?;
    let bg_spec = a.value(&["--background-spec"])?;
    let ground = a.value(&["--ground"])?;
    let output = a.value(&["-o", "--output"])?;
    let input = a
        .positionals(1)?
        .into_iter()
        .next()
        .ok_or("no input image given")?;
    let spec: PrintSpec = read_json(&spec_path)?;
    let (img, _) = kata_engine::open_image(Path::new(&input)).map_err(|e| e.to_string())?;
    let mask = mask.as_deref().map(load_mask).transpose()?;
    let out = match (bg_spec, &mask) {
        (Some(bg), Some(m)) => {
            let bg: PrintSpec = read_json(&bg)?;
            print::print_scene(&img, m, &bg, &spec)
        }
        (Some(_), None) => return Err("--background-spec needs --mask".into()),
        _ => print::print(&img, mask.as_ref(), &spec),
    }
    .map_err(|e| e.to_string())?;
    let output = output.unwrap_or_else(|| {
        format!(
            "{}-print.png",
            Path::new(&input).with_extension("").display()
        )
    });
    save(&out, Path::new(&output))?;
    if let Some(g) = ground {
        let findings =
            inspect::ink_against_ground(&out, Rgba::from_hex(&g).map_err(|e| e.to_string())?);
        return Ok(print_findings(&findings));
    }
    Ok(ExitCode::SUCCESS)
}

fn compose_command(mut a: Args) -> Result<ExitCode, String> {
    let output = a
        .value(&["-o", "--output"])?
        .ok_or("-o <png> is required")?;
    let target = a.value(&["--target"])?;
    let brand = a.value(&["--brand"])?;
    let scene_path = a
        .positionals(1)?
        .into_iter()
        .next()
        .ok_or("no scene file given")?;
    let mut scene: Scene = read_json(&scene_path)?;
    if scene.base_dir.is_none() {
        scene.base_dir = Path::new(&scene_path)
            .parent()
            .map(|p| p.display().to_string());
    }
    let brand = match brand {
        Some(b) => {
            let text = std::fs::read_to_string(&b).map_err(|e| e.to_string())?;
            Some(Brand::from_json(&text).map_err(|e| e.to_string())?)
        }
        None => None,
    };
    let rendered = compose::render(&scene).map_err(|e| e.to_string())?;
    save(&rendered.image, Path::new(&output))?;
    let rev = review::review(&rendered, target.as_deref(), brand.as_ref());
    let crops_dir = Path::new(&output).with_extension("review");
    let crops = review::crops(&rendered.image, &rev.findings, &crops_dir, "finding");
    let summary = serde_json::json!({"review": rev, "placed": rendered.placed, "crops": crops});
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).unwrap_or_default()
    );
    Ok(if rev.errors > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn inspect_command(mut a: Args) -> Result<ExitCode, String> {
    let mask = a.value(&["--mask"])?;
    let layer = a.value(&["--layer"])?;
    let ground = a.value(&["--ground"])?;
    let tex = a.value(&["--texture"])?;
    a.positionals(0)?;
    let mut findings = Vec::new();
    if let Some(m) = mask {
        findings.extend(inspect::mask_leftovers(&load_mask(&m)?));
    }
    if let (Some(l), Some(g)) = (layer, ground) {
        let img = image::open(&l).map_err(|e| e.to_string())?.to_rgba8();
        findings.extend(inspect::ink_against_ground(
            &img,
            Rgba::from_hex(&g).map_err(|e| e.to_string())?,
        ));
    }
    if let Some(t) = tex {
        let gray = image::open(&t).map_err(|e| e.to_string())?.to_luma8();
        for c in texture::find_creases(&gray) {
            findings.push(inspect::Finding {
                check: "texture_crease".into(),
                severity: inspect::Severity::Error,
                message: format!(
                    "a {} line at {} px ({:.0} degrees)",
                    c.orientation, c.position, c.angle_degrees
                ),
                bbox: None,
            });
        }
    }
    Ok(print_findings(&findings))
}

fn palette_command(mut a: Args) -> Result<ExitCode, String> {
    let count = match a.value(&["--count"])? {
        Some(c) => c
            .parse::<usize>()
            .map_err(|_| "--count must be a number".to_string())?,
        None => 6,
    };
    let input = a
        .positionals(1)?
        .into_iter()
        .next()
        .ok_or("no image given")?;
    let img = image::open(&input).map_err(|e| e.to_string())?.to_rgba8();
    for (colour, share) in extract_palette(&img, count) {
        println!(
            "{}  {:>5.1}%  L{:.0}",
            colour.to_hex(),
            share * 100.0,
            kata_core::lab::Lab::from_rgba(colour).l
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn texture_command(mut a: Args) -> Result<ExitCode, String> {
    if let Some(check) = a.value(&["--check"])? {
        a.positionals(0)?;
        let gray = image::open(&check).map_err(|e| e.to_string())?.to_luma8();
        let creases = texture::find_creases(&gray);
        println!(
            "{}",
            serde_json::to_string_pretty(&creases).unwrap_or_default()
        );
        return Ok(if creases.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }
    let output = a
        .value(&["-o", "--output"])?
        .ok_or("-o <png> is required")?;
    let size = a.value(&["--size"])?.unwrap_or_else(|| "1080x1080".into());
    let seed = a
        .value(&["--seed"])?
        .and_then(|s| s.parse().ok())
        .unwrap_or(7);
    a.positionals(0)?;
    let (w, h) = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .ok_or("--size must look like 1080x1350")?;
    let paper = image::DynamicImage::ImageLuma8(texture::paper(w, h, seed, 1.0)).to_rgba8();
    save(&paper, Path::new(&output))?;
    Ok(ExitCode::SUCCESS)
}

fn export_command(mut a: Args) -> Result<ExitCode, String> {
    let ids = a
        .value(&["--targets"])?
        .ok_or("--targets <id,id> is required")?;
    let dir = a.value(&["-d", "--dir"])?;
    let input = a
        .positionals(1)?
        .into_iter()
        .next()
        .ok_or("no image given")?;
    let img = image::open(&input).map_err(|e| e.to_string())?.to_rgba8();
    let stem = Path::new(&input)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export")
        .to_owned();
    let dir = dir.map(PathBuf::from).unwrap_or_else(|| {
        Path::new(&input)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    });
    for id in ids.split(',') {
        let t = targets::by_id(id.trim()).ok_or_else(|| {
            let known: Vec<&str> = targets::TARGETS.iter().map(|t| t.id).collect();
            format!("unknown target `{id}`; known: {}", known.join(", "))
        })?;
        let (w, h) = img.dimensions();
        let scale = (t.width as f32 / w as f32).max(t.height as f32 / h as f32);
        let sw = ((w as f32 * scale).round() as u32).max(t.width);
        let sh = ((h as f32 * scale).round() as u32).max(t.height);
        let scaled = image::imageops::resize(&img, sw, sh, image::imageops::FilterType::Lanczos3);
        let out = image::imageops::crop_imm(
            &scaled,
            (sw - t.width) / 2,
            (sh - t.height) / 2,
            t.width,
            t.height,
        )
        .to_image();
        save(&out, &dir.join(format!("{stem}-{}.png", t.id)))?;
    }
    Ok(ExitCode::SUCCESS)
}

fn models_command(rest: &[String]) -> Result<ExitCode, String> {
    match rest.first().map(String::as_str) {
        None | Some("list") => {
            for m in models::status() {
                println!(
                    "{:<24} {:>6.0} MB  {:<4} {:<10}  {}",
                    m.id,
                    m.megabytes,
                    m.license,
                    if m.downloaded { "downloaded" } else { "-" },
                    m.note
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("download") => {
            let id = rest
                .get(1)
                .ok_or("which model? `kata models download <id>`")?;
            let spec = model::require(id).map_err(|e| e.to_string())?;
            let mut last = 0u64;
            let path = models::download(spec, &mut |done, total| {
                let pct = done * 100 / total.max(1);
                if pct >= last + 5 {
                    last = pct;
                    eprint!("\rkata: {id} {pct}%");
                }
            })
            .map_err(|e| e.to_string())?;
            eprintln!();
            println!("{} verified at {}", spec.id, path.display());
            Ok(ExitCode::SUCCESS)
        }
        Some(other) => Err(format!(
            "`models {other}`? Use `kata models list` or `kata models download <id>`"
        )),
    }
}

fn status_command() -> Result<ExitCode, String> {
    println!("kata {}", env!("CARGO_PKG_VERSION"));
    match runtime::find_library() {
        Some(p) => println!("onnx runtime   {}", p.display()),
        None => {
            println!("onnx runtime   not found. Looked in:");
            for c in runtime::candidates() {
                println!("                 {}", c.display());
            }
        }
    }
    match models::resolve(None) {
        Ok(r) => println!("model          {} ({})", r.id, r.path.display()),
        Err(e) => println!("model          {e}"),
    }
    println!("data folder    {}", runtime::data_dir().display());
    println!("mask cache     {}", cut::cache_dir().display());
    Ok(ExitCode::SUCCESS)
}

fn mcp_command(mut a: Args) -> Result<ExitCode, String> {
    let work = a
        .value(&["--work"])?
        .map(PathBuf::from)
        .unwrap_or_else(|| runtime::data_dir().join("work"));
    let mut roots: Vec<PathBuf> = a
        .values(&["--allow"])?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    a.positionals(0)?;
    if roots.is_empty() {
        roots.push(std::env::current_dir().map_err(|e| e.to_string())?);
    }
    eprintln!(
        "kata mcp: work folder {}, allowed {:?}",
        work.display(),
        roots
    );
    kata_mcp::serve(kata_mcp::Config::new(work, roots)).map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}
