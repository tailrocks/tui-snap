use super::*;
use tuiscotti::{Cell, Color, Cursor, CursorStyle, Mods, Rgb, Screen};

pub(crate) fn mods_bold() -> Mods {
    Mods {
        bold: true,
        ..Mods::default()
    }
}

pub(crate) fn mods_underline() -> Mods {
    Mods {
        underline: true,
        ..Mods::default()
    }
}

pub(crate) fn mods_hidden_blink() -> Mods {
    Mods {
        hidden: true,
        blink: true,
        ..Mods::default()
    }
}

pub(crate) fn mods_reverse() -> Mods {
    Mods {
        reverse: true,
        ..Mods::default()
    }
}

/// 4x2 screen: indexed color, wide char + continuation, styled blank,
/// hidden+blink cell, visible blinking cursor. Nonzero origin.
pub(crate) fn screen_gen1() -> Result<Screen, Box<dyn std::error::Error>> {
    let mut cells: Vec<Cell> = (0..2)
        .flat_map(|y| (0..4).map(move |x| Cell::blank(x, y)))
        .collect();
    cells[0].symbol = "A".into();
    cells[0].fg = Color::Indexed(1);
    cells[0].mods = mods_bold();
    cells[1].symbol = "中".into();
    cells[1].width = 2;
    cells[2].symbol.clear();
    cells[2].width = 0;
    cells[2].continuation = true;
    cells[3].bg = Color::Indexed(4);
    cells[4].symbol = "B".into();
    cells[4].fg = Color::Rgb(Rgb::new(1, 2, 3));
    cells[4].mods = mods_underline();
    cells[5].symbol = "s".into();
    cells[5].mods = mods_hidden_blink();
    cells[6].symbol = "C".into();
    cells[6].mods = mods_reverse();
    Ok(Screen::validate(
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
    )?)
}

/// Generation 2: one symbol change (the "app" changed).
pub(crate) fn screen_gen2() -> Result<Screen, Box<dyn std::error::Error>> {
    let s = screen_gen1()?;
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
    Ok(Screen::validate(4, 2, 5, 7, cells, cursor)?)
}

pub(crate) fn rgba_image(
    pixels: &[[u8; 4]],
    w: u32,
    h: u32,
) -> Result<image::RgbaImage, Box<dyn std::error::Error>> {
    assert_eq!(pixels.len(), (w * h) as usize);
    let mut img = image::RgbaImage::new(w, h);
    for (i, p) in pixels.iter().enumerate() {
        let i = u32::try_from(i)?;
        img.put_pixel(i % w, i / w, image::Rgba(*p));
    }
    Ok(img)
}

pub(crate) fn encode_png(
    img: &image::RgbaImage,
    compression: image::codecs::png::CompressionType,
    filter: image::codecs::png::FilterType,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use image::ImageEncoder;
    use image::codecs::png::PngEncoder;
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(&mut buf, compression, filter).write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(buf)
}

pub(crate) fn pixels_gen1() -> Result<[[u8; 4]; 16], Box<dyn std::error::Error>> {
    let mut p = [[0u8; 4]; 16];
    for (i, cell) in p.iter_mut().enumerate() {
        let i = u8::try_from(i)?;
        *cell = [i * 16, 255 - i * 8, 64, 255];
    }
    Ok(p)
}

/// Semi-transparent gradient (alpha 128): alpha-policy decisions are
/// observable only on distinctly-encoded identical pixels.
pub(crate) fn pixels_semi() -> Result<[[u8; 4]; 16], Box<dyn std::error::Error>> {
    let mut p = [[0u8; 4]; 16];
    for (i, cell) in p.iter_mut().enumerate() {
        let i = u8::try_from(i)?;
        *cell = [i * 9, 40, 90, 128];
    }
    Ok(p)
}

/// Approved PNG bytes for a generation: encoded pixels + tEXt generation tag.
pub(crate) fn png_for_generation(
    gen_pixels: &[[u8; 4]; 16],
    generation: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let img = rgba_image(gen_pixels, 4, 4)?;
    let raw = encode_png(
        &img,
        image::codecs::png::CompressionType::Default,
        image::codecs::png::FilterType::Adaptive,
    )?;
    png_insert_text(&raw, PNG_GEN_KEYWORD, generation)
}

pub(crate) fn png_gen1() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    png_for_generation(&pixels_gen1()?, GEN1)
}

pub(crate) fn png_gen2() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut p = pixels_gen1()?;
    p[5] = [9, 9, 9, 255];
    png_for_generation(&p, GEN2)
}

const PNG_SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = if crc & 1 == 1 { 0xEDB8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

/// Insert a `tEXt` chunk before `IEND`. Decoders ignore it (pixel verdict
/// unaffected); [`check_consistent`] reads it back.
pub(crate) fn png_insert_text(
    png: &[u8],
    keyword: &str,
    value: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    assert!(png.starts_with(&PNG_SIG), "not a PNG");
    assert!(!keyword.contains('\0') && keyword.len() <= 79);
    let mut data = Vec::new();
    data.extend_from_slice(keyword.as_bytes());
    data.push(0);
    data.extend_from_slice(value.as_bytes());
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&u32::try_from(data.len())?.to_be_bytes());
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
    Ok(out)
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
            if let Some(z) = data.iter().position(|&b| b == 0)
                && &data[..z] == keyword.as_bytes()
            {
                return Some(String::from_utf8_lossy(&data[z + 1..]).into_owned());
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
pub(crate) fn png_gen1_no_tag_for_test() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let img = rgba_image(&pixels_gen1()?, 4, 4)?;
    encode_png(
        &img,
        image::codecs::png::CompressionType::Default,
        image::codecs::png::FilterType::Adaptive,
    )
}
