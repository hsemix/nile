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
mod pathimg;
mod svg;
mod pagination;
mod paged;
pub use paged::{PagedBox, SHEET_GAP};
pub use pagination::{paginate, paginate_boxes, paginate_tree, paginate_with, Fragments, Repeat};
pub use images::{FileImages, ImageData, ImageLoader, SvgInfo};

pub use nile_dom::NodeId;
use nile_dom::{Document, NodeKind};
use nile_style::{area_lines, Breadth, BorderStyle, GridLine, GridPlace, Position, Track, TrackItem, TrackRepeat, PseudoElement, TextTransform, 
    Align, Color, ComputedStyle, Display, Edges, FlexDirection, Length, OverflowKind, StyleMap, TextAlign, VerticalAlign,
};
use nile_text::{
    break_lines, break_lines_varying, collapse_whitespace, Affinity, Alignment, Cluster, Cursor, InlineBoxSpec, Selection, PositionedLayoutItem, TextEngine,
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

/// Side of a selection handle, in px.
pub const HANDLE: f32 = 8.0;

/// The eight resize handles of a selected box: `(dx, dy)` says which edges they move (-1 left or
/// top, 1 right or bottom, 0 neither) and `rect` is the little square to draw and hit.
pub fn handles_of(r: Rect) -> [((i8, i8), Rect); 8] {
    let (x, y, w, h) = (r.x, r.y, r.width, r.height);
    let sq = |cx: f32, cy: f32| Rect { x: cx - HANDLE / 2.0, y: cy - HANDLE / 2.0, width: HANDLE, height: HANDLE };
    [
        ((-1, -1), sq(x, y)),
        ((0, -1), sq(x + w / 2.0, y)),
        ((1, -1), sq(x + w, y)),
        ((1, 0), sq(x + w, y + h / 2.0)),
        ((1, 1), sq(x + w, y + h)),
        ((0, 1), sq(x + w / 2.0, y + h)),
        ((-1, 1), sq(x, y + h)),
        ((-1, 0), sq(x, y + h / 2.0)),
    ]
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

/// A position in the DOM: a character offset in a text node, or a child index in an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomPoint {
    pub node: NodeId,
    pub offset: usize,
}

/// Where a text node's characters landed in a run's shaped text, for turning DOM positions
/// into caret positions and back.
#[derive(Debug, Clone)]
pub struct TextNodeMap {
    pub node: NodeId,
    /// Byte range of the node's text in the run.
    pub start: usize,
    pub end: usize,
    /// For each character of the node (and one past the last): its byte in the run's text.
    /// Collapsed whitespace points at where it would have been.
    pub offsets: Vec<usize>,
}

/// Where an inline element (or `<br>`) sits in a run's text.
#[derive(Debug, Clone, Copy)]
pub struct ElementMark {
    pub node: NodeId,
    pub start: usize,
    pub end: usize,
}

pub struct TextContent {
    pub layout: TextLayout,
    /// DOM text nodes and inline elements in this run, in order.
    pub nodes: Vec<TextNodeMap>,
    pub marks: Vec<ElementMark>,
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
    /// `text-overflow: ellipsis` on a clipping box: the renderer cuts a too-long line short
    /// and draws "…".
    pub ellipsis: bool,
    /// `-webkit-line-clamp`: only this many lines show, the last ending in an ellipsis.
    pub clamp: Option<u32>,
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
    /// Sciter's `foreground-image`, painted over the box's content.
    pub fg_image: Option<Rc<ImageData>>,
    /// `transform` of the element, as layout coordinates to layout coordinates (the
    /// origin is already folded in). Applies to the box and its subtree.
    pub transform: Option<Affine>,
    /// The container's perspective `(distance, origin x, origin y)` baked into `transform`.
    pub persp: Option<(f32, f32, f32)>,
    /// `position: fixed`: the box and its subtree stay put while the page scrolls.
    pub fixed: bool,
    /// `position: sticky`: the box slides while the page scrolls (see `StickyBox::shift`).
    pub sticky: Option<StickyBox>,
    /// A `<fieldset>` whose `<legend>` sits on its top border: the horizontal span (absolute x) the
    /// border is left out of.
    pub border_gap: Option<(f32, f32)>,
    /// `backface-visibility: hidden`.
    pub backface_hidden: bool,
    /// `pointer-events: none` or `visibility: hidden`: pointer hit testing passes through the box.
    pub inert: bool,
    /// Collapsed table borders: the sides of this cell whose border lost to its neighbour's (the
    /// neighbour paints the shared line, see `resolve_collapsed_borders`).
    pub border_hidden: Edges<bool>,
}

/// What a sticky box needs to compute its shift: its insets and its parent's vertical extent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StickyBox {
    pub top: Option<f32>,
    pub bottom: Option<f32>,
    pub parent_top: f32,
    pub parent_bottom: f32,
}

impl LayoutBox {
    /// How far a sticky box is pushed down (negative: up) for a page scrolled by `scroll`
    /// in a viewport `viewport_h` tall. Zero for other boxes.
    // TODO: only the page's scroll counts, not an `overflow` ancestor's; horizontal insets are ignored.
    pub fn sticky_shift(&self, scroll: f32, viewport_h: f32) -> f32 {
        let Some(s) = self.sticky else { return 0.0 };
        let (y, bottom) = (self.rect.y, self.rect.y + self.rect.height);
        let dy = match (s.top, s.bottom) {
            (Some(top), _) => (scroll + top - y).max(0.0),
            (None, Some(b)) => (scroll + viewport_h - b - bottom).min(0.0),
            _ => 0.0,
        };
        dy.min(s.parent_bottom - bottom).max(s.parent_top - y)
    }
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
    /// Thickness of its scrollbars (`scrollbar-width`, `::-webkit-scrollbar`); 0 hides them.
    pub thick: f32,
}

/// Overlay scrollbar metrics, shared by painting and dragging.
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
pub fn page_thumb(viewport: (f32, f32), content_height: f32, scroll_y: f32, thick: f32) -> Option<Thumb> {
    if thick <= 0.0 {
        return None;
    }
    let track = Rect { x: viewport.0 - thick - SCROLLBAR_EDGE, y: 0.0, width: thick, height: viewport.1 };
    Thumb::new(track, true, viewport.1, content_height, scroll_y)
}

impl ScrollBox {
    /// The scrollbars to show at scroll offsets `(ox, oy)`.
    pub fn thumbs(&self, ox: f32, oy: f32) -> Vec<Thumb> {
        let (c, mut out) = (self.clip, Vec::new());
        if self.thick <= 0.0 {
            return out;
        }
        let thick = self.thick;
        if self.wheel_y {
            let track = Rect { x: c.x + c.width - thick - SCROLLBAR_EDGE, y: c.y, width: thick, height: c.height };
            out.extend(Thumb::new(track, true, c.height, self.content_height, oy));
        }
        if self.wheel_x {
            let track = Rect { x: c.x, y: c.y + c.height - thick - SCROLLBAR_EDGE, width: c.width, height: thick };
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
    /// Inside the paged element `LayoutTree::pages[n]`: box coordinates are flow positions that
    /// appear on that element's sheets.
    pub paged: Option<usize>,
    /// The box (or a container) has `backface-visibility: hidden` and is turned away: it is neither
    /// painted nor hit.
    pub hidden: bool,
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
    /// List item markers by `<li>` node, placed left of the item.
    markers: std::collections::HashMap<NodeId, ListMarker>,
    /// `<select multiple>` / `size > 1` list boxes by node: their rows are drawn inline.
    lists: std::collections::HashMap<NodeId, ListBox>,
    /// Element boxes by node, for `node_rect`.
    index: std::collections::HashMap<NodeId, usize>,
    /// Text node -> (run box index, index in that run's `nodes`); inline element -> (run, mark).
    text_nodes: std::collections::HashMap<NodeId, (usize, usize)>,
    elem_marks: std::collections::HashMap<NodeId, (usize, usize)>,
    /// Generated (`::before`/`::after`) box -> its element, so hits land on the element.
    origins: std::collections::HashMap<NodeId, NodeId>,
    /// How far the page is scrolled; `position: fixed` boxes are shifted by it. Set by the
    /// embedder (`set_page_scroll`) before anything asks for placements.
    page_scroll: f32,
    viewport_h: f32,
    /// Elements shown as sheets of paper (`page-view: paged`).
    pub pages: Vec<PagedBox>,
}

/// Ask for a `<select>`'s drop-down to be laid out under it.
#[derive(Debug, Clone, Copy)]
pub struct MenuRequest {
    pub select: NodeId,
    /// The row under the pointer or keyboard cursor.
    pub highlight: Option<usize>,
    /// How far the list is scrolled, in pixels (clamped when it's laid out).
    pub scroll: f32,
}

/// Text an input method is composing, shown in place of characters `at..end` of the input's
/// value (the selection it will replace; `at == end` at a bare caret).
#[derive(Debug, Clone, PartialEq)]
pub struct Preedit {
    pub node: NodeId,
    pub at: usize,
    pub end: usize,
    pub text: String,
}

/// Inputs to layout beyond the document and styles.
#[derive(Default)]
pub struct LayoutOptions<'a> {
    /// Composition in progress in a text input.
    pub preedit: Option<Preedit>,
    pub menu: Option<MenuRequest>,
    /// The page's scroll position: the drop-down opens upward when it doesn't fit below.
    pub scroll_y: f32,
    /// Scroll offsets, so the menu opens under where its select actually paints.
    pub offsets: Option<&'a ScrollOffsets>,
    /// Where `<img src>` and `url()` backgrounds come from; without one they don't show.
    pub images: Option<&'a dyn ImageLoader>,
}

pub struct MenuItem {
    pub text: TextLayout,
    pub disabled: bool,
    /// An `<optgroup>` heading (bold, not a choice).
    pub header: bool,
    /// Inside an `<optgroup>`.
    pub indent: bool,
    /// The option's own `background`, when the page sets one (`option { background: ... }`).
    pub background: Option<Color>,
}

/// An open `<select>` list in document coordinates.
pub struct MenuBox {
    pub rect: Rect,
    pub row_height: f32,
    pub pad_x: f32,
    pub items: Vec<MenuItem>,
    pub selected: Option<usize>,
    pub highlight: Option<usize>,
    /// Pixels scrolled; the list is taller than its panel when `max_scroll > 0`.
    pub scroll: f32,
    pub max_scroll: f32,
    /// The select's own look, so a styled select opens a list that matches it: its `background`, border
    /// colour, corner radius and `accent-color` (None = the platform palette's).
    pub background: Option<Color>,
    pub border: Option<Color>,
    pub radius: f32,
    pub accent: Option<Color>,
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
        let i = ((y - self.rect.y - 1.0 + self.scroll) / self.row_height).floor();
        (i >= 0.0 && (i as usize) < self.items.len()).then_some(i as usize)
    }
}

/// A list item's marker, positioned in document coordinates.
pub struct ListMarker {
    pub kind: MarkerKind,
    /// Colour from a `::marker` rule; otherwise the item's own (live) colour is used.
    pub color: Option<Color>,
    /// Where the marker goes: the glyph text's box, or the bullet's bounding square.
    pub rect: Rect,
}

pub enum MarkerKind {
    /// A `<summary>` triangle: pointing down when open.
    Disclosure { open: bool },
    Disc,
    Circle,
    Square,
    Text(Box<TextLayout>),
    /// `list-style-image`.
    Image(Rc<ImageData>),
}

/// One row of a [`ListBox`].
pub struct ListItem {
    pub text: TextLayout,
    pub disabled: bool,
    pub selected: bool,
    pub header: bool,
    pub indent: bool,
}

/// The rows of a list-box `<select>`, painted inside its box and scrolled by `scroll`.
pub struct ListBox {
    pub items: Vec<ListItem>,
    pub row_height: f32,
    /// Distance from the box's top edge to the first row (border + padding).
    pub inset_top: f32,
    /// Distance from the box's left edge to the row text.
    pub pad_x: f32,
    /// Height of the visible rows area.
    pub view_height: f32,
    pub scroll: f32,
    pub max_scroll: f32,
}

impl ListBox {
    /// The row at `y` pixels below the box's top edge.
    pub fn row_at(&self, y: f32) -> Option<usize> {
        let i = ((y - self.inset_top + self.scroll) / self.row_height).floor();
        (i >= 0.0 && (i as usize) < self.items.len()).then_some(i as usize)
    }
}

/// How many rows a `<select>` list box shows, or `None` for a drop-down:
/// `multiple` shows `size` rows (4 by default), a `size` above 1 does too.
pub fn list_rows(doc: &Document, select: NodeId) -> Option<usize> {
    let n = doc.node(select);
    if n.tag() != Some("select") {
        return None;
    }
    let size = n.attr("size").and_then(|s| s.trim().parse::<usize>().ok()).filter(|s| *s > 0);
    match (n.attr("multiple").is_some(), size) {
        (true, s) => Some(s.unwrap_or(4)),
        (false, Some(s)) if s > 1 => Some(s),
        _ => None,
    }
}

/// Height of one row in a drop-down or list box.
fn option_row_height(style: &ComputedStyle) -> f32 {
    (style.font_size * style.line_height + 6.0).ceil()
}

/// The rows of a `<select>` in document order: its `<option>`s, and an `<optgroup>`'s heading
/// as a row of its own (a disabled one, so selecting and stepping skip it). Row indices are
/// what the list, the menu and the keyboard use.
pub fn select_options(doc: &Document, select: NodeId) -> Vec<NodeId> {
    doc.descendants(select).into_iter().filter(|n| matches!(doc.node(*n).tag(), Some("option" | "optgroup"))).collect()
}

/// An `<optgroup>` heading row.
pub fn is_group(doc: &Document, row: NodeId) -> bool {
    doc.node(row).tag() == Some("optgroup")
}

/// Whether a row can't be chosen: a heading, a disabled option, or one in a disabled group.
pub fn option_disabled(doc: &Document, row: NodeId) -> bool {
    is_group(doc, row)
        || doc.has_state(row, "disabled")
        || doc.node(row).parent.is_some_and(|p| is_group(doc, p) && doc.node(p).attr("disabled").is_some())
}

/// An option inside an `<optgroup>` is drawn indented.
pub fn option_indented(doc: &Document, row: NodeId) -> bool {
    !is_group(doc, row) && doc.node(row).parent.is_some_and(|p| is_group(doc, p))
}

/// A row's visible text: an option's `label`, else its text with whitespace collapsed; a
/// group's `label`.
pub fn option_label(doc: &Document, option: NodeId) -> String {
    match doc.node(option).attr("label") {
        Some(l) if !l.is_empty() => l.to_string(),
        _ if is_group(doc, option) => String::new(),
        _ => collapse_whitespace(&doc.text_content(option)).trim().to_string(),
    }
}

