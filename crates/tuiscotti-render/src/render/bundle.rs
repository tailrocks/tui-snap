//! Opt-in byte contracts and evidence bundles.

use super::{Artifacts, Fidelity, RenderError};
use crate::profile::RENDERER_VERSION;
use crate::profile::RenderProfile;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Byte-sensitive contracts, opt-in (V10).
// ---------------------------------------------------------------------------

/// The exact representation bytes of one render: normalized ANSI dump, plain
/// text, and standalone HTML. Byte comparison of these is OPT-IN — for
/// suites where the representation bytes themselves are contractual — via
/// [`check_contract_bytes`]. No gate calls it by default.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContractBytes {
    /// Normalized ANSI dump bytes.
    pub ansi: String,
    /// Plain-text projection bytes.
    pub txt: String,
    /// Standalone HTML document bytes.
    pub html: String,
}

impl Artifacts {
    /// The byte-contract view of these artifacts.
    #[must_use]
    pub fn contract(&self) -> ContractBytes {
        ContractBytes {
            ansi: self.ansi.clone(),
            txt: self.txt.clone(),
            html: self.html.clone(),
        }
    }
}

/// Opt-in byte-identity check over [`ContractBytes`]: every field must match
/// byte-for-byte. The error names the first differing field with lengths and
/// the first differing byte offset. Never part of the default gate.
///
/// # Errors
///
/// Returns `RenderError` naming the first differing field.
pub fn check_contract_bytes(
    actual: &ContractBytes,
    expected: &ContractBytes,
) -> Result<(), RenderError> {
    for (label, a, e) in [
        ("ansi", actual.ansi.as_bytes(), expected.ansi.as_bytes()),
        ("txt", actual.txt.as_bytes(), expected.txt.as_bytes()),
        ("html", actual.html.as_bytes(), expected.html.as_bytes()),
    ] {
        if a != e {
            let off = a
                .iter()
                .zip(e.iter())
                .position(|(x, y)| x != y)
                .unwrap_or(a.len().min(e.len()));
            return Err(RenderError(format!(
                "contract bytes differ in {label}: actual {} bytes, expected {} bytes, first diff at byte {off}",
                a.len(),
                e.len()
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Portable offline bundle (V09 support).
// ---------------------------------------------------------------------------

/// Manifest for a portable offline evidence bundle: renderer version, strict
/// profile identity + hash, every pinned face hash, scale, and the fidelity
/// verdict. Together with the artifact files it makes the bundle
/// self-describing with no network and no viewer fonts required (the HTML
/// embeds the PNG as a data URI).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BundleManifest {
    /// Renderer version that produced the bundle.
    pub renderer_version: u32,
    /// Strict profile name.
    pub profile: String,
    /// Strict profile content hash.
    pub profile_hash: String,
    /// Pinned styled-face hashes (regular, bold, italic, bold-italic).
    pub face_hashes: [String; 4],
    /// Fallback faces as `desc:sha256`, in chain order.
    pub fallback_faces: Vec<String>,
    /// Integer rasterization scale of the render.
    pub scale: u32,
    /// Fidelity verdict of the render.
    pub approximate: bool,
}

impl BundleManifest {
    /// Manifest for one strict-profile render plus its fidelity record.
    #[must_use]
    pub fn for_render(rp: &RenderProfile<'_>, fidelity: &Fidelity) -> Self {
        Self {
            renderer_version: RENDERER_VERSION,
            profile: rp.name().to_string(),
            profile_hash: rp.hash(),
            face_hashes: rp.face_hashes().clone(),
            fallback_faces: rp
                .fallback_order()
                .iter()
                .map(|f| format!("{}:{}", f.desc, f.sha256))
                .collect(),
            scale: rp.scale(),
            approximate: fidelity.approximate,
        }
    }

    /// Pretty JSON manifest content (`manifest.json`).
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self)
            .unwrap_or_else(|e| unreachable!("BundleManifest is plain serializable data: {e}"))
    }
}

impl Artifacts {
    /// Write a portable offline bundle: `screen.ansi`, `screen.txt`,
    /// `screen.png`, `screen.html`, `fidelity.json`, `manifest.json`.
    /// Returns the written paths. The HTML is self-contained (PNG embedded,
    /// no external references); the manifest pins the renderer and fonts.
    ///
    /// # Errors
    ///
    /// Returns `RenderError` when the directory or any file cannot be written.
    pub fn write_bundle(
        &self,
        dir: &Path,
        manifest: &BundleManifest,
    ) -> Result<Vec<PathBuf>, RenderError> {
        std::fs::create_dir_all(dir)
            .map_err(|e| RenderError(format!("cannot create bundle dir {}: {e}", dir.display())))?;
        let fidelity_json = self.fidelity.to_json();
        let manifest_json = manifest.to_json();
        let files: Vec<(&str, &[u8])> = vec![
            ("screen.ansi", self.ansi.as_bytes()),
            ("screen.txt", self.txt.as_bytes()),
            ("screen.png", &self.png),
            ("screen.html", self.html.as_bytes()),
            ("fidelity.json", fidelity_json.as_bytes()),
            ("manifest.json", manifest_json.as_bytes()),
        ];
        let mut out = Vec::with_capacity(files.len());
        for (name, bytes) in files {
            let path = dir.join(name);
            std::fs::write(&path, bytes)
                .map_err(|e| RenderError(format!("bundle write {name} failed: {e}")))?;
            out.push(path);
        }
        Ok(out)
    }
}
