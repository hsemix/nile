//! SVG for `<svg>`, `<img src="*.svg">` and `background-image: url(*.svg)`: parsing and rasterising
//! with resvg. Pixels come back premultiplied RGBA8888, ready for Skia.
//!
//! Not supported: embedded raster `<image>` elements, SMIL animation, scripts, and CSS that reaches
//! into the SVG from the page beyond what usvg's own style-sheet support understands.

use resvg::{tiny_skia, usvg};

/// What the page asks of an SVG besides its source.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Style {
    /// CSS applied to the document (the page's `<style>` rules), for selectors that reach the SVG.
    pub css: Option<String>,
}

/// An SVG's natural size in CSS pixels (`width`/`height`, else the `viewBox`).
pub fn size(source: &[u8]) -> Option<(f32, f32)> {
    let tree = parse(source, &Style::default())?;
    let s = tree.size();
    (s.width() > 0.0 && s.height() > 0.0).then(|| (s.width(), s.height()))
}

/// Whether `bytes` look like an SVG document (a leading `<svg`, possibly after an XML prologue,
/// comments or a doctype).
pub fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with("<svg") {
        return true;
    }
    (t.starts_with("<?xml") || t.starts_with("<!--") || t.starts_with("<!DOCTYPE")) && text.contains("<svg")
}

#[cfg(feature = "text")]
fn fonts() -> std::sync::Arc<usvg::fontdb::Database> {
    use std::{cell::RefCell, sync::Arc};
    thread_local! {
        static FONTS: RefCell<Option<Arc<usvg::fontdb::Database>>> = const { RefCell::new(None) };
    }
    FONTS.with(|f| {
        f.borrow_mut()
            .get_or_insert_with(|| {
                let mut db = usvg::fontdb::Database::new();
                db.load_system_fonts();
                Arc::new(db)
            })
            .clone()
    })
}

fn parse(source: &[u8], style: &Style) -> Option<usvg::Tree> {
    #[allow(unused_mut)]
    let mut options = usvg::Options { style_sheet: style.css.clone(), ..usvg::Options::default() };
    // System fonts are scanned once per thread, and only for documents that have text.
    #[cfg(feature = "text")]
    if source.windows(5).any(|w| w == b"<text") {
        options.fontdb = fonts();
    }
    usvg::Tree::from_data(source, &options).ok()
}

/// A rendered picture: premultiplied RGBA8888, `width * height * 4` bytes.
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Draw the document into a `width` x `height` pixel bitmap. The picture is fitted inside it and
/// centred (`preserveAspectRatio="xMidYMid meet"`), or stretched when `stretch` is set.
pub fn rasterize(source: &[u8], style: &Style, width: u32, height: u32, stretch: bool) -> Option<Raster> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return None;
    }
    let tree = parse(source, style)?;
    let natural = tree.size();
    let (sx, sy) = (width as f32 / natural.width(), height as f32 / natural.height());
    let transform = if stretch {
        tiny_skia::Transform::from_scale(sx, sy)
    } else {
        let s = sx.min(sy);
        tiny_skia::Transform::from_translate((width as f32 - natural.width() * s) / 2.0, (height as f32 - natural.height() * s) / 2.0).pre_scale(s, s)
    };
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(Raster { width, height, pixels: pixmap.take() })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED_SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10" viewBox="0 0 20 10"><rect width="20" height="10" fill="#ff0000"/></svg>"##;

    fn at(r: &Raster, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * r.width + x) * 4) as usize;
        [r.pixels[i], r.pixels[i + 1], r.pixels[i + 2], r.pixels[i + 3]]
    }

    #[test]
    fn size_comes_from_the_document() {
        assert_eq!(size(RED_SQUARE.as_bytes()), Some((20.0, 10.0)));
        let viewbox = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 24"><path d="M0 0h48v24z"/></svg>"##;
        assert_eq!(size(viewbox.as_bytes()), Some((48.0, 24.0)));
        assert_eq!(size(b"not svg"), None);
    }

    #[test]
    fn sniffing_tells_svg_from_other_files() {
        assert!(looks_like_svg(RED_SQUARE.as_bytes()));
        assert!(looks_like_svg(b"<?xml version=\"1.0\"?>\n<!-- hi -->\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>"));
        assert!(!looks_like_svg(b"\x89PNG\r\n\x1a\n"));
        assert!(!looks_like_svg(b"<html><body></body></html>"));
    }

    #[test]
    fn it_rasterises_scaled_and_centred() {
        let big = rasterize(RED_SQUARE.as_bytes(), &Style::default(), 40, 20, false).unwrap();
        assert_eq!(at(&big, 20, 10), [255, 0, 0, 255]);
        // Into a square box the 2:1 picture is fitted and centred: bars above and below stay clear.
        let fitted = rasterize(RED_SQUARE.as_bytes(), &Style::default(), 40, 40, false).unwrap();
        assert_eq!(at(&fitted, 20, 20), [255, 0, 0, 255]);
        assert_eq!(at(&fitted, 20, 2)[3], 0);
        let stretched = rasterize(RED_SQUARE.as_bytes(), &Style::default(), 40, 40, true).unwrap();
        assert_eq!(at(&stretched, 20, 2), [255, 0, 0, 255]);
        assert!(rasterize(b"junk", &Style::default(), 4, 4, false).is_none());
    }

    #[test]
    fn page_css_reaches_the_document() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect class="r" width="10" height="10" fill="#ff0000"/></svg>"##;
        let styled = Style { css: Some(".r { fill: #00ff00 }".into()) };
        let r = rasterize(svg.as_bytes(), &styled, 10, 10, false).unwrap();
        assert_eq!(at(&r, 5, 5), [0, 255, 0, 255]);
    }

    #[test]
    fn current_colour_follows_the_color_attribute() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" color="#0000ff"><rect width="10" height="10" fill="currentColor"/></svg>"##;
        let r = rasterize(svg.as_bytes(), &Style::default(), 10, 10, false).unwrap();
        assert_eq!(at(&r, 5, 5), [0, 0, 255, 255]);
    }
}