/// Index of the selected option: the first marked `selected`, else the first
/// enabled one (a single-choice select always has something chosen).
pub fn selected_index(doc: &Document, select: NodeId) -> Option<usize> {
    let options = select_options(doc, select);
    options
        .iter()
        .position(|o| !is_group(doc, *o) && doc.has_state(*o, "selected"))
        .or_else(|| options.iter().position(|o| !option_disabled(doc, *o)))
        .or_else(|| options.iter().position(|o| !is_group(doc, *o)))
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
                thick: styles.scrollbar_look(b.node).thickness,
            };
            self.boxes[i].scroll = Some(scroll);
        }
    }

    /// The scroll box of element `node`, if it clips content.
    pub fn scroll_box(&self, node: NodeId) -> Option<&ScrollBox> {
        self.index.get(&node).and_then(|&i| self.boxes[i].scroll.as_ref())
    }

    /// Where every box paints under `offsets` (parallel to `boxes`).
    /// Bring paint-level state back in step with `styles` after a restyle that changed
    /// nothing `StyleMap::same_layout` cares about: only transforms are baked into boxes.
    pub fn refresh_paint(&mut self, styles: &StyleMap) {
        for b in &mut self.boxes {
            if matches!(b.content, BoxContent::Element) {
                b.transform = box_transform(styles.get(b.node), b.rect, b.persp);
            }
        }
    }

    /// The marker of a list item.
    pub fn marker(&self, node: NodeId) -> Option<&ListMarker> {
        self.markers.get(&node)
    }

    /// The rows of a list-box `<select>`.
    pub fn list_box(&self, select: NodeId) -> Option<&ListBox> {
        self.lists.get(&select)
    }

    /// Tell the tree how far the page is scrolled (see `LayoutTree::page_scroll`).
    pub fn set_page_scroll(&mut self, y: f32) {
        self.page_scroll = y;
    }

    /// A painted point in box `p`'s own coordinates: layout coordinates, or inside a paged element
    /// the flow position shown there. `strict` refuses margins and gaps between the sheets.
    fn to_box(&self, p: &Placement, x: f32, y: f32, strict: bool) -> Option<(f32, f32)> {
        let (lx, ly) = p.to_layout(x, y);
        match p.paged {
            Some(k) => self.pages[k].to_flow(ly, strict).map(|fy| (lx, fy)),
            None => Some((lx, ly)),
        }
    }

    /// A rectangle in the coordinates of box `i` as unscrolled layout coordinates: inside a paged
    /// element, where the flow position appears on its sheets.
    pub fn to_layout_rect(&self, i: usize, r: Rect) -> Rect {
        match self.pages.iter().find(|pb| pb.index < i && i < pb.end) {
            Some(pb) => pb.rect_to_screen(r),
            None => r,
        }
    }

    /// For an element in (or being) a paged element: the room between one sheet's text and the
    /// next sheet's (bottom margin + gap + top margin).
    pub fn page_zone_of(&self, node: NodeId) -> Option<f32> {
        let i = *self.index.get(&node)?;
        let pb = self.pages.iter().find(|pb| pb.index <= i && i < pb.end)?;
        Some(pb.margin[0] + pb.margin[2] + SHEET_GAP)
    }

    /// Where a rectangle of box `p` paints.
    fn painted(&self, p: &Placement, r: Rect) -> Rect {
        let r = match p.paged {
            Some(k) => self.pages[k].rect_to_screen(r),
            None => r,
        };
        p.xf.bounds(r)
    }

    pub fn placements(&self, offsets: &ScrollOffsets) -> Vec<Placement> {
        let mut out = Vec::with_capacity(self.boxes.len());
        let mut stack: Vec<(usize, Placement)> = Vec::new();
        for (i, b) in self.boxes.iter().enumerate() {
            while stack.last().is_some_and(|(end, _)| i >= *end) {
                stack.pop();
            }
            let mut cur = stack.last().map_or(Placement::default(), |(_, p)| *p);
            if b.fixed {
                // Anchored to the viewport: ignore every ancestor's scroll, clip and transform.
                cur = Placement { xf: Affine::translate(0.0, self.page_scroll), clip: None, paged: None, hidden: false };
                stack.push((b.subtree_end.max(i + 1), cur));
            }
            // A transform applies to the box itself and everything inside it.
            let base = match b.sticky_shift(self.page_scroll, self.viewport_h) {
                dy if dy != 0.0 => cur.xf.then_after(Affine::translate(0.0, dy)),
                _ => cur.xf,
            };
            let xf = b.transform.map_or(base, |t| base.then_after(t));
            let own = Placement { xf, clip: cur.clip, paged: cur.paged, hidden: cur.hidden || (b.backface_hidden && xf.faces_away()) };
            out.push(own);
            if let Some(k) = self.pages.iter().position(|pb| pb.index == i) {
                // Everything inside is a flow position on this element's sheets.
                stack.push((self.pages[k].end, Placement { paged: Some(k), ..own }));
                continue;
            }
            if let Some(sb) = &b.scroll {
                let (ox, oy) = offsets.get(&b.node).copied().unwrap_or_default();
                let clip = intersect(own.clip, own.xf.bounds(sb.clip));
                let xf = own.xf.then_after(Affine::translate(-ox, -oy));
                stack.push((b.subtree_end, Placement { xf, clip: Some(clip), paged: own.paged, hidden: own.hidden }));
            } else if b.transform.is_some() || b.sticky.is_some() || own.hidden != cur.hidden {
                stack.push((b.subtree_end.max(i + 1), own));
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
            let Some((lx, ly)) = self.to_box(p, x, y, false) else { continue };
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

    // ---- DOM positions ---------------------------------------------------------------------

    fn run_text(&self, run: usize) -> Option<&TextContent> {
        match &self.boxes.get(run)?.content {
            BoxContent::Text(c) => Some(c),
            _ => None,
        }
    }

    /// The run and byte a node's start (or end) sits at, looking into its descendants.
    fn leaf_position(&self, doc: &Document, node: NodeId, end: bool) -> Option<TextPoint> {
        if let Some(&(run, k)) = self.text_nodes.get(&node) {
            let m = &self.run_text(run)?.nodes[k];
            return Some(TextPoint { run, byte: if end { *m.offsets.last()? } else { m.offsets[0] } });
        }
        if let Some(&(run, k)) = self.elem_marks.get(&node) {
            let m = self.run_text(run)?.marks[k];
            return Some(TextPoint { run, byte: if end { m.end } else { m.start } });
        }
        let kids = &doc.node(node).children;
        if end {
            kids.iter().rev().find_map(|c| self.leaf_position(doc, *c, true))
        } else {
            kids.iter().find_map(|c| self.leaf_position(doc, *c, false))
        }
    }

    /// The run and byte a DOM position is drawn at. `None` for positions with no text near
    /// them (an empty block).
    pub fn text_point_of(&self, doc: &Document, p: DomPoint) -> Option<TextPoint> {
        if let Some(&(run, k)) = self.text_nodes.get(&p.node) {
            let m = &self.run_text(run)?.nodes[k];
            return Some(TextPoint { run, byte: m.offsets[p.offset.min(m.offsets.len() - 1)] });
        }
        if matches!(doc.node(p.node).kind, NodeKind::Text(_)) {
            return None;
        }
        let kids = &doc.node(p.node).children;
        let at = p.offset.min(kids.len());
        kids[at..]
            .iter()
            .find_map(|c| self.leaf_position(doc, *c, false))
            .or_else(|| kids[..at].iter().rev().find_map(|c| self.leaf_position(doc, *c, true)))
    }

    /// The DOM position of a run position: the inverse of [`text_point_of`](Self::text_point_of).
    pub fn dom_point_of(&self, doc: &Document, t: TextPoint) -> Option<DomPoint> {
        let c = self.run_text(t.run)?;
        let inside = c.nodes.iter().find(|n| n.start <= t.byte && t.byte < n.end);
        let node = inside.or_else(|| c.nodes.iter().rev().find(|n| n.end == t.byte && n.start < n.end));
        if let Some(n) = node {
            let i = n.offsets.iter().position(|o| *o >= t.byte).unwrap_or(n.offsets.len() - 1);
            return Some(DomPoint { node: n.node, offset: i.min(n.offsets.len() - 1) });
        }
        // On a <br>: before it, in its parent.
        let br = c.marks.iter().find(|m| m.start == t.byte && doc.node(m.node).tag() == Some("br"))?;
        let parent = doc.node(br.node).parent?;
        let index = doc.node(parent).children.iter().position(|x| *x == br.node)?;
        Some(DomPoint { node: parent, offset: index })
    }

    /// The caret rectangle of a DOM position in layout coordinates (`x`, `y` top-left, `height`)
    /// and the index of the box it is drawn in. Positions in empty elements use the element's
    /// content box.
    pub fn dom_caret_rect(&self, doc: &Document, styles: &StyleMap, p: DomPoint) -> Option<(Rect, usize)> {
        if let Some(t) = self.text_point_of(doc, p) {
            let c = self.run_text(t.run)?;
            let r = self.boxes[t.run].rect;
            let g = Cursor::from_byte_index(&c.layout, t.byte.min(c.text.len()), Affinity::Downstream).geometry(&c.layout, 1.0);
            return Some((Rect { x: r.x + g.x0 as f32, y: r.y + g.y0 as f32, width: 1.0, height: (g.y1 - g.y0) as f32 }, t.run));
        }
        // An empty element: the start of its content box, one line high.
        let mut node = p.node;
        if matches!(doc.node(node).kind, NodeKind::Text(_)) {
            node = doc.node(node).parent?;
        }
        let i = *self.index.get(&node)?;
        let r = self.boxes[i].rect;
        let s = styles.get(node);
        Some((
            Rect {
                x: r.x + s.border_width.left + s.padding.left,
                y: r.y + s.border_width.top + s.padding.top,
                width: 1.0,
                height: s.font_size * s.line_height,
            },
            i,
        ))
    }

    /// A DOM position's caret as `(x, y, height)` in painted coordinates (after scrolling and
    /// transforms), for moving the caret up and down.
    pub fn caret_painted(&self, doc: &Document, styles: &StyleMap, p: DomPoint, offsets: &ScrollOffsets) -> Option<(f32, f32, f32)> {
        let (r, i) = self.dom_caret_rect(doc, styles, p)?;
        let at = self.painted(&self.placements(offsets)[i], r);
        Some((at.x, at.y, r.height))
    }

    /// The DOM position nearest a point in painted coordinates, among selectable text.
    pub fn dom_point_at(&self, doc: &Document, x: f32, y: f32, offsets: &ScrollOffsets) -> Option<DomPoint> {
        let t = self.text_point_at(x, y, offsets)?;
        self.dom_point_of(doc, t)
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
        let (lx, ly) = self.to_box(&p, x, y, false)?;
        Some(Cursor::from_point(&c.layout, lx - b.rect.x, ly - b.rect.y).index())
    }

    /// The word (byte range of `node`'s displayed text) under a point in painted
    /// coordinates, for double-click selection.
    pub fn word_range_at(&self, node: NodeId, x: f32, y: f32, offsets: &ScrollOffsets) -> Option<std::ops::Range<usize>> {
        let (i, b) = self.text_box_of(node)?;
        let BoxContent::Text(c) = &b.content else { return None };
        let p = self.placements(offsets)[i];
        let (lx, ly) = self.to_box(&p, x, y, false)?;
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
    /// The caret of input `node` at byte `at` of its displayed text, in painted coordinates.
    pub fn input_caret_rect(&self, node: NodeId, at: usize, offsets: &ScrollOffsets) -> Option<Rect> {
        let i = self.boxes.iter().position(|b| matches!(&b.content, BoxContent::Text(c) if c.control && c.owners.iter().any(|(_, n)| *n == node)))?;
        let BoxContent::Text(c) = &self.boxes[i].content else { return None };
        let g = Cursor::from_byte_index(&c.layout, at.min(c.text.len()), Affinity::Downstream).geometry(&c.layout, 1.0);
        let r = self.boxes[i].rect;
        let xf = self.placements(offsets)[i].xf;
        let (x, y) = xf.apply(r.x + g.x0 as f32, r.y + g.y0 as f32);
        Some(Rect { x, y, width: (g.x1 - g.x0) as f32, height: (g.y1 - g.y0) as f32 })
    }

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
    /// The element's box and everything inside it as a range of `boxes`.
    pub fn subtree_range(&self, node: NodeId) -> Option<(usize, usize)> {
        let &i = self.index.get(&node)?;
        Some((i, self.boxes[i].subtree_end.max(i + 1)))
    }

    pub fn node_rect(&self, node: NodeId) -> Option<Rect> {
        self.node_rect_with(node, None)
    }

    /// [`LayoutTree::node_rect`] where the element paints: shifted by the scroll
    /// offsets of the containers around it.
    /// An element's own box (layout coordinates) and where it paints: the transform from layout to
    /// painted page coordinates (scrolling, `transform`, sticky) and the clip its containers impose.
    pub fn placement_of(&self, node: NodeId, offsets: &ScrollOffsets) -> Option<(Rect, Placement)> {
        let &i = self.index.get(&node)?;
        Some((self.boxes[i].rect, self.placements(offsets)[i]))
    }

    pub fn node_rect_scrolled(&self, node: NodeId, offsets: &ScrollOffsets) -> Option<Rect> {
        self.node_rect_with(node, Some(&self.placements(offsets)))
    }

    fn node_rect_with(&self, node: NodeId, places: Option<&[Placement]>) -> Option<Rect> {
        let xf = |i: usize, r: Rect| places.map_or_else(|| self.to_layout_rect(i, r), |p| self.painted(&p[i], r));
        if let Some(&i) = self.index.get(&node) {
            return Some(xf(i, self.boxes[i].rect));
        }
        let (mut l, mut t, mut r, mut b) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for (ti, tb) in self.boxes.iter().enumerate() {
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
                    let run = Rect {
                        x: tb.rect.x + gr.offset(),
                        y: tb.rect.y + gr.baseline() - fm.ascent,
                        width: gr.advance(),
                        height: fm.ascent + fm.descent,
                    };
                    let bounds = xf(ti, run);
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
            if p.hidden || b.inert {
                continue;
            }
            if let Some(c) = p.clip {
                if x < c.x || y < c.y || x >= c.x + c.width || y >= c.y + c.height {
                    continue;
                }
            }
            let Some((lx, ly)) = self.to_box(p, x, y, true) else { continue };
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

/// What a paragraph's lines must avoid: the content box it sits in (an x range) and the margin boxes
/// of the floats beside it (`[x0, y0, x1, y1]`, with whether it floats left). Everything is relative
/// to the paragraph's own top-left corner, because a second layout pass can move the whole group.
#[derive(Clone)]
struct FlowSpec {
    x0: f32,
    x1: f32,
    /// A line's height, to test it against the floats.
    line_h: f32,
    floats: Vec<FloatSpec>,
}

/// One float a paragraph wraps around: its margin box, which side, and its `shape-outside` outline
/// (resolved, with the reference box's top-left corner) if it has one.
#[derive(Clone)]
struct FloatSpec {
    rect: [f32; 4],
    left: bool,
    shape: Option<Outline>,
}

/// A `shape-outside` shape: its reference box's top-left corner, the shape, and `shape-margin`.
type Outline = (f32, f32, nile_style::ResolvedShape, f32);

impl FlowSpec {
    /// Where a line whose top is at `y` (from the paragraph's top) can sit: `(left edge, width)`.
    fn slot(&self, y: f32) -> (f32, f32) {
        let (mut left, mut right) = (self.x0, self.x1);
        for fl in &self.floats {
            let f = &fl.rect;
            if f[1] < y + self.line_h && f[3] > y {
                let (mut near, mut far) = (f[0], f[2]);
                if let Some((ox, oy, shape, margin)) = &fl.shape {
                    // The outline's reach across this line (a line the shape misses isn't pushed aside).
                    match shape.span(y - oy, y + self.line_h - oy, *margin) {
                        Some((lo, hi)) => (near, far) = ((ox + lo).max(f[0]), (ox + hi).min(f[2])),
                        None => continue,
                    }
                }
                if fl.left {
                    left = left.max(far);
                } else {
                    right = right.min(near);
                }
            }
        }
        (left, (right - left).max(1.0))
    }
}

/// Lay out text so its lines stop for floats and run full width below them. Taffy gives a text run
/// one slot (the width beside the floats at its top); this finds the paragraphs that sit next to a
/// float, breaks their lines again with a width per line (`FLOW_SPECS`, used by `flatten`), and makes
/// each leaf as tall as its new lines so what follows moves up. True if any height changed.
fn plan_floats(tree: &mut TaffyTree<NodeCtx>, doc: &Document, styles: &StyleMap, root: taffy::NodeId) -> bool {
    FLOW_SPECS.with(|s| s.borrow_mut().clear());
    TEXT_LEAVES.with(|t| t.borrow_mut().clear());
    TEXT_LEAVES.with(|t| t.borrow_mut().clear());
    compute_static_positions(tree, doc, styles, root);
    let mut tmp = Vec::new();
    flatten(tree, styles, root, 0.0, 0.0, &mut tmp);
    let leaves = TEXT_LEAVES.with(|t| std::mem::take(&mut *t.borrow_mut()));
    let px = |l: Length| if let Length::Px(v) = l { v } else { 0.0 };
    // Floats: margin boxes, in paint order.
    let floats: Vec<(usize, FloatSpec)> = tmp
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b.content, BoxContent::Element) && !styles.is_pseudo(b.node))
        .filter_map(|(i, b)| {
            let s = styles.get(b.node);
            let left = match s.float {
                nile_style::Float::Left => true,
                nile_style::Float::Right => false,
                nile_style::Float::None => return None,
            };
            let m = s.margin;
            let shape = s.shape_outside.as_ref().map(|so| {
                // The box the outline's coordinates refer to.
                let (bw, pd) = (s.border_width, s.padding);
                let (mut x0, mut y0, mut x1, mut y1) = (b.rect.x, b.rect.y, b.rect.x + b.rect.width, b.rect.y + b.rect.height);
                let (mut l, mut t, mut r, mut bt) = (0.0, 0.0, 0.0, 0.0);
                match so.reference {
                    nile_style::ShapeBox::MarginBox => (l, t, r, bt) = (-px(m.left), -px(m.top), -px(m.right), -px(m.bottom)),
                    nile_style::ShapeBox::BorderBox => {}
                    nile_style::ShapeBox::PaddingBox => (l, t, r, bt) = (bw.left, bw.top, bw.right, bw.bottom),
                    nile_style::ShapeBox::ContentBox => (l, t, r, bt) = (bw.left + pd.left, bw.top + pd.top, bw.right + pd.right, bw.bottom + pd.bottom),
                }
                x0 += l;
                y0 += t;
                x1 -= r;
                y1 -= bt;
                (x0, y0, so.shape.resolve((x1 - x0).max(0.0), (y1 - y0).max(0.0), s.shape_margin), s.shape_margin)
            });
            Some((i, FloatSpec { rect: [b.rect.x - px(m.left), b.rect.y - px(m.top), b.rect.x + b.rect.width + px(m.right), b.rect.y + b.rect.height + px(m.bottom)], left, shape }))
        })
        .collect();
    if floats.is_empty() {
        return false;
    }
    let mut changed = false;
    for (idx, leaf) in leaves {
        let (BoxContent::Text(c), Some(tb)) = (&tmp[idx].content, tmp.get(idx)) else { continue };
        if c.control || c.layout.lines().count() == 0 {
            continue;
        }
        // The block the text sits in: the nearest element ancestor that isn't inline.
        let mut p = doc.node(tb.node).parent.filter(|_| matches!(doc.node(tb.node).kind, NodeKind::Text(_))).unwrap_or(tb.node);
        while styles.get(p).display == Display::Inline && styles.get(p).float == nile_style::Float::None {
            match doc.node(p).parent {
                Some(up) => p = up,
                None => break,
            }
        }
        let Some(pb) = tmp.iter().find(|b| b.node == p && matches!(b.content, BoxContent::Element) && !styles.is_pseudo(b.node)) else { continue };
        let ps = styles.get(p);
        let (x0, x1) = (pb.rect.x + ps.border_width.left + ps.padding.left, pb.rect.x + pb.rect.width - ps.border_width.right - ps.padding.right);
        let (top, bottom) = (tb.rect.y, tb.rect.y + tb.rect.height);
        // A box that is a float, is positioned out of flow, clips, or is a flex/grid/table/inline
        // container makes its own formatting context: outer floats don't narrow its text.
        let own_context = |n: NodeId| {
            let s = styles.get(n);
            s.float != nile_style::Float::None
                || s.position.out_of_flow()
                || s.overflow_x != OverflowKind::Visible
                || s.overflow_y != OverflowKind::Visible
                || !matches!(s.display, Display::Block | Display::ListItem | Display::Inline)
        };
        // Floats of the block's own parent chain flow around it, unless something between forms a context.
        let reaches = |fp: NodeId| {
            let mut n = Some(p);
            while let Some(cur) = n {
                if cur == fp {
                    return true;
                }
                if own_context(cur) {
                    return false;
                }
                n = doc.node(cur).parent;
            }
            false
        };
        let beside: Vec<FloatSpec> = floats
            .iter()
            .filter(|(fi, f)| *fi < idx && f.rect[3] > top && f.rect[1] < bottom && f.rect[2] > x0 && f.rect[0] < x1)
            .filter(|(fi, ..)| doc.node(tmp[*fi].node).parent.is_some_and(reaches))
            .map(|(_, f)| f.clone())
            .collect();
        if beside.is_empty() {
            continue;
        }
        let lines = c.layout.lines().count() as f32;
        let (ox, oy) = (tb.rect.x, tb.rect.y);
        let floats = beside
            .into_iter()
            .map(|f| FloatSpec { rect: [f.rect[0] - ox, f.rect[1] - oy, f.rect[2] - ox, f.rect[3] - oy], left: f.left, shape: f.shape.map(|(x, y, s, m)| (x - ox, y - oy, s, m)) })
            .collect();
        let spec = FlowSpec { x0: x0 - ox, x1: x1 - ox, line_h: (c.layout.height() / lines).max(1.0), floats };
        let mut probe = c.layout.clone();
        break_lines_varying(&mut probe, spec.x1 - spec.x0, Alignment::Start, |y| {
            let (x, w) = spec.slot(y);
            (x - spec.x0, w)
        });
        let height = probe.height();
        if (height - tb.rect.height).abs() > 0.5 {
            let mut st = tree.style(leaf).unwrap().clone();
            st.size.height = Dimension::length(height);
            tree.set_style(leaf, st).unwrap();
            changed = true;
        }
        FLOW_SPECS.with(|s| s.borrow_mut().insert(leaf, spec));
    }
    changed
}

/// `-webkit-line-clamp` and `text-wrap: balance` for a text leaf.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct TextFlow {
    clamp: Option<u32>,
    balance: bool,
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
        ellipsis: bool,
        nodes: Vec<TextNodeMap>,
        marks: Vec<ElementMark>,
        /// Never wrap (single-line text inputs).
        nowrap: bool,
        flow: TextFlow,
    },
}

fn measure_node(
    inputs: taffy::LayoutInput,
    ctx: Option<&mut NodeCtx>,
    style: &Style,
) -> taffy::LayoutOutput {
    taffy::compute_leaf_layout(inputs, style, |_, _| 0.0, |known, available| match ctx {
        Some(NodeCtx::Text { layout, align, nowrap, flow, .. }) => {
            measure_text(layout, *align, known, available, *nowrap, flow.clamp)
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

/// Loads the page's `@font-face` fonts into the text engine (each source only once).
fn register_fonts(text: &mut TextEngine, styles: &StyleMap, loader: Option<&dyn ImageLoader>) {
    let Some(loader) = loader else { return };
    for face in &styles.font_faces {
        // The first source that is a usable font wins.
        for url in &face.urls {
            if text.has_font(&face.family, url, face.weight, face.italic) {
                break;
            }
            if let Some(bytes) = loader.bytes(url) {
                if text.register_font(&face.family, url, bytes, face.weight, face.italic) {
                    break;
                }
            }
        }
    }
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
    register_fonts(text, styles, options.images);
    let html = doc.find(|n| n.tag() == Some("html")).unwrap_or(Document::ROOT);
    // Absolute boxes with no positioned ancestor use the root as their containing block.
    CONTAINING_BLOCKS.with(|s| s.borrow_mut().push(Vec::new()));
    FIXED_BLOCKS.with(|f| f.borrow_mut().clear());
    HOISTED.with(|h| h.borrow_mut().clear());
    ABS_HANDED.with(|a| a.borrow_mut().clear());
    STATIC_AT.with(|a| a.borrow_mut().clear());
    COLLAPSED.with(|c| c.borrow_mut().clear());
    FLOW_SPECS.with(|s| s.borrow_mut().clear());
    PREEDIT.with(|p| *p.borrow_mut() = options.preedit.clone());
    let was = PARENT_IS_CB.replace(false);
    let root = build(doc, styles, text, &mut tree, html).unwrap_or_else(|| {
        tree.new_leaf_with_context(Style::default(), NodeCtx::Element(html)).unwrap()
    });
    PARENT_IS_CB.set(was);
    for abs in CONTAINING_BLOCKS.with(|s| s.borrow_mut().pop()).unwrap_or_default() {
        tree.add_child(root, abs).unwrap();
    }

    // Fixed boxes share an invisible viewport-sized containing block.
    let fixed = FIXED_BLOCKS.with(|f| std::mem::take(&mut *f.borrow_mut()));
    if !fixed.is_empty() {
        let layer = tree
            .new_with_children(
                Style {
                    display: taffy::Display::Block,
                    position: taffy::Position::Absolute,
                    inset: taffy::Rect { left: LengthPercentageAuto::length(0.0), top: LengthPercentageAuto::length(0.0), right: LengthPercentageAuto::auto(), bottom: LengthPercentageAuto::auto() },
                    size: Size { width: Dimension::length(viewport_width), height: Dimension::length(viewport_height) },
                    ..Style::default()
                },
                &fixed,
            )
            .unwrap();
        tree.add_child(root, layer).unwrap();
    }

    // The root fills the viewport, like a browser's initial containing block.
    let mut root_style = tree.style(root).unwrap().clone();
    root_style.size.width = Dimension::length(viewport_width);
    root_style.min_size.height = LengthPercentageAuto::length(viewport_height);
    tree.set_style(root, root_style).unwrap();
    // `html { height: 100% }` is a percentage of the viewport (the initial containing block), which
    // Taffy only knows as a minimum; give such a root element a definite height so `body` can use it.
    let mut definite_root = false;
    for child in tree.children(root).unwrap_or_default() {
        let Some(NodeCtx::Element(e)) = tree.get_node_context(child) else { continue };
        if let Length::Percent(pct) = styles.get(*e).height {
            let mut st = tree.style(child).unwrap().clone();
            st.size.height = Dimension::length(pct * viewport_height);
            tree.set_style(child, st).unwrap();
            definite_root = true;
        }
    }

    // `page-view: paged` elements take the paper's width and the `@page` margins as padding.
    let paged_nodes = paged::find(&tree, root, styles);
    for (id, tn) in &paged_nodes {
        paged::prepare(&mut tree, *tn, styles, *id);
    }

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

    if styles.styles.iter().any(|s| s.float != nile_style::Float::None) && plan_floats(&mut tree, doc, styles, root) {
        compute(&mut tree);
    }
    compute_static_positions(&tree, doc, styles, root);
    // Paged elements: find where the sheets break, then make each element as tall as its sheets.
    // Starts are kept relative to the element's top, which can move in the second pass.
    type Sheet = (NodeId, Vec<f32>, f32, Vec<Option<Repeat>>, Vec<Option<Repeat>>);
    let mut sheets: Vec<Sheet> = Vec::new();
    if !paged_nodes.is_empty() {
        let page = styles.page_style(0);
        let (mt, mb) = (page.margin[0], page.margin[2]);
        let content_h = (page.size.1 - mt - mb).max(1.0);
        let mut first = Vec::new();
        flatten(&mut tree, styles, root, 0.0, 0.0, &mut first);
        for (id, tn) in &paged_nodes {
            let Some(i) = first.iter().position(|b| b.node == *id && matches!(b.content, BoxContent::Element)) else { continue };
            let top = first[i].rect.y;
            let end = first[i].subtree_end.max(i + 1);
            let content_end = paged::content_end(&first, i);
            let frags = pagination::paginate_with(&first[i + 1..end], i + 1, Some(doc), styles, top + mt, content_end.max(top + mt + 1.0), content_h);
            let height = frags.starts.len() as f32 * page.size.1 + (frags.starts.len() - 1) as f32 * SHEET_GAP;
            paged::set_height(&mut tree, *tn, height, styles, *id);
            let repeats = frags.repeats.into_iter().map(|r| r.map(|r| Repeat { y: r.y - top, ..r })).collect();
            let feet = frags.feet.into_iter().map(|r| r.map(|r| Repeat { y: r.y - top, ..r })).collect();
            sheets.push((*id, frags.starts.iter().map(|s| s - top).collect(), content_end - top, repeats, feet));
        }
        compute(&mut tree);
    }
    let mut boxes = Vec::new();
    flatten(&mut tree, styles, root, 0.0, 0.0, &mut boxes);
    let mut content_height = tree.layout(root).map(|l| l.size.height).unwrap_or(0.0);
    if definite_root {
        // A fixed-height root no longer grows with its content: the page scrolls as far as the
        // content reaches (not into clipped boxes or `fixed` ones).
        let mut i = 0;
        while i < boxes.len() {
            let b = &boxes[i];
            if b.fixed {
                i = b.subtree_end.max(i + 1);
                continue;
            }
            content_height = content_height.max(b.rect.y + b.rect.height);
            i = if b.scroll.is_some() { b.subtree_end.max(i + 1) } else { i + 1 };
        }
    }
    let index = boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b.content, BoxContent::Element) && !styles.is_pseudo(b.node))
        .map(|(i, b)| (b.node, i))
        .collect();
    let origins = styles.origins().clone();
    let mut tree = LayoutTree { pages: Vec::new(), page_scroll: options.scroll_y, viewport_h: viewport_height, boxes, content_height, menu: None, lists: Default::default(), markers: Default::default(), index, text_nodes: Default::default(), elem_marks: Default::default(), origins };
    tree.compute_scroll_boxes(styles);
    for (id, rel, rel_end, rel_repeats, rel_feet) in sheets {
        let Some(&i) = tree.index.get(&id) else { continue };
        let b = &tree.boxes[i];
        let top = b.rect.y;
        let page = styles.page_style(0);
        tree.pages.push(PagedBox {
            node: id,
            index: i,
            end: b.subtree_end.max(i + 1),
            rect: b.rect,
            page_h: page.size.1,
            margin: page.margin,
            starts: rel.iter().map(|s| s + top).collect(),
            content_end: rel_end + top,
            repeats: rel_repeats.into_iter().map(|r| r.map(|r| Repeat { y: r.y + top, ..r })).collect(),
            feet: rel_feet.into_iter().map(|r| r.map(|r| Repeat { y: r.y + top, ..r })).collect(),
        });
    }
    for (i, b) in tree.boxes.iter().enumerate() {
        let BoxContent::Text(c) = &b.content else { continue };
        for (k, n) in c.nodes.iter().enumerate() {
            tree.text_nodes.insert(n.node, (i, k));
        }
        for (k, m) in c.marks.iter().enumerate() {
            tree.elem_marks.insert(m.node, (i, k));
        }
    }
    tree.lists = build_lists(doc, styles, text, &tree, options.offsets);
    tree.markers = build_markers(doc, styles, text, &tree);
    open_legend_gaps(doc, &mut tree.boxes);
    COLLAPSED.with(|c| {
        let c = c.borrow();
        for b in tree.boxes.iter_mut().filter(|b| matches!(b.content, BoxContent::Element)) {
            if let Some(h) = c.get(&b.node) {
                b.border_hidden = *h;
            }
        }
    });
    if let Some(req) = options.menu {
        tree.menu = build_menu(doc, styles, text, &tree, req, options.offsets, (options.scroll_y, viewport_height));
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
    clamp: Option<u32>,
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
        height: known.height.unwrap_or_else(|| clamped_height(layout, clamp)),
    }
}

/// The height of `layout`, cut after `clamp` lines when it has more.
fn clamped_height(layout: &TextLayout, clamp: Option<u32>) -> f32 {
    match clamp {
        Some(n) if n >= 1 && layout.len() > n as usize => layout.lines().nth(n as usize - 1).map_or(layout.height(), |l| l.metrics().block_max_coord),
        _ => layout.height(),
    }
}

thread_local! {
    /// Images the page uses, by the `src`/`url()` text they were requested with.
    static IMAGES: std::cell::RefCell<std::collections::HashMap<String, Rc<ImageData>>> = std::cell::RefCell::default();
    /// Absolutely positioned boxes waiting to be attached to their containing block: one
    /// list per positioned ancestor being built, the root's at the bottom.
    static CONTAINING_BLOCKS: std::cell::RefCell<Vec<Vec<taffy::NodeId>>> = const { std::cell::RefCell::new(Vec::new()) };
    /// `position: fixed` boxes, whose containing block is the viewport.
    /// Boxes `flatten` has set aside to paint at their stacking context's turn.
    /// The composition being laid out (see `LayoutOptions::preedit`).
    static PREEDIT: std::cell::RefCell<Option<Preedit>> = const { std::cell::RefCell::new(None) };
    /// Absolute boxes moved to a containing block above their parent, and where `auto`-inset ones
    /// belong (absolute page coordinates of the border box) once layout has run.
    static ABS_HANDED: std::cell::RefCell<Vec<taffy::NodeId>> = const { std::cell::RefCell::new(Vec::new()) };
    static STATIC_AT: std::cell::RefCell<std::collections::HashMap<taffy::NodeId, StaticAt>> = std::cell::RefCell::default();
    /// Collapsed-border cells' sides that lost the conflict with a neighbour (filled by `build_table`).
    static COLLAPSED: std::cell::RefCell<std::collections::HashMap<NodeId, Edges<bool>>> = std::cell::RefCell::default();
    static HOISTED: std::cell::RefCell<std::collections::HashSet<taffy::NodeId>> = std::cell::RefCell::default();
    static FIXED_BLOCKS: std::cell::RefCell<Vec<taffy::NodeId>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Text beside floats: per text leaf, the floats its lines must avoid (see `plan_floats`).
    static FLOW_SPECS: std::cell::RefCell<std::collections::HashMap<taffy::NodeId, FlowSpec>> = std::cell::RefCell::default();
    /// While planning: the paint-list index of each text leaf `flatten` emitted.
    static TEXT_LEAVES: std::cell::RefCell<Vec<(usize, taffy::NodeId)>> = const { std::cell::RefCell::new(Vec::new()) };
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
// With `auto` insets such a box belongs at its static position; `compute_static_positions`
// finds it after layout.
fn hand_off_absolute(style: &ComputedStyle, n: taffy::NodeId) -> Option<taffy::NodeId> {
    if style.position == Position::Fixed {
        FIXED_BLOCKS.with(|f| f.borrow_mut().push(n));
        return None;
    }
    if style.position.out_of_flow() && !PARENT_IS_CB.get() {
        let taken = CONTAINING_BLOCKS.with(|s| s.borrow_mut().last_mut().map(|list| list.push(n)).is_some());
        if taken {
            ABS_HANDED.with(|a| a.borrow_mut().push(n));
            return None;
        }
    }
    Some(n)
}

/// A box's static position on the axes where its insets are `auto` (absolute page coordinates).
type StaticAt = (Option<f32>, Option<f32>);

/// Where each absolute box with `auto` insets sits when its containing block isn't its parent:
/// at the start of its parent's content box, below the previous element sibling that is in
/// flow (Taffy, which only sees the containing block, would put it at that block's corner).
fn compute_static_positions(tree: &TaffyTree<NodeCtx>, doc: &Document, styles: &StyleMap, root: taffy::NodeId) {
    let handed = ABS_HANDED.with(|a| a.borrow().clone());
    if handed.is_empty() {
        return;
    }
    // DOM element -> its Taffy node.
    let mut by_dom = std::collections::HashMap::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if let Some(NodeCtx::Element(id) | NodeCtx::Replaced { id, .. }) = tree.get_node_context(n) {
            by_dom.insert(*id, n);
        }
        stack.extend(tree.children(n).unwrap_or_default());
    }
    let origin = |mut n: taffy::NodeId| {
        let (mut x, mut y) = (0.0, 0.0);
        loop {
            if let Ok(l) = tree.layout(n) {
                x += l.location.x;
                y += l.location.y;
            }
            match tree.parent(n) {
                Some(p) => n = p,
                None => return (x, y),
            }
        }
    };
    let px = |l: Length| if let Length::Px(v) = l { v } else { 0.0 };
    for n in handed {
        let Some(NodeCtx::Element(id) | NodeCtx::Replaced { id, .. }) = tree.get_node_context(n) else { continue };
        let st = styles.get(*id);
        if styles.is_pseudo(*id) || st.position != Position::Absolute {
            continue;
        }
        let auto_x = st.inset.left == Length::Auto && st.inset.right == Length::Auto;
        let auto_y = st.inset.top == Length::Auto && st.inset.bottom == Length::Auto;
        if !auto_x && !auto_y {
            continue;
        }
        let Some(parent) = doc.node(*id).parent else { continue };
        let Some(&pn) = by_dom.get(&parent) else { continue };
        let ps = styles.get(parent);
        let (ox, oy) = origin(pn);
        let x = ox + ps.border_width.left + ps.padding.left + px(st.margin.left);
        // Below the previous element sibling that is laid out in flow, else the parent's top.
        let before = doc.node(parent).children.iter().take_while(|c| **c != *id).filter_map(|c| by_dom.get(c).map(|t| (*c, *t)));
        let flow_bottom = before
            .filter(|(c, _)| !styles.get(*c).position.out_of_flow() && styles.get(*c).float == nile_style::Float::None && styles.get(*c).display != Display::Inline)
            .last()
            .and_then(|(c, t)| {
                let (_, sy) = origin(t);
                tree.layout(t).ok().map(|l| sy + l.size.height + px(styles.get(c).margin.bottom))
            });
        let y = flow_bottom.unwrap_or(oy + ps.border_width.top + ps.padding.top) + px(st.margin.top);
        STATIC_AT.with(|a| a.borrow_mut().insert(n, (auto_x.then_some(x), auto_y.then_some(y))));
    }
}

/// Ask `loader` for every image the document and styles name.
fn load_images(doc: &Document, styles: &StyleMap, loader: Option<&dyn ImageLoader>) {
    IMAGES.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        // Sciter's `url(path:...)`: drawn from the element's fill and stroke, not loaded.
        for s in &styles.styles {
            for (url, key) in [(&s.background_url, bg_key(s)), (&s.fg_url, fg_key(s))] {
                // Sciter's `url(path:...)`: drawn from the element's fill and stroke, not loaded.
                if let Some(url) = url.as_deref().filter(|u| pathimg::path_of(u).is_some()) {
                    if let std::collections::hash_map::Entry::Vacant(slot) = m.entry(key) {
                        if let Some(img) = pathimg::image(url, s) {
                            slot.insert(Rc::new(img));
                        }
                    }
                }
            }
        }
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
            if let Some(url) = s.background_url.as_deref().filter(|u| pathimg::path_of(u).is_none()) {
                want(url);
            }
            if let Some(url) = s.fg_url.as_deref().filter(|u| pathimg::path_of(u).is_none()) {
                want(url);
            }

            if let Some(url) = &s.list_style_image {
                want(url);
            }
        }
    });
}

fn image_for(src: &str) -> Option<Rc<ImageData>> {
    IMAGES.with(|m| m.borrow().get(src).cloned())
}

/// Keys of `url(path:...)` pictures: the path plus what colours it.
fn path_key(url: &str, s: &ComputedStyle) -> String {
    format!("{url}\u{1}{:?}{:?}{:?}{:?}", s.fill, s.stroke, s.stroke_width, s.color)
}

fn bg_key(s: &ComputedStyle) -> String {
    s.background_url.as_deref().map_or_else(String::new, |u| path_key(u, s))
}

fn fg_key(s: &ComputedStyle) -> String {
    s.fg_url.as_deref().map_or_else(String::new, |u| path_key(u, s))
}

fn bg_image_of(s: &ComputedStyle) -> Option<Rc<ImageData>> {
    let url = s.background_url.as_deref()?;
    image_for(&if pathimg::path_of(url).is_some() { bg_key(s) } else { url.to_string() })
}

fn fg_image_of(s: &ComputedStyle) -> Option<Rc<ImageData>> {
    let url = s.fg_url.as_deref()?;
    image_for(&if pathimg::path_of(url).is_some() { fg_key(s) } else { url.to_string() })
}

/// An inline `<svg>` as a picture: its subtree written out as a document, sized by its
/// `width`/`height`, else its `viewBox`, else the 300 x 150 browsers use.
fn inline_svg(doc: &Document, id: NodeId, style: &ComputedStyle) -> ImageData {
    use std::hash::{Hash, Hasher};
    let node = doc.node(id);
    let source = svg::serialize(doc, id, Some(style.color));
    let css = svg::page_rules(doc, id);
    let length = |name: &str| node.attr(name).and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok()).filter(|v| *v > 0.0);
    let view_box: Option<(f32, f32)> = node.attr("viewBox").and_then(|v| {
        let n: Vec<f32> = v.split([' ', ',']).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect();
        (n.len() == 4 && n[2] > 0.0 && n[3] > 0.0).then(|| (n[2], n[3]))
    });
    let (w, h) = match (length("width"), length("height"), view_box) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some((vw, vh))) => (w, w * vh / vw),
        (None, Some(h), Some((vw, vh))) => (h * vw / vh, h),
        (None, None, Some(vb)) => vb,
        (w, h, _) => (w.unwrap_or(300.0), h.unwrap_or(150.0)),
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    css.hash(&mut hasher);
    let stretch = node.attr("preserveAspectRatio").is_some_and(|v| v.trim().starts_with("none"));
    ImageData { id: hasher.finish() | (1 << 63), bytes: source.into_bytes(), width: w.ceil() as u32, height: h.ceil() as u32, canvas: None, svg: Some(SvgInfo { css, stretch }) }
}

