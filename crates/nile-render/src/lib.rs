//! Painting: walks the layout tree and draws it onto a Skia canvas.
//!
//! The canvas can come from anywhere — a CPU raster surface (current shell),
//! a Metal/D3D/Vulkan/GL surface later, or a PNG for tests. Paint code works in
//! logical pixels; callers apply the device scale to the canvas.

use std::collections::HashMap;

use nile_layout::{ImageData, BoxContent, LayoutBox, LayoutTree, MenuBox, NodeId, Rect as LRect, ScrollOffsets, TextContent};
use nile_style::{BgSize, Appearance, BorderStyle, Color as CssColor, Edges, Extent, Gradient, GradientKind, LinearDir, StopPos, StyleMap};
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
    /// The window's logical size, for the page's own scrollbar.
    pub viewport: (f32, f32),
    /// Selected page text: `(text run, from, to)` byte ranges.
    pub page_selection: &'a [(usize, usize, usize)],
    /// The caret in `contenteditable` content (`None` while blinked off or without one).
    pub dom_caret: Option<DomCaret>,
    /// An input method's composition in a text input: the input and the byte range of its text.
    pub preedit: Option<(NodeId, usize, usize)>,
    /// Paint only these boxes `[from, to)` of the paint list (an element's subtree, for a popup
    /// shown in a window of its own); no menu or page scrollbar then.
    pub only: Option<(usize, usize)>,
    /// The selected image's box in layout coordinates: drawn with an outline and resize handles.
    pub selected_image: Option<LRect>,
    /// Boxes `[from, to)` not to paint here (a popup that has a window of its own).
    pub skip: &'a [(usize, usize)],
    /// The scrollbar under the pointer (its scroller, `None` for the page, and whether it is the
    /// vertical one), and whether it is being dragged.
    pub bar: Option<(Option<NodeId>, bool, bool)>,
}

/// Where the caret of an editing host is drawn.
#[derive(Clone, Copy, Debug)]
pub enum DomCaret {
    /// In a text run at a byte offset.
    Run { run: usize, byte: usize },
    /// In an empty element: a line-high bar at `rect` (layout coordinates) inside box `run`.
    Empty { run: usize, rect: LRect },
}

