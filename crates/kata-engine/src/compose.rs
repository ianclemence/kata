//! Posters from a JSON scene: layers, type and print finish, rendered with resvg.
//!
//! A scene is data, so an agent can write one, render it, read the review, move
//! one thing and render again. Every layer can carry an `id`; the render reports
//! each element's box so the review can say exactly what collides with what.

use crate::runtime;
use base64::Engine as _;
use image::RgbaImage;
use kata_core::color::Rgba;
use kata_core::error::{KataError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_background")]
    pub background: String,
    /// Font files to load (TTF/OTF). System fonts are loaded too.
    #[serde(default)]
    pub fonts: Vec<String>,
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub finish: Finish,
    /// Relative paths in the scene resolve against this folder.
    #[serde(default)]
    pub base_dir: Option<String>,
}

fn default_background() -> String {
    "#FFFFFF".into()
}

/// The print finish laid over everything: paper and halftone, like the covers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Finish {
    /// Paper texture strength 0..1. 0 is off.
    #[serde(default)]
    pub paper: f32,
    #[serde(default = "default_seed")]
    pub seed: u32,
    /// Halftone dot opacity 0..1. 0 is off.
    #[serde(default)]
    pub halftone: f32,
    /// Distance between halftone dots in pixels.
    #[serde(default = "default_cell")]
    pub halftone_cell: f32,
}

impl Default for Finish {
    fn default() -> Self {
        Finish {
            paper: 0.0,
            seed: default_seed(),
            halftone: 0.0,
            halftone_cell: default_cell(),
        }
    }
}

fn default_seed() -> u32 {
    7
}
fn default_cell() -> f32 {
    6.6
}
fn one() -> f32 {
    1.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    #[default]
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Layer {
    Rect {
        #[serde(default)]
        id: Option<String>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fill: String,
        #[serde(default)]
        radius: f32,
        #[serde(default = "one")]
        opacity: f32,
    },
    Circle {
        #[serde(default)]
        id: Option<String>,
        cx: f32,
        cy: f32,
        r: f32,
        #[serde(default)]
        fill: Option<String>,
        #[serde(default)]
        stroke: Option<String>,
        #[serde(default)]
        stroke_width: f32,
        /// e.g. "2 12" for a dotted ring.
        #[serde(default)]
        dash: Option<String>,
    },
    Image {
        #[serde(default)]
        id: Option<String>,
        src: String,
        x: f32,
        y: f32,
        w: f32,
        /// Defaults to keeping the image's aspect ratio.
        #[serde(default)]
        h: Option<f32>,
        #[serde(default = "one")]
        opacity: f32,
        /// Clip to a circle inscribed in the box.
        #[serde(default)]
        clip_circle: bool,
        #[serde(default)]
        rotate: f32,
        /// e.g. "multiply", "overlay", "screen".
        #[serde(default)]
        blend: Option<String>,
    },
    /// A logo with real transparency, optionally recoloured to one ink.
    Logo {
        #[serde(default)]
        id: Option<String>,
        src: String,
        x: f32,
        y: f32,
        w: f32,
        #[serde(default)]
        color: Option<String>,
    },
    Text {
        #[serde(default)]
        id: Option<String>,
        /// Lines separated by "\n".
        text: String,
        x: f32,
        y: f32,
        font: String,
        size: f32,
        #[serde(default = "default_weight")]
        weight: u16,
        color: String,
        #[serde(default)]
        anchor: Anchor,
        #[serde(default)]
        letter_spacing: f32,
        /// Multiple of the size.
        #[serde(default = "default_line_height")]
        line_height: f32,
        #[serde(default)]
        uppercase: bool,
        #[serde(default)]
        rotate: f32,
        #[serde(default = "one")]
        opacity: f32,
        /// Vertical writing, top to bottom (obi strips, spines).
        #[serde(default)]
        vertical: bool,
    },
    /// A star-burst sticker with a word or two in it.
    Burst {
        #[serde(default)]
        id: Option<String>,
        cx: f32,
        cy: f32,
        size: f32,
        fill: String,
        #[serde(default = "default_points")]
        points: u32,
        #[serde(default = "default_inner")]
        inner: f32,
        #[serde(default)]
        text: String,
        #[serde(default = "default_burst_color")]
        color: String,
        #[serde(default = "default_burst_font")]
        font: String,
        #[serde(default)]
        rotate: f32,
    },
    /// A raw SVG fragment for anything else (borders, patterns, rules).
    Svg {
        #[serde(default)]
        id: Option<String>,
        markup: String,
    },
}

fn default_weight() -> u16 {
    400
}
fn default_line_height() -> f32 {
    0.92
}
fn default_points() -> u32 {
    18
}
fn default_inner() -> f32 {
    0.8
}
fn default_burst_color() -> String {
    "#000000".into()
}
fn default_burst_font() -> String {
    "Anton".into()
}

/// Where each layer landed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Placed {
    pub id: String,
    pub kind: String,
    /// x, y, w, h in canvas pixels.
    pub bbox: [u32; 4],
    /// For text and logos: the colour drawn, for legibility checks.
    #[serde(default)]
    pub color: Option<String>,
    /// For text: font size in px.
    #[serde(default)]
    pub size: Option<f32>,
    /// For images and logos: the file drawn.
    #[serde(default)]
    pub src: Option<String>,
}

