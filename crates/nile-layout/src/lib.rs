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
use std::rc::Rc;

mod affine;
pub use affine::Affine;
mod images;
pub use images::{FileImages, ImageData, ImageLoader};

pub use nile_dom::NodeId;
use nile_dom::{Document, NodeKind};
use nile_style::{area_lines, Breadth, BorderStyle, GridLine, GridPlace, Position, Track, TrackItem, TrackRepeat, PseudoElement, TextTransform, 
    Align, Color, ComputedStyle, Display, Edges, FlexDirection, Length, OverflowKind, StyleMap, TextAlign,
};
use nile_text::{
    break_lines, collapse_whitespace, Affinity, Alignment, Cluster, Cursor, InlineBoxSpec, Selection, PositionedLayoutItem, TextEngine,
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
    pub border_color: Edges<Color>,
    pub border_style: Edges<BorderStyle>,
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
    /// The text the layout shaped.
    pub text: String,
    /// Byte ranges of `text` that are padding/border spacers, not content.
    pub spacers: Vec<Range<usize>>,
    /// The text of a form control; page selection skips it.
    pub control: bool,
    /// May the user select this text (`user-select`).
    pub user_select: nile_style::UserSelect,
}

impl TextContent {
    /// Can page selection include this run: not a form control, and not `user-select: none`.
    pub fn selectable(&self) -> bool {
        !self.control && self.user_select != nile_style::UserSelect::None
    }
}

/// A place in the page's text: a text run (an index into [`LayoutTree::boxes`]) and a
/// byte offset into its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPoint {
    pub run: usize,
    pub byte: usize,
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
    /// The picture an `<img>` shows.
    pub image: Option<Rc<ImageData>>,
    /// The picture of `background-image: url(...)`.
    pub bg_image: Option<Rc<ImageData>>,
    /// `transform` of the element, as layout coordinates to layout coordinates (the
    /// origin is already folded in). Applies to the box and its subtree.
    pub transform: Option<Affine>,
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

/// Overlay scrollbar metrics, shared by painting and dragging.
pub const SCROLLBAR_THICK: f32 = 6.0;
pub const SCROLLBAR_EDGE: f32 = 2.0;

/// One scrollbar: its thumb and the strip it slides in, in the same coordinates as the
/// box it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thumb {
    pub rect: Rect,
    pub track: Rect,
    pub vertical: bool,
    /// Largest scroll offset along this axis.
    pub max: f32,
}

impl Thumb {
    fn new(track: Rect, vertical: bool, view: f32, content: f32, offset: f32) -> Option<Thumb> {
        let max = (content - view).max(0.0);
        if max <= 0.0 || view <= 0.0 {
            return None;
        }
        let len = (view * view / content).clamp(16.0_f32.min(view), view);
        let at = (view - len) * (offset / max).clamp(0.0, 1.0);
        let rect = if vertical {
            Rect { x: track.x, y: track.y + at, width: track.width, height: len }
        } else {
            Rect { x: track.x + at, y: track.y, width: len, height: track.height }
        };
        Some(Thumb { rect, track, vertical, max })
    }

    /// The scroll offset that puts the thumb's start at `pos` (a coordinate along the track).
    pub fn offset_for(&self, pos: f32) -> f32 {
        let (start, len, thumb) = if self.vertical {
            (self.track.y, self.track.height, self.rect.height)
        } else {
            (self.track.x, self.track.width, self.rect.width)
        };
        let free = (len - thumb).max(f32::EPSILON);
        ((pos - start) / free).clamp(0.0, 1.0) * self.max
    }
}

/// The page's own vertical scrollbar, in viewport coordinates.
pub fn page_thumb(viewport: (f32, f32), content_height: f32, scroll_y: f32) -> Option<Thumb> {
    let track = Rect { x: viewport.0 - SCROLLBAR_THICK - SCROLLBAR_EDGE, y: 0.0, width: SCROLLBAR_THICK, height: viewport.1 };
    Thumb::new(track, true, viewport.1, content_height, scroll_y)
}

impl ScrollBox {
    /// The scrollbars to show at scroll offsets `(ox, oy)`.
    pub fn thumbs(&self, ox: f32, oy: f32) -> Vec<Thumb> {
        let (c, mut out) = (self.clip, Vec::new());
        if self.wheel_y {
            let track = Rect { x: c.x + c.width - SCROLLBAR_THICK - SCROLLBAR_EDGE, y: c.y, width: SCROLLBAR_THICK, height: c.height };
            out.extend(Thumb::new(track, true, c.height, self.content_height, oy));
        }
        if self.wheel_x {
            let track = Rect { x: c.x, y: c.y + c.height - SCROLLBAR_THICK - SCROLLBAR_EDGE, width: c.width, height: SCROLLBAR_THICK };
            out.extend(Thumb::new(track, false, c.width, self.content_width, ox));
        }
        out
    }

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
    /// Layout (unscrolled document) coordinates to where the box paints.
    pub xf: Affine,
    /// What containers clip it to, as a rectangle in painted coordinates.
    pub clip: Option<Rect>,
}

impl Placement {
    /// A painted point mapped back into layout coordinates.
    pub fn to_layout(&self, x: f32, y: f32) -> (f32, f32) {
        self.xf.inverse().map_or((x, y), |inv| inv.apply(x, y))
    }
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
    /// The open `<select>` drop-down, painted above everything else.
    pub menu: Option<MenuBox>,
    /// Element boxes by node, for `node_rect`.
    index: std::collections::HashMap<NodeId, usize>,
    /// Generated (`::before`/`::after`) box -> its element, so hits land on the element.
    origins: std::collections::HashMap<NodeId, NodeId>,
}

/// Ask for a `<select>`'s drop-down to be laid out under it.
#[derive(Debug, Clone, Copy)]
pub struct MenuRequest {
    pub select: NodeId,
    /// The row under the pointer or keyboard cursor.
    pub highlight: Option<usize>,
}

/// Inputs to layout beyond the document and styles.
#[derive(Default)]
pub struct LayoutOptions<'a> {
    pub menu: Option<MenuRequest>,
    /// Scroll offsets, so the menu opens under where its select actually paints.
    pub offsets: Option<&'a ScrollOffsets>,
    /// Where `<img src>` and `url()` backgrounds come from; without one they don't show.
    pub images: Option<&'a dyn ImageLoader>,
}

pub struct MenuItem {
    pub text: TextLayout,
    pub disabled: bool,
}

/// An open `<select>` list in document coordinates.
pub struct MenuBox {
    pub rect: Rect,
    pub row_height: f32,
    pub pad_x: f32,
    pub items: Vec<MenuItem>,
    pub selected: Option<usize>,
    pub highlight: Option<usize>,
}

impl MenuBox {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.rect.x && y >= self.rect.y && x < self.rect.x + self.rect.width && y < self.rect.y + self.rect.height
    }

    /// The row under a point in the menu, if any.
    pub fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        if !self.contains(x, y) {
            return None;
        }
        let i = ((y - self.rect.y - 1.0) / self.row_height).floor();
        (i >= 0.0 && (i as usize) < self.items.len()).then_some(i as usize)
    }
}

/// The `<option>` elements of a `<select>`, in document order.
pub fn select_options(doc: &Document, select: NodeId) -> Vec<NodeId> {
    doc.descendants(select).into_iter().filter(|n| doc.node(*n).tag() == Some("option")).collect()
}

