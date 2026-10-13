//! `@page`: paper size, margins and the margin boxes (headers, footers, page numbers) of paged output.

use std::cell::Cell;

use crate::parse::{parse_values, Cx};
use crate::Color;

/// Where a margin box sits on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarginPlace {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// One piece of a margin box's `content`.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Text(String),
    /// `counter(page)`.
    Page,
    /// `counter(pages)`.
    Pages,
}

/// A margin box such as `@bottom-center { content: "Page " counter(page) }`.
#[derive(Debug, Clone, PartialEq)]
pub struct MarginBox {
    pub place: MarginPlace,
    pub content: Vec<Piece>,
    pub font_size: f32,
    pub color: Color,
}

impl MarginBox {
    /// The text on page `page` (1-based) of `pages`.
    pub fn text(&self, page: usize, pages: usize) -> String {
        self.content
            .iter()
            .map(|p| match p {
                Piece::Text(t) => t.clone(),
                Piece::Page => page.to_string(),
                Piece::Pages => pages.to_string(),
            })
            .collect()
    }
}

/// The resolved `@page` rules for one page.
#[derive(Debug, Clone, PartialEq)]
pub struct PageStyle {
    /// Paper size in CSS px (96 per inch).
    pub size: (f32, f32),
    /// Margins in px: top, right, bottom, left.
    pub margin: [f32; 4],
    pub boxes: Vec<MarginBox>,
}

impl Default for PageStyle {
    /// A4 portrait with 0.75 inch margins.
    fn default() -> Self {
        PageStyle { size: (793.7, 1122.5), margin: [72.0; 4], boxes: Vec::new() }
    }
}

impl PageStyle {
    /// The area content flows into.
    pub fn content_size(&self) -> (f32, f32) {
        (
            (self.size.0 - self.margin[1] - self.margin[3]).max(1.0),
            (self.size.1 - self.margin[0] - self.margin[2]).max(1.0),
        )
    }

    /// Applies an `@page` body (declarations and margin-box blocks).
    fn apply(&mut self, body: &str) {
        let (decls, blocks) = split_body(body);
        for (name, value) in decls {
            match name.as_str() {
                "size" => {
                    if let Some(size) = parse_size(&value) {
                        self.size = size;
                    }
                }
                "margin" => {
                    let l: Vec<f32> = value.split_whitespace().filter_map(length).collect();
                    self.margin = match l[..] {
                        [a] => [a; 4],
                        [v, h] => [v, h, v, h],
                        [t, h, b] => [t, h, b, h],
                        [t, r, b, l] => [t, r, b, l],
                        _ => self.margin,
                    };
                }
                "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
                    if let Some(px) = length(value.trim()) {
                        let i = ["margin-top", "margin-right", "margin-bottom", "margin-left"].iter().position(|n| *n == name).unwrap_or(0);
                        self.margin[i] = px;
                    }
                }
                _ => {}
            }
        }
        for (name, inner) in blocks {
            let place = match name.as_str() {
                "top-left" => MarginPlace::TopLeft,
                "top-center" => MarginPlace::TopCenter,
                "top-right" => MarginPlace::TopRight,
                "bottom-left" => MarginPlace::BottomLeft,
                "bottom-center" => MarginPlace::BottomCenter,
                "bottom-right" => MarginPlace::BottomRight,
                _ => continue,
            };
            let (decls, _) = split_body(&inner);
            let mut mb = MarginBox { place, content: Vec::new(), font_size: 11.0, color: Color::BLACK };
            for (n, v) in decls {
                match n.as_str() {
                    "content" => mb.content = parse_content(&v),
                    "font-size" => mb.font_size = length(v.trim()).unwrap_or(mb.font_size),
                    "color" => {
                        let used = Cell::new(false);
                        let cx = Cx { viewport: (800.0, 600.0), uses_viewport: &used };
                        if let Some(c) = parse_values(&v, &cx).first().and_then(|v| v.color()) {
                            mb.color = c;
                        }
                    }
                    _ => {}
                }
            }
            self.boxes.retain(|b| b.place != place);
            if mb.content.iter().any(|p| !matches!(p, Piece::Text(t) if t.is_empty())) {
                self.boxes.push(mb);
            }
        }
    }
}

/// The `@page` rules of the sheets as `(selector, body)`, resolved for page `index` (0-based) of a
/// document whose first page is `:first`; pages alternate `:right` (odd) and `:left` (even).
pub fn resolve(rules: &[(String, String)], index: usize) -> PageStyle {
    let mut style = PageStyle::default();
    let side = if index.is_multiple_of(2) { ":right" } else { ":left" };
    for pass in 0..2 {
        for (sel, body) in rules {
            let sel = sel.trim();
            let specific = sel == side || (sel == ":first" && index == 0);
            if (pass == 0 && sel.is_empty()) || (pass == 1 && specific) {
                style.apply(body);
            }
        }
    }
    style
}

/// Top-level `name: value;` declarations and `@name { ... }` blocks in an `@page` body.
type Pairs = Vec<(String, String)>;

