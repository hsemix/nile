//! `page-view: paged`: an element whose content is shown as a stack of paper sheets.
//!
//! The content is laid out as one column (padded by the `@page` margins), `paginate` finds where each
//! sheet's content starts, and the element is then made tall enough for all the sheets. Boxes keep
//! their *flow* positions; this module maps between flow positions and where they appear on the
//! sheets, which is what hit testing, the caret and the renderer use.

use nile_dom::NodeId;
use nile_style::StyleMap;
use taffy::prelude::*;

use crate::{LayoutBox, NodeCtx, Rect};

/// Space between two sheets, in px.
pub const SHEET_GAP: f32 = 20.0;

/// A paged element laid out.
#[derive(Debug, Clone)]
pub struct PagedBox {
    pub node: NodeId,
    /// The element's box and the end of its subtree in `LayoutTree::boxes`.
    pub index: usize,
    pub end: usize,
    /// The stack of sheets, in layout coordinates.
    pub rect: Rect,
    /// Paper height; `margin` is top, right, bottom, left.
    pub page_h: f32,
    pub margin: [f32; 4],
    /// Flow y of the first line on each sheet.
    pub starts: Vec<f32>,
    /// Flow y where the content ends.
    pub content_end: f32,
}

impl PagedBox {
    pub fn count(&self) -> usize {
        self.starts.len()
    }

    /// Top of sheet `i`.
    pub fn top(&self, i: usize) -> f32 {
        self.rect.y + i as f32 * (self.page_h + SHEET_GAP)
    }

    /// The flow range sheet `i` shows.
    pub fn band(&self, i: usize) -> (f32, f32) {
        (self.starts[i], self.starts.get(i + 1).copied().unwrap_or(self.content_end.max(self.starts[i])))
    }

    /// Height of the area content flows into on a sheet.
    pub fn content_h(&self) -> f32 {
        (self.page_h - self.margin[0] - self.margin[2]).max(1.0)
    }

    /// The sheet that shows flow position `y`.
    pub fn page_of_flow(&self, y: f32) -> usize {
        self.starts.partition_point(|s| *s <= y + 0.01).saturating_sub(1)
    }

    /// Where flow position `y` appears.
    pub fn to_screen(&self, y: f32) -> f32 {
        let i = self.page_of_flow(y);
        self.top(i) + self.margin[0] + (y - self.starts[i])
    }

    /// Where a flow rectangle appears (on the sheet its top is on).
    pub fn rect_to_screen(&self, r: Rect) -> Rect {
        Rect { y: self.to_screen(r.y), ..r }
    }

    /// The flow position shown at layout y `y`. `strict`: only inside a sheet's content area (for
    /// hit testing); otherwise the margins and the gaps map to the nearest line (for placing a caret).
    pub fn to_flow(&self, y: f32, strict: bool) -> Option<f32> {
        let n = self.count();
        let stride = self.page_h + SHEET_GAP;
        let i = (((y - self.rect.y) / stride).floor().max(0.0) as usize).min(n - 1);
        let u = y - self.top(i);
        let (from, to) = self.band(i);
        let local = u - self.margin[0];
        if strict {
            return (u >= 0.0 && local >= 0.0 && local < to - from).then_some(from + local);
        }
        Some(if u < self.margin[0] {
            from
        } else if local < to - from {
            from + local
        } else if u < self.page_h + SHEET_GAP / 2.0 || i + 1 >= n {
            (to - 0.01).max(from)
        } else {
            self.starts[i + 1]
        })
    }
}

/// The paged elements of a Taffy tree, with the `@page` geometry their style needs.
pub(crate) fn find(tree: &taffy::TaffyTree<NodeCtx>, root: taffy::NodeId, styles: &StyleMap) -> Vec<(NodeId, taffy::NodeId)> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if let Some(NodeCtx::Element(id)) = tree.get_node_context(n) {
            if styles.get(*id).page_view && !styles.is_pseudo(*id) {
                out.push((*id, n));
                continue;
            }
        }
        stack.extend(tree.children(n).unwrap_or_default());
    }
    out
}

/// Gives a paged element the paper's width, the `@page` margins as padding and an auto height.
pub(crate) fn prepare(tree: &mut taffy::TaffyTree<NodeCtx>, node: taffy::NodeId, styles: &StyleMap, id: NodeId) {
    let page = styles.page_style(0);
    let [t, r, b, l] = page.margin;
    let mut st = tree.style(node).unwrap().clone();
    st.padding = taffy::Rect { left: LengthPercentage::length(l), right: LengthPercentage::length(r), top: LengthPercentage::length(t), bottom: LengthPercentage::length(b) };
    st.border = taffy::Rect::zero();
    let paper = page.size.0;
    st.size.width = Dimension::length(if styles.get(id).border_box { paper } else { paper - l - r });
    st.size.height = Dimension::auto();
    st.min_size.height = LengthPercentageAuto::length(0.0);
    st.max_size = Size::auto();
    tree.set_style(node, st).unwrap();
}

/// Pass two: the element is as tall as its sheets.
pub(crate) fn set_height(tree: &mut taffy::TaffyTree<NodeCtx>, node: taffy::NodeId, height: f32, styles: &StyleMap, id: NodeId) {
    let page = styles.page_style(0);
    let mut st = tree.style(node).unwrap().clone();
    // Content flows inside the margins; the sheets themselves add them back.
    let pad = if styles.get(id).border_box { 0.0 } else { page.margin[0] + page.margin[2] };
    st.padding.top = LengthPercentage::length(page.margin[0]);
    st.padding.bottom = LengthPercentage::length(page.margin[2]);
    st.size.height = Dimension::length(height - pad);
    st.min_size.height = LengthPercentageAuto::length(height - pad);
    tree.set_style(node, st).unwrap();
}

/// Where the content of the element at `boxes[index]` ends (the lowest bottom edge inside it).
pub(crate) fn content_end(boxes: &[LayoutBox], index: usize) -> f32 {
    let end = boxes[index].subtree_end.max(index + 1);
    boxes[index + 1..end].iter().filter(|b| !b.fixed).map(|b| b.rect.y + b.rect.height).fold(boxes[index].rect.y, f32::max)
}
