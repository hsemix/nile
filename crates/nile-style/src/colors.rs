//! CSS named colors (CSS Color Level 4) and `hsl()` conversion.

use crate::Color;

const fn c(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

pub fn named(name: &str) -> Option<Color> {
    if name == "transparent" {
        return Some(Color::TRANSPARENT);
    }
    // System colours follow the OS theme; they are looked up when a style is computed.
    if let Some(c) = crate::system::system_color(name) {
        return Some(c);
    }
    NAMED.iter().find(|(n, _)| *n == name).map(|(_, col)| *col)
}

/// `h` in degrees, `s` and `l` in 0..=1.
pub fn hsl(h: f32, s: f32, l: f32, a: u8) -> Color {
    let h = h.rem_euclid(360.0) / 60.0;
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = l - chroma / 2.0;
    let byte = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color { r: byte(r), g: byte(g), b: byte(b), a }
}

static NAMED: &[(&str, Color)] = &[
    ("aliceblue", c(240, 248, 255)),
    ("antiquewhite", c(250, 235, 215)),
    ("aqua", c(0, 255, 255)),
    ("aquamarine", c(127, 255, 212)),
    ("azure", c(240, 255, 255)),
    ("beige", c(245, 245, 220)),
    ("bisque", c(255, 228, 196)),
    ("black", c(0, 0, 0)),
    ("blanchedalmond", c(255, 235, 205)),
    ("blue", c(0, 0, 255)),
    ("blueviolet", c(138, 43, 226)),
    ("brown", c(165, 42, 42)),
    ("burlywood", c(222, 184, 135)),
    ("cadetblue", c(95, 158, 160)),
    ("chartreuse", c(127, 255, 0)),
    ("chocolate", c(210, 105, 30)),
    ("coral", c(255, 127, 80)),
    ("cornflowerblue", c(100, 149, 237)),
    ("cornsilk", c(255, 248, 220)),
    ("crimson", c(220, 20, 60)),
    ("cyan", c(0, 255, 255)),
    ("darkblue", c(0, 0, 139)),
    ("darkcyan", c(0, 139, 139)),
    ("darkgoldenrod", c(184, 134, 11)),
    ("darkgray", c(169, 169, 169)),
    ("darkgreen", c(0, 100, 0)),
    ("darkgrey", c(169, 169, 169)),
    ("darkkhaki", c(189, 183, 107)),
    ("darkmagenta", c(139, 0, 139)),
    ("darkolivegreen", c(85, 107, 47)),
    ("darkorange", c(255, 140, 0)),
    ("darkorchid", c(153, 50, 204)),
    ("darkred", c(139, 0, 0)),
    ("darksalmon", c(233, 150, 122)),
    ("darkseagreen", c(143, 188, 143)),
    ("darkslateblue", c(72, 61, 139)),
    ("darkslategray", c(47, 79, 79)),
    ("darkslategrey", c(47, 79, 79)),
    ("darkturquoise", c(0, 206, 209)),
    ("darkviolet", c(148, 0, 211)),
    ("deeppink", c(255, 20, 147)),
    ("deepskyblue", c(0, 191, 255)),
    ("dimgray", c(105, 105, 105)),
    ("dimgrey", c(105, 105, 105)),
    ("dodgerblue", c(30, 144, 255)),
    ("firebrick", c(178, 34, 34)),
    ("floralwhite", c(255, 250, 240)),
    ("forestgreen", c(34, 139, 34)),
    ("fuchsia", c(255, 0, 255)),
    ("gainsboro", c(220, 220, 220)),
    ("ghostwhite", c(248, 248, 255)),
    ("gold", c(255, 215, 0)),
    ("goldenrod", c(218, 165, 32)),
    ("gray", c(128, 128, 128)),
    ("green", c(0, 128, 0)),
    ("greenyellow", c(173, 255, 47)),
    ("grey", c(128, 128, 128)),
    ("honeydew", c(240, 255, 240)),
    ("hotpink", c(255, 105, 180)),
    ("indianred", c(205, 92, 92)),
    ("indigo", c(75, 0, 130)),
    ("ivory", c(255, 255, 240)),
    ("khaki", c(240, 230, 140)),
    ("lavender", c(230, 230, 250)),
    ("lavenderblush", c(255, 240, 245)),
    ("lawngreen", c(124, 252, 0)),
    ("lemonchiffon", c(255, 250, 205)),
    ("lightblue", c(173, 216, 230)),
    ("lightcoral", c(240, 128, 128)),
    ("lightcyan", c(224, 255, 255)),
    ("lightgoldenrodyellow", c(250, 250, 210)),
    ("lightgray", c(211, 211, 211)),
    ("lightgreen", c(144, 238, 144)),
    ("lightgrey", c(211, 211, 211)),
    ("lightpink", c(255, 182, 193)),
    ("lightsalmon", c(255, 160, 122)),
    ("lightseagreen", c(32, 178, 170)),
    ("lightskyblue", c(135, 206, 250)),
    ("lightslategray", c(119, 136, 153)),
    ("lightslategrey", c(119, 136, 153)),
    ("lightsteelblue", c(176, 196, 222)),
    ("lightyellow", c(255, 255, 224)),
    ("lime", c(0, 255, 0)),
    ("limegreen", c(50, 205, 50)),
    ("linen", c(250, 240, 230)),
    ("magenta", c(255, 0, 255)),
    ("maroon", c(128, 0, 0)),
    ("mediumaquamarine", c(102, 205, 170)),
    ("mediumblue", c(0, 0, 205)),
    ("mediumorchid", c(186, 85, 211)),
    ("mediumpurple", c(147, 112, 219)),
    ("mediumseagreen", c(60, 179, 113)),
    ("mediumslateblue", c(123, 104, 238)),
    ("mediumspringgreen", c(0, 250, 154)),
    ("mediumturquoise", c(72, 209, 204)),
    ("mediumvioletred", c(199, 21, 133)),
    ("midnightblue", c(25, 25, 112)),
    ("mintcream", c(245, 255, 250)),
    ("mistyrose", c(255, 228, 225)),
    ("moccasin", c(255, 228, 181)),
    ("navajowhite", c(255, 222, 173)),
    ("navy", c(0, 0, 128)),
    ("oldlace", c(253, 245, 230)),
    ("olive", c(128, 128, 0)),
    ("olivedrab", c(107, 142, 35)),
    ("orange", c(255, 165, 0)),
    ("orangered", c(255, 69, 0)),
    ("orchid", c(218, 112, 214)),
    ("palegoldenrod", c(238, 232, 170)),
    ("palegreen", c(152, 251, 152)),
    ("paleturquoise", c(175, 238, 238)),
    ("palevioletred", c(219, 112, 147)),
    ("papayawhip", c(255, 239, 213)),
    ("peachpuff", c(255, 218, 185)),
    ("peru", c(205, 133, 63)),
    ("pink", c(255, 192, 203)),
    ("plum", c(221, 160, 221)),
    ("powderblue", c(176, 224, 230)),
    ("purple", c(128, 0, 128)),
    ("rebeccapurple", c(102, 51, 153)),
    ("red", c(255, 0, 0)),
    ("rosybrown", c(188, 143, 143)),
    ("royalblue", c(65, 105, 225)),
    ("saddlebrown", c(139, 69, 19)),
    ("salmon", c(250, 128, 114)),
    ("sandybrown", c(244, 164, 96)),
    ("seagreen", c(46, 139, 87)),
    ("seashell", c(255, 245, 238)),
    ("sienna", c(160, 82, 45)),
    ("silver", c(192, 192, 192)),
    ("skyblue", c(135, 206, 235)),
    ("slateblue", c(106, 90, 205)),
    ("slategray", c(112, 128, 144)),
    ("slategrey", c(112, 128, 144)),
    ("snow", c(255, 250, 250)),
    ("springgreen", c(0, 255, 127)),
    ("steelblue", c(70, 130, 180)),
    ("tan", c(210, 180, 140)),
    ("teal", c(0, 128, 128)),
    ("thistle", c(216, 191, 216)),
    ("tomato", c(255, 99, 71)),
    ("turquoise", c(64, 224, 208)),
    ("violet", c(238, 130, 238)),
    ("wheat", c(245, 222, 179)),
    ("white", c(255, 255, 255)),
    ("whitesmoke", c(245, 245, 245)),
    ("yellow", c(255, 255, 0)),
    ("yellowgreen", c(154, 205, 50)),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_hsl() {
        assert_eq!(named("seagreen"), Some(Color::rgb(46, 139, 87)));
        assert_eq!(named("nope"), None);
        assert_eq!(hsl(0.0, 1.0, 0.5, 255), Color::rgb(255, 0, 0));
        assert_eq!(hsl(120.0, 1.0, 0.25, 255), Color::rgb(0, 128, 0));
        assert_eq!(hsl(240.0, 1.0, 0.5, 255), Color::rgb(0, 0, 255));
    }
}
