//! Inline `<svg>`: the element's subtree written back out as an SVG document for nile-svg.
//!
//! The HTML parser keeps SVG's camel-case names (`viewBox`, `linearGradient`), but script-made
//! elements and `setAttribute` are lower-cased by the DOM, so the names are put back from a table.

use nile_dom::{Document, NodeId, NodeKind};
use nile_style::Color;

/// Attribute carrying an element's node id into the SVG document.
const MARK: &str = "data-nile-svg";

const CAMEL_ATTRS: &[&str] = &[
    "viewBox", "preserveAspectRatio", "gradientUnits", "gradientTransform", "patternUnits", "patternContentUnits", "patternTransform",
    "clipPathUnits", "maskUnits", "maskContentUnits", "markerWidth", "markerHeight", "markerUnits", "refX", "refY", "orient",
    "stdDeviation", "spreadMethod", "startOffset", "textLength", "lengthAdjust", "baseFrequency", "numOctaves", "filterUnits",
    "primitiveUnits", "targetX", "targetY", "tableValues", "pathLength", "kernelMatrix", "kernelUnitLength", "surfaceScale",
    "specularConstant", "specularExponent", "diffuseConstant", "limitingConeAngle", "pointsAtX", "pointsAtY", "pointsAtZ",
    "xChannelSelector", "yChannelSelector", "edgeMode", "stitchTiles", "fx", "fy", "gradientUnits", "repeatCount", "repeatDur",
    "attributeName", "attributeType", "keyTimes", "keySplines", "calcMode", "requiredExtensions", "requiredFeatures",
];

const CAMEL_TAGS: &[&str] = &[
    "linearGradient", "radialGradient", "clipPath", "textPath", "foreignObject", "feBlend", "feColorMatrix", "feComponentTransfer",
    "feComposite", "feConvolveMatrix", "feDiffuseLighting", "feDisplacementMap", "feDistantLight", "feDropShadow", "feFlood",
    "feFuncA", "feFuncB", "feFuncG", "feFuncR", "feGaussianBlur", "feImage", "feMerge", "feMergeNode", "feMorphology", "feOffset",
    "fePointLight", "feSpecularLighting", "feSpotLight", "feTile", "feTurbulence", "animateMotion", "animateTransform", "altGlyph",
    "glyphRef", "altGlyphDef", "altGlyphItem",
];

fn restore<'a>(name: &'a str, table: &'a [&'a str]) -> &'a str {
    table.iter().find(|c| c.eq_ignore_ascii_case(name)).copied().unwrap_or(name)
}

fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn write(doc: &Document, id: NodeId, root: bool, color: Option<Color>, out: &mut String) {
    let node = doc.node(id);
    match &node.kind {
        NodeKind::Text(t) => escape(t, out),
        NodeKind::Element { tag, attrs } => {
            let name = restore(tag, CAMEL_TAGS);
            out.push('<');
            out.push_str(name);
            let has = |n: &str| attrs.iter().any(|(k, _)| k == n);
            if root {
                if !has("xmlns") {
                    out.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
                }
                if !has("xmlns:xlink") {
                    out.push_str(" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
                }
                // Text colour reaches `currentColor` through the root's `color` attribute.
                if let (Some(c), false) = (color, has("color")) {
                    out.push_str(&format!(" color=\"rgba({}, {}, {}, {})\"", c.r, c.g, c.b, f32::from(c.a) / 255.0));
                }
            }
            // Which page rule reaches which element is decided here, with the page's selectors, and
            // handed to usvg as rules on this attribute (see `page_rules`).
            out.push_str(&format!(" {MARK}=\"{}\"", id.0));
            for (k, v) in attrs {
                if root && matches!(k.as_str(), "xmlns") {
                    continue;
                }
                out.push(' ');
                out.push_str(restore(k, CAMEL_ATTRS));
                out.push_str("=\"");
                escape(v, out);
                out.push('"');
            }
            if node.children.is_empty() {
                out.push_str("/>");
            } else {
                out.push('>');
                for c in &node.children {
                    write(doc, *c, false, None, out);
                }
                out.push_str("</");
                out.push_str(name);
                out.push('>');
            }
        }
        _ => {}
    }
}

/// The `<svg>` element `id` as a standalone document; `color` is the element's CSS colour.
pub fn serialize(doc: &Document, id: NodeId, color: Option<Color>) -> String {
    let mut out = String::new();
    write(doc, id, true, color, &mut out);
    out
}

/// The page's style rules that reach elements inside the `<svg>` `root`, rewritten for usvg.
///
/// usvg applies a style sheet itself but only sees the SVG, so a rule like `.themed .hot` whose
/// `.themed` is outside it wouldn't match. Each rule's selectors are tried against the real DOM
/// instead, and every matching element gets the rule's declarations through its marker attribute.
/// Source order is kept; specificity between page rules is not (later wins). At-rules are skipped.
pub fn page_rules(doc: &Document, root: NodeId) -> Option<String> {
    let mut css = String::new();
    for n in &doc.nodes {
        if n.tag() == Some("style") {
            for c in &n.children {
                if let NodeKind::Text(t) = &doc.node(*c).kind {
                    css.push_str(t);
                    css.push('\n');
                }
            }
        }
    }
    let css = strip_comments(&css);
    let members: Vec<NodeId> = std::iter::once(root).chain(doc.descendants(root)).filter(|n| matches!(doc.node(*n).kind, NodeKind::Element { .. })).collect();
    let mut out = String::new();
    let mut rest = css.as_str();
    while let Some(open) = rest.find('{') {
        let selectors = rest[..open].trim();
        let (body, after) = match balanced(&rest[open..]) {
            Some((inner, len)) => (inner, &rest[open + len..]),
            None => break,
        };
        rest = after;
        if selectors.starts_with('@') || body.contains('{') {
            continue;
        }
        let body = body.trim();
        for selector in split_selectors(selectors) {
            let Ok(query) = nile_style::Query::parse(selector) else { continue };
            for &n in &members {
                if query.matches(doc, n) {
                    out.push_str(&format!("[{MARK}=\"{}\"] {{ {body} }}\n", n.0));
                }
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..].split_once("*/").map_or("", |(_, after)| after);
    }
    out.push_str(rest);
    out
}

/// For `{ ... }` at the start of `s`: the inside and the length through the closing brace.
fn balanced(s: &str) -> Option<(&str, usize)> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[1..i], i + 1));
                }
            }
            _ => {}
        }
    }
    None
}

/// A selector list split at its top-level commas.
fn split_selectors(list: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut start) = (Vec::new(), 0, 0);
    for (i, c) in list.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(list[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(list[start..].trim());
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inline_svg_is_written_back_with_its_original_names() {
        let doc = Document::parse(r#"<body><svg id="s" viewBox="0 0 10 10"><defs><linearGradient id="g"><stop offset="0" stop-color="red"/></linearGradient></defs><rect width="10" height="10" fill="url(#g)"/><text>a &amp; b</text></svg></body>"#);
        let id = doc.find(|n| n.attr("id") == Some("s")).unwrap();
        let out = serialize(&doc, id, Some(Color::rgb(1, 2, 3)));
        assert!(out.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""), "{out}");
        assert!(out.contains("viewBox=\"0 0 10 10\"") && out.contains("<linearGradient ") && out.contains("</linearGradient>"), "{out}");
        assert!(out.contains("color=\"rgba(1, 2, 3, 1)\"") && out.contains(">a &amp; b</text>"), "{out}");
        assert!(nile_svg::size(out.as_bytes()).is_some(), "usvg reads it back");
    }

    #[test]
    fn page_rules_are_matched_against_the_real_dom() {
        let doc = Document::parse(r#"<style>/* c */ .themed .hot, .none { fill: red } @media print { .hot { fill: blue } } .other { fill: green }</style>
            <body class="x"><div class="themed"><svg id="s"><rect class="hot" id="r"/><rect id="p"/></svg></div></body>"#);
        let root = doc.find(|n| n.attr("id") == Some("s")).unwrap();
        let rect = doc.find(|n| n.attr("id") == Some("r")).unwrap();
        let css = page_rules(&doc, root).unwrap();
        assert!(css.contains(&format!("[data-nile-svg=\"{}\"] {{ fill: red }}", rect.0)), "{css}");
        assert!(!css.contains("blue") && !css.contains("green"), "at-rules and non-matching rules are left out: {css}");
        assert_eq!(css.matches("fill: red").count(), 1, "only the one element matches");
        let plain = Document::parse("<svg id=\"s\"><rect/></svg>");
        assert!(page_rules(&plain, plain.find(|n| n.attr("id") == Some("s")).unwrap()).is_none());
    }
}
