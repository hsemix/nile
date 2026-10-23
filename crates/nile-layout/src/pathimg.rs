//! Sciter's `url(path:M0 0 L10 10 ...)` images: an SVG path drawn with the element's `fill`,
//! `stroke` and `stroke-width` (else its text colour). The path becomes a one-path SVG document
//! sized by the path's bounding box, so `background-size`/`foreground-size` scale it like any picture.

use crate::images::{ImageData, SvgInfo};
use nile_style::{Color, ComputedStyle};
use std::hash::{Hash, Hasher};

/// The path data of a `path:` URL.
pub(crate) fn path_of(url: &str) -> Option<&str> {
    url.strip_prefix("path:").map(str::trim).filter(|d| !d.is_empty())
}

/// Corners of the box the path's points span (control points and arc ends included).
fn bounds(d: &str) -> Option<(f32, f32, f32, f32)> {
    let mut tokens: Vec<(char, Vec<f32>)> = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            tokens.push((c, Vec::new()));
            i += 1;
        } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            let mut dot = c == '.';
            while i < b.len() {
                let n = b[i] as char;
                if n.is_ascii_digit() || (n == '.' && !dot) {
                    dot |= n == '.';
                    i += 1;
                } else if (n == 'e' || n == 'E') && i + 1 < b.len() && (b[i + 1] == b'-' || b[i + 1] == b'+' || b[i + 1].is_ascii_digit()) {
                    i += 2;
                } else {
                    break;
                }
            }
            if let (Ok(v), Some(last)) = (d[start..i].parse::<f32>(), tokens.last_mut()) {
                last.1.push(v);
            }
        } else {
            i += 1;
        }
    }
    let (mut x, mut y, mut sx, mut sy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut add = |x: f32, y: f32| {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    };
    for (cmd, nums) in tokens {
        let rel = cmd.is_ascii_lowercase();
        let (step, kind) = match cmd.to_ascii_uppercase() {
            'M' | 'L' | 'T' => (2, 'P'),
            'H' => (1, 'H'),
            'V' => (1, 'V'),
            'C' => (6, 'P'),
            'S' | 'Q' => (4, 'P'),
            'A' => (7, 'A'),
            'Z' => {
                x = sx;
                y = sy;
                continue;
            }
            _ => continue,
        };
        for (n, chunk) in nums.chunks_exact(step).enumerate() {
            let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
            match kind {
                'H' => x = chunk[0] + ox,
                'V' => y = chunk[0] + oy,
                'A' => {
                    x = chunk[5] + ox;
                    y = chunk[6] + oy;
                }
                _ => {
                    for p in chunk.as_chunks::<2>().0 {
                        add(p[0] + ox, p[1] + oy);
                    }
                    x = chunk[step - 2] + ox;
                    y = chunk[step - 1] + oy;
                }
            }
            add(x, y);
            if cmd.eq_ignore_ascii_case(&'M') && n == 0 {
                sx = x;
                sy = y;
            }
        }
    }
    (x0 <= x1 && y0 <= y1).then_some((x0, y0, x1, y1))
}

fn paint(c: Color) -> (String, f32) {
    (format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b), c.a as f32 / 255.0)
}

/// The picture for `path:` `url` as `style` draws it, or `None` for an empty or invalid path.
pub(crate) fn image(url: &str, style: &ComputedStyle) -> Option<ImageData> {
    let d = path_of(url)?;
    let (x0, y0, x1, y1) = bounds(d)?;
    let stroke = style.stroke.filter(|c| c.a > 0);
    let pad = if stroke.is_some() { style.stroke_width / 2.0 } else { 0.0 };
    let (w, h) = ((x1 - x0).max(0.01) + 2.0 * pad, (y1 - y0).max(0.01) + 2.0 * pad);
    let fill = match (style.fill, stroke) {
        (Some(c), _) => Some(c),
        (None, Some(_)) => None,
        (None, None) => Some(style.color),
    };
    let mut attrs = String::new();
    match fill {
        Some(c) => {
            let (hex, a) = paint(c);
            attrs.push_str(&format!(" fill=\"{hex}\" fill-opacity=\"{a}\""));
        }
        None => attrs.push_str(" fill=\"none\""),
    }
    if let Some(c) = stroke {
        let (hex, a) = paint(c);
        attrs.push_str(&format!(" stroke=\"{hex}\" stroke-opacity=\"{a}\" stroke-width=\"{}\"", style.stroke_width));
    }
    let d = d.replace('"', "");
    let source = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"{} {} {w} {h}\"><path d=\"{d}\"{attrs}/></svg>",
        x0 - pad,
        y0 - pad
    );
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    Some(ImageData { id: hasher.finish() | (1 << 63), bytes: source.into_bytes(), width: w.ceil().max(1.0) as u32, height: h.ceil().max(1.0) as u32, canvas: None, svg: Some(SvgInfo { css: None, stretch: false }) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bounding_box_follows_absolute_and_relative_commands() {
        assert_eq!(bounds("M0 0 H10 V10 H0 Z"), Some((0.0, 0.0, 10.0, 10.0)));
        assert_eq!(bounds("M0 0 M0 5 H10 M10 10"), Some((0.0, 0.0, 10.0, 10.0)));
        assert_eq!(bounds("M0 2 h8 v8 h-8 Z M2 2 v-2 h8 v8 h-2"), Some((0.0, 0.0, 10.0, 10.0)));
        assert_eq!(bounds("M4,4v1v23h24V4H4z"), Some((4.0, 4.0, 28.0, 28.0)));
        assert_eq!(bounds("M-1.5-2.5L3 4"), Some((-1.5, -2.5, 3.0, 4.0)));
        assert_eq!(bounds("Z"), None);
    }
}
