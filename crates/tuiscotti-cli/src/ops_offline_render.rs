//! Offline `render` plumbing: raster font loading plus single-format dispatch.
//!
//! Split from `ops_offline` (which keeps the `cmd_*` entry points): the
//! raster renderer is lazily constructed and reused across formats so the
//! font faces parse once per invocation.

use std::path::Path;

use crate::cli::RenderFormat;

/// Raster profile plus the font bytes backing it: a `--font-file` override
/// (one file used for all four faces) or the vendored faces.
pub struct RasterFonts {
    profile: tuiscotti::Profile,
    owned: Option<[Vec<u8>; 4]>,
}

impl RasterFonts {
    pub fn load(font_file: Option<&Path>) -> Result<Self, String> {
        let Some(path) = font_file else {
            return Ok(Self {
                profile: tuiscotti::Profile::default_profile(),
                owned: None,
            });
        };
        let bytes =
            std::fs::read(path).map_err(|e| format!("read font {}: {e}", path.display()))?;
        let profile = tuiscotti::Profile::default_profile()
            .with_font_file(path.display().to_string(), &bytes);
        Ok(Self {
            profile,
            owned: Some([bytes.clone(), bytes.clone(), bytes.clone(), bytes]),
        })
    }

    pub fn faces(&self) -> tuiscotti::FontFaces<'_> {
        if let Some(owned) = &self.owned {
            tuiscotti::FontFaces {
                regular: owned[0].as_slice(),
                bold: owned[1].as_slice(),
                italic: owned[2].as_slice(),
                bold_italic: owned[3].as_slice(),
            }
        } else {
            tuiscotti::FontFaces {
                regular: tuiscotti::VENDORED_FONT,
                bold: tuiscotti::VENDORED_FONT_BOLD,
                italic: tuiscotti::VENDORED_FONT_ITALIC,
                bold_italic: tuiscotti::VENDORED_FONT_BOLD_ITALIC,
            }
        }
    }
}

/// Render `frame` in one `format` to `path`. The raster renderer is lazily
/// constructed and reused across formats (faces parse once).
pub fn render_format_to(
    frame: &tuiscotti::Frame,
    fonts: &RasterFonts,
    renderer: &mut Option<tuiscotti::Renderer>,
    format: RenderFormat,
    path: &str,
) -> Result<(), String> {
    match format {
        RenderFormat::Txt => std::fs::write(path, frame.text()).map_err(|e| e.to_string()),
        RenderFormat::Ansi => {
            std::fs::write(path, tuiscotti::render::ansi_dump(frame)).map_err(|e| e.to_string())
        }
        RenderFormat::Json => std::fs::write(path, frame.to_json()).map_err(|e| e.to_string()),
        RenderFormat::Svg => {
            std::fs::write(path, tuiscotti::render::render_svg(frame, &fonts.profile))
                .map_err(|e| e.to_string())
        }
        RenderFormat::Html => {
            let html = cached_renderer(fonts, renderer)?
                .render_html(frame, "frame")
                .map_err(|e| e.to_string())?;
            std::fs::write(path, html).map_err(|e| e.to_string())
        }
        RenderFormat::Png => {
            let rendered = cached_renderer(fonts, renderer)?
                .render(frame)
                .map_err(|e| e.to_string())?;
            std::fs::write(path, &rendered.png).map_err(|e| e.to_string())?;
            std::fs::write(format!("{path}.fidelity.json"), rendered.fidelity.to_json())
                .map_err(|e| e.to_string())
        }
    }
}

/// Lazily construct (once) and borrow the raster renderer.
fn cached_renderer<'r>(
    fonts: &RasterFonts,
    renderer: &'r mut Option<tuiscotti::Renderer>,
) -> Result<&'r mut tuiscotti::Renderer, String> {
    if renderer.is_none() {
        let faces = fonts.faces();
        let built = tuiscotti::Renderer::new(&fonts.profile, &faces).map_err(|e| e.to_string())?;
        *renderer = Some(built);
    }
    renderer
        .as_mut()
        .ok_or_else(|| "renderer build failed".to_string())
}
