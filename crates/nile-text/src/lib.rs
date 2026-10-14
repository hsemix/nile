//! Text shaping and line breaking, via Parley.
//!
//! Parley handles font fallback (CoreText on macOS, DirectWrite on Windows,
//! fontconfig on Linux), shaping, bidi, and line breaking. This crate keeps the
//! engine-facing surface small so the rest of the engine doesn't depend on
//! Parley's API directly.

use std::borrow::Cow;
use std::ops::Range;

pub use parley::{
    Affinity, Alignment, BoundingBox, Cluster, Cursor, Layout, Line, PositionedLayoutItem, Selection,
};
use parley::{InlineBox, InlineBoxKind, VerticalAlign};
use parley::{
    AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, LayoutContext, LineHeight, StyleProperty,
};

/// Colors carried through shaping so painting knows each run's look.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextBrush {
    pub color: [u8; 4],
    /// 1-based index of the innermost decorated inline element (background,
    /// border, padding) this run belongs to; 0 = none. Layout owns the table.
    pub deco: u32,
    /// A padding/border spacer run: only its horizontal extent is meaningful.
    pub spacer: bool,
    /// Baseline shift in px, positive = up (`vertical-align`).
    pub shift: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: f32,
    pub line_height: f32,
    pub color: [u8; 4],
    /// See [`TextBrush::deco`].
    pub deco: u32,
    pub spacer: bool,
    /// See [`TextBrush::shift`].
    pub shift: f32,
    pub italic: bool,
    pub underline: bool,
    pub line_through: bool,
    pub letter_spacing: f32,
    pub align: Alignment,
}

/// A box of known size placed inline at a byte offset of the text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InlineBoxSpec {
    pub id: u64,
    pub index: usize,
    pub width: f32,
    pub height: f32,
    /// Baseline offset from the box's top edge; `None` = its bottom edge.
    pub baseline: Option<f32>,
}

pub type TextLayout = Layout<TextBrush>;

/// Shared font database and layout scratch space. Create one per engine.
#[derive(Default)]
pub struct TextEngine {
    fonts: FontContext,
    layouts: LayoutContext<TextBrush>,
    nbsp_ratio: std::collections::HashMap<(String, u32), f32>,
    /// `@font-face` fonts registered so far (family, weight bits, italic, source id), so each loads once.
    registered: std::collections::HashSet<(String, u32, bool, String)>,
}

