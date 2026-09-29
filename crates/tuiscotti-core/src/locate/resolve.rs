use super::locator::LocatorKind;
use super::regex::MiniRegex;
use super::text::{
    match_regex_scrollback, match_regex_viewport, match_text_scrollback, match_text_viewport,
};
use super::viewport::{match_region_viewport, match_style_viewport};
use super::{LocateError, Locator, Span, TextMode};
use crate::screen::{Observation, Screen};

impl Locator {
    /// Resolve against a screen at a revision (viewport only).
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::Usage`] for invalid patterns/regions and
    /// [`LocateError::Unsupported`] for non-ASCII case-insensitive haystacks.
    pub fn resolve(&self, screen: &Screen, revision: u64) -> Result<Vec<Span>, LocateError> {
        self.resolve_with_scrollback(screen, revision, &[])
    }

    /// Resolve against an [`Observation`] (viewport only).
    ///
    /// # Errors
    ///
    /// Same failures as [`Locator::resolve`].
    pub fn resolve_obs(&self, obs: &Observation) -> Result<Vec<Span>, LocateError> {
        self.resolve(&obs.screen, obs.revision)
    }

    /// Resolve against a screen plus scrollback lines (plain text, oldest
    /// first). Scrollback matches are flagged `scrollback: true` and are
    /// never clickable.
    ///
    /// # Errors
    ///
    /// Same failures as [`Locator::resolve`].
    pub fn resolve_with_scrollback(
        &self,
        screen: &Screen,
        revision: u64,
        scrollback: &[String],
    ) -> Result<Vec<Span>, LocateError> {
        let mut spans = self.resolve_core(screen, revision, scrollback)?;
        spans.sort_by(|a, b| a.key().cmp(&b.key()).then(a.end_key().cmp(&b.end_key())));
        Ok(spans)
    }

    /// Resolve requiring exactly one match: 0 → [`LocateError::NotFound`],
    /// 2+ → [`LocateError::Ambiguous`] listing every match (Q03 strictness).
    ///
    /// # Errors
    ///
    /// Returns [`LocateError::NotFound`] on zero matches,
    /// [`LocateError::Ambiguous`] on 2+, plus [`Locator::resolve`] failures.
    pub fn resolve_unique(&self, screen: &Screen, revision: u64) -> Result<Span, LocateError> {
        self.resolve_unique_with_scrollback(screen, revision, &[])
    }

    /// [`Locator::resolve_unique`] with scrollback lines included.
    ///
    /// # Errors
    ///
    /// Same failures as [`Locator::resolve_unique`].
    pub fn resolve_unique_with_scrollback(
        &self,
        screen: &Screen,
        revision: u64,
        scrollback: &[String],
    ) -> Result<Span, LocateError> {
        let spans = self.resolve_with_scrollback(screen, revision, scrollback)?;
        if spans.len() > 1 {
            return Err(LocateError::Ambiguous { matches: spans });
        }
        match spans.into_iter().next() {
            Some(span) => Ok(span),
            None => Err(LocateError::not_found("no matches")),
        }
    }

    fn resolve_core(
        &self,
        screen: &Screen,
        revision: u64,
        scrollback: &[String],
    ) -> Result<Vec<Span>, LocateError> {
        match &self.kind {
            LocatorKind::Text { pattern, mode } => {
                self.resolve_text_leaf(screen, revision, scrollback, pattern, *mode)
            }
            LocatorKind::Regex { source, re } => {
                self.resolve_regex_leaf(screen, revision, scrollback, source, re)
            }
            // Scrollback carries no cell geometry, so style/region locators
            // are viewport-only by construction (documented, not an error).
            LocatorKind::Style { query } => Ok(match_style_viewport(query, screen, revision)),
            LocatorKind::Region { x, y, cols, rows } => {
                match_region_viewport(*x, *y, *cols, *rows, screen, revision)
            }
            LocatorKind::Within { scope, inner } => {
                let outer = scope.resolve_core(screen, revision, scrollback)?;
                let inner = inner.resolve_core(screen, revision, scrollback)?;
                Ok(inner
                    .into_iter()
                    .filter(|s| outer.iter().any(|o| contains(o, s)))
                    .collect())
            }
            LocatorKind::Before { main, anchor } => {
                let main = main.resolve_core(screen, revision, scrollback)?;
                let anchor = anchor.resolve_core(screen, revision, scrollback)?;
                Ok(main
                    .into_iter()
                    .filter(|s| anchor.iter().any(|a| a.key() > s.key()))
                    .collect())
            }
            LocatorKind::After { main, anchor } => {
                let main = main.resolve_core(screen, revision, scrollback)?;
                let anchor = anchor.resolve_core(screen, revision, scrollback)?;
                Ok(main
                    .into_iter()
                    .filter(|s| anchor.iter().any(|a| a.key() < s.key()))
                    .collect())
            }
            LocatorKind::Nth { inner, index } => {
                let mut spans = inner.resolve_core(screen, revision, scrollback)?;
                spans.sort_by_key(Span::key);
                Ok(spans.into_iter().nth(*index).into_iter().collect())
            }
            LocatorKind::First(inner) => {
                let mut spans = inner.resolve_core(screen, revision, scrollback)?;
                spans.sort_by_key(Span::key);
                Ok(spans.into_iter().next().into_iter().collect())
            }
            LocatorKind::Last(inner) => {
                let mut spans = inner.resolve_core(screen, revision, scrollback)?;
                spans.sort_by_key(Span::key);
                Ok(spans.into_iter().last().into_iter().collect())
            }
            LocatorKind::And { a, b } => {
                let a = a.resolve_core(screen, revision, scrollback)?;
                let b = b.resolve_core(screen, revision, scrollback)?;
                Ok(a.into_iter()
                    .filter(|s| b.iter().any(|o| overlaps(s, o)))
                    .collect())
            }
            LocatorKind::Or { a, b } => {
                let mut out = a.resolve_core(screen, revision, scrollback)?;
                for s in b.resolve_core(screen, revision, scrollback)? {
                    if !out.contains(&s) {
                        out.push(s);
                    }
                }
                Ok(out)
            }
            LocatorKind::Filter { inner, pred } => {
                let spans = inner.resolve_core(screen, revision, scrollback)?;
                Ok(spans.into_iter().filter(pred).collect())
            }
        }
    }

    /// Resolve one text leaf: scrollback lines plus the viewport.
    fn resolve_text_leaf(
        &self,
        screen: &Screen,
        revision: u64,
        scrollback: &[String],
        pattern: &str,
        mode: TextMode,
    ) -> Result<Vec<Span>, LocateError> {
        Self::check_text_pattern(pattern)?;
        let mut out = Vec::new();
        for (idx, line) in scrollback.iter().enumerate() {
            out.extend(match_text_scrollback(
                pattern,
                mode,
                line,
                idx,
                screen.origin(),
                revision,
            )?);
        }
        out.extend(match_text_viewport(
            pattern,
            mode,
            self.join_wrapped,
            screen,
            revision,
        )?);
        Ok(out)
    }

    /// Resolve one regex leaf: scrollback lines plus the viewport.
    fn resolve_regex_leaf(
        &self,
        screen: &Screen,
        revision: u64,
        scrollback: &[String],
        source: &str,
        re: &MiniRegex,
    ) -> Result<Vec<Span>, LocateError> {
        let mut out = Vec::new();
        for (idx, line) in scrollback.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            Self::check_regex_haystack(re, &chars, source)?;
            out.extend(match_regex_scrollback(
                re,
                &chars,
                idx,
                screen.origin(),
                revision,
            ));
        }
        out.extend(match_regex_viewport(
            re,
            source,
            self.join_wrapped,
            screen,
            revision,
        )?);
        Ok(out)
    }

    fn check_text_pattern(pattern: &str) -> Result<(), LocateError> {
        if pattern.is_empty() {
            return Err(LocateError::Usage("text pattern is empty".to_string()));
        }
        if pattern.contains('\n') {
            return Err(LocateError::Usage(
                "text pattern contains newline; match one logical line (wrapped-line joining is on by default)".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn check_regex_haystack(
        re: &MiniRegex,
        chars: &[char],
        source: &str,
    ) -> Result<(), LocateError> {
        if re.case_insensitive && chars.iter().any(|c| !c.is_ascii()) {
            return Err(LocateError::Unsupported(format!(
                "case-insensitive regex {source:?} over non-ASCII text: first-char lowering is unreliable"
            )));
        }
        Ok(())
    }
}

/// True when `outer` fully contains `inner` (same area, range inside).
fn contains(outer: &Span, inner: &Span) -> bool {
    if outer.scrollback != inner.scrollback {
        return false;
    }
    if outer.scrollback {
        return outer.scrollback_index == inner.scrollback_index
            && outer.x <= inner.x
            && inner.end_x <= outer.end_x;
    }
    outer.key() <= inner.key() && inner.end_key() <= outer.end_key()
}

/// True when both spans share an area+line and their column ranges intersect.
fn overlaps(a: &Span, b: &Span) -> bool {
    if a.scrollback != b.scrollback {
        return false;
    }
    if a.scrollback {
        if a.scrollback_index != b.scrollback_index {
            return false;
        }
    } else if a.y != b.y || a.end_y != b.end_y {
        return false;
    }
    a.x < b.end_x && b.x < a.end_x
}