/// An `<img>`: sized by CSS, else its `width`/`height` attributes, else its picture.
fn build_image(doc: &Document, styles: &StyleMap, tree: &mut TaffyTree<NodeCtx>, id: NodeId) -> Option<taffy::NodeId> {
    let node = doc.node(id);
    let style = styles.get(id);
    let canvas = node.tag() == Some("canvas");
    // A canvas is a picture that changes: its box is sized by `width`/`height` (300x150 by default)
    // and the renderer reads its pixels from the drawing surface.
    let image = if node.tag() == Some("svg") {
        Some(Rc::new(inline_svg(doc, id, style)))
    } else if canvas {
        let side = |name: &str, default: u32| node.attr(name).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(default);
        Some(Rc::new(ImageData { id: 0, bytes: Vec::new(), width: side("width", 300), height: side("height", 150), canvas: node.attr("nile-canvas").and_then(|v| v.parse().ok()), svg: None }))
    } else {
        node.attr("src").and_then(image_for)
    };
    let mut ts = taffy_style(style);
    let attr = |name: &str| node.attr(name).and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok());
    let (mut wide, mut high) = (style.width != Length::Auto, style.height != Length::Auto);
    if !wide {
        if let Some(w) = attr("width") {
            ts.size.width = Dimension::length(w);
            wide = true;
        }
    }
    if !high {
        if let Some(h) = attr("height") {
            ts.size.height = Dimension::length(h);
            high = true;
        }
    }
    let size = image.as_ref().map_or((0.0, 0.0), |i| (i.width as f32, i.height as f32));
    // The picture's proportions fill in a missing dimension; two given ones are taken as they are.
    if size.0 > 0.0 && size.1 > 0.0 && !(wide && high) {
        ts.aspect_ratio = Some(size.0 / size.1);
    }
    if let Some(r) = style.aspect_ratio {
        ts.aspect_ratio = Some(r);
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
    if matches!(node.tag(), Some("img" | "canvas" | "svg")) && style.display != Display::None {
        return build_image(doc, styles, tree, id);
    }

    match &node.kind {
        // Text outside any block container (e.g. directly under the document).
        NodeKind::Text(_) => build_inline_run(doc, styles, text, tree, &[id], styles.get(id)),
        NodeKind::Element { .. } | NodeKind::Document => {
            if style.display == Display::None {
                return None;
            }
            if style.display == Display::Table {
                return build_table(doc, styles, text, tree, id);
            }
            let children = with_containing_block(style.position != Position::Static, || {
                if list_rows(doc, id).is_some() {
                    Vec::new()
                } else if is_value_control(node) {
                    build_control_text(doc, styles, text, tree, id).into_iter().collect()
                } else {
                    build_children(doc, styles, text, tree, &ordered(styles, style, kids(doc, styles, id)), style)
                }
            });
            let mut ts = taffy_style(style);
            if node.tag() == Some("frameset") {
                frameset_tracks(doc, styles, id, &mut ts);
            }
            if let Some(rows) = list_rows(doc, id) {
                ts.size.height = Dimension::length(rows as f32 * option_row_height(style));
            }
            if node.tag() == Some("select") && style.width == Length::Auto {
                // Wide enough for the longest option, whichever is showing.
                // A list box has no chevron but needs room for its row padding and scroll bar.
                // The chevron needs room even when the page's own `padding` took the platform's away.
                let extra = if list_rows(doc, id).is_some() { 22.0 } else { (24.0 - style.padding.right).max(0.0) };
                ts.min_size.width = LengthPercentageAuto::length(widest_option(doc, style, text, id) + extra);
            }
            // A button's label sits in the middle of its box, however tall the page makes it.
            // (Not when the author made the button a flex or grid container: then its children are theirs to lay out.)
            let centred = node.tag() == Some("button") || (node.tag() == Some("select") && list_rows(doc, id).is_none());
            let children = if centred && !children.is_empty() && !matches!(style.display, Display::Flex | Display::Grid) {
                ts.display = taffy::Display::Flex;
                ts.flex_direction = taffy::FlexDirection::Column;
                vec![anonymous_aligned(tree, &children, true)?]
            } else {
                children
            };
            let n = tree.new_with_children(ts, &children).ok()?;
            tree.set_node_context(n, Some(NodeCtx::Element(id))).ok()?;
            // `width: fit-content`: size the box on its own at max-content and give it that width,
            // never wider than its container.
            // Buttons shrink to their content even when block-level, as in browsers.
            let button_fit = node.tag() == Some("button") && style.width == Length::Auto && style.star_w == 0.0 && matches!(style.display, Display::Block | Display::Flex);
            if (style.width_fit || button_fit) && matches!(style.display, Display::Block | Display::Flex | Display::ListItem) {
                tree.compute_layout_with_measure(n, Size { width: AvailableSpace::MaxContent, height: AvailableSpace::MaxContent }, |inputs, _id, ctx, st| measure_node(inputs, ctx, st)).ok()?;
                let outer = tree.layout(n).ok()?.size.width;
                let frame = style.padding.left + style.padding.right + style.border_width.left + style.border_width.right;
                let mut st = tree.style(n).ok()?.clone();
                st.size.width = Dimension::length(if style.border_box { outer } else { (outer - frame).max(0.0) });
                if st.max_size.width == LengthPercentageAuto::auto() {
                    st.max_size.width = LengthPercentageAuto::percent(1.0);
                }
                tree.set_style(n, st).ok()?;
            }
            hand_off_absolute(style, n)
        }
    }
}

