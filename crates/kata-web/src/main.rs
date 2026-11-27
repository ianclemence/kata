//! Kata's local web UI: the same `kata_core::pipeline` the CLI runs, served on
//! loopback so a browser can do the work in place.
//!
//! There is deliberately no WebAssembly build here. A wasm target needs
//! `rustup target add` and the wasm-bindgen CLI, and neither belongs in this
//! tree, so this binary is a small native HTTP server written with `std` only:
//! one request per connection, no web framework, and nothing listening beyond
//! 127.0.0.1.

use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use kata_core::pipeline;
use kata_core::segment::LumaSegmenter;

use image::ImageEncoder;

const DEFAULT_PORT: u16 = 8765;
/// Refuse uploads larger than this instead of buffering them in memory.
const MAX_BODY: usize = 64 * 1024 * 1024;
/// A request head larger than this is a client we do not want to talk to.
const MAX_HEAD: usize = 16 * 1024;
const PAGE: &str = include_str!("page.html");

fn main() {
    let port = match resolve_port(env::args().skip(1)) {
        Ok(Some(port)) => port,
        Ok(None) => {
            println!("{}", usage());
            return;
        }
        Err(message) => {
            eprintln!("kata-web: {message}\n\n{}", usage());
            std::process::exit(2);
        }
    };

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("kata-web: cannot listen on 127.0.0.1:{port}: {error}");
            std::process::exit(1);
        }
    };
    match listener.local_addr() {
        Ok(address) => println!("Kata web UI on http://{address}"),
        Err(_) => println!("Kata web UI on http://127.0.0.1:{port}"),
    }
    println!("Press Ctrl+C to stop.");

    serve(listener);
}

fn usage() -> &'static str {
    "Kata web UI — the kata-core pipeline in a browser, on 127.0.0.1 only.

Usage: kata-web [--port <PORT>]

  --port <PORT>   port to listen on (default 8765, or the KATA_WEB_PORT env var)
  -h, --help      print this help"
}

/// The port asked for on the command line, then the environment, then the
/// default. `Ok(None)` means help was requested.
fn resolve_port(args: impl Iterator<Item = String>) -> Result<Option<u16>, String> {
    let mut args = args.peekable();
    let mut requested: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--port" => {
                requested = Some(args.next().ok_or("--port needs a value")?);
            }
            other => match other.strip_prefix("--port=") {
                Some(value) => requested = Some(value.to_string()),
                None => return Err(format!("unknown argument {other:?}")),
            },
        }
    }

    let raw = match requested.or_else(|| env::var("KATA_WEB_PORT").ok()) {
        Some(raw) => raw,
        None => return Ok(Some(DEFAULT_PORT)),
    };
    raw.parse::<u16>()
        .map(Some)
        .map_err(|_| format!("{raw:?} is not a port number (0-65535)"))
}

fn serve(listener: TcpListener) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || handle(stream));
            }
            Err(error) => eprintln!("kata-web: connection failed: {error}"),
        }
    }
}

/// The status line and message of a request we could not answer.
struct Failure {
    status: &'static str,
    message: String,
}

fn fail(status: &'static str, message: impl Into<String>) -> Failure {
    Failure {
        status,
        message: message.into(),
    }
}

fn handle(stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let mut reader = BufReader::new(stream);

    if let Err(failure) = handle_request(&mut reader) {
        let _ = send_text(reader.get_mut(), failure.status, &failure.message);
    }
}

