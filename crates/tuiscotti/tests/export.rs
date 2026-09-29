//! Evidence exports + graphics inspection (backlog A06, A07).

#[path = "export/formats.rs"]
mod formats;
#[path = "export/graphics.rs"]
mod graphics;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn png_solid(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
    use image::ImageEncoder as _;
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba(px));
    let mut buf = Vec::new();
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .unwrap();
    buf
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn kitty(params: &str, payload_b64: &str) -> Vec<u8> {
    format!("\x1b_G{params};{payload_b64}\x1b\\").into_bytes()
}

fn sixel(params: &str, data: &str) -> Vec<u8> {
    format!("\x1bP{params}q{data}\x1b\\").into_bytes()
}

// ---------------------------------------------------------------------------
// Cast (A06)
// ---------------------------------------------------------------------------
