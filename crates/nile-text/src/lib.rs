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
    /// `font-variation-settings` and `font-feature-settings` in CSS syntax (`"wght" 650, "wdth" 80`); empty for none.
    pub font_variations: String,
    pub font_features: String,
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
            // Shaped as the padding spacers are (see `spacer_variations`).
            spacer: true,
            shift: 0.0,
            italic: false,
            underline: false,
            line_through: false,
            letter_spacing: 0.0,
            align: Alignment::Start,
            font_variations: String::new(),
            font_features: String::new(),
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
        b.push_default(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(system_families(&base.font_family)))));
        b.push_default(StyleProperty::FontSize(base.font_size));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(base.font_weight)));
        b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(base.line_height)));
        b.push_default(StyleProperty::Brush(brush(base)));
        b.push_default(StyleProperty::FontStyle(font_style(base)));
        b.push_default(StyleProperty::Underline(base.underline));
        b.push_default(StyleProperty::Strikethrough(base.line_through));
        b.push_default(StyleProperty::LetterSpacing(base.letter_spacing));
        let base_variations = if base.spacer {
            spacer_variations(&base.font_variations, &system_families(&base.font_family))
        } else {
            with_optical_size(&base.font_variations, base.font_size, &system_families(&base.font_family))
        };
        b.push_default(StyleProperty::FontVariations(parley::style::FontVariations::Source(Cow::Owned(base_variations))));
        if !base.font_features.is_empty() {
            b.push_default(StyleProperty::FontFeatures(parley::style::FontFeatures::Source(Cow::Owned(base.font_features.clone()))));
        }
        for (range, s) in spans {
            if range.is_empty() || *s == *base {
                continue;
            }
            b.push(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(system_families(&s.font_family)))), range.clone());
            b.push(StyleProperty::FontSize(s.font_size), range.clone());
            b.push(StyleProperty::FontWeight(FontWeight::new(s.font_weight)), range.clone());
            b.push(StyleProperty::LineHeight(LineHeight::FontSizeRelative(s.line_height)), range.clone());
            b.push(StyleProperty::Brush(brush(s)), range.clone());
            b.push(StyleProperty::FontStyle(font_style(s)), range.clone());
            b.push(StyleProperty::Underline(s.underline), range.clone());
            b.push(StyleProperty::Strikethrough(s.line_through), range.clone());
            b.push(StyleProperty::LetterSpacing(s.letter_spacing), range.clone());
            // Padding spacers are sized from the font's NBSP advance (`nbsp_ratio`), shaped at one fixed
            // optical size so the two agree whatever size the spacer is.
            if s.spacer {
                let v = spacer_variations(&s.font_variations, &system_families(&s.font_family));
                if v != s.font_variations {
                    b.push(StyleProperty::FontVariations(parley::style::FontVariations::Source(Cow::Owned(v))), range.clone());
                }
            } else if s.font_variations != base.font_variations || s.font_size != base.font_size {
                b.push(StyleProperty::FontVariations(parley::style::FontVariations::Source(Cow::Owned(with_optical_size(&s.font_variations, s.font_size, &system_families(&s.font_family))))), range.clone());
            }
            if s.font_features != base.font_features {
                b.push(StyleProperty::FontFeatures(parley::style::FontFeatures::Source(Cow::Owned(s.font_features.clone()))), range.clone());
            }
        }
        // Symbol characters the system fallback misses get named fallback fonts.
        for (i, c) in text.char_indices().filter(|(_, c)| is_symbol_gap(*c)) {
            let family = spans
                .iter()
                .rev()
                .find(|(r, _)| r.contains(&i))
                .map_or(&base.font_family, |(_, s)| &s.font_family);
            b.push(
                StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(with_symbol_fallbacks(&system_families(family))))),
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

/// The arrows from `↰` on (`↶ ↷ ⇄ ...`), Miscellaneous Technical (`⌕ ⌘ ⏎ ...`), the marked and
/// fill-pattern squares of Geometric Shapes (`▣ ▤ ▥ ▦ ▧ ▨ ▩ ▯ ...`), and the end of
/// Miscellaneous Symbols (`⚠ ⚡ ⛔ ...`) through Dingbats (`✓ ✔ ✗ ✂ ...`). The rest of Geometric
/// Shapes (`○ ● ▲ ▼`) is found by the system fallback.
///
/// Windows has no working system fallback for the symbol blocks at all (the plain stack shows boxes
/// where Segoe UI Symbol has the glyph), so there the whole range from Arrows through Miscellaneous
/// Symbols and Arrows (U+2190-2BFF) goes to the named list, Segoe UI Symbol being the system's own.
fn is_symbol_gap(c: char) -> bool {
    let u = c as u32;
    if cfg!(windows) && matches!(u, 0x2190..=0x2BFF) {
        return true;
    }
    matches!(u, 0x21B0..=0x21FF | 0x2300..=0x23FF | 0x25A2..=0x25AF | 0x26A0..=0x27BF)
}

