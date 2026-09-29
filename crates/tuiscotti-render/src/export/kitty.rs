//! Kitty graphics protocol: chunk reassembly, placement, iTerm2/OSC handling.

use super::{
    GraphicsDiagKind, GraphicsKind, GraphicsPayload, PendingKitty, Placement, Scanner,
    base64_decode, num_pair,
};

impl<'a> Scanner<'a> {
    /// A non-final (`m=1`) Kitty chunk: accumulate, enforcing the spec rule
    /// that continuation chunks carry only `m` (+`q`, +`a=f` for animation).
    pub(crate) fn on_kitty_chunk(
        &mut self,
        offset: usize,
        params: Vec<(String, String)>,
        b64: &[u8],
    ) {
        if self.pending.is_none() {
            self.pending = Some(PendingKitty {
                offset,
                params,
                b64: b64.to_vec(),
            });
            return;
        }
        let action_f = params.iter().any(|(k, v)| k == "a" && v == "f");
        for (k, v) in &params {
            let allowed = k == "m" || k == "q" || (action_f && k == "a" && v == "f");
            if !allowed {
                self.diag(
                    offset,
                    GraphicsDiagKind::Malformed,
                    format!("kitty continuation chunk repeats key '{k}={v}' (only m/q allowed)"),
                );
            }
        }
        if let Some(pending) = self.pending.as_mut() {
            pending.b64.extend_from_slice(b64);
        }
    }

    /// A final (`m=0`/absent) Kitty command: close any pending chain,
    /// base64-decode, resolve `a=p` references, extract placement.
    pub(crate) fn on_kitty_final(
        &mut self,
        offset: usize,
        params: Vec<(String, String)>,
        b64: &[u8],
        _resume: usize,
    ) {
        let (first_offset, first_params, all_b64) = self.take_pending_chain(offset, params, b64);
        let action = Self::param_lookup(&first_params, "a")
            .unwrap_or("t")
            .to_string();
        let (data, references) =
            self.decode_kitty_data(first_offset, &action, &first_params, &all_b64);
        self.retain_transmitted(first_offset, &action, &first_params, &data);
        let (data, truncated) = if data.len() > self.policy.max_payload_bytes {
            self.diag(
                first_offset,
                GraphicsDiagKind::Truncated,
                format!(
                    "kitty payload cut from {} to {} bytes",
                    data.len(),
                    self.policy.max_payload_bytes
                ),
            );
            (data[..self.policy.max_payload_bytes].to_vec(), true)
        } else {
            (data, false)
        };
        let placement = self.kitty_placement(first_offset, &action, &first_params);
        self.push_payload(GraphicsPayload {
            kind: GraphicsKind::Kitty,
            params: first_params,
            data,
            truncated,
            references,
            placement,
            stream_offset: first_offset,
        });
    }

    /// Close a pending multi-chunk chain: merge first-chunk keys with the
    /// final chunk (flagging keys the first chunk lacked), strip the `m`
    /// transport marker, and concatenate payload bytes.
    fn take_pending_chain(
        &mut self,
        offset: usize,
        params: Vec<(String, String)>,
        b64: &[u8],
    ) -> (usize, Vec<(String, String)>, Vec<u8>) {
        let (first_offset, mut first_params, mut all_b64) = match self.pending.take() {
            None => (offset, params, Vec::new()),
            Some(pending) => {
                for (k, v) in &params {
                    let allowed = k == "m" || k == "q" || k == "a" && v == "f";
                    if !allowed && Self::param_lookup(&pending.params, k).is_none() {
                        // Final-chunk keys the first chunk lacked are kept
                        // (lenient) but flagged (loud).
                        self.diag(
                            offset,
                            GraphicsDiagKind::Malformed,
                            format!(
                                "kitty final chunk adds key '{k}={v}' missing from first chunk"
                            ),
                        );
                    }
                }
                let mut merged = pending.params;
                for (k, v) in params {
                    if k != "m" && Self::param_lookup(&merged, &k).is_none() {
                        merged.push((k, v));
                    }
                }
                (pending.offset, merged, pending.b64)
            }
        };
        // Strip the chunking marker: equality must not see transport.
        first_params.retain(|(k, _)| k != "m");
        all_b64.extend_from_slice(b64);
        (first_offset, first_params, all_b64)
    }

    /// Base64-decode the payload bytes. An `a=p` command with no inline
    /// bytes displays a previously transmitted image id instead.
    fn decode_kitty_data(
        &mut self,
        first_offset: usize,
        action: &str,
        first_params: &[(String, String)],
        all_b64: &[u8],
    ) -> (Vec<u8>, Option<u32>) {
        let mut references = None;
        let mut data = match base64_decode(all_b64) {
            Ok(d) => d,
            Err(e) => {
                self.diag(
                    first_offset,
                    GraphicsDiagKind::Malformed,
                    format!("kitty base64 payload invalid: {e}"),
                );
                Vec::new()
            }
        };
        // `a=p` with no inline bytes displays a transmitted id.
        if action == "p" && data.is_empty() {
            if let Some(id) =
                Self::param_lookup(first_params, "i").and_then(|s| s.parse::<u32>().ok())
            {
                if id != 0 {
                    references = Some(id);
                    match self.images.get(&id) {
                        Some(bytes) => data = bytes.clone(),
                        None => self.diag(
                            first_offset,
                            GraphicsDiagKind::UnknownReference,
                            format!("kitty a=p references untransmitted image id {id}"),
                        ),
                    }
                }
            }
        }
        (data, references)
    }