fn split_body(body: &str) -> (Pairs, Pairs) {
    let (mut decls, mut blocks) = (Vec::new(), Vec::new());
    let mut rest = body.trim();
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('@') {
            let Some(open) = r.find('{') else { break };
            let name = r[..open].trim().to_ascii_lowercase();
            let mut depth = 0;
            let mut end = r.len();
            for (i, c) in r.char_indices().skip(open) {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            blocks.push((name, r[open + 1..end].to_string()));
            rest = r.get(end + 1..).unwrap_or("").trim_start();
        } else {
            // A declaration ends at the next `;` that isn't inside quotes.
            let mut quote = None;
            let mut end = rest.len();
            for (i, c) in rest.char_indices() {
                match (quote, c) {
                    (None, '"' | '\'') => quote = Some(c),
                    (Some(q), c) if c == q => quote = None,
                    (None, ';') => {
                        end = i;
                        break;
                    }
                    _ => {}
                }
            }
            if let Some((n, v)) = rest[..end].split_once(':') {
                decls.push((n.trim().to_ascii_lowercase(), v.trim().to_string()));
            }
            rest = rest.get(end + 1..).unwrap_or("").trim_start();
        }
    }
    (decls, blocks)
}

/// A length in px; `in`, `cm`, `mm`, `q`, `pt` and `pc` convert at 96 px per inch.
fn length(s: &str) -> Option<f32> {
    let s = s.trim().to_ascii_lowercase();
    let at = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))?;
    let (n, unit) = s.split_at(at);
    let n: f32 = n.parse().ok()?;
    Some(match unit {
        "px" => n,
        "in" => n * 96.0,
        "cm" => n * 96.0 / 2.54,
        "mm" => n * 96.0 / 25.4,
        "q" => n * 96.0 / 101.6,
        "pt" => n * 96.0 / 72.0,
        "pc" => n * 16.0,
        _ => return None,
    })
}

/// `size`: a paper name, one or two lengths, and/or `landscape`/`portrait`.
fn parse_size(value: &str) -> Option<(f32, f32)> {
    let mut size = None;
    let mut landscape = false;
    let mut lens = Vec::new();
    for word in value.split_whitespace() {
        let w = word.to_ascii_lowercase();
        let mm = |a: f32, b: f32| (a * 96.0 / 25.4, b * 96.0 / 25.4);
        match w.as_str() {
            "landscape" => landscape = true,
            "portrait" | "auto" => {}
            "a3" => size = Some(mm(297.0, 420.0)),
            "a4" => size = Some(mm(210.0, 297.0)),
            "a5" => size = Some(mm(148.0, 210.0)),
            "b4" => size = Some(mm(250.0, 353.0)),
            "b5" => size = Some(mm(176.0, 250.0)),
            "letter" => size = Some((8.5 * 96.0, 11.0 * 96.0)),
            "legal" => size = Some((8.5 * 96.0, 14.0 * 96.0)),
            "ledger" => size = Some((11.0 * 96.0, 17.0 * 96.0)),
            _ => lens.push(length(&w)?),
        }
    }
    let size = match lens[..] {
        [a] => (a, a),
        [a, b] => (a, b),
        _ => size?,
    };
    Some(if landscape { (size.0.max(size.1), size.0.min(size.1)) } else { size })
}

/// `content`: quoted strings and `counter(page)` / `counter(pages)`.
fn parse_content(value: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut rest = value.trim();
    while !rest.is_empty() {
        let c = rest.chars().next().unwrap_or(' ');
        if c == '"' || c == '\'' {
            let Some(end) = rest[1..].find(c) else { break };
            out.push(Piece::Text(rest[1..1 + end].replace("\\A", "\n")));
            rest = rest[end + 2..].trim_start();
        } else if let Some(r) = rest.strip_prefix("counter(") {
            let Some(end) = r.find(')') else { break };
            match r[..end].trim() {
                "page" => out.push(Piece::Page),
                "pages" => out.push(Piece::Pages),
                _ => {}
            }
            rest = r[end + 1..].trim_start();
        } else {
            // Skip a word we don't know.
            rest = rest.split_once(char::is_whitespace).map_or("", |(_, r)| r.trim_start());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(css: &str) -> Vec<(String, String)> {
        crate::Stylesheet::parse(css).pages
    }

    #[test]
    fn page_size_margins_and_margin_boxes() {
        let r = rules(r#"@page { size: letter landscape; margin: 1in 0.5in; @bottom-center { content: "Page " counter(page) " of " counter(pages); font-size: 9pt; color: #666 } }
                         @page :first { margin-top: 2in; @bottom-center { content: "" } }"#);
        let p = resolve(&r, 1);
        assert_eq!(p.size, (11.0 * 96.0, 8.5 * 96.0));
        assert_eq!(p.margin, [96.0, 48.0, 96.0, 48.0]);
        assert_eq!(p.boxes.len(), 1);
        assert_eq!(p.boxes[0].text(2, 5), "Page 2 of 5");
        assert_eq!(p.boxes[0].font_size, 12.0);
        let first = resolve(&r, 0);
        assert_eq!(first.margin[0], 192.0);
        assert!(first.boxes.is_empty(), "the first page's own empty box replaces the footer");
        assert_eq!(resolve(&[], 0), PageStyle::default());
        assert_eq!(parse_size("a4").map(|s| s.0.round()), Some(794.0));
        assert_eq!(parse_size("10cm 5cm").map(|s| (s.0.round(), s.1.round())), Some((378.0, 189.0)));
    }
}
