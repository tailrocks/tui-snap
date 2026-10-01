use super::{LocateError, Span, StyleQuery};
use crate::screen::Screen;

pub(crate) fn match_style_viewport(
    query: &StyleQuery,
    screen: &Screen,
    revision: u64,
) -> Vec<Span> {
    let mut out = Vec::new();
    for y in 0..screen.rows() {
        let mut run_start: Option<u16> = None;
        let mut run_text = String::new();
        let mut run_end = 0u16;
        let flush = |out: &mut Vec<Span>,
                     run_start: &mut Option<u16>,
                     run_text: &mut String,
                     run_end: u16| {
            if let Some(x) = run_start.take() {
                out.push(Span {
                    x,
                    y,
                    end_x: run_end,
                    end_y: y,
                    width_cols: run_end.saturating_sub(x),
                    origin: screen.origin(),
                    revision,
                    text: std::mem::take(run_text),
                    scrollback: false,
                    scrollback_index: None,
                });
            }
        };
        for x in 0..screen.cols() {
            let Some(cell) = screen.get(x, y) else {
                continue;
            };
            if cell.continuation {
                continue;
            }
            if query.matches(cell) {
                if run_start.is_none() {
                    run_start = Some(x);
                }
                run_text.push_str(&cell.symbol);
                run_end = x + u16::from(cell.width);
            } else {
                flush(&mut out, &mut run_start, &mut run_text, run_end);
            }
        }
        flush(&mut out, &mut run_start, &mut run_text, run_end);
    }
    out
}

pub(crate) fn match_region_viewport(
    x: u16,
    y: u16,
    cols: u16,
    rows: u16,
    screen: &Screen,
    revision: u64,
) -> Result<Vec<Span>, LocateError> {
    if cols == 0 || rows == 0 {
        return Err(LocateError::Usage(
            "region dimensions must be nonzero".to_string(),
        ));
    }
    if u32::from(x) + u32::from(cols) > u32::from(screen.cols())
        || u32::from(y) + u32::from(rows) > u32::from(screen.rows())
    {
        return Err(LocateError::Usage(format!(
            "region ({x},{y}) {cols}x{rows} outside {}x{} screen",
            screen.cols(),
            screen.rows()
        )));
    }
    // Never silently cut a wide grapheme (Q10): same rule as Screen::region.
    let missing = |cx: u16, cy: u16| {
        LocateError::Usage(format!(
            "region cell ({cx},{cy}) outside {}x{} screen",
            screen.cols(),
            screen.rows()
        ))
    };
    for r in y..y + rows {
        let left = screen.get(x, r).ok_or_else(|| missing(x, r))?;
        if left.continuation {
            return Err(LocateError::Usage(format!(
                "region left edge ({x},{r}) splits a wide grapheme"
            )));
        }
        let right = screen
            .get(x + cols - 1, r)
            .ok_or_else(|| missing(x + cols - 1, r))?;
        if right.width == 2 && !right.continuation {
            return Err(LocateError::Usage(format!(
                "region right edge ({},{r}) splits a wide grapheme",
                x + cols - 1
            )));
        }
    }
    let mut out = Vec::new();
    for r in y..y + rows {
        let mut text = String::new();
        for c in x..x + cols {
            let cell = screen.get(c, r).ok_or_else(|| missing(c, r))?;
            if !cell.continuation {
                text.push_str(&cell.symbol);
            }
        }
        out.push(Span {
            x,
            y: r,
            end_x: x + cols,
            end_y: r,
            width_cols: cols,
            origin: screen.origin(),
            revision,
            text,
            scrollback: false,
            scrollback_index: None,
        });
    }
    Ok(out)
}
