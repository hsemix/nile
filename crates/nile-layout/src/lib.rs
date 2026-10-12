//! Box layout: maps the styled DOM onto a Taffy tree, measures text with
//! Parley, and flattens the result into absolutely positioned boxes.
//!
//! Inline formatting: a block's consecutive inline children (text, `span`,
//! `b`, `a`, ...) are gathered into one anonymous leaf holding a single Parley
//! layout, with each text node's style applied as a span.
//! `inline-block` elements are laid out on their own (shrink-to-fit, via a
//! nested Taffy pass) and placed in the flow with Parley inline boxes.
//! An inline element with a background, border or padding gets a [`InlineDeco`];
//! its horizontal padding+border become no-break-space "spacers" (sized by font
//! size) at both ends of its text, and the renderer paints one fragment per line.
//! TODO: shrink-to-fit against the available width, replaced elements, inline
//! margins, border/radius clipping at line breaks, and nested decorated inlines
//! (only the innermost is painted).

use std::ops::Range;

pub use nile_dom::NodeId;
use nile_dom::{Document, NodeKind};
use nile_style::{
    Align, Color, ComputedStyle, Display, Edges, FlexDirection, Length, OverflowKind, StyleMap, TextAlign,
};
use nile_text::{
    break_lines, collapse_whitespace, Affinity, Alignment, Cluster, Cursor, InlineBoxSpec, PositionedLayoutItem, TextEngine,
    TextLayout, TextStyle,
};
use taffy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub enum BoxContent {
    /// An element box. Padding/border are read from its style when painting.
    Element,
    Text(Box<TextContent>),
}

/// Paint data for an inline element's fragments inside a text run.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineDeco {
    pub background: Option<Color>,
    pub border_color: Color,
    pub border: Edges<f32>,
    pub padding: Edges<f32>,
    pub radius: f32,
}

pub struct TextContent {
    pub layout: TextLayout,
    /// Indexed by `TextBrush::deco - 1`.
    pub decos: Vec<InlineDeco>,
    /// Which element each byte range of the run's text belongs to (for hit testing).
    pub owners: Vec<(Range<usize>, NodeId)>,
}

pub struct LayoutBox {
    pub node: NodeId,
    /// Border box in absolute document coordinates (logical pixels).
    pub rect: Rect,
    pub content: BoxContent,
    /// Index in `LayoutTree::boxes` just past this box's last descendant, so
    /// painting can scope a clip to the subtree.
    pub subtree_end: usize,
    /// Set for elements with non-visible `overflow` that have content to clip.
    pub scroll: Option<ScrollBox>,
}

/// Clipping and scroll extent of an `overflow` box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollBox {
    /// The padding box (where content is clipped), in unscrolled document coordinates.
    pub clip: Rect,
    /// The wheel may scroll this axis (`overflow: auto | scroll`).
    pub wheel_x: bool,
    pub wheel_y: bool,
    /// Size of the scrollable content from the padding box's origin (`scrollWidth/Height`).
    pub content_width: f32,
    pub content_height: f32,
}

impl ScrollBox {
    /// Largest scroll offsets (x, y).
    pub fn max_offset(&self) -> (f32, f32) {
        ((self.content_width - self.clip.width).max(0.0), (self.content_height - self.clip.height).max(0.0))
    }
}

/// Scroll offsets of `overflow` elements, in CSS px.
pub type ScrollOffsets = std::collections::HashMap<NodeId, (f32, f32)>;

/// Where a box actually paints: its rect shifted by the scroll offsets of the
/// containers around it, and the clip those containers impose.
#[derive(Debug, Clone, Copy, Default)]
pub struct Placement {
    pub dx: f32,
    pub dy: f32,
    pub clip: Option<Rect>,
}

fn intersect(a: Option<Rect>, b: Rect) -> Rect {
    let Some(a) = a else { return b };
    let (l, t) = (a.x.max(b.x), a.y.max(b.y));
    let (r, bt) = ((a.x + a.width).min(b.x + b.width), (a.y + a.height).min(b.y + b.height));
    Rect { x: l, y: t, width: (r - l).max(0.0), height: (bt - t).max(0.0) }
}

/// Boxes in paint order (pre-order DOM traversal).
pub struct LayoutTree {
    pub boxes: Vec<LayoutBox>,
    pub content_height: f32,
    /// Element boxes by node, for `node_rect`.
    index: std::collections::HashMap<NodeId, usize>,
}

/// An `inline-block` inside a text run: its Taffy subtree (sized already, not
/// attached to the tree) and the margin offset to its border box.
#[derive(Clone, Copy)]
struct InlineKid {
    node: taffy::NodeId,
    dx: f32,
    dy: f32,
}

