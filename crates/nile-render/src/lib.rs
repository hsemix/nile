//! Painting: walks the layout tree and draws it onto a Skia canvas.
//!
//! The canvas can come from anywhere — a CPU raster surface (current shell),
//! a Metal/D3D/Vulkan/GL surface later, or a PNG for tests. Paint code works in
//! logical pixels; callers apply the device scale to the canvas.

use std::collections::HashMap;

use nile_layout::{BoxContent, LayoutBox, LayoutTree, MenuBox, NodeId, Rect as LRect, ScrollOffsets, TextContent};
use nile_style::{Appearance, BorderStyle, Color as CssColor, Edges, Extent, Gradient, GradientKind, LinearDir, StopPos, StyleMap};
use nile_text::{Affinity, Cursor, PositionedLayoutItem, Selection};
use skia_safe::{
    font::Edging, Canvas, Color, Font, FontMgr, GlyphId, Paint, Point, RRect, Rect, Typeface,
};

/// What to draw on top of the page content for one frame.
#[derive(Clone, Copy)]
pub struct Overlay<'a> {
    /// Page scroll in logical pixels.
    pub scroll_y: f32,
    /// Scroll offsets of `overflow` elements.
    pub offsets: &'a ScrollOffsets,
    /// The input being edited and the caret's byte offset in its displayed text
    /// (`None` while the caret is in the off phase of its blink).
    pub caret: Option<(NodeId, usize)>,
    /// The input with a selection and the selected byte range of its displayed text.
    pub selection: Option<(NodeId, usize, usize)>,
}

