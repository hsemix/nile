//! `font`, `fillText`, `strokeText` and `measureText`.

use std::cell::RefCell;
use std::collections::HashMap;

use skia_safe::{font_style::{Slant, Weight, Width}, Font, FontMgr, FontStyle, Paint, Typeface};

use super::{Align, Baseline, Canvas2D, State};

/// A parsed `font` value: `italic bold 16px/20px "Helvetica Neue", Arial, sans-serif`.
#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    pub size: f32,
    pub weight: i32,
    pub italic: bool,
    pub families: Vec<String>,
}

impl Default for FontSpec {
    fn default() -> Self {
        FontSpec { size: 10.0, weight: 400, italic: false, families: vec!["sans-serif".into()] }
    }
}

fn size_px(token: &str) -> Option<f32> {
    // `16px/20px`: the line height is ignored.
    let t = token.split('/').next()?;
    let (num, unit) = t.split_at(t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len()));
    let n: f32 = num.parse().ok()?;
    Some(match unit {
        "px" => n,
        "pt" => n * 4.0 / 3.0,
        "em" | "rem" => n * 16.0,
        "pc" => n * 16.0,
        _ => return None,
    })
}

impl FontSpec {
    pub fn parse(text: &str) -> Option<FontSpec> {
        let text = text.trim();
        let mut spec = FontSpec { families: Vec::new(), ..FontSpec::default() };
        let mut rest = text;
        loop {
            let (token, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if token.is_empty() {
                return None;
            }
            match token {
                "normal" | "small-caps" => {}
                "italic" | "oblique" => spec.italic = true,
                "bold" => spec.weight = 700,
                "bolder" => spec.weight = 700,
                "lighter" => spec.weight = 300,
                t if t.parse::<f32>().is_ok_and(|w| (1.0..=1000.0).contains(&w)) => spec.weight = t.parse::<f32>().ok()? as i32,
                t => {
                    spec.size = match t {
                        "xx-small" => 9.0,
                        "x-small" => 10.0,
                        "small" => 13.0,
                        "medium" => 16.0,
                        "large" => 18.0,
                        "x-large" => 24.0,
                        "xx-large" => 32.0,
                        _ => size_px(t)?,
                    };
                    rest = after.trim();
                    break;
                }
            }
            rest = after.trim();
        }
        spec.families = rest.split(',').map(|f| f.trim().trim_matches(['"', '\'']).to_string()).filter(|f| !f.is_empty()).collect();
        (!spec.families.is_empty()).then_some(spec)
    }

    /// How the `font` property reads back.
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        if self.italic {
            out.push_str("italic ");
        }
        if self.weight != 400 {
            out.push_str(&format!("{} ", if self.weight == 700 { "bold".to_string() } else { self.weight.to_string() }));
        }
        let family = self.families.iter().map(|f| if f.contains(' ') { format!("\"{f}\"") } else { f.clone() }).collect::<Vec<_>>().join(", ");
        format!("{out}{}px {family}", if self.size.fract() == 0.0 { format!("{}", self.size as i32) } else { self.size.to_string() })
    }

    pub fn font(&self) -> Font {
        match typeface(self) {
            Some(t) => Font::from_typeface(t, self.size),
            // A machine with no fonts at all: Skia's empty default draws nothing but doesn't fail.
            None => {
                let mut f = Font::default();
                f.set_size(self.size);
                f
            }
        }
    }
}

/// Families, weight, italic.
type FaceKey = (Vec<String>, i32, bool);

thread_local! {
    static TYPEFACES: RefCell<HashMap<FaceKey, Typeface>> = RefCell::default();
}

fn candidates(family: &str) -> Vec<&str> {
    match family {
        "sans-serif" => vec!["Helvetica Neue", "Helvetica", "Segoe UI", "Arial", "DejaVu Sans", "Liberation Sans"],
        "system-ui" | "-apple-system" | "BlinkMacSystemFont" | "ui-sans-serif" => vec![".AppleSystemUIFont", "Segoe UI", "Cantarell", "Noto Sans", "DejaVu Sans", "Arial"],
        "serif" | "ui-serif" => vec!["Times New Roman", "Times", "Georgia", "DejaVu Serif", "Liberation Serif"],
        "monospace" | "ui-monospace" => vec!["Menlo", "Monaco", "Consolas", "Courier New", "DejaVu Sans Mono", "Liberation Mono"],
        "cursive" => vec!["Snell Roundhand", "Comic Sans MS", "Apple Chancery"],
        "fantasy" => vec!["Papyrus", "Impact"],
        other => vec![other],
    }
}