pub struct Renderer {
    font_mgr: FontMgr,
    /// Skia typefaces keyed by (Parley font blob id, collection index).
    typefaces: HashMap<(u64, u32), Option<Typeface>>,
    /// Variable fonts at the axis positions (normalized, as Parley reports them) a run uses.
    varied: HashMap<(u64, u32, Vec<i16>), Option<Typeface>>,
    /// Decoded pictures by `ImageData::id`; `None` where decoding failed.
    pictures: HashMap<u64, Option<skia_safe::Image>>,
    /// SVG pictures rasterised at the pixel size they were drawn at: `(ImageData::id, width, height)`.
    svg_pictures: HashMap<(u64, u32, u32), Option<skia_safe::Image>>,
    /// The system theme's colours for what no stylesheet reaches (set at the start of each paint).
    pal: nile_style::Palette,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Renderer { font_mgr: FontMgr::new(), typefaces: HashMap::new(), varied: HashMap::new(), pictures: HashMap::new(), svg_pictures: HashMap::new(), pal: nile_style::Palette::new(false, nile_style::default_accent()) }
    }

    /// Paint the whole tree.
    pub fn paint(&mut self, canvas: &Canvas, tree: &LayoutTree, styles: &StyleMap, overlay: &Overlay) {
        self.pal = styles.palette();
        let Overlay { scroll_y, viewport, only, selected_image, .. } = *overlay;
        let base = canvas.local_to_device_as_3x3();
        canvas.save();
        canvas.translate((0.0, -scroll_y));
        let (from, to) = only.unwrap_or((0, tree.boxes.len()));
        self.paint_boxes(canvas, tree, styles, overlay, &base, from, to);
        if let Some(r) = selected_image.filter(|_| only.is_none()) {
            Self::paint_selected_image(canvas, r);
        }
        if let Some(menu) = tree.menu.as_ref().filter(|_| only.is_none()) {
            self.paint_menu(canvas, menu);
        }
        canvas.restore();
        let look = styles.page_scrollbar();
        if let Some(t) = nile_layout::page_thumb(viewport, tree.content_height, scroll_y, look.thickness).filter(|_| only.is_none()) {
            paint_thumb(canvas, &t, &look, bar_state(overlay, None, t.vertical), styles.dark);
        }
    }

    /// Boxes `[from, to)` of the paint list. A paged element's subtree is painted once per sheet.
    #[allow(clippy::too_many_arguments)]
    fn paint_boxes(&mut self, canvas: &Canvas, tree: &LayoutTree, styles: &StyleMap, overlay: &Overlay, base: &skia_safe::Matrix, from: usize, to: usize) {
        let Overlay { scroll_y, offsets, caret, selection, viewport, page_selection, dom_caret, preedit, skip, .. } = *overlay;
        // Boxes that wrap their subtree (an `overflow` clip, or an `opacity` layer) end
        // where the subtree does. `None` is an opacity layer.
        let mut open: Vec<(usize, Option<&LayoutBox>)> = Vec::new();
        let mut jump = 0;
        for (i, b) in tree.boxes.iter().enumerate().skip(from).take(to.saturating_sub(from)) {
            if i < jump {
                continue;
            }
            while open.last().is_some_and(|(end, _)| i >= *end) {
                let (_, container) = open.pop().expect("checked above");
                canvas.restore();
                if let Some(container) = container {
                    paint_scrollbars(canvas, container, offsets, styles, overlay);
                }
            }
            if skip.iter().any(|&(s, e)| i >= s && i < e) {
                continue;
            }
            if let Some(pb) = tree.pages.iter().find(|pb| pb.index == i && pb.end <= to) {
                self.paint_sheets(canvas, tree, styles, overlay, base, pb);
                jump = pb.end;
                continue;
            }
            match &b.content {
                BoxContent::Element => {
                    if b.fixed {
                        // Pinned to the viewport: drop the page scroll and any ancestor transform.
                        canvas.save();
                        canvas.set_matrix(&skia_safe::M44::from(base));
                        open.push((b.subtree_end.max(i + 1), None));
                    }
                    let dy = b.sticky_shift(scroll_y, viewport.1);
                    if dy != 0.0 {
                        canvas.save();
                        canvas.translate((0.0, dy));
                        open.push((b.subtree_end.max(i + 1), None));
                    }
                    let opacity = styles.get(b.node).opacity;
                    if opacity < 1.0 {
                        canvas.save_layer_alpha_f(None, opacity);
                        open.push((b.subtree_end.max(i + 1), None));
                    }
                    if let Some(t) = &b.transform {
                        canvas.save();
                        canvas.concat(&skia_safe::Matrix::new_all(t.a, t.c, t.e, t.b, t.d, t.f, t.g, t.h, t.i));
                        open.push((b.subtree_end.max(i + 1), None));
                    }
                    if b.backface_hidden && canvas_faces_away(canvas) {
                        // Turned away (a flipped card's other side): neither it nor its content shows.
                        jump = b.subtree_end.max(i + 1);
                        continue;
                    }
                    let shown = styles.get(b.node).visible;
                    if shown {
                        self.paint_box(canvas, b, styles);
                    }
                    if let Some(m) = tree.marker(b.node).filter(|_| shown) {
                        self.paint_marker(canvas, m, m.color.unwrap_or(styles.get(b.node).color));
                    }
                    if let Some(DomCaret::Empty { run, rect }) = dom_caret {
                        if run == i {
                            canvas.draw_rect(Rect::from_xywh(rect.x, rect.y, 1.0, rect.height), &fill(styles.get(b.node).color));
                        }
                    }
                    if let Some(list) = tree.list_box(b.node) {
                        self.paint_list(canvas, b.rect, styles.get(b.node), list);
                    }
                    if let Some(sb) = &b.scroll {
                        let (ox, oy) = offsets.get(&b.node).copied().unwrap_or_default();
                        canvas.save();
                        let st = styles.get(b.node);
                        if st.has_rounding() {
                            // Content follows the rounded padding box: the border radii less the border widths.
                            let [tl, tr, br, bl] = st.corner_radii(b.rect.width, b.rect.height);
                            let bw = st.border_width;
                            let inner = [
                                ((tl.0 - bw.left).max(0.0), (tl.1 - bw.top).max(0.0)),
                                ((tr.0 - bw.right).max(0.0), (tr.1 - bw.top).max(0.0)),
                                ((br.0 - bw.right).max(0.0), (br.1 - bw.bottom).max(0.0)),
                                ((bl.0 - bw.left).max(0.0), (bl.1 - bw.bottom).max(0.0)),
                            ];
                            canvas.clip_rrect(round_rect(skia_rect(sb.clip), inner), skia_safe::ClipOp::Intersect, true);
                        } else {
                            canvas.clip_rect(skia_rect(sb.clip), None, true);
                        }
                        canvas.translate((-ox, -oy));
                        open.push((b.subtree_end, Some(b)));
                    }
                }
                BoxContent::Text(content) => {
                    let accent = styles.palette().accent;
                    let highlight = styles.get(b.node).selection_bg.or(Some(CssColor { r: accent.r, g: accent.g, b: accent.b, a: 90 }));
                    if let Some((_, from, to)) = selection.filter(|(n, ..)| content.owners.iter().any(|(_, o)| o == n)) {
                        paint_selection(canvas, b.rect, content, from, to, highlight);
                    }
                    if let Some(&(_, from, to)) = page_selection.iter().find(|(run, ..)| *run == i) {
                        paint_selection(canvas, b.rect, content, from, to, highlight);
                    }
                    if styles.get(b.node).visible {
                        self.paint_text(canvas, b.rect, content, styles);
                    }
                    if let Some((_, from, to)) = preedit.filter(|(n, ..)| content.control && content.owners.iter().any(|(_, o)| o == n)) {
                        paint_underline(canvas, b.rect, content, from, to, styles.get(b.node).color);
                    }
                    if let Some((_, at)) = caret.filter(|(n, _)| content.owners.iter().any(|(_, o)| o == n)) {
                        paint_caret(canvas, b.rect, content, at, styles.get(b.node).color);
                    }
                    if let Some(DomCaret::Run { run, byte }) = dom_caret {
                        if run == i {
                            paint_caret(canvas, b.rect, content, byte, styles.get(b.node).color);
                        }
                    }
                }
            }
        }
        while let Some((_, container)) = open.pop() {
            canvas.restore();
            if let Some(container) = container {
                paint_scrollbars(canvas, container, offsets, styles, overlay);
            }
        }
    }

    /// The sheets of a paged element: each is a page-sized box drawn with the element's own
    /// background, border and shadow (white if it has none), the element's content for that
    /// sheet clipped inside the margins, and the `@page` margin boxes (headers, footers).
    fn paint_sheets(&mut self, canvas: &Canvas, tree: &LayoutTree, styles: &StyleMap, overlay: &Overlay, base: &skia_safe::Matrix, pb: &nile_layout::PagedBox) {
        use skia_safe::QuickReject;
        let s = styles.get(pb.node);
        let n = pb.count();
        for i in 0..n {
            let top = pb.top(i);
            let paper = LRect { x: pb.rect.x, y: top, width: pb.rect.width, height: pb.page_h };
            if canvas.quick_reject(&skia_rect(paper)) {
                continue;
            }
            let radii = s.corner_radii(paper.width, paper.height);
            paint_shadows(canvas, skia_rect(paper), &s.box_shadow, radii);
            let border = Border { width: s.border_width, color: s.border_color, style: s.border_style };
            paint_decorated(canvas, skia_rect(paper), Some(s.background.unwrap_or(CssColor::rgb(255, 255, 255))), s.background_image.as_ref(), None, &border, radii);
            let (from, _) = pb.band(i);
            // A table that continues from the sheet before shows its header row(s) again at the top.
            let head = pb.head_h(i);
            if let Some(repeat) = pb.repeats.get(i).and_then(Option::as_ref) {
                canvas.save();
                canvas.clip_rect(Rect::from_xywh(paper.x, top + pb.margin[0], paper.width, head), None, true);
                canvas.translate((0.0, top + pb.margin[0] - repeat.y));
                for &(a, b) in &repeat.ranges {
                    self.paint_boxes(canvas, tree, styles, overlay, base, a, b);
                }
                canvas.restore();
            }
            canvas.save();
            canvas.clip_rect(Rect::from_xywh(paper.x, top + pb.margin[0] + head, paper.width, pb.shown_h(i)), None, true);
            canvas.translate((0.0, top + pb.margin[0] + head - from));
            self.paint_boxes(canvas, tree, styles, overlay, base, pb.index + 1, pb.end);
            canvas.restore();
            // A table that goes on to the next sheet shows its footer again at the bottom.
            if let Some(foot) = pb.feet.get(i).and_then(Option::as_ref) {
                let at = top + pb.margin[0] + pb.content_h() - foot.height;
                canvas.save();
                canvas.clip_rect(Rect::from_xywh(paper.x, at, paper.width, foot.height), None, true);
                canvas.translate((0.0, at - foot.y));
                for &(a, b) in &foot.ranges {
                    self.paint_boxes(canvas, tree, styles, overlay, base, a, b);
                }
                canvas.restore();
            }
            self.paint_margin_boxes(canvas, &styles.page_style(i), paper, i, n);
        }
    }

    /// Headers and footers from `@page` margin boxes, on the sheet at `paper`.
    fn paint_margin_boxes(&mut self, canvas: &Canvas, page: &nile_style::PageStyle, paper: LRect, index: usize, count: usize) {
        use nile_style::MarginPlace::*;
        for mb in &page.boxes {
            let text = mb.text(index + 1, count);
            let mut font = Font::default();
            font.set_size(mb.font_size);
            if let Some(tf) = ["Helvetica", "Arial", "DejaVu Sans", "Liberation Sans", "sans-serif"]
                .iter()
                .find_map(|f| self.font_mgr.match_family_style(f, skia_safe::FontStyle::normal()))
                .or_else(|| self.font_mgr.legacy_make_typeface(None, skia_safe::FontStyle::normal()))
            {
                font.set_typeface(tf);
            }
            let (width, _) = font.measure_str(&text, None);
            let (left, right) = (paper.x + page.margin[3], paper.x + paper.width - page.margin[1]);
            let x = match mb.place {
                TopLeft | BottomLeft => left,
                TopCenter | BottomCenter => (left + right - width) / 2.0,
                TopRight | BottomRight => right - width,
            };
            let y = match mb.place {
                TopLeft | TopCenter | TopRight => paper.y + (page.margin[0] + mb.font_size * 0.7) / 2.0,
                _ => paper.y + paper.height - (page.margin[2] - mb.font_size * 0.7) / 2.0,
            };
            let c = mb.color;
            canvas.draw_str(&text, (x, y), &font, &Paint::default().set_color(Color::from_argb(c.a, c.r, c.g, c.b)).clone());
        }
    }

    /// A selected image: an outline and eight square handles.