pub struct Renderer {
    font_mgr: FontMgr,
    /// Skia typefaces keyed by (Parley font blob id, collection index).
    typefaces: HashMap<(u64, u32), Option<Typeface>>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Renderer { font_mgr: FontMgr::new(), typefaces: HashMap::new() }
    }

    /// Paint the whole tree.
    pub fn paint(&mut self, canvas: &Canvas, tree: &LayoutTree, styles: &StyleMap, overlay: &Overlay) {
        let Overlay { scroll_y, offsets, caret, selection } = *overlay;
        canvas.save();
        canvas.translate((0.0, -scroll_y));
        // Boxes that wrap their subtree (an `overflow` clip, or an `opacity` layer) end
        // where the subtree does. `None` is an opacity layer.
        let mut open: Vec<(usize, Option<&LayoutBox>)> = Vec::new();
        for (i, b) in tree.boxes.iter().enumerate() {
            while open.last().is_some_and(|(end, _)| i >= *end) {
                let (_, container) = open.pop().expect("checked above");
                canvas.restore();
                if let Some(container) = container {
                    paint_scrollbars(canvas, container, offsets);
                }
            }
            match &b.content {
                BoxContent::Element => {
                    let opacity = styles.get(b.node).opacity;
                    if opacity < 1.0 {
                        canvas.save_layer_alpha_f(None, opacity);
                        open.push((b.subtree_end.max(i + 1), None));
                    }
                    self.paint_box(canvas, b, styles);
                    if let Some(sb) = &b.scroll {
                        let (ox, oy) = offsets.get(&b.node).copied().unwrap_or_default();
                        canvas.save();
                        canvas.clip_rect(skia_rect(sb.clip), None, true);
                        canvas.translate((-ox, -oy));
                        open.push((b.subtree_end, Some(b)));
                    }
                }
                BoxContent::Text(content) => {
                    if let Some((_, from, to)) = selection.filter(|(n, ..)| content.owners.iter().any(|(_, o)| o == n)) {
                        paint_selection(canvas, b.rect, content, from, to);
                    }
                    self.paint_text(canvas, b.rect, content);
                    if let Some((_, at)) = caret.filter(|(n, _)| content.owners.iter().any(|(_, o)| o == n)) {
                        paint_caret(canvas, b.rect, content, at, styles.get(b.node).color);
                    }
                }
            }
        }
        while let Some((_, container)) = open.pop() {
            canvas.restore();
            if let Some(container) = container {
                paint_scrollbars(canvas, container, offsets);
            }
        }
        if let Some(menu) = &tree.menu {
            self.paint_menu(canvas, menu);
        }
        canvas.restore();
    }

    /// The open `<select>` list: white panel, highlighted row, a mark on the chosen one.
    fn paint_menu(&mut self, canvas: &Canvas, m: &MenuBox) {
        let r = m.rect;
        let panel = RRect::new_rect_xy(skia_rect(r), 3.0, 3.0);
        canvas.draw_rrect(panel, &fill(CssColor::rgb(255, 255, 255)));
        let mut border = fill(CssColor::rgb(118, 118, 118));
        border.set_style(skia_safe::PaintStyle::Stroke);
        border.set_stroke_width(1.0);
        canvas.draw_rrect(RRect::new_rect_xy(Rect::from_xywh(r.x + 0.5, r.y + 0.5, r.width - 1.0, r.height - 1.0), 3.0, 3.0), &border);
        for (i, item) in m.items.iter().enumerate() {
            let top = r.y + 1.0 + m.row_height * i as f32;
            let on = m.highlight == Some(i) && !item.disabled;
            if on {
                canvas.draw_rect(Rect::from_xywh(r.x + 1.0, top, r.width - 2.0, m.row_height), &fill(CssColor::rgb(29, 78, 216)));
            }
            let line_h = item.text.height();
            let at = LRect { x: r.x + m.pad_x, y: top + (m.row_height - line_h) / 2.0, width: r.width, height: line_h };
            // Text carries its own color; selected-row and disabled rows are tinted by an overlay.
            self.paint_text_tinted(canvas, at, &item.text, if on { Some([255, 255, 255, 255]) } else if item.disabled { Some([150, 150, 150, 255]) } else { None });
            if m.selected == Some(i) && !on {
                canvas.draw_rect(Rect::from_xywh(r.x + 2.0, top + 2.0, 2.0, m.row_height - 4.0), &fill(CssColor::rgb(29, 78, 216)));
            }
        }
    }

    fn paint_box(&mut self, canvas: &Canvas, b: &LayoutBox, styles: &StyleMap) {
        let s = styles.get(b.node);
        paint_shadows(canvas, skia_rect(b.rect), &s.box_shadow, s.border_radius);
        paint_decorated(canvas, skia_rect(b.rect), s.background, s.background_image.as_ref(), &Border { width: s.border_width, color: s.border_color, style: s.border_style }, s.border_radius);
        paint_control_mark(canvas, s, b.rect);
    }

    fn paint_text(&mut self, canvas: &Canvas, rect: LRect, content: &TextContent) {
        self.paint_layout(canvas, rect, &content.layout, Some(content), None);
    }

    /// Paint a bare text layout (menu rows), optionally overriding its color.
    fn paint_text_tinted(&mut self, canvas: &Canvas, rect: LRect, layout: &nile_text::TextLayout, tint: Option<[u8; 4]>) {
        self.paint_layout(canvas, rect, layout, None, tint);
    }

    fn paint_layout(
        &mut self,
        canvas: &Canvas,
        rect: LRect,
        layout: &nile_text::TextLayout,
        content: Option<&TextContent>,
        tint: Option<[u8; 4]>,
    ) {
        let mut glyph_ids: Vec<GlyphId> = Vec::new();
        let mut positions: Vec<Point> = Vec::new();

        for line in layout.lines() {
            if let Some(content) = content {
                paint_inline_fragments(canvas, rect, content, &line);
            }
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };
                let run = glyph_run.run();
                let font_data = run.font();
                let Some(typeface) = self.typeface(font_data.data.id(), font_data.index, font_data.data.data())
                else {
                    continue;
                };

                let mut font = Font::from_typeface(typeface, run.font_size());
                font.set_edging(Edging::SubpixelAntiAlias);
                font.set_subpixel(true);
                let synth = run.synthesis();
                font.set_embolden(synth.embolden());
                if let Some(deg) = synth.skew() {
                    font.set_skew_x(-deg.to_radians().tan());
                }
                // TODO: apply variable font coords.

                glyph_ids.clear();
                positions.clear();
                for g in glyph_run.positioned_glyphs() {
                    glyph_ids.push(g.id as GlyphId);
                    positions.push(Point::new(g.x, g.y));
                }

                let style = glyph_run.style();
                let [r, g, b, a] = tint.unwrap_or(style.brush.color);
                let mut paint = Paint::default();
                paint.set_anti_alias(true);
                paint.set_color(Color::from_argb(a, r, g, b));

                let x = rect.x + glyph_run.offset();
                let baseline = rect.y + glyph_run.baseline();
                let fm = run.font_metrics();
                canvas.draw_glyphs_at(&glyph_ids, positions.as_slice(), (rect.x, rect.y), &font, &paint);

                let line_at = |offset: f32, size: f32| {
                    let top = baseline - offset;
                    canvas.draw_rect(Rect::from_ltrb(x, top, x + glyph_run.advance(), top + size.max(1.0)), &paint);
                };
                if style.underline.is_some() {
                    line_at(fm.underline_offset, fm.underline_size);
                }
                if style.strikethrough.is_some() {
                    line_at(fm.strikethrough_offset, fm.strikethrough_size);
                }
            }
        }
    }

    fn typeface(&mut self, blob_id: u64, index: u32, bytes: &[u8]) -> Option<Typeface> {
        self.typefaces
            .entry((blob_id, index))
            .or_insert_with(|| {
                self.font_mgr.new_from_data(bytes, index as usize).or_else(|| {
                    // Some backends (CoreText) refuse a non-zero face index in
                    // a collection; hand them the face as a standalone font.
                    let single = extract_collection_face(bytes, index as usize)?;
                    self.font_mgr.new_from_data(&single, 0)
                })
            })
            .clone()
    }
}