impl TextEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether [`register_font`](Self::register_font) already took this font (so the caller can skip loading it).
    pub fn has_font(&self, family: &str, id: &str, weight: Option<f32>, italic: Option<bool>) -> bool {
        self.registered.contains(&(family.to_ascii_lowercase(), weight.map_or(0, f32::to_bits), italic.unwrap_or(false), id.to_string()))
    }

    /// Add a web font under `family` (what `@font-face` does). `bytes` may be TrueType/OpenType, a
    /// collection, or WOFF/WOFF2. `weight` and `italic` describe this face (`None`: take them from the
    /// font, which is what a variable font wants). `id` names the source so that registering it again
    /// is free. False if the data isn't a usable font.
    pub fn register_font(&mut self, family: &str, id: &str, bytes: Vec<u8>, weight: Option<f32>, italic: Option<bool>) -> bool {
        let key = (family.to_ascii_lowercase(), weight.map_or(0, f32::to_bits), italic.unwrap_or(false), id.to_string());
        if self.registered.contains(&key) {
            return true;
        }
        let data = if bytes.starts_with(b"wOF2") {
            wuff::decompress_woff2(&bytes).ok()
        } else if bytes.starts_with(b"wOFF") {
            wuff::decompress_woff1(&bytes).ok()
        } else {
            Some(bytes)
        };
        let Some(data) = data else { return false };
        let blob = parley::fontique::Blob::from(data);
        let info = parley::fontique::FontInfoOverride {
            family_name: Some(family),
            weight: weight.map(parley::fontique::FontWeight::new),
            style: italic.map(|i| if i { parley::fontique::FontStyle::Italic } else { parley::fontique::FontStyle::Normal }),
            ..Default::default()
        };
        let added = self.fonts.collection.register_fonts(blob, Some(info));
        self.registered.insert(key);
        !added.is_empty()
    }

    /// Advance of a no-break space as a fraction of the font size. Layout uses
    /// it to size padding/border spacers: a no-break space can't be separated
    /// from its neighbours, so it keeps a spacer attached to the word after it.
    pub fn nbsp_ratio(&mut self, family: &str, weight: f32) -> f32 {
        let key = (family.to_string(), weight.to_bits());
        if let Some(r) = self.nbsp_ratio.get(&key) {
            return *r;
        }
        let style = TextStyle {
            font_family: family.to_string(),
            font_size: 100.0,
            font_weight: weight,
            line_height: 1.0,
            color: [0; 4],
            deco: 0,
            spacer: false,
            shift: 0.0,
            italic: false,
            underline: false,
            line_through: false,
            letter_spacing: 0.0,
            align: Alignment::Start,
        };
        let mut layout = self.shape("\u{a0}", &style, 1.0);
        break_lines(&mut layout, None, Alignment::Start);
        let ratio = layout.width() / 100.0;
        let ratio = if ratio > 0.01 { ratio } else { 0.25 };
        self.nbsp_ratio.insert(key, ratio);
        ratio
    }

    /// Shape `text`. Call [`break_lines`] before reading positions.
    pub fn shape(&mut self, text: &str, style: &TextStyle, scale: f32) -> TextLayout {
        self.shape_spans(text, style, &[], &[], scale)
    }

    /// Shape `text` with `base` as the default style and `spans` (byte ranges
    /// into `text`) overriding font, size, weight, line height and color.
    /// Later spans win where they overlap. `boxes` are laid out in the flow
    /// (like `inline-block`); find them again via `PositionedLayoutItem::InlineBox`.
    pub fn shape_spans(
        &mut self,
        text: &str,
        base: &TextStyle,
        spans: &[(Range<usize>, TextStyle)],
        boxes: &[InlineBoxSpec],
        scale: f32,
    ) -> TextLayout {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, scale, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(
            base.font_family.clone(),
        ))));
        b.push_default(StyleProperty::FontSize(base.font_size));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(base.font_weight)));
        b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(base.line_height)));
        b.push_default(StyleProperty::Brush(brush(base)));
        b.push_default(StyleProperty::FontStyle(font_style(base)));
        b.push_default(StyleProperty::Underline(base.underline));
        b.push_default(StyleProperty::Strikethrough(base.line_through));
        b.push_default(StyleProperty::LetterSpacing(base.letter_spacing));
        for (range, s) in spans {
            if range.is_empty() || *s == *base {
                continue;
            }
            b.push(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(s.font_family.clone()))), range.clone());
            b.push(StyleProperty::FontSize(s.font_size), range.clone());
            b.push(StyleProperty::FontWeight(FontWeight::new(s.font_weight)), range.clone());
            b.push(StyleProperty::LineHeight(LineHeight::FontSizeRelative(s.line_height)), range.clone());
            b.push(StyleProperty::Brush(brush(s)), range.clone());
            b.push(StyleProperty::FontStyle(font_style(s)), range.clone());
            b.push(StyleProperty::Underline(s.underline), range.clone());
            b.push(StyleProperty::Strikethrough(s.line_through), range.clone());
            b.push(StyleProperty::LetterSpacing(s.letter_spacing), range.clone());
        }
        // Symbol characters the system fallback misses get named fallback fonts.
        for (i, c) in text.char_indices().filter(|(_, c)| is_symbol_gap(*c)) {
            let family = spans
                .iter()
                .rev()
                .find(|(r, _)| r.contains(&i))
                .map_or(&base.font_family, |(_, s)| &s.font_family);
            b.push(
                StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(with_symbol_fallbacks(family)))),
                i..i + c.len_utf8(),
            );
        }
        for bx in boxes {
            b.push_inline_box(InlineBox {
                id: bx.id,
                kind: InlineBoxKind::InFlow,
                index: bx.index,
                width: bx.width,
                height: bx.height,
                baseline: bx.baseline,
                vertical_align: VerticalAlign::BASELINE,
            });
        }
        b.build(text)
    }
}

/// System fallback finds fonts per script (CJK, Arabic, ...) but not for some
/// symbol blocks, so characters in them get these fonts named explicitly; ones
/// that aren't installed are skipped. Other symbols (`★`, `→`, ...) keep the
/// system's choice, which looks better where it works.
const SYMBOL_FALLBACKS: &str =
    "Apple Symbols, Zapf Dingbats, Segoe UI Symbol, Noto Sans Symbols 2, Noto Sans Symbols, DejaVu Sans";

/// The end of Miscellaneous Symbols (`⚠ ⚡ ⛔ ...`) through Dingbats (`✓ ✔ ✗ ✂ ...`).
fn is_symbol_gap(c: char) -> bool {
    matches!(c as u32, 0x26A0..=0x27BF)
}

fn with_symbol_fallbacks(family: &str) -> String {
    format!("{family}, {SYMBOL_FALLBACKS}")
}

fn brush(s: &TextStyle) -> TextBrush {
    TextBrush { color: s.color, deco: s.deco, spacer: s.spacer, shift: s.shift }
}

fn font_style(s: &TextStyle) -> FontStyle {
    if s.italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    }
}

