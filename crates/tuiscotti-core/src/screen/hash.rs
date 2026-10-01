use crate::frame::{Cell, Cursor, Rgb};
use std::hash::{Hash, Hasher};

// ---------------------------------------------------------------------------
// Hash impls for frame types (same crate, so these overlap-free impls live here
// to keep `frame.rs` untouched).
// ---------------------------------------------------------------------------

impl Hash for Rgb {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.r.hash(state);
        self.g.hash(state);
        self.b.hash(state);
    }
}

impl Hash for crate::frame::Color {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            crate::frame::Color::Default => {}
            crate::frame::Color::Indexed(n) => n.hash(state),
            crate::frame::Color::Rgb(rgb) => rgb.hash(state),
        }
    }
}

impl Hash for crate::frame::Mods {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hidden.hash(state);
        self.blink.hash(state);
        self.bold.hash(state);
        self.dim.hash(state);
        self.italic.hash(state);
        self.underline.hash(state);
        self.underline_style.hash(state);
        self.strikethrough.hash(state);
        self.reverse.hash(state);
    }
}

impl Hash for Cell {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x.hash(state);
        self.y.hash(state);
        self.symbol.hash(state);
        self.width.hash(state);
        self.continuation.hash(state);
        self.fg.hash(state);
        self.bg.hash(state);
        self.mods.hash(state);
        self.underline_color.hash(state);
    }
}

impl Hash for crate::frame::CursorStyle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
    }
}

impl Hash for Cursor {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x.hash(state);
        self.y.hash(state);
        self.visible.hash(state);
        self.style.hash(state);
        self.blinking.hash(state);
    }
}
