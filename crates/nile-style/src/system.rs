//! The system's colour theme: light or dark, and the accent colour. The embedder sets it
//! ([`set_system_theme`]; the `nile` shell reads it from the OS), styles read it: `prefers-color-scheme`,
//! the system colour keywords (`Canvas`, `ButtonFace`, `AccentColor`, ...), the user-agent sheet's
//! accent-coloured parts and the renderer's own (progress bars, menus, selections).
//!
//! `NILE_COLOR_SCHEME=dark|light` and `NILE_ACCENT=#rrggbb` override whatever the embedder says (for
//! screenshots and tests). The setting is per thread, like the parsed-sheet caches.

use std::cell::Cell;

use crate::Color;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SystemTheme {
    pub dark: bool,
    pub accent: Color,
}

impl Default for SystemTheme {
    fn default() -> Self {
        SystemTheme { dark: false, accent: default_accent() }
    }
}

/// The platform's stock accent colour, for when the embedder has none to report.
pub fn default_accent() -> Color {
    if cfg!(windows) {
        Color::rgb(0, 120, 212)
    } else if cfg!(target_os = "macos") {
        Color::rgb(0, 122, 255)
    } else {
        Color::rgb(53, 132, 228)
    }
}

/// Black or white, whichever reads better on `c`.
pub fn text_on(c: Color) -> Color {
    let lum = 0.2126 * f32::from(c.r) + 0.7152 * f32::from(c.g) + 0.0722 * f32::from(c.b);
    if lum > 150.0 { Color::rgb(0, 0, 0) } else { Color::rgb(255, 255, 255) }
}

thread_local! {
    static THEME: Cell<Option<SystemTheme>> = const { Cell::new(None) };
    /// The scheme the element being styled uses, when it differs from the system's (see `with_scheme`).
    static SCHEME: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Whether colours are being resolved for a dark scheme (`light-dark()` reads this).
pub(crate) fn current_dark() -> bool {
    SCHEME.with(Cell::get).unwrap_or_else(|| system_theme().dark)
}

/// Run `f` with system colours resolved in the dark (`true`) or light scheme.
pub(crate) fn with_scheme<R>(dark: bool, f: impl FnOnce() -> R) -> R {
    let before = SCHEME.with(|c| c.replace(Some(dark)));
    let out = f();
    SCHEME.with(|c| c.set(before));
    out
}

/// Whether an element whose `color-scheme` is `scheme` is dark: `dark` always is, `light dark`
/// follows the system, `normal`/`light` never are.
pub fn used_dark(scheme: ColorScheme) -> bool {
    match scheme {
        ColorScheme::Dark => true,
        ColorScheme::Both => system_theme().dark,
        ColorScheme::Light => false,
    }
}

fn env_override(mut t: SystemTheme) -> SystemTheme {
    match std::env::var("NILE_COLOR_SCHEME").ok().as_deref().map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("dark") => t.dark = true,
        Some(v) if v.eq_ignore_ascii_case("light") => t.dark = false,
        _ => {}
    }
    if let Some(c) = std::env::var("NILE_ACCENT").ok().and_then(|v| parse_hex(&v)) {
        t.accent = c;
    }
    t
}

/// `#rrggbb` or `rrggbb`.
pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.trim().trim_start_matches('#');
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok()).flatten().map(|v| Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// The theme styles are computed against.
pub fn system_theme() -> SystemTheme {
    env_override(THEME.with(Cell::get).unwrap_or_default())
}

/// Tell the style engine the system's theme; returns whether it changed (then restyle).
pub fn set_system_theme(t: SystemTheme) -> bool {
    let before = system_theme();
    THEME.with(|c| c.set(Some(t)));
    before != system_theme()
}

/// What a system colour keyword (`canvas`, `buttonface`, `accentcolor`, ...) is right now. Names are
/// lower case.
pub fn system_color(name: &str) -> Option<Color> {
    let SystemTheme { dark, accent } = system_theme();
    // An element with its own `color-scheme` resolves system colours in that scheme.
    let dark = SCHEME.with(Cell::get).unwrap_or(dark);
    let pick = |light: (u8, u8, u8), dk: (u8, u8, u8)| {
        let (r, g, b) = if dark { dk } else { light };
        Some(Color::rgb(r, g, b))
    };
    match name {
        "canvas" => pick((255, 255, 255), (31, 31, 31)),
        "canvastext" => pick((0, 0, 0), (240, 240, 240)),
        "linktext" => pick((0, 0, 238), (138, 180, 248)),
        "visitedtext" => pick((85, 26, 139), (197, 138, 249)),
        "activetext" => pick((238, 0, 0), (255, 107, 107)),
        "buttonface" => pick((239, 239, 239), (58, 58, 58)),
        "buttontext" => pick((0, 0, 0), (255, 255, 255)),
        "buttonborder" => pick((118, 118, 118), (138, 138, 138)),
        "field" => pick((255, 255, 255), (43, 43, 43)),
        "fieldtext" => pick((0, 0, 0), (255, 255, 255)),
        "graytext" => pick((109, 109, 109), (160, 160, 160)),
        "mark" => pick((255, 255, 0), (102, 92, 0)),
        "marktext" => pick((0, 0, 0), (255, 255, 255)),
        "highlight" | "selecteditem" | "accentcolor" => Some(accent),
        "highlighttext" | "selecteditemtext" | "accentcolortext" => Some(text_on(accent)),
        _ => None,
    }
}

/// `color-scheme`: which schemes the page says it supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorScheme {
    /// `normal` / `light`: the light look only.
    #[default]
    Light,
    Dark,
    /// `light dark`: follows the system.
    Both,
}

impl ColorScheme {
    pub fn allows_dark(self) -> bool {
        matches!(self, ColorScheme::Dark | ColorScheme::Both)
    }

    /// From the value's words (`light`, `dark`, `only`, `normal`).
    pub fn parse<'a>(words: impl Iterator<Item = &'a str>) -> ColorScheme {
        let (mut light, mut dark) = (false, false);
        for w in words {
            match w {
                "light" => light = true,
                "dark" => dark = true,
                _ => {}
            }
        }
        match (light, dark) {
            (_, false) => ColorScheme::Light,
            (false, true) => ColorScheme::Dark,
            (true, true) => ColorScheme::Both,
        }
    }
}

/// The colours the renderer uses for what no stylesheet reaches: menu panels, scroll thumbs, bars.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub accent: Color,
    pub accent_text: Color,
    pub panel: Color,
    pub panel_border: Color,
    pub panel_text: Color,
    pub trough: Color,
}

impl Palette {
    pub fn new(dark: bool, accent: Color) -> Palette {
        Palette {
            dark,
            accent,
            accent_text: text_on(accent),
            panel: if dark { Color::rgb(43, 43, 43) } else { Color::rgb(255, 255, 255) },
            panel_border: if dark { Color::rgb(90, 90, 90) } else { Color::rgb(118, 118, 118) },
            panel_text: if dark { Color::rgb(240, 240, 240) } else { Color::rgb(34, 34, 34) },
            trough: if dark { Color::rgb(70, 70, 70) } else { Color::rgb(230, 230, 230) },
        }
    }
}