/// `<input>` types whose `value` is drawn as a line of text.
/// An anonymous box around `children` for a column-flex parent: auto margins push it to the
/// middle (or, without `middle`, the bottom) of the parent's height. Anonymous nodes have no
/// `NodeCtx`, which `flatten` skips over while still laying out their children.
fn anonymous_aligned(tree: &mut TaffyTree<NodeCtx>, children: &[taffy::NodeId], middle: bool) -> Option<taffy::NodeId> {
    let auto = LengthPercentageAuto::auto();
    let zero = LengthPercentageAuto::length(0.0);
    let style = Style {
        display: taffy::Display::Flex,
        flex_direction: taffy::FlexDirection::Column,
        margin: taffy::Rect { top: auto, bottom: if middle { auto } else { zero }, left: zero, right: zero },
        flex_shrink: 0.0,
        size: Size { width: Dimension::percent(1.0), height: Dimension::auto() },
        ..Default::default()
    };
    tree.new_with_children(style, children).ok()
}

/// A table laid out as a grid: columns and rows are auto-sized tracks, each cell is a grid
/// item spanning `colspan` x `rowspan` tracks, and each `<tr>` is an empty item behind its
/// cells so row backgrounds and borders still paint.
fn build_table(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &mut TaffyTree<NodeCtx>,
    id: NodeId,
) -> Option<taffy::NodeId> {
    let style = styles.get(id);
    let shown = |n: &NodeId| styles.get(*n).display != Display::None;
    let with_display = |parent: NodeId, d: Display| -> Vec<NodeId> {
        kids(doc, styles, parent).into_iter().filter(|n| shown(n) && !styles.is_pseudo(*n) && styles.get(*n).display == d).collect()
    };
    // Rows in visual order: header groups first, footers last.
    let tag = |n: NodeId| doc.node(n).tag().map(str::to_string);
    let mut head = Vec::new();
    let mut body = Vec::new();
    let mut foot = Vec::new();
    let mut captions = Vec::new();
    // Row groups that paint something: (group, rows in its section before it, its row count).
    let mut groups: Vec<(NodeId, u8, usize, usize)> = Vec::new();
    for c in kids(doc, styles, id).into_iter().filter(|n| shown(n) && !styles.is_pseudo(*n)) {
        match styles.get(c).display {
            Display::TableRow => body.push(c),
            Display::TableRowGroup => {
                let rows = with_display(c, Display::TableRow);
                let (section, list) = match tag(c).as_deref() {
                    Some("thead") => (0, &mut head),
                    Some("tfoot") => (2, &mut foot),
                    _ => (1, &mut body),
                };
                if !rows.is_empty() && paints_box(styles.get(c)) {
                    groups.push((c, section, list.len(), rows.len()));
                }
                list.extend(rows);
            }
            Display::TableCaption => captions.push(c),
            _ => {}
        }
    }
    let section_start = [0, head.len(), head.len() + body.len()];
    let rows: Vec<NodeId> = head.into_iter().chain(body).chain(foot).collect();

    // Give every cell its grid rectangle, skipping slots taken by earlier row spans.
    struct Cell {
        node: NodeId,
        row: usize,
        col: usize,
        rows: usize,
        cols: usize,
    }
    let span = |n: NodeId, attr: &str| doc.node(n).attr(attr).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, 1000);
    let mut taken: Vec<Vec<bool>> = vec![Vec::new(); rows.len()];
    let mut cells = Vec::new();
    let mut columns = 0;
    for (r, row) in rows.iter().enumerate() {
        let mut c = 0;
        for cell in with_display(*row, Display::TableCell) {
            while taken[r].get(c).copied().unwrap_or(false) {
                c += 1;
            }
            let (cs, rs) = (span(cell, "colspan"), span(cell, "rowspan").min(rows.len() - r));
            for row_taken in &mut taken[r..r + rs] {
                if row_taken.len() < c + cs {
                    row_taken.resize(c + cs, false);
                }
                row_taken[c..c + cs].fill(true);
            }
            cells.push(Cell { node: cell, row: r, col: c, rows: rs, cols: cs });
            columns = columns.max(c + cs);
            c += cs;
        }
    }
    let columns = columns.max(1);
    if style.border_collapse {
        let grid: Vec<(NodeId, usize, usize, usize, usize)> = cells.iter().map(|c| (c.node, c.row, c.col, c.rows, c.cols)).collect();
        resolve_collapsed_borders(styles, &grid);
    }
    let bottom_caption = |c: &NodeId| styles.get(*c).caption_bottom;
    let first_row = captions.iter().filter(|c| !bottom_caption(c)).count();
    let line = |n: usize, span: usize| taffy::Line {
        start: taffy::style_helpers::line((n + 1) as i16),
        end: taffy::style_helpers::span(span as u16),
    };

    let mut items = Vec::new();
    // Captions sit above the grid, each on its own row.
    let (mut above, mut below) = (0, 0);
    for cap in &captions {
        if let Some(n) = build(doc, styles, text, tree, *cap) {
            let mut st = tree.style(n).ok()?.clone();
            let at = if bottom_caption(cap) {
                below += 1;
                first_row + rows.len() + below - 1
            } else {
                above += 1;
                above - 1
            };
            st.grid_row = line(at, 1);
            st.grid_column = line(0, columns);
            tree.set_style(n, st).ok()?;
            items.push(n);
        }
    }
    // A row group has no box of its own; one that paints (background, border, shadow) gets an
    // empty item spanning its rows, behind them.
    for (group, section, start, len) in &groups {
        let mut st = taffy_style(styles.get(*group));
        st.display = taffy::Display::Block;
        st.margin = taffy::Rect::zero();
        st.size = Size::auto();
        st.grid_row = taffy::Line {
            start: taffy::style_helpers::line((first_row + section_start[*section as usize] + start + 1) as i16),
            end: taffy::style_helpers::span(*len as u16),
        };
        st.grid_column = line(0, columns);
        items.push(tree.new_leaf_with_context(st, NodeCtx::Element(*group)).ok()?);
    }
    for (r, row) in rows.iter().enumerate() {
        let rs = styles.get(*row);
        let mut st = taffy_style(rs);
        st.display = taffy::Display::Block;
        st.margin = taffy::Rect::zero();
        st.size = Size::auto();
        st.grid_row = line(first_row + r, 1);
        st.grid_column = line(0, columns);
        let n = tree.new_leaf_with_context(st, NodeCtx::Element(*row)).ok()?;
        items.push(n);
    }
    for cell in &cells {
        let Some(n) = build(doc, styles, text, tree, cell.node) else { continue };
        let cs = styles.get(cell.node);
        let mut st = tree.style(n).ok()?.clone();
        st.display = taffy::Display::Block;
        st.grid_row = line(first_row + cell.row, cell.rows);
        st.grid_column = line(cell.col, cell.cols);
        st.margin = taffy::Rect::zero();
        // Collapsed borders are shared: overlap each cell's top/left edge with its neighbour's.
        if style.border_collapse {
            if cell.col > 0 {
                st.margin.left = LengthPercentageAuto::length(-cs.border_width.left);
            }
            if cell.row > 0 {
                st.margin.top = LengthPercentageAuto::length(-cs.border_width.top);
            }
        }
        let attr_align = doc.node(cell.node).attr("valign").or_else(|| doc.node(rows[cell.row]).attr("valign"));
        // The `valign` attribute is a presentational hint; it beats the UA default.
        let v = match attr_align.map(str::to_ascii_lowercase).as_deref() {
            Some("top") => VerticalAlign::Top,
            Some("bottom") => VerticalAlign::Bottom,
            Some("middle") => VerticalAlign::Middle,
            _ => cs.vertical_align,
        };
        // The cell box always fills its slot; its content sits at the top, or is pushed to the
        // middle or bottom by auto margins on a wrapper inside a column flex cell.
        if matches!(v, VerticalAlign::Middle | VerticalAlign::Bottom) {
            let inner = tree.children(n).ok()?;
            let wrapper = anonymous_aligned(tree, &inner, v == VerticalAlign::Middle)?;
            tree.set_children(n, &[wrapper]).ok()?;
            st.display = taffy::Display::Flex;
            st.flex_direction = taffy::FlexDirection::Column;
        }
        tree.set_style(n, st).ok()?;
        items.push(n);
    }

    let mut ts = taffy_style(style);
    let fixed = style.table_fixed && style.width != Length::Auto;
    if fixed {
        // Column widths come from `<col>`s, else the first row's cells; the rest share what's left.
        let breadth = |l: Length| match l {
            Length::Px(p) => Some(Breadth::Px(p)),
            Length::Percent(p) => Some(Breadth::Percent(p)),
            _ => None,
        };
        let attr_width = |n: NodeId| {
            doc.node(n).attr("width").and_then(|w| {
                let w = w.trim();
                match w.strip_suffix('%') {
                    Some(p) => p.trim().parse::<f32>().ok().map(|p| Breadth::Percent(p / 100.0)),
                    None => w.trim_end_matches("px").trim().parse::<f32>().ok().map(Breadth::Px),
                }
            })
        };
        let mut widths: Vec<Option<Breadth>> = Vec::new();
        let push_col = |col: NodeId, widths: &mut Vec<Option<Breadth>>| {
            let w = breadth(styles.get(col).width).or_else(|| attr_width(col));
            widths.extend(std::iter::repeat_n(w, span(col, "span").min(columns)));
        };
        for c in kids(doc, styles, id) {
            match doc.node(c).tag() {
                Some("colgroup") => {
                    for col in kids(doc, styles, c).into_iter().filter(|k| doc.node(*k).tag() == Some("col")) {
                        push_col(col, &mut widths);
                    }
                }
                Some("col") => push_col(c, &mut widths),
                _ => {}
            }
        }
        widths.resize(columns, None);
        for cell in cells.iter().filter(|c| c.row == 0) {
            let w = breadth(styles.get(cell.node).width).or_else(|| attr_width(cell.node));
            let end = (cell.col + cell.cols).min(columns);
            for slot in widths[cell.col..end].iter_mut().filter(|s| s.is_none()) {
                *slot = w.map(|b| match b {
                    Breadth::Px(p) => Breadth::Px(p / cell.cols as f32),
                    Breadth::Percent(p) => Breadth::Percent(p / cell.cols as f32),
                    other => other,
                });
            }
        }
        ts.grid_template_columns = widths
            .iter()
            .map(|w| {
                let t = match w {
                    Some(b) => Track { min: *b, max: *b },
                    None => Track { min: Breadth::Px(0.0), max: Breadth::Fr(1.0) },
                };
                taffy::GridTemplateComponent::Single(track_fn(t))
            })
            .collect();
        // Cells fill their column instead of asking for their own width.
        for cell in &cells {
            if let Some(n) = items.iter().copied().find(|n| matches!(tree.get_node_context(*n), Some(NodeCtx::Element(e)) if *e == cell.node)) {
                let mut cst = tree.style(n).ok()?.clone();
                cst.size.width = Dimension::auto();
                tree.set_style(n, cst).ok()?;
            }
        }
    }
    if !fixed && style.width != Length::Auto && ts.grid_template_columns.is_empty() {
        // Automatic layout in a table of a given width: the free space is shared in proportion to
        // what each column wants (its widest cell's text on one line), not equally.
        let mut wants = vec![0.0f32; columns];
        let mut ok = true;
        for cell in cells.iter().filter(|c| c.cols == 1) {
            let cs = styles.get(cell.node);
            let own = match cs.width {
                Length::Px(w) => w,
                Length::Percent(_) => {
                    ok = false;
                    0.0
                }
                _ => {
                    let raw = doc.text_content(cell.node);
                    let line = if matches!(cs.white_space, nile_style::WhiteSpace::Pre | nile_style::WhiteSpace::PreWrap) { raw } else { raw.split_whitespace().collect::<Vec<_>>().join(" ") };
                    let mut shaped = text.shape(&line, &text_style(cs), 1.0);
                    nile_text::break_lines(&mut shaped, None, nile_text::Alignment::Start);
                    shaped.width() + cs.padding.left + cs.padding.right + cs.border_width.left + cs.border_width.right
                }
            };
            wants[cell.col] = wants[cell.col].max(own);
        }
        if ok && wants.iter().any(|w| *w > 0.0) {
            ts.grid_template_columns = wants
                .iter()
                .map(|w| taffy::GridTemplateComponent::Single(track_fn(Track { min: Breadth::Auto, max: Breadth::Fr(w.max(1.0)) })))
                .collect();
        }
    }
    ts.grid_auto_columns = vec![taffy::style_helpers::auto()];
    ts.grid_auto_rows = vec![taffy::style_helpers::auto()];
    if !style.border_collapse {
        let (h, v) = style.border_spacing;
        ts.gap = Size { width: LengthPercentage::length(h), height: LengthPercentage::length(v) };
        let p = style.padding;
        ts.padding = taffy::Rect {
            left: LengthPercentage::length(p.left + h),
            right: LengthPercentage::length(p.right + h),
            top: LengthPercentage::length(p.top + v),
            bottom: LengthPercentage::length(p.bottom + v),
        };
    } else {
        ts.gap = Size::zero();
    }
    let n = tree.new_with_children(ts, &items).ok()?;
    tree.set_node_context(n, Some(NodeCtx::Element(id))).ok()?;
    hand_off_absolute(style, n)
}