fn skia_rect(r: LRect) -> Rect {
    Rect::from_xywh(r.x, r.y, r.width, r.height)
}

fn fill(c: CssColor) -> Paint {
    let mut p = Paint::default();
    p.set_anti_alias(true);
    p.set_color(Color::from_argb(c.a, c.r, c.g, c.b));
    p
}

/// Copy face `index` out of a TrueType/OpenType collection (`.ttc`) into a
/// standalone sfnt. Returns `None` if `bytes` isn't a collection or is malformed.
fn extract_collection_face(bytes: &[u8], index: usize) -> Option<Vec<u8>> {
    let u16_at = |o: usize| bytes.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as usize);
    let u32_at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);

    if bytes.get(0..4)? != b"ttcf" || index >= u32_at(8)? {
        return None;
    }
    let face = u32_at(12 + 4 * index)?;
    let num_tables = u16_at(face + 4)?;
    let dir_len = 12 + 16 * num_tables;

    let mut out = bytes.get(face..face + 12)?.to_vec();
    let mut tables: Vec<u8> = Vec::new();
    for i in 0..num_tables {
        let rec = face + 12 + 16 * i;
        let (offset, length) = (u32_at(rec + 8)?, u32_at(rec + 12)?);
        let data = bytes.get(offset..offset.checked_add(length)?)?;
        let new_offset = dir_len + tables.len();
        out.extend_from_slice(bytes.get(rec..rec + 8)?); // tag + checksum
        out.extend_from_slice(&(new_offset as u32).to_be_bytes());
        out.extend_from_slice(&(length as u32).to_be_bytes());
        tables.extend_from_slice(data);
        tables.resize(tables.len().next_multiple_of(4), 0);
    }
    out.extend_from_slice(&tables);
    Some(out)
}

