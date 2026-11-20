//! The primitives the sticker style is built from: lightness, histograms,
//! CLAHE, a bilateral smooth, and a resampler.
//!
//! The resampler is our own separable Lanczos-3 rather than a SIMD library on
//! purpose: the golden-image tests need the same bytes on Windows, macOS, Linux
//! and WebAssembly, and a kernel that changes with the CPU's instruction set
//! cannot promise that. See README, "Resampling".

use crate::parallel::for_each_row_mut;
use image::{GrayImage, Luma, RgbaImage};

/// Perceptual lightness of every pixel, 0..=255.
pub fn to_luma(image: &RgbaImage) -> GrayImage {
    let (width, height) = image.dimensions();
    let mut out = GrayImage::new(width, height);
    let source = image.as_raw();
    for (index, pixel) in out.as_mut().iter_mut().enumerate() {
        let p = &source[index * 4..index * 4 + 4];
        // Rec. 709, in integer arithmetic to stay deterministic.
        let y = (2126 * u32::from(p[0]) + 7152 * u32::from(p[1]) + 722 * u32::from(p[2]) + 5000)
            / 10000;
        *pixel = y as u8;
    }
    out
}

pub fn histogram(plane: &GrayImage) -> [u32; 256] {
    let mut hist = [0u32; 256];
    for &value in plane.as_raw() {
        hist[value as usize] += 1;
    }
    hist
}

/// The value below which `fraction` of pixels fall. A fraction of 0.0 is the lowest value present, not the value before the first bin, so the search always lands on one.
pub fn percentile(hist: &[u32; 256], fraction: f32) -> u8 {
    let total: u64 = hist.iter().map(|&c| u64::from(c)).sum();
    if total == 0 {
        return 0;
    }
    let target = ((f64::from(fraction.clamp(0.0, 1.0)) * total as f64).ceil() as u64).max(1);
    let mut running = 0u64;
    for (value, &count) in hist.iter().enumerate() {
        running += u64::from(count);
        if running >= target {
            return value as u8;
        }
    }
    255
}

/// Contrast-limited adaptive histogram equalisation.
///
/// Local contrast is what makes a photo read as a screen print; a global curve
/// cannot lift the shadows in one corner without blowing the highlights on the
/// other. `tiles` is the grid size per axis, `clip` the limit as a multiple of
/// the tile's average bin count.
pub fn clahe(plane: &GrayImage, tiles: u32, clip: f32) -> GrayImage {
    let (width, height) = plane.dimensions();
    let tiles = tiles.max(1);
    if width < tiles || height < tiles {
        return plane.clone();
    }

    let tile_w = width.div_ceil(tiles);
    let tile_h = height.div_ceil(tiles);
    let cols = width.div_ceil(tile_w);
    let rows = height.div_ceil(tile_h);

    // One mapping table per tile.
    let mut maps = vec![vec![0u8; 256]; (cols * rows) as usize];
    for tile_y in 0..rows {
        for tile_x in 0..cols {
            let x0 = tile_x * tile_w;
            let y0 = tile_y * tile_h;
            let x1 = (x0 + tile_w).min(width);
            let y1 = (y0 + tile_h).min(height);
            let mut hist = [0u32; 256];
            let mut count = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    hist[plane.get_pixel(x, y).0[0] as usize] += 1;
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }

            // Clip the histogram and spread the excess evenly, which is what
            // stops noise in a flat tile from being amplified into contours.
            // The limit is `clip` times the average count of an occupied bin.
            // Averaging over all 256 bins would make a tile with a few values
            // clip nearly everything, and spreading that excess over the empty
            // bins invents contrast a flat tile does not have.
            let occupied = hist.iter().filter(|&&bin| bin > 0).count().max(1) as f32;
            let limit = ((count as f32 / occupied) * clip).max(1.0) as u32;
            let mut excess = 0u32;
            for bin in &mut hist {
                if *bin > limit {
                    excess += *bin - limit;
                    *bin = limit;
                }
            }
            let share = excess / 256;
            let remainder = excess % 256;
            for (bin, value) in hist.iter_mut().enumerate() {
                *value += share + u32::from((bin as u32) < remainder);
            }

            let mut cumulative = 0u32;
            let mut map = [0u8; 256];
            for (value, &bin) in hist.iter().enumerate() {
                cumulative += bin;
                let scaled = (cumulative as f32 / count as f32) * 255.0;
                map[value] = scaled.round().clamp(0.0, 255.0) as u8;
            }
            maps[(tile_y * cols + tile_x) as usize] = map.to_vec();
        }
    }

    // Bilinear interpolation between the four surrounding tile mappings keeps
    // the tile grid from showing as blocks.
    let mut out = GrayImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let fx = (x as f32 / tile_w as f32) - 0.5;
            let fy = (y as f32 / tile_h as f32) - 0.5;
            let tx0 = fx.floor();
            let ty0 = fy.floor();
            let wx = fx - tx0;
            let wy = fy - ty0;
            let tx0 = (tx0 as i32).clamp(0, cols as i32 - 1) as u32;
            let ty0 = (ty0 as i32).clamp(0, rows as i32 - 1) as u32;
            let tx1 = (tx0 + 1).min(cols - 1);
            let ty1 = (ty0 + 1).min(rows - 1);
            let value = plane.get_pixel(x, y).0[0] as f32;

            let sample = |tx: u32, ty: u32| -> f32 {
                f32::from(maps[(ty * cols + tx) as usize][value as usize])
            };
            let top = sample(tx0, ty0) * (1.0 - wx) + sample(tx1, ty0) * wx;
            let bottom = sample(tx0, ty1) * (1.0 - wx) + sample(tx1, ty1) * wx;
            out.put_pixel(x, y, Luma([(top * (1.0 - wy) + bottom * wy).round() as u8]));
        }
    }
    out
}

