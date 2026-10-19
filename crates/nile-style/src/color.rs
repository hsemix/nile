//! Modern CSS colour spaces: `oklab()`, `oklch()`, `lab()`, `lch()`, `hwb()`, `color()` and
//! `color-mix()`. Everything converts through linear sRGB; out-of-gamut colours are clamped.

// The matrices are published to more digits than an f32 holds.
#![allow(clippy::excessive_precision)]

use crate::Color;

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Linear sRGB, 0..1 per channel (not clamped).
type Lin = [f32; 3];

fn color_from_lin(l: Lin, alpha: f32) -> Color {
    let byte = |v: f32| (from_linear(v) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color { r: byte(l[0]), g: byte(l[1]), b: byte(l[2]), a: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8 }
}

fn lin_of(c: Color) -> Lin {
    [to_linear(f32::from(c.r) / 255.0), to_linear(f32::from(c.g) / 255.0), to_linear(f32::from(c.b) / 255.0)]
}

pub fn oklab_to_lin(l: f32, a: f32, b: f32) -> Lin {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_93 * s, -1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s, -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s]
}

pub fn lin_to_oklab(c: Lin) -> [f32; 3] {
    let l = 0.412_221_47 * c[0] + 0.536_332_55 * c[1] + 0.051_445_995 * c[2];
    let m = 0.211_903_5 * c[0] + 0.680_699_5 * c[1] + 0.107_396_96 * c[2];
    let s = 0.088_302_46 * c[0] + 0.281_718_85 * c[1] + 0.629_978_7 * c[2];
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}

fn lab_to_lin(l: f32, a: f32, b: f32) -> Lin {
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (a / 500.0 + fy, fy - b / 200.0);
    let (e, k) = (216.0 / 24389.0, 24389.0 / 27.0);
    let inv = |f: f32| if f * f * f > e { f * f * f } else { (116.0 * f - 16.0) / k };
    let y = if l > k * e { fy * fy * fy } else { l / k };
    let (x, z) = (0.964_22 * inv(fx), 0.825_21 * inv(fz));
    [3.133_856_1 * x - 1.616_866_7 * y - 0.490_614_6 * z, -0.978_768_4 * x + 1.916_141_5 * y + 0.033_454 * z, 0.071_945_3 * x - 0.228_991_4 * y + 1.405_242_7 * z]
}