pub struct Rendered {
    pub image: RgbaImage,
    pub placed: Vec<Placed>,
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn resolve_path(scene: &Scene, src: &str) -> PathBuf {
    let p = PathBuf::from(src);
    if p.is_absolute() {
        return p;
    }
    match &scene.base_dir {
        Some(base) => Path::new(base).join(p),
        None => p,
    }
}

fn data_uri_png(image: &RgbaImage) -> Result<String> {
    let bytes = crate::encode_full(image)?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

fn load_image(scene: &Scene, src: &str) -> Result<RgbaImage> {
    let path = resolve_path(scene, src);
    Ok(image::open(&path)
        .map_err(|e| KataError::Invalid(format!("cannot open image {}: {e}", path.display())))?
        .to_rgba8())
}

fn burst_points(cx: f32, cy: f32, size: f32, points: u32, inner: f32) -> String {
    let n = points.clamp(5, 64) * 2;
    (0..n)
        .map(|i| {
            let r = if i % 2 == 0 {
                size / 2.0
            } else {
                size / 2.0 * inner.clamp(0.3, 0.98)
            };
            let a = std::f32::consts::PI * i as f32 / (n / 2) as f32 - std::f32::consts::FRAC_PI_2;
            format!("{:.2},{:.2}", cx + r * a.cos(), cy + r * a.sin())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the SVG for a scene. Exposed so a caller can inspect or hand-edit it.
pub fn to_svg(scene: &Scene) -> Result<(String, Vec<(String, String)>)> {
    if scene.width == 0 || scene.height == 0 || scene.width > 10_000 || scene.height > 10_000 {
        return Err(KataError::Invalid(format!(
            "a scene of {}x{} is not drawable",
            scene.width, scene.height
        )));
    }
    let (w, h) = (scene.width, scene.height);
    let mut body = String::new();
    let mut defs = String::new();
    let mut ids: Vec<(String, String)> = Vec::new();
    for (index, layer) in scene.layers.iter().enumerate() {
        let auto = |id: &Option<String>, kind: &str| {
            id.clone().unwrap_or_else(|| format!("{kind}-{index}"))
        };
        match layer {
            Layer::Rect {
                id,
                x,
                y,
                w,
                h,
                fill,
                radius,
                opacity,
            } => {
                let id = auto(id, "rect");
                body += &format!(
                    r#"<rect id="{}" x="{x}" y="{y}" width="{w}" height="{h}" rx="{radius}" fill="{}" opacity="{opacity}"/>"#,
                    esc(&id),
                    esc(fill)
                );
                ids.push((id, "rect".into()));
            }
            Layer::Circle {
                id,
                cx,
                cy,
                r,
                fill,
                stroke,
                stroke_width,
                dash,
            } => {
                let id = auto(id, "circle");
                body += &format!(
                    r#"<circle id="{}" cx="{cx}" cy="{cy}" r="{r}" fill="{}" stroke="{}" stroke-width="{stroke_width}" stroke-linecap="round" {}/>"#,
                    esc(&id),
                    esc(fill.as_deref().unwrap_or("none")),
                    esc(stroke.as_deref().unwrap_or("none")),
                    dash.as_ref()
                        .map(|d| format!(r#"stroke-dasharray="{}""#, esc(d)))
                        .unwrap_or_default()
                );
                ids.push((id, "circle".into()));
            }
            Layer::Image {
                id,
                src,
                x,
                y,
                w: iw,
                h: ih,
                opacity,
                clip_circle,
                rotate,
                blend,
            } => {
                let id = auto(id, "image");
                let img = load_image(scene, src)?;
                let ih = ih.unwrap_or(iw * img.height() as f32 / img.width().max(1) as f32);
                let clip = if *clip_circle {
                    let cid = format!("clip-{index}");
                    defs += &format!(
                        r#"<clipPath id="{cid}"><circle cx="{}" cy="{}" r="{}"/></clipPath>"#,
                        x + iw / 2.0,
                        y + ih / 2.0,
                        iw.min(ih) / 2.0
                    );
                    format!(r#" clip-path="url(#{cid})""#)
                } else {
                    String::new()
                };
                let style = blend
                    .as_ref()
                    .map(|b| format!(r#" style="mix-blend-mode:{}""#, esc(b)))
                    .unwrap_or_default();
                body += &format!(
                    r#"<g{clip}{style}><image id="{}" x="{x}" y="{y}" width="{iw}" height="{ih}" opacity="{opacity}" preserveAspectRatio="none" transform="rotate({rotate} {} {})" href="{}"/></g>"#,
                    esc(&id),
                    x + iw / 2.0,
                    y + ih / 2.0,
                    data_uri_png(&img)?
                );
                ids.push((id, "image".into()));
            }
            Layer::Logo {
                id,
                src,
                x,
                y,
                w: lw,
                color,
            } => {
                let id = auto(id, "logo");
                let mut img = load_image(scene, src)?;
                if let Some(c) = color {
                    let ink = Rgba::from_hex(c)?;
                    for p in img.pixels_mut() {
                        p.0[0] = ink.r;
                        p.0[1] = ink.g;
                        p.0[2] = ink.b;
                    }
                }
                let lh = lw * img.height() as f32 / img.width().max(1) as f32;
                body += &format!(
                    r#"<image id="{}" x="{x}" y="{y}" width="{lw}" height="{lh}" preserveAspectRatio="none" href="{}"/>"#,
                    esc(&id),
                    data_uri_png(&img)?
                );
                ids.push((id, "logo".into()));
            }
            Layer::Text {
                id,
                text,
                x,
                y,
                font,
                size,
                weight,
                color,
                anchor,
                letter_spacing,
                line_height,
                uppercase,
                rotate,
                opacity,
                vertical,
            } => {
                let id = auto(id, "text");
                let anchor = match anchor {
                    Anchor::Start => "start",
                    Anchor::Middle => "middle",
                    Anchor::End => "end",
                };
                let content = if *uppercase {
                    text.to_uppercase()
                } else {
                    text.clone()
                };
                let mut spans = String::new();
                if *vertical {
                    // One glyph per line, stacked: how obi strips set Latin type.
                    for (i, ch) in content.chars().enumerate() {
                        let dy = if i == 0 {
                            0.0
                        } else {
                            size * line_height.max(0.5)
                        };
                        spans += &format!(
                            r#"<tspan x="{x}" dy="{dy}">{}</tspan>"#,
                            esc(&ch.to_string())
                        );
                    }
                } else {
                    for (i, line) in content.split('\n').enumerate() {
                        let dy = if i == 0 { 0.0 } else { size * line_height };
                        spans += &format!(r#"<tspan x="{x}" dy="{dy}">{}</tspan>"#, esc(line));
                    }
                }
                body += &format!(
                    r#"<text id="{}" x="{x}" y="{y}" font-family="{}" font-size="{size}" font-weight="{weight}" fill="{}" text-anchor="{anchor}" letter-spacing="{letter_spacing}" opacity="{opacity}" transform="rotate({rotate} {x} {y})">{spans}</text>"#,
                    esc(&id),
                    esc(font),
                    esc(color)
                );
                ids.push((id, "text".into()));
            }
            Layer::Burst {
                id,
                cx,
                cy,
                size,
                fill,
                points,
                inner,
                text,
                color,
                font,
                rotate,
            } => {
                let id = auto(id, "burst");
                let lines: Vec<&str> = text.split('\n').filter(|l| !l.is_empty()).collect();
                let fs = size * 0.2;
                let top = cy - (lines.len() as f32 - 1.0) * fs * 0.5 + fs * 0.35;
                let words: String = lines
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        format!(
                            r#"<tspan x="{cx}" dy="{}">{}</tspan>"#,
                            if i == 0 { 0.0 } else { fs * 0.98 },
                            esc(&l.to_uppercase())
                        )
                    })
                    .collect();
                body += &format!(
                    r#"<g id="{}" transform="rotate({rotate} {cx} {cy})"><polygon points="{}" fill="{}"/><text x="{cx}" y="{top}" font-family="{}" font-size="{fs}" fill="{}" text-anchor="middle">{words}</text></g>"#,
                    esc(&id),
                    burst_points(*cx, *cy, *size, *points, *inner),
                    esc(fill),
                    esc(font),
                    esc(color)
                );
                ids.push((id, "burst".into()));
            }
            Layer::Svg { id, markup } => {
                let id = auto(id, "svg");
                body += &format!(r#"<g id="{}">{markup}</g>"#, esc(&id));
                ids.push((id, "svg".into()));
            }
        }
    }
    if scene.finish.halftone > 0.0 {
        let c = scene.finish.halftone_cell.max(2.0);
        defs += &format!(
            r##"<pattern id="kata-dots" width="{c}" height="{c}" patternUnits="userSpaceOnUse" patternTransform="rotate(15)"><circle cx="{}" cy="{}" r="{}" fill="#000"/></pattern>"##,
            c / 2.0,
            c / 2.0,
            c * 0.23
        );
        body += &format!(
            r#"<rect x="0" y="0" width="{w}" height="{h}" fill="url(#kata-dots)" opacity="{}" style="mix-blend-mode:multiply"/>"#,
            scene.finish.halftone.clamp(0.0, 1.0)
        );
    }
    if scene.finish.paper > 0.0 {
        let paper = kata_core::texture::paper(w, h, scene.finish.seed, 1.0);
        let rgba = image::DynamicImage::ImageLuma8(paper).to_rgba8();
        body += &format!(
            r#"<image x="0" y="0" width="{w}" height="{h}" opacity="{}" style="mix-blend-mode:overlay" href="{}"/>"#,
            scene.finish.paper.clamp(0.0, 1.0),
            data_uri_png(&rgba)?
        );
    }
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><defs>{defs}</defs><rect width="{w}" height="{h}" fill="{}"/>{body}</svg>"#,
        esc(&scene.background)
    );
    Ok((svg, ids))
}

pub fn render(scene: &Scene) -> Result<Rendered> {
    let (svg, ids) = to_svg(scene)?;
    let mut options = resvg::usvg::Options::default();
    {
        let db = options.fontdb_mut();
        db.load_system_fonts();
        for font in &scene.fonts {
            let path = resolve_path(scene, font);
            db.load_font_file(&path).map_err(|e| {
                KataError::Invalid(format!("cannot load font {}: {e}", path.display()))
            })?;
        }
    }
    let tree = resvg::usvg::Tree::from_str(&svg, &options)
        .map_err(|e| KataError::Invalid(format!("the scene did not build: {e}")))?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(scene.width, scene.height)
        .ok_or_else(|| KataError::Invalid("cannot allocate the canvas".into()))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    // tiny-skia stores premultiplied alpha; the canvas is opaque so it is plain RGBA.
    let image = RgbaImage::from_raw(scene.width, scene.height, pixmap.data().to_vec())
        .expect("pixmap size");

    let by_id: std::collections::HashMap<String, &Layer> = scene
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| (ids[i].0.clone(), l))
        .collect();
    let placed = ids
        .iter()
        .filter_map(|(id, kind)| {
            let node = tree.node_by_id(id)?;
            let b = node.abs_bounding_box()?;
            let (x0, y0) = (b.x().max(0.0), b.y().max(0.0));
            let (x1, y1) = (
                (b.x() + b.width()).min(scene.width as f32),
                (b.y() + b.height()).min(scene.height as f32),
            );
            if x1 <= x0 || y1 <= y0 {
                return None;
            }
            let layer = by_id.get(id);
            let (color, size, src) = match layer {
                Some(Layer::Text { color, size, .. }) => (Some(color.clone()), Some(*size), None),
                Some(Layer::Logo { color, src, .. }) => (
                    color.clone(),
                    None,
                    Some(resolve_path(scene, src).display().to_string()),
                ),
                Some(Layer::Image { src, .. }) => (
                    None,
                    None,
                    Some(resolve_path(scene, src).display().to_string()),
                ),
                Some(Layer::Burst { color, .. }) => (Some(color.clone()), None, None),
                _ => (None, None, None),
            };
            Some(Placed {
                id: id.clone(),
                kind: kind.clone(),
                bbox: [
                    x0 as u32,
                    y0 as u32,
                    (x1 - x0).ceil() as u32,
                    (y1 - y0).ceil() as u32,
                ],
                color,
                size,
                src,
            })
        })
        .collect();
    Ok(Rendered { image, placed })
}

/// Fonts Kata can find without being told (for error messages and the MCP tool).
pub fn font_families() -> Vec<String> {
    let mut db = resvg::usvg::fontdb::Database::new();
    db.load_system_fonts();
    let mut names: Vec<String> = db
        .faces()
        .flat_map(|f| f.families.iter().map(|(n, _)| n.clone()))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Where scenes keep generated helper files.
pub fn scratch_dir() -> PathBuf {
    runtime::data_dir().join("scratch")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(json: &str) -> Scene {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_rect_scene_renders_its_colours() {
        let s = scene(
            r##"{"width":40,"height":30,"background":"#112233","layers":[{"type":"rect","id":"box","x":10,"y":10,"w":10,"h":10,"fill":"#FF0000"}]}"##,
        );
        let out = render(&s).unwrap();
        assert_eq!(out.image.get_pixel(2, 2).0, [0x11, 0x22, 0x33, 255]);
        assert_eq!(out.image.get_pixel(15, 15).0, [255, 0, 0, 255]);
        let b = out.placed.iter().find(|p| p.id == "box").unwrap();
        assert_eq!(b.bbox, [10, 10, 10, 10]);
    }

    #[test]
    fn a_burst_is_drawn_and_placed() {
        let s = scene(
            r##"{"width":100,"height":100,"layers":[{"type":"burst","id":"b","cx":50,"cy":50,"size":60,"fill":"#F5C518"}]}"##,
        );
        let out = render(&s).unwrap();
        assert_eq!(out.image.get_pixel(50, 50).0[..3], [0xF5, 0xC5, 0x18]);
        assert!(out.placed.iter().any(|p| p.id == "b"));
    }

    #[test]
    fn the_paper_finish_textures_a_flat_ground() {
        let s =
            scene(r##"{"width":120,"height":120,"background":"#F05A47","finish":{"paper":0.9}}"##);
        let out = render(&s).unwrap();
        let distinct: std::collections::HashSet<[u8; 4]> =
            out.image.pixels().map(|p| p.0).collect();
        assert!(distinct.len() > 20, "{}", distinct.len());
    }

    #[test]
    fn text_is_escaped_and_bad_sizes_are_refused() {
        let s = scene(
            r##"{"width":10,"height":10,"layers":[{"type":"text","text":"a<b & c","x":0,"y":5,"font":"Arial","size":4,"color":"#000"}]}"##,
        );
        let (svg, _) = to_svg(&s).unwrap();
        assert!(svg.contains("a&lt;b &amp; c"));
        assert!(to_svg(&scene(r#"{"width":0,"height":10}"#)).is_err());
    }
}
