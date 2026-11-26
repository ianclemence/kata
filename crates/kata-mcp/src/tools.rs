//! The tools, their schemas, and the rules for touching the disk.

use crate::Config;
use base64::Engine as _;
use image::RgbaImage;
use kata_core::brand::{extract_palette, Brand};
use kata_core::color::Rgba;
use kata_core::inspect;
use kata_core::mask::Mask;
use kata_core::ops::{self, Hint, MaskOp};
use kata_core::print::{self, PrintSpec};
use kata_core::targets;
use kata_core::texture;
use kata_engine::compose::{self, Scene};
use kata_engine::cut::{self, CutOptions};
use kata_engine::{models, review, runtime};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const PREVIEW_SIDE: u32 = 768;

pub struct Server {
    config: Config,
    counter: usize,
    artifacts: HashMap<String, PathBuf>,
    /// Image hash by path, so the mask cache hits across calls.
    hashes: HashMap<PathBuf, String>,
}

type Out = Result<Vec<Value>, String>;

fn text(value: impl Into<String>) -> Value {
    json!({"type": "text", "text": value.into()})
}

fn report(value: &Value) -> Value {
    text(serde_json::to_string_pretty(value).unwrap_or_default())
}

fn preview(image: &RgbaImage) -> Value {
    let (w, h) = image.dimensions();
    let scale = (PREVIEW_SIDE as f32 / w.max(h) as f32).min(1.0);
    let small = if scale < 1.0 {
        image::imageops::resize(
            image,
            ((w as f32 * scale) as u32).max(1),
            ((h as f32 * scale) as u32).max(1),
            image::imageops::FilterType::Triangle,
        )
    } else {
        image.clone()
    };
    let bytes = kata_engine::encode_full(&small).unwrap_or_default();
    json!({"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(bytes), "mimeType": "image/png"})
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a Value, String> {
    args.get(key).ok_or_else(|| format!("`{key}` is required"))
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    arg(args, key)?
        .as_str()
        .ok_or_else(|| format!("`{key}` must be a string"))
}

fn parse<T: serde::de::DeserializeOwned>(value: &Value, what: &str) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|e| format!("`{what}` is not valid: {e}"))
}

fn e(err: impl std::fmt::Display) -> String {
    err.to_string()
}

impl Server {
    pub fn new(config: Config) -> std::io::Result<Server> {
        std::fs::create_dir_all(&config.work_dir)?;
        Ok(Server {
            config,
            counter: 0,
            artifacts: HashMap::new(),
            hashes: HashMap::new(),
        })
    }

    fn allowed(&self, path: &Path) -> bool {
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        // For a file that does not exist yet, check its folder.
        let target = if path.exists() {
            canonical(path)
        } else {
            path.parent()
                .map(canonical)
                .unwrap_or_else(|| path.to_path_buf())
                .join(path.file_name().unwrap_or_default())
        };
        std::iter::once(&self.config.work_dir)
            .chain(self.config.roots.iter())
            .map(|r| canonical(r))
            .any(|root| target.starts_with(root))
    }

    /// An input: an artifact id or a path inside the allowed folders.
    fn input(&self, reference: &str) -> Result<PathBuf, String> {
        if let Some(path) = self.artifacts.get(reference) {
            return Ok(path.clone());
        }
        let path = PathBuf::from(reference);
        if !path.exists() {
            return Err(format!(
                "`{reference}` is neither an artifact id from this session nor an existing file"
            ));
        }
        if !self.allowed(&path) {
            return Err(format!(
                "{} is outside the folders this server may read. Allowed: {} and {}. Start the server with --allow <folder> to add one.",
                path.display(),
                self.config.work_dir.display(),
                self.config.roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", ")
            ));
        }
        Ok(path)
    }

    fn image(&mut self, reference: &str) -> Result<(RgbaImage, String, PathBuf), String> {
        let path = self.input(reference)?;
        let (img, hash) = kata_engine::open_image(&path).map_err(e)?;
        self.hashes.insert(path.clone(), hash.clone());
        Ok((img, hash, path))
    }

    fn mask(&self, reference: &str) -> Result<Mask, String> {
        let path = self.input(reference)?;
        let gray = image::open(&path).map_err(e)?.to_luma8();
        let (w, h) = gray.dimensions();
        Ok(Mask {
            width: w,
            height: h,
            data: gray.into_raw(),
        })
    }

    /// Save an output: to `requested` if given (and allowed), else the work folder.
    fn save(
        &mut self,
        kind: &str,
        requested: Option<&str>,
        write: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<(String, PathBuf), String> {
        self.counter += 1;
        let id = format!("art_{}_{kind}", self.counter);
        let path = match requested {
            Some(p) => {
                let p = PathBuf::from(p);
                if !self.allowed(&p) {
                    return Err(format!(
                        "{} is outside the folders this server may write",
                        p.display()
                    ));
                }
                p
            }
            None => self.config.work_dir.join(format!("{id}.png")),
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(e)?;
        }
        write(&path)?;
        self.artifacts.insert(id.clone(), path.clone());
        Ok((id, path))
    }

    fn save_image(
        &mut self,
        kind: &str,
        requested: Option<&str>,
        image: &RgbaImage,
    ) -> Result<(String, PathBuf), String> {
        self.save(kind, requested, |p| {
            kata_engine::save_png(image, p).map(|_| ()).map_err(e)
        })
    }

    fn save_mask(&mut self, mask: &Mask) -> Result<(String, PathBuf), String> {
        let gray = image::GrayImage::from_raw(mask.width, mask.height, mask.data.clone())
            .ok_or("mask size mismatch")?;
        self.save("mask", None, |p| gray.save(p).map_err(e))
    }

    pub fn call(&mut self, name: &str, args: &Value) -> Out {
        match name {
            "kata_status" => self.status(),
            "kata_models_download" => self.download(args),
            "kata_segment" => self.segment(args),
            "kata_refine_mask" => self.refine(args),
            "kata_cutout" => self.cutout(args),
            "kata_print" => self.print(args),
            "kata_palette" => self.palette(args),
            "kata_brand" => self.brand(args),
            "kata_texture" => self.texture(args),
            "kata_compose" => self.compose(args),
            "kata_inspect" => self.inspect(args),
            "kata_view" => self.view(args),
            "kata_export" => self.export(args),
            other => Err(format!("there is no tool called {other}")),
        }
    }

    fn status(&mut self) -> Out {
        let runtime = runtime::find_library();
        let models = models::status();
        let ready = runtime.is_some() && models.iter().any(|m| m.downloaded);
        Ok(vec![report(&json!({
            "ready_to_segment": ready,
            "onnx_runtime": runtime.map(|p| p.display().to_string()),
            "runtime_searched": runtime::candidates().iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "models": models,
            "work_dir": self.config.work_dir,
            "allowed_roots": self.config.roots,
            "mask_cache": cut::cache_dir(),
            "targets": targets::TARGETS,
            "next": if ready { "Read kata://skills/kata-poster, then segment a photo." } else { "Download a model with kata_models_download (birefnet-lite-fp32 is the fast CPU choice), or set KATA_MODEL to a .onnx file." }
        }))])
    }

    fn download(&mut self, args: &Value) -> Out {
        let id = str_arg(args, "id")?;
        let spec = kata_core::model::require(id).map_err(e)?;
        let path = models::download(spec, &mut |_, _| {}).map_err(e)?;
        Ok(vec![report(
            &json!({"downloaded": id, "path": path, "megabytes": spec.megabytes()}),
        )])
    }

    fn segment(&mut self, args: &Value) -> Out {
        let (img, hash, _) = self.image(str_arg(args, "image")?)?;
        let hint: Hint = match args.get("hint") {
            Some(h) => parse(h, "hint")?,
            None => Hint::None,
        };
        let options = CutOptions {
            model: args.get("model").and_then(Value::as_str).map(str::to_owned),
            hint,
            fresh: args.get("fresh").and_then(Value::as_bool).unwrap_or(false),
            luma: false,
        };
        let (mask, rep) = cut::segment(&img, &hash, &options).map_err(e)?;
        let leftovers = inspect::mask_leftovers(&mask);
        let (mask_id, mask_path) = self.save_mask(&mask)?;
        let cut_img = cut::cutout(&img, &mask);
        let (cut_id, cut_path) = self.save_image("cutout", None, &cut_img)?;
        Ok(vec![
            report(&json!({
                "mask": mask_id, "mask_path": mask_path, "cutout": cut_id, "cutout_path": cut_path,
                "report": rep, "findings": leftovers,
                "advice": "This is the model's raw mask. Check edges with kata_view at 100% before editing. Only edit what a finding or your own look shows is wrong; never 'improve' a clean cut."
            })),
            preview(&cut_img),
        ])
    }

    fn refine(&mut self, args: &Value) -> Out {
        let mask = self.mask(str_arg(args, "mask")?)?;
        let ops_list: Vec<MaskOp> = parse(arg(args, "ops")?, "ops")?;
        let img = match args.get("image").and_then(Value::as_str) {
            Some(r) => Some(self.image(r)?.0),
            None => None,
        };
        let out = ops::apply(&mask, img.as_ref(), &ops_list).map_err(e)?;
        let findings = inspect::mask_leftovers(&out);
        let (id, path) = self.save_mask(&out)?;
        let mut content = vec![report(
            &json!({"mask": id, "path": path, "applied": ops_list, "findings": findings}),
        )];
        if let Some(img) = img {
            let preview_img = cut::cutout(&img, &out);
            let (cid, _) = self.save_image("cutout", None, &preview_img)?;
            content[0] = report(
                &json!({"mask": id, "path": path, "cutout": cid, "applied": ops_list, "findings": findings}),
            );
            content.push(preview(&preview_img));
        }
        Ok(content)
    }

    fn cutout(&mut self, args: &Value) -> Out {
        let (img, _, _) = self.image(str_arg(args, "image")?)?;
        let mask = self.mask(str_arg(args, "mask")?)?;
        if mask.dimensions() != img.dimensions() {
            return Err("the mask and the image are different sizes".into());
        }
        let mut out = cut::cutout(&img, &mask);
        if args.get("trim").and_then(Value::as_bool).unwrap_or(false) {
            out = cut::trim(&out, 4);
        }
        let (id, path) =
            self.save_image("cutout", args.get("output").and_then(Value::as_str), &out)?;
        Ok(vec![
            report(&json!({"cutout": id, "path": path, "size": out.dimensions()})),
            preview(&out),
        ])
    }

    fn print(&mut self, args: &Value) -> Out {
        let (img, _, _) = self.image(str_arg(args, "image")?)?;
        let spec: PrintSpec = parse(arg(args, "spec")?, "spec")?;
        let mask = match args.get("mask").and_then(Value::as_str) {
            Some(r) => Some(self.mask(r)?),
            None => None,
        };
        let out = match (args.get("background_spec"), &mask) {
            (Some(bg), Some(m)) => {
                let bg: PrintSpec = parse(bg, "background_spec")?;
                print::print_scene(&img, m, &bg, &spec).map_err(e)?
            }
            (Some(_), None) => {
                return Err(
                    "background_spec needs a mask: the subject is printed over the background"
                        .into(),
                )
            }
            _ => print::print(&img, mask.as_ref(), &spec).map_err(e)?,
        };
        let mut findings = Vec::new();
        if let Some(ground) = args.get("ground").and_then(Value::as_str) {
            findings = inspect::ink_against_ground(&out, Rgba::from_hex(ground).map_err(e)?);
        }
        let (id, path) =
            self.save_image("print", args.get("output").and_then(Value::as_str), &out)?;
        Ok(vec![
            report(&json!({"print": id, "path": path, "findings": findings})),
            preview(&out),
        ])
    }

    fn palette(&mut self, args: &Value) -> Out {
        if let Some(r) = args.get("image").and_then(Value::as_str) {
            let (img, _, _) = self.image(r)?;
            let count = args.get("count").and_then(Value::as_u64).unwrap_or(6) as usize;
            let colours: Vec<Value> = extract_palette(&img, count)
                .into_iter()
                .map(|(c, share)| json!({"hex": c.to_hex(), "share": (share * 1000.0).round() / 1000.0, "lightness": kata_core::lab::Lab::from_rgba(c).l.round()}))
                .collect();
            return Ok(vec![report(
                &json!({"colours": colours, "advice": "Give each colour a role (ground, ink, paper, accent, action) in a brand file. Never print a subject in the ground colour."}),
            )]);
        }
        if let Some(hexes) = args.get("hex").and_then(Value::as_array) {
            let colours: Vec<Rgba> = hexes
                .iter()
                .filter_map(Value::as_str)
                .map(Rgba::from_hex)
                .collect::<Result<_, _>>()
                .map_err(e)?;
            let mut pairs = Vec::new();
            for (i, a) in colours.iter().enumerate() {
                for b in &colours[i + 1..] {
                    pairs.push(json!({"a": a.to_hex(), "b": b.to_hex(), "contrast": (kata_core::lab::contrast_ratio(*a, *b) * 100.0).round() / 100.0, "delta_e": kata_core::lab::Lab::from_rgba(*a).delta_e(&kata_core::lab::Lab::from_rgba(*b)).round()}));
                }
            }
            return Ok(vec![report(
                &json!({"pairs": pairs, "advice": "Contrast 4.5 for body text, 3 for display type and logos. Delta E under 10 reads as the same ink."}),
            )]);
        }
        Err("pass `image` (extract a palette) or `hex` (compare colours)".into())
    }

    fn brand(&mut self, args: &Value) -> Out {
        let path = self.input(str_arg(args, "path")?)?;
        let brand = Brand::from_json(&std::fs::read_to_string(&path).map_err(e)?).map_err(e)?;
        let inks = brand.inks.clone();
        let mut warnings = Vec::new();
        if let Some(ground) = brand.with_role(kata_core::brand::Role::Ground).first() {
            let g = ground.colour().map_err(e)?;
            for ink in &brand.inks {
                let c = ink.colour().map_err(e)?;
                if ink.name != ground.name
                    && kata_core::lab::Lab::from_rgba(c).delta_e(&kata_core::lab::Lab::from_rgba(g))
                        < 10.0
                {
                    warnings.push(format!(
                        "ink `{}` is almost the ground colour: anything printed in it disappears",
                        ink.name
                    ));
                }
            }
        }
        Ok(vec![report(
            &json!({"brand": brand.name, "inks": inks, "fonts": brand.fonts, "logos": brand.logos, "voice": brand.voice, "website": brand.website, "warnings": warnings}),
        )])
    }

    fn texture(&mut self, args: &Value) -> Out {
        if let Some(r) = args.get("check").and_then(Value::as_str) {
            let path = self.input(r)?;
            let gray = image::open(&path).map_err(e)?.to_luma8();
            let creases = texture::find_creases(&gray);
            return Ok(vec![report(
                &json!({"creases": creases, "verdict": if creases.is_empty() { "clean" } else { "this texture has straight lines that will print on everything; use generated paper instead" }}),
            )]);
        }
        let w = args.get("width").and_then(Value::as_u64).unwrap_or(1080) as u32;
        let h = args.get("height").and_then(Value::as_u64).unwrap_or(1080) as u32;
        let seed = args.get("seed").and_then(Value::as_u64).unwrap_or(7) as u32;
        let strength = args.get("strength").and_then(Value::as_f64).unwrap_or(1.0) as f32;
        let paper = image::DynamicImage::ImageLuma8(texture::paper(
            w.min(8000),
            h.min(8000),
            seed,
            strength,
        ))
        .to_rgba8();
        let (id, path) =
            self.save_image("paper", args.get("output").and_then(Value::as_str), &paper)?;
        Ok(vec![report(
            &json!({"paper": id, "path": path, "use": "overlay blend at 0.6-0.9 opacity, or set finish.paper in a scene"}),
        )])
    }

    fn compose(&mut self, args: &Value) -> Out {
        let mut scene: Scene = parse(arg(args, "scene")?, "scene")?;
        // Resolve artifact ids inside the scene so agents can reference earlier outputs.
        for layer in scene.layers.iter_mut() {
            let src = match layer {
                compose::Layer::Image { src, .. } | compose::Layer::Logo { src, .. } => src,
                _ => continue,
            };
            let base = scene.base_dir.as_ref().map(PathBuf::from);
            let full = match (&base, PathBuf::from(&*src).is_absolute()) {
                (Some(b), false) if !self.artifacts.contains_key(src.as_str()) => {
                    b.join(&*src).display().to_string()
                }
                _ => src.clone(),
            };
            *src = self.input(&full)?.display().to_string();
        }
        for font in scene.fonts.iter_mut() {
            *font = self.input(font)?.display().to_string();
        }
        let brand = match args.get("brand").and_then(Value::as_str) {
            Some(r) => Some(
                Brand::from_json(&std::fs::read_to_string(self.input(r)?).map_err(e)?)
                    .map_err(e)?,
            ),
            None => None,
        };
        let target = args.get("target").and_then(Value::as_str);
        let rendered = compose::render(&scene).map_err(e)?;
        let review = review::review(&rendered, target, brand.as_ref());
        let (id, path) = self.save_image(
            "poster",
            args.get("output").and_then(Value::as_str),
            &rendered.image,
        )?;
        let crops_dir = self.config.work_dir.join(format!("{id}-review"));
        let crops = review::crops(&rendered.image, &review.findings, &crops_dir, &id);
        for (i, c) in crops.iter().enumerate() {
            self.artifacts
                .insert(format!("{id}_finding_{i}"), c.clone());
        }
        Ok(vec![
            report(&json!({
                "poster": id, "path": path, "placed": rendered.placed, "review": review,
                "finding_crops": crops,
                "next": if review.errors > 0 { "Fix every error, then compose again." } else { "Now zoom in with kata_view on the logo, every headline edge and the subject's edges before calling it done." }
            })),
            preview(&rendered.image),
        ])
    }

    fn inspect(&mut self, args: &Value) -> Out {
        let mut findings = Vec::new();
        if let Some(r) = args.get("mask").and_then(Value::as_str) {
            findings.extend(inspect::mask_leftovers(&self.mask(r)?));
        }
        if let (Some(r), Some(g)) = (
            args.get("layer").and_then(Value::as_str),
            args.get("ground").and_then(Value::as_str),
        ) {
            let (img, _, _) = self.image(r)?;
            findings.extend(inspect::ink_against_ground(
                &img,
                Rgba::from_hex(g).map_err(e)?,
            ));
        }
        if let (Some(r), Some(regions)) =
            (args.get("image").and_then(Value::as_str), args.get("faces"))
        {
            let (img, _, _) = self.image(r)?;
            let regions: Vec<(String, [u32; 4])> = parse::<Vec<[u32; 4]>>(regions, "faces")?
                .into_iter()
                .enumerate()
                .map(|(i, b)| (format!("face {}", i + 1), b))
                .collect();
            findings.extend(inspect::dark_regions(
                &img,
                &regions,
                args.get("min_luma").and_then(Value::as_u64).unwrap_or(55) as u8,
            ));
        }
        if let Some(r) = args.get("texture").and_then(Value::as_str) {
            let gray = image::open(self.input(r)?).map_err(e)?.to_luma8();
            for c in texture::find_creases(&gray) {
                findings.push(inspect::Finding {
                    check: "texture_crease".into(),
                    severity: inspect::Severity::Error,
                    message: format!("a {} line at {} px ({:.0} degrees) will print on everything this texture covers", c.orientation, c.position, c.angle_degrees),
                    bbox: None,
                });
            }
        }
        if let (Some(fg), Some(bg)) = (
            args.get("foreground").and_then(Value::as_str),
            args.get("background").and_then(Value::as_str),
        ) {
            let large = args.get("large").and_then(Value::as_bool).unwrap_or(true);
            findings.extend(inspect::legibility(
                "the element",
                Rgba::from_hex(fg).map_err(e)?,
                Rgba::from_hex(bg).map_err(e)?,
                large,
                None,
            ));
        }
        let review = review::Review::new(findings);
        Ok(vec![report(&json!(review))])
    }

    fn view(&mut self, args: &Value) -> Out {
        let (img, _, _) = self.image(str_arg(args, "image")?)?;
        let (w, h) = img.dimensions();
        let [x, y, rw, rh] = match args.get("region") {
            Some(r) => parse::<[u32; 4]>(r, "region")?,
            None => [0, 0, w, h],
        };
        let (x, y) = (x.min(w - 1), y.min(h - 1));
        let (rw, rh) = (rw.min(w - x).max(1), rh.min(h - y).max(1));
        let mut crop = image::imageops::crop_imm(&img, x, y, rw, rh).to_image();
        let zoom = args
            .get("zoom")
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
            .clamp(0.1, 8.0) as f32;
        if (zoom - 1.0).abs() > 1e-3 {
            crop = image::imageops::resize(
                &crop,
                ((rw as f32 * zoom) as u32).max(1),
                ((rh as f32 * zoom) as u32).max(1),
                image::imageops::FilterType::Nearest,
            );
        }
        if args.get("boost").and_then(Value::as_bool).unwrap_or(false) {
            // Blur away grain and halftone, then stretch: hidden creases and banding jump out.
            let gray = image::imageops::blur(
                &image::DynamicImage::ImageRgba8(crop.clone()).to_luma8(),
                4.0,
            );
            let mut values: Vec<u8> = gray.as_raw().clone();
            values.sort_unstable();
            let median = f32::from(values[values.len() / 2]);
            crop = image::DynamicImage::ImageLuma8(image::GrayImage::from_fn(
                gray.width(),
                gray.height(),
                |px, py| {
                    image::Luma([
                        ((f32::from(gray.get_pixel(px, py).0[0]) - median) * 12.0 + 128.0)
                            .clamp(0.0, 255.0) as u8,
                    ])
                },
            ))
            .to_rgba8();
        }
        if args
            .get("checker")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            for (px, py, p) in crop.enumerate_pixels_mut() {
                let bg = if (px / 12 + py / 12) % 2 == 0 {
                    200
                } else {
                    140
                };
                let a = f32::from(p.0[3]) / 255.0;
                for c in 0..3 {
                    p.0[c] = (f32::from(p.0[c]) * a + bg as f32 * (1.0 - a)) as u8;
                }
                p.0[3] = 255;
            }
        }
        let mut content = vec![text(format!(
            "region {x},{y} {rw}x{rh} of {w}x{h}, zoom {zoom}"
        ))];
        // Views are for looking: send them at full detail up to a sane cap.
        let cap = 1400u32;
        let s = (cap as f32 / crop.width().max(crop.height()) as f32).min(1.0);
        let shown = if s < 1.0 {
            image::imageops::resize(
                &crop,
                (crop.width() as f32 * s) as u32,
                (crop.height() as f32 * s) as u32,
                image::imageops::FilterType::Triangle,
            )
        } else {
            crop
        };
        let bytes = kata_engine::encode_full(&shown).map_err(e)?;
        content.push(json!({"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(bytes), "mimeType": "image/png"}));
        Ok(content)
    }

    fn export(&mut self, args: &Value) -> Out {
        let (img, _, source) = self.image(str_arg(args, "image")?)?;
        let ids: Vec<String> = parse(arg(args, "targets")?, "targets")?;
        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("export")
            .to_owned();
        let dir = match args.get("output_dir").and_then(Value::as_str) {
            Some(d) => {
                let d = PathBuf::from(d);
                if !self.allowed(&d.join("x")) {
                    return Err(format!(
                        "{} is outside the folders this server may write",
                        d.display()
                    ));
                }
                d
            }
            None => self.config.work_dir.clone(),
        };
        let mut outputs = Vec::new();
        for tid in ids {
            let t = targets::by_id(&tid)
                .ok_or_else(|| format!("unknown target {tid}; see kata_status for the list"))?;
            // Cover-fit: scale to fill, centre-crop the overflow.
            let (w, h) = img.dimensions();
            let scale = (t.width as f32 / w as f32).max(t.height as f32 / h as f32);
            let (sw, sh) = (
                ((w as f32 * scale).round() as u32).max(t.width),
                ((h as f32 * scale).round() as u32).max(t.height),
            );
            let scaled =
                image::imageops::resize(&img, sw, sh, image::imageops::FilterType::Lanczos3);
            let out = image::imageops::crop_imm(
                &scaled,
                (sw - t.width) / 2,
                (sh - t.height) / 2,
                t.width,
                t.height,
            )
            .to_image();
            let path = dir.join(format!("{stem}-{}.png", t.id));
            let (id, path) = self.save_image("export", Some(&path.display().to_string()), &out)?;
            // Share of the (scaled) picture that falls outside the target.
            let lost = 1.0 - (t.width as f32 * t.height as f32) / (sw as f32 * sh as f32);
            outputs.push(json!({"target": t.id, "id": id, "path": path, "size": [t.width, t.height], "cropped_share": (lost.max(0.0) * 100.0).round() / 100.0}));
        }
        Ok(vec![report(
            &json!({"exports": outputs, "advice": "If a target crops more than a few percent, compose a separate scene at that size instead of exporting."}),
        )])
    }
}

/// Tool definitions with JSON Schemas, as `tools/list` returns them.
pub fn definitions() -> Value {
    let image = json!({"type": "string", "description": "An artifact id (art_N_kind) or a path inside the allowed folders"});
    let polygon = json!({"type": "array", "items": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2}});
    let read_only = json!({"readOnlyHint": true, "openWorldHint": false});
    let writes = json!({"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false});
    json!([
        {"name": "kata_status", "title": "Kata status", "description": "Whether Kata can segment right now: ONNX Runtime location, downloaded models, folders, and the export targets. Call first.", "inputSchema": {"type": "object", "properties": {}}, "annotations": read_only},
        {"name": "kata_models_download", "title": "Download a model", "description": "Download a segmentation model from the registry and verify its SHA-256. birefnet-lite-fp32 (224 MB) is the fast CPU choice; birefnet-general-fp16 has the best edges.", "inputSchema": {"type": "object", "properties": {"id": {"type": "string", "enum": kata_core::model::MODELS.iter().map(|m| m.id).collect::<Vec<_>>()}}, "required": ["id"]}, "annotations": {"readOnlyHint": false, "openWorldHint": true, "idempotentHint": true}},
        {"name": "kata_segment", "title": "Cut out a subject", "description": "Segment the subject of a photo with a matting model. Returns the raw mask, a cutout and leftover findings (islands, slivers). In a busy scene give a box hint around the subject: the model locks onto what it sees. Masks are cached by image bytes, model and hint, so repeat calls are instant.", "inputSchema": {"type": "object", "properties": {
            "image": image,
            "hint": {"description": "Where the subject is", "oneOf": [
                {"type": "object", "properties": {"kind": {"const": "box"}, "x": {"type": "integer"}, "y": {"type": "integer"}, "w": {"type": "integer"}, "h": {"type": "integer"}, "margin": {"type": "number", "description": "Padding as a fraction of the box, default 0.08"}}, "required": ["kind", "x", "y", "w", "h"]},
                {"type": "object", "properties": {"kind": {"const": "points"}, "points": {"type": "array", "items": {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2}}}, "required": ["kind", "points"]}
            ]},
            "model": {"type": "string", "description": "Registry id or path to a .onnx file. Default: KATA_MODEL, then the first downloaded model."},
            "fresh": {"type": "boolean", "description": "Ignore the cache"}
        }, "required": ["image"]}, "annotations": writes},
        {"name": "kata_refine_mask", "title": "Edit a mask", "description": "Apply named, explicit edits to a mask, in order. Only edit what is wrong: keep_largest, keep_at (points), drop_specks, fill_holes (small holes only; gaps between limbs are background), remove/add (polygon), grow, shrink, feather, harden, matte (photo-guided edge for hair; needs image).", "inputSchema": {"type": "object", "properties": {
            "mask": image, "image": image,
            "ops": {"type": "array", "items": {"type": "object", "properties": {"op": {"type": "string", "enum": ["keep_largest", "keep_at", "drop_specks", "fill_holes", "remove", "add", "grow", "shrink", "feather", "harden", "matte"]}, "points": {"type": "array"}, "min_area": {"type": "integer"}, "max_area": {"type": "integer"}, "polygon": polygon, "px": {"type": "number"}, "low": {"type": "integer"}, "high": {"type": "integer"}, "radius": {"type": "integer"}, "epsilon": {"type": "number"}}, "required": ["op"]}}
        }, "required": ["mask", "ops"]}, "annotations": writes},
        {"name": "kata_cutout", "title": "Make a cutout", "description": "Apply a mask to its photo as alpha and save a transparent PNG.", "inputSchema": {"type": "object", "properties": {"image": image, "mask": image, "trim": {"type": "boolean"}, "output": {"type": "string"}}, "required": ["image", "mask"]}, "annotations": writes},
        {"name": "kata_print", "title": "Screen-print in inks", "description": "Print a photo or its subject in a few flat inks. mode tonal ranks inks by lightness (portraits); mode spot gives each patch its nearest ink by colour (busy, colourful scenes). prep {gamma 0.7, clahe 40} keeps faces readable. regions swap forbidden inks inside polygons (no paper ink on faces). line adds a stroke plate (pencil, pen). With background_spec and a mask, prints a quiet scene with the subject at full strength on top. Pass ground to check no part of the subject is printed in the ground colour.", "inputSchema": {"type": "object", "properties": {
            "image": image, "mask": image,
            "spec": {"type": "object", "properties": {"mode": {"enum": ["tonal", "spot"]}, "inks": {"type": "array", "items": {"type": "string"}}, "prep": {"type": "object", "properties": {"gamma": {"type": "number"}, "clahe": {"type": "integer"}}}, "smooth": {"type": "integer"}, "hue_weight": {"type": "number"}, "despeckle": {"type": "integer"}, "regions": {"type": "array", "items": {"type": "object", "properties": {"polygon": polygon, "forbid": {"type": "array", "items": {"type": "string"}}, "replace_with": {"type": "string"}}}}, "line": {"type": "object", "properties": {"ink": {"type": "string"}, "threshold": {"type": "integer"}, "width": {"type": "integer"}}}}, "required": ["inks"]},
            "background_spec": {"type": "object", "description": "Same shape as spec, for the scene behind the subject"},
            "ground": {"type": "string", "description": "Hex of the poster ground, for the ink-against-ground check"},
            "output": {"type": "string"}
        }, "required": ["image", "spec"]}, "annotations": writes},
        {"name": "kata_palette", "title": "Palette", "description": "Extract a palette from an image (a logo, a mood photo) with shares, or compare hex colours (contrast ratio and delta E for every pair).", "inputSchema": {"type": "object", "properties": {"image": image, "count": {"type": "integer"}, "hex": {"type": "array", "items": {"type": "string"}}}}, "annotations": read_only},
        {"name": "kata_brand", "title": "Load a brand", "description": "Validate and summarise a brand.json: inks with roles, fonts, logos and size rules, voice notes. Warns when an ink is too close to the ground.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}, "annotations": read_only},
        {"name": "kata_texture", "title": "Paper texture", "description": "Generate a crease-free paper texture, or check an existing texture for fold lines that would print on every poster.", "inputSchema": {"type": "object", "properties": {"width": {"type": "integer"}, "height": {"type": "integer"}, "seed": {"type": "integer"}, "strength": {"type": "number"}, "check": image, "output": {"type": "string"}}}, "annotations": writes},
        {"name": "kata_compose", "title": "Compose a poster", "description": "Render a poster from a JSON scene (rect, circle, image, logo, text, burst, svg layers; finish.paper and finish.halftone) and review it: text running into art, overlapping type, legibility against what is really behind each element, logo size against the brand, and platform safe areas. Returns where every element landed and a crop for each finding.", "inputSchema": {"type": "object", "properties": {
            "scene": {"type": "object", "properties": {"width": {"type": "integer"}, "height": {"type": "integer"}, "background": {"type": "string"}, "fonts": {"type": "array", "items": {"type": "string"}}, "layers": {"type": "array", "items": {"type": "object", "properties": {"type": {"enum": ["rect", "circle", "image", "logo", "text", "burst", "svg"]}, "id": {"type": "string"}}, "required": ["type"]}}, "finish": {"type": "object", "properties": {"paper": {"type": "number"}, "seed": {"type": "integer"}, "halftone": {"type": "number"}, "halftone_cell": {"type": "number"}}}, "base_dir": {"type": "string"}}, "required": ["width", "height"]},
            "target": {"type": "string", "enum": targets::TARGETS.iter().map(|t| t.id).collect::<Vec<_>>()},
            "brand": {"type": "string", "description": "Path to a brand.json for logo-size rules"},
            "output": {"type": "string"}
        }, "required": ["scene"]}, "annotations": writes},
        {"name": "kata_inspect", "title": "Inspect", "description": "Run individual checks: mask leftovers (mask), ink against ground (layer + ground), dark faces (image + faces boxes), texture creases (texture), legibility (foreground + background).", "inputSchema": {"type": "object", "properties": {"mask": image, "layer": image, "ground": {"type": "string"}, "image": image, "faces": {"type": "array", "items": {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4}}, "min_luma": {"type": "integer"}, "texture": image, "foreground": {"type": "string"}, "background": {"type": "string"}, "large": {"type": "boolean"}}}, "annotations": read_only},
        {"name": "kata_view", "title": "Look closely", "description": "See any region of an image: crop, zoom (2-4 to see edges pixel by pixel), boost (blur and stretch contrast to reveal hidden creases or banding), checker (show transparency). Use it on every edge, logo and line of type before calling a design done.", "inputSchema": {"type": "object", "properties": {"image": image, "region": {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4, "description": "x, y, w, h"}, "zoom": {"type": "number"}, "boost": {"type": "boolean"}, "checker": {"type": "boolean"}}, "required": ["image"]}, "annotations": read_only},
        {"name": "kata_export", "title": "Export for platforms", "description": "Export an image to platform sizes (cover-fit) and report how much each crops. Prefer composing at the right size; use this for quick variants.", "inputSchema": {"type": "object", "properties": {"image": image, "targets": {"type": "array", "items": {"type": "string", "enum": targets::TARGETS.iter().map(|t| t.id).collect::<Vec<_>>()}}, "output_dir": {"type": "string"}}, "required": ["image", "targets"]}, "annotations": writes}
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> (Server, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "kata-mcp-test-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (
            Server::new(Config::new(dir.join("work"), vec![dir.clone()])).unwrap(),
            dir,
        )
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn every_tool_has_a_schema_and_a_handler() {
        let (mut s, _) = server();
        for tool in definitions().as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            assert!(tool["inputSchema"]["type"] == "object", "{name}");
            let err = s.call(name, &json!({"__probe": true}));
            if let Err(message) = err {
                assert!(
                    !message.starts_with("there is no tool"),
                    "{name} has no handler"
                );
            }
        }
    }

    #[test]
    fn paths_outside_the_roots_are_refused() {
        let (s, _) = server();
        let outside = std::env::temp_dir().join("kata-definitely-outside.png");
        std::fs::write(&outside, b"x").unwrap();
        assert!(s
            .input(outside.to_str().unwrap())
            .unwrap_err()
            .contains("outside"));
    }

    #[test]
    fn texture_compose_view_and_artifact_ids_work_together() {
        let (mut s, dir) = server();
        let out = s
            .call("kata_texture", &json!({"width": 64, "height": 64}))
            .unwrap();
        let rep: Value = serde_json::from_str(out[0]["text"].as_str().unwrap()).unwrap();
        let paper = rep["paper"].as_str().unwrap().to_owned();
        let scene = json!({"width": 200, "height": 100, "background": "#F05A47", "layers": [
            {"type": "image", "id": "paper", "src": paper, "x": 0, "y": 0, "w": 64},
            {"type": "rect", "id": "plate", "x": 120, "y": 20, "w": 60, "h": 60, "fill": "#2A1308"}
        ], "finish": {"paper": 0.5}});
        let out = s.call("kata_compose", &json!({"scene": scene})).unwrap();
        assert_eq!(out[1]["type"], "image");
        let rep: Value = serde_json::from_str(out[0]["text"].as_str().unwrap()).unwrap();
        let poster = rep["poster"].as_str().unwrap().to_owned();
        let view = s
            .call(
                "kata_view",
                &json!({"image": poster, "region": [100, 0, 100, 100], "zoom": 2, "boost": true}),
            )
            .unwrap();
        assert_eq!(view[1]["mimeType"], "image/png");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn print_spot_and_inspect_ink_against_ground() {
        let (mut s, dir) = server();
        let mut img = RgbaImage::from_pixel(60, 40, image::Rgba([250, 120, 40, 255]));
        for x in 30..60 {
            for y in 0..40 {
                img.put_pixel(x, y, image::Rgba([30, 40, 160, 255]));
            }
        }
        let path = dir.join("photo.png");
        img.save(&path).unwrap();
        let out = s
            .call("kata_print", &json!({"image": path, "spec": {"mode": "spot", "inks": ["#FF7A2E", "#2A1630"], "smooth": 0}, "ground": "#FF7A2E"}))
            .unwrap();
        let rep: Value = serde_json::from_str(out[0]["text"].as_str().unwrap()).unwrap();
        assert!(!rep["findings"].as_array().unwrap().is_empty(), "{rep}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
