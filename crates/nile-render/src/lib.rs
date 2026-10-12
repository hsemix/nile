//! Painting: walks the layout tree and draws it onto a Skia canvas.
//!
//! The canvas can come from anywhere — a CPU raster surface (current shell),
//! a Metal/D3D/Vulkan/GL surface later, or a PNG for tests. Paint code works in
//! logical pixels; callers apply the device scale to the canvas.

use std::collections::HashMap;

use nile_layout::{BoxContent, LayoutBox, LayoutTree, NodeId, Rect as LRect, TextContent};
use nile_style::{Appearance, Color as CssColor, Edges, StyleMap};
use nile_text::{Affinity, Cursor, PositionedLayoutItem};
use skia_safe::{
    font::Edging, Canvas, Color, Font, FontMgr, GlyphId, Paint, Point, RRect, Rect, Typeface,
};

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

    /// Paint the whole tree. `scroll_y` is in logical pixels. `caret` is the
    /// input being edited and the caret's byte offset in its displayed text.
    pub fn paint(
        &mut self,
        canvas: &Canvas,
        tree: &LayoutTree,
        styles: &StyleMap,
        scroll_y: f32,
        caret: Option<(NodeId, usize)>,
    ) {
        canvas.save();
        canvas.translate((0.0, -scroll_y));
        // `overflow: hidden` clips a box's subtree; each clip ends where its subtree does.
        let mut clips: Vec<usize> = Vec::new();
        for (i, b) in tree.boxes.iter().enumerate() {
            while clips.last().is_some_and(|end| i >= *end) {
                canvas.restore();
                clips.pop();
            }
            match &b.content {
                BoxContent::Element => {
                    self.paint_box(canvas, b, styles);
                    let s = styles.get(b.node);
                    if s.overflow_hidden && b.subtree_end > i + 1 {
                        let w = s.border_width;
                        let r = b.rect;
                        canvas.save();
                        canvas.clip_rect(
                            Rect::from_ltrb(r.x + w.left, r.y + w.top, r.x + r.width - w.right, r.y + r.height - w.bottom),
                            None,
                            true,
                        );
                        clips.push(b.subtree_end);
                    }
                }
                BoxContent::Text(content) => {
                    let caret_here = caret.filter(|(n, _)| content.owners.iter().any(|(_, o)| o == n));
                    // Keep the caret in view by scrolling the text left inside its input.
                    let (sx, sy) = caret_here.map_or((0.0, 0.0), |(n, at)| caret_scroll(tree, styles, n, content, at));
                    canvas.save();
                    canvas.translate((-sx, -sy));
                    self.paint_text(canvas, b.rect, content);
                    if let Some((_, at)) = caret_here {
                        paint_caret(canvas, b.rect, content, at, styles.get(b.node).color);
                    }
                    canvas.restore();
                }
            }
        }
        for _ in clips {
            canvas.restore();
        }
        canvas.restore();
    }

    fn paint_box(&mut self, canvas: &Canvas, b: &LayoutBox, styles: &StyleMap) {
        let s = styles.get(b.node);
        paint_decorated(canvas, skia_rect(b.rect), s.background, s.border_width, s.border_color, s.border_radius);
        paint_control_mark(canvas, s, b.rect);
        // TODO: opacity needs a save_layer around the element's whole subtree,
        // which needs the paint list to keep tree structure (stacking contexts).
    }

    fn paint_text(&mut self, canvas: &Canvas, rect: LRect, content: &TextContent) {
        let layout = &content.layout;
        let mut glyph_ids: Vec<GlyphId> = Vec::new();
        let mut positions: Vec<Point> = Vec::new();

        for line in layout.lines() {
            paint_inline_fragments(canvas, rect, content, &line);
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
                let [r, g, b, a] = style.brush.color;
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

/// Draw `bg` and a (per-side) border inside `outer`, with rounded corners.
fn paint_decorated(
    canvas: &Canvas,
    outer: Rect,
    bg: Option<CssColor>,
    w: Edges<f32>,
    border_color: CssColor,
    radius: f32,
) {
    let outer_r = RRect::new_rect_xy(outer, radius, radius);
    if let Some(bg) = bg.filter(|c| c.a > 0) {
        canvas.draw_rrect(outer_r, &fill(bg));
    }
    if (w.top > 0.0 || w.right > 0.0 || w.bottom > 0.0 || w.left > 0.0) && border_color.a > 0 {
        let inner_rect = Rect::from_ltrb(
            outer.left + w.left,
            outer.top + w.top,
            outer.right - w.right,
            outer.bottom - w.bottom,
        );
        let inner_radius = (radius - w.top.max(w.left)).max(0.0);
        let inner = RRect::new_rect_xy(inner_rect, inner_radius, inner_radius);
        canvas.draw_drrect(outer_r, inner, &fill(border_color));
    }
}

/// The check or radio dot for `appearance: checkbox | radio` when checked.
fn paint_control_mark(canvas: &Canvas, s: &nile_style::ComputedStyle, r: LRect) {
    if !s.checked {
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
        Appearance::None => {}
    }
}

/// How far to scroll an input's text (left, up) so the caret at `at` stays inside its content box.
fn caret_scroll(tree: &LayoutTree, styles: &StyleMap, input: NodeId, content: &TextContent, at: usize) -> (f32, f32) {
    let Some(rect) = tree.node_rect(input) else { return (0.0, 0.0) };
    let s = styles.get(input);
    let inner_w = rect.width - s.border_width.left - s.border_width.right - s.padding.left - s.padding.right;
    let inner_h = rect.height - s.border_width.top - s.border_width.bottom - s.padding.top - s.padding.bottom;
    let g = Cursor::from_byte_index(&content.layout, at, Affinity::Downstream).geometry(&content.layout, 1.0);
    ((g.x0 as f32 + 1.0 - inner_w).max(0.0), (g.y1 as f32 - inner_h).max(0.0))
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
        paint_decorated(canvas, rect, d.background, d.border, d.border_color, d.radius);
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