impl LayoutTree {
    /// Find `overflow` boxes and measure what they hold.
    fn compute_scroll_boxes(&mut self, styles: &StyleMap) {
        let n = self.boxes.len();
        let clips: Vec<bool> = self
            .boxes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                matches!(b.content, BoxContent::Element) && styles.get(b.node).clips() && b.subtree_end > i + 1
            })
            .collect();
        for i in 0..n {
            if !clips[i] {
                continue;
            }
            let b = &self.boxes[i];
            let s = styles.get(b.node);
            let w = s.border_width;
            let clip = Rect {
                x: b.rect.x + w.left,
                y: b.rect.y + w.top,
                width: (b.rect.width - w.left - w.right).max(0.0),
                height: (b.rect.height - w.top - w.bottom).max(0.0),
            };
            // Extent of the contents. A nested `overflow` box counts as its own
            // rect only: what it clips doesn't make this one scrollable.
            let (mut right, mut bottom) = (clip.x + clip.width, clip.y + clip.height);
            let mut j = i + 1;
            while j < b.subtree_end {
                let c = &self.boxes[j];
                let (cr, cb) = match &c.content {
                    BoxContent::Text(t) => (
                        c.rect.x + c.rect.width.max(t.layout.full_width()),
                        c.rect.y + c.rect.height.max(t.layout.height()),
                    ),
                    BoxContent::Element => (c.rect.x + c.rect.width, c.rect.y + c.rect.height),
                };
                right = right.max(cr);
                bottom = bottom.max(cb);
                j = if clips[j] { c.subtree_end } else { j + 1 };
            }
            let scroll = ScrollBox {
                clip,
                wheel_x: s.overflow_x == OverflowKind::Scroll,
                wheel_y: s.overflow_y == OverflowKind::Scroll,
                content_width: right - clip.x,
                content_height: bottom - clip.y,
            };
            self.boxes[i].scroll = Some(scroll);
        }
    }

    /// The scroll box of element `node`, if it clips content.
    pub fn scroll_box(&self, node: NodeId) -> Option<&ScrollBox> {
        self.index.get(&node).and_then(|&i| self.boxes[i].scroll.as_ref())
    }

    /// Where every box paints under `offsets` (parallel to `boxes`).
    pub fn placements(&self, offsets: &ScrollOffsets) -> Vec<Placement> {
        let mut out = Vec::with_capacity(self.boxes.len());
        let mut stack: Vec<(usize, Placement)> = Vec::new();
        for (i, b) in self.boxes.iter().enumerate() {
            while stack.last().is_some_and(|(end, _)| i >= *end) {
                stack.pop();
            }
            let cur = stack.last().map_or(Placement::default(), |(_, p)| *p);
            out.push(cur);
            if let Some(sb) = &b.scroll {
                let (ox, oy) = offsets.get(&b.node).copied().unwrap_or_default();
                let shifted = Rect { x: sb.clip.x + cur.dx, y: sb.clip.y + cur.dy, ..sb.clip };
                stack.push((b.subtree_end, Placement { dx: cur.dx - ox, dy: cur.dy - oy, clip: Some(intersect(cur.clip, shifted)) }));
            }
        }
        out
    }

    /// Byte offset into the text of `node` (an input) nearest to a point in
    /// document coordinates, for placing a caret by clicking.
    pub fn text_index_at(&self, node: NodeId, x: f32, y: f32) -> Option<usize> {
        self.text_index_at_scrolled(node, x, y, &ScrollOffsets::new())
    }

    /// [`LayoutTree::text_index_at`] where the point is in painted (scrolled) coordinates.
    pub fn text_index_at_scrolled(&self, node: NodeId, x: f32, y: f32, offsets: &ScrollOffsets) -> Option<usize> {
        let (i, b) = self.text_box_of(node)?;
        let BoxContent::Text(c) = &b.content else { return None };
        let p = self.placements(offsets)[i];
        Some(Cursor::from_point(&c.layout, x - p.dx - b.rect.x, y - p.dy - b.rect.y).index())
    }

    fn text_box_of(&self, node: NodeId) -> Option<(usize, &LayoutBox)> {
        self.boxes.iter().enumerate().find(|(_, b)| match &b.content {
            BoxContent::Text(c) => c.owners.iter().any(|(_, n)| *n == node),
            _ => false,
        })
    }

    /// The caret rectangle at byte offset `at` of `node`'s text, in unscrolled
    /// document coordinates (used to scroll it into view).
    pub fn caret_rect(&self, node: NodeId, at: usize) -> Option<Rect> {
        let (_, b) = self.text_box_of(node)?;
        let BoxContent::Text(c) = &b.content else { return None };
        let g = Cursor::from_byte_index(&c.layout, at, Affinity::Downstream).geometry(&c.layout, 1.0);
        Some(Rect {
            x: b.rect.x + g.x0 as f32,
            y: b.rect.y + g.y0 as f32,
            width: (g.x1 - g.x0) as f32,
            height: (g.y1 - g.y0) as f32,
        })
    }

    /// The caret one line up (`dir < 0`) or down from byte offset `at` of
    /// `node`'s text, keeping its x position. `None` if there is no such line.
    pub fn move_caret_vertically(&self, node: NodeId, at: usize, dir: i32) -> Option<usize> {
        let b = self.boxes.iter().find(|b| match &b.content {
            BoxContent::Text(c) => c.owners.iter().any(|(_, n)| *n == node),
            _ => false,
        })?;
        let BoxContent::Text(c) = &b.content else { return None };
        let g = Cursor::from_byte_index(&c.layout, at, Affinity::Downstream).geometry(&c.layout, 1.0);
        let line = (g.y1 - g.y0) as f32;
        let y = if dir < 0 { g.y0 as f32 - line / 2.0 } else { g.y1 as f32 + line / 2.0 };
        if y < 0.0 || y > c.layout.height() {
            return None;
        }
        Some(Cursor::from_point(&c.layout, g.x0 as f32, y).index())
    }

    /// Border box of `node` in document coordinates. Block-level elements have
    /// a box; an inline element's rect is the union of its text runs (without
    /// padding). `None` if it generated nothing (e.g. `display: none`, or empty).
    pub fn node_rect(&self, node: NodeId) -> Option<Rect> {
        self.node_rect_with(node, None)
    }

    /// [`LayoutTree::node_rect`] where the element paints: shifted by the scroll
    /// offsets of the containers around it.
    pub fn node_rect_scrolled(&self, node: NodeId, offsets: &ScrollOffsets) -> Option<Rect> {
        self.node_rect_with(node, Some(&self.placements(offsets)))
    }

    fn node_rect_with(&self, node: NodeId, places: Option<&[Placement]>) -> Option<Rect> {
        let shift = |i: usize| places.map_or((0.0, 0.0), |p| (p[i].dx, p[i].dy));
        if let Some(&i) = self.index.get(&node) {
            let (dx, dy) = shift(i);
            let r = self.boxes[i].rect;
            return Some(Rect { x: r.x + dx, y: r.y + dy, ..r });
        }
        let (mut l, mut t, mut r, mut b) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for (ti, tb) in self.boxes.iter().enumerate() {
            let BoxContent::Text(c) = &tb.content else { continue };
            let (dx, dy) = shift(ti);
            let ranges: Vec<_> = c.owners.iter().filter(|(_, n)| *n == node).map(|(r, _)| r.clone()).collect();
            if ranges.is_empty() {
                continue;
            }
            for line in c.layout.lines() {
                for item in line.items() {
                    let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
                    if gr.style().brush.spacer {
                        continue;
                    }
                    let tr = gr.run().text_range();
                    if !ranges.iter().any(|o| o.start < tr.end && tr.start < o.end) {
                        continue;
                    }
                    let fm = gr.run().font_metrics();
                    l = l.min(tb.rect.x + dx + gr.offset());
                    r = r.max(tb.rect.x + dx + gr.offset() + gr.advance());
                    t = t.min(tb.rect.y + dy + gr.baseline() - fm.ascent);
                    b = b.max(tb.rect.y + dy + gr.baseline() + fm.descent);
                }
            }
        }
        (l <= r).then_some(Rect { x: l, y: t, width: r - l, height: b - t })
    }

    /// The innermost element under `(x, y)` in document coordinates: the
    /// topmost element box, or the inline element owning the text there.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        self.hit_test_scrolled(x, y, &ScrollOffsets::new())
    }

    /// [`LayoutTree::hit_test`] with scrolled containers: boxes are tested where
    /// they paint, and a container's clip hides what it has scrolled out of view.
    pub fn hit_test_scrolled(&self, x: f32, y: f32, offsets: &ScrollOffsets) -> Option<NodeId> {
        let places = self.placements(offsets);
        for (b, p) in self.boxes.iter().zip(&places).rev() {
            if let Some(c) = p.clip {
                if x < c.x || y < c.y || x >= c.x + c.width || y >= c.y + c.height {
                    continue;
                }
            }
            let r = Rect { x: b.rect.x + p.dx, y: b.rect.y + p.dy, ..b.rect };
            if x < r.x || y < r.y || x >= r.x + r.width || y >= r.y + r.height {
                continue;
            }
            match &b.content {
                BoxContent::Element => return Some(b.node),
                BoxContent::Text(c) => {
                    let Some((cluster, _)) = Cluster::from_point_exact(&c.layout, x - r.x, y - r.y) else {
                        continue;
                    };
                    let at = cluster.text_range().start;
                    if let Some((_, owner)) = c.owners.iter().find(|(range, _)| range.contains(&at)) {
                        return Some(*owner);
                    }
                }
            }
        }
        None
    }
}