/// The Skia shader for `g` painted over `r`.
fn gradient_shader(g: &Gradient, r: Rect) -> Option<skia_safe::Shader> {
    use skia_safe::gradient::{interpolation::InPremul, Colors, Gradient as SkGradient, Interpolation};
    let (w, h) = (r.width(), r.height());
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let resolve = |p: StopPos, whole: f32| match p {
        StopPos::Px(px) => px,
        StopPos::Fraction(f) => f * whole,
    };
    let (cx, cy) = (r.left + w / 2.0, r.top + h / 2.0);
    // The gradient line's length in px, and its start/end in the box.
    let (length, geometry) = match g.kind {
        GradientKind::Linear(dir) => {
            let (dx, dy) = match dir {
                LinearDir::Angle(a) => (a.to_radians().sin(), -a.to_radians().cos()),
                LinearDir::Corner(sx, sy) => {
                    let (vx, vy) = (f32::from(sx) * h, f32::from(sy) * w);
                    let n = vx.hypot(vy);
                    (vx / n, vy / n)
                }
            };
            let length = (w * dx).abs() + (h * dy).abs();
            (length, Shape::Line { from: (cx - dx * length / 2.0, cy - dy * length / 2.0), dir: (dx, dy) })
        }
        GradientKind::Radial { circle, extent, at } => {
            let (px, py) = (r.left + resolve(at.0, w), r.top + resolve(at.1, h));
            let (l, t, rt, b) = (px - r.left, py - r.top, r.right - px, r.bottom - py);
            let (near_x, near_y, far_x, far_y) = (l.min(rt), t.min(b), l.max(rt), t.max(b));
            let (rx, ry) = match extent {
                Extent::Radii(a, b) => (resolve(a, w), resolve(b, h)),
                Extent::ClosestSide if circle => (near_x.min(near_y), near_x.min(near_y)),
                Extent::ClosestSide => (near_x, near_y),
                Extent::FarthestSide if circle => (far_x.max(far_y), far_x.max(far_y)),
                Extent::FarthestSide => (far_x, far_y),
                Extent::ClosestCorner if circle => {
                    let d = near_x.hypot(near_y);
                    (d, d)
                }
                Extent::ClosestCorner => (near_x * std::f32::consts::SQRT_2, near_y * std::f32::consts::SQRT_2),
                Extent::FarthestCorner if circle => {
                    let d = far_x.hypot(far_y);
                    (d, d)
                }
                Extent::FarthestCorner => (far_x * std::f32::consts::SQRT_2, far_y * std::f32::consts::SQRT_2),
            };
            (rx, Shape::Radial { center: (px, py), rx: rx.max(0.01), ry: ry.max(0.01) })
        }
    };
    let stops = g.resolved_stops(length);
    let (first, last) = (stops.first()?.0, stops.last()?.0);
    // A repeating gradient tiles the span between its first and last stop.
    let (lo, hi) = if g.repeating && last > first { (first, last) } else { (0.0, 1.0) };
    let colors: Vec<skia_safe::Color4f> = stops
        .iter()
        .map(|(_, c)| skia_safe::Color4f::new(f32::from(c.r) / 255.0, f32::from(c.g) / 255.0, f32::from(c.b) / 255.0, f32::from(c.a) / 255.0))
        .collect();
    let mut prev = f32::MIN;
    let positions: Vec<f32> = stops
        .iter()
        .map(|(p, _)| {
            prev = ((p - lo) / (hi - lo)).clamp(0.0, 1.0).max(prev);
            prev
        })
        .collect();
    let mode = if g.repeating && last > first { skia_safe::TileMode::Repeat } else { skia_safe::TileMode::Clamp };
    let interpolation = Interpolation { in_premul: InPremul::Yes, ..Interpolation::default() };
    let sk = SkGradient::new(Colors::new(&colors, Some(&positions), mode, None), interpolation);
    match geometry {
        Shape::Line { from, dir } => {
            let a = (from.0 + dir.0 * length * lo, from.1 + dir.1 * length * lo);
            let b = (from.0 + dir.0 * length * hi, from.1 + dir.1 * length * hi);
            skia_safe::shaders::linear_gradient((a, b), &sk, None)
        }
        Shape::Radial { center, rx, ry } => {
            // A unit circle scaled to the ellipse; the repeat span is `lo..hi` of the radius.
            let m = skia_safe::Matrix::translate(center) * skia_safe::Matrix::scale((rx * (hi - lo), ry * (hi - lo)));
            skia_safe::shaders::radial_gradient(((0.0, 0.0), 1.0), &sk, &m)
        }
    }
}

enum Shape {
    Line { from: (f32, f32), dir: (f32, f32) },
    Radial { center: (f32, f32), rx: f32, ry: f32 },
}

