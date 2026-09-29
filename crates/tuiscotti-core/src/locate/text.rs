use super::regex::{MiniRegex, fold_char};
use super::rows::{logical_lines, span_for_char_range};
use super::{LocateError, Locator, Span, TextMode};
use crate::screen::Screen;

/// Collapse every whitespace run to one space; returns normalized chars plus
/// the source index of each normalized char.
fn normalize_with_map(chars: &[char]) -> (Vec<char>, Vec<usize>) {
    let mut out = Vec::new();
    let mut map = Vec::new();
    let mut in_ws = true; // also trims leading whitespace
    for (i, c) in chars.iter().enumerate() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(' ');
                map.push(i);
                in_ws = true;
            }
        } else {
            out.push(*c);
            map.push(i);
            in_ws = false;
        }
    }
    if out.last() == Some(&' ') {
        out.pop();
        map.pop();
    }
    (out, map)
}

fn find_substring(hay: &[char], needle: &[char]) -> Vec<(usize, usize)> {
    if needle.is_empty() || needle.len() > hay.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut s = 0;
    while s + needle.len() <= hay.len() {
        if hay[s..s + needle.len()] == needle[..] {
            out.push((s, s + needle.len()));
            s += needle.len();
        } else {
            s += 1;
        }
    }
    out
}

/// Char ranges of `pattern` in `chars` under `mode` (scrollback/logical-line
/// shared core; match text is the ORIGINAL slice).
fn text_ranges(pattern: &str, mode: TextMode, chars: &[char]) -> Vec<(usize, usize)> {
    match mode {
        TextMode::Substring => {
            let needle: Vec<char> = pattern.chars().collect();
            find_substring(chars, &needle)
        }
        TextMode::Exact => {
            let text: String = chars.iter().collect();
            if text == pattern {
                vec![(0, chars.len())]
            } else {
                Vec::new()
            }
        }
        TextMode::CaseInsensitive => {
            let needle: Vec<char> = pattern.chars().map(fold_char).collect();
            let folded: Vec<char> = chars.iter().map(|c| fold_char(*c)).collect();
            find_substring(&folded, &needle)
        }
        TextMode::Normalized => {
            let (hay_n, map) = normalize_with_map(chars);
            let pat_chars: Vec<char> = pattern.chars().collect();
            let (pat_n, _) = normalize_with_map(&pat_chars);
            find_substring(&hay_n, &pat_n)
                .into_iter()
                .map(|(s, e)| (map[s], map[e - 1] + 1))
                .collect()
        }
    }
}

pub(crate) fn match_text_scrollback(
    pattern: &str,
    mode: TextMode,
    line: &str,
    index: usize,
    origin: (i32, i32),
    revision: u64,
) -> Result<Vec<Span>, LocateError> {
    let chars: Vec<char> = line.chars().collect();
    if mode == TextMode::CaseInsensitive && chars.iter().any(|c| !c.is_ascii()) {
        return Err(LocateError::Unsupported(
            "case-insensitive text over non-ASCII text: first-char lowering is unreliable"
                .to_string(),
        ));
    }
    Ok(text_ranges(pattern, mode, &chars)
        .into_iter()
        .map(|(s, e)| {
            let text: String = chars[s..e].iter().collect();
            Span {
                x: s as u16,
                y: 0,
                end_x: e as u16,
                end_y: 0,
                width_cols: (e - s) as u16,
                origin,
                revision,
                text,
                scrollback: true,
                scrollback_index: Some(index),
            }
        })
        .collect())
}

pub(crate) fn match_text_viewport(
    pattern: &str,
    mode: TextMode,
    join_wrapped: bool,
    screen: &Screen,
    revision: u64,
) -> Result<Vec<Span>, LocateError> {
    let mut out = Vec::new();
    for line in logical_lines(screen, join_wrapped) {
        if mode == TextMode::CaseInsensitive && line.chars.iter().any(|c| !c.is_ascii()) {
            return Err(LocateError::Unsupported(
                "case-insensitive text over non-ASCII text: first-char lowering is unreliable"
                    .to_string(),
            ));
        }
        for (s, e) in text_ranges(pattern, mode, &line.chars) {
            out.push(span_for_char_range(
                &line,
                s,
                e,
                screen.origin(),
                revision,
                screen.cols(),
            ));
        }
    }
    Ok(out)
}

pub(crate) fn match_regex_scrollback(
    re: &MiniRegex,
    chars: &[char],
    index: usize,
    origin: (i32, i32),
    revision: u64,
) -> Vec<Span> {
    re.find_all(chars)
        .into_iter()
        .map(|(s, e)| {
            let text: String = chars[s..e].iter().collect();
            Span {
                x: s as u16,
                y: 0,
                end_x: e as u16,
                end_y: 0,
                width_cols: (e - s) as u16,
                origin,
                revision,
                text,
                scrollback: true,
                scrollback_index: Some(index),
            }
        })
        .collect()
}

pub(crate) fn match_regex_viewport(
    re: &MiniRegex,
    source: &str,
    join_wrapped: bool,
    screen: &Screen,
    revision: u64,
) -> Result<Vec<Span>, LocateError> {
    let mut out = Vec::new();
    for line in logical_lines(screen, join_wrapped) {
        Locator::check_regex_haystack(re, &line.chars, source)?;
        for (s, e) in re.find_all(&line.chars) {
            out.push(span_for_char_range(
                &line,
                s,
                e,
                screen.origin(),
                revision,
                screen.cols(),
            ));
        }
    }
    Ok(out)
}
