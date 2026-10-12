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

use nile_dom::{Document, NodeId, NodeKind};
use nile_style::{
    Align, Color, ComputedStyle, Display, Edges, FlexDirection, Length, StyleMap, TextAlign,
};
use nile_text::{
    break_lines, collapse_whitespace, Alignment, Cluster, InlineBoxSpec, PositionedLayoutItem, TextEngine,
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
    /// Border box of `node` in document coordinates. Block-level elements have
    /// a box; an inline element's rect is the union of its text runs (without
    /// padding). `None` if it generated nothing (e.g. `display: none`, or empty).
    pub fn node_rect(&self, node: NodeId) -> Option<Rect> {
        if let Some(&i) = self.index.get(&node) {
            return Some(self.boxes[i].rect);
        }
        let (mut l, mut t, mut r, mut b) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for tb in &self.boxes {
            let BoxContent::Text(c) = &tb.content else { continue };
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
                    l = l.min(tb.rect.x + gr.offset());
                    r = r.max(tb.rect.x + gr.offset() + gr.advance());
                    t = t.min(tb.rect.y + gr.baseline() - fm.ascent);
                    b = b.max(tb.rect.y + gr.baseline() + fm.descent);
                }
            }
        }
        (l <= r).then_some(Rect { x: l, y: t, width: r - l, height: b - t })
    }

    /// The innermost element under `(x, y)` in document coordinates: the
    /// topmost element box, or the inline element owning the text there.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        for b in self.boxes.iter().rev() {
            let r = b.rect;
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
    },
}

fn measure_node(
    inputs: taffy::LayoutInput,
    ctx: Option<&mut NodeCtx>,
    style: &Style,
) -> taffy::LayoutOutput {
    taffy::compute_leaf_layout(inputs, style, |_, _| 0.0, |known, available| match ctx {
        Some(NodeCtx::Text { layout, align, .. }) => measure_text(layout, *align, known, available),
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
    LayoutTree { boxes, content_height, index }
}

fn measure_text(
    layout: &mut TextLayout,
    align: Alignment,
    known: Size<Option<f32>>,
    available: Size<AvailableSpace>,
) -> Size<f32> {
    let max_width = known.width.or(match available.width {
        AvailableSpace::Definite(w) => Some(w),
        AvailableSpace::MinContent => Some(0.0),
        AvailableSpace::MaxContent => None,
    });
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
            let children = build_children(doc, styles, text, tree, node.children.as_slice(), style);
            let n = tree.new_with_children(taffy_style(style), &children).ok()?;
            tree.set_node_context(n, Some(NodeCtx::Element(id))).ok()?;
            Some(n)
        }
    }
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
            let baseline = last_baseline(tree, n);
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
    let l = *tree.layout(n).unwrap();
    let rect = Rect {
        x: parent_x + l.location.x,
        y: parent_y + l.location.y,
        width: l.size.width,
        height: l.size.height,
    };

    match tree.get_node_context_mut(n) {
        Some(NodeCtx::Element(id)) => {
            out.push(LayoutBox { node: *id, rect, content: BoxContent::Element })
        }
        Some(NodeCtx::Text { node, layout, align, kids, decos, owners }) => {
            // Final line breaking at the resolved width, so alignment is right.
            // Taffy rounds box sizes to whole pixels, which can shave a
            // fraction off the measured width and force a spurious wrap, so
            // allow one pixel of slack.
            break_lines(layout, Some(rect.width + 1.0), *align);
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
            out.push(LayoutBox { node: *node, rect, content: BoxContent::Text(Box::new(TextContent { layout, decos, owners })) });
            for b in placed {
                let kid = kids[b.id as usize];
                flatten(tree, kid.node, rect.x + b.x + kid.dx, rect.y + b.y + kid.dy, out);
            }
        }
        None => {}
    }

    for child in tree.children(n).unwrap() {
        flatten(tree, child, rect.x, rect.y, out);
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
