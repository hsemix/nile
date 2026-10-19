//! The picture for a taskbar badge: a red disc with the text in white, 32x32 RGBA. Windows puts it
//! on the taskbar button as an overlay icon; it is plain Skia, so it is built (and tested) everywhere.

use skia_safe::{surfaces, AlphaType, Color, ColorType, Font, FontMgr, FontStyle, ImageInfo, Paint, Point};

pub const SIZE: i32 = 32;

#[cfg_attr(not(windows), allow(dead_code))]
pub fn render(text: &str) -> Option<(Vec<u8>, u32, u32)> {
    let mut surface = surfaces::raster_n32_premul((SIZE, SIZE))?;
    let canvas = surface.canvas();
    canvas.clear(Color::TRANSPARENT);
    let mut disc = Paint::default();
    disc.set_anti_alias(true);
    disc.set_color(Color::from_rgb(0xD1, 0x34, 0x38));
    let half = SIZE as f32 / 2.0;
    canvas.draw_circle(Point::new(half, half), half - 1.0, &disc);
    // Long text is cut: a badge is a count or a mark, and the disc is small.
    let label: String = if text.chars().count() > 3 { "99+".to_string() } else { text.to_string() };
    let size = match label.chars().count() {
        1 => 22.0,
        2 => 18.0,
        _ => 13.0,
    };
    let face = FontMgr::default().legacy_make_typeface(None, FontStyle::bold());
    let font = match face {
        Some(face) => Font::from_typeface(face, size),
        None => Font::default(),
    };
    let (width, bounds) = font.measure_str(&label, None);
    let mut ink = Paint::default();
    ink.set_anti_alias(true);
    ink.set_color(Color::WHITE);
    canvas.draw_str(&label, Point::new(half - width / 2.0, half - (bounds.top + bounds.bottom) / 2.0), &font, &ink);
    let info = ImageInfo::new((SIZE, SIZE), ColorType::RGBA8888, AlphaType::Unpremul, None);
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    surface.image_snapshot().read_pixels(&info, &mut rgba, SIZE as usize * 4, (0, 0), skia_safe::image::CachingHint::Allow).then_some(())?;
    Some((rgba, SIZE as u32, SIZE as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_badge_is_a_disc_with_ink_in_it() {
        let (rgba, w, h) = render("7").unwrap();
        assert_eq!((w, h, rgba.len()), (32, 32, 32 * 32 * 4));
        let at = |x: usize, y: usize| rgba[(y * 32 + x) * 4 + 3];
        assert_eq!(at(0, 0), 0, "the corner is outside the disc");
        assert_eq!(at(16, 3), 255, "the disc is opaque");
        assert!(rgba.as_chunks::<4>().0.iter().any(|p| p[0] > 240 && p[1] > 240 && p[2] > 240 && p[3] == 255), "white text");
    }
}