enum NodeCtx {
    Element(NodeId),
    Text {
        node: NodeId,
        layout: Box<TextLayout>,
        align: Alignment,
        kids: Vec<InlineKid>,
        decos: Vec<InlineDeco>,
        owners: Vec<(Range<usize>, NodeId)>,
        /// Never wrap (single-line text inputs).
        nowrap: bool,
    },
}

fn measure_node(
    inputs: taffy::LayoutInput,
    ctx: Option<&mut NodeCtx>,
    style: &Style,
) -> taffy::LayoutOutput {
    taffy::compute_leaf_layout(inputs, style, |_, _| 0.0, |known, available| match ctx {
        Some(NodeCtx::Text { layout, align, nowrap, .. }) => {
            measure_text(layout, *align, known, available, *nowrap)
        }
        _ => Size::ZERO,
    })
}

pub fn layout(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    viewport_width: f32,
    viewport_height: f32,
) -> LayoutTree {
    let mut tree: TaffyTree<NodeCtx> = TaffyTree::new();

    let html = doc.find(|n| n.tag() == Some("html")).unwrap_or(Document::ROOT);
    let root = build(doc, styles, text, &mut tree, html).unwrap_or_else(|| {
        tree.new_leaf_with_context(Style::default(), NodeCtx::Element(html)).unwrap()
    });

    // The root fills the viewport, like a browser's initial containing block.
    let mut root_style = tree.style(root).unwrap().clone();
    root_style.size.width = Dimension::length(viewport_width);
    root_style.min_size.height = LengthPercentageAuto::length(viewport_height);
    tree.set_style(root, root_style).unwrap();

    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(viewport_width), height: AvailableSpace::MaxContent },
        |inputs, _id, ctx, style| measure_node(inputs, ctx, style),
    )
    .expect("layout of a well-formed tree cannot fail");

    let mut boxes = Vec::new();
    flatten(&mut tree, root, 0.0, 0.0, &mut boxes);
    let content_height = tree.layout(root).map(|l| l.size.height).unwrap_or(0.0);
    let index = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b.content, BoxContent::Element))
        .map(|(i, b)| (b.node, i))
        .collect();
    let mut tree = LayoutTree { boxes, content_height, index };
    tree.compute_scroll_boxes(styles);
    tree
}

fn measure_text(
    layout: &mut TextLayout,
    align: Alignment,
    known: Size<Option<f32>>,
    available: Size<AvailableSpace>,
    nowrap: bool,
) -> Size<f32> {
    let max_width = known.width.or(match available.width {
        AvailableSpace::Definite(w) => Some(w),
        AvailableSpace::MinContent => Some(0.0),
        AvailableSpace::MaxContent => None,
    });
    let max_width = if nowrap { None } else { max_width };
    break_lines(layout, max_width, align);
    Size {
        width: known.width.unwrap_or_else(|| layout.width()),
        height: known.height.unwrap_or_else(|| layout.height()),
    }
}

fn build(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &mut TaffyTree<NodeCtx>,
    id: NodeId,
) -> Option<taffy::NodeId> {
    let node = doc.node(id);
    let style = styles.get(id);

    match &node.kind {
        // Text outside any block container (e.g. directly under the document).
        NodeKind::Text(_) => build_inline_run(doc, styles, text, tree, &[id], styles.get(id)),
        NodeKind::Element { .. } | NodeKind::Document => {
            if style.display == Display::None {
                return None;
            }
            let children = if is_value_control(node) {
                build_control_text(doc, styles, text, tree, id).into_iter().collect()
            } else {
                build_children(doc, styles, text, tree, node.children.as_slice(), style)
            };
            let n = tree.new_with_children(taffy_style(style), &children).ok()?;
            tree.set_node_context(n, Some(NodeCtx::Element(id))).ok()?;
            Some(n)
        }
    }
}

/// `<input>` types whose `value` is drawn as a line of text.
fn is_value_control(node: &nile_dom::Node) -> bool {
    node.tag() == Some("textarea")
        || node.tag() == Some("input")
        && !matches!(node.attr("type").map(str::to_ascii_lowercase).as_deref(), Some("checkbox" | "radio" | "hidden" | "file" | "image"))
}

fn is_password(node: &nile_dom::Node) -> bool {
    node.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("password"))
}

/// The current value of a form control: the `value` attribute, or for a
/// `<textarea>` without one its initial text content.
pub fn control_value(doc: &Document, id: NodeId) -> String {
    let n = doc.node(id);
    match n.attr("value") {
        Some(v) => v.to_string(),
        None if n.tag() == Some("textarea") => doc.text_content(id),
        None => String::new(),
    }
}