/// `font-optical-sizing: auto` for the system UI fonts: San Francisco (macOS) and Segoe UI Variable
/// have an `opsz` axis, which is set to the text size, unless the author's `font-variation-settings` name the axis themselves.
/// Without it such fonts come out at their display size: tight and spindly for body text.
fn with_optical_size(variations: &str, size: f32, family: &str) -> String {
    // Only for the system UI fonts (San Francisco, Segoe UI Variable), which have the axis and look
    // wrong without it. Other fonts shape exactly as before.
    let family = family.to_ascii_lowercase();
    let has_axis = family.contains("system-ui") || family.contains("segoe ui variable") || family.contains("sf pro");
    if !has_axis || variations.to_ascii_lowercase().contains("opsz") {
        return variations.to_string();
    }
    let axis = format!("\"opsz\" {}", size.clamp(1.0, 1000.0));
    if variations.trim().is_empty() { axis } else { format!("{variations}, {axis}") }
}

/// The optical size padding spacers are shaped at (and their NBSP measured at), whatever their size.
fn spacer_variations(variations: &str, family: &str) -> String {
    with_optical_size(variations, 17.0, family)
}

/// The platform's own fonts behind the CSS system font names: `system-ui` (and the aliases pages
/// use for it, `-apple-system`, `BlinkMacSystemFont`, `ui-sans-serif`), `ui-monospace`, `ui-serif`
/// and `ui-rounded`. Everything else in a family list is left as it is.
pub fn system_families(list: &str) -> String {
    let (ui, mono, serif, rounded) = if cfg!(target_os = "macos") {
        ("system-ui", "SF Mono, Menlo, Monaco, monospace", "New York, Georgia, Times, serif", "SF Pro Rounded, system-ui")
    } else if cfg!(windows) {
        ("Segoe UI Variable Text, Segoe UI, sans-serif", "Cascadia Mono, Consolas, Courier New, monospace", "Cambria, Georgia, Times New Roman, serif", "Segoe UI, sans-serif")
    } else {
        ("system-ui, Cantarell, Noto Sans, DejaVu Sans, sans-serif", "DejaVu Sans Mono, Noto Sans Mono, Liberation Mono, monospace", "Noto Serif, DejaVu Serif, serif", "system-ui, Cantarell, Noto Sans, sans-serif")
    };
    if !list.split(',').any(|f| is_system_family(f.trim().trim_matches(['"', '\''])).is_some()) {
        return list.to_string();
    }
    list.split(',')
        .map(|f| match is_system_family(f.trim().trim_matches(['"', '\''])) {
            Some("ui") => ui.to_string(),
            Some("mono") => mono.to_string(),
            Some("serif") => serif.to_string(),
            Some("rounded") => rounded.to_string(),
            _ => f.trim().to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_system_family(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "system-ui" | "-apple-system" | "blinkmacsystemfont" | "ui-sans-serif" | "-webkit-system-font" => Some("ui"),
        "ui-monospace" => Some("mono"),
        "ui-serif" => Some("serif"),
        "ui-rounded" => Some("rounded"),
        _ => None,
    }
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
            font_variations: String::new(),
            font_features: String::new(),
        };
        let mut layout = engine.shape("the quick brown fox jumps over the lazy dog", &style, 1.0);
        break_lines(&mut layout, None, Alignment::Start);
        let one_line = layout.height();
        break_lines(&mut layout, Some(80.0), Alignment::Start);
        assert!(layout.height() > one_line * 2.0, "text should wrap onto several lines");
    }

    #[test]
    fn optical_size_follows_the_font_size_unless_the_author_sets_it() {
        assert_eq!(with_optical_size("", 15.0, "system-ui"), "\"opsz\" 15");
        assert_eq!(with_optical_size("\"wght\" 600", 13.5, "Segoe UI Variable Text, Segoe UI"), "\"wght\" 600, \"opsz\" 13.5");
        assert_eq!(with_optical_size("\"OPSZ\" 20", 13.0, "system-ui"), "\"OPSZ\" 20");
        // Fonts that don't need it are shaped as before.
        assert_eq!(with_optical_size("", 15.0, "Helvetica, sans-serif"), "");
    }

    #[test]
    fn system_font_names_become_the_platforms_fonts() {
        // Other families are untouched, and a list without a system name comes back as it was.
        assert_eq!(system_families("Arial, sans-serif"), "Arial, sans-serif");
        let ui = system_families("-apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif");
        assert!(ui.ends_with("\"Segoe UI\", sans-serif") || ui.ends_with("Segoe UI\", sans-serif"), "{ui}");
        assert!(!ui.to_ascii_lowercase().contains("blinkmac") && !ui.contains("-apple-system"), "{ui}");
        assert!(system_families("ui-monospace").contains("monospace"));
        assert!(system_families("ui-serif").contains("serif"));
        assert!(system_families("ui-rounded").to_ascii_lowercase().contains("system-ui") || cfg!(windows));
    }

    #[test]
    fn symbol_gap_covers_dingbats_not_arrows() {
        assert!(is_symbol_gap('✓') && is_symbol_gap('⚠') && is_symbol_gap('▤') && is_symbol_gap('▯') && is_symbol_gap('↶') && is_symbol_gap('⌕'));
        // Elsewhere the rest of Geometric Shapes and the arrows keep the system's choice.
        assert_eq!((is_symbol_gap('○'), is_symbol_gap('→'), is_symbol_gap('▲')), (cfg!(windows), cfg!(windows), cfg!(windows)));
        assert!(!is_symbol_gap('a'));
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
            font_variations: String::new(),
            font_features: String::new(),
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