/// Edge-preserving smoothing. Weighted by lightness distance so that a face and
/// the wall behind it do not average into each other, which is what a plain
/// Gaussian would do and what would make the posterise step look like a smear.
pub fn bilateral(image: &RgbaImage, radius: u32, sigma_space: f32, sigma_color: f32) -> RgbaImage {
    let (width, height) = image.dimensions();
    if radius == 0 {
        return image.clone();
    }
    let source = image.as_raw();
    // A row of the RGBA buffer is four bytes per pixel; the luma plane below
    // indexes with the width alone.
    let stride = width as usize * 4;
    let radius_i = radius as i32;

    // Spatial weights only depend on distance.
    let side = (radius * 2 + 1) as usize;
    let mut spatial = vec![0f32; side * side];
    for dy in -radius_i..=radius_i {
        for dx in -radius_i..=radius_i {
            let d2 = (dx * dx + dy * dy) as f32;
            spatial[(dy + radius_i) as usize * side + (dx + radius_i) as usize] =
                (-d2 / (2.0 * sigma_space * sigma_space)).exp();
        }
    }
    // Colour weights are a function of the 0..255 lightness difference.
    let color_lut: Vec<f32> = (0..=255)
        .map(|d| {
            let d = d as f32;
            (-(d * d) / (2.0 * sigma_color * sigma_color)).exp()
        })
        .collect();

    let gray = to_luma(image);
    let gray = gray.as_raw();

    let mut out = image.clone();
    let dest: &mut [u8] = out.as_mut();

    for_each_row_mut(dest, stride, |y, row| {
        for (x, pixel) in row.chunks_mut(4).enumerate() {
            let center = gray[y * width as usize + x];
            let mut acc = [0f32; 3];
            let mut weight_sum = 0f32;
            let y0 = (y as i32 - radius_i).max(0) as usize;
            let y1 = ((y as i32 + radius_i) as usize).min(height as usize - 1);
            let x0 = (x as i32 - radius_i).max(0) as usize;
            let x1 = ((x as i32 + radius_i) as usize).min(width as usize - 1);

            for ny in y0..=y1 {
                let dy = ny as i32 - y as i32;
                for nx in x0..=x1 {
                    let dx = nx as i32 - x as i32;
                    let spatial_w = spatial
                        [(dy + radius_i) as usize * side + (dx + radius_i) as usize];
                    let diff = i32::from(gray[ny * width as usize + nx]) - i32::from(center);
                    let color_w = color_lut[diff.unsigned_abs() as usize];
                    let weight = spatial_w * color_w;
                    weight_sum += weight;
                    let index = (ny * width as usize + nx) * 4;
                    for c in 0..3 {
                        acc[c] += f32::from(source[index + c]) * weight;
                    }
                }
            }

            if weight_sum > 0.0 {
                for c in 0..3 {
                    pixel[c] = (acc[c] / weight_sum).round().clamp(0.0, 255.0) as u8;
                }
            }
            pixel[3] = source[(y * width as usize + x) * 4 + 3];
        }
    });
    out
}