/// An option's visible text: its `label`, else its text with whitespace collapsed.
pub fn option_label(doc: &Document, option: NodeId) -> String {
    match doc.node(option).attr("label") {
        Some(l) if !l.is_empty() => l.to_string(),
        _ => collapse_whitespace(&doc.text_content(option)).trim().to_string(),
    }
}

/// Index of the selected option: the first marked `selected`, else the first
/// enabled one (a single-choice select always has something chosen).
pub fn selected_index(doc: &Document, select: NodeId) -> Option<usize> {
    let options = select_options(doc, select);
    options
        .iter()
        .position(|o| doc.has_state(*o, "selected"))
        .or_else(|| options.iter().position(|o| !doc.has_state(*o, "disabled")))
        .or((!options.is_empty()).then_some(0))
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
            // A transform applies to the box itself and everything inside it.
            let own = Placement { xf: b.transform.map_or(cur.xf, |t| cur.xf.then_after(t)), clip: cur.clip };
            out.push(own);
            if let Some(sb) = &b.scroll {
                let (ox, oy) = offsets.get(&b.node).copied().unwrap_or_default();
                let clip = intersect(own.clip, own.xf.bounds(sb.clip));
                let xf = own.xf.then_after(Affine::translate(-ox, -oy));
                stack.push((b.subtree_end, Placement { xf, clip: Some(clip) }));
            } else if b.transform.is_some() {
                stack.push((b.subtree_end, own));
            }
        }
        out
    }

    /// The text position nearest a point in painted coordinates, among the page's own text
    /// (form controls are skipped): the run under the point, else the closest one.
    pub fn text_point_at(&self, x: f32, y: f32, offsets: &ScrollOffsets) -> Option<TextPoint> {
        let places = self.placements(offsets);
        let mut best: Option<(f32, f32, usize, f32, f32)> = None;
        for (i, (b, p)) in self.boxes.iter().zip(&places).enumerate() {
            let BoxContent::Text(c) = &b.content else { continue };
            if !c.selectable() || c.text.trim().is_empty() {
                continue;
            }
            let (lx, ly) = p.to_layout(x, y);
            let r = b.rect;
            let dy = if ly < r.y { r.y - ly } else if ly > r.y + r.height { ly - r.y - r.height } else { 0.0 };
            let dx = if lx < r.x { r.x - lx } else if lx > r.x + r.width { lx - r.x - r.width } else { 0.0 };
            // Same row beats a nearer column on another row; later boxes win ties.
            if best.is_none_or(|(by, bx, ..)| (dy, dx) <= (by, bx)) {
                best = Some((dy, dx, i, lx, ly));
            }
        }
        let (_, _, i, lx, ly) = best?;
        let BoxContent::Text(c) = &self.boxes[i].content else { return None };
        let r = self.boxes[i].rect;
        Some(TextPoint { run: i, byte: Cursor::from_point(&c.layout, lx - r.x, ly - r.y).index().min(c.text.len()) })
    }

    /// The word (or the whole run, for `whole_run`) around a point returned by `text_point_at`.
    pub fn word_at(&self, p: TextPoint, whole_run: bool) -> Option<(TextPoint, TextPoint)> {
        let BoxContent::Text(c) = &self.boxes[p.run].content else { return None };
        let range = if whole_run {
            0..c.text.len()
        } else {
            // The word whose glyphs sit at the caret position.
            let g = Cursor::from_byte_index(&c.layout, p.byte.min(c.text.len()), Affinity::Downstream).geometry(&c.layout, 1.0);
            let (x, y) = ((g.x0 + g.x1) as f32 / 2.0 + 0.5, (g.y0 + g.y1) as f32 / 2.0);
            Selection::word_from_point(&c.layout, x, y).text_range()
        };
        Some((TextPoint { run: p.run, byte: range.start }, TextPoint { run: p.run, byte: range.end }))
    }

    /// A click on this run selects all of it (`user-select: all`).
    pub fn selects_whole_run(&self, run: usize) -> bool {
        matches!(&self.boxes[run].content, BoxContent::Text(c) if c.user_select == nile_style::UserSelect::All)
    }

    /// The first and last text positions of the page.
    pub fn text_extent(&self) -> Option<(TextPoint, TextPoint)> {
        let mut runs = self.boxes.iter().enumerate().filter_map(|(i, b)| match &b.content {
            BoxContent::Text(c) if c.selectable() && !c.text.trim().is_empty() => Some((i, c.text.len())),
            _ => None,
        });
        let (first, _) = runs.next().or(None)?;
        let (last, len) = runs.next_back().unwrap_or((first, self.text_len(first)));
        Some((TextPoint { run: first, byte: 0 }, TextPoint { run: last, byte: len }))
    }

    fn text_len(&self, run: usize) -> usize {
        match &self.boxes[run].content {
            BoxContent::Text(c) => c.text.len(),
            _ => 0,
        }
    }

    /// What `a..b` (either order) selects, as `(run, from, to)` per text run.
    pub fn selection_ranges(&self, a: TextPoint, b: TextPoint) -> Vec<(usize, usize, usize)> {
        let (from, to) = if a <= b { (a, b) } else { (b, a) };
        let mut out = Vec::new();
        for run in from.run..=to.run.min(self.boxes.len().saturating_sub(1)) {
            let BoxContent::Text(c) = &self.boxes[run].content else { continue };
            if !c.selectable() {
                continue;
            }
            let start = if run == from.run { from.byte } else { 0 };
            let end = if run == to.run { to.byte } else { c.text.len() };
            if start < end {
                out.push((run, start, end.min(c.text.len())));
            }
        }
        out
    }

    /// The text of a selection, as copying would give it: padding spacers dropped, runs
    /// on different lines separated by newlines.
    pub fn selection_text(&self, ranges: &[(usize, usize, usize)]) -> String {
        let mut out = String::new();
        let mut prev: Option<&LayoutBox> = None;
        for &(run, from, to) in ranges {
            let b = &self.boxes[run];
            let BoxContent::Text(c) = &b.content else { continue };
            if let Some(p) = prev {
                out.push(if b.rect.y >= p.rect.y + p.rect.height - 1.0 { '\n' } else { ' ' });
            }
            let mut at = from;
            while at < to {
                let Some(ch) = c.text[at..].chars().next() else { break };
                let spacer = c.spacers.iter().any(|r| r.contains(&at));
                if !spacer {
                    out.push(if ch == '\u{a0}' { ' ' } else { ch });
                }
                at += ch.len_utf8();
            }
            prev = Some(b);
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
        let (lx, ly) = p.to_layout(x, y);
        Some(Cursor::from_point(&c.layout, lx - b.rect.x, ly - b.rect.y).index())
    }

    /// The word (byte range of `node`'s displayed text) under a point in painted
    /// coordinates, for double-click selection.
    pub fn word_range_at(&self, node: NodeId, x: f32, y: f32, offsets: &ScrollOffsets) -> Option<std::ops::Range<usize>> {
        let (i, b) = self.text_box_of(node)?;
        let BoxContent::Text(c) = &b.content else { return None };
        let p = self.placements(offsets)[i];
        let (lx, ly) = p.to_layout(x, y);
        Some(Selection::word_from_point(&c.layout, lx - b.rect.x, ly - b.rect.y).text_range())
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
        let xf = |i: usize| places.map_or(Affine::IDENTITY, |p| p[i].xf);
        if let Some(&i) = self.index.get(&node) {
            return Some(xf(i).bounds(self.boxes[i].rect));
        }
        let (mut l, mut t, mut r, mut b) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for (ti, tb) in self.boxes.iter().enumerate() {
            let BoxContent::Text(c) = &tb.content else { continue };
            let m = xf(ti);
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
                    let run = Rect {
                        x: tb.rect.x + gr.offset(),
                        y: tb.rect.y + gr.baseline() - fm.ascent,
                        width: gr.advance(),
                        height: fm.ascent + fm.descent,
                    };
                    let bounds = m.bounds(run);
                    l = l.min(bounds.x);
                    r = r.max(bounds.x + bounds.width);
                    t = t.min(bounds.y);
                    b = b.max(bounds.y + bounds.height);
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
            let (lx, ly) = p.to_layout(x, y);
            let r = b.rect;
            if lx < r.x || ly < r.y || lx >= r.x + r.width || ly >= r.y + r.height {
                continue;
            }
            match &b.content {
                BoxContent::Element => return Some(self.origins.get(&b.node).copied().unwrap_or(b.node)),
                BoxContent::Text(c) => {
                    let Some((cluster, _)) = Cluster::from_point_exact(&c.layout, lx - r.x, ly - r.y) else {
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
    /// An `<img>`: a leaf of its picture's size (0 x 0 without one).
    Replaced { id: NodeId, size: (f32, f32), image: Option<Rc<ImageData>> },
    Text {
        node: NodeId,
        layout: Box<TextLayout>,
        align: Alignment,
        kids: Vec<InlineKid>,
        decos: Vec<InlineDeco>,
        owners: Vec<(Range<usize>, NodeId)>,
        text: String,
        spacers: Vec<Range<usize>>,
        control: bool,
        user_select: nile_style::UserSelect,
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
        Some(NodeCtx::Replaced { size: (w, h), .. }) => {
            let (w, h) = (*w, *h);
            match (known.width, known.height) {
                (Some(kw), Some(kh)) => Size { width: kw, height: kh },
                (Some(kw), None) if w > 0.0 => Size { width: kw, height: kw * h / w },
                (None, Some(kh)) if h > 0.0 => Size { width: kh * w / h, height: kh },
                (kw, kh) => Size { width: kw.unwrap_or(w), height: kh.unwrap_or(h) },
            }
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
    layout_with(doc, styles, text, viewport_width, viewport_height, &LayoutOptions::default())
}

pub fn layout_with(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    viewport_width: f32,
    viewport_height: f32,
    options: &LayoutOptions,
) -> LayoutTree {
    let mut tree: TaffyTree<NodeCtx> = TaffyTree::new();

    load_images(doc, styles, options.images);
    let html = doc.find(|n| n.tag() == Some("html")).unwrap_or(Document::ROOT);
    // Absolute boxes with no positioned ancestor use the root as their containing block.
    CONTAINING_BLOCKS.with(|s| s.borrow_mut().push(Vec::new()));
    let was = PARENT_IS_CB.replace(false);
    let root = build(doc, styles, text, &mut tree, html).unwrap_or_else(|| {
        tree.new_leaf_with_context(Style::default(), NodeCtx::Element(html)).unwrap()
    });
    PARENT_IS_CB.set(was);
    for abs in CONTAINING_BLOCKS.with(|s| s.borrow_mut().pop()).unwrap_or_default() {
        tree.add_child(root, abs).unwrap();
    }

    // The root fills the viewport, like a browser's initial containing block.
    let mut root_style = tree.style(root).unwrap().clone();
    root_style.size.width = Dimension::length(viewport_width);
    root_style.min_size.height = LengthPercentageAuto::length(viewport_height);
    tree.set_style(root, root_style).unwrap();

    let compute = |tree: &mut TaffyTree<NodeCtx>| {
        tree.compute_layout_with_measure(
            root,
            Size { width: AvailableSpace::Definite(viewport_width), height: AvailableSpace::MaxContent },
            |inputs, _id, ctx, style| measure_node(inputs, ctx, style),
        )
        .expect("layout of a well-formed tree cannot fail");
    };
    compute(&mut tree);
    // `calc()` mixing `%` and `px` can't be expressed to Taffy; resolve it against the
    // containing block's laid-out size and lay out again until it settles.
    if styles.styles.iter().any(has_calc) {
        for _ in 0..4 {
            if !resolve_calcs(&mut tree, root, styles, None) {
                break;
            }
            compute(&mut tree);
        }
    }

    let mut boxes = Vec::new();
    flatten(&mut tree, styles, root, 0.0, 0.0, &mut boxes);
    let content_height = tree.layout(root).map(|l| l.size.height).unwrap_or(0.0);
    let index = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b.content, BoxContent::Element) && !styles.is_pseudo(b.node))
        .map(|(i, b)| (b.node, i))
        .collect();
    let origins = styles.origins().clone();
    let mut tree = LayoutTree { boxes, content_height, menu: None, index, origins };
    tree.compute_scroll_boxes(styles);
    if let Some(req) = options.menu {
        tree.menu = build_menu(doc, styles, text, &tree, req, options.offsets);
    }
    tree
}

fn has_calc(s: &ComputedStyle) -> bool {
    let calc = |l: Length| matches!(l, Length::Calc { .. });
    let m = s.margin;
    [s.width, s.height, s.min_width, s.min_height, s.max_width, s.max_height, s.flex_basis, m.top, m.right, m.bottom, m.left]
        .into_iter()
        .any(calc)
}

/// Gives every `calc()` size and margin under `node` a concrete value from `parent`'s
/// content size (`None` for the root). True if any style changed.
// TODO: a percentage height should only resolve against a definite parent height; here
// it uses the parent's laid-out height whenever the parent's own height isn't `auto`.
fn resolve_calcs(
    tree: &mut TaffyTree<NodeCtx>,
    node: taffy::NodeId,
    styles: &StyleMap,
    parent: Option<(f32, Option<f32>)>,
) -> bool {
    let mut changed = false;
    let id = match tree.get_node_context(node) {
        Some(NodeCtx::Element(id)) => Some(*id),
        _ => None,
    };
    let mut content = None;
    if let Some(id) = id {
        let cs = styles.get(id);
        if let (true, Some((pw, ph))) = (has_calc(cs), parent) {
            let mut ts = tree.style(node).unwrap().clone();
            let across = |l: Length, basis: f32| match l {
                Length::Calc { px, pct } => Some(px + pct * basis),
                _ => None,
            };
            let w = |l: Length| across(l, pw).map(Dimension::length);
            let h = |l: Length| ph.and_then(|b| across(l, b)).map(Dimension::length);
            let wl = |l: Length| across(l, pw).map(LengthPercentageAuto::length);
            let hl = |l: Length| ph.and_then(|b| across(l, b)).map(LengthPercentageAuto::length);
            if let Some(v) = w(cs.width) {
                ts.size.width = v;
            }
            if let Some(v) = h(cs.height) {
                ts.size.height = v;
            }
            if let Some(v) = w(cs.flex_basis) {
                ts.flex_basis = v;
            }
            for (slot, l) in [(&mut ts.min_size.width, cs.min_width), (&mut ts.max_size.width, cs.max_width)] {
                if let Some(v) = wl(l) {
                    *slot = v;
                }
            }
            for (slot, l) in [(&mut ts.min_size.height, cs.min_height), (&mut ts.max_size.height, cs.max_height)] {
                if let Some(v) = hl(l) {
                    *slot = v;
                }
            }
            for (slot, l) in [
                (&mut ts.margin.left, cs.margin.left),
                (&mut ts.margin.right, cs.margin.right),
                (&mut ts.margin.top, cs.margin.top),
                (&mut ts.margin.bottom, cs.margin.bottom),
            ] {
                if let Some(v) = wl(l) {
                    *slot = v;
                }
            }
            if ts != *tree.style(node).unwrap() {
                tree.set_style(node, ts).unwrap();
                changed = true;
            }
        }
        let l = tree.layout(node).unwrap();
        let (p, b) = (cs.padding, cs.border_width);
        let cw = (l.size.width - p.left - p.right - b.left - b.right).max(0.0);
        let ch = (l.size.height - p.top - p.bottom - b.top - b.bottom).max(0.0);
        // Containing-block height is only definite when the element sets one.
        let definite = !matches!(cs.height, Length::Auto);
        content = Some((cw, definite.then_some(ch)));
    }
    let kids = tree.children(node).unwrap();
    for k in kids {
        changed |= resolve_calcs(tree, k, styles, content.or(parent));
    }
    changed
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

thread_local! {
    /// Images the page uses, by the `src`/`url()` text they were requested with.
    static IMAGES: std::cell::RefCell<std::collections::HashMap<String, Rc<ImageData>>> = std::cell::RefCell::default();
    /// Absolutely positioned boxes waiting to be attached to their containing block: one
    /// list per positioned ancestor being built, the root's at the bottom.
    static CONTAINING_BLOCKS: std::cell::RefCell<Vec<Vec<taffy::NodeId>>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The element whose children are being built is itself a containing block.
    static PARENT_IS_CB: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `build_children` for a box that may be a containing block for absolute
/// descendants; those are attached to the box's Taffy node, not their DOM parent's.
fn with_containing_block(
    positioned: bool,
    children: impl FnOnce() -> Vec<taffy::NodeId>,
) -> Vec<taffy::NodeId> {
    if positioned {
        CONTAINING_BLOCKS.with(|s| s.borrow_mut().push(Vec::new()));
    }
    let was = PARENT_IS_CB.replace(positioned);
    let mut kids = children();
    PARENT_IS_CB.set(was);
    if positioned {
        kids.extend(CONTAINING_BLOCKS.with(|s| s.borrow_mut().pop()).unwrap_or_default());
    }
    kids
}

/// An absolutely positioned box whose containing block is above its parent goes to that
/// block's list. `None` if it was handed off, else the node to place as usual.
// TODO: with `auto` insets such a box sits at its containing block's corner, not at its
// static position in the flow.
fn hand_off_absolute(style: &ComputedStyle, n: taffy::NodeId) -> Option<taffy::NodeId> {
    if style.position.out_of_flow() && !PARENT_IS_CB.get() {
        let taken = CONTAINING_BLOCKS.with(|s| s.borrow_mut().last_mut().map(|list| list.push(n)).is_some());
        if taken {
            return None;
        }
    }
    Some(n)
}

/// Ask `loader` for every image the document and styles name.
fn load_images(doc: &Document, styles: &StyleMap, loader: Option<&dyn ImageLoader>) {
    IMAGES.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        let Some(loader) = loader else { return };
        let mut want = |src: &str| {
            if !m.contains_key(src) {
                if let Some(img) = loader.load(src) {
                    m.insert(src.to_string(), img);
                }
            }
        };
        for n in &doc.nodes {
            if n.tag() == Some("img") {
                if let Some(src) = n.attr("src") {
                    want(src);
                }
            }
        }
        for s in &styles.styles {
            if let Some(url) = &s.background_url {
                want(url);
            }
        }
    });
}

fn image_for(src: &str) -> Option<Rc<ImageData>> {
    IMAGES.with(|m| m.borrow().get(src).cloned())
}

/// An `<img>`: sized by CSS, else its `width`/`height` attributes, else its picture.
fn build_image(doc: &Document, styles: &StyleMap, tree: &mut TaffyTree<NodeCtx>, id: NodeId) -> Option<taffy::NodeId> {
    let node = doc.node(id);
    let style = styles.get(id);
    let image = node.attr("src").and_then(image_for);
    let mut ts = taffy_style(style);
    let attr = |name: &str| node.attr(name).and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok());
    if style.width == Length::Auto {
        if let Some(w) = attr("width") {
            ts.size.width = Dimension::length(w);
        }
    }
    if style.height == Length::Auto {
        if let Some(h) = attr("height") {
            ts.size.height = Dimension::length(h);
        }
    }
    let size = image.as_ref().map_or((0.0, 0.0), |i| (i.width as f32, i.height as f32));
    if size.0 > 0.0 && size.1 > 0.0 {
        ts.aspect_ratio = Some(size.0 / size.1);
    }
    let n = tree.new_leaf_with_context(ts, NodeCtx::Replaced { id, size, image }).ok()?;
    hand_off_absolute(style, n)
}

fn build(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &mut TaffyTree<NodeCtx>,
    id: NodeId,
) -> Option<taffy::NodeId> {
    if styles.is_pseudo(id) {
        return build_pseudo(doc, styles, text, tree, id);
    }
    let node = doc.node(id);
    let style = styles.get(id);
    if node.tag() == Some("img") && style.display != Display::None {
        return build_image(doc, styles, tree, id);
    }

    match &node.kind {
        // Text outside any block container (e.g. directly under the document).
        NodeKind::Text(_) => build_inline_run(doc, styles, text, tree, &[id], styles.get(id)),
        NodeKind::Element { .. } | NodeKind::Document => {
            if style.display == Display::None {
                return None;
            }
            let children = with_containing_block(style.position != Position::Static, || {
                if is_value_control(node) {
                    build_control_text(doc, styles, text, tree, id).into_iter().collect()
                } else {
                    build_children(doc, styles, text, tree, &kids(doc, styles, id), style)
                }
            });
            let mut ts = taffy_style(style);
            if node.tag() == Some("select") && style.width == Length::Auto {
                // Wide enough for the longest option, whichever is showing.
                ts.min_size.width = LengthPercentageAuto::length(widest_option(doc, style, text, id));
            }
            let n = tree.new_with_children(ts, &children).ok()?;
            tree.set_node_context(n, Some(NodeCtx::Element(id))).ok()?;
            hand_off_absolute(style, n)
        }
    }
}

/// `<input>` types whose `value` is drawn as a line of text.
/// The children of `id` with its generated `::before` and `::after` boxes around them.
fn kids(doc: &Document, styles: &StyleMap, id: NodeId) -> Vec<NodeId> {
    let node = doc.node(id);
    let mut out = Vec::with_capacity(node.children.len() + 2);
    out.extend(styles.pseudo(id, PseudoElement::Before));
    out.extend(node.children.iter().copied());
    out.extend(styles.pseudo(id, PseudoElement::After));
    out
}

/// A `::before`/`::after` box that isn't part of an inline run: a block, flex item or
/// inline-block, holding its text.
fn build_pseudo(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &mut TaffyTree<NodeCtx>,
    id: NodeId,
) -> Option<taffy::NodeId> {
    let style = styles.get(id);
    if style.display == Display::None {
        return None;
    }
    let children = with_containing_block(style.position != Position::Static, || {
        build_inline_run_in(doc, styles, text, tree, &[id], style, Some(id)).into_iter().collect()
    });
    let n = tree.new_with_children(taffy_style(style), &children).ok()?;
    tree.set_node_context(n, Some(NodeCtx::Element(id))).ok()?;
    hand_off_absolute(style, n)
}

fn is_value_control(node: &nile_dom::Node) -> bool {
    node.tag() == Some("textarea")
        || node.tag() == Some("select")
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
    if node.tag() == Some("select") {
        let label = selected_index(doc, id).map(|i| option_label(doc, select_options(doc, id)[i])).unwrap_or_default();
        return (if label.is_empty() { "\u{200b}".into() } else { label }, false);
    }
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

/// Width of the longest option's text, so a select doesn't resize as the choice changes.
fn widest_option(doc: &Document, style: &ComputedStyle, text: &mut TextEngine, select: NodeId) -> f32 {
    let ts = text_style(style);
    select_options(doc, select)
        .into_iter()
        .map(|o| {
            let mut l = text.shape(&option_label(doc, o), &ts, 1.0);
            break_lines(&mut l, None, Alignment::Start);
            l.full_width()
        })
        .fold(0.0, f32::max)
        .ceil()
}

/// The drop-down list for `req.select`, below its box.
fn build_menu(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &LayoutTree,
    req: MenuRequest,
    offsets: Option<&ScrollOffsets>,
) -> Option<MenuBox> {
    let style = styles.get(req.select);
    let rect = match offsets {
        Some(o) => tree.node_rect_scrolled(req.select, o)?,
        None => tree.node_rect(req.select)?,
    };
    let ts = text_style(style);
    let items: Vec<MenuItem> = select_options(doc, req.select)
        .into_iter()
        .map(|o| {
            let mut l = text.shape(&option_label(doc, o), &ts, 1.0);
            break_lines(&mut l, None, Alignment::Start);
            MenuItem { text: l, disabled: doc.has_state(o, "disabled") }
        })
        .collect();
    if items.is_empty() {
        return None;
    }
    let pad_x = 8.0;
    let widest = items.iter().map(|i| i.text.full_width()).fold(0.0, f32::max);
    let row_height = (style.font_size * style.line_height + 6.0).ceil();
    Some(MenuBox {
        rect: Rect {
            x: rect.x,
            y: rect.y + rect.height,
            width: rect.width.max((widest + 2.0 * pad_x + 2.0).ceil()),
            height: row_height * items.len() as f32 + 2.0,
        },
        row_height,
        pad_x,
        selected: selected_index(doc, req.select),
        highlight: req.highlight,
        items,
    })
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
            text: display.clone(),
            spacers: Vec::new(),
            control: true,
            user_select: nile_style::UserSelect::None,
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
        let pseudo = styles.is_pseudo(c);
        let is_inline = if pseudo {
            let cs = styles.get(c);
            matches!(cs.display, Display::Inline | Display::InlineBlock) && !cs.position.out_of_flow() && cs.float == nile_style::Float::None
        } else {
            match &doc.node(c).kind {
                NodeKind::Text(_) => true,
                NodeKind::Element { .. } => {
                    let cs = styles.get(c);
                    matches!(cs.display, Display::Inline | Display::InlineBlock) && !cs.position.out_of_flow() && cs.float == nile_style::Float::None
                }
                NodeKind::Document => false,
            }
        };
        if is_inline && !matches!(parent_style.display, Display::Flex | Display::Grid) {
            run.push(c);
        } else {
            flush(&mut run, &mut out, text, tree);
            if !pseudo && matches!(doc.node(c).kind, NodeKind::Text(_)) {
                out.extend(build_inline_run(doc, styles, text, tree, &[c], parent_style));
            } else {
                let built = build(doc, styles, text, tree, c);
                if let (Some(n), Some(name)) = (built, &styles.get(c).grid_area_name) {
                    place_in_area(tree, n, &parent_style.grid_areas, name);
                }
                out.extend(built);
            }
        }
    }
    flush(&mut run, &mut out, text, tree);
    out
}

/// Put a grid item named by `grid-area` on the cells of that area.
fn place_in_area(tree: &mut TaffyTree<NodeCtx>, n: taffy::NodeId, areas: &[Vec<String>], name: &str) {
    let Some((r0, r1, c0, c1)) = area_lines(areas, name) else { return };
    let mut style = tree.style(n).unwrap().clone();
    style.grid_row = taffy::Line { start: taffy::style_helpers::line(r0), end: taffy::style_helpers::line(r1) };
    style.grid_column = taffy::Line { start: taffy::style_helpers::line(c0), end: taffy::style_helpers::line(c1) };
    tree.set_style(n, style).unwrap();
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
    /// Set while a generated box lays out its own text.
    inside: Option<NodeId>,
}

/// Builds and sizes an inline-block, returning its Parley box spec (at index 0).
type BoxMaker<'a> = dyn FnMut(NodeId) -> Option<InlineBoxSpec> + 'a;

fn has_inline_paint(s: &ComputedStyle) -> bool {
    let e = |e: Edges<f32>| e.top > 0.0 || e.right > 0.0 || e.bottom > 0.0 || e.left > 0.0;
    s.background.is_some_and(|c| c.a > 0) || (e(s.border_width) && [s.border_color.top, s.border_color.right, s.border_color.bottom, s.border_color.left].iter().any(|c| c.a > 0)) || e(s.padding)
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
            inside: None,
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
        if styles.is_pseudo(id) {
            self.collect_pseudo(styles, id, deco, make_box);
            return;
        }
        let node = doc.node(id);
        match &node.kind {
            NodeKind::Text(raw) => {
                let collapsed = transform_text(&collapse_whitespace(raw), styles.get(id).text_transform);
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
                        border_style: es.border_style,
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
                for c in kids(doc, styles, id) {
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

    /// A generated box in the flow: an inline with its text, or an inline-block.
    /// `inside` is the box itself when it's building its own contents.
    fn collect_pseudo(&mut self, styles: &StyleMap, id: NodeId, deco: u32, make_box: &mut BoxMaker) {
        let es = styles.get(id);
        let inside = self.inside == Some(id);
        if es.display == Display::None {
            return;
        }
        if !inside && es.display == Display::InlineBlock {
            if let Some(spec) = make_box(id) {
                self.boxes.push(InlineBoxSpec { index: self.text.len(), ..spec });
                self.collapse = false;
            }
            return;
        }
        let origin = styles.origin(id);
        let own = (!inside && has_inline_paint(es)).then(|| {
            self.decos.push(InlineDeco {
                background: es.background,
                border_color: es.border_color,
                border_style: es.border_style,
                border: es.border_width,
                padding: es.padding,
                radius: es.border_radius,
            });
            self.decos.len() - 1
        });
        let deco = own.map_or(deco, |i| i as u32 + 1);
        if own.is_some() {
            self.push_spacer(origin, es, deco, es.padding.left + es.border_width.left);
        }
        let collapsed = transform_text(&collapse_whitespace(styles.content(id)), es.text_transform);
        let piece = if self.collapse { collapsed.strip_prefix(' ').unwrap_or(&collapsed) } else { &collapsed };
        if !piece.is_empty() {
            let start = self.text.len();
            self.text.push_str(piece);
            let mut ts = text_style(es);
            ts.deco = deco;
            self.spans.push((start..self.text.len(), ts));
            let range = start..self.text.len();
            self.owners.push((range.clone(), origin));
            for &outer in self.open.iter().rev() {
                self.owners.push((range.clone(), outer));
            }
            self.collapse = piece.ends_with(' ');
        }
        if own.is_some() {
            self.push_spacer(origin, es, deco, es.padding.right + es.border_width.right);
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
    build_inline_run_in(doc, styles, text, tree, run, block_style, None)
}

/// [`build_inline_run`], where `inside` is a generated box whose own text this is.
fn build_inline_run_in(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &mut TaffyTree<NodeCtx>,
    run: &[NodeId],
    block_style: &ComputedStyle,
    inside: Option<NodeId>,
) -> Option<taffy::NodeId> {
    let mut inline = InlineText::new();
    inline.inside = inside;
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
            let baseline = if !styles.is_pseudo(id) && doc.node(id).tag() == Some("textarea") { None } else { last_baseline(tree, n) };
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
    let spacers = inline.spans.iter().filter(|(_, ts)| ts.spacer).map(|(r, _)| r.clone()).collect();
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
            text: inline.text,
            spacers,
            control: false,
            user_select: block_style.user_select,
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
    styles: &StyleMap,
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
            let bg_image = styles.get(*id).background_url.as_deref().and_then(image_for);
            let transform = box_transform(styles.get(*id), rect);
            out.push(LayoutBox { node: *id, rect, content: BoxContent::Element, subtree_end: 0, scroll: None, image: None, bg_image, transform })
        }
        Some(NodeCtx::Replaced { id, image, .. }) => {
            let bg_image = styles.get(*id).background_url.as_deref().and_then(image_for);
            let transform = box_transform(styles.get(*id), rect);
            out.push(LayoutBox { node: *id, rect, content: BoxContent::Element, subtree_end: 0, scroll: None, image: image.clone(), bg_image, transform })
        }
        Some(NodeCtx::Text { node, layout, align, kids, decos, owners, text, spacers, control, user_select, nowrap }) => {
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
            let (text, spacers, control, user_select) = (text.clone(), spacers.clone(), *control, *user_select);
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
                content: BoxContent::Text(Box::new(TextContent { layout, decos, owners, text, spacers, control, user_select })),
                subtree_end: 0,
                scroll: None,
                image: None,
                bg_image: None,
                transform: None,
            });
            for b in placed {
                let kid = kids[b.id as usize];
                flatten(tree, styles, kid.node, rect.x + b.x + kid.dx, rect.y + b.y + kid.dy, out);
            }
        }
        None => {}
    }

    // Only a node that pushed its own box (at `first`) owns a subtree.
    let pushed_self = matches!(out.get(first), Some(b) if b.subtree_end == 0);
    let mut children = tree.children(n).unwrap();
    // Paint order inside a stacking context: negative `z-index`, then ordinary boxes,
    // then positioned ones (`z-index` auto or 0) in tree order, then positive `z-index`.
    // Only siblings are reordered, so every subtree stays contiguous.
    children.sort_by_key(|c| match tree.get_node_context(*c) {
        Some(NodeCtx::Element(id)) => paint_layer(styles.get(*id)),
        _ => (1, 0),
    });
    for child in children {
        flatten(tree, styles, child, rect.x, rect.y, out);
    }
    if pushed_self {
        out[first].subtree_end = out.len();
    }
}

fn box_transform(s: &ComputedStyle, rect: Rect) -> Option<Affine> {
    (!s.transform.is_empty()).then(|| Affine::from_style(&s.transform, s.transform_origin, rect))
}

/// (layer, z-index) for sorting siblings into paint order.
fn paint_layer(s: &ComputedStyle) -> (u8, i32) {
    let positioned = s.position != Position::Static;
    match s.z_index {
        Some(z) if positioned && z < 0 => (0, z),
        Some(z) if positioned && z > 0 => (3, z),
        _ if positioned || s.opacity < 1.0 || !s.transform.is_empty() => (2, 0),
        _ => (1, 0),
    }
}

fn transform_text(text: &str, t: TextTransform) -> String {
    match t {
        TextTransform::None => text.to_string(),
        TextTransform::Uppercase => text.to_uppercase(),
        TextTransform::Lowercase => text.to_lowercase(),
        TextTransform::Capitalize => {
            let mut start = true;
            text.chars()
                .flat_map(|c| {
                    let up = start && c.is_alphanumeric();
                    start = !c.is_alphanumeric() && c != '\'';
                    if up { c.to_uppercase().collect::<Vec<_>>() } else { vec![c] }
                })
                .collect()
        }
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
        letter_spacing: s.letter_spacing,
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
        // Resolved after the first layout pass (`resolve_calcs`).
        Length::Calc { .. } => Dimension::auto(),
    }
}

fn lpa(l: Length) -> LengthPercentageAuto {
    match l {
        Length::Auto => LengthPercentageAuto::auto(),
        Length::Px(px) => LengthPercentageAuto::length(px),
        Length::Percent(p) => LengthPercentageAuto::percent(p),
        Length::Calc { .. } => LengthPercentageAuto::auto(),
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

fn track_fn(t: Track) -> taffy::TrackSizingFunction {
    let min = match t.min {
        Breadth::Px(p) => taffy::MinTrackSizingFunction::length(p),
        Breadth::Percent(p) => taffy::MinTrackSizingFunction::percent(p),
        Breadth::MinContent => taffy::MinTrackSizingFunction::min_content(),
        Breadth::MaxContent => taffy::MinTrackSizingFunction::max_content(),
        Breadth::Auto | Breadth::Fr(_) => taffy::MinTrackSizingFunction::auto(),
    };
    let max = match t.max {
        Breadth::Px(p) => taffy::MaxTrackSizingFunction::length(p),
        Breadth::Percent(p) => taffy::MaxTrackSizingFunction::percent(p),
        Breadth::Fr(f) => taffy::MaxTrackSizingFunction::fr(f),
        Breadth::MinContent => taffy::MaxTrackSizingFunction::min_content(),
        Breadth::MaxContent => taffy::MaxTrackSizingFunction::max_content(),
        Breadth::Auto => taffy::MaxTrackSizingFunction::auto(),
    };
    taffy::style_helpers::minmax(min, max)
}

fn template_item(item: &TrackItem) -> taffy::GridTemplateComponent<String> {
    match item {
        TrackItem::Track(t) => taffy::GridTemplateComponent::Single(track_fn(*t)),
        TrackItem::Repeat(kind, tracks) => taffy::GridTemplateComponent::Repeat(taffy::GridTemplateRepetition {
            count: match kind {
                TrackRepeat::Count(n) => taffy::RepetitionCount::Count(*n),
                TrackRepeat::AutoFill => taffy::RepetitionCount::AutoFill,
                TrackRepeat::AutoFit => taffy::RepetitionCount::AutoFit,
            },
            tracks: tracks.iter().copied().map(track_fn).collect(),
            line_names: Vec::new(),
        }),
    }
}

fn grid_line(l: GridLine) -> taffy::GridPlacement {
    match l {
        GridLine::Auto => taffy::GridPlacement::Auto,
        GridLine::Line(n) => taffy::style_helpers::line(n),
        GridLine::Span(n) => taffy::style_helpers::span(n),
    }
}

fn placement(p: GridPlace) -> taffy::Line<taffy::GridPlacement> {
    taffy::Line { start: grid_line(p.start), end: grid_line(p.end) }
}

fn taffy_style(s: &ComputedStyle) -> Style {
    Style {
        display: match s.display {
            Display::Flex => taffy::Display::Flex,
            Display::Grid => taffy::Display::Grid,
            Display::None => taffy::Display::None,
            Display::Block | Display::Inline | Display::InlineBlock => taffy::Display::Block,
        },
        position: match s.position {
            Position::Static => taffy::Position::Relative,
            Position::Relative => taffy::Position::Relative,
            Position::Absolute | Position::Fixed => taffy::Position::Absolute,
        },
        inset: if s.position == Position::Static {
            taffy::Rect::auto()
        } else {
            taffy::Rect { top: lpa(s.inset.top), right: lpa(s.inset.right), bottom: lpa(s.inset.bottom), left: lpa(s.inset.left) }
        },
        float: match s.float {
            nile_style::Float::None => taffy::Float::None,
            nile_style::Float::Left => taffy::Float::Left,
            nile_style::Float::Right => taffy::Float::Right,
        },
        clear: match s.clear {
            nile_style::Clear::None => taffy::Clear::None,
            nile_style::Clear::Left => taffy::Clear::Left,
            nile_style::Clear::Right => taffy::Clear::Right,
            nile_style::Clear::Both => taffy::Clear::Both,
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
        align_self: align_items(s.align_self),
        align_content: justify(s.align_content),
        justify_items: align_items(s.justify_items),
        justify_self: align_items(s.justify_self),
        gap: Size { width: LengthPercentage::length(s.column_gap), height: LengthPercentage::length(s.row_gap) },
        grid_template_columns: s.grid_template_columns.iter().map(template_item).collect(),
        grid_template_rows: s.grid_template_rows.iter().map(template_item).collect(),
        grid_auto_columns: s.grid_auto_columns.iter().copied().map(track_fn).collect(),
        grid_auto_rows: s.grid_auto_rows.iter().copied().map(track_fn).collect(),
        grid_auto_flow: match (s.grid_auto_flow.column, s.grid_auto_flow.dense) {
            (false, false) => taffy::GridAutoFlow::Row,
            (true, false) => taffy::GridAutoFlow::Column,
            (false, true) => taffy::GridAutoFlow::RowDense,
            (true, true) => taffy::GridAutoFlow::ColumnDense,
        },
        grid_row: placement(s.grid_row),
        grid_column: placement(s.grid_column),
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
    fn select_shows_its_choice_sizes_to_the_longest_option_and_opens_a_menu() {
        let doc = Document::parse(
            r#"<body style="margin:0"><select id="s"><option>Red</option><option selected>A much longer option</option><option disabled>Blue</option></select>
               <select id="e"></select></body>"#,
        );
        let styles = compute_styles(&doc);
        let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
        assert_eq!(selected_index(&doc, id("s")), Some(1));
        assert_eq!(option_label(&doc, select_options(&doc, id("s"))[2]), "Blue");
        assert_eq!(selected_index(&doc, id("e")), None);

        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 800.0, 400.0);
        let (s, e) = (tree.node_rect(id("s")).unwrap(), tree.node_rect(id("e")).unwrap());
        assert!(s.width > 100.0, "wide enough for the longest option: {s:?}");
        assert!(e.height > 16.0, "an empty select still has a line");
        assert!(tree.menu.is_none());

        let opts = LayoutOptions { menu: Some(MenuRequest { select: id("s"), highlight: Some(0) }), offsets: None, images: None };
        let tree = layout_with(&doc, &styles, &mut text, 800.0, 400.0, &opts);
        let m = tree.menu.as_ref().unwrap();
        assert_eq!((m.rect.x, m.rect.y), (s.x, s.y + s.height), "opens right under the select");
        assert_eq!(m.items.len(), 3);
        assert!(m.items[2].disabled && m.selected == Some(1) && m.highlight == Some(0));
        let mid = |row: usize| m.rect.y + 1.0 + m.row_height * (row as f32 + 0.5);
        assert_eq!(m.row_at(m.rect.x + 5.0, mid(1)), Some(1));
        assert_eq!(m.row_at(m.rect.x + 5.0, mid(2)), Some(2));
        assert_eq!(m.row_at(m.rect.x - 5.0, mid(1)), None);
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

    #[test]
    fn calc_mixing_percent_and_px_resolves_against_the_parent() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div style="width:300px; height:100px">
               <div id="a" style="width:calc(100% - 20px); height:calc(50% + 10px); margin-left:calc(10% + 5px)"></div>
               <div id="b" style="width:calc(50% - 10px)"></div></div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 100.0);
        let rect = |id: &str| tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).unwrap();
        let a = rect("a");
        assert_eq!((a.x, a.width, a.height), (35.0, 280.0, 60.0));
        assert_eq!(rect("b").width, 140.0);
    }

    #[test]
    fn text_transform_and_letter_spacing() {
        let (plain, _) = run_width(r#"<body><p style="margin:0">hello world</p></body>"#);
        let (spaced, _) = run_width(r#"<body><p style="margin:0; letter-spacing: 2px">hello world</p></body>"#);
        assert!((spaced - plain - 22.0).abs() < 1.0, "11 characters x 2px: {plain} -> {spaced}");
        assert_eq!(transform_text("it's a dog", TextTransform::Capitalize), "It's A Dog");
        assert_eq!(transform_text("Straße", TextTransform::Uppercase), "STRASSE");
        assert_eq!(transform_text("ABC", TextTransform::Lowercase), "abc");
    }

    #[test]
    fn position_relative_and_absolute() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div id="box" style="position:relative; width:200px; height:100px">
               <span id="a" style="position:absolute; top:10px; left:20px; width:50px; height:30px"></span>
               <div id="b" style="position:absolute; right:0; bottom:0; width:50px; height:30px"></div>
               <div id="c" style="height:20px"></div>
               <div id="d" style="position:relative; top:5px; left:7px; height:10px"></div></div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 300.0);
        let rect = |id: &str| tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).unwrap();
        let a = rect("a");
        assert_eq!((a.x, a.y, a.width, a.height), (20.0, 10.0, 50.0, 30.0));
        let b = rect("b");
        assert_eq!((b.x, b.y), (150.0, 70.0));
        assert_eq!(rect("c").y, 0.0); // out-of-flow boxes take no space
        let d = rect("d");
        assert_eq!((d.x, d.y), (7.0, 25.0)); // 20 (after c) + 5
    }

    #[test]
    fn generated_content_flows_and_hits_its_element() {
        let doc = Document::parse(
            r#"<style>p::before { content: "ab"; } p::after { content: ""; display: block; height: 10px }
               #m::before { content: ""; display: inline-block; width: 30px; height: 12px }</style>
               <body style="margin:0"><p id="p" style="margin:0">text</p><div id="m" style="width:200px"></div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 300.0, 200.0);
        let p = doc.find(|n| n.attr("id") == Some("p")).unwrap();
        let rect = tree.node_rect(p).unwrap();
        // One text line plus the 10px block ::after.
        assert!(rect.height > 10.0 + 16.0 && rect.height < 10.0 + 16.0 * 1.2 + 2.0, "height {}", rect.height);
        let runs = text_boxes(&tree);
        let BoxContent::Text(c) = &runs[0].content else { unreachable!() };
        assert!(c.owners.iter().all(|(_, n)| *n == p), "generated text belongs to its element");
        assert_eq!(tree.hit_test(2.0, 2.0), Some(p));
        // The generated block is a box with a generated id, but hits and rects map to the element.
        assert!(tree.boxes.iter().any(|b| styles.is_pseudo(b.node)));
        assert_eq!(tree.hit_test(2.0, rect.y + rect.height - 3.0), Some(p));
        // An inline-block ::before gives the (otherwise empty) div a line, like any inline content.
        let m = doc.find(|n| n.attr("id") == Some("m")).unwrap();
        assert_eq!(tree.node_rect(m).unwrap().height, 19.0);
    }

    #[test]
    fn absolute_boxes_use_the_nearest_positioned_ancestor() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div style="height:30px"></div>
               <div id="cb" style="position:relative; margin-left:40px; width:200px; height:100px">
                 <div style="padding:10px"><div id="inner" style="position:absolute; right:0; bottom:0; width:50px; height:20px"></div></div>
               </div>
               <div><div id="page" style="position:absolute; top:5px; left:6px; width:10px; height:10px"></div></div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 300.0);
        let rect = |id: &str| tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).unwrap();
        // Against #cb (x 40, y 30, 200x100), not its static parent.
        let r = rect("inner");
        assert_eq!((r.x, r.y), (40.0 + 200.0 - 50.0, 30.0 + 100.0 - 20.0));
        // No positioned ancestor: the page.
        let p = rect("page");
        assert_eq!((p.x, p.y), (6.0, 5.0));
    }

    #[test]
    fn z_index_orders_painting_and_hit_testing() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div style="position:relative; width:100px; height:100px">
               <div id="hi" style="position:absolute; z-index:5; left:0; top:0; width:50px; height:50px"></div>
               <div id="lo" style="position:absolute; z-index:-1; left:0; top:0; width:50px; height:50px"></div>
               <div id="plain" style="position:absolute; left:0; top:0; width:50px; height:50px"></div>
               <div id="flow" style="height:20px"></div></div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 300.0);
        let node = |id: &str| doc.find(|n| n.attr("id") == Some(id)).unwrap();
        let order: Vec<NodeId> = tree.boxes.iter().map(|b| b.node).collect();
        let at = |id: &str| order.iter().position(|n| *n == node(id)).unwrap();
        assert!(at("lo") < at("flow") && at("flow") < at("plain") && at("plain") < at("hi"));
        assert_eq!(tree.hit_test(5.0, 5.0), Some(node("hi")));
    }

    #[test]
    fn grid_tracks_gaps_spans_and_areas() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div style="display:grid; width:320px; grid-template-columns: 100px 1fr 2fr; grid-auto-rows: 20px; gap: 10px 5px">
               <div id="a"></div><div id="b" style="grid-column: span 2"></div><div id="c"></div></div>
               <div style="display:grid; width:200px; height:100px; grid-template-columns: 50px 1fr; grid-template-rows: 30px 1fr;
                           grid-template-areas: 'h h' 's m'">
               <div id="m" style="grid-area: m"></div><div id="h" style="grid-area: h"></div></div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 300.0);
        let r = |id: &str| tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).unwrap();
        // 320 - 100 - 2 gaps of 5 = 210 split 1:2 -> 70 and 140.
        let (a, b, c) = (r("a"), r("b"), r("c"));
        assert_eq!((a.x, a.width, a.y, a.height), (0.0, 100.0, 0.0, 20.0));
        assert_eq!((b.x, b.width), (105.0, 70.0 + 5.0 + 140.0));
        assert_eq!((c.x, c.y), (0.0, 30.0));
        let (m, h) = (r("m"), r("h"));
        assert_eq!((h.x, h.y, h.width, h.height), (0.0, 50.0, 200.0, 30.0));
        assert_eq!((m.x, m.width, m.height), (50.0, 150.0, 70.0));
    }

    #[test]
    fn floats_sit_side_by_side_and_clear_drops_below() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div style="width:300px"><div id="a" style="float:left; width:50px; height:20px"></div>
               <div id="b" style="float:left; width:60px; height:30px"></div><div id="r" style="float:right; width:40px; height:10px"></div>
               <div id="c" style="clear:both; height:5px"></div></div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 300.0);
        let r = |id: &str| tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).unwrap();
        assert_eq!((r("a").x, r("a").y), (0.0, 0.0));
        assert_eq!((r("b").x, r("b").y), (50.0, 0.0));
        assert_eq!((r("r").x, r("r").y), (260.0, 0.0));
        assert_eq!(r("c").y, 30.0); // below the tallest float
    }

    #[test]
    fn images_size_from_the_picture_or_attributes() {
        const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        // The test picture is 1x1; scale it up with attributes and CSS.
        let html = format!(
            r#"<body style="margin:0"><img id="a" src="{PNG}"><img id="b" src="{PNG}" width="40"><img id="c" src="{PNG}" style="height: 30px">
               <img id="d" src="missing.png" width="20" height="10"><div id="e" style="width:50px; height:50px; background: url({PNG})"></div></body>"#
        );
        let doc = Document::parse(&html);
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let loader = FileImages::new("/nonexistent");
        let options = LayoutOptions { images: Some(&loader), ..Default::default() };
        let tree = layout_with(&doc, &styles, &mut text, 600.0, 300.0, &options);
        let find = |id: &str| doc.find(|n| n.attr("id") == Some(id)).unwrap();
        let size = |id: &str| tree.node_rect(find(id)).map(|r| (r.width, r.height)).unwrap();
        assert_eq!(size("a"), (1.0, 1.0));
        assert_eq!(size("b"), (40.0, 40.0)); // aspect ratio keeps it square
        assert_eq!(size("c"), (30.0, 30.0));
        assert_eq!(size("d"), (20.0, 10.0)); // no picture: attributes still size it
        let boxes = |id: &str| tree.boxes.iter().find(|b| b.node == find(id) && matches!(b.content, BoxContent::Element)).unwrap();
        assert!(boxes("a").image.is_some() && boxes("d").image.is_none());
        assert!(boxes("e").bg_image.is_some());
    }

    #[test]
    fn transforms_move_hits_and_bounding_boxes() {
        let doc = Document::parse(
            r#"<body style="margin:0"><div id="a" style="width:100px; height:20px; transform: rotate(90deg)"></div>
               <div id="b" style="width:50px; height:50px; margin-top:200px; transform: translate(100px, 0) scale(2); transform-origin: 0 0"></div></body>"#,
        );
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 400.0);
        let a = doc.find(|n| n.attr("id") == Some("a")).unwrap();
        let b = doc.find(|n| n.attr("id") == Some("b")).unwrap();
        // Rotated about its center (50, 10): now 20 wide, 100 tall.
        let r = tree.node_rect_scrolled(a, &ScrollOffsets::new()).unwrap();
        assert!((r.x - 40.0).abs() < 0.01 && (r.y + 40.0).abs() < 0.01 && (r.width - 20.0).abs() < 0.01 && (r.height - 100.0).abs() < 0.01, "{r:?}");
        // A point inside the rotated box but outside the original one hits it.
        assert_eq!(tree.hit_test_scrolled(50.0, -30.0, &ScrollOffsets::new()), Some(a));
        assert_ne!(tree.hit_test_scrolled(90.0, 10.0, &ScrollOffsets::new()), Some(a));
        // b: layout rect (0,220,50,50) -> translated and doubled: x 100..200, y 220..320.
        let rb = tree.node_rect_scrolled(b, &ScrollOffsets::new()).unwrap();
        assert_eq!((rb.x, rb.y, rb.width, rb.height), (100.0, 220.0, 100.0, 100.0));
        assert_eq!(tree.hit_test_scrolled(190.0, 310.0, &ScrollOffsets::new()), Some(b));
        assert_ne!(tree.hit_test_scrolled(40.0, 240.0, &ScrollOffsets::new()), Some(b));
    }
}
