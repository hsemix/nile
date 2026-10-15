//! Where a laid-out page breaks when it is cut into pages of a fixed height.
//!
//! Layout stays one long column; pagination only chooses the vertical offsets at which a new page
//! starts. A cut may not fall inside a text line, a table row, a picture or an element with
//! `break-inside: avoid` (when it fits on a page), nor leave fewer than two lines of a paragraph
//! on either side (`orphans`/`widows`); `break-before/after: page` force one.

use nile_dom::Document;
use nile_style::{Break, Display, StyleMap};

use crate::{BoxContent, LayoutTree};

/// A table's header rows (painted again at the top of each page the table continues onto) or
/// footer rows (painted at the bottom of each page it continues from).
#[derive(Debug, Clone, PartialEq)]
pub struct Repeat {
    /// Boxes of the header rows and their cells, as `[from, to)` ranges of the paint list.
    pub ranges: Vec<(usize, usize)>,
    /// Where the header sits in the flow, and how tall it is.
    pub y: f32,
    pub height: f32,
}

/// Where pages start, and which pages repeat a table header at their top (parallel lists).
#[derive(Debug, Clone, PartialEq)]
pub struct Fragments {
    pub starts: Vec<f32>,
    pub repeats: Vec<Option<Repeat>>,
    /// Per page: a table footer (`<tfoot>`) shown again at the bottom of a page its table goes on from.
    pub feet: Vec<Option<Repeat>>,
}

/// Page start offsets (the first is 0) for `tree` cut into pages `page_h` tall.
pub fn paginate(tree: &LayoutTree, styles: &StyleMap, page_h: f32) -> Vec<f32> {
    paginate_boxes(&tree.boxes, styles, 0.0, tree.content_height, page_h)
}

/// [`paginate`], with table headers repeated on the pages their table continues onto.
pub fn paginate_tree(tree: &LayoutTree, doc: &Document, styles: &StyleMap, page_h: f32) -> Fragments {
    paginate_with(&tree.boxes, 0, Some(doc), styles, 0.0, tree.content_height, page_h)
}

/// Page starts for `boxes` (a subtree of the paint list) whose content runs from `origin` to
/// `end` in the boxes' own y coordinates; the first start is `origin`.
pub fn paginate_boxes(boxes: &[crate::LayoutBox], styles: &StyleMap, origin: f32, end: f32, page_h: f32) -> Vec<f32> {
    paginate_with(boxes, 0, None, styles, origin, end, page_h).starts
}

struct Head {
    table_top: f32,
    table_bottom: f32,
    repeat: Repeat,
}

/// The tables in `boxes` (whose first box is paint-list index `base`) that have a `<thead>` or `<tfoot>` (`group`).
fn table_parts(boxes: &[crate::LayoutBox], base: usize, doc: &Document, group: &str) -> Vec<Head> {
    let mut out = Vec::new();
    for (k, t) in boxes.iter().enumerate() {
        if !matches!(t.content, BoxContent::Element) || doc.node(t.node).tag() != Some("table") {
            continue;
        }
        let end = t.subtree_end.saturating_sub(base).clamp(k + 1, boxes.len());
        let mut ranges = Vec::new();
        let (mut top, mut bottom) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut header_rows = Vec::new();
        for (j, row) in boxes.iter().enumerate().take(end).skip(k + 1) {
            let n = doc.node(row.node);
            if matches!(row.content, BoxContent::Element) && n.tag() == Some("tr") && n.parent.is_some_and(|p| doc.node(p).tag() == Some(group)) {
                header_rows.push(row.node);
                ranges.push((base + j, base + j + 1));
                top = top.min(row.rect.y);
                bottom = bottom.max(row.rect.y + row.rect.height);
            }
        }
        if header_rows.is_empty() {
            continue;
        }
        // A group that paints a background has a box of its own behind its rows.
        for (j, g) in boxes.iter().enumerate().take(end).skip(k + 1) {
            if matches!(g.content, BoxContent::Element) && g.node.0 < doc.node_count() && doc.node(g.node).tag() == Some(group) {
                ranges.push((base + j, base + j + 1));
            }
        }
        for (j, cell) in boxes.iter().enumerate().take(end).skip(k + 1) {
            if matches!(cell.content, BoxContent::Element) && doc.node(cell.node).parent.is_some_and(|p| header_rows.contains(&p)) && doc.node(cell.node).tag() != Some("tr") {
                ranges.push((base + j, cell.subtree_end.max(base + j + 1)));
            }
        }
        out.push(Head { table_top: t.rect.y, table_bottom: t.rect.y + t.rect.height, repeat: Repeat { ranges, y: top, height: bottom - top } });
    }
    out
}