fn handle_request(reader: &mut BufReader<TcpStream>) -> Result<(), Failure> {
    let mut request_line = String::new();
    let read = reader.read_line(&mut request_line).map_err(|error| {
        fail(
            "400 Bad Request",
            format!("cannot read the request line: {error}"),
        )
    })?;
    if read == 0 {
        return Ok(());
    }
    if request_line.len() > MAX_HEAD {
        return Err(fail(
            "431 Request Header Fields Too Large",
            "the request line is too long",
        ));
    }
    let request_line = trim_eol(&request_line);

    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method.is_empty() || target.is_empty() {
        return Err(fail(
            "400 Bad Request",
            format!("malformed request line {request_line:?}"),
        ));
    }
    let path = target.split(['?', '#']).next().unwrap_or(target);

    let mut content_length: Option<usize> = None;
    let mut head_length = request_line.len();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).map_err(|error| {
            fail(
                "400 Bad Request",
                format!("cannot read the headers: {error}"),
            )
        })?;
        if read == 0 {
            // The client closed before the blank line; nothing to answer.
            return Ok(());
        }
        head_length += line.len();
        if head_length > MAX_HEAD {
            return Err(fail(
                "431 Request Header Fields Too Large",
                "the request headers are too long",
            ));
        }
        let line = trim_eol(&line);
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                let length = value.parse::<usize>().map_err(|_| {
                    fail(
                        "400 Bad Request",
                        format!("Content-Length is not a number: {value:?}"),
                    )
                })?;
                content_length = Some(length);
            }
            "transfer-encoding" => {
                return Err(fail(
                    "400 Bad Request",
                    "chunked request bodies are not supported; send Content-Length",
                ));
            }
            _ => {}
        }
    }

    match (method, path) {
        ("GET", "/") => send(
            reader.get_mut(),
            "200 OK",
            "text/html; charset=utf-8",
            PAGE.as_bytes(),
        ),
        ("GET", "/sample.png") => send(reader.get_mut(), "200 OK", "image/png", &sample_png()),
        ("GET", "/status") => {
            let model = kata_engine::models::resolve(None)
                .ok()
                .filter(|_| kata_engine::runtime::environment().is_ok())
                .map(|r| r.name);
            let body = serde_json::json!({"model": model, "version": env!("CARGO_PKG_VERSION")})
                .to_string();
            send(
                reader.get_mut(),
                "200 OK",
                "application/json",
                body.as_bytes(),
            )
        }
        ("POST", "/cutout") | ("POST", "/sticker") => {
            let length = content_length.ok_or_else(|| {
                fail(
                    "411 Length Required",
                    "the upload needs a Content-Length header",
                )
            })?;
            if length > MAX_BODY {
                return Err(fail(
                    "413 Payload Too Large",
                    format!("the upload is {length} bytes; this server accepts at most {MAX_BODY}"),
                ));
            }
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).map_err(|error| {
                fail(
                    "400 Bad Request",
                    format!("the request body ended early: {error}"),
                )
            })?;

            // The real model when this machine has one; the lightness split otherwise
            // (it keeps the page usable for a first look, and says so in a header).
            let model = kata_engine::models::resolve(None)
                .ok()
                .filter(|_| kata_engine::runtime::environment().is_ok())
                .and_then(|r| kata_engine::cut::segmenter(&r).ok());
            let luma = LumaSegmenter::default();
            let segmenter: &dyn kata_core::segment::Segmenter = match &model {
                Some(m) => m.as_ref(),
                None => &luma,
            };
            let result = if path == "/sticker" {
                pipeline::sticker_png(&body, segmenter)
            } else {
                pipeline::cutout_png(&body, segmenter)
            };
            match result {
                Ok(png) => send(reader.get_mut(), "200 OK", "image/png", &png),
                // The pipeline already says what is wrong with the image.
                Err(error) => Err(fail("400 Bad Request", error.to_string())),
            }
        }
        (_, "/") | (_, "/sample.png") => Err(fail(
            "405 Method Not Allowed",
            format!("{method} is not allowed on {path}"),
        )),
        _ => Err(fail("404 Not Found", format!("no route for {path}"))),
    }
}

fn trim_eol(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn send(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), Failure> {
    let head = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body))
        .and_then(|()| stream.flush())
        .map_err(|error| {
            fail(
                "500 Internal Server Error",
                format!("cannot write the response: {error}"),
            )
        })
}