/// The text shown for a control: its value (bullets for passwords), else the
/// placeholder. A zero-width space stands in for empty text (and ends a value
/// that ends in a newline) so the last line keeps its height and the caret has
/// a place to sit.
fn control_display(doc: &Document, id: NodeId) -> (String, bool) {
    let node = doc.node(id);
    let multiline = node.tag() == Some("textarea");
    let value = control_value(doc, id);
    if value.is_empty() {
        let ph = node.attr("placeholder").unwrap_or("");
        let ph = if multiline { ph.to_string() } else { ph.replace('\n', " ") };
        let is_placeholder = !ph.is_empty();
        return (if is_placeholder { ph } else { "\u{200b}".into() }, is_placeholder);
    }
    if is_password(node) {
        ("\u{2022}".repeat(value.chars().count()), false)
    } else if multiline {
        let tail = if value.ends_with('\n') { "\u{200b}" } else { "" };
        (format!("{value}{tail}"), false)
    } else {
        (value.replace('\n', " "), false)
    }
}

/// Byte offset in an input's displayed text of character `char_idx` of its value.
pub fn caret_byte(doc: &Document, node: NodeId, char_idx: usize) -> usize {
    let n = doc.node(node);
    let value = control_value(doc, node);
    if is_password(n) {
        return char_idx.min(value.chars().count()) * '\u{2022}'.len_utf8();
    }
    value.char_indices().nth(char_idx).map_or(value.len(), |(i, _)| i)
}

/// Inverse of [`caret_byte`].
pub fn caret_char(doc: &Document, node: NodeId, display_byte: usize) -> usize {
    let n = doc.node(node);
    let value = control_value(doc, node);
    if is_password(n) {
        return (display_byte / '\u{2022}'.len_utf8()).min(value.chars().count());
    }
    value.char_indices().take_while(|(i, _)| *i < display_byte).count()
}

/// The single text leaf inside an input.
fn build_control_text(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &mut TaffyTree<NodeCtx>,
    id: NodeId,
) -> Option<taffy::NodeId> {
    let node = doc.node(id);
    let style = styles.get(id);
    let (display, is_placeholder) = control_display(doc, id);
    let mut ts = text_style(style);
    if is_placeholder {
        ts.color = [117, 117, 117, 255];
    }
    let shaped = text.shape(&display, &ts, 1.0);
    let owners = vec![(0..display.len(), id)];
    tree.new_leaf_with_context(
        Style::default(),
        NodeCtx::Text {
            node: id,
            layout: Box::new(shaped),
            align: alignment(style.text_align),
            kids: Vec::new(),
            decos: Vec::new(),
            owners,
            nowrap: node.tag() != Some("textarea"),
        },
    )
    .ok()
}

/// Build a container's children. In a block container, consecutive inline
/// children share one inline run; in a flex container every child is its own
/// item (inline elements are blockified, text becomes an anonymous item).
fn build_children(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &mut TaffyTree<NodeCtx>,
    children: &[NodeId],
    parent_style: &ComputedStyle,
) -> Vec<taffy::NodeId> {
    let mut out = Vec::new();
    let mut run: Vec<NodeId> = Vec::new();
    let flush = |run: &mut Vec<NodeId>,
                 out: &mut Vec<taffy::NodeId>,
                 text: &mut TextEngine,
                 tree: &mut TaffyTree<NodeCtx>| {
        if !run.is_empty() {
            out.extend(build_inline_run(doc, styles, text, tree, run, parent_style));
            run.clear();
        }
    };
    for &c in children {
        let node = doc.node(c);
        let is_inline = match &node.kind {
            NodeKind::Text(_) => true,
            NodeKind::Element { .. } => {
                matches!(styles.get(c).display, Display::Inline | Display::InlineBlock)
            }
            NodeKind::Document => false,
        };
        if is_inline && parent_style.display != Display::Flex {
            run.push(c);
        } else {
            flush(&mut run, &mut out, text, tree);
            if matches!(node.kind, NodeKind::Text(_)) {
                out.extend(build_inline_run(doc, styles, text, tree, &[c], parent_style));
            } else {
                out.extend(build(doc, styles, text, tree, c));
            }
        }
    }
    flush(&mut run, &mut out, text, tree);
    out
}

struct InlineText {
    text: String,
    spans: Vec<(Range<usize>, TextStyle)>,
    boxes: Vec<InlineBoxSpec>,
    decos: Vec<InlineDeco>,
    owners: Vec<(Range<usize>, NodeId)>,
    /// Inline elements currently being collected, outermost first.
    open: Vec<NodeId>,
    /// True where a following space would be collapsed away.
    collapse: bool,
}

/// Builds and sizes an inline-block, returning its Parley box spec (at index 0).
type BoxMaker<'a> = dyn FnMut(NodeId) -> Option<InlineBoxSpec> + 'a;

fn has_inline_paint(s: &ComputedStyle) -> bool {
    let e = |e: Edges<f32>| e.top > 0.0 || e.right > 0.0 || e.bottom > 0.0 || e.left > 0.0;
    s.background.is_some_and(|c| c.a > 0) || (e(s.border_width) && s.border_color.a > 0) || e(s.padding)
}

impl InlineText {
    fn new() -> Self {
        InlineText {
            text: String::new(),
            spans: Vec::new(),
            boxes: Vec::new(),
            decos: Vec::new(),
            owners: Vec::new(),
            open: Vec::new(),
            collapse: true,
        }
    }

    /// Append the inline content of `id`, collapsing whitespace across node
    /// boundaries so `Hello <b> world</b>` keeps a single space.
    fn collect(
        &mut self,
        doc: &Document,
        styles: &StyleMap,
        id: NodeId,
        deco: u32,
        make_box: &mut BoxMaker,
    ) {
        let node = doc.node(id);
        match &node.kind {
            NodeKind::Text(raw) => {
                let collapsed = collapse_whitespace(raw);
                let piece = if self.collapse {
                    collapsed.strip_prefix(' ').unwrap_or(&collapsed)
                } else {
                    &collapsed
                };
                if !piece.is_empty() {
                    let start = self.text.len();
                    self.text.push_str(piece);
                    let mut ts = text_style(styles.get(id));
                    ts.deco = deco;
                    self.spans.push((start..self.text.len(), ts));
                    // Innermost owner first (hit testing takes the first match);
                    // enclosing inline elements follow, so their rects cover this text.
                    let range = start..self.text.len();
                    if let Some(parent) = node.parent {
                        self.owners.push((range.clone(), parent));
                    }
                    for &outer in self.open.iter().rev().skip(1) {
                        self.owners.push((range.clone(), outer));
                    }
                    self.collapse = piece.ends_with(' ');
                }
            }
            NodeKind::Element { .. } => {
                let es = styles.get(id);
                if es.display == Display::None {
                    return;
                }
                if es.display == Display::InlineBlock {
                    if let Some(spec) = make_box(id) {
                        self.boxes.push(InlineBoxSpec { index: self.text.len(), ..spec });
                        self.collapse = false;
                    }
                    return;
                }
                if node.tag() == Some("br") {
                    self.trim_end();
                    self.text.push('\n');
                    self.collapse = true;
                    return;
                }
                let own = has_inline_paint(es).then(|| {
                    self.decos.push(InlineDeco {
                        background: es.background,
                        border_color: es.border_color,
                        border: es.border_width,
                        padding: es.padding,
                        radius: es.border_radius,
                    });
                    self.decos.len() - 1
                });
                let deco = own.map_or(deco, |i| i as u32 + 1);
                if own.is_some() {
                    self.push_spacer(id, es, deco, es.padding.left + es.border_width.left);
                }
                // TODO: block-level descendants of an inline element should
                // split the run; for now their text is flowed inline.
                self.open.push(id);
                for &c in &node.children {
                    self.collect(doc, styles, c, deco, make_box);
                }
                self.open.pop();
                if own.is_some() {
                    self.push_spacer(id, es, deco, es.padding.right + es.border_width.right);
                }
            }
            NodeKind::Document => {}
        }
    }

