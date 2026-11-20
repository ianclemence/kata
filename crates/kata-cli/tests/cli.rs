//! End-to-end tests for the `kata` binary: a real PNG in, a real cutout out.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use image::{Rgba, RgbaImage};

fn kata() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kata"))
}

fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kata-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

/// A light background with a dark square subject in the centre, 64x64.
fn synthetic_png(path: &Path) {
    let mut image = RgbaImage::from_pixel(64, 64, Rgba([230, 230, 230, 255]));
    for y in 20..44 {
        for x in 20..44 {
            image.put_pixel(x, y, Rgba([30, 30, 30, 255]));
        }
    }
    image.save(path).expect("write the synthetic PNG");
}

fn run(command: &mut Command) -> Output {
    command.output().expect("run the kata binary")
}

#[test]
fn photo_in_cutout_out() {
    let dir = test_dir("cutout");
    let input = dir.join("input.png");
    let output = dir.join("out.png");
    synthetic_png(&input);

    let result = run(kata().arg(&input).arg("-o").arg(&output));
    assert!(
        result.status.success(),
        "expected success, got {:?}; stderr: {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.exists(), "the cutout was not written");

    let cutout = image::open(&output).expect("decode the cutout").to_rgba8();
    let (width, height) = cutout.dimensions();
    assert!(width > 0 && height > 0);
    assert_eq!(
        cutout.get_pixel(0, 0).0[3],
        0,
        "the corner is not transparent"
    );
    assert_eq!(
        cutout.get_pixel(width / 2, height / 2).0[3],
        255,
        "the subject is not opaque"
    );

    let alphas: Vec<u8> = cutout.pixels().map(|pixel| pixel.0[3]).collect();
    assert!(alphas.contains(&0), "no pixel is fully transparent");
    assert!(alphas.contains(&255), "no pixel is fully opaque");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn onnx_segmenter_without_a_runtime_is_an_honest_error() {
    let dir = test_dir("onnx");
    let input = dir.join("input.png");
    let output = dir.join("out.png");
    let absent = dir.join("absent.onnx");
    synthetic_png(&input);

    let result = run(kata()
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--segmenter")
        .arg("onnx")
        .arg("--model")
        .arg(&absent));

    assert!(!result.status.success(), "onnx must not silently succeed");
    assert_eq!(result.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains(&absent.to_string_lossy().into_owned()),
        "stderr does not name the expected model path: {stderr}"
    );
    assert!(
        stderr.contains("luma"),
        "stderr does not say what to do instead: {stderr}"
    );
    assert!(!output.exists(), "a failed run must not write a cutout");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn help_lists_the_segmenter_option() {
    let result = run(kata().arg("--help"));
    assert!(result.status.success());
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("--segmenter"), "help output: {stdout}");
}