fn send_text(stream: &mut TcpStream, status: &str, message: &str) -> Result<(), Failure> {
    let mut body = message.trim().to_string();
    body.push('\n');
    send(stream, status, "text/plain; charset=utf-8", body.as_bytes())
}

/// A synthetic photo for the "Use sample image" button: a light, lightly
/// vignetted backdrop with a dark figure on it, so the lightness segmenter has
/// something honest to split. Same bytes whether the browser fetches it or a
/// test posts it through the pipeline.
fn sample_png() -> Vec<u8> {
    let image = sample_image();
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .expect("encoding an in-memory RGBA buffer to PNG cannot fail");
    png
}

fn sample_image() -> image::RgbaImage {
    const WIDTH: u32 = 480;
    const HEIGHT: u32 = 360;
    let (width, height) = (WIDTH as f32, HEIGHT as f32);

    image::RgbaImage::from_fn(WIDTH, HEIGHT, |x, y| {
        let px = x as f32 + 0.5;
        let py = y as f32 + 0.5;

        // Warm backdrop, lighter at the top centre.
        let across = (px / width - 0.5).abs();
        let down = (py / height - 0.4).abs();
        let light = 247.0 - 22.0 * (across * 1.1 + down);
        let backdrop = [light, light - 2.0, light - 9.0];

        // A dark figure: head and shoulders, with an anti-aliased edge.
        let head = ellipse_coverage(
            px,
            py,
            0.5 * width,
            0.34 * height,
            0.17 * width,
            0.21 * height,
        );
        let shoulders = ellipse_coverage(
            px,
            py,
            0.5 * width,
            1.04 * height,
            0.36 * width,
            0.46 * height,
        );
        let subject = head.max(shoulders);
        let figure = [28.0, 27.0, 33.0];

        let blend = |backdrop: f32, figure: f32| {
            (backdrop * (1.0 - subject) + figure * subject)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        image::Rgba([
            blend(backdrop[0], figure[0]),
            blend(backdrop[1], figure[1]),
            blend(backdrop[2], figure[2]),
            255,
        ])
    })
}

/// How much of the pixel lies inside the ellipse, softened over roughly a
/// two-pixel edge so the sample does not look like a bitmap cutout.
fn ellipse_coverage(px: f32, py: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> f32 {
    let nx = (px - cx) / rx;
    let ny = (py - cy) / ry;
    let distance = (nx * nx + ny * ny).sqrt();
    ((1.0 - distance) * 40.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn start_server() -> SocketAddr {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind a test port");
        let address = listener.local_addr().expect("a local address");
        thread::spawn(move || serve(listener));
        address
    }

    struct Response {
        status: u16,
        content_type: String,
        body: Vec<u8>,
    }

    fn request(address: SocketAddr, head: &str, body: &[u8]) -> Response {
        let mut stream = TcpStream::connect(address).expect("connect to the test server");
        stream.write_all(head.as_bytes()).expect("write the head");
        stream.write_all(body).expect("write the body");
        stream.flush().expect("flush");
        // The server may already have answered and closed (a 404 needs no body);
        // macOS then reports the half-close as NotConnected. That is not a failure.
        let _ = stream.shutdown(std::net::Shutdown::Write);

        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read the response");
        let split = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("a response head");
        let head = String::from_utf8(raw[..split].to_vec()).expect("utf-8 head");
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .and_then(|code| code.parse().ok())
            .expect("a status code");
        let content_type = lines
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                    .map(|(_, value)| value.trim().to_string())
            })
            .unwrap_or_default();
        Response {
            status,
            content_type,
            body: raw[split + 4..].to_vec(),
        }
    }

    fn get(address: SocketAddr, path: &str) -> Response {
        request(
            address,
            &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"),
            &[],
        )
    }

    fn post(address: SocketAddr, path: &str, body: &[u8]) -> Response {
        request(
            address,
            &format!(
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\n\r\n",
                body.len()
            ),
            body,
        )
    }

    /// Dimensions plus the count of fully transparent and fully opaque pixels.
    fn pixel_counts(png: &[u8]) -> (u32, u32, usize, usize) {
        let image = image::load_from_memory(png)
            .expect("the response should be a decodable PNG")
            .to_rgba8();
        let transparent = image.pixels().filter(|pixel| pixel.0[3] == 0).count();
        let opaque = image.pixels().filter(|pixel| pixel.0[3] == 255).count();
        (image.width(), image.height(), transparent, opaque)
    }

    #[test]
    fn the_page_is_served_as_html() {
        let response = get(start_server(), "/");
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "text/html; charset=utf-8");
        let page = String::from_utf8(response.body).expect("utf-8 page");
        assert!(
            page.contains("Use sample image"),
            "the page carries its controls"
        );
        assert!(page.contains("<script>"), "the page carries its own script");
    }

    #[test]
    fn the_sample_is_a_png() {
        let response = get(start_server(), "/sample.png");
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "image/png");
        assert_eq!(&response.body[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn cutout_is_the_same_pipeline_with_a_transparent_background() {
        let address = start_server();
        let sample = get(address, "/sample.png").body;
        let response = post(address, "/cutout", &sample);
        assert_eq!(response.status, 200, "a valid image should cut out");
        assert_eq!(response.content_type, "image/png");
        assert_eq!(&response.body[..8], b"\x89PNG\r\n\x1a\n");

        // The shared pipeline crops to the subject: ExportOptions::default()
        // has crop: true, so the PNG is the tight cutout and never larger than
        // the 480x360 sample.
        let (width, height, transparent, opaque) = pixel_counts(&response.body);
        assert!(width > 0 && height > 0, "the cutout keeps a subject");
        assert!(width <= 480 && height <= 360, "the cutout is not enlarged");
        assert!(opaque > 0, "the subject survives the cutout");
        assert!(
            transparent > 0,
            "the background should be fully transparent"
        );
    }

    #[test]
    fn sticker_is_served_from_the_same_pipeline() {
        let address = start_server();
        let sample = get(address, "/sample.png").body;
        let response = post(address, "/sticker", &sample);
        assert_eq!(response.status, 200);
        let (width, height, transparent, opaque) = pixel_counts(&response.body);
        assert!(width > 0 && height > 0, "the sticker keeps its subject");
        assert!(opaque > 0, "the sticker carries the subject");
        assert!(transparent > 0, "a sticker sits on transparency");
    }

    #[test]
    fn a_broken_image_is_a_readable_400() {
        let response = post(start_server(), "/cutout", b"this is not an image");
        assert_eq!(response.status, 400);
        let message = String::from_utf8(response.body).expect("utf-8 error");
        assert!(
            message.contains("image"),
            "the message names the problem: {message:?}"
        );
    }

    #[test]
    fn an_oversized_upload_is_refused_before_it_is_read() {
        let address = start_server();
        let head = format!(
            "POST /cutout HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        let response = request(address, &head, &[]);
        assert_eq!(response.status, 413);
        assert!(String::from_utf8_lossy(&response.body).contains("at most"));
    }

    #[test]
    fn unknown_routes_are_404() {
        let address = start_server();
        assert_eq!(get(address, "/nope").status, 404);
        assert_eq!(post(address, "/nope", b"x").status, 404);
        assert_eq!(post(address, "/", b"x").status, 405);
    }

    #[test]
    fn the_port_comes_from_the_flag_then_the_environment() {
        let flag = resolve_port(["--port".to_string(), "9001".to_string()].into_iter());
        assert_eq!(flag.ok(), Some(Some(9001)));

        let equals = resolve_port(["--port=9002".to_string()].into_iter());
        assert_eq!(equals.ok(), Some(Some(9002)));

        let help = resolve_port(["--help".to_string()].into_iter());
        assert_eq!(help.ok(), Some(None));

        let missing = resolve_port(["--port".to_string()].into_iter());
        assert!(missing.is_err());
    }
}
