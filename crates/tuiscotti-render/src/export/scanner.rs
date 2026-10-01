//! Byte-stream scanner: DCS/APC/OSC dispatch for graphics payloads.

use super::{
    GraphicsDiagKind, GraphicsDiagnostic, GraphicsKind, GraphicsPayload, GraphicsPolicy,
    GraphicsScan, Placement, bound_bytes, parse_sixel_raster,
};
use std::collections::HashMap;

pub(crate) struct PendingKitty {
    pub(crate) offset: usize,
    pub(crate) params: Vec<(String, String)>,
    pub(crate) b64: Vec<u8>,
}

pub(crate) struct Scanner<'a> {
    pub(crate) stream: &'a [u8],
    pub(crate) policy: &'a GraphicsPolicy,
    pub(crate) scan: GraphicsScan,
    pub(crate) pending: Option<PendingKitty>,
    /// Transmitted image bytes by Kitty image id (`a=t/T/f`, `i=<id> != 0`).
    pub(crate) images: HashMap<u32, Vec<u8>>,
    pub(crate) images_full: bool,
}

impl<'a> Scanner<'a> {
    pub(crate) fn new(stream: &'a [u8], policy: &'a GraphicsPolicy) -> Self {
        Self {
            stream,
            policy,
            scan: GraphicsScan::default(),
            pending: None,
            images: HashMap::new(),
            images_full: false,
        }
    }

    pub(crate) fn diag(&mut self, offset: usize, kind: GraphicsDiagKind, message: String) {
        self.scan.diagnostics.push(GraphicsDiagnostic {
            offset,
            kind,
            message,
        });
    }

    pub(crate) fn push_payload(&mut self, payload: GraphicsPayload) {
        if self.scan.payloads.len() >= self.policy.max_payloads {
            self.diag(
                payload.stream_offset,
                GraphicsDiagKind::Truncated,
                format!(
                    "payload limit {} reached; remainder of stream uninspected",
                    self.policy.max_payloads
                ),
            );
            return;
        }
        self.scan.payloads.push(payload);
    }

    pub(crate) fn run(mut self) -> GraphicsScan {
        let mut i = 0;
        while i < self.stream.len() {
            let b = self.stream[i];
            if b == 0x1B && i + 1 < self.stream.len() {
                let n = self.stream[i + 1];
                match n {
                    b'P' => {
                        i = self.on_dcs(i, i + 2);
                        continue;
                    }
                    b'_' => {
                        i = self.on_apc(i, i + 2);
                        continue;
                    }
                    b']' => {
                        i = self.on_osc(i, i + 2);
                        continue;
                    }
                    _ => {}
                }
                i += 1;
            } else if b == 0x90 {
                i = self.on_dcs(i, i + 1);
            } else if b == 0x9F {
                i = self.on_apc(i, i + 1);
            } else if b == 0x9D {
                i = self.on_osc(i, i + 1);
            } else {
                i += 1;
            }
            if self.scan.payloads.len() >= self.policy.max_payloads {
                break;
            }
        }
        if let Some(pending) = self.pending.take() {
            self.diag(
                pending.offset,
                GraphicsDiagKind::Malformed,
                "kitty chunk chain ends with m=1 (missing final chunk)".to_string(),
            );
        }
        self.scan
    }

    /// Find `ESC \` or `0x9C` from `start`; returns (content, `resume_at`).
    fn st_content(&self, start: usize) -> Option<(&'a [u8], usize)> {
        let s = self.stream;
        let mut j = start;
        while j < s.len() {
            if s[j] == 0x1B && j + 1 < s.len() && s[j + 1] == b'\\' {
                return Some((&s[start..j], j + 2));
            }
            if s[j] == 0x9C {
                return Some((&s[start..j], j + 1));
            }
            j += 1;
        }
        None
    }

    fn on_dcs(&mut self, offset: usize, start: usize) -> usize {
        let Some((content, resume)) = self.st_content(start) else {
            self.diag(
                offset,
                GraphicsDiagKind::Malformed,
                "unterminated DCS (no ST)".to_string(),
            );
            return start;
        };
        // First byte >= 0x40 ends params/intermediates: that is the final.
        let mut fin = None;
        for (k, &c) in content.iter().enumerate() {
            if c >= 0x40 {
                fin = Some(k);
                break;
            }
        }
        let Some(f) = fin else {
            self.diag(
                offset,
                GraphicsDiagKind::Malformed,
                "DCS with no final byte".to_string(),
            );
            return resume;
        };
        if content[f] != b'q' {
            self.diag(
                offset,
                GraphicsDiagKind::Unsupported,
                format!(
                    "non-sixel DCS (final '{}', {} param bytes): not inspected",
                    content[f] as char, f
                ),
            );
            return resume;
        }
        let params_raw = String::from_utf8_lossy(&content[..f]).into_owned();
        let data = &content[f + 1..];
        let (kept, truncated) = bound_bytes(data, self.policy.max_payload_bytes);
        if truncated {
            self.diag(
                offset,
                GraphicsDiagKind::Truncated,
                format!(
                    "sixel payload cut from {} to {} bytes",
                    data.len(),
                    kept.len()
                ),
            );
        }
        let mut params = vec![("P".to_string(), params_raw)];
        let mut placement = Placement::default();
        // Raster attributes `"Pan;Pad;Ph;Pv`: only Ph/Pv (pixel dims) survive.
        if let Some(raster) = parse_sixel_raster(data) {
            params.push(("Ph".to_string(), raster.0.to_string()));
            params.push(("Pv".to_string(), raster.1.to_string()));
            placement.image_px = Some(raster);
        }
        self.push_payload(GraphicsPayload {
            kind: GraphicsKind::Sixel,
            params,
            data: kept,
            truncated,
            references: None,
            placement,
            stream_offset: offset,
        });
        resume
    }

    fn on_apc(&mut self, offset: usize, start: usize) -> usize {
        let Some((content, resume)) = self.st_content(start) else {
            self.diag(
                offset,
                GraphicsDiagKind::Malformed,
                "unterminated APC (no ST)".to_string(),
            );
            return start;
        };
        if content.first() != Some(&b'G') {
            let first = content.first().copied().unwrap_or(b'?');
            self.diag(
                offset,
                GraphicsDiagKind::Unsupported,
                format!(
                    "non-kitty APC (first byte '{}'): not inspected",
                    first as char
                ),
            );
            return resume;
        }
        let body = &content[1..];
        let Some(semi) = body.iter().position(|&c| c == b';') else {
            self.diag(
                offset,
                GraphicsDiagKind::Malformed,
                "kitty command without ';' header/payload separator".to_string(),
            );
            return resume;
        };
        let (header, payload_b64) = (&body[..semi], &body[semi + 1..]);
        let mut params = Vec::new();
        for pair in header.split(|&c| c == b',') {
            if pair.is_empty() {
                continue;
            }
            if let Some(eq) = pair.iter().position(|&c| c == b'=') {
                params.push((
                    String::from_utf8_lossy(&pair[..eq]).into_owned(),
                    String::from_utf8_lossy(&pair[eq + 1..]).into_owned(),
                ));
            } else {
                self.diag(
                    offset,
                    GraphicsDiagKind::Malformed,
                    format!("kitty key without '=': {:?}", String::from_utf8_lossy(pair)),
                );
                params.push((String::from_utf8_lossy(pair).into_owned(), String::new()));
            }
        }
        let more = params.iter().any(|(k, v)| k == "m" && v == "1");
        if more {
            self.on_kitty_chunk(offset, params, payload_b64);
            return resume;
        }
        self.on_kitty_final(offset, params, payload_b64, resume);
        resume
    }
}