/// Break lines to `max_width` (None = no wrapping) and apply alignment.
pub fn break_lines(layout: &mut TextLayout, max_width: Option<f32>, align: Alignment) {
    layout.break_all_lines(max_width);
    layout.align(align, AlignmentOptions::default());
}

/// Break lines where each line has its own start and width: `slot(y)` gives `(x, width)` for a line
/// whose top is at `y` in the layout (text beside a float is narrower and starts further in).
/// `container` is the widest any line can be. Alignment applies within each line's own width.
pub fn break_lines_varying(layout: &mut TextLayout, container: f32, align: Alignment, mut slot: impl FnMut(f32) -> (f32, f32)) {
    let mut lines = layout.break_lines();
    lines.state_mut().set_layout_max_advance(container);
    // A line takes at least one cluster, so the loop ends; the cap is only a guard.
    for _ in 0..100_000 {
        let (x, width) = slot(lines.committed_y() as f32);
        lines.state_mut().set_line_x(x);
        lines.state_mut().set_line_max_advance(width.clamp(1.0, container.max(1.0)));
        if lines.break_next().is_none() {
            break;
        }
    }
    lines.finish();
    layout.align(align, AlignmentOptions::default());
}

/// CSS `white-space: normal`: collapse runs of whitespace to one space.
pub fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_text() {
        let mut engine = TextEngine::new();
        let style = TextStyle {
            font_family: "sans-serif".into(),
            font_size: 16.0,
            font_weight: 400.0,
            line_height: 1.2,
            color: [0, 0, 0, 255],
            deco: 0,
            spacer: false,
            shift: 0.0,
            italic: false,
            underline: false,
            line_through: false,
            letter_spacing: 0.0,
            align: Alignment::Start,
        };
        let mut layout = engine.shape("the quick brown fox jumps over the lazy dog", &style, 1.0);
        break_lines(&mut layout, None, Alignment::Start);
        let one_line = layout.height();
        break_lines(&mut layout, Some(80.0), Alignment::Start);
        assert!(layout.height() > one_line * 2.0, "text should wrap onto several lines");
    }

    #[test]
    fn symbol_gap_covers_dingbats_not_arrows() {
        assert!(is_symbol_gap('✓') && is_symbol_gap('⚠'));
        assert!(!is_symbol_gap('→') && !is_symbol_gap('a') && !is_symbol_gap('★'));
    }

    #[test]
    fn collapses() {
        assert_eq!(collapse_whitespace("  a \n\t b "), " a b ");
    }

    /// Some installed TrueType font to use as test data.
    fn some_system_font() -> Option<Vec<u8>> {
        [
            "/System/Library/Fonts/Supplemental/Georgia.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
        ]
        .iter()
        .find_map(|p| std::fs::read(p).ok())
    }

    fn width_in(engine: &mut TextEngine, family: &str) -> f32 {
        let style = TextStyle {
            font_family: family.to_string(),
            font_size: 40.0,
            font_weight: 400.0,
            line_height: 1.0,
            color: [0; 4],
            deco: 0,
            spacer: false,
            shift: 0.0,
            italic: false,
            underline: false,
            line_through: false,
            letter_spacing: 0.0,
            align: Alignment::Start,
        };
        let mut layout = engine.shape("Registered fonts", &style, 1.0);
        break_lines(&mut layout, None, Alignment::Start);
        layout.width()
    }

    #[test]
    fn a_registered_font_is_found_under_its_family_name() {
        let Some(bytes) = some_system_font() else { return };
        let mut engine = TextEngine::new();
        let _before = width_in(&mut engine, "NileWebFont");
        assert!(engine.register_font("NileWebFont", "a.ttf", bytes.clone(), None, None));
        assert!(engine.has_font("NileWebFont", "a.ttf", None, None));
        let after = width_in(&mut engine, "NileWebFont");
        // The fallback may already be this very font (Linux), so the widths can't be compared.
        assert!(after > 0.0);
        // Registering the same source again does nothing; a broken file isn't a font.
        assert!(engine.register_font("NileWebFont", "a.ttf", Vec::new(), None, None));
        assert!(!engine.register_font("Broken", "b.woff2", b"wOF2garbage".to_vec(), None, None));
        assert!(!engine.register_font("Broken", "c.ttf", b"not a font".to_vec(), None, None));
    }

    #[test]
    fn a_missing_family_falls_through_to_the_next_one() {
        let mut engine = TextEngine::new();
        let serif = width_in(&mut engine, "serif");
        let sans = width_in(&mut engine, "sans-serif");
        assert_ne!(serif, sans, "the generic families differ on this machine");
        assert_eq!(width_in(&mut engine, "'No Such Font', serif"), serif);
        assert_eq!(width_in(&mut engine, "\"No Such Font\", serif"), serif);
        assert_eq!(width_in(&mut engine, "NoSuchFont, serif"), serif);
    }
}