/// Outer `box-shadow`s: a blurred copy of the box outline, kept out from under the box.
// TODO: `inset` shadows.
fn paint_shadows(canvas: &Canvas, outer: Rect, shadows: &[nile_style::BoxShadow], radius: f32) {
    for sh in shadows.iter().rev().filter(|s| !s.inset && s.color.a > 0) {
        let shape = Rect::from_ltrb(
            outer.left - sh.spread + sh.dx,
            outer.top - sh.spread + sh.dy,
            outer.right + sh.spread + sh.dx,
            outer.bottom + sh.spread + sh.dy,
        );
        let r = (radius + sh.spread).max(0.0);
        let mut paint = fill(sh.color);
        if sh.blur > 0.0 {
            paint.set_mask_filter(skia_safe::MaskFilter::blur(skia_safe::BlurStyle::Normal, sh.blur / 2.0, false));
        }
        canvas.save();
        canvas.clip_rrect(RRect::new_rect_xy(outer, radius, radius), skia_safe::ClipOp::Difference, true);
        canvas.draw_rrect(RRect::new_rect_xy(shape, r, r), &paint);
        canvas.restore();
    }
}

/// A box's border: width, color and line style for each side.
struct Border {
    width: Edges<f32>,
    color: Edges<CssColor>,
    style: Edges<BorderStyle>,
}

/// Draw `bg` and a (per-side) border inside `outer`, with rounded corners.
fn paint_decorated(canvas: &Canvas, outer: Rect, bg: Option<CssColor>, image: Option<&Gradient>, border: &Border, radius: f32) {
    let (w, c, st) = (border.width, border.color, border.style);
    let outer_r = RRect::new_rect_xy(outer, radius, radius);
    if let Some(bg) = bg.filter(|c| c.a > 0) {
        canvas.draw_rrect(outer_r, &fill(bg));
    }
    if let Some(shader) = image.and_then(|g| gradient_shader(g, outer)) {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_shader(shader);
        canvas.draw_rrect(outer_r, &paint);
    }
    if !(w.top > 0.0 || w.right > 0.0 || w.bottom > 0.0 || w.left > 0.0) {
        return;
    }
    let uniform = c.top == c.right && c.top == c.bottom && c.top == c.left;
    let solid = [st.top, st.right, st.bottom, st.left].iter().all(|s| matches!(*s, BorderStyle::Solid | BorderStyle::None));
    if uniform && solid {
        if c.top.a == 0 {
            return;
        }
        let inner_rect = Rect::from_ltrb(outer.left + w.left, outer.top + w.top, outer.right - w.right, outer.bottom - w.bottom);
        let inner_radius = (radius - w.top.max(w.left)).max(0.0);
        let inner = RRect::new_rect_xy(inner_rect, inner_radius, inner_radius);
        canvas.draw_drrect(outer_r, inner, &fill(c.top));
        return;
    }
    // Mixed colors or styles: each side on its own, clipped to the rounded outline.
    canvas.save();
    canvas.clip_rrect(outer_r, skia_safe::ClipOp::Intersect, true);
    let (l, t, r, b) = (outer.left, outer.top, outer.right, outer.bottom);
    let sides = [
        (w.top, c.top, st.top, [(l, t), (r, t), (r - w.right, t + w.top), (l + w.left, t + w.top)], ((l, t + w.top / 2.0), (r, t + w.top / 2.0))),
        (w.right, c.right, st.right, [(r, t), (r, b), (r - w.right, b - w.bottom), (r - w.right, t + w.top)], ((r - w.right / 2.0, t), (r - w.right / 2.0, b))),
        (w.bottom, c.bottom, st.bottom, [(l, b), (r, b), (r - w.right, b - w.bottom), (l + w.left, b - w.bottom)], ((l, b - w.bottom / 2.0), (r, b - w.bottom / 2.0))),
        (w.left, c.left, st.left, [(l, t), (l, b), (l + w.left, b - w.bottom), (l + w.left, t + w.top)], ((l + w.left / 2.0, t), (l + w.left / 2.0, b))),
    ];
    for (width, color, style, quad, (from, to)) in sides {
        if width <= 0.0 || color.a == 0 {
            continue;
        }
        let mut paint = fill(color);
        match style {
            BorderStyle::None => {}
            BorderStyle::Solid => {
                let mut path = skia_safe::PathBuilder::new();
                path.move_to(quad[0]).line_to(quad[1]).line_to(quad[2]).line_to(quad[3]).close();
                canvas.draw_path(&path.detach(), &paint);
            }
            BorderStyle::Dashed | BorderStyle::Dotted => {
                let dotted = style == BorderStyle::Dotted;
                let on = if dotted { width } else { width * 3.0 };
                paint.set_style(skia_safe::PaintStyle::Stroke);
                paint.set_stroke_width(width);
                if dotted {
                    paint.set_stroke_cap(skia_safe::PaintCap::Round);
                }
                // Round dots add half the width at each end, so the gap makes up for it.
                let intervals = if dotted { [0.01, width * 2.0] } else { [on, on] };
                paint.set_path_effect(skia_safe::PathEffect::dash(&intervals, 0.0));
                let mut path = skia_safe::PathBuilder::new();
                path.move_to(from).line_to(to);
                canvas.draw_path(&path.detach(), &paint);
            }
        }
    }
    canvas.restore();
}