    /// Retain transmitted bytes for later `a=p` (bounded table).
    fn retain_transmitted(
        &mut self,
        first_offset: usize,
        action: &str,
        first_params: &[(String, String)],
        data: &[u8],
    ) {
        if (action == "t" || action == "T" || action == "f") && !data.is_empty() {
            if let Some(id) =
                Self::param_lookup(first_params, "i").and_then(|s| s.parse::<u32>().ok())
            {
                if id != 0 && !self.images.contains_key(&id) {
                    if self.images.len() >= self.policy.max_images {
                        if !self.images_full {
                            self.images_full = true;
                            self.diag(
                                first_offset,
                                GraphicsDiagKind::Truncated,
                                format!(
                                    "transmitted-image table full ({}); id {id} inspected but not retained",
                                    self.policy.max_images
                                ),
                            );
                        }
                    } else {
                        self.images.insert(id, data.to_vec());
                    }
                }
            }
        }
    }

    fn param_lookup<'p>(params: &'p [(String, String)], key: &str) -> Option<&'p str> {
        params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Best-effort placement extraction. Display actions (`t`/`T`/`p`) read
    /// `X`/`Y`/`z`/`c`/`r`/`w`/`h`; animation actions (`f`/`a`/`c`) reuse
    /// those letters for frames/gaps/rectangles, so only `s`/`v` image dims
    /// cross over. Unparseable numbers diagnose + fall back (loud, lenient).
    fn kitty_placement(
        &mut self,
        offset: usize,
        action: &str,
        params: &[(String, String)],
    ) -> Placement {
        let mut p = Placement::default();
        match (num_pair(params, "s", "v"), action) {
            (Some(dims), _) => p.image_px = Some(dims),
            (None, _) => {
                if Self::param_lookup(params, "s").is_some()
                    || Self::param_lookup(params, "v").is_some()
                {
                    self.diag(
                        offset,
                        GraphicsDiagKind::Malformed,
                        "kitty s/v dims unparseable; image size unknown".to_string(),
                    );
                }
            }
        }
        if !matches!(action, "t" | "T" | "p") {
            return p;
        }
        if let Some(x) = Self::param_lookup(params, "X") {
            match x.parse::<u32>() {
                Ok(v) => p.dx_px = v,
                Err(_) => self.diag(
                    offset,
                    GraphicsDiagKind::Malformed,
                    format!("kitty X offset unparseable ({x:?}); using 0"),
                ),
            }
        }
        if let Some(y) = Self::param_lookup(params, "Y") {
            match y.parse::<u32>() {
                Ok(v) => p.dy_px = v,
                Err(_) => self.diag(
                    offset,
                    GraphicsDiagKind::Malformed,
                    format!("kitty Y offset unparseable ({y:?}); using 0"),
                ),
            }
        }
        match Self::param_lookup(params, "z") {
            None => p.z = Some(0),
            Some(z) => match z.parse::<i32>() {
                Ok(v) => p.z = Some(v),
                Err(_) => {
                    self.diag(
                        offset,
                        GraphicsDiagKind::Malformed,
                        format!("kitty z-index unparseable ({z:?}); using 0"),
                    );
                    p.z = Some(0);
                }
            },
        }
        p.display_cells = num_pair(params, "c", "r");
        p.display_px = num_pair(params, "w", "h");
        p
    }

    pub(crate) fn on_osc(&mut self, offset: usize, start: usize) -> usize {
        // OSC terminators: ST (ESC \ / 0x9C) or BEL.
        let s = self.stream;
        let mut j = start;
        let mut end = None;
        while j < s.len() {
            if s[j] == 0x1B && j + 1 < s.len() && s[j + 1] == b'\\' {
                end = Some((j, j + 2));
                break;
            }
            if s[j] == 0x9C || s[j] == 0x07 {
                end = Some((j, j + 1));
                break;
            }
            j += 1;
        }
        let Some((term, resume)) = end else {
            self.diag(
                offset,
                GraphicsDiagKind::Malformed,
                "unterminated OSC (no ST/BEL)".to_string(),
            );
            return start;
        };
        let content = &s[start..term];
        if content.starts_with(b"1337;") {
            self.diag(
                offset,
                GraphicsDiagKind::Unsupported,
                "iTerm2 inline image (OSC 1337): recognized, not inspected".to_string(),
            );
        }
        resume
    }
}