/// How strongly a border claims a shared edge in a collapsed table (CSS 2.1 17.6.2.1): wider wins,
/// then solid over dashed over dotted; none never wins.
fn border_strength(width: f32, style: BorderStyle) -> (u32, u8) {
    let rank = match style {
        BorderStyle::None => return (0, 0),
        BorderStyle::Solid => 3,
        BorderStyle::Dashed => 2,
        BorderStyle::Dotted => 1,
    };
    if width > 0.0 { ((width * 100.0) as u32, rank) } else { (0, 0) }
}

/// Collapsed borders are shared by neighbouring cells, which each carry their own copy. On every
/// shared edge the stronger border keeps its line and the other cell leaves its side out
/// (`LayoutBox::border_hidden`); on a tie the left or upper cell wins. A side facing several
/// neighbours (spans) is left out only when it loses to all of them. `grid` is
/// `(cell, row, col, rows, cols)`.
fn resolve_collapsed_borders(styles: &StyleMap, grid: &[(NodeId, usize, usize, usize, usize)]) {
    let side = |n: NodeId, vertical_edge: bool, far: bool| {
        let s = styles.get(n);
        let (w, st) = match (vertical_edge, far) {
            (true, true) => (s.border_width.right, s.border_style.right),
            (true, false) => (s.border_width.left, s.border_style.left),
            (false, true) => (s.border_width.bottom, s.border_style.bottom),
            (false, false) => (s.border_width.top, s.border_style.top),
        };
        border_strength(w, st)
    };
    // Per cell side: (contests, losses).
    let mut tally: std::collections::HashMap<(NodeId, u8), (u32, u32)> = std::collections::HashMap::new();
    let overlaps = |a: usize, alen: usize, b: usize, blen: usize| a < b + blen && b < a + alen;
    for &(a, ar, ac, arows, acols) in grid {
        for &(b, br, bc, brows, bcols) in grid {
            // `b` right of `a`: a's right edge against b's left edge; below: bottom against top.
            let pairs = [
                (bc == ac + acols && overlaps(ar, arows, br, brows), true, 1u8, 3u8),
                (br == ar + arows && overlaps(ac, acols, bc, bcols), false, 2u8, 0u8),
            ];
            for (touching, vertical, a_side, b_side) in pairs {
                if !touching {
                    continue;
                }
                let (sa, sb) = (side(a, vertical, true), side(b, vertical, false));
                let a_loses = sb > sa;
                let b_loses = sa >= sb;
                let ea = tally.entry((a, a_side)).or_default();
                ea.0 += 1;
                ea.1 += u32::from(a_loses);
                let eb = tally.entry((b, b_side)).or_default();
                eb.0 += 1;
                eb.1 += u32::from(b_loses);
            }
        }
    }
    COLLAPSED.with(|c| {
        let mut c = c.borrow_mut();
        for ((cell, side), (total, lost)) in tally {
            if total > 0 && lost == total {
                let e = c.entry(cell).or_default();
                match side {
                    0 => e.top = true,
                    1 => e.right = true,
                    2 => e.bottom = true,
                    _ => e.left = true,
                }
            }
        }
    });
}