/// The check or radio dot for `appearance: checkbox | radio` when checked.
fn paint_control_mark(canvas: &Canvas, s: &nile_style::ComputedStyle, r: LRect) {
    // Checkbox and radio marks show only when checked; a select's chevron always does.
    if !s.checked && s.appearance != Appearance::Select {
        return;
    }
    let mut paint = fill(s.color);
    match s.appearance {
        Appearance::Checkbox => {
            let mut builder = skia_safe::PathBuilder::new();
            builder
                .move_to((r.x + r.width * 0.24, r.y + r.height * 0.53))
                .line_to((r.x + r.width * 0.43, r.y + r.height * 0.72))
                .line_to((r.x + r.width * 0.78, r.y + r.height * 0.30));
            let path = builder.detach();
            paint.set_style(skia_safe::PaintStyle::Stroke);
            paint.set_stroke_width((r.width * 0.16).max(1.5));
            paint.set_stroke_cap(skia_safe::PaintCap::Round);
            paint.set_stroke_join(skia_safe::PaintJoin::Round);
            canvas.draw_path(&path, &paint);
        }
        Appearance::Radio => {
            canvas.draw_circle((r.x + r.width / 2.0, r.y + r.height / 2.0), r.width.min(r.height) * 0.24, &paint);
        }
        Appearance::Select => {
            // A downward chevron in the right padding.
            let (cx, cy) = (r.x + r.width - 13.0, r.y + r.height / 2.0);
            let mut builder = skia_safe::PathBuilder::new();
            builder.move_to((cx - 3.5, cy - 1.5)).line_to((cx, cy + 2.0)).line_to((cx + 3.5, cy - 1.5));
            let path = builder.detach();
            paint.set_style(skia_safe::PaintStyle::Stroke);
            paint.set_stroke_width(1.6);
            paint.set_stroke_cap(skia_safe::PaintCap::Round);
            paint.set_stroke_join(skia_safe::PaintJoin::Round);
            canvas.draw_path(&path, &paint);
        }
        Appearance::None => {}
    }
}

/// Thin overlay scrollbars for an `overflow: auto | scroll` box that has more to show.
fn paint_scrollbars(canvas: &Canvas, b: &LayoutBox, offsets: &ScrollOffsets) {
    let Some(sb) = &b.scroll else { return };
    let (max_x, max_y) = sb.max_offset();
    let (ox, oy) = offsets.get(&b.node).copied().unwrap_or_default();
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(Color::from_argb(110, 0, 0, 0));
    const THICK: f32 = 6.0;
    const EDGE: f32 = 2.0;
    let c = sb.clip;
    if sb.wheel_y && max_y > 0.0 {
        let len = (c.height * c.height / sb.content_height).clamp(16.0, c.height);
        let top = c.y + (c.height - len) * (oy / max_y).clamp(0.0, 1.0);
        let r = Rect::from_xywh(c.x + c.width - THICK - EDGE, top, THICK, len);
        canvas.draw_round_rect(r, THICK / 2.0, THICK / 2.0, &paint);
    }
    if sb.wheel_x && max_x > 0.0 {
        let len = (c.width * c.width / sb.content_width).clamp(16.0, c.width);
        let left = c.x + (c.width - len) * (ox / max_x).clamp(0.0, 1.0);
        let r = Rect::from_xywh(left, c.y + c.height - THICK - EDGE, len, THICK);
        canvas.draw_round_rect(r, THICK / 2.0, THICK / 2.0, &paint);
    }
}

