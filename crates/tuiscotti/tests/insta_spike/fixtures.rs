use super::*;
use tuiscotti::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, Screen, UnderlineStyle};

#[allow(clippy::too_many_arguments)]
pub(crate) fn cell(
    x: u16,
    y: u16,
    sym: &str,
    width: u8,
    continuation: bool,
    fg: Color,
    bg: Color,
    mods: Mods,
) -> Cell {
    Cell {
        x,
        y,
        symbol: sym.to_string(),
        width,
        continuation,
        fg,
        bg,
        mods,
        underline_color: Color::Default,
    }
}

pub(crate) fn mods_of(
    bold: bool,
    underline: bool,
    hidden: bool,
    blink: bool,
    reverse: bool,
) -> Mods {
    Mods {
        hidden,
        blink,
        bold,
        dim: false,
        italic: false,
        underline,
        underline_style: UnderlineStyle::None,
        strikethrough: false,
        reverse,
    }
}

/// 4x2 screen: indexed color, wide char + continuation, styled blank,
/// hidden+blink cell, visible blinking cursor. Nonzero origin.
pub(crate) fn screen_gen1() -> Screen {
    let cells = vec![
        cell(
            0,
            0,
            "A",
            1,
            false,
            Color::Indexed(1),
            Color::Default,
            mods_of(true, false, false, false, false),
        ),
        cell(
            1,
            0,
            "中",
            2,
            false,
            Color::Default,
            Color::Default,
            Mods::default(),
        ),
        cell(
            2,
            0,
            "",
            0,
            true,
            Color::Default,
            Color::Default,
            Mods::default(),
        ),
        cell(
            3,
            0,
            " ",
            1,
            false,
            Color::Default,
            Color::Indexed(4),
            Mods::default(),
        ),
        cell(
            0,
            1,
            "B",
            1,
            false,
            Color::Rgb(Rgb::new(1, 2, 3)),
            Color::Default,
            mods_of(false, true, false, false, false),
        ),
        cell(
            1,
            1,
            "s",
            1,
            false,
            Color::Default,
            Color::Default,
            mods_of(false, false, true, true, false),
        ),
        cell(
            2,
            1,
            "C",
            1,
            false,
            Color::Default,
            Color::Default,
            mods_of(false, false, false, false, true),
        ),
        cell(
            3,
            1,
            " ",
            1,
            false,
            Color::Default,
            Color::Default,
            Mods::default(),
        ),
    ];
    Screen::validate(
        4,
        2,
        5,
        7,
        cells,
        Cursor {
            x: 1,
            y: 0,
            visible: true,
            style: CursorStyle::Block,
            blinking: true,
        },
    )
    .unwrap()
}

/// Generation 2: one symbol change (the "app" changed).
pub(crate) fn screen_gen2() -> Screen {
    let mut s = screen_gen1();
    let cells: Vec<Cell> = s
        .cells()
        .iter()
        .map(|c| {
            let mut c = c.clone();
            if c.x == 0 && c.y == 0 {
                c.symbol = "Z".to_string();
            }
            c
        })
        .collect();
    let cursor = *s.cursor();
    s = Screen::validate(4, 2, 5, 7, cells, cursor).unwrap();
    s
}

pub(crate) fn rgba_image(pixels: &[[u8; 4]], w: u32, h: u32) -> image::RgbaImage {
    assert_eq!(pixels.len(), (w * h) as usize);
    let mut img = image::RgbaImage::new(w, h);
    for (i, p) in pixels.iter().enumerate() {
        img.put_pixel(i as u32 % w, i as u32 / w, image::Rgba(*p));
    }
    img
}

pub(crate) fn encode_png(
    img: &image::RgbaImage,
    compression: image::codecs::png::CompressionType,
    filter: image::codecs::png::FilterType,
) -> Vec<u8> {
    use image::ImageEncoder;
    use image::codecs::png::PngEncoder;
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(&mut buf, compression, filter)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    buf
}

pub(crate) fn pixels_gen1() -> [[u8; 4]; 16] {
    let mut p = [[0u8; 4]; 16];
    for (i, cell) in p.iter_mut().enumerate() {
        *cell = [(i as u8) * 16, 255 - (i as u8) * 8, 64, 255];
    }
    p
}

/// Approved PNG bytes for a generation: encoded pixels + tEXt generation tag.
pub(crate) fn png_for_generation(gen_pixels: &[[u8; 4]; 16], generation: &str) -> Vec<u8> {
    let img = rgba_image(gen_pixels, 4, 4);
    let raw = encode_png(
        &img,
        image::codecs::png::CompressionType::Default,
        image::codecs::png::FilterType::Adaptive,
    );
    png_insert_text(&raw, PNG_GEN_KEYWORD, generation)
}

pub(crate) fn png_gen1() -> Vec<u8> {
    png_for_generation(&pixels_gen1(), GEN1)
}

pub(crate) fn png_gen2() -> Vec<u8> {
    let mut p = pixels_gen1();
    p[5] = [9, 9, 9, 255];
    png_for_generation(&p, GEN2)
}

const PNG_SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = if crc & 1 == 1 { 0xEDB8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

/// Insert a `tEXt` chunk before `IEND`. Decoders ignore it (pixel verdict
/// unaffected); [`check_consistent`] reads it back.
pub(crate) fn png_insert_text(png: &[u8], keyword: &str, value: &str) -> Vec<u8> {
    assert!(png.starts_with(&PNG_SIG), "not a PNG");
    assert!(!keyword.contains('\0') && keyword.len() <= 79);
    let mut data = Vec::new();
    data.extend_from_slice(keyword.as_bytes());
    data.push(0);
    data.extend_from_slice(value.as_bytes());
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(&data);
    let mut crc_input = b"tEXt".to_vec();
    crc_input.extend_from_slice(&data);
    chunk.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    assert!(png.len() > 12 && &png[png.len() - 8..png.len() - 4] == b"IEND");
    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..png.len() - 12]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[png.len() - 12..]);
    out
}

pub(crate) fn png_find_text(png: &[u8], keyword: &str) -> Option<String> {
    if !png.starts_with(&PNG_SIG) || png.len() < 12 {
        return None;
    }
    let mut i = 8;
    while i + 8 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().ok()?) as usize;
        let typ = &png[i + 4..i + 8];
        if i + 8 + len + 4 > png.len() {
            return None;
        }
        if typ == b"tEXt" {
            let data = &png[i + 8..i + 8 + len];
            if let Some(z) = data.iter().position(|&b| b == 0) {
                if &data[..z] == keyword.as_bytes() {
                    return Some(String::from_utf8_lossy(&data[z + 1..]).into_owned());
                }
            }
        }
        if typ == b"IEND" {
            break;
        }
        i += 8 + len + 4;
    }
    None
}

/// Raw gen1 PNG without the generation tag (CRC path exercised separately).
pub(crate) fn png_gen1_no_tag_for_test() -> Vec<u8> {
    let img = rgba_image(&pixels_gen1(), 4, 4);
    encode_png(
        &img,
        image::codecs::png::CompressionType::Default,
        image::codecs::png::FilterType::Adaptive,
    )
}