/// A fieldset's legend sits on its top border: leave the border out behind it.
fn open_legend_gaps(doc: &Document, boxes: &mut [LayoutBox]) {
    for i in 0..boxes.len() {
        let node = boxes[i].node;
        if !matches!(boxes[i].content, BoxContent::Element) || node.0 >= doc.node_count() || doc.node(node).tag() != Some("fieldset") {
            continue;
        }
        let end = boxes[i].subtree_end.clamp(i + 1, boxes.len());
        let legend = boxes[i + 1..end].iter().find(|b| matches!(b.content, BoxContent::Element) && b.node.0 < doc.node_count() && doc.node(b.node).tag() == Some("legend") && doc.node(b.node).parent == Some(node));
        if let Some(l) = legend {
            boxes[i].border_gap = Some((l.rect.x - 1.0, l.rect.x + l.rect.width + 1.0));
        }
    }
}

/// Whether an element's own style paints a background, border or shadow.
fn paints_box(s: &ComputedStyle) -> bool {
    s.background.is_some_and(|c| c.a > 0) || s.background_image.is_some() || s.background_url.is_some() || !s.box_shadow.is_empty() || s.border_width != Edges::default()
}

/// The children of `id` with its generated `::before` and `::after` boxes around them.
fn is_inert(s: &ComputedStyle) -> bool {
    !s.pointer_events || !s.visible
}

/// The children of a flex or grid container in `order` (stable, so equal orders keep the
/// document order).
fn ordered(styles: &StyleMap, container: &ComputedStyle, mut ids: Vec<NodeId>) -> Vec<NodeId> {
    if matches!(container.display, Display::Flex | Display::Grid) && ids.iter().any(|n| styles.get(*n).order != 0) {
        ids.sort_by_key(|n| styles.get(*n).order);
    }
    ids
}

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
        && !matches!(node.attr("type").map(str::to_ascii_lowercase).as_deref(), Some("checkbox" | "radio" | "hidden" | "image" | "range"))
}

fn is_password(node: &nile_dom::Node) -> bool {
    node.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("password"))
}

thread_local! {
    /// `nile-password-char` per password input, keyed by (document address, node): recorded when the
    /// input is laid out so `caret_byte`/`caret_char`, which only see the document, agree with it.
    static MASKS: std::cell::RefCell<std::collections::HashMap<(usize, NodeId), char>> = std::cell::RefCell::default();
}

/// The character a password input shows for each typed one.
fn mask_char(doc: &Document, id: NodeId) -> char {
    MASKS.with(|m| m.borrow().get(&(doc as *const Document as usize, id)).copied()).unwrap_or('\u{2022}')
}

/// The current value of a form control: the `value` attribute, or for a
/// `<textarea>` without one its initial text content.
pub fn control_value(doc: &Document, id: NodeId) -> String {
    let n = doc.node(id);
    match n.attr("value") {
        Some(v) => v.to_string(),
        None if n.tag() == Some("textarea") => doc.text_content(id),
        None if n.tag() == Some("input") && n.attr("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("color")) => "#000000".into(),
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
    if node.tag() == Some("input") && node.attr("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("file")) {
        let names: Vec<&str> = node.attr("nile-files").unwrap_or("").lines().filter(|p| !p.is_empty()).map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p)).collect();
        let shown = match names.len() {
            0 => "No file chosen".to_string(),
            1 => names[0].to_string(),
            n => format!("{n} files"),
        };
        return (format!("Choose File  {shown}"), false);
    }
    let multiline = node.tag() == Some("textarea");
    let value = control_value(doc, id);
    let composing = PREEDIT.with(|p| p.borrow().clone()).filter(|p| p.node == id);
    if let Some(p) = &composing {
        // Composing: the in-progress text goes at the caret; a password hides only what's typed.
        let chars: Vec<char> = value.chars().collect();
        let at = p.at.min(chars.len());
        let end = p.end.clamp(at, chars.len());
        let hide = |s: &[char]| if is_password(node) { mask_char(doc, id).to_string().repeat(s.len()) } else { s.iter().collect() };
        return (format!("{}{}{}", hide(&chars[..at]), p.text, hide(&chars[end..])).replace('\n', if multiline { "\n" } else { " " }), false);
    }
    if value.is_empty() {
        let ph = node.attr("placeholder").unwrap_or("");
        let ph = if multiline { ph.to_string() } else { ph.replace('\n', " ") };
        let is_placeholder = !ph.is_empty();
        return (if is_placeholder { ph } else { "\u{200b}".into() }, is_placeholder);
    }
    if is_password(node) {
        (mask_char(doc, id).to_string().repeat(value.chars().count()), false)
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
        return char_idx.min(value.chars().count()) * mask_char(doc, node).len_utf8();
    }
    value.char_indices().nth(char_idx).map_or(value.len(), |(i, _)| i)
}