    /// Reserve `width` px of horizontal space (padding + border) as a no-break
    /// space. Its font size is a placeholder holding the width; layout turns
    /// it into a real size once the font's no-break-space advance is known.
    fn push_spacer(&mut self, owner: NodeId, es: &ComputedStyle, deco: u32, width: f32) {
        if width > 0.0 {
            let start = self.text.len();
            self.text.push('\u{a0}');
            let mut ts = text_style(es);
            ts.deco = deco;
            ts.spacer = true;
            ts.font_size = width;
            ts.line_height = 0.0;
            ts.underline = false;
            ts.line_through = false;
            self.spans.push((start..self.text.len(), ts));
            self.owners.push((start..self.text.len(), owner));
        }
    }

    fn trim_end(&mut self) {
        if self.collapse && self.text.ends_with(' ') {
            self.text.pop();
            let len = self.text.len();
            for (r, _) in &mut self.spans {
                r.end = r.end.min(len);
                r.start = r.start.min(len);
            }
            self.spans.retain(|(r, _)| !r.is_empty());
            for (r, _) in &mut self.owners {
                r.end = r.end.min(len);
                r.start = r.start.min(len);
            }
            self.owners.retain(|(r, _)| !r.is_empty());
        }
    }
}

fn build_inline_run(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &mut TaffyTree<NodeCtx>,
    run: &[NodeId],
    block_style: &ComputedStyle,
) -> Option<taffy::NodeId> {
    let mut inline = InlineText::new();
    let mut kids: Vec<InlineKid> = Vec::new();
    {
        let mut make_box = |id: NodeId| {
            let n = build(doc, styles, text, tree, id)?;
            // Shrink-to-fit: size the box on its own at max-content.
            tree.compute_layout_with_measure(
                n,
                Size { width: AvailableSpace::MaxContent, height: AvailableSpace::MaxContent },
                |inputs, _id, ctx, style| measure_node(inputs, ctx, style),
            )
            .ok()?;
            let size = tree.layout(n).ok()?.size;
            // A textarea sits on its bottom edge, not on its text.
            let baseline = if doc.node(id).tag() == Some("textarea") { None } else { last_baseline(tree, n) };
            let m = styles.get(id).margin;
            let px = |l: Length| if let Length::Px(v) = l { v } else { 0.0 };
            kids.push(InlineKid { node: n, dx: px(m.left), dy: px(m.top) });
            Some(InlineBoxSpec {
                id: (kids.len() - 1) as u64,
                index: 0,
                width: size.width + px(m.left) + px(m.right),
                height: size.height + px(m.top) + px(m.bottom),
                baseline: baseline.map(|b| b + px(m.top)),
            })
        };
        for &id in run {
            inline.collect(doc, styles, id, 0, &mut make_box);
        }
    }
    inline.trim_end();
    if inline.text.is_empty() && inline.boxes.is_empty() {
        return None;
    }
    for (_, ts) in &mut inline.spans {
        if ts.spacer {
            ts.font_size /= text.nbsp_ratio(&ts.font_family, ts.font_weight);
        }
    }
    let base = text_style(block_style);
    let shaped = text.shape_spans(&inline.text, &base, &inline.spans, &inline.boxes, 1.0);
    // Attribute the run to the first node; paint only needs the layout.
    tree.new_leaf_with_context(
        Style::default(),
        NodeCtx::Text {
            node: run[0],
            layout: Box::new(shaped),
            align: alignment(block_style.text_align),
            kids,
            decos: inline.decos,
            owners: inline.owners,
            nowrap: block_style.nowrap,
        },
    )
    .ok()
}

/// Baseline of the last text line inside `n`, from `n`'s top edge.
fn last_baseline(tree: &TaffyTree<NodeCtx>, n: taffy::NodeId) -> Option<f32> {
    let y = tree.layout(n).ok()?.location.y;
    if let Some(NodeCtx::Text { layout, .. }) = tree.get_node_context(n) {
        return Some(y + layout.lines().last()?.metrics().baseline);
    }
    tree.children(n).ok()?.into_iter().rev().find_map(|c| last_baseline(tree, c)).map(|b| y + b)
}

