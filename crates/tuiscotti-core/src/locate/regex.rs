use super::LocateError;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReToken {
    Start,
    End,
    Dot,
    Lit(char),
    Class {
        ranges: Vec<(char, char)>,
        negated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReAtom {
    token: ReToken,
    min: usize,
    max: Option<usize>, // None = unbounded
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MiniRegex {
    atoms: Vec<ReAtom>,
    pub(crate) case_insensitive: bool,
}

pub(crate) fn fold_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

impl MiniRegex {
    pub(crate) fn parse(pattern: &str, case_insensitive: bool) -> Result<Self, LocateError> {
        let usage = |m: String| LocateError::Usage(format!("bad regex {pattern:?}: {m}"));
        if pattern.is_empty() {
            return Err(usage("pattern is empty".to_string()));
        }
        let raw: Vec<char> = pattern.chars().collect();
        let mut atoms: Vec<ReAtom> = Vec::new();
        let mut i = 0;
        let push = |atoms: &mut Vec<ReAtom>, token: ReToken| {
            atoms.push(ReAtom {
                token,
                min: 1,
                max: Some(1),
            });
        };
        while i < raw.len() {
            let c = raw[i];
            match c {
                '^' if i == 0 => push(&mut atoms, ReToken::Start),
                '$' if i == raw.len() - 1 => push(&mut atoms, ReToken::End),
                '.' => push(&mut atoms, ReToken::Dot),
                '*' | '+' | '?' => Self::apply_quantifier(&mut atoms, c, pattern)?,
                '(' | ')' | '|' | '{' | '}' => {
                    return Err(usage(format!(
                        "unsupported construct {c:?}: no groups, alternation, or {{m,n}}"
                    )));
                }
                '[' => {
                    let (token, next) = Self::parse_class(&raw, i)?;
                    push(&mut atoms, token);
                    i = next;
                    continue;
                }
                '\\' => {
                    i += 1;
                    if i >= raw.len() {
                        return Err(usage("trailing backslash".to_string()));
                    }
                    push(&mut atoms, Self::escape_token(raw[i]));
                }
                lit => push(&mut atoms, ReToken::Lit(lit)),
            }
            i += 1;
        }
        let mut re = Self {
            atoms,
            case_insensitive,
        };
        if case_insensitive {
            re.fold_pattern();
        }
        Ok(re)
    }

    /// Apply a `*`/`+`/`?` quantifier to the last pushed atom. Dangling
    /// quantifiers and quantifiers on anchors are usage errors.
    fn apply_quantifier(
        atoms: &mut Vec<ReAtom>,
        c: char,
        pattern: &str,
    ) -> Result<(), LocateError> {
        let usage = |m: String| LocateError::Usage(format!("bad regex {pattern:?}: {m}"));
        let prev = atoms
            .last_mut()
            .ok_or_else(|| usage("dangling quantifier".to_string()))?;
        if matches!(prev.token, ReToken::Start | ReToken::End) {
            return Err(usage("quantifier on anchor".to_string()));
        }
        match c {
            '*' => {
                prev.min = 0;
                prev.max = None;
            }
            '+' => {
                prev.min = 1;
                prev.max = None;
            }
            _ => {
                prev.min = 0;
                prev.max = Some(1);
            }
        }
        Ok(())
    }

    /// Token for one `\X` escape: `\d \w \s` classes (plus negations) or the
    /// escaped literal.
    fn escape_token(esc: char) -> ReToken {
        match esc {
            'd' => ReToken::Class {
                ranges: vec![('0', '9')],
                negated: false,
            },
            'w' => ReToken::Class {
                ranges: vec![('A', 'Z'), ('a', 'z'), ('0', '9'), ('_', '_')],
                negated: false,
            },
            's' => ReToken::Class {
                ranges: vec![(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')],
                negated: false,
            },
            'D' => ReToken::Class {
                ranges: vec![('0', '9')],
                negated: true,
            },
            'W' => ReToken::Class {
                ranges: vec![('A', 'Z'), ('a', 'z'), ('0', '9'), ('_', '_')],
                negated: true,
            },
            'S' => ReToken::Class {
                ranges: vec![(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')],
                negated: true,
            },
            lit => ReToken::Lit(lit),
        }
    }

    fn parse_class(raw: &[char], open: usize) -> Result<(ReToken, usize), LocateError> {
        let usage = |m: String| LocateError::Usage(format!("bad regex class: {m}"));
        let mut i = open + 1;
        let mut negated = false;
        if i < raw.len() && raw[i] == '^' {
            negated = true;
            i += 1;
        }
        let mut ranges: Vec<(char, char)> = Vec::new();
        let mut first = true;
        while i < raw.len() {
            let c = raw[i];
            if c == ']' && !first {
                if ranges.is_empty() {
                    return Err(usage("empty class".to_string()));
                }
                return Ok((ReToken::Class { ranges, negated }, i + 1));
            }
            first = false;
            let lo = if c == '\\' {
                i += 1;
                if i >= raw.len() {
                    return Err(usage("trailing backslash in class".to_string()));
                }
                match raw[i] {
                    'd' => {
                        ranges.push(('0', '9'));
                        i += 1;
                        continue;
                    }
                    'w' => {
                        ranges.extend([('A', 'Z'), ('a', 'z'), ('0', '9'), ('_', '_')]);
                        i += 1;
                        continue;
                    }
                    's' => {
                        ranges.extend([(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')]);
                        i += 1;
                        continue;
                    }
                    lit => lit,
                }
            } else {
                c
            };
            if i + 2 < raw.len() && raw[i + 1] == '-' && raw[i + 2] != ']' {
                let hi = raw[i + 2];
                if hi < lo {
                    return Err(usage(format!("reversed range {lo:?}-{hi:?}")));
                }
                ranges.push((lo, hi));
                i += 3;
            } else {
                ranges.push((lo, lo));
                i += 1;
            }
        }
        Err(usage("unclosed `[`".to_string()))
    }

    fn fold_pattern(&mut self) {
        for atom in &mut self.atoms {
            match &mut atom.token {
                ReToken::Lit(c) => *c = fold_char(*c),
                ReToken::Class { ranges, .. } => {
                    for (lo, hi) in ranges.iter_mut() {
                        *lo = fold_char(*lo);
                        *hi = fold_char(*hi);
                    }
                }
                _ => {}
            }
        }
    }

    fn atom_matches(token: &ReToken, c: char) -> bool {
        match token {
            ReToken::Dot => true,
            ReToken::Lit(l) => *l == c,
            ReToken::Class { ranges, negated } => {
                let hit = ranges.iter().any(|(lo, hi)| *lo <= c && c <= *hi);
                hit != *negated
            }
            ReToken::Start | ReToken::End => false,
        }
    }

    /// Greedy backtracking match of `atoms[ai..]` at `ci`; returns end index.
    fn match_rest(&self, ai: usize, chars: &[char], ci: usize) -> Option<usize> {
        if ai == self.atoms.len() {
            return Some(ci);
        }
        let atom = &self.atoms[ai];
        match &atom.token {
            ReToken::Start if ci == 0 => self.match_rest(ai + 1, chars, ci),
            ReToken::End if ci == chars.len() => self.match_rest(ai + 1, chars, ci),
            ReToken::Start | ReToken::End => None,
            _ => {
                let mut ends = vec![ci];
                let mut j = ci;
                while atom.max.is_none_or(|m| ends.len() - 1 < m)
                    && j < chars.len()
                    && Self::atom_matches(&atom.token, chars[j])
                {
                    j += 1;
                    ends.push(j);
                }
                for k in (atom.min..ends.len()).rev() {
                    if let Some(end) = self.match_rest(ai + 1, chars, ends[k]) {
                        return Some(end);
                    }
                }
                None
            }
        }
    }

    /// Leftmost non-overlapping matches as char ranges. Empty matches are
    /// skipped (documented): patterns that match empty advance one char.
    pub(crate) fn find_all(&self, chars: &[char]) -> Vec<(usize, usize)> {
        let hay: Vec<char> = if self.case_insensitive {
            chars.iter().map(|c| fold_char(*c)).collect()
        } else {
            chars.to_vec()
        };
        let mut out = Vec::new();
        let mut s = 0;
        while s <= hay.len() {
            match self.match_rest(0, &hay, s) {
                Some(e) if e > s => {
                    out.push((s, e));
                    s = e;
                }
                _ => s += 1,
            }
        }
        out
    }
}