fn typeface(spec: &FontSpec) -> Option<Typeface> {
    let key = (spec.families.clone(), spec.weight, spec.italic);
    if let Some(t) = TYPEFACES.with(|c| c.borrow().get(&key).cloned()) {
        return Some(t);
    }
    let style = FontStyle::new(Weight::from(spec.weight), Width::NORMAL, if spec.italic { Slant::Italic } else { Slant::Upright });
    let mgr = FontMgr::default();
    let found = spec
        .families
        .iter()
        .flat_map(|f| candidates(f))
        .find_map(|name| mgr.match_family_style(name, style))
        .or_else(|| mgr.legacy_make_typeface(None, style))
        .or_else(|| mgr.match_family_style("", style))?;
    TYPEFACES.with(|c| c.borrow_mut().insert(key, found.clone()));
    Some(found)
}

/// Where the text's origin sits against its box, from the alignment and the baseline.
fn offsets(state: &State, font: &Font, width: f32) -> (f32, f32) {
    let (_, m) = font.metrics();
    let (asc, desc) = (-m.ascent, m.descent);
    let dx = match state.align {
        Align::Start | Align::Left => 0.0,
        Align::End | Align::Right => -width,
        Align::Center => -width / 2.0,
    };
    let dy = match state.baseline {
        Baseline::Alphabetic => 0.0,
        Baseline::Top => asc,
        Baseline::Hanging => asc * 0.8,
        Baseline::Middle => (asc - desc) / 2.0,
        Baseline::Ideographic | Baseline::Bottom => -desc,
    };
    (dx, dy)
}

pub fn draw(cv: &mut Canvas2D, text: &str, x: f32, y: f32, max: Option<f32>, paint: &Paint) {
    let font = cv.state.font.font();
    let (width, _) = font.measure_str(text, None);
    let (dx, dy) = offsets(&cv.state, &font, width);
    let squeeze = max.filter(|m| width > *m && width > 0.0).map(|m| m / width);
    let canvas = cv.canvas();
    canvas.save();
    canvas.translate((x, y));
    if let Some(s) = squeeze {
        canvas.scale((s, 1.0));
    }
    canvas.draw_str(text, (dx, dy), &font, paint);
    canvas.restore();
}

/// A `TextMetrics` as JSON.
pub fn measure(state: &State, text: &str) -> String {
    let font = state.font.font();
    let (width, bounds) = font.measure_str(text, None);
    let (dx, dy) = offsets(state, &font, width);
    let (_, m) = font.metrics();
    format!(
        "{{\"width\":{width},\"actualBoundingBoxLeft\":{},\"actualBoundingBoxRight\":{},\"actualBoundingBoxAscent\":{},\"actualBoundingBoxDescent\":{},\"fontBoundingBoxAscent\":{},\"fontBoundingBoxDescent\":{}}}",
        -(bounds.left + dx),
        bounds.right + dx,
        -(bounds.top + dy),
        bounds.bottom + dy,
        -m.ascent - dy,
        m.descent + dy
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_values_parse() {
        let f = FontSpec::parse("italic bold 16px/20px \"Helvetica Neue\", Arial, sans-serif").unwrap();
        assert_eq!((f.size, f.weight, f.italic), (16.0, 700, true));
        assert_eq!(f.families, ["Helvetica Neue", "Arial", "sans-serif"]);
        assert_eq!(FontSpec::parse("12pt serif").unwrap().size, 16.0);
        assert_eq!(FontSpec::parse("600 2em monospace").unwrap().weight, 600);
        assert!(FontSpec::parse("nonsense").is_none());
        assert!(FontSpec::parse("16px").is_none());
        assert_eq!(FontSpec::parse("bold 14px Arial").unwrap().canonical(), "bold 14px Arial");
    }
}