fn flatten(
    tree: &mut TaffyTree<NodeCtx>,
    n: taffy::NodeId,
    parent_x: f32,
    parent_y: f32,
    out: &mut Vec<LayoutBox>,
) {
    let first = out.len();
    let l = *tree.layout(n).unwrap();
    let rect = Rect {
        x: parent_x + l.location.x,
        y: parent_y + l.location.y,
        width: l.size.width,
        height: l.size.height,
    };

    match tree.get_node_context_mut(n) {
        Some(NodeCtx::Element(id)) => {
            out.push(LayoutBox { node: *id, rect, content: BoxContent::Element, subtree_end: 0, scroll: None })
        }
        Some(NodeCtx::Text { node, layout, align, kids, decos, owners, nowrap }) => {
            // Final line breaking at the resolved width, so alignment is right.
            // Taffy rounds box sizes to whole pixels, which can shave a
            // fraction off the measured width and force a spurious wrap, so
            // allow one pixel of slack.
            // A non-wrapping run breaks at least as wide as its text, so it stays on
            // one line but still aligns within a wider box.
            let width = if *nowrap {
                break_lines(layout, None, Alignment::Start);
                rect.width.max(layout.full_width())
            } else {
                rect.width
            };
            break_lines(layout, Some(width + 1.0), *align);
            let layout = (**layout).clone();
            let kids = kids.clone();
            let decos = decos.clone();
            let owners = owners.clone();
            let placed: Vec<_> = layout
                .lines()
                .flat_map(|line| line.items().collect::<Vec<_>>())
                .filter_map(|item| match item {
                    PositionedLayoutItem::InlineBox(b) => Some(b),
                    _ => None,
                })
                .collect();
            out.push(LayoutBox {
                node: *node,
                rect,
                content: BoxContent::Text(Box::new(TextContent { layout, decos, owners })),
                subtree_end: 0,
                scroll: None,
            });
            for b in placed {
                let kid = kids[b.id as usize];
                flatten(tree, kid.node, rect.x + b.x + kid.dx, rect.y + b.y + kid.dy, out);
            }
        }
        None => {}
    }

    // Only a node that pushed its own box (at `first`) owns a subtree.
    let pushed_self = matches!(out.get(first), Some(b) if b.subtree_end == 0);
    for child in tree.children(n).unwrap() {
        flatten(tree, child, rect.x, rect.y, out);
    }
    if pushed_self {
        out[first].subtree_end = out.len();
    }
}

fn text_style(s: &ComputedStyle) -> TextStyle {
    TextStyle {
        font_family: s.font_family.clone(),
        font_size: s.font_size,
        font_weight: s.font_weight,
        line_height: s.line_height,
        color: [s.color.r, s.color.g, s.color.b, s.color.a],
        deco: 0,
        spacer: false,
        italic: s.italic,
        underline: s.underline,
        line_through: s.line_through,
        align: alignment(s.text_align),
    }
}

fn alignment(a: TextAlign) -> Alignment {
    match a {
        TextAlign::Start => Alignment::Start,
        TextAlign::Center => Alignment::Center,
        TextAlign::End => Alignment::End,
        TextAlign::Justify => Alignment::Justify,
    }
}

fn dim(l: Length) -> Dimension {
    match l {
        Length::Auto => Dimension::auto(),
        Length::Px(px) => Dimension::length(px),
        Length::Percent(p) => Dimension::percent(p),
    }
}

fn lpa(l: Length) -> LengthPercentageAuto {
    match l {
        Length::Auto => LengthPercentageAuto::auto(),
        Length::Px(px) => LengthPercentageAuto::length(px),
        Length::Percent(p) => LengthPercentageAuto::percent(p),
    }
}

fn rect_px(e: Edges<f32>) -> taffy::Rect<LengthPercentage> {
    taffy::Rect {
        top: LengthPercentage::length(e.top),
        right: LengthPercentage::length(e.right),
        bottom: LengthPercentage::length(e.bottom),
        left: LengthPercentage::length(e.left),
    }
}

fn justify(a: Align) -> Option<JustifyContent> {
    Some(match a {
        Align::Normal => return None,
        Align::Start => JustifyContent::FLEX_START,
        Align::End => JustifyContent::FLEX_END,
        Align::Center => JustifyContent::CENTER,
        Align::Stretch => JustifyContent::STRETCH,
        Align::SpaceBetween => JustifyContent::SPACE_BETWEEN,
        Align::SpaceAround => JustifyContent::SPACE_AROUND,
        Align::SpaceEvenly => JustifyContent::SPACE_EVENLY,
    })
}

fn align_items(a: Align) -> Option<AlignItems> {
    Some(match a {
        Align::Start => AlignItems::FLEX_START,
        Align::End => AlignItems::FLEX_END,
        Align::Center => AlignItems::CENTER,
        Align::Stretch => AlignItems::STRETCH,
        _ => return None,
    })
}