/// Inverse of [`caret_byte`].
pub fn caret_char(doc: &Document, node: NodeId, display_byte: usize) -> usize {
    let n = doc.node(node);
    let value = control_value(doc, node);
    if is_password(n) {
        return (display_byte / mask_char(doc, node).len_utf8()).min(value.chars().count());
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

fn alpha(mut n: i64, upper: bool) -> String {
    if n <= 0 {
        return n.to_string();
    }
    let base = if upper { b'A' } else { b'a' };
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(base + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn roman(n: i64, upper: bool) -> String {
    if !(1..4000).contains(&n) {
        return n.to_string();
    }
    let table = [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
    let (mut n, mut out) = (n, String::new());
    for (v, r) in table {
        while n >= v {
            out.push_str(r);
            n -= v;
        }
    }
    if upper { out.to_uppercase() } else { out }
}

/// The text of a counter-style marker for item number `n`, or `None` for bullets and `none`.
/// Room an `inside` marker takes at the start of its item: its shape plus a gap. The text
/// of a numbered marker (or a `::marker { content }`) isn't known when styles become Taffy
/// boxes, so those get an estimate.
fn inside_marker_width(s: &ComputedStyle) -> f32 {
    use nile_style::ListStyleType as L;
    if s.list_style_image.as_deref().and_then(image_for).is_some() {
        return s.font_size * 1.6;
    }
    match s.list_style_type {
        L::None | L::DisclosureClosed | L::DisclosureOpen => 0.0,
        L::Disc | L::Circle | L::Square => (s.font_size * 0.34).round().max(3.0) + s.font_size * 0.5,
        _ => s.font_size * 1.9,
    }
}

fn marker_text(t: nile_style::ListStyleType, n: i64) -> Option<String> {
    use nile_style::ListStyleType as L;
    Some(match t {
        L::Decimal => format!("{n}."),
        L::DecimalLeadingZero => format!("{n:02}."),
        L::LowerAlpha => format!("{}.", alpha(n, false)),
        L::UpperAlpha => format!("{}.", alpha(n, true)),
        L::LowerRoman => format!("{}.", roman(n, false)),
        L::UpperRoman => format!("{}.", roman(n, true)),
        L::LowerGreek => {
            let g: Vec<char> = "αβγδεζηθικλμνξοπρστυφχψω".chars().collect();
            format!("{}.", if n >= 1 { g[(n as usize - 1) % g.len()].to_string() } else { n.to_string() })
        }
        L::Disc | L::Circle | L::Square | L::DisclosureClosed | L::DisclosureOpen | L::None => return None,
    })
}

/// Numbers for each `display: list-item` child of `parent`: `start`, `reversed` and `value`
/// on the list and its items.
fn list_numbers(doc: &Document, styles: &StyleMap, parent: NodeId) -> std::collections::HashMap<NodeId, i64> {
    let items: Vec<NodeId> = doc.node(parent).children.iter().copied().filter(|c| styles.get(*c).display == Display::ListItem).collect();
    let attr = |n: NodeId, name: &str| doc.node(n).attr(name).and_then(|v| v.trim().parse::<i64>().ok());
    let reversed = doc.node(parent).attr("reversed").is_some();
    let step = if reversed { -1 } else { 1 };
    let mut next = attr(parent, "start").unwrap_or(if reversed { items.len() as i64 } else { 1 });
    let mut out = std::collections::HashMap::new();
    for item in items {
        let n = attr(item, "value").unwrap_or(next);
        out.insert(item, n);
        next = n + step;
    }
    out
}

/// A marker for every `<li>` (display: list-item) whose `list-style-type` isn't `none`.
fn build_markers(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &LayoutTree,
) -> std::collections::HashMap<NodeId, ListMarker> {
    let mut out = std::collections::HashMap::new();
    let mut numbers: std::collections::HashMap<NodeId, std::collections::HashMap<NodeId, i64>> = Default::default();
    for b in &tree.boxes {
        if !matches!(b.content, BoxContent::Element) || styles.is_pseudo(b.node) {
            continue;
        }
        let style = styles.get(b.node);
        // A `::marker` rule can restyle the marker, replace it (`content`) or hide it.
        let ms = styles.marker(b.node);
        let content = ms.and_then(|m| m.content.as_deref());
        let image = style.list_style_image.as_deref().and_then(image_for).filter(|i| i.width > 0 && i.height > 0);
        if style.display != Display::ListItem || (style.list_style_type == nile_style::ListStyleType::None && image.is_none() && content.is_none_or(str::is_empty)) || content == Some("") {
            continue;
        }
        let mstyle = ms.map_or(style, |m| &m.style);
        let fs = mstyle.font_size;
        let line_h = style.font_size * style.line_height;
        let top = b.rect.y + style.border_width.top + style.padding.top;
        // `list-style-position: inside` puts the marker at the content's left edge; the item
        // reserves room for it in its padding (see `inside_marker_width`).
        let inside_x = b.rect.x + style.border_width.left + style.padding.left;
        let kind_and_rect = match style.list_style_type {
            _ if content.is_none() && image.is_some() => {
                // The picture, scaled down to about a line of text, centred on the first line.
                let img = image.clone().expect("checked");
                let scale = (fs * 1.2 / img.height as f32).min(1.0);
                let (w, h) = (img.width as f32 * scale, img.height as f32 * scale);
                let x = if style.list_style_inside { inside_x } else { b.rect.x - fs * 0.4 - w };
                Some((MarkerKind::Image(img), Rect { x, y: top + (line_h - h) / 2.0, width: w, height: h }))
            }
            _ if content.is_some() => {
                let mut l = text.shape(content.unwrap_or_default(), &text_style(mstyle), 1.0);
                break_lines(&mut l, None, Alignment::Start);
                let (w, h) = (l.full_width(), l.height());
                let x = if style.list_style_inside { inside_x } else { b.rect.x - fs * 0.3 - w };
                Some((MarkerKind::Text(Box::new(l)), Rect { x, y: top + (line_h - h) / 2.0, width: w, height: h }))
            }
            t @ (nile_style::ListStyleType::Disc | nile_style::ListStyleType::Circle | nile_style::ListStyleType::Square) => {
                let d = (fs * 0.34).round().max(3.0);
                let x = if style.list_style_inside { inside_x } else { b.rect.x - fs * 0.5 - d };
                let rect = Rect { x, y: top + (line_h - d) / 2.0 + fs * 0.05, width: d, height: d };
                let kind = match t {
                    nile_style::ListStyleType::Disc => MarkerKind::Disc,
                    nile_style::ListStyleType::Circle => MarkerKind::Circle,
                    _ => MarkerKind::Square,
                };
                Some((kind, rect))
            }
            t @ (nile_style::ListStyleType::DisclosureClosed | nile_style::ListStyleType::DisclosureOpen) => {
                // Drawn inside the box's left padding.
                let d = (fs * 0.5).round().max(4.0);
                let rect = Rect { x: b.rect.x + style.border_width.left + fs * 0.25, y: top + (line_h - d) / 2.0, width: d, height: d };
                Some((MarkerKind::Disclosure { open: t == nile_style::ListStyleType::DisclosureOpen }, rect))
            }
            t => {
                let parent = doc.node(b.node).parent;
                let n = parent
                    .map(|p| *numbers.entry(p).or_insert_with(|| list_numbers(doc, styles, p)).get(&b.node).unwrap_or(&1))
                    .unwrap_or(1);
                marker_text(t, n).map(|label| {
                    let mut l = text.shape(&label, &text_style(mstyle), 1.0);
                    break_lines(&mut l, None, Alignment::Start);
                    let (w, h) = (l.full_width(), l.height());
                    let x = if style.list_style_inside { inside_x } else { b.rect.x - fs * 0.3 - w };
                    (MarkerKind::Text(Box::new(l)), Rect { x, y: top + (line_h - h) / 2.0, width: w, height: h })
                })
            }
        };
        if let Some((kind, rect)) = kind_and_rect {
            out.insert(b.node, ListMarker { kind, rect, color: ms.and_then(|m| m.color) });
        }
    }
    out
}

/// Rows for every list-box select, positioned from its laid-out box.
fn build_lists(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &LayoutTree,
    offsets: Option<&ScrollOffsets>,
) -> std::collections::HashMap<NodeId, ListBox> {
    let mut out = std::collections::HashMap::new();
    for b in &tree.boxes {
        if !matches!(b.content, BoxContent::Element) || styles.is_pseudo(b.node) || list_rows(doc, b.node).is_none() {
            continue;
        }
        let style = styles.get(b.node);
        let ts = text_style(style);
        let row_height = option_row_height(style);
        let items: Vec<ListItem> = select_options(doc, b.node)
            .into_iter()
            .map(|o| {
                let header = is_group(doc, o);
                let mut l = text.shape(&option_label(doc, o), &if header { TextStyle { font_weight: 700.0, ..ts.clone() } } else { ts.clone() }, 1.0);
                break_lines(&mut l, None, Alignment::Start);
                ListItem { text: l, disabled: option_disabled(doc, o), selected: !header && doc.has_state(o, "selected"), header, indent: option_indented(doc, o) }
            })
            .collect();
        let inset_top = style.border_width.top + style.padding.top;
        let view_height = (b.rect.height - inset_top - style.border_width.bottom - style.padding.bottom).max(0.0);
        let max_scroll = (items.len() as f32 * row_height - view_height).max(0.0);
        let scroll = offsets.and_then(|o| o.get(&b.node)).map_or(0.0, |o| o.1).clamp(0.0, max_scroll);
        out.insert(b.node, ListBox { items, row_height, inset_top, pad_x: 6.0, view_height, scroll, max_scroll });
    }
    out
}

/// The drop-down list for `req.select`, below its box.
fn build_menu(
    doc: &Document,
    styles: &StyleMap,
    text: &mut TextEngine,
    tree: &LayoutTree,
    req: MenuRequest,
    offsets: Option<&ScrollOffsets>,
    (view_top, view_height): (f32, f32),
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
            let header = is_group(doc, o);
            let mut l = text.shape(&option_label(doc, o), &if header { TextStyle { font_weight: 700.0, ..ts.clone() } } else { ts.clone() }, 1.0);
            break_lines(&mut l, None, Alignment::Start);
            let background = styles.try_get(o).and_then(|st| st.background).filter(|c| c.a > 0);
            MenuItem { text: l, disabled: option_disabled(doc, o), header, indent: option_indented(doc, o), background }
        })
        .collect();
    if items.is_empty() {
        return None;
    }
    let pad_x = 8.0;
    let widest = items.iter().map(|i| i.text.full_width() + if i.indent { 12.0 } else { 0.0 }).fold(0.0, f32::max);
    let row_height = (style.font_size * style.line_height + 6.0).ceil();
    // Open below the select; if the list doesn't fit there, use whichever side has more
    // room and scroll the rest.
    let full = row_height * items.len() as f32 + 2.0;
    const MARGIN: f32 = 4.0;
    let below = (view_top + view_height - (rect.y + rect.height) - MARGIN).max(0.0);
    let above = (rect.y - view_top - MARGIN).max(0.0);
    let (y, height) = if full <= below || below >= above {
        (rect.y + rect.height, full.min(below.max(row_height + 2.0)))
    } else {
        let h = full.min(above);
        (rect.y - h, h)
    };
    let max_scroll = (full - height).max(0.0);
    Some(MenuBox {
        rect: Rect { x: rect.x, y, width: rect.width.max((widest + 2.0 * pad_x + 2.0).ceil()), height },
        row_height,
        pad_x,
        selected: selected_index(doc, req.select),
        highlight: req.highlight,
        scroll: req.scroll.clamp(0.0, max_scroll),
        max_scroll,
        items,
        background: style.background.filter(|c| c.a > 0),
        border: (style.border_width.top > 0.0 && style.border_style.top != nile_style::BorderStyle::None).then_some(style.border_color.top),
        radius: style.border_radius.tl,
        accent: style.accent_color,
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
    MASKS.with(|m| {
        let key = (doc as *const Document as usize, id);
        match style.password_char {
            Some(c) => m.borrow_mut().insert(key, c),
            None => m.borrow_mut().remove(&key),
        }
    });
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
            ellipsis: false,
            nodes: Vec::new(),
            marks: Vec::new(),
            nowrap: node.tag() != Some("textarea"),
            flow: TextFlow::default(),
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
            (cs.display == Display::Inline || cs.is_inline_block()) && !cs.position.out_of_flow() && cs.float == nile_style::Float::None
        } else {
            match &doc.node(c).kind {
                NodeKind::Text(_) => true,
                NodeKind::Element { .. } => {
                    let cs = styles.get(c);
                    (cs.display == Display::Inline || cs.is_inline_block()) && !cs.position.out_of_flow() && cs.float == nile_style::Float::None
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
    nodes: Vec<TextNodeMap>,
    marks: Vec<ElementMark>,
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
            nodes: Vec::new(),
            marks: Vec::new(),
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
                let es = styles.get(id);
                let (collapsed, offs) = apply_white_space_mapped(raw, es.white_space, &self.text);
                let collapsed_len = collapsed.len();
                let collapsed = transform_text(&collapsed, es.text_transform);
                let strip = self.collapse && !es.white_space.keeps_spaces() && collapsed.starts_with(' ');
                let piece = if strip { &collapsed[1..] } else { &collapsed[..] };
                let at = self.text.len();
                // Characters of the node -> bytes of the run. (Case changes that alter the
                // length leave positions approximate.)
                let _ = collapsed_len;
                let dropped = usize::from(strip);
                self.nodes.push(TextNodeMap {
                    node: id,
                    start: at,
                    end: at + piece.len(),
                    offsets: offs.iter().map(|o| at + o.saturating_sub(dropped).min(piece.len())).collect(),
                });
                if !piece.is_empty() {
                    let start = self.text.len();
                    self.text.push_str(piece);
                    let mut ts = text_style(styles.get(id));
                    ts.deco = deco;
                    ts.shift = self.open.iter().map(|&o| baseline_shift(styles.get(o))).sum();
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
                    self.collapse = !es.white_space.keeps_spaces() && (piece.ends_with(' ') || (es.white_space.keeps_newlines() && piece.ends_with('\n')));
                }
            }
            NodeKind::Element { .. } => {
                let es = styles.get(id);
                if es.display == Display::None {
                    return;
                }
                if es.is_inline_block() {
                    if let Some(spec) = make_box(id) {
                        self.boxes.push(InlineBoxSpec { index: self.text.len(), ..spec });
                        self.collapse = false;
                    }
                    return;
                }
                if node.tag() == Some("br") {
                    self.trim_end();
                    let at = self.text.len();
                    self.text.push('\n');
                    self.marks.push(ElementMark { node: id, start: at, end: at + 1 });
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
                        radius: es.border_radius.max(),
                    });
                    self.decos.len() - 1
                });
                // Horizontal margins are spacers of the enclosing decoration (the margin shows
                // the parent's background, not the element's own).
                let outer = deco;
                let margin = |l: Length| if let Length::Px(v) = l { v.max(0.0) } else { 0.0 };
                self.push_spacer(id, es, outer, margin(es.margin.left));
                let deco = own.map_or(deco, |i| i as u32 + 1);
                if own.is_some() {
                    self.push_spacer(id, es, deco, es.padding.left + es.border_width.left);
                }
                // TODO: block-level descendants of an inline element should
                // split the run; for now their text is flowed inline.
                let mark_start = self.text.len();
                self.open.push(id);
                for c in kids(doc, styles, id) {
                    self.collect(doc, styles, c, deco, make_box);
                }
                self.open.pop();
                self.marks.push(ElementMark { node: id, start: mark_start, end: self.text.len() });
                if own.is_some() {
                    self.push_spacer(id, es, deco, es.padding.right + es.border_width.right);
                }
                self.push_spacer(id, es, outer, margin(es.margin.right));
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
        if !inside && es.is_inline_block() {
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
                radius: es.border_radius.max(),
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
            ts.shift = self.open.iter().map(|&o| baseline_shift(styles.get(o))).sum::<f32>() + baseline_shift(es);
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
            for n in &mut self.nodes {
                n.end = n.end.min(len);
                n.start = n.start.min(len);
                for o in &mut n.offsets {
                    *o = (*o).min(len);
                }
            }
            for m in &mut self.marks {
                m.end = m.end.min(len);
                m.start = m.start.min(len);
            }
        }
    }
}

/// Text as `white-space` leaves it (`before` is the run so far, for tab stops), plus for each character of `raw` (and one past the end) the byte of
/// the result where it landed; dropped whitespace points at where it would have been.
fn apply_white_space_mapped(raw: &str, ws: nile_style::WhiteSpace, before: &str) -> (String, Vec<usize>) {
    use nile_style::WhiteSpace as W;
    let mut out = String::with_capacity(raw.len());
    let mut offs = Vec::with_capacity(raw.len() + 1);
    match ws {
        W::Normal | W::Nowrap => {
            let mut last_space = false;
            for c in raw.chars() {
                offs.push(out.len());
                if c.is_whitespace() {
                    if !last_space {
                        out.push(' ');
                    }
                    last_space = true;
                } else {
                    out.push(c);
                    last_space = false;
                }
            }
        }
        W::Pre | W::PreWrap => {
            let mut column = before.rsplit('\n').next().map_or(0, |l| l.chars().count());
            let mut chars = raw.chars().peekable();
            while let Some(c) = chars.next() {
                offs.push(out.len());
                match c {
                    '\r' => {
                        if chars.peek() == Some(&'\n') {
                            continue;
                        }
                        out.push('\n');
                        column = 0;
                    }
                    '\n' => {
                        out.push('\n');
                        column = 0;
                    }
                    // Tab stops every 8 columns.
                    '\t' => {
                        let n = 8 - column % 8;
                        out.extend(std::iter::repeat_n(' ', n));
                        column += n;
                    }
                    c => {
                        out.push(c);
                        column += 1;
                    }
                }
            }
        }
        W::PreLine => {
            let mut last_space = false;
            for c in raw.chars() {
                match c {
                    '\n' => {
                        if out.ends_with(' ') {
                            out.pop();
                        }
                        offs.push(out.len());
                        out.push('\n');
                        last_space = true;
                    }
                    c if c.is_whitespace() => {
                        offs.push(out.len());
                        if !last_space {
                            out.push(' ');
                        }
                        last_space = true;
                    }
                    c => {
                        offs.push(out.len());
                        out.push(c);
                        last_space = false;
                    }
                }
            }
        }
    }
    offs.push(out.len());
    (out, offs)
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
        // `text-indent`: a spacer before the first character (the first line only, since it is
        // the run's first thing). Positive indents only.
        let indent = block_style.text_indent > 0.0;
        if indent {
            inline.push_spacer(run[0], block_style, 0, block_style.text_indent);
        }
        for &id in run {
            inline.collect(doc, styles, id, 0, &mut make_box);
        }
    }
    inline.trim_end();
    if (inline.text.is_empty() || inline.text == "\u{a0}" && block_style.text_indent > 0.0) && inline.boxes.is_empty() {
        return None;
    }
    for (_, ts) in &mut inline.spans {
        if ts.spacer {
            // A one-pixel no-break space plus letter spacing makes up the width, so the run's
            // font metrics never stretch the line (a 120px font would for a 30px margin).
            let ratio = text.nbsp_ratio(&ts.font_family, ts.font_weight);
            ts.letter_spacing = ts.font_size - ratio;
            ts.font_size = 1.0;
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
            nodes: inline.nodes,
            marks: inline.marks,
            text: inline.text,
            spacers,
            control: false,
            user_select: block_style.user_select,
            ellipsis: block_style.text_ellipsis && block_style.overflow_x != OverflowKind::Visible && !block_style.white_space.wraps(),
            nowrap: !block_style.white_space.wraps(),
            flow: TextFlow { clamp: block_style.line_clamp, balance: block_style.text_wrap == nile_style::TextWrap::Balance },
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
    let mut rect = Rect {
        x: parent_x + l.location.x,
        y: parent_y + l.location.y,
        width: l.size.width,
        height: l.size.height,
    };
    if let Some((sx, sy)) = STATIC_AT.with(|a| a.borrow().get(&n).copied()) {
        rect.x = sx.unwrap_or(rect.x);
        rect.y = sy.unwrap_or(rect.y);
    }

    let persp = parent_perspective(tree, styles, n, parent_x, parent_y);
    let sticky = match tree.get_node_context(n) {
        Some(NodeCtx::Element(id) | NodeCtx::Replaced { id, .. }) => sticky_box(tree, styles, n, *id, parent_y),
        _ => None,
    };
    match tree.get_node_context_mut(n) {
        Some(NodeCtx::Element(id)) => {
            let bg_image = bg_image_of(styles.get(*id));
            let fg_image = fg_image_of(styles.get(*id));
            let transform = box_transform(styles.get(*id), rect, persp);
            let fixed = styles.get(*id).position == Position::Fixed;
            out.push(LayoutBox { node: *id, rect, content: BoxContent::Element, subtree_end: 0, scroll: None, image: None, bg_image, fg_image, transform, persp, fixed, sticky, border_gap: None, backface_hidden: styles.get(*id).backface_hidden, inert: is_inert(styles.get(*id)), border_hidden: Edges::default() })
        }
        Some(NodeCtx::Replaced { id, image, .. }) => {
            let bg_image = bg_image_of(styles.get(*id));
            let fg_image = fg_image_of(styles.get(*id));
            let transform = box_transform(styles.get(*id), rect, persp);
            let fixed = styles.get(*id).position == Position::Fixed;
            out.push(LayoutBox { node: *id, rect, content: BoxContent::Element, subtree_end: 0, scroll: None, image: image.clone(), bg_image, fg_image, transform, persp, fixed, sticky, border_gap: None, backface_hidden: styles.get(*id).backface_hidden, inert: is_inert(styles.get(*id)), border_hidden: Edges::default() })
        }
        Some(NodeCtx::Text { node, layout, align, kids, decos, owners, text, spacers, control, user_select, ellipsis, nodes, marks, nowrap, flow }) => {
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
            let spec = FLOW_SPECS.with(|s| s.borrow().get(&n).cloned());
            match &spec {
                // Beside floats: the box spans the whole content box and each line has its own slot.
                Some(spec) if !*nowrap => {
                    rect.x += spec.x0;
                    rect.width = spec.x1 - spec.x0;
                    // `slot` is relative to where Taffy put the paragraph, `rect` now starts at spec.x0 from there.
                    break_lines_varying(layout, rect.width + 1.0, *align, |y| {
                        let (x, w) = spec.slot(y);
                        (x - spec.x0, w)
                    });
                }
                _ if flow.balance && !*nowrap => {
                    let container = width + 1.0;
                    let narrow = nile_text::balanced_width(layout, container);
                    if narrow < container - 1.0 {
                        // Lines sit in a narrower column, placed where the alignment would put it.
                        let shift = match align {
                            nile_text::Alignment::Center => 0.5,
                            nile_text::Alignment::End | nile_text::Alignment::Right => 1.0,
                            _ => 0.0,
                        };
                        let x = (container - narrow) * shift;
                        break_lines_varying(layout, container, *align, |_| (x, narrow));
                    } else {
                        break_lines(layout, Some(container), *align);
                    }
                }
                _ => break_lines(layout, Some(width + 1.0), *align),
            }
            TEXT_LEAVES.with(|t| t.borrow_mut().push((out.len(), n)));
            let layout = (**layout).clone();
            let kids = kids.clone();
            let decos = decos.clone();
            let owners = owners.clone();
            let (text, spacers, control, user_select, ellipsis) = (text.clone(), spacers.clone(), *control, *user_select, *ellipsis);
            let (nodes, marks) = (nodes.clone(), marks.clone());
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
                content: BoxContent::Text(Box::new(TextContent { layout, nodes, marks, decos, owners, text, spacers, control, user_select, ellipsis, clamp: flow.clamp })),
                subtree_end: 0,
                scroll: None,
                image: None,
                bg_image: None,
                fg_image: None,
                transform: None,
                persp: None,
                fixed: false,
                sticky: None,
                border_gap: None,
                backface_hidden: false,
                inert: is_inert(styles.get(*node)),
                border_hidden: Edges::default(),
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
    let (self_style, flex_parent) = match tree.get_node_context(n) {
        Some(NodeCtx::Element(id)) => (Some(styles.get(*id)), is_flex_or_grid(styles.get(*id))),
        _ => (None, false),
    };
    // A stacking context (the root, or a box that forms one) paints its positioned and
    // z-indexed descendants in its own order, wherever they sit in the tree: negative
    // `z-index` first, then the ordinary boxes, then positioned ones (`z-index` auto or 0) in
    // tree order, then positive `z-index`. Descendants under a clipping, faded, transformed
    // or sticky box stay inside that box, which is the only place they can be clipped.
    let mut late: Vec<(taffy::NodeId, f32, f32, (u8, i32))> = Vec::new();
    let mut early: Vec<(taffy::NodeId, f32, f32, (u8, i32))> = Vec::new();
    let item_of_flex = tree.parent(n).is_some_and(|p| matches!(tree.get_node_context(p), Some(NodeCtx::Element(id)) if is_flex_or_grid(styles.get(*id))));
    let is_context = pushed_self && (tree.parent(n).is_none() || self_style.is_some_and(|s| forms_context(s, item_of_flex)));
    if is_context {
        let mut found = Vec::new();
        collect_hoisted(tree, styles, n, rect.x, rect.y, true, &mut found);
        HOISTED.with(|h| h.borrow_mut().extend(found.iter().map(|f| f.0)));
        for f in found {
            if f.3 .0 == 0 {
                early.push(f);
            } else {
                late.push(f);
            }
        }
        early.sort_by_key(|f| f.3);
        late.sort_by_key(|f| f.3);
    }
    // Siblings left in place are still ordered among themselves.
    children.retain(|c| !HOISTED.with(|h| h.borrow().contains(c)));
    children.sort_by_key(|c| match tree.get_node_context(*c) {
        Some(NodeCtx::Element(id)) => paint_layer(styles.get(*id), flex_parent),
        _ => (1, 0),
    });
    for (c, px, py, _) in early {
        HOISTED.with(|h| h.borrow_mut().remove(&c));
        flatten(tree, styles, c, px, py, out);
    }
    for child in children {
        flatten(tree, styles, child, rect.x, rect.y, out);
    }
    for (c, px, py, _) in late {
        HOISTED.with(|h| h.borrow_mut().remove(&c));
        flatten(tree, styles, c, px, py, out);
    }
    if pushed_self {
        out[first].subtree_end = out.len();
    }
}

fn is_flex_or_grid(s: &ComputedStyle) -> bool {
    matches!(s.display, Display::Flex | Display::Grid)
}

/// Whether a box is a stacking context: positioned with a `z-index` (or a flex/grid item with
/// one), faded, transformed or `fixed`. Its descendants are painted as a unit.
fn forms_context(s: &ComputedStyle, flex_item: bool) -> bool {
    (s.z_index.is_some() && (s.position != Position::Static || flex_item)) || s.opacity < 1.0 || !s.transform.is_empty() || s.layered() || s.position == Position::Fixed
}

/// Boxes that clip, fade, transform or stick their contents: a positioned descendant inside
/// one can't be painted outside it, so it isn't hoisted past it.
fn confines(s: &ComputedStyle) -> bool {
    s.clips() || s.opacity < 1.0 || !s.transform.is_empty() || s.layered() || s.position == Position::Sticky
}

/// The descendants of `n` that belong in the paint order of the stacking context `n` is (or
/// is part of), with the origin of their parent and their paint layer. `x`, `y` are `n`'s
/// absolute position; `safe` says nothing between the context and `n` confines them.
fn collect_hoisted(
    tree: &TaffyTree<NodeCtx>,
    styles: &StyleMap,
    n: taffy::NodeId,
    x: f32,
    y: f32,
    safe: bool,
    out: &mut Vec<(taffy::NodeId, f32, f32, (u8, i32))>,
) {
    let flex = match tree.get_node_context(n) {
        Some(NodeCtx::Element(id)) => is_flex_or_grid(styles.get(*id)),
        _ => false,
    };
    for c in tree.children(n).unwrap_or_default() {
        let Ok(l) = tree.layout(c) else { continue };
        let (cx, cy) = (x + l.location.x, y + l.location.y);
        match tree.get_node_context(c) {
            Some(NodeCtx::Element(id)) => {
                let s = styles.get(*id);
                let layer = paint_layer(s, flex);
                if layer.0 != 1 {
                    if safe {
                        out.push((c, x, y, layer));
                        // Positioned but not a context: what's inside it is part of this context too.
                        if !forms_context(s, flex) {
                            collect_hoisted(tree, styles, c, cx, cy, !confines(s), out);
                        }
                    }
                } else {
                    collect_hoisted(tree, styles, c, cx, cy, safe && !confines(s), out);
                }
            }
            Some(_) => {}
            None => collect_hoisted(tree, styles, c, cx, cy, safe, out),
        }
    }
}

fn box_transform(s: &ComputedStyle, rect: Rect, persp: Option<(f32, f32, f32)>) -> Option<Affine> {
    (!s.transform.is_empty()).then(|| Affine::from_style_in(&s.transform, s.transform_origin, rect, persp))
}

/// The perspective a box's parent gives it: `(distance, origin x, origin y)` in page
/// coordinates, the origin resolved against the parent's border box.
fn parent_perspective(tree: &TaffyTree<NodeCtx>, styles: &StyleMap, n: taffy::NodeId, parent_x: f32, parent_y: f32) -> Option<(f32, f32, f32)> {
    let parent = tree.parent(n)?;
    let Some(NodeCtx::Element(pid)) = tree.get_node_context(parent) else { return None };
    let ps = styles.get(*pid);
    let d = ps.perspective?;
    let size = tree.layout(parent).ok()?.size;
    let along = |p: nile_style::StopPos, whole: f32| match p {
        nile_style::StopPos::Px(v) => v,
        nile_style::StopPos::Fraction(f) => f * whole,
    };
    Some((d, parent_x + along(ps.perspective_origin.0, size.width), parent_y + along(ps.perspective_origin.1, size.height)))
}

/// (layer, z-index) for sorting siblings into paint order.
fn paint_layer(s: &ComputedStyle, flex_item: bool) -> (u8, i32) {
    let positioned = s.position != Position::Static;
    match s.z_index {
        Some(z) if (positioned || flex_item) && z < 0 => (0, z),
        Some(z) if (positioned || flex_item) && z > 0 => (3, z),
        Some(_) if flex_item => (2, 0),
        _ if positioned || s.opacity < 1.0 || !s.transform.is_empty() || s.layered() => (2, 0),
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

/// How far an inline element's `vertical-align` lifts its text (px, up is positive).
fn baseline_shift(s: &ComputedStyle) -> f32 {
    match s.vertical_align {
        VerticalAlign::Sub => -0.2 * s.font_size,
        VerticalAlign::Super => 0.4 * s.font_size / 0.83,
        VerticalAlign::Shift(px) => px,
        _ => 0.0,
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
        shift: 0.0,
        italic: s.italic,
        underline: s.underline,
        line_through: s.line_through,
        letter_spacing: s.letter_spacing,
        word_spacing: s.word_spacing,
        word_break: match s.word_break {
            nile_style::WordBreak::Normal => nile_text::WordBreak::Normal,
            nile_style::WordBreak::BreakAll => nile_text::WordBreak::BreakAll,
            nile_style::WordBreak::KeepAll => nile_text::WordBreak::KeepAll,
        },
        overflow_wrap: match s.overflow_wrap {
            nile_style::OverflowWrap::Normal => nile_text::OverflowWrap::Normal,
            nile_style::OverflowWrap::Anywhere => nile_text::OverflowWrap::Anywhere,
            nile_style::OverflowWrap::BreakWord => nile_text::OverflowWrap::BreakWord,
        },
        decoration: nile_text::Decoration {
            style: match s.deco.style {
                nile_style::DecoStyle::Solid => nile_text::DecoStyle::Solid,
                nile_style::DecoStyle::Double => nile_text::DecoStyle::Double,
                nile_style::DecoStyle::Dotted => nile_text::DecoStyle::Dotted,
                nile_style::DecoStyle::Dashed => nile_text::DecoStyle::Dashed,
                nile_style::DecoStyle::Wavy => nile_text::DecoStyle::Wavy,
            },
            color: s.deco.color.map(|c| [c.r, c.g, c.b, c.a]),
            thickness: s.deco.thickness,
            offset: s.deco.offset,
        },
        align: alignment(s.text_align),
        font_variations: s.font_variations.clone(),
        font_features: match (s.font_variant.is_empty(), s.font_features.is_empty()) {
            (true, _) => s.font_features.clone(),
            (false, true) => s.font_variant.clone(),
            (false, false) => format!("{}, {}", s.font_variant, s.font_features),
        },
    }
}

/// A `position: sticky` element's insets and the vertical extent of its parent.
fn sticky_box(tree: &TaffyTree<NodeCtx>, styles: &StyleMap, n: taffy::NodeId, id: NodeId, parent_y: f32) -> Option<StickyBox> {
    let st = styles.get(id);
    if st.position != Position::Sticky {
        return None;
    }
    let px = |l: Length| match l {
        Length::Px(v) => Some(v),
        _ => None,
    };
    let parent = tree.parent(n)?;
    let height = tree.layout(parent).ok()?.size.height;
    Some(StickyBox { top: px(st.inset.top), bottom: px(st.inset.bottom), parent_top: parent_y, parent_bottom: parent_y + height })
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

/// Size of the strip a `<splitter>` takes, in px.
pub const SPLITTER: f32 = 6.0;

/// One entry of a frameset's `cols`/`rows` list: `120px`, `30%`, `2*`, a bare number (px), else `*`.
pub fn frameset_track(entry: &str) -> Track {
    let e = entry.trim();
    let flex = |f: f32| Track { min: Breadth::Px(0.0), max: Breadth::Fr(f) };
    if let Some(n) = e.strip_suffix('*') {
        return flex(if n.trim().is_empty() { 1.0 } else { n.trim().parse().unwrap_or(1.0) });
    }
    if let Some(p) = e.strip_suffix('%').and_then(|n| n.trim().parse::<f32>().ok()) {
        return Track { min: Breadth::Percent(p / 100.0), max: Breadth::Percent(p / 100.0) };
    }
    match e.trim_end_matches("px").trim_end_matches("dip").trim().parse::<f32>() {
        Ok(px) => Track { min: Breadth::Px(px), max: Breadth::Px(px) },
        Err(_) => flex(1.0),
    }
}

/// A `<frameset>` is a grid: one track per pane from `cols` (or `rows`), and a thin one for each
/// `<splitter>` between them.
fn frameset_tracks(doc: &Document, styles: &StyleMap, id: NodeId, ts: &mut Style) {
    let (list, vertical) = match (doc.node(id).attr("cols"), doc.node(id).attr("rows")) {
        (Some(c), _) => (c, false),
        (None, Some(r)) => (r, true),
        _ => return,
    };
    let entries: Vec<&str> = list.split(',').collect();
    let mut pane = 0;
    let tracks: Vec<_> = kids(doc, styles, id)
        .into_iter()
        .filter(|k| doc.node(*k).tag().is_some())
        .map(|k| {
            let t = if doc.node(k).tag() == Some("splitter") {
                Track { min: Breadth::Px(SPLITTER), max: Breadth::Px(SPLITTER) }
            } else {
                pane += 1;
                entries.get(pane - 1).map_or(frameset_track("*"), |e| frameset_track(e))
            };
            taffy::GridTemplateComponent::Single(track_fn(t))
        })
        .collect();
    if vertical {
        ts.grid_template_rows = tracks;
    } else {
        ts.grid_template_columns = tracks;
    }
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
            Display::Table => taffy::Display::Grid,
            Display::Block
            | Display::Inline
            | Display::InlineBlock
            | Display::ListItem
            | Display::TableRowGroup
            | Display::TableRow
            | Display::TableCell
            | Display::TableCaption => taffy::Display::Block,
        },
        item_is_table: s.display == Display::Table,
        position: match s.position {
            Position::Static => taffy::Position::Relative,
            Position::Relative | Position::Sticky => taffy::Position::Relative,
            Position::Absolute | Position::Fixed => taffy::Position::Absolute,
        },
        inset: if matches!(s.position, Position::Static | Position::Sticky) {
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
        padding: {
            let mut p = rect_px(s.padding);
            if s.display == Display::ListItem && s.list_style_inside {
                p.left = taffy::LengthPercentage::length(s.padding.left + inside_marker_width(s));
            }
            p
        },
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
        aspect_ratio: s.aspect_ratio,
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
mod tests;
