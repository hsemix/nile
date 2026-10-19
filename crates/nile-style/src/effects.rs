//! `filter` / `backdrop-filter` function lists and `mix-blend-mode`, parsed from their text.

use crate::Color;

/// One `filter` function. Amounts are the CSS number (`50%` is 0.5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterFn {
    /// Blur radius in px (the standard deviation).
    Blur(f32),
    Brightness(f32),
    Contrast(f32),
    Grayscale(f32),
    HueRotate(f32),
    Invert(f32),
    Opacity(f32),
    Saturate(f32),
    Sepia(f32),
    DropShadow { dx: f32, dy: f32, blur: f32, color: Color },
}

fn length(t: &str) -> Option<f32> {
    let t = t.trim();
    if let Some(v) = t.strip_suffix("px") {
        return v.trim().parse().ok();
    }
    if let Some(v) = t.strip_suffix("rem").or_else(|| t.strip_suffix("em")) {
        return v.trim().parse::<f32>().ok().map(|n| n * 16.0);
    }
    t.parse::<f32>().ok().filter(|_| t == "0" || t.starts_with("0."))
}

fn amount(t: &str) -> Option<f32> {
    let t = t.trim();
    match t.strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().ok().map(|n| n / 100.0),
        None => t.parse().ok(),
    }
}

fn angle(t: &str) -> Option<f32> {
    let t = t.trim();
    let num = |s: &str| s.trim().parse::<f32>().ok();
    if let Some(v) = t.strip_suffix("deg") {
        num(v)
    } else if let Some(v) = t.strip_suffix("grad") {
        num(v).map(|n| n * 0.9)
    } else if let Some(v) = t.strip_suffix("rad") {
        num(v).map(f32::to_degrees)
    } else if let Some(v) = t.strip_suffix("turn") {
        num(v).map(|n| n * 360.0)
    } else {
        (t == "0").then_some(0.0)
    }
}

/// A `filter` value: `none` or a list of functions. An unusable function makes the whole value
/// invalid (empty), as in browsers.
pub fn parse_filter(text: &str, current: Color) -> Vec<FilterFn> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let Some(open) = rest.find('(') else { return Vec::new() };
        let name = rest[..open].trim().to_ascii_lowercase();
        let mut depth = 0;
        let mut close = None;
        for (i, c) in rest[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { return Vec::new() };
        let args = rest[open + 1..close].trim();
        rest = rest[close + 1..].trim_start();
        let one = |default: f32| if args.is_empty() { Some(default) } else { amount(args) };
        let f = match name.as_str() {
            "blur" => Some(FilterFn::Blur(if args.is_empty() { 0.0 } else { length(args).unwrap_or(0.0).max(0.0) })),
            "brightness" => one(1.0).map(|a| FilterFn::Brightness(a.max(0.0))),
            "contrast" => one(1.0).map(|a| FilterFn::Contrast(a.max(0.0))),
            "grayscale" => one(1.0).map(|a| FilterFn::Grayscale(a.clamp(0.0, 1.0))),
            "invert" => one(1.0).map(|a| FilterFn::Invert(a.clamp(0.0, 1.0))),
            "opacity" => one(1.0).map(|a| FilterFn::Opacity(a.clamp(0.0, 1.0))),
            "saturate" => one(1.0).map(|a| FilterFn::Saturate(a.max(0.0))),
            "sepia" => one(1.0).map(|a| FilterFn::Sepia(a.clamp(0.0, 1.0))),
            "hue-rotate" => (if args.is_empty() { Some(0.0) } else { angle(args) }).map(FilterFn::HueRotate),
            "drop-shadow" => {
                let mut lens = Vec::new();
                let mut color = current;
                // Split on spaces outside parentheses (`rgb(0, 0, 0)` holds spaces).
                let mut parts = Vec::new();
                let (mut start, mut d) = (0, 0);
                for (i, c) in args.char_indices() {
                    match c {
                        '(' => d += 1,
                        ')' => d -= 1,
                        ' ' if d == 0 => {
                            parts.push(&args[start..i]);
                            start = i + 1;
                        }
                        _ => {}
                    }
                }
                parts.push(&args[start..]);
                for part in parts.into_iter().filter(|p| !p.is_empty()) {
                    if let Some(l) = length(part) {
                        lens.push(l);
                    } else if part.eq_ignore_ascii_case("currentcolor") {
                        color = current;
                    } else if let Some(c) = crate::parse_color(part) {
                        color = c;
                    }
                }
                match lens.as_slice() {
                    [dx, dy] => Some(FilterFn::DropShadow { dx: *dx, dy: *dy, blur: 0.0, color }),
                    [dx, dy, b, ..] => Some(FilterFn::DropShadow { dx: *dx, dy: *dy, blur: b.max(0.0), color }),
                    _ => None,
                }
            }
            _ => None,
        };
        match f {
            Some(f) => out.push(f),
            None => return Vec::new(),
        }
    }
    out
}

/// `mix-blend-mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Blend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl Blend {
    pub fn parse(name: &str) -> Option<Blend> {
        Some(match name {
            "normal" => Blend::Normal,
            "multiply" => Blend::Multiply,
            "screen" => Blend::Screen,
            "overlay" => Blend::Overlay,
            "darken" => Blend::Darken,
            "lighten" => Blend::Lighten,
            "color-dodge" => Blend::ColorDodge,
            "color-burn" => Blend::ColorBurn,
            "hard-light" => Blend::HardLight,
            "soft-light" => Blend::SoftLight,
            "difference" => Blend::Difference,
            "exclusion" => Blend::Exclusion,
            "hue" => Blend::Hue,
            "saturation" => Blend::Saturation,
            "color" => Blend::Color,
            "luminosity" => Blend::Luminosity,
            _ => return None,
        })
    }
}

/// `object-fit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObjectFit {
    #[default]
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_lists_parse() {
        let red = Color::rgb(255, 0, 0);
        let f = parse_filter("blur(4px) brightness(120%) hue-rotate(0.25turn) drop-shadow(2px 3px 4px rgb(0, 0, 255))", red);
        assert_eq!(f.len(), 4);
        assert_eq!(f[0], FilterFn::Blur(4.0));
        assert_eq!(f[1], FilterFn::Brightness(1.2));
        assert_eq!(f[2], FilterFn::HueRotate(90.0));
        assert_eq!(f[3], FilterFn::DropShadow { dx: 2.0, dy: 3.0, blur: 4.0, color: Color::rgb(0, 0, 255) });
        assert!(parse_filter("none", red).is_empty());
        assert!(parse_filter("blur(2px) wobble(3)", red).is_empty(), "an unknown function drops the value");
        assert_eq!(parse_filter("drop-shadow(1px 1px)", red), vec![FilterFn::DropShadow { dx: 1.0, dy: 1.0, blur: 0.0, color: red }]);
    }
}