fn taffy_style(s: &ComputedStyle) -> Style {
    Style {
        display: match s.display {
            Display::Flex => taffy::Display::Flex,
            Display::None => taffy::Display::None,
            Display::Block | Display::Inline | Display::InlineBlock => taffy::Display::Block,
        },
        box_sizing: if s.border_box { BoxSizing::BorderBox } else { BoxSizing::ContentBox },
        size: Size { width: dim(s.width), height: dim(s.height) },
        min_size: Size { width: lpa(s.min_width), height: lpa(s.min_height) },
        max_size: Size { width: lpa(s.max_width), height: lpa(s.max_height) },
        margin: taffy::Rect {
            top: lpa(s.margin.top),
            right: lpa(s.margin.right),
            bottom: lpa(s.margin.bottom),
            left: lpa(s.margin.left),
        },
        padding: rect_px(s.padding),
        border: rect_px(s.border_width),
        overflow: taffy::Point {
            x: if s.overflow_x != OverflowKind::Visible { taffy::Overflow::Clip } else { taffy::Overflow::Visible },
            y: if s.overflow_y != OverflowKind::Visible { taffy::Overflow::Clip } else { taffy::Overflow::Visible },
        },
        flex_direction: match s.flex_direction {
            FlexDirection::Row => taffy::FlexDirection::Row,
            FlexDirection::Column => taffy::FlexDirection::Column,
            FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
            FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
        },
        flex_wrap: if s.flex_wrap { FlexWrap::Wrap } else { FlexWrap::NoWrap },
        flex_grow: s.flex_grow,
        flex_shrink: s.flex_shrink,
        flex_basis: dim(s.flex_basis),
        justify_content: justify(s.justify_content),
        align_items: align_items(s.align_items),
        gap: Size { width: LengthPercentage::length(s.gap), height: LengthPercentage::length(s.gap) },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nile_style::compute_styles;

    #[test]
    fn flex_row_splits_width() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div style="display:flex; gap:10px">
                 <div id="a" style="flex:1; height:20px"></div>
                 <div id="b" style="flex:1; height:20px"></div>
               </div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 210.0, 100.0);
        let find = |id: &str| {
            let n = doc.find(|n| n.attr("id") == Some(id)).unwrap();
            tree.boxes.iter().find(|b| b.node == n).unwrap().rect
        };
        assert_eq!(find("a"), Rect { x: 0.0, y: 0.0, width: 100.0, height: 20.0 });
        assert_eq!(find("b").x, 110.0);
    }

    fn text_boxes(tree: &LayoutTree) -> Vec<&LayoutBox> {
        tree.boxes.iter().filter(|b| matches!(b.content, BoxContent::Text(_))).collect()
    }

    #[test]
    fn inline_elements_share_one_line() {
        let doc = Document::parse(
            r#"<body style="margin:0"><p style="margin:0">Hello <b>world</b> and <span>more</span>.</p></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
        let texts = text_boxes(&tree);
        assert_eq!(texts.len(), 1, "inline content must be a single run");
        // One line of 16px text at line-height 1.2 (rounded by Taffy).
        assert!(texts[0].rect.height < 16.0 * 1.2 * 1.5, "got {}", texts[0].rect.height);
    }

    #[test]
    fn whitespace_collapses_across_elements() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div>  a <b> b </b> c  </div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let _ = layout(&doc, &styles, &mut text, 800.0, 100.0);
        let mut inline = InlineText::new();
        let div = doc.find(|n| n.tag() == Some("div")).unwrap();
        for &c in &doc.node(div).children {
            inline.collect(&doc, &styles, c, 0, &mut |_| None);
        }
        inline.trim_end();
        assert_eq!(inline.text, "a b c");
    }

    fn run_width(html: &str) -> (f32, usize) {
        let doc = Document::parse(html);
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
        let t = text_boxes(&tree)[0];
        let BoxContent::Text(c) = &t.content else { unreachable!() };
        (c.layout.full_width(), c.decos.len())
    }

    #[test]
    fn inline_padding_and_border_take_horizontal_space() {
        let plain = run_width(r#"<body style="margin:0"><p style="margin:0">a<span>b</span>c</p></body>"#);
        let padded = run_width(
            r#"<body style="margin:0"><p style="margin:0">a<span style="padding:0 6px; border:2px solid red">b</span>c</p></body>"#,
        );
        assert_eq!(plain.1, 0);
        assert_eq!(padded.1, 1);
        assert!((padded.0 - plain.0 - 16.0).abs() < 0.5, "plain {} padded {}", plain.0, padded.0);
    }

    #[test]
    fn spaces_collapse_around_padded_inline() {
        let a = run_width(r#"<body style="margin:0"><p style="margin:0">a <span style="padding:0 5px"> b </span> c</p></body>"#);
        let b = run_width(r#"<body style="margin:0"><p style="margin:0">a <span style="padding:0 5px">b</span> c</p></body>"#);
        assert!((a.0 - b.0).abs() < 0.5, "{} vs {}", a.0, b.0);
    }

    #[test]
    fn node_rect_for_blocks_and_inline_elements() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div id="d" style="width:200px; padding:10px; height:50px"><span id="s">hi <b id="b">bold</b></span><i id="none" style="display:none">x</i></div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
        let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
        assert_eq!(tree.node_rect(id("d")).unwrap(), Rect { x: 0.0, y: 0.0, width: 220.0, height: 70.0 });
        let (s, b) = (tree.node_rect(id("s")).unwrap(), tree.node_rect(id("b")).unwrap());
        assert!(s.x >= 10.0 && b.x > s.x && b.x + b.width <= s.x + s.width + 0.5, "{s:?} {b:?}");
        assert!(b.height > 10.0 && b.y >= 10.0);
        assert_eq!(tree.node_rect(id("none")), None);
    }

    #[test]
    fn text_inputs_show_value_or_placeholder_without_wrapping() {
        let doc = Document::parse(
            r#"<body style="margin:0"><input id="a" value="hello world, a long value that must not wrap" style="width:60px">
               <input id="b" placeholder="Name"><input id="c" type="password" value="abc"><input id="d"></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
        let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
        let text_of = |n: NodeId| {
            tree.boxes.iter().find(|b| b.node == n && matches!(b.content, BoxContent::Text(_))).unwrap()
        };
        let a = text_of(id("a"));
        let BoxContent::Text(c) = &a.content else { unreachable!() };
        assert_eq!(c.layout.lines().count(), 1, "long value stays on one line");
        // The empty input still has a line's height; placeholder is its text.
        let d = tree.node_rect(id("d")).unwrap();
        assert!(d.height > 16.0, "{d:?}");
        let b = text_of(id("b"));
        let BoxContent::Text(c) = &b.content else { unreachable!() };
        assert_eq!(c.owners[0].1, id("b"));
        // Caret mapping: passwords draw bullets (3 bytes each).
        assert_eq!(caret_byte(&doc, id("c"), 2), 6);
        assert_eq!(caret_char(&doc, id("c"), 6), 2);
        assert_eq!(caret_byte(&doc, id("a"), 5), 5);
        let a_rect = tree.node_rect(id("a")).unwrap();
        let idx = tree.text_index_at(id("a"), a_rect.x + 1.0, a_rect.y + a_rect.height / 2.0).unwrap();
        assert!(idx <= 1, "clicking the left edge puts the caret at the start, got {idx}");
        assert!(tree.text_index_at(id("a"), a_rect.x + 500.0, a_rect.y + 8.0).unwrap() > 5);
    }

    #[test]
    fn textareas_wrap_and_move_vertically() {
        let doc = Document::parse(
            r#"<body style="margin:0"><textarea id="t" style="width:80px; height:60px">one two three four five six
second</textarea><textarea id="e"></textarea></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 400.0);
        let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
        assert_eq!(control_value(&doc, id("t")).lines().count(), 2);
        let tb = tree
            .boxes
            .iter()
            .find(|b| matches!(&b.content, BoxContent::Text(c) if c.owners.iter().any(|(_, n)| *n == id("t"))))
            .unwrap();
        let BoxContent::Text(c) = &tb.content else { unreachable!() };
        assert!(c.layout.lines().count() >= 3, "wraps and honours the newline: {}", c.layout.lines().count());
        // Down from the start of line one lands later in the text; up returns.
        let down = tree.move_caret_vertically(id("t"), 0, 1).unwrap();
        assert!(down > 0);
        assert_eq!(tree.move_caret_vertically(id("t"), down, -1), Some(0));
        assert_eq!(tree.move_caret_vertically(id("t"), 0, -1), None);
        let rect = tree.node_rect(id("e")).unwrap();
        assert_eq!((rect.width, rect.height), (210.0, 68.0), "UA size plus padding and border");
    }

    #[test]
    fn nowrap_text_still_aligns_in_a_wider_box() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div id="d" style="width:200px; text-align:center; white-space:nowrap">hi</div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 100.0);
        let d = doc.find(|n| n.attr("id") == Some("d")).unwrap();
        let tb = tree
            .boxes
            .iter()
            .find(|b| matches!(&b.content, BoxContent::Text(c) if c.owners.iter().any(|(_, n)| *n == d)));
        let BoxContent::Text(c) = &tb.unwrap().content else { unreachable!() };
        let line = c.layout.lines().next().unwrap();
        let (x, adv) = (line.metrics().offset, line.metrics().advance);
        assert!(x > 80.0 && x + adv < 130.0, "centered in 200px: offset {x}, advance {adv}");
    }

    #[test]
    fn nowrap_keeps_text_on_one_line() {
        let html = |ws: &str| {
            format!(
                r#"<body style="margin:0"><div id="d" style="width:60px; {ws}">a fairly long run of words</div></body>"#
            )
        };
        let height = |ws: &str| {
            let doc = Document::parse(&html(ws));
            let styles = compute_styles(&doc);
            let mut text = TextEngine::new();
            let tree = layout(&doc, &styles, &mut text, 400.0, 100.0);
            tree.node_rect(doc.find(|n| n.attr("id") == Some("d")).unwrap()).unwrap().height
        };
        assert!(height("") > height("white-space: nowrap") * 2.0);
    }

    #[test]
    fn overflow_boxes_scroll_clip_and_hit_test() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div id="box" style="width:100px; height:50px; overflow:auto; border:2px solid red">
                 <div id="a" style="height:40px">a</div><div id="b" style="height:40px">b</div><div id="c" style="height:40px">c</div>
               </div><div id="hidden" style="width:50px; height:20px; overflow:hidden"><div style="width:200px; height:10px"></div></div>
               <div id="plain" style="height:10px"></div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 400.0);
        let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
        let sb = tree.scroll_box(id("box")).unwrap();
        assert_eq!((sb.clip.width, sb.clip.height), (100.0, 50.0));
        assert_eq!(sb.content_height, 120.0, "three 40px rows");
        assert_eq!(sb.max_offset(), (0.0, 70.0));
        assert!(sb.wheel_y && sb.wheel_x);
        let h = tree.scroll_box(id("hidden")).unwrap();
        assert!(!h.wheel_x && h.max_offset().0 == 150.0, "hidden clips and can be scrolled from script");
        assert!(tree.scroll_box(id("plain")).is_none());

        // Unscrolled: row c is clipped away; scrolled by 70 it is under the pointer.
        let none = ScrollOffsets::new();
        let mut off = ScrollOffsets::new();
        off.insert(id("box"), (0.0, 70.0));
        let (x, y) = (20.0, 2.0 + 45.0); // near the bottom of the viewport of the box
        assert_eq!(tree.hit_test_scrolled(x, y, &none), Some(id("b")));
        assert_eq!(tree.hit_test_scrolled(x, y, &off), Some(id("c")));
        // Outside the clip the scrolled-away content can't be hit; the page behind can.
        assert_ne!(tree.hit_test_scrolled(x, 2.0 + 60.0, &none), Some(id("c")));
        // Rects follow the scroll.
        let r0 = tree.node_rect(id("c")).unwrap();
        let r1 = tree.node_rect_scrolled(id("c"), &off).unwrap();
        assert_eq!(r0.y - r1.y, 70.0);
    }

    #[test]
    fn hit_test_resolves_inline_elements() {
        let doc = Document::parse(
            r#"<body style="margin:0"><p id="p" style="margin:0">Hello <b id="b" style="padding:0 10px">world</b></p></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
        let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
        let tb = text_boxes(&tree)[0];
        let (t, BoxContent::Text(c)) = (tb.rect, &tb.content) else { unreachable!() };
        let text_w = c.layout.full_width();
        let y = t.y + t.height / 2.0;
        assert_eq!(tree.hit_test(t.x + 2.0, y), Some(id("p")));
        assert_eq!(tree.hit_test(t.x + text_w - 14.0, y), Some(id("b")));
        // Below the text but inside the page: not the paragraph.
        assert_ne!(tree.hit_test(t.x + 2.0, t.y + t.height + 30.0), Some(id("p")));
        assert_eq!(tree.hit_test(-5.0, -5.0), None);
    }

    #[test]
    fn br_breaks_the_line() {
        let doc = Document::parse(
            r#"<body style="margin:0"><p style="margin:0">one<br>two</p></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
        let texts = text_boxes(&tree);
        assert_eq!(texts.len(), 1);
        assert!(texts[0].rect.height > 16.0 * 1.2 * 1.5, "got {}", texts[0].rect.height);
    }

    #[test]
    fn inline_block_sits_in_the_text_flow() {
        let doc = Document::parse(
            r#"<body style="margin:0"><p style="margin:0">before <span id="ib" style="display:inline-block; width:50px; height:30px"></span> after</p></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
        let ib = doc.find(|n| n.attr("id") == Some("ib")).unwrap();
        let r = tree.boxes.iter().find(|b| b.node == ib && matches!(b.content, BoxContent::Element)).unwrap().rect;
        assert_eq!((r.width, r.height), (50.0, 30.0));
        assert!(r.x > 20.0, "box should follow the text, got x={}", r.x);
        let t = text_boxes(&tree)[0];
        assert!(t.rect.height >= 30.0, "line grows to fit the box, got {}", t.rect.height);
    }

    #[test]
    fn inline_block_shrinks_to_fit_content() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div><span id="ib" style="display:inline-block; padding:4px">Hi</span></div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
        let ib = doc.find(|n| n.attr("id") == Some("ib")).unwrap();
        let r = tree.boxes.iter().find(|b| b.node == ib && matches!(b.content, BoxContent::Element)).unwrap().rect;
        assert!(r.width > 8.0 && r.width < 60.0, "got {}", r.width);
    }

    #[test]
    fn blocks_split_inline_runs() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div>one <b>two</b><div>three</div>four</div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
        let texts = text_boxes(&tree);
        assert_eq!(texts.len(), 3);
        assert!(texts[0].rect.y < texts[1].rect.y && texts[1].rect.y < texts[2].rect.y);
    }

    #[test]
    fn text_wraps_inside_fixed_width() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div style="width:60px">a fairly long run of words</div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 100.0);
        let t = tree.boxes.iter().find(|b| matches!(b.content, BoxContent::Text(_))).unwrap();
        assert_eq!(t.rect.width, 60.0);
        assert!(t.rect.height > 16.0 * 1.2 * 2.0);
    }
}