fn lin_to_lab(c: Lin) -> [f32; 3] {
    let x = 0.436_074_7 * c[0] + 0.385_064_9 * c[1] + 0.143_080_4 * c[2];
    let y = 0.222_504_5 * c[0] + 0.716_878_6 * c[1] + 0.060_616_9 * c[2];
    let z = 0.013_932_2 * c[0] + 0.097_104_5 * c[1] + 0.714_173_3 * c[2];
    let (e, k) = (216.0 / 24389.0, 24389.0 / 27.0);
    let f = |t: f32| if t > e { t.cbrt() } else { (k * t + 16.0) / 116.0 };
    let (fx, fy, fz) = (f(x / 0.964_22), f(y), f(z / 0.825_21));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn polar(l: f32, a: f32, b: f32) -> [f32; 3] {
    [l, a.hypot(b), b.atan2(a).to_degrees().rem_euclid(360.0)]
}

fn rect(l: f32, c: f32, h: f32) -> [f32; 3] {
    let (s, co) = h.to_radians().sin_cos();
    [l, c * co, c * s]
}

pub fn oklab(l: f32, a: f32, b: f32, alpha: f32) -> Color {
    color_from_lin(oklab_to_lin(l, a, b), alpha)
}

pub fn oklch(l: f32, c: f32, h: f32, alpha: f32) -> Color {
    let [l, a, b] = rect(l, c, h);
    oklab(l, a, b, alpha)
}

pub fn lab(l: f32, a: f32, b: f32, alpha: f32) -> Color {
    color_from_lin(lab_to_lin(l, a, b), alpha)
}

pub fn lch(l: f32, c: f32, h: f32, alpha: f32) -> Color {
    let [l, a, b] = rect(l, c, h);
    lab(l, a, b, alpha)
}

pub fn hwb(h: f32, w: f32, b: f32, alpha: u8) -> Color {
    let (w, b) = (w.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
    let (w, b) = if w + b > 1.0 { (w / (w + b), b / (w + b)) } else { (w, b) };
    let base = crate::colors::hsl(h, 1.0, 0.5, 255);
    let mix = |c: u8| ((f32::from(c) / 255.0 * (1.0 - w - b) + w) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color { r: mix(base.r), g: mix(base.g), b: mix(base.b), a: alpha }
}

/// `color(<space> r g b)`: `srgb`, `srgb-linear`, `display-p3`, `a98-rgb`/`rec2020` (as sRGB-ish
/// approximations are not attempted: they are read as sRGB), `xyz`/`xyz-d65` and `xyz-d50`.
pub fn in_space(space: &str, v: [f32; 3], alpha: f32) -> Option<Color> {
    let lin = match space {
        "srgb" => [to_linear(v[0]), to_linear(v[1]), to_linear(v[2])],
        "srgb-linear" => v,
        "display-p3" => {
            let p = [to_linear(v[0]), to_linear(v[1]), to_linear(v[2])];
            [1.224_940_1 * p[0] - 0.224_940_4 * p[1], -0.042_056_9 * p[0] + 1.042_057_1 * p[1], -0.019_637_6 * p[0] - 0.078_636_1 * p[1] + 1.098_273_5 * p[2]]
        }
        "xyz" | "xyz-d65" => [3.240_97 * v[0] - 1.537_383_2 * v[1] - 0.498_610_76 * v[2], -0.969_243_6 * v[0] + 1.875_967_5 * v[1] + 0.041_555_06 * v[2], 0.055_630_08 * v[0] - 0.203_976_96 * v[1] + 1.056_971_5 * v[2]],
        "xyz-d50" => [3.133_856_1 * v[0] - 1.616_866_7 * v[1] - 0.490_614_6 * v[2], -0.978_768_4 * v[0] + 1.916_141_5 * v[1] + 0.033_454 * v[2], 0.071_945_3 * v[0] - 0.228_991_4 * v[1] + 1.405_242_7 * v[2]],
        _ => return None,
    };
    Some(color_from_lin(lin, alpha))
}

/// The interpolation spaces `color-mix()` knows.
#[derive(Clone, Copy, PartialEq)]
enum Space {
    Srgb,
    SrgbLinear,
    Oklab,
    Oklch,
    Lab,
    Lch,
    Hsl,
    Hwb,
}

/// Coordinates of `c` in `space` (hue in degrees where the space has one), plus which index is the hue.
fn coords(c: Color, space: Space) -> ([f32; 3], Option<usize>) {
    let rgb = [f32::from(c.r) / 255.0, f32::from(c.g) / 255.0, f32::from(c.b) / 255.0];
    match space {
        Space::Srgb => (rgb, None),
        Space::SrgbLinear => (lin_of(c), None),
        Space::Oklab => (lin_to_oklab(lin_of(c)), None),
        Space::Lab => (lin_to_lab(lin_of(c)), None),
        Space::Oklch => {
            let [l, a, b] = lin_to_oklab(lin_of(c));
            (polar(l, a, b), Some(2))
        }
        Space::Lch => {
            let [l, a, b] = lin_to_lab(lin_of(c));
            (polar(l, a, b), Some(2))
        }
        Space::Hsl | Space::Hwb => {
            let (max, min) = (rgb[0].max(rgb[1]).max(rgb[2]), rgb[0].min(rgb[1]).min(rgb[2]));
            let d = max - min;
            let h = if d == 0.0 {
                0.0
            } else if max == rgb[0] {
                60.0 * ((rgb[1] - rgb[2]) / d).rem_euclid(6.0)
            } else if max == rgb[1] {
                60.0 * ((rgb[2] - rgb[0]) / d + 2.0)
            } else {
                60.0 * ((rgb[0] - rgb[1]) / d + 4.0)
            };
            if space == Space::Hsl {
                let l = (max + min) / 2.0;
                let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()) };
                ([h, s, l], Some(0))
            } else {
                ([h, min, 1.0 - max], Some(0))
            }
        }
    }
}

fn from_coords(v: [f32; 3], space: Space, alpha: f32) -> Color {
    match space {
        Space::Srgb => {
            let byte = |x: f32| (x * 255.0).round().clamp(0.0, 255.0) as u8;
            Color { r: byte(v[0]), g: byte(v[1]), b: byte(v[2]), a: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8 }
        }
        Space::SrgbLinear => color_from_lin(v, alpha),
        Space::Oklab => oklab(v[0], v[1], v[2], alpha),
        Space::Oklch => oklch(v[0], v[1].max(0.0), v[2], alpha),
        Space::Lab => lab(v[0], v[1], v[2], alpha),
        Space::Lch => lch(v[0], v[1].max(0.0), v[2], alpha),
        Space::Hsl => crate::colors::hsl(v[0], v[1], v[2], (alpha.clamp(0.0, 1.0) * 255.0).round() as u8),
        Space::Hwb => hwb(v[0], v[1], v[2], (alpha.clamp(0.0, 1.0) * 255.0).round() as u8),
    }
}

/// `color-mix(in <space>, a [pa], b [pb])`: percentages in 0..100, `None` when omitted. Hues take
/// the shorter arc; colours mix premultiplied by alpha.
pub fn mix(space: &str, a: (Color, Option<f32>), b: (Color, Option<f32>)) -> Option<Color> {
    let space = match space {
        "srgb" => Space::Srgb,
        "srgb-linear" => Space::SrgbLinear,
        "oklab" => Space::Oklab,
        "oklch" => Space::Oklch,
        "lab" => Space::Lab,
        "lch" => Space::Lch,
        "hsl" => Space::Hsl,
        "hwb" => Space::Hwb,
        _ => return None,
    };
    let (pa, pb) = match (a.1, b.1) {
        (None, None) => (50.0, 50.0),
        (Some(p), None) => (p, 100.0 - p),
        (None, Some(p)) => (100.0 - p, p),
        (Some(p), Some(q)) => (p, q),
    };
    let total = pa + pb;
    if !(pa >= 0.0 && pb >= 0.0) || total <= 0.0 {
        return None;
    }
    let (wa, wb) = (pa / total, pb / total);
    // A total under 100% makes the result partly transparent.
    let alpha_scale = (total / 100.0).min(1.0);
    let (ca, hue_a) = coords(a.0, space);
    let (cb, _) = coords(b.0, space);
    let (aa, ab) = (f32::from(a.0.a) / 255.0, f32::from(b.0.a) / 255.0);
    let alpha = aa * wa + ab * wb;
    let mut out = [0.0; 3];
    for i in 0..3 {
        out[i] = if Some(i) == hue_a {
            let (mut h1, mut h2) = (ca[i], cb[i]);
            // A hueless (grey) colour takes the other's hue.
            let grey = |c: Color| c.r == c.g && c.g == c.b;
            if grey(a.0) {
                h1 = h2;
            } else if grey(b.0) {
                h2 = h1;
            }
            let d = (h2 - h1 + 540.0).rem_euclid(360.0) - 180.0;
            (h1 + d * wb).rem_euclid(360.0)
        } else if alpha > 0.0 {
            // Premultiplied: the colour is weighted by each side's alpha.
            (ca[i] * aa * wa + cb[i] * ab * wb) / alpha
        } else {
            ca[i] * wa + cb[i] * wb
        };
    }
    Some(from_coords(out, space, alpha * alpha_scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(c: Color, rgb: (u8, u8, u8)) -> bool {
        c.r.abs_diff(rgb.0) <= 2 && c.g.abs_diff(rgb.1) <= 2 && c.b.abs_diff(rgb.2) <= 2
    }

    #[test]
    fn spaces_round_trip_known_colours() {
        // oklab of white and the sRGB primaries.
        assert!(near(oklab(1.0, 0.0, 0.0, 1.0), (255, 255, 255)));
        assert!(near(oklab(0.627_955, 0.224_863, 0.125_846, 1.0), (255, 0, 0)));
        assert!(near(oklch(0.627_955, 0.257_683, 29.233_9, 1.0), (255, 0, 0)));
        assert!(near(lab(54.2905, 80.8049, 69.8910, 1.0), (255, 0, 0)));
        assert!(near(lch(100.0, 0.0, 0.0, 1.0), (255, 255, 255)));
        assert!(near(hwb(0.0, 0.0, 0.0, 255), (255, 0, 0)));
        assert!(near(hwb(0.0, 0.5, 0.5, 255), (128, 128, 128)));
        assert!(near(in_space("srgb", [0.0, 0.5, 1.0], 1.0).unwrap(), (0, 128, 255)));
    }

    #[test]
    fn mixing() {
        let (red, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        assert!(near(mix("srgb", (red, None), (blue, None)).unwrap(), (128, 0, 128)));
        assert!(near(mix("srgb", (red, Some(25.0)), (blue, None)).unwrap(), (64, 0, 191)));
        assert!(near(mix("srgb", (red, Some(100.0)), (blue, Some(0.0))).unwrap(), (255, 0, 0)));
        // In oklab the midpoint of red and blue is a lighter, greyer purple than sRGB's.
        let ok = mix("oklab", (red, None), (blue, None)).unwrap();
        assert!(ok.r.abs_diff(140) <= 4 && ok.g.abs_diff(83) <= 4 && ok.b.abs_diff(162) <= 4, "{ok:?}");
        assert!(mix("nonsense", (red, None), (blue, None)).is_none());
        let half = mix("srgb", (red, Some(30.0)), (blue, Some(30.0))).unwrap();
        assert!((i32::from(half.a) - 153).abs() <= 2, "percentages adding to 60% give 60% alpha: {half:?}");
    }
}