/// Highlight bytes `from..to` of a text run.
fn paint_selection(canvas: &Canvas, origin: LRect, content: &TextContent, from: usize, to: usize) {
    let layout = &content.layout;
    let sel = Selection::new(
        Cursor::from_byte_index(layout, from, Affinity::Downstream),
        Cursor::from_byte_index(layout, to, Affinity::Upstream),
    );
    let mut paint = Paint::default();
    paint.set_color(Color::from_argb(90, 59, 130, 246));
    for (r, _) in sel.geometry(layout) {
        let rect = Rect::from_ltrb(
            origin.x + r.x0 as f32,
            origin.y + r.y0 as f32,
            origin.x + r.x1 as f32,
            origin.y + r.y1 as f32,
        );
        canvas.draw_rect(rect, &paint);
    }
}

/// A 1px caret at byte offset `at` of a text run.
fn paint_caret(canvas: &Canvas, origin: LRect, content: &TextContent, at: usize, color: CssColor) {
    let g = Cursor::from_byte_index(&content.layout, at, Affinity::Downstream).geometry(&content.layout, 1.0);
    let rect = Rect::from_ltrb(origin.x + g.x0 as f32, origin.y + g.y0 as f32, origin.x + g.x1 as f32, origin.y + g.y1 as f32);
    canvas.draw_rect(rect, &fill(color));
}

/// One line's piece of a decorated inline element.
struct Fragment {
    deco: usize,
    x0: f32,
    x1: f32,
    top: f32,
    bottom: f32,
}

/// Paint backgrounds and borders of decorated inline elements on one line:
/// each element's glyph runs and padding spacers are unioned into a fragment.
fn paint_inline_fragments(
    canvas: &Canvas,
    origin: LRect,
    content: &TextContent,
    line: &nile_text::Line<nile_text::TextBrush>,
) {
    let mut frags: Vec<Fragment> = Vec::new();
    // The fragment that the next item may extend.
    let mut open: Option<usize> = None;
    let new_frag = |deco| Fragment { deco, x0: 0.0, x1: 0.0, top: f32::INFINITY, bottom: f32::NEG_INFINITY };

    for item in line.items() {
        match item {
            PositionedLayoutItem::GlyphRun(gr) => {
                let d = gr.style().brush.deco;
                if d == 0 {
                    open = None;
                    continue;
                }
                let deco = d as usize - 1;
                let fm = gr.run().font_metrics();
                let (x0, x1) = (gr.offset(), gr.offset() + gr.advance());
                // Spacers only contribute width; their font size is arbitrary.
                let (top, bottom) = if gr.style().brush.spacer {
                    (f32::INFINITY, f32::NEG_INFINITY)
                } else {
                    (gr.baseline() - fm.ascent, gr.baseline() + fm.descent)
                };
                let i = match open {
                    Some(i) if frags[i].deco == deco => i,
                    _ => {
                        frags.push(Fragment { x0, ..new_frag(deco) });
                        frags.len() - 1
                    }
                };
                let f = &mut frags[i];
                f.x1 = x1;
                f.top = f.top.min(top);
                f.bottom = f.bottom.max(bottom);
                open = Some(i);
            }
            PositionedLayoutItem::InlineBox(b) => {
                if let Some(i) = open {
                    let f = &mut frags[i];
                    f.x1 = b.x + b.width;
                    f.top = f.top.min(b.y);
                    f.bottom = f.bottom.max(b.y + b.height);
                }
            }
        }
    }

    for f in frags.iter().filter(|f| f.top <= f.bottom) {
        let Some(d) = content.decos.get(f.deco) else { continue };
        let rect = Rect::from_ltrb(
            origin.x + f.x0,
            origin.y + f.top - d.padding.top - d.border.top,
            origin.x + f.x1,
            origin.y + f.bottom + d.padding.bottom + d.border.bottom,
        );
        paint_decorated(canvas, rect, d.background, None, &Border { width: d.border, color: d.border_color, style: d.border_style }, d.radius);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_collections() {
        assert!(extract_collection_face(b"\0\x01\0\0rest", 0).is_none());
        assert!(extract_collection_face(b"", 0).is_none());
    }
}