/// [`paginate_boxes`] with table headers (when the document is given): `base` is the paint-list index of `boxes[0]`.
pub fn paginate_with(boxes: &[crate::LayoutBox], base: usize, doc: Option<&Document>, styles: &StyleMap, origin: f32, end: f32, page_h: f32) -> Fragments {
    let page_h = page_h.max(1.0);
    let total = end;
    let table_heads = doc.map(|d| table_parts(boxes, base, d, "thead")).unwrap_or_default();
    let table_feet = doc.map(|d| table_parts(boxes, base, d, "tfoot")).unwrap_or_default();
    let mut forced: Vec<f32> = Vec::new();
    // Open intervals no cut may fall into, split by how important they are.
    let mut lines: Vec<(f32, f32)> = Vec::new();
    let mut blocks: Vec<(f32, f32)> = Vec::new();
    // Where content can begin on a page: line tops and the tops of unbreakable boxes.
    let mut tops: Vec<f32> = Vec::new();
    for b in boxes {
        match &b.content {
            BoxContent::Element => {
                let s = styles.get(b.node);
                if s.display == Display::None || s.display == Display::Inline {
                    continue;
                }
                let (y0, y1) = (b.rect.y, b.rect.y + b.rect.height);
                match s.break_before {
                    Break::Always if y0 > origin + 0.5 => forced.push(y0),
                    Break::Avoid => blocks.push((y0 - 0.01, y0 + 0.01)),
                    _ => {}
                }
                match s.break_after {
                    Break::Always if y1 < total - 0.5 => forced.push(y1),
                    Break::Avoid => blocks.push((y1 - 0.01, y1 + 0.01)),
                    _ => {}
                }
                let atomic = s.break_inside == Break::Avoid || s.display == Display::TableRow || b.image.is_some();
                if atomic && b.rect.height <= page_h && b.rect.height > 0.0 {
                    blocks.push((y0, y1));
                    tops.push(y0);
                }
            }
            BoxContent::Text(c) => {
                let all: Vec<(f32, f32)> = c
                    .layout
                    .lines()
                    .map(|l| {
                        let m = l.metrics();
                        (b.rect.y + m.block_min_coord, b.rect.y + m.block_max_coord)
                    })
                    .collect();
                lines.extend(all.iter().copied());
                tops.extend(all.iter().map(|l| l.0));
                // Orphans and widows of two lines: keep short runs whole, and the first and last
                // two lines of longer ones together.
                let n = all.len();
                if n >= 2 {
                    let (top, bottom) = (all[0].0, all[n - 1].1);
                    if n < 4 {
                        if bottom - top <= page_h {
                            blocks.push((top, bottom));
                        }
                    } else {
                        blocks.push((all[0].0, all[1].1));
                        blocks.push((all[n - 2].0, all[n - 1].1));
                    }
                }
            }
        }
    }
    // A cut right after a table's header would leave it alone at the bottom of a page.
    for h in &table_heads {
        let after = h.repeat.y + h.repeat.height;
        // (A point-sized interval would never contain a cut: back up to the header's top instead.)
        blocks.push((h.repeat.y, after + 0.5));
    }
    forced.sort_by(f32::total_cmp);
    tops.sort_by(f32::total_cmp);
    // A page starts at its first line, not in the margin that preceded it.
    let snap = |cut: f32| tops.iter().find(|&&t| t >= cut - 0.01).filter(|&&t| t - cut < page_h).copied().unwrap_or(cut);
    let inside = |set: &[(f32, f32)], y: f32| set.iter().find(|&&(a, b)| y > a + 0.01 && y < b - 0.01).copied();

    // The header a page starting at `start` repeats: its table began earlier and goes on past it.
    let repeat_at = |start: f32| table_heads.iter().find(|h| h.table_top < start - 0.5 && start >= h.repeat.y + h.repeat.height - 0.01 && start < h.table_bottom).map(|h| h.repeat.clone());
    let mut starts = vec![origin];
    let mut repeats: Vec<Option<Repeat>> = vec![None];
    // The footer a page starting at `start` shows at its bottom: a table is on the page, and its
    // footer doesn't fit on it where it naturally falls (it is on a later page).
    let foot_at = |start: f32, limit: f32| table_feet.iter().find(|f| f.table_top < limit - 0.5 && f.table_bottom > start && f.repeat.y + f.repeat.height > limit + 0.01).map(|f| f.repeat.clone());
    let mut feet: Vec<Option<Repeat>> = Vec::new();
    // A page that ends before its table begins (the table moved whole to the next page) shows no footer.
    let drop_foot = |feet: &mut Vec<Option<Repeat>>, cut: f32| {
        if let Some(Some(r)) = feet.last() {
            if table_feet.iter().any(|f| f.repeat.y == r.y && f.table_top >= cut - 0.5) {
                *feet.last_mut().expect("just checked") = None;
            }
        }
    };
    let mut start = origin;
    while starts.len() < 10_000 {
        let after_head = page_h - repeat_at(start).map_or(0.0, |r| r.height).min(page_h / 2.0);
        let foot = foot_at(start, start + after_head);
        let room = after_head - foot.as_ref().map_or(0.0, |f| f.height).min(page_h / 2.0);
        feet.push(foot);
        let limit = start + room;
        if let Some(&f) = forced.iter().find(|&&f| f > start + 0.5 && f <= limit + 0.01) {
            let f = snap(f);
            drop_foot(&mut feet, f);
            starts.push(f);
            repeats.push(repeat_at(f));
            start = f;
            continue;
        }
        if total - start <= room + 0.5 {
            break;
        }
        // Back up from the limit past everything a cut may not split; without a way out, only
        // keep text lines whole, and then cut where the page ends.
        let find = |sets: &[&[(f32, f32)]]| {
            let mut y = limit;
            for _ in 0..1000 {
                match sets.iter().find_map(|s| inside(s, y)) {
                    Some((a, _)) => y = a,
                    None => return (y > start + 1.0).then_some(y),
                }
            }
            None
        };
        let cut = find(&[&lines, &blocks]).or_else(|| find(&[&lines])).unwrap_or(limit);
        let cut = snap(cut);
        drop_foot(&mut feet, cut);
        starts.push(cut);
        repeats.push(repeat_at(cut));
        start = cut;
    }
    feet.resize(starts.len(), None);
    Fragments { starts, repeats, feet }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout;
    use nile_dom::Document;
    use nile_style::compute_styles;

    fn pages(html: &str, page_h: f32) -> Vec<f32> {
        let doc = Document::parse(html);
        let styles = compute_styles(&doc);
        let mut text = nile_text::TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 1.0);
        paginate(&tree, &styles, page_h)
    }

    #[test]
    fn cuts_between_lines_and_honours_breaks() {
        // 20px lines in a 100px page: cuts land on line boundaries.
        let css = "<style>body{margin:0} p{margin:0;font:16px/20px sans-serif} .row{height:30px;margin:0;break-inside:avoid}</style>";
        let rows: String = (0..8).map(|_| "<div class=row></div>").collect();
        let starts = pages(&format!("{css}{rows}"), 100.0);
        // 8 rows of 30px = 240px: a cut at 90 (3 whole rows), not at 100.
        assert_eq!(starts, vec![0.0, 90.0, 180.0], "{starts:?}");

        let text = pages(&format!("{css}<p>{}</p>", "line<br>".repeat(10)), 90.0);
        // Four whole lines per page (the pitch is the font's line height), never 90px.
        assert_eq!(text.len(), 3, "{text:?}");
        assert!(text[1] < 90.0 && text[1] > 70.0 && (text[2] - 2.0 * text[1]).abs() < 1.0, "{text:?}");

        let forced = pages(&format!("{css}<div class=row></div><div class=row style=\"break-before:page\"></div><div class=row></div>"), 100.0);
        assert_eq!(forced, vec![0.0, 30.0], "{forced:?}");

        let after = pages(&format!("{css}<div class=row style=\"page-break-after:always\"></div><div class=row></div>"), 100.0);
        assert_eq!(after, vec![0.0, 30.0]);

        let avoid = pages(&format!("{css}<div class=row></div><div style=\"break-inside:avoid;height:80px\"></div><div class=row></div>"), 100.0);
        assert_eq!(avoid, vec![0.0, 30.0, 110.0], "{avoid:?}");
    }

    #[test]
    fn table_headers_repeat_on_the_pages_a_table_continues_onto() {
        let rows: String = (0..10).map(|i| format!("<tr><td>r{i}</td></tr>")).collect();
        let doc = Document::parse(&format!(
            "<style>body{{margin:0}} table{{border-spacing:0}} td,th{{padding:0;height:20px}}</style><table><thead><tr><th>h</th></tr></thead><tbody>{rows}</tbody></table>"
        ));
        let styles = compute_styles(&doc);
        let mut text = nile_text::TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 1.0);
        let f = paginate_tree(&tree, &doc, &styles, 100.0);
        // Page one: the header and four rows. Page two repeats the header (20px), so four more rows.
        assert_eq!(f.starts, vec![0.0, 100.0, 180.0], "{:?}", f.starts);
        assert!(f.repeats[0].is_none());
        let r = f.repeats[1].as_ref().expect("page two repeats the header");
        assert_eq!((r.y, r.height), (0.0, 20.0));
        assert!(f.repeats[2].is_some());
        // Without a header nothing repeats and the pages are as tall as the paper.
        let plain = Document::parse(&format!("<style>body{{margin:0}} table{{border-spacing:0}} td{{padding:0;height:20px}}</style><table><tbody>{rows}</tbody></table>"));
        let ps = compute_styles(&plain);
        let t = layout(&plain, &ps, &mut text, 400.0, 1.0);
        let f = paginate_tree(&t, &plain, &ps, 100.0);
        assert_eq!(f.starts, vec![0.0, 100.0]);
        assert!(f.repeats.iter().all(Option::is_none));
    }

    #[test]
    fn table_footers_repeat_at_the_bottom_of_pages_the_table_goes_on_from() {
        let rows: String = (0..10).map(|i| format!("<tr><td>r{i}</td></tr>")).collect();
        let doc = Document::parse(&format!(
            "<style>body{{margin:0}} table{{border-spacing:0}} td,th{{padding:0;height:20px}}</style><table><thead><tr><th>h</th></tr></thead><tbody>{rows}</tbody><tfoot><tr><td>total</td></tr></tfoot></table>"
        ));
        let styles = compute_styles(&doc);
        let mut text = nile_text::TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 1.0);
        let f = paginate_tree(&tree, &doc, &styles, 100.0);
        // The footer (20px) is kept at the bottom of every page but the last, which shows the real one.
        assert_eq!(f.feet.len(), f.starts.len());
        let last = f.starts.len() - 1;
        assert!(f.feet[..last].iter().all(Option::is_some), "{:?}", f.feet);
        assert!(f.feet[last].is_none());
        assert_eq!(f.starts[..3], [0.0, 80.0, 140.0], "{:?}", f.starts);
    }
}
