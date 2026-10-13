//! Where a laid-out page breaks when it is cut into pages of a fixed height.
//!
//! Layout stays one long column; pagination only chooses the vertical offsets at which a new page
//! starts. A cut may not fall inside a text line, a table row, a picture or an element with
//! `break-inside: avoid` (when it fits on a page), nor leave fewer than two lines of a paragraph
//! on either side (`orphans`/`widows`); `break-before/after: page` force one.

use nile_style::{Break, Display, StyleMap};

use crate::{BoxContent, LayoutTree};

/// Page start offsets (the first is 0) for `tree` cut into pages `page_h` tall.
pub fn paginate(tree: &LayoutTree, styles: &StyleMap, page_h: f32) -> Vec<f32> {
    let page_h = page_h.max(1.0);
    let total = tree.content_height;
    let mut forced: Vec<f32> = Vec::new();
    // Open intervals no cut may fall into, split by how important they are.
    let mut lines: Vec<(f32, f32)> = Vec::new();
    let mut blocks: Vec<(f32, f32)> = Vec::new();
    for b in &tree.boxes {
        match &b.content {
            BoxContent::Element => {
                let s = styles.get(b.node);
                if s.display == Display::None || s.display == Display::Inline {
                    continue;
                }
                let (y0, y1) = (b.rect.y, b.rect.y + b.rect.height);
                match s.break_before {
                    Break::Always if y0 > 0.5 => forced.push(y0),
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
    forced.sort_by(f32::total_cmp);
    let inside = |set: &[(f32, f32)], y: f32| set.iter().find(|&&(a, b)| y > a + 0.01 && y < b - 0.01).copied();

    let mut starts = vec![0.0f32];
    let mut start = 0.0f32;
    while starts.len() < 10_000 {
        let limit = start + page_h;
        if let Some(&f) = forced.iter().find(|&&f| f > start + 0.5 && f <= limit + 0.01) {
            starts.push(f);
            start = f;
            continue;
        }
        if total - start <= page_h + 0.5 {
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
        starts.push(cut);
        start = cut;
    }
    starts
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
}