fn paint_selected_image(canvas: &Canvas, r: LRect) {
    let accent = skia_safe::Color::from_argb(255, 0x2b, 0x6c, 0xd9);
    let mut line = skia_safe::Paint::default();
    line.set_style(skia_safe::PaintStyle::Stroke);
    line.set_stroke_width(2.0);
    line.set_color(accent);
    canvas.draw_rect(Rect::from_xywh(r.x, r.y, r.width, r.height), &line);
    for (_, h) in nile_layout::handles_of(r) {
        let sq = Rect::from_xywh(h.x, h.y, h.width, h.height);
        let mut paint = skia_safe::Paint::default();
        paint.set_color(skia_safe::Color::WHITE);
        canvas.draw_rect(sq, &paint);
        paint.set_style(skia_safe::PaintStyle::Stroke);
        paint.set_stroke_width(1.5);
        paint.set_color(accent);
        canvas.draw_rect(sq, &paint);
    }
}

/// A list item's bullet or number.
    fn paint_marker(&mut self, canvas: &Canvas, m: &nile_layout::ListMarker, color: CssColor) {
        use nile_layout::MarkerKind;
        let r = m.rect;
        let mut paint = fill(color);
        paint.set_anti_alias(true);
        match &m.kind {
            MarkerKind::Disclosure { open } => {
                let mut path = skia_safe::PathBuilder::new();
                if *open {
                    path.move_to((r.x, r.y + r.height * 0.2)).line_to((r.x + r.width, r.y + r.height * 0.2)).line_to((r.x + r.width / 2.0, r.y + r.height * 0.85));
                } else {
                    path.move_to((r.x + r.width * 0.15, r.y)).line_to((r.x + r.width * 0.15, r.y + r.height)).line_to((r.x + r.width * 0.85, r.y + r.height / 2.0));
                }
                path.close();
                canvas.draw_path(&path.detach(), &paint);
            }
            MarkerKind::Disc => {
                canvas.draw_circle((r.x + r.width / 2.0, r.y + r.height / 2.0), r.width / 2.0, &paint);
            }
            MarkerKind::Circle => {
                paint.set_style(skia_safe::PaintStyle::Stroke);
                paint.set_stroke_width(1.0);
                canvas.draw_circle((r.x + r.width / 2.0, r.y + r.height / 2.0), (r.width - 1.0) / 2.0, &paint);
            }
            MarkerKind::Square => {
                canvas.draw_rect(Rect::from_xywh(r.x, r.y, r.width, r.height), &paint);
            }
            MarkerKind::Image(data) => {
                if let Some(img) = self.picture(data, (r.width, r.height), device_scale(canvas)) {
                    canvas.draw_image_rect(&img, None, Rect::from_xywh(r.x, r.y, r.width, r.height), &Paint::default());
                }
            }
            MarkerKind::Text(layout) => {
                self.paint_text_tinted(canvas, LRect { x: r.x, y: r.y, width: r.width, height: r.height }, layout, Some([color.r, color.g, color.b, color.a]));
            }
        }
    }

    /// The rows of a list-box `<select>`, clipped to its padding box.
    fn paint_list(&mut self, canvas: &Canvas, r: LRect, s: &nile_style::ComputedStyle, l: &nile_layout::ListBox) {
        let b = s.border_width;
        canvas.save();
        canvas.clip_rect(Rect::from_ltrb(r.x + b.left, r.y + b.top, r.x + r.width - b.right, r.y + r.height - b.bottom), None, true);
        for (i, item) in l.items.iter().enumerate() {
            let top = r.y + l.inset_top - l.scroll + l.row_height * i as f32;
            if top + l.row_height < r.y || top > r.y + r.height {
                continue;
            }
            if item.selected {
                canvas.draw_rect(Rect::from_xywh(r.x + b.left, top, r.width - b.left - b.right, l.row_height), &fill(self.pal.accent));
            }
            let line_h = item.text.height();
            let at = LRect { x: r.x + b.left + l.pad_x + if item.indent { 12.0 } else { 0.0 }, y: top + (l.row_height - line_h) / 2.0, width: r.width, height: line_h };
            let tint = if item.selected { Some([255, 255, 255, 255]) } else if item.header { Some([70, 70, 70, 255]) } else if item.disabled { Some([150, 150, 150, 255]) } else { Some([s.color.r, s.color.g, s.color.b, s.color.a]) };
            self.paint_text_tinted(canvas, at, &item.text, tint);
        }
        canvas.restore();
        if l.max_scroll > 0.0 {
            let track = l.view_height - 4.0;
            let thumb = (track * l.view_height / (l.view_height + l.max_scroll)).max(16.0).min(track);
            let at = r.y + l.inset_top + 2.0 + (track - thumb) * l.scroll / l.max_scroll;
            canvas.draw_rrect(
                RRect::new_rect_xy(Rect::from_xywh(r.x + r.width - b.right - 7.0, at, 4.0, thumb), 2.0, 2.0),
                &fill(CssColor { r: 0, g: 0, b: 0, a: 110 }),
            );
        }
    }

    /// The open `<select>` list: white panel, highlighted row, a mark on the chosen one.
    fn paint_menu(&mut self, canvas: &Canvas, m: &MenuBox) {
        let r = m.rect;
        let panel = RRect::new_rect_xy(skia_rect(r), 3.0, 3.0);
        canvas.draw_rrect(panel, &fill(self.pal.panel));
        let mut border = fill(self.pal.panel_border);
        border.set_style(skia_safe::PaintStyle::Stroke);
        border.set_stroke_width(1.0);
        canvas.draw_rrect(RRect::new_rect_xy(Rect::from_xywh(r.x + 0.5, r.y + 0.5, r.width - 1.0, r.height - 1.0), 3.0, 3.0), &border);
        canvas.save();
        canvas.clip_rect(Rect::from_xywh(r.x + 1.0, r.y + 1.0, r.width - 2.0, r.height - 2.0), None, false);
        for (i, item) in m.items.iter().enumerate() {
            let top = r.y + 1.0 - m.scroll + m.row_height * i as f32;
            if top + m.row_height < r.y || top > r.y + r.height {
                continue;
            }
            let on = m.highlight == Some(i) && !item.disabled;
            if on {
                canvas.draw_rect(Rect::from_xywh(r.x + 1.0, top, r.width - 2.0, m.row_height), &fill(self.pal.accent));
            }
            let line_h = item.text.height();
            let at = LRect { x: r.x + m.pad_x + if item.indent { 12.0 } else { 0.0 }, y: top + (m.row_height - line_h) / 2.0, width: r.width, height: line_h };
            // Text carries its own color; selected-row and disabled rows are tinted by an overlay.
            self.paint_text_tinted(canvas, at, &item.text, if on { Some([self.pal.accent_text.r, self.pal.accent_text.g, self.pal.accent_text.b, 255]) } else if item.header { Some(if self.pal.dark { [190, 190, 190, 255] } else { [70, 70, 70, 255] }) } else if item.disabled { Some([150, 150, 150, 255]) } else { None });
            if m.selected == Some(i) && !on {
                canvas.draw_rect(Rect::from_xywh(r.x + 2.0, top + 2.0, 2.0, m.row_height - 4.0), &fill(self.pal.accent));
            }
        }
        canvas.restore();
        if m.max_scroll > 0.0 {
            let track = r.height - 4.0;
            let thumb = (track * track / (track + m.max_scroll)).max(16.0).min(track);
            let at = r.y + 2.0 + (track - thumb) * m.scroll / m.max_scroll;
            let mut paint = fill(if self.pal.dark { CssColor { r: 255, g: 255, b: 255, a: 110 } } else { CssColor { r: 0, g: 0, b: 0, a: 110 } });
            paint.set_anti_alias(true);
            canvas.draw_rrect(RRect::new_rect_xy(Rect::from_xywh(r.x + r.width - 6.0, at, 4.0, thumb), 2.0, 2.0), &paint);
        }
    }

    fn paint_box(&mut self, canvas: &Canvas, b: &LayoutBox, styles: &StyleMap) {
        let s = styles.get(b.node);
        let radii = s.corner_radii(b.rect.width, b.rect.height);
        paint_shadows(canvas, skia_rect(b.rect), &s.box_shadow, radii);
        let photo = b.bg_image.as_ref().and_then(|data| self.background_shader(data, s, b.rect, device_scale(canvas)));
        let mut border = Border { width: s.border_width, color: s.border_color, style: s.border_style };
        // A checked box takes the author's `accent-color` (the user-agent sheet paints the system accent).
        let accent_box = s.checked && matches!(s.appearance, Appearance::Checkbox | Appearance::Radio);
        let background = match (accent_box, s.accent_color) {
            (true, Some(a)) => {
                border.color = nile_style::Edges::all(a);
                Some(a)
            }
            _ => s.background,
        };
        // A collapsed-table cell leaves out the sides whose border lost to its neighbour's.
        let h = b.border_hidden;
        if h.top || h.right || h.bottom || h.left {
            for (hidden, style) in [(h.top, &mut border.style.top), (h.right, &mut border.style.right), (h.bottom, &mut border.style.bottom), (h.left, &mut border.style.left)] {
                if hidden {
                    *style = BorderStyle::None;
                }
            }
        }
        if let Some((x0, x1)) = b.border_gap {
            // The legend sits on the top border: clip a notch out of the box's top edge for it.
            canvas.save();
            let notch = Rect::from_ltrb(x0, b.rect.y - 1.0, x1, b.rect.y + s.border_width.top.max(1.0) + 1.0);
            canvas.clip_rect(notch, skia_safe::ClipOp::Difference, false);
            paint_decorated(canvas, skia_rect(b.rect), background, s.background_image.as_ref(), photo, &border, radii);
            canvas.restore();
        } else {
            paint_decorated(canvas, skia_rect(b.rect), background, s.background_image.as_ref(), photo, &border, radii);
        }
        paint_inset_shadows(canvas, skia_rect(b.rect), s, radii);
        if let Some(data) = &b.image {
            self.paint_picture(canvas, data, s, b.rect);
        }
        paint_control_mark(canvas, s, b.rect, &styles.palette());
        paint_outline(canvas, skia_rect(b.rect), s, radii);
    }

    /// The picture for `data` drawn `want` logical pixels big on a surface scaled by `scale`. Bitmaps
    /// are decoded once; an SVG is rasterised at the size it is drawn, so it stays sharp.
    fn picture(&mut self, data: &ImageData, want: (f32, f32), scale: f32) -> Option<skia_safe::Image> {
        // A canvas is drawn from its surface as it is now, never from the cache.
        if let Some(canvas) = data.canvas {
            return nile_canvas::snapshot(canvas);
        }
        if let Some(svg) = &data.svg {
            let (w, h) = ((want.0 * scale).ceil().max(1.0) as u32, (want.1 * scale).ceil().max(1.0) as u32);
            if self.svg_pictures.len() > 64 {
                self.svg_pictures.clear();
            }
            return self
                .svg_pictures
                .entry((data.id, w, h))
                .or_insert_with(|| {
                    let style = nile_svg::Style { css: svg.css.clone() };
                    let raster = nile_svg::rasterize(&data.bytes, &style, w, h, svg.stretch)?;
                    let info = skia_safe::ImageInfo::new((raster.width as i32, raster.height as i32), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Premul, None);
                    skia_safe::images::raster_from_data(&info, skia_safe::Data::new_copy(&raster.pixels), raster.width as usize * 4)
                })
                .clone();
        }
        self.pictures
            .entry(data.id)
            .or_insert_with(|| skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&data.bytes)))
            .clone()
    }

    /// An `<img>`'s picture, stretched over its content box.
    fn paint_picture(&mut self, canvas: &Canvas, data: &ImageData, s: &nile_style::ComputedStyle, r: LRect) {
        let (b, p) = (s.border_width, s.padding);
        let dest = Rect::from_ltrb(r.x + b.left + p.left, r.y + b.top + p.top, r.x + r.width - b.right - p.right, r.y + r.height - b.bottom - p.bottom);
        if dest.width() <= 0.0 || dest.height() <= 0.0 {
            return;
        }
        let Some(img) = self.picture(data, (dest.width(), dest.height()), device_scale(canvas)) else { return };
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        canvas.save();
        if s.has_rounding() {
            // The picture follows the padding edge: each corner's radius less the border and padding there.
            let radii = s.corner_radii(r.width, r.height);
            let inset = [(b.left + p.left, b.top + p.top), (b.right + p.right, b.top + p.top), (b.right + p.right, b.bottom + p.bottom), (b.left + p.left, b.bottom + p.bottom)];
            let shrunk: Vec<skia_safe::Point> = radii.iter().zip(inset).map(|(&(rx, ry), (ix, iy))| skia_safe::Point::new((rx - ix).max(0.0), (ry - iy).max(0.0))).collect();
            canvas.clip_rrect(RRect::new_rect_radii(dest, &[shrunk[0], shrunk[1], shrunk[2], shrunk[3]]), skia_safe::ClipOp::Intersect, true);
        }
        let sampling = skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::Linear);
        canvas.draw_image_rect_with_sampling_options(&img, None, dest, sampling, &paint);
        canvas.restore();
    }

    /// A shader that paints `data` as `background-image: url()` over the box `r`.
    fn background_shader(&mut self, data: &ImageData, s: &nile_style::ComputedStyle, r: LRect, scale: f32) -> Option<skia_safe::Shader> {
        let b = s.border_width;
        // The image is placed in the padding box.
        let (ax, ay) = (r.x + b.left, r.y + b.top);
        let (aw, ah) = (r.width - b.left - b.right, r.height - b.top - b.bottom);
        let (iw, ih) = (data.width as f32, data.height as f32);
        if aw <= 0.0 || ah <= 0.0 || iw <= 0.0 || ih <= 0.0 {
            return None;
        }
        let len = |p: StopPos, whole: f32| match p {
            StopPos::Px(px) => px,
            StopPos::Fraction(f) => f * whole,
        };
        let (tw, th) = match s.background_size {
            BgSize::Auto => (iw, ih),
            BgSize::Cover => {
                let k = (aw / iw).max(ah / ih);
                (iw * k, ih * k)
            }
            BgSize::Contain => {
                let k = (aw / iw).min(ah / ih);
                (iw * k, ih * k)
            }
            BgSize::Explicit(w, h) => match (w.map(|w| len(w, aw)), h.map(|h| len(h, ah))) {
                (Some(w), Some(h)) => (w, h),
                (Some(w), None) => (w, w * ih / iw),
                (None, Some(h)) => (h * iw / ih, h),
                (None, None) => (iw, ih),
            },
        };
        if tw <= 0.0 || th <= 0.0 {
            return None;
        }
        let img = self.picture(data, (tw, th), scale)?;
        // An SVG was rasterised at the tile's size, so the shader scales by the bitmap's own size.
        let (iw, ih) = if data.svg.is_some() { (img.width() as f32, img.height() as f32) } else { (iw, ih) };
        let (px, py) = s.background_position;
        let ox = ax + match px {
            StopPos::Px(v) => v,
            StopPos::Fraction(f) => (aw - tw) * f,
        };
        let oy = ay + match py {
            StopPos::Px(v) => v,
            StopPos::Fraction(f) => (ah - th) * f,
        };
        let m = skia_safe::Matrix::translate((ox, oy)) * skia_safe::Matrix::scale((tw / iw, th / ih));
        let mode = |repeat: bool| if repeat { skia_safe::TileMode::Repeat } else { skia_safe::TileMode::Decal };
        let sampling = skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::Linear);
        img.to_shader((mode(s.background_repeat.0), mode(s.background_repeat.1)), sampling, &m)
    }

    fn paint_text(&mut self, canvas: &Canvas, rect: LRect, content: &TextContent, styles: &StyleMap) {
        self.paint_layout(canvas, rect, &content.layout, Some((content, styles)), None);
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
        content: Option<(&TextContent, &StyleMap)>,
        tint: Option<[u8; 4]>,
    ) {
        let mut glyph_ids: Vec<GlyphId> = Vec::new();
        let mut positions: Vec<Point> = Vec::new();

        // `text-overflow: ellipsis`: where the first line must stop to leave room for "…".
        let ellipsis = content.filter(|(c, _)| c.ellipsis && layout.full_width() > rect.width + 0.5);
        let mut truncated = false;
        for line in layout.lines() {
            if truncated {
                break;
            }
            if let Some((content, _)) = content {
                paint_inline_fragments(canvas, rect, content, &line);
            }
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };
                if truncated {
                    break;
                }
                let run = glyph_run.run();
                let font_data = run.font();
                let Some(typeface) = self.typeface(font_data.data.id(), font_data.index, font_data.data.data())
                else {
                    continue;
                };

                // A variable font drawn at the run's axis positions (`font-variation-settings`, weight, width).
                let coords: Vec<i16> = run.normalized_coords().iter().map(|c| c.to_bits()).collect();
                let typeface = if coords.iter().any(|c| *c != 0) { self.variable_typeface(&typeface, font_data.data.id(), font_data.index, &coords).unwrap_or(typeface) } else { typeface };
                let mut font = Font::from_typeface(typeface, run.font_size());
                font.set_edging(Edging::SubpixelAntiAlias);
                font.set_subpixel(true);
                let synth = run.synthesis();
                font.set_embolden(synth.embolden());
                if let Some(deg) = synth.skew() {
                    font.set_skew_x(-deg.to_radians().tan());
                }

                glyph_ids.clear();
                positions.clear();
                let mut cut: Option<(f32, f32)> = None;
                let dots = font.unichar_to_glyph('\u{2026}' as i32);
                let dots_w = font.measure_str("\u{2026}", None).0;
                for g in glyph_run.positioned_glyphs() {
                    if ellipsis.is_some() && dots != 0 && g.x + g.advance > rect.width - dots_w {
                        cut = Some((g.x, g.y));
                        break;
                    }
                    glyph_ids.push(g.id as GlyphId);
                    positions.push(Point::new(g.x, g.y - glyph_run.style().brush.shift));
                }
                if let Some((x, y)) = cut {
                    glyph_ids.push(dots);
                    positions.push(Point::new(x, y));
                    truncated = true;
                }

                let style = glyph_run.style();
                // The element's current color, so a color change needs no new text layout. Not for
                // form control text (placeholders and such have their own) or generated content.
                let live = content.filter(|(c, s)| !c.control && !s.has_generated()).and_then(|(c, s)| {
                    let at = run.text_range().start;
                    let owner = c.owners.iter().find(|(r, _)| r.contains(&at)).map(|(_, n)| *n)?;
                    let col = s.get(owner).color;
                    Some([col.r, col.g, col.b, col.a])
                });
                let [r, g, b, a] = tint.or(live).unwrap_or(style.brush.color);
                let mut paint = Paint::default();
                paint.set_anti_alias(true);
                paint.set_color(Color::from_argb(a, r, g, b));

                let x = rect.x + glyph_run.offset();
                let baseline = rect.y + glyph_run.baseline() - style.brush.shift;
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

    fn variable_typeface(&mut self, base: &Typeface, blob_id: u64, index: u32, coords: &[i16]) -> Option<Typeface> {
        use skia_safe::font_arguments::{variation_position::Coordinate, VariationPosition};
        self.varied
            .entry((blob_id, index, coords.to_vec()))
            .or_insert_with(|| {
                let axes = base.variation_design_parameters()?;
                // Normalized -1..1 (default at 0) back to each axis's design units.
                let position: Vec<Coordinate> = axes
                    .iter()
                    .zip(coords)
                    .map(|(a, &n)| {
                        let n = f32::from(n) / 16384.0;
                        let value = if n < 0.0 { a.def + n * (a.def - a.min) } else { a.def + n * (a.max - a.def) };
                        Coordinate { axis: a.tag, value }
                    })
                    .collect();
                let args = skia_safe::FontArguments::new().set_variation_design_position(VariationPosition { coordinates: &position });
                let made = base.clone_with_arguments(&args);
                drop(args);
                made
            })
            .clone()
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
/// How many device pixels a logical pixel is on `canvas` now (its transform's scale).
fn device_scale(canvas: &Canvas) -> f32 {
    let m = canvas.local_to_device_as_3x3();
    m.scale_x().hypot(m.skew_y()).max(0.01)
}

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
fn paint_shadows(canvas: &Canvas, outer: Rect, shadows: &[nile_style::BoxShadow], radii: [(f32, f32); 4]) {
    for sh in shadows.iter().rev().filter(|s| !s.inset && s.color.a > 0) {
        let shape = Rect::from_ltrb(
            outer.left - sh.spread + sh.dx,
            outer.top - sh.spread + sh.dy,
            outer.right + sh.spread + sh.dx,
            outer.bottom + sh.spread + sh.dy,
        );
        let grown = radii.map(|(x, y)| skia_safe::Point::new((x + sh.spread).max(0.0), (y + sh.spread).max(0.0)));
        let mut paint = fill(sh.color);
        if sh.blur > 0.0 {
            paint.set_mask_filter(skia_safe::MaskFilter::blur(skia_safe::BlurStyle::Normal, sh.blur / 2.0, false));
        }
        canvas.save();
        canvas.clip_rrect(round_rect(outer, radii), skia_safe::ClipOp::Difference, true);
        canvas.draw_rrect(RRect::new_rect_radii(shape, &grown), &paint);
        canvas.restore();
    }
}

/// `inset` shadows: inside the padding box, a blurred frame around a hole that is the box
/// moved by the offset and shrunk by the spread. The first shadow in the list is on top.
fn paint_inset_shadows(canvas: &Canvas, outer: Rect, s: &nile_style::ComputedStyle, radii: [(f32, f32); 4]) {
    if !s.box_shadow.iter().any(|sh| sh.inset && sh.color.a > 0) {
        return;
    }
    let w = s.border_width;
    let pad = Rect::from_ltrb(outer.left + w.left, outer.top + w.top, outer.right - w.right, outer.bottom - w.bottom);
    if pad.width() <= 0.0 || pad.height() <= 0.0 {
        return;
    }
    let cut = [(w.left, w.top), (w.right, w.top), (w.right, w.bottom), (w.left, w.bottom)];
    let inner: Vec<(f32, f32)> = radii.iter().zip(cut).map(|(&(rx, ry), (cx, cy))| ((rx - cx).max(0.0), (ry - cy).max(0.0))).collect();
    let inner = [inner[0], inner[1], inner[2], inner[3]];
    for sh in s.box_shadow.iter().rev().filter(|sh| sh.inset && sh.color.a > 0) {
        let hole = Rect::from_ltrb(pad.left + sh.spread + sh.dx, pad.top + sh.spread + sh.dy, pad.right - sh.spread + sh.dx, pad.bottom - sh.spread + sh.dy);
        let hole_radii = inner.map(|(x, y)| skia_safe::Point::new((x - sh.spread).max(0.0), (y - sh.spread).max(0.0)));
        // Everything but the hole, painted with an even-odd fill and clipped to the padding box.
        let reach = sh.blur + sh.spread.abs() + sh.dx.abs().max(sh.dy.abs()) + 8.0;
        let mut path = skia_safe::PathBuilder::new();
        path.add_rect(Rect::from_ltrb(pad.left - reach, pad.top - reach, pad.right + reach, pad.bottom + reach), None, None);
        if hole.width() > 0.0 && hole.height() > 0.0 {
            path.add_rrect(RRect::new_rect_radii(hole, &hole_radii), None, None);
        }
        path.set_fill_type(skia_safe::PathFillType::EvenOdd);
        let mut paint = fill(sh.color);
        if sh.blur > 0.0 {
            paint.set_mask_filter(skia_safe::MaskFilter::blur(skia_safe::BlurStyle::Normal, sh.blur / 2.0, false));
        }
        canvas.save();
        canvas.clip_rrect(round_rect(pad, inner), skia_safe::ClipOp::Intersect, true);
        canvas.draw_path(&path.detach(), &paint);
        canvas.restore();
    }
}

/// A box's border: width, color and line style for each side.
struct Border {
    width: Edges<f32>,
    color: Edges<CssColor>,
    style: Edges<BorderStyle>,
}

/// A rounded rectangle with a radius per corner: top left, top right, bottom right, bottom left.
fn round_rect(r: Rect, radii: [(f32, f32); 4]) -> RRect {
    RRect::new_rect_radii(r, &radii.map(|(x, y)| skia_safe::Point::new(x, y)))
}

/// Draw `bg` and a (per-side) border inside `outer`, with rounded corners.
fn paint_decorated(
    canvas: &Canvas,
    outer: Rect,
    bg: Option<CssColor>,
    image: Option<&Gradient>,
    photo: Option<skia_safe::Shader>,
    border: &Border,
    radii: [(f32, f32); 4],
) {
    let (w, c, st) = (border.width, border.color, border.style);
    let outer_r = round_rect(outer, radii);
    if let Some(bg) = bg.filter(|c| c.a > 0) {
        canvas.draw_rrect(outer_r, &fill(bg));
    }
    if let Some(shader) = image.and_then(|g| gradient_shader(g, outer)) {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_shader(shader);
        canvas.draw_rrect(outer_r, &paint);
    }
    if let Some(shader) = photo {
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
        // Inside the border each corner shrinks by the widths of the two sides meeting there.
        let cut = [(w.left, w.top), (w.right, w.top), (w.right, w.bottom), (w.left, w.bottom)];
        let inner_radii = radii.iter().zip(cut).map(|(&(rx, ry), (cx, cy))| skia_safe::Point::new((rx - cx).max(0.0), (ry - cy).max(0.0))).collect::<Vec<_>>();
        let inner = RRect::new_rect_radii(inner_rect, &[inner_radii[0], inner_radii[1], inner_radii[2], inner_radii[3]]);
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

/// `outline`: a line around the border box, `outline-offset` away from it, that takes no space.
fn paint_outline(canvas: &Canvas, rect: Rect, s: &nile_style::ComputedStyle, radii: [(f32, f32); 4]) {
    let o = s.outline;
    if o.style == BorderStyle::None || o.width <= 0.0 {
        return;
    }
    let color = o.color.unwrap_or(s.color);
    if color.a == 0 {
        return;
    }
    // The stroke is centred on a line `offset + width / 2` out from the border box.
    let grow = o.offset + o.width / 2.0;
    let line = Rect::from_ltrb(rect.left - grow, rect.top - grow, rect.right + grow, rect.bottom + grow);
    if line.width() <= 0.0 || line.height() <= 0.0 {
        return;
    }
    let radii = radii.map(|(rx, ry)| ((rx + grow).max(0.0), (ry + grow).max(0.0)));
    let mut paint = fill(color);
    paint.set_style(skia_safe::PaintStyle::Stroke);
    paint.set_stroke_width(o.width);
    paint.set_anti_alias(true);
    match o.style {
        BorderStyle::Dashed => {
            paint.set_path_effect(skia_safe::PathEffect::dash(&[o.width * 3.0, o.width * 3.0], 0.0));
        }
        BorderStyle::Dotted => {
            paint.set_stroke_cap(skia_safe::PaintCap::Round);
            paint.set_path_effect(skia_safe::PathEffect::dash(&[0.01, o.width * 2.0], 0.0));
        }
        _ => {}
    }
    canvas.draw_rrect(round_rect(line, radii), &paint);
}

/// The check or radio dot for `appearance: checkbox | radio` when checked.
fn paint_control_mark(canvas: &Canvas, s: &nile_style::ComputedStyle, r: LRect, pal: &nile_style::Palette) {
    let accent = s.accent_color.unwrap_or(pal.accent);
    let glyph = if pal.dark { 190 } else { 90 };
    // Checkbox and radio marks show only when checked; a select's chevron always does.
    if !s.checked && !matches!(s.appearance, Appearance::Select | Appearance::Progress | Appearance::Meter | Appearance::Range | Appearance::Spinner | Appearance::Swatch | Appearance::Calendar | Appearance::Clock) {
        return;
    }
    let mut paint = fill(match s.accent_color {
        Some(a) if s.checked && matches!(s.appearance, Appearance::Checkbox | Appearance::Radio) => nile_style::text_on(a),
        _ => s.color,
    });
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
        Appearance::Progress | Appearance::Meter => {
            // A rounded trough with a filled part.
            let bar = Rect::from_xywh(r.x, r.y + r.height * 0.15, r.width, r.height * 0.7);
            let radius = bar.height() / 2.0;
            let mut trough = fill(pal.trough);
            trough.set_anti_alias(true);
            canvas.draw_rrect(RRect::new_rect_xy(bar, radius, radius), &trough);
            if let Some(f) = s.control_fraction {
                let color = if s.appearance == Appearance::Meter {
                    if s.meter_warn { CssColor::rgb(245, 158, 11) } else { CssColor::rgb(22, 163, 74) }
                } else {
                    accent
                };
                let mut paint = fill(color);
                paint.set_anti_alias(true);
                canvas.save();
                canvas.clip_rrect(RRect::new_rect_xy(bar, radius, radius), skia_safe::ClipOp::Intersect, true);
                canvas.draw_rect(Rect::from_xywh(bar.left, bar.top, bar.width() * f.clamp(0.0, 1.0), bar.height()), &paint);
                canvas.restore();
            }
        }
        Appearance::Swatch => {
            let c = s.swatch.unwrap_or(CssColor::BLACK);
            let chip = Rect::from_xywh(r.x + 6.0, r.y + r.height / 2.0 - 7.0, 18.0, 14.0);
            let mut p = Paint::default();
            p.set_anti_alias(true);
            p.set_color(Color::from_argb(255, c.r, c.g, c.b));
            canvas.draw_rrect(RRect::new_rect_xy(chip, 2.0, 2.0), &p);
            p.set_style(skia_safe::PaintStyle::Stroke);
            p.set_stroke_width(1.0);
            p.set_color(Color::from_argb(255, 118, 118, 118));
            canvas.draw_rrect(RRect::new_rect_xy(chip, 2.0, 2.0), &p);
        }
        Appearance::Calendar | Appearance::Clock => {
            // A small glyph in the right padding: the opener of the date/time picker.
            let (cx, cy) = (r.x + r.width - 13.0, r.y + r.height / 2.0);
            paint.set_anti_alias(true);
            paint.set_style(skia_safe::PaintStyle::Stroke);
            paint.set_stroke_width(1.3);
            paint.set_color(Color::from_argb(255, glyph, glyph, glyph));
            if s.appearance == Appearance::Calendar {
                let body = Rect::from_xywh(cx - 6.0, cy - 5.0, 12.0, 11.0);
                canvas.draw_rrect(RRect::new_rect_xy(body, 1.5, 1.5), &paint);
                canvas.draw_line((cx - 6.0, cy - 1.5), (cx + 6.0, cy - 1.5), &paint);
                canvas.draw_line((cx - 3.0, cy - 7.0), (cx - 3.0, cy - 4.0), &paint);
                canvas.draw_line((cx + 3.0, cy - 7.0), (cx + 3.0, cy - 4.0), &paint);
            } else {
                canvas.draw_circle((cx, cy), 6.0, &paint);
                canvas.draw_line((cx, cy), (cx, cy - 3.5), &paint);
                canvas.draw_line((cx, cy), (cx + 3.0, cy + 1.5), &paint);
            }
        }
        Appearance::Spinner => {
            // Up and down arrows stacked in the right padding.
            let cx = r.x + r.width - 11.0;
            let mid = r.y + r.height / 2.0;
            paint.set_anti_alias(true);
            paint.set_color(Color::from_argb(255, glyph, glyph, glyph));
            for (dy, dir) in [(-4.0, -1.0), (4.0, 1.0)] {
                let mut builder = skia_safe::PathBuilder::new();
                builder.move_to((cx - 3.5, mid + dy - 1.5 * dir)).line_to((cx + 3.5, mid + dy - 1.5 * dir)).line_to((cx, mid + dy + 2.0 * dir)).close();
                canvas.draw_path(&builder.detach(), &paint);
            }
        }
        Appearance::Range => {
            let f = s.control_fraction.unwrap_or(0.5).clamp(0.0, 1.0);
            let radius = (r.height / 2.0).min(8.0);
            let x = r.x + radius + (r.width - 2.0 * radius) * f;
            let cy = r.y + r.height / 2.0;
            let track = Rect::from_xywh(r.x + radius, cy - 2.0, r.width - 2.0 * radius, 4.0);
            let mut p = fill(if pal.dark { CssColor::rgb(85, 85, 85) } else { CssColor::rgb(214, 214, 214) });
            p.set_anti_alias(true);
            canvas.draw_rrect(RRect::new_rect_xy(track, 2.0, 2.0), &p);
            let mut filled = fill(accent);
            filled.set_anti_alias(true);
            canvas.draw_rrect(RRect::new_rect_xy(Rect::from_ltrb(track.left, track.top, x, track.bottom), 2.0, 2.0), &filled);
            let mut thumb = fill(CssColor::rgb(255, 255, 255));
            thumb.set_anti_alias(true);
            canvas.draw_circle((x, cy), radius - 1.0, &thumb);
            thumb.set_style(skia_safe::PaintStyle::Stroke);
            thumb.set_stroke_width(1.5);
            thumb.set_color(Color::from_argb(255, accent.r, accent.g, accent.b));
            canvas.draw_circle((x, cy), radius - 1.0, &thumb);
        }
        Appearance::None => {}
    }
}

/// Whether the canvas's current matrix shows the back of a surface (it flips orientation).
fn canvas_faces_away(canvas: &Canvas) -> bool {
    let m = canvas.local_to_device_as_3x3();
    nile_layout::Affine { a: m[0], c: m[1], e: m[2], b: m[3], d: m[4], f: m[5], g: m[6], h: m[7], i: m[8] }.faces_away()
}

/// Thin overlay scrollbars for an `overflow: auto | scroll` box that has more to show.
fn paint_scrollbars(canvas: &Canvas, b: &LayoutBox, offsets: &ScrollOffsets, styles: &StyleMap, overlay: &Overlay) {
    let Some(sb) = &b.scroll else { return };
    let (ox, oy) = offsets.get(&b.node).copied().unwrap_or_default();
    let look = styles.scrollbar_look(b.node);
    for t in sb.thumbs(ox, oy) {
        paint_thumb(canvas, &t, &look, bar_state(overlay, Some(b.node), t.vertical), styles.dark);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum BarState {
    Rest,
    Hover,
    Active,
}

fn bar_state(overlay: &Overlay, node: Option<NodeId>, vertical: bool) -> BarState {
    match overlay.bar {
        Some((n, v, active)) if n == node && v == vertical => if active { BarState::Active } else { BarState::Hover },
        _ => BarState::Rest,
    }
}

/// A scrollbar: while the pointer is over it (or it is dragged) it grows toward the content, shows its
/// track and darkens the thumb.
fn paint_thumb(canvas: &Canvas, t: &nile_layout::Thumb, look: &nile_style::ScrollbarLook, state: BarState, dark: bool) {
    let grow = if state == BarState::Rest { 0.0 } else { nile_style::scrollbar_grow() };
    let widen = |r: nile_layout::Rect| {
        if t.vertical {
            Rect::from_xywh(r.x - grow, r.y, r.width + grow, r.height)
        } else {
            Rect::from_xywh(r.x, r.y - grow, r.width, r.height + grow)
        }
    };
    let thick = if t.vertical { t.rect.width } else { t.rect.height } + grow;
    let radius = look.thumb_radius.unwrap_or(thick / 2.0);
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    if let Some(track) = look.track.filter(|c| c.a > 0).or_else(|| (state != BarState::Rest).then_some(if dark { CssColor { r: 255, g: 255, b: 255, a: 22 } } else { CssColor { r: 0, g: 0, b: 0, a: 18 } })) {
        paint.set_color(Color::from_argb(track.a, track.r, track.g, track.b));
        canvas.draw_round_rect(widen(t.track), thick / 2.0, thick / 2.0, &paint);
    }
    let (r, g, b, a) = match look.thumb {
        Some(c) => {
            // A chosen colour darkens a little when hovered or dragged.
            let k = match state {
                BarState::Rest => 1.0,
                BarState::Hover => 0.85,
                BarState::Active => 0.7,
            };
            ((f32::from(c.r) * k) as u8, (f32::from(c.g) * k) as u8, (f32::from(c.b) * k) as u8, c.a)
        }
        None => (if dark { 255 } else { 0 }, if dark { 255 } else { 0 }, if dark { 255 } else { 0 }, match state {
            BarState::Rest => 110,
            BarState::Hover => 150,
            BarState::Active => 190,
        }),
    };
    paint.set_color(Color::from_argb(a, r, g, b));
    let rect = widen(t.rect);
    canvas.draw_round_rect(rect, radius, radius, &paint);
}

/// Highlight bytes `from..to` of a text run.
fn paint_selection(canvas: &Canvas, origin: LRect, content: &TextContent, from: usize, to: usize, color: Option<CssColor>) {
    let layout = &content.layout;
    let sel = Selection::new(
        Cursor::from_byte_index(layout, from, Affinity::Downstream),
        Cursor::from_byte_index(layout, to, Affinity::Upstream),
    );
    let mut paint = Paint::default();
    match color {
        Some(c) => paint.set_color(Color::from_argb(c.a, c.r, c.g, c.b)),
        None => paint.set_color(Color::from_argb(90, 59, 130, 246)),
    };
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

/// A line under the bytes `from..to` of a text run (the text an input method is composing).
fn paint_underline(canvas: &Canvas, origin: LRect, content: &TextContent, from: usize, to: usize, color: CssColor) {
    let sel = Selection::new(Cursor::from_byte_index(&content.layout, from, Affinity::Downstream), Cursor::from_byte_index(&content.layout, to, Affinity::Upstream));
    for (r, _) in sel.geometry(&content.layout) {
        let y = origin.y + r.y1 as f32 - 1.0;
        canvas.draw_rect(Rect::from_ltrb(origin.x + r.x0 as f32, y, origin.x + r.x1 as f32, y + 1.0), &fill(color));
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
        paint_decorated(canvas, rect, d.background, None, None, &Border { width: d.border, color: d.border_color, style: d.border_style }, [(d.radius, d.radius); 4]);
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