/// Lanczos-3 kernel, used by the resampler and by the sticker sheet.
fn lanczos(x: f32) -> f32 {
    if x.abs() < 1e-6 {
        return 1.0;
    }
    if x.abs() >= 3.0 {
        return 0.0;
    }
    let pix = std::f32::consts::PI * x;
    (pix.sin() / pix) * ((pix / 3.0).sin() / (pix / 3.0))
}

fn contribs(source_len: u32, dest_len: u32) -> Vec<Vec<(usize, f32)>> {
    let scale = source_len as f32 / dest_len as f32;
    let support = if scale < 1.0 { 3.0 * scale } else { 3.0 };
    let mut table = Vec::with_capacity(dest_len as usize);
    for i in 0..dest_len {
        let center = (i as f32 + 0.5) * scale - 0.5;
        let start = (center - support).floor() as i32;
        let end = (center + support).ceil() as i32;
        let mut entries = Vec::new();
        let mut total = 0f32;
        for j in start..=end {
            let clamped = j.clamp(0, source_len as i32 - 1) as usize;
            let weight = lanczos((j as f32 - center) / support.max(f32::EPSILON) * 3.0);
            if weight.abs() > 1e-6 {
                entries.push((clamped, weight));
                total += weight;
            }
        }
        if total.abs() > f32::EPSILON {
            for (_, weight) in &mut entries {
                *weight /= total;
            }
        }
        table.push(entries);
    }
    table
}

