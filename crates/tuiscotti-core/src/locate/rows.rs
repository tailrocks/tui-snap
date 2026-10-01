use super::Span;
use crate::screen::Screen;

/// One physical row as chars with per-char grid columns. Continuation cells
/// contribute no chars; each char of a lead symbol maps to the lead's column,
/// and the row's end column accounts for the last lead's display width.
/// Trailing blank cells are excluded.
pub(crate) struct RowText {
    chars: Vec<char>,
    /// Grid column of each char's lead cell.
    cols: Vec<u16>,
    /// Display width of each char's lead cell.
    widths: Vec<u8>,
}

impl RowText {
    pub(crate) fn extract(screen: &Screen, y: u16) -> Self {
        let mut chars = Vec::new();
        let mut cols = Vec::new();
        let mut widths = Vec::new();
        for x in 0..screen.cols() {
            let Some(cell) = screen.get(x, y) else {
                continue;
            };
            if cell.continuation {
                continue;
            }
            for c in cell.symbol.chars() {
                chars.push(c);
                cols.push(cell.x);
                widths.push(cell.width);
            }
        }
        while chars.last() == Some(&' ') {
            chars.pop();
            cols.pop();
            widths.pop();
        }
        Self {
            chars,
            cols,
            widths,
        }
    }

    /// Wrap heuristic: the row's last column holds a non-blank cell, so text
    /// runs to the edge and (by convention) continues on the next row.
    pub(crate) fn row_full(screen: &Screen, y: u16) -> bool {
        let last = screen.cols() - 1;
        let Some(cell) = screen.get(last, y) else {
            return false;
        };
        if cell.continuation {
            return true;
        }
        cell.symbol != " " && !cell.symbol.is_empty()
    }
}

/// A logical line: one physical row, or consecutive rows joined by the wrap
/// heuristic. `row_of[i]`/`col_of[i]`/`width_of[i]` locate char `i`.
pub(crate) struct LogicalLine {
    pub(crate) chars: Vec<char>,
    pub(crate) rows: Vec<u16>,
    pub(crate) cols: Vec<u16>,
    pub(crate) widths: Vec<u8>,
    pub(crate) end_row: u16,
}

pub(crate) fn logical_lines(screen: &Screen, join_wrapped: bool) -> Vec<LogicalLine> {
    let mut lines = Vec::new();
    let mut y = 0;
    while y < screen.rows() {
        let mut line = LogicalLine {
            chars: Vec::new(),
            rows: Vec::new(),
            cols: Vec::new(),
            widths: Vec::new(),
            end_row: y,
        };
        loop {
            let row = RowText::extract(screen, y);
            for i in 0..row.chars.len() {
                line.chars.push(row.chars[i]);
                line.rows.push(y);
                line.cols.push(row.cols[i]);
                line.widths.push(row.widths[i]);
            }
            line.end_row = y;
            if !join_wrapped || y + 1 >= screen.rows() || !RowText::row_full(screen, y) {
                break;
            }
            y += 1;
        }
        lines.push(line);
        y += 1;
    }
    lines
}

pub(crate) fn span_for_char_range(
    line: &LogicalLine,
    start: usize,
    end: usize,
    origin: (i32, i32),
    revision: u64,
    cols: u16,
) -> Span {
    let text: String = line.chars[start..end].iter().collect();
    let x = line.cols[start];
    let y = line.rows[start];
    let (end_x, end_y) = if end > start {
        let last = end - 1;
        (
            line.cols[last] + u16::from(line.widths[last]),
            line.rows[last],
        )
    } else {
        (x, y)
    };
    let width_cols = if end_y == y {
        end_x.saturating_sub(x)
    } else {
        (end_y - y - 1) * cols + (cols - x) + end_x
    };
    Span {
        x,
        y,
        end_x,
        end_y,
        width_cols,
        origin,
        revision,
        text,
        scrollback: false,
        scrollback_index: None,
    }
}
