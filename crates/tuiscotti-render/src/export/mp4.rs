//! MP4 export via an external `ffmpeg` binary (never vendored).

use super::{ExportError, Mp4Policy, decode_png_frames, json_string};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// MP4 via external ffmpeg (A06)
// ---------------------------------------------------------------------------

/// Install hint carried by [`ExportError::EncoderMissing`] for `ffmpeg`.
pub const FFMPEG_INSTALL: &str = "install ffmpeg: https://ffmpeg.org/download.html (Debian/Ubuntu: `apt install ffmpeg`; macOS: `brew install ffmpeg`)";

/// Sidecar record of one MP4 encode: output paths plus the exact encoder
/// identity (MP4 bytes are NOT deterministic across ffmpeg builds).
#[derive(Debug, Clone)]
pub struct Mp4Sidecar {
    /// The encoded video.
    pub mp4: PathBuf,
    /// JSON sidecar next to it (`<name>.ffmpeg.json`).
    pub sidecar: PathBuf,
    /// First line of `ffmpeg -version` output.
    pub ffmpeg_version: String,
}

/// Probe for an external `ffmpeg`: run `ffmpeg -version`, return its first
/// output line. Missing binary → [`ExportError::EncoderMissing`].
pub fn ffmpeg_version() -> Result<String, ExportError> {
    match std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
    {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ExportError::EncoderMissing {
            tool: "ffmpeg",
            install: FFMPEG_INSTALL,
        }),
        Err(e) => Err(ExportError::Encode(format!("ffmpeg probe failed: {e}"))),
        Ok(out) if !out.status.success() => Err(ExportError::Encode(format!(
            "ffmpeg -version exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ))),
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            Ok(stdout
                .lines()
                .next()
                .unwrap_or("ffmpeg (empty version output)")
                .to_string())
        }
    }
}

/// Encode PNG frames as MP4 via an EXTERNAL `ffmpeg` binary.
///
/// Pipeline: PNGs are staged verbatim plus a concat-demuxer playlist into a
/// sibling `<name>.mp4frames/` directory, then `ffmpeg` runs with pinned
/// flags (`-c:v libx264 -pix_fmt <pix_fmt> -crf <crf> -preset <preset>
/// `-movflags +faststart`). The staging directory is removed on success and
/// KEPT on failure (named in the error) for diagnosis. A `<name>.ffmpeg.json`
/// sidecar records the ffmpeg version line, argv, frames, and dims.
///
/// Output bytes are explicitly NOT deterministic: they depend on the ffmpeg
/// build (encoder version, platform SIMD). Only the sidecar pins identity.
pub fn mp4(
    frames_png: &[Vec<u8>],
    delays_ms: &[u32],
    path: &Path,
) -> Result<Mp4Sidecar, ExportError> {
    mp4_with(frames_png, delays_ms, path, &Mp4Policy::default())
}

/// [`mp4`] with an explicit policy.
pub fn mp4_with(
    frames_png: &[Vec<u8>],
    delays_ms: &[u32],
    path: &Path,
    policy: &Mp4Policy,
) -> Result<Mp4Sidecar, ExportError> {
    if policy.crf > 51 {
        return Err(ExportError::InvalidInput(format!(
            "mp4 crf must be 0..=51, got {}",
            policy.crf
        )));
    }
    if policy.preset.is_empty() || policy.pix_fmt.is_empty() {
        return Err(ExportError::InvalidInput(
            "mp4 preset and pix_fmt must be nonempty".to_string(),
        ));
    }
    let version = ffmpeg_version()?;
    let frames = decode_png_frames(frames_png, delays_ms, "mp4")?;
    let (w, h) = (frames[0].width(), frames[0].height());
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let staging = stage_mp4_frames(path, frames_png, delays_ms)?;
    let args = run_ffmpeg_encode(path, policy, &staging)?;
    let sidecar_path = write_mp4_sidecar(path, &version, &args, frames.len(), w, h)?;
    let _staging_removed = std::fs::remove_dir_all(&staging);
    Ok(Mp4Sidecar {
        mp4: path.to_path_buf(),
        sidecar: sidecar_path,
        ffmpeg_version: version,
    })
}

/// Stage PNG frames plus a concat-demuxer playlist under
/// `<path>.mp4frames/`. Each file is followed by its display duration;
/// the final file is repeated so the last duration takes effect.
fn stage_mp4_frames(
    path: &Path,
    frames_png: &[Vec<u8>],
    delays_ms: &[u32],
) -> Result<PathBuf, ExportError> {
    let staging = path.with_extension("mp4frames");
    std::fs::create_dir_all(&staging)?;
    for (i, png) in frames_png.iter().enumerate() {
        std::fs::write(staging.join(format!("f{i:06}.png")), png)?;
    }
    let mut list = String::new();
    for (i, delay) in delays_ms.iter().enumerate() {
        list.push_str(&format!("file 'f{i:06}.png'\n"));
        list.push_str(&format!("duration {}\n", format_secs(*delay)));
    }
    list.push_str(&format!("file 'f{:06}.png'\n", frames_png.len() - 1));
    std::fs::write(staging.join("list.txt"), &list)?;
    Ok(staging)
}

/// Run the pinned ffmpeg encode inside `staging`; returns the argv for the
/// sidecar. A failed encode keeps the staging dir and names it in the error.
fn run_ffmpeg_encode(
    path: &Path,
    policy: &Mp4Policy,
    staging: &Path,
) -> Result<Vec<String>, ExportError> {
    // Absolute output: the child runs with cwd=staging.
    let out_abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|c| c.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let args = [
        "-y".to_string(),
        "-f".to_string(),
        "concat".to_string(),
        "-safe".to_string(),
        "0".to_string(),
        "-i".to_string(),
        "list.txt".to_string(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-pix_fmt".to_string(),
        policy.pix_fmt.clone(),
        "-crf".to_string(),
        policy.crf.to_string(),
        "-preset".to_string(),
        policy.preset.clone(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        out_abs.to_string_lossy().into_owned(),
    ];
    let run = std::process::Command::new("ffmpeg")
        .args(&args)
        .current_dir(staging)
        .output()
        .map_err(|e| ExportError::Encode(format!("ffmpeg encode spawn failed: {e}")))?;
    if !run.status.success() {
        let mut stderr = String::from_utf8_lossy(&run.stderr).into_owned();
        if stderr.len() > 2048 {
            stderr = format!("...<truncated>...{}", &stderr[stderr.len() - 2048..]);
        }
        return Err(ExportError::Encode(format!(
            "ffmpeg exited {} (staging kept at {}): {stderr}",
            run.status,
            staging.display()
        )));
    }
    Ok(args.into_iter().collect())
}

/// Record tool identity, argv, and frame geometry next to the MP4.
fn write_mp4_sidecar(
    path: &Path,
    version: &str,
    args: &[String],
    frames: usize,
    w: u32,
    h: u32,
) -> Result<PathBuf, ExportError> {
    let sidecar_path = path.with_extension("ffmpeg.json");
    let mut sidecar = String::from("{\n");
    sidecar.push_str("  \"tool\": \"ffmpeg\",\n");
    sidecar.push_str(&format!("  \"version\": {},\n", json_string(version)));
    sidecar.push_str("  \"args\": [");
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            sidecar.push_str(", ");
        }
        sidecar.push_str(&json_string(a));
    }
    sidecar.push_str(&format!(
        "],\n  \"frames\": {frames},\n  \"width\": {w},\n  \"height\": {h},\n  \"deterministic\": false\n}}\n"
    ));
    std::fs::write(&sidecar_path, &sidecar)?;
    Ok(sidecar_path)
}

/// `delays_ms` → concat-demuxer seconds with exact millisecond precision.
fn format_secs(ms: u32) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}