/// High-quality separable resample of an RGBA image.
pub fn resize(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let (src_w, src_h) = image.dimensions();
    if width == 0 || height == 0 {
        return RgbaImage::new(width.max(1), height.max(1));
    }
    if width == src_w && height == src_h {
        return image.clone();
    }
    let source = image.as_raw();

    // Horizontal pass into f32 at the destination width.
    let x_table = contribs(src_w, width);
    let mut temp = vec![0f32; width as usize * src_h as usize * 4];
    for_each_row_mut(&mut temp, width as usize * 4, |y, row| {
        for (x, out) in row.chunks_mut(4).enumerate() {
            let mut acc = [0f32; 4];
            for &(sx, weight) in &x_table[x] {
                let index = (y * src_w as usize + sx) * 4;
                for c in 0..4 {
                    acc[c] += f32::from(source[index + c]) * weight;
                }
            }
            out.copy_from_slice(&acc);
        }
    });

    // Vertical pass into the result.
    let y_table = contribs(src_h, height);
    let mut out = RgbaImage::new(width, height);
    let dest: &mut [u8] = out.as_mut();
    for_each_row_mut(dest, width as usize * 4, |y, row| {
        for (x, pixel) in row.chunks_mut(4).enumerate() {
            let mut acc = [0f32; 4];
            for &(sy, weight) in &y_table[y] {
                let index = (sy * width as usize + x) * 4;
                for c in 0..4 {
                    acc[c] += temp[index + c] * weight;
                }
            }
            for c in 0..4 {
                pixel[c] = acc[c].round().clamp(0.0, 255.0) as u8;
            }
        }
    });
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn ramp(width: u32, height: u32) -> GrayImage {
        let mut img = GrayImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                img.put_pixel(x, y, Luma([(x * 255 / width.max(1)) as u8]));
            }
        }
        img
    }

    #[test]
    fn luma_follows_rec709() {
        let mut img = RgbaImage::new(1, 1);
        img.put_pixel(0, 0, Rgba([255, 255, 255, 255]));
        assert_eq!(to_luma(&img).get_pixel(0, 0).0[0], 255);

        img.put_pixel(0, 0, Rgba([0, 255, 0, 255]));
        let green = to_luma(&img).get_pixel(0, 0).0[0];
        assert!((170..=182).contains(&green), "green luma was {green}");
    }

    #[test]
    fn percentile_reads_the_distribution() {
        let mut hist = [0u32; 256];
        hist[10] = 50;
        hist[200] = 50;
        assert_eq!(percentile(&hist, 0.0), 10);
        assert_eq!(percentile(&hist, 0.49), 10);
        assert_eq!(percentile(&hist, 0.51), 200);
        assert_eq!(percentile(&hist, 1.0), 200);
    }

    #[test]
    fn clahe_raises_local_contrast_without_clipping_the_range() {
        // Left half dark, right half bright: a global curve would do nothing
        // useful, a tile-local one must still span the full range per tile.
        let mut img = GrayImage::new(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                let v = if x < 32 { 60 + (x % 8) as u8 } else { 190 + (x % 8) as u8 };
                img.put_pixel(x, y, Luma([v]));
            }
        }
        let out = clahe(&img, 4, 2.0);
        let dark_max = (0..32).flat_map(|x| (0..64).map(move |y| (x, y)))
            .map(|(x, y)| out.get_pixel(x, y).0[0])
            .max()
            .unwrap();
        assert!(dark_max > 100, "the dark tile did not gain contrast: {dark_max}");
    }

    #[test]
    fn clahe_is_a_no_op_on_a_flat_image() {
        let img = GrayImage::from_pixel(32, 32, Luma([120]));
        let out = clahe(&img, 4, 2.0);
        // Every tile is uniform, so the mapping must not invent structure.
        let spread = out.as_raw().iter().max().unwrap() - out.as_raw().iter().min().unwrap();
        assert_eq!(spread, 0);
    }

    #[test]
    fn bilateral_keeps_a_step_edge_sharp() {
        let mut img = RgbaImage::new(32, 8);
        for y in 0..8 {
            for x in 0..32 {
                let v = if x < 16 { 20 } else { 235 };
                img.put_pixel(x, y, Rgba([v, v, v, 255]));
            }
        }
        let out = bilateral(&img, 4, 2.0, 20.0);
        assert!(
            out.get_pixel(15, 4).0[0] < 40,
            "the dark side bled across the edge"
        );
        assert!(out.get_pixel(16, 4).0[0] > 200, "the bright side bled back");
    }

    #[test]
    fn resize_keeps_average_brightness() {
        let mut img = RgbaImage::new(64, 64);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let v = ((x + y) % 2 * 200) as u8;
            *p = Rgba([v, v, v, 255]);
        }
        let small = resize(&img, 16, 16);
        assert_eq!(small.dimensions(), (16, 16));
        let mean_in: f64 = img.pixels().map(|p| f64::from(p.0[0])).sum::<f64>() / 4096.0;
        let mean_out: f64 = small.pixels().map(|p| f64::from(p.0[0])).sum::<f64>() / 256.0;
        assert!((mean_in - mean_out).abs() < 3.0, "{mean_in} vs {mean_out}");
    }

    #[test]
    fn resize_preserves_alpha() {
        let img = RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 128]));
        let out = resize(&img, 4, 4);
        assert!(out.pixels().all(|p| p.0[3] == 128));
    }

    #[test]
    fn resize_to_the_same_size_is_identity() {
        let img = RgbaImage::from_pixel(5, 5, Rgba([1, 2, 3, 4]));
        assert_eq!(resize(&img, 5, 5), img);
    }
}
