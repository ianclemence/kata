//! Review a rendered scene: the checks a careful designer runs before posting.

use crate::compose::{Placed, Rendered};
use image::RgbaImage;
use kata_core::brand::Brand;
use kata_core::color::Rgba;
use kata_core::inspect::{self, Finding, Severity};
use kata_core::lab::Lab;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Review {
    pub findings: Vec<Finding>,
    pub errors: usize,
    pub warnings: usize,
}

impl Review {
    pub fn new(mut findings: Vec<Finding>) -> Review {
        findings.sort_by_key(|f| match f.severity {
            Severity::Error => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
        });
        let errors = findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count();
        let warnings = findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count();
        Review {
            findings,
            errors,
            warnings,
        }
    }
}

/// The colour behind an element: the most common colour in its box that is not
/// the element's own colour.
pub fn ground_under(image: &RgbaImage, bbox: [u32; 4], own: Option<Rgba>) -> Option<Rgba> {
    let [x, y, w, h] = bbox;
    let own = own.map(Lab::from_rgba);
    let mut counts: std::collections::HashMap<[u8; 3], u32> = std::collections::HashMap::new();
    let step = ((w * h) / 4000).max(1);
    let mut i = 0;
    for yy in y..(y + h).min(image.height()) {
        for xx in x..(x + w).min(image.width()) {
            i += 1;
            if i % step != 0 {
                continue;
            }
            let p = image.get_pixel(xx, yy).0;
            // Quantise a little so paper grain does not split one colour into hundreds.
            let key = [p[0] & 0xF8, p[1] & 0xF8, p[2] & 0xF8];
            if let Some(o) = own {
                if Lab::from_rgb(p[0], p[1], p[2]).delta_e(&o) < 14.0 {
                    continue;
                }
            }
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(c, _)| Rgba::rgb(c[0] | 4, c[1] | 4, c[2] | 4))
}

fn alpha_cover(placed: &Placed, other: &Placed) -> f32 {
    // Share of `placed`'s box covered by the other element.
    let [ax, ay, aw, ah] = placed.bbox;
    let [bx, by, bw, bh] = other.bbox;
    let (x0, y0) = (ax.max(bx), ay.max(by));
    let (x1, y1) = ((ax + aw).min(bx + bw), (ay + ah).min(by + bh));
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    let inter = ((x1 - x0) * (y1 - y0)) as f32;
    let box_share = inter / (aw * ah).max(1) as f32;
    // Stickers and other drawn shapes: the box is the shape.
    let Some(src) = &other.src else {
        return box_share;
    };
    let Ok(img) = image::open(src) else {
        return box_share;
    };
    let img = img.to_rgba8();
    // Type set over a full-bleed, opaque photo is overprinting, not a collision:
    // legibility judges that. Only pictures with transparency (cutouts, logos)
    // have a subject that type can run into.
    if img.pixels().all(|p| p.0[3] >= 250) {
        return 0.0;
    }
    let mut hit = 0u32;
    let mut n = 0u32;
    for y in (y0..y1).step_by(2) {
        for x in (x0..x1).step_by(2) {
            let sx = ((x - bx) as f32 / bw as f32 * img.width() as f32) as u32;
            let sy = ((y - by) as f32 / bh as f32 * img.height() as f32) as u32;
            n += 1;
            if img
                .get_pixel(sx.min(img.width() - 1), sy.min(img.height() - 1))
                .0[3]
                > 64
            {
                hit += 1;
            }
        }
    }
    (hit as f32 / n.max(1) as f32) * box_share
}

/// Review a rendered scene. `target` is a target id ("ig-portrait"); `brand`
/// adds the brand's logo-size rules.
pub fn review(rendered: &Rendered, target: Option<&str>, brand: Option<&Brand>) -> Review {
    let mut findings = Vec::new();
    let image = &rendered.image;
    let texts: Vec<&Placed> = rendered
        .placed
        .iter()
        .filter(|p| p.kind == "text")
        .collect();
    let art: Vec<&Placed> = rendered
        .placed
        .iter()
        .filter(|p| matches!(p.kind.as_str(), "image" | "logo" | "burst"))
        .collect();

    // 1. Type that runs into pictures, logos and stickers. For pictures with
    //    transparency only opaque pixels count: text beside a cutout is fine.
    for t in &texts {
        for a in &art {
            let cover = alpha_cover(t, a);
            if cover > 0.06 {
                findings.push(Finding {
                    check: "text_collision".into(),
                    severity: if cover > 0.2 { Severity::Error } else { Severity::Warning },
                    message: format!("`{}` runs into `{}` ({:.0}% of the text's box). Move or resize one of them.", t.id, a.id, cover * 100.0),
                    bbox: Some(t.bbox),
                });
            }
        }
    }
    // 2. Text boxes overlapping each other.
    let boxes: Vec<(String, [u32; 4])> = texts.iter().map(|t| (t.id.clone(), t.bbox)).collect();
    findings.extend(inspect::overlaps(&boxes, 0.08).into_iter().map(|mut f| {
        f.check = "text_overlap".into();
        f
    }));
    // 3. Legibility of type and logos against what is actually behind them.
    for p in rendered
        .placed
        .iter()
        .filter(|p| p.kind == "text" || p.kind == "logo")
    {
        let colour = match (&p.color, p.kind.as_str()) {
            (Some(c), _) => Rgba::from_hex(c).ok(),
            (None, "logo") => p
                .src
                .as_ref()
                .and_then(|s| image::open(s).ok())
                .and_then(|img| {
                    kata_core::brand::extract_palette(&img.to_rgba8(), 3)
                        .first()
                        .map(|c| c.0)
                }),
            _ => None,
        };
        let Some(colour) = colour else { continue };
        if let Some(ground) = ground_under(image, p.bbox, Some(colour)) {
            let large = p.kind == "logo" || p.size.unwrap_or(0.0) >= 24.0;
            if let Some(f) =
                inspect::legibility(&format!("`{}`", p.id), colour, ground, large, Some(p.bbox))
            {
                findings.push(f);
            }
        }
    }
    // 4. Logo size against the brand's rules.
    if let Some(brand) = brand {
        for logo in rendered.placed.iter().filter(|p| p.kind == "logo") {
            let share = logo.bbox[2] as f32 / image.width() as f32;
            if share > brand.logos.max_width || share < brand.logos.min_width {
                findings.push(Finding {
                    check: "logo_size".into(),
                    severity: Severity::Warning,
                    message: format!(
                        "`{}` is {:.0}% of the width; {}'s logo should be {:.0}–{:.0}%.",
                        logo.id,
                        share * 100.0,
                        brand.name,
                        brand.logos.min_width * 100.0,
                        brand.logos.max_width * 100.0
                    ),
                    bbox: Some(logo.bbox),
                });
            }
        }
    }
    // 5. Platform safe areas.
    if let Some(t) = target.and_then(kata_core::targets::by_id) {
        if (t.width, t.height) != image.dimensions() {
            findings.push(Finding {
                check: "target_size".into(),
                severity: Severity::Error,
                message: format!(
                    "the canvas is {:?} but {} is {}x{}.",
                    image.dimensions(),
                    t.id,
                    t.width,
                    t.height
                ),
                bbox: None,
            });
        } else {
            let important: Vec<(String, [u32; 4])> = rendered
                .placed
                .iter()
                .filter(|p| matches!(p.kind.as_str(), "text" | "logo" | "burst"))
                .map(|p| (p.id.clone(), p.bbox))
                .collect();
            findings.extend(inspect::safe_area(t, &important));
        }
    }
    Review::new(findings)
}

/// Save a crop of each finding (2x) so a reviewer can see exactly what it means.
pub fn crops(
    image: &RgbaImage,
    findings: &[Finding],
    dir: &std::path::Path,
    stem: &str,
) -> Vec<std::path::PathBuf> {
    let _ = std::fs::create_dir_all(dir);
    findings
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let [x, y, w, h] = f.bbox?;
            let pad = 24;
            let (x0, y0) = (x.saturating_sub(pad), y.saturating_sub(pad));
            let (x1, y1) = (
                (x + w + pad).min(image.width()),
                (y + h + pad).min(image.height()),
            );
            let crop = image::imageops::crop_imm(image, x0, y0, x1 - x0, y1 - y0).to_image();
            let scaled = image::imageops::resize(
                &crop,
                crop.width() * 2,
                crop.height() * 2,
                image::imageops::FilterType::Nearest,
            );
            let path = dir.join(format!("{stem}-{i:02}-{}.png", f.check));
            scaled.save(&path).ok().map(|_| path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::{render, Scene};
    use std::path::Path;

    #[test]
    fn a_coral_logo_on_coral_and_colliding_text_are_caught() {
        let scene: Scene = serde_json::from_str(
            r##"{"width":300,"height":200,"background":"#F05A47","layers":[
                {"type":"rect","id":"logo-plate","x":10,"y":10,"w":60,"h":20,"fill":"#F25C49"},
                {"type":"text","id":"headline","text":"HELLO","x":10,"y":120,"font":"Arial","size":60,"color":"#F9ECDD"},
                {"type":"text","id":"deck","text":"overlapping","x":20,"y":110,"font":"Arial","size":30,"color":"#F9ECDD"}
            ]}"##,
        )
        .unwrap();
        let rendered = render(&scene).unwrap();
        let review = review(&rendered, None, None);
        assert!(
            review.findings.iter().any(|f| f.check == "text_overlap"),
            "{:?}",
            review.findings
        );
    }

    #[test]
    fn collisions_mean_type_hitting_a_subject_not_type_over_a_photo() {
        let dir = std::env::temp_dir().join(format!("kata-review-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("photo.png");
        RgbaImage::from_pixel(100, 100, image::Rgba([90, 120, 160, 255]))
            .save(&photo)
            .unwrap();
        let cutout = dir.join("cutout.png");
        let mut c = RgbaImage::new(100, 100);
        for y in 0..100 {
            for x in 0..50 {
                c.put_pixel(x, y, image::Rgba([20, 20, 20, 255]));
            }
        }
        c.save(&cutout).unwrap();
        let place = |id: &str, kind: &str, bbox: [u32; 4], src: Option<&Path>| Placed {
            id: id.into(),
            kind: kind.into(),
            bbox,
            color: None,
            size: None,
            src: src.map(|p| p.display().to_string()),
        };
        let text = place("headline", "text", [10, 10, 30, 20], None);
        assert_eq!(
            alpha_cover(
                &text,
                &place("photo", "image", [0, 0, 100, 100], Some(&photo))
            ),
            0.0
        );
        assert!(
            alpha_cover(
                &text,
                &place("subject", "image", [0, 0, 100, 100], Some(&cutout))
            ) > 0.9
        );
        assert_eq!(
            alpha_cover(&text, &place("sticker", "burst", [60, 60, 30, 30], None)),
            0.0
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wrong_canvas_for_the_target_is_an_error() {
        let scene: Scene = serde_json::from_str(r##"{"width":100,"height":100}"##).unwrap();
        let review = review(&render(&scene).unwrap(), Some("ig-portrait"), None);
        assert_eq!(review.errors, 1);
    }

    #[test]
    fn ground_under_skips_the_text_colour() {
        let mut img = RgbaImage::from_pixel(20, 20, image::Rgba([240, 90, 70, 255]));
        for x in 5..15 {
            img.put_pixel(x, 10, image::Rgba([250, 236, 221, 255]));
        }
        let g = ground_under(&img, [0, 0, 20, 20], Some(Rgba::rgb(250, 236, 221))).unwrap();
        assert!(Lab::from_rgba(g).delta_e(&Lab::from_rgb(240, 90, 70)) < 5.0);
    }
}
