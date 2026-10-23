//! `<link rel="stylesheet" href="app.css">` and `@import "other.css"`: external style sheets are
//! spliced in as `<style>` text before parsing, like `<include>` fragments, so the cascade only
//! ever sees inline sheets. `media` on the link becomes an `@media` wrapper.

use crate::include::{attributes, tag_end};

const MAX_DEPTH: usize = 8;

/// Replaces each stylesheet `<link>` in `html` with a `<style>` holding `load(href)`'s text (with its
/// own `@import`s inlined, paths relative to the importing file). A link that can't be loaded is dropped.
pub fn inline_links(html: &str, load: &mut dyn FnMut(&str) -> Option<String>) -> String {
    let html = &inline_style_src(html, load);
    let lower = html.to_ascii_lowercase();
    if !lower.contains("<link") {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    while let Some(start) = lower[at..].find("<link").map(|i| i + at) {
        // Links inside comments stay comments.
        if let Some(c) = lower[at..start].rfind("<!--") {
            if !lower[at + c..start].contains("-->") {
                let end = lower[start..].find("-->").map_or(html.len(), |e| start + e + 3);
                out.push_str(&html[at..end]);
                at = end;
                continue;
            }
        }
        let after = start + "<link".len();
        let Some(end) = (matches!(html.as_bytes().get(after), Some(b) if b.is_ascii_whitespace() || *b == b'/' || *b == b'>')).then(|| tag_end(html, after)).flatten() else {
            out.push_str(&html[at..after]);
            at = after;
            continue;
        };
        let attrs = attributes(&html[after..end]);
        let get = |n: &str| attrs.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
        out.push_str(&html[at..start]);
        at = end + 1;
        let is_sheet = get("rel").is_some_and(|r| r.split_whitespace().any(|t| t.eq_ignore_ascii_case("stylesheet")));
        match (is_sheet, get("href")) {
            (true, Some(href)) => {
                if let Some(css) = load(href).map(|css| inline_imports(&rebase_urls(&css, dir_of(href)), dir_of(href), load, 0)) {
                    let css = match get("media").map(str::trim).filter(|m| !m.is_empty() && *m != "all") {
                        Some(m) => format!("@media {m} {{\n{css}\n}}"),
                        None => css,
                    };
                    out.push_str(&format!("<style data-href=\"{}\">{css}</style>", href.replace('"', "&quot;")));
                }
            }
            _ => out.push_str(&html[start..at]),
        }
    }
    out.push_str(&html[at..]);
    out
}

/// Sciter's `<style src="a.css" />` (also `<style src=a.css></style>`): the file's text becomes the
/// style's content. The HTML parser would read a self-closing `<style/>` as an open raw-text element
/// and swallow the rest of the page, so this runs first. Ordinary `<style>` blocks are left alone.
fn inline_style_src(html: &str, load: &mut dyn FnMut(&str) -> Option<String>) -> String {
    let lower = html.to_ascii_lowercase();
    if !lower.contains("<style") {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    while let Some(start) = lower[at..].find("<style").map(|i| i + at) {
        let after = start + "<style".len();
        let delimited = matches!(html.as_bytes().get(after), Some(b) if b.is_ascii_whitespace() || *b == b'/' || *b == b'>');
        let comment = lower[at..start].rfind("<!--").is_some_and(|c| !lower[at + c..start].contains("-->"));
        let Some(end) = (delimited && !comment).then(|| tag_end(html, after)).flatten() else {
            out.push_str(&html[at..after]);
            at = after;
            continue;
        };
        let attrs = attributes(&html[after..end]);
        let get = |n: &str| attrs.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
        let self_closing = html[after..end].trim_end().ends_with('/');
        // The element's end: `/>`, or the matching `</style>`.
        let resume = if self_closing {
            end + 1
        } else {
            lower[end + 1..].find("</style").and_then(|c| lower[end + 1 + c..].find('>').map(|g| end + 1 + c + g + 1)).unwrap_or(html.len())
        };
        let Some(src) = get("src") else {
            out.push_str(&html[at..resume]);
            at = resume;
            continue;
        };
        out.push_str(&html[at..start]);
        if let Some(css) = load(src).map(|css| inline_imports(&rebase_urls(&css, dir_of(src)), dir_of(src), load, 0)) {
            let media = get("media").map(str::trim).filter(|m| !m.is_empty() && *m != "all");
            let css = match media {
                Some(m) => format!("@media {m} {{\n{css}\n}}"),
                None => css,
            };
            out.push_str(&format!("<style data-href=\"{}\">{css}</style>", src.replace('"', "&quot;")));
        }
        at = resume;
    }
    out.push_str(&html[at..]);
    out
}

/// `url(x)` in a style sheet is relative to the sheet, but the sheet's text ends up inside the
/// page, so relative references get the sheet's folder (`dir`, relative to the page) put in front.
/// Absolute paths, `data:`/`http:` style schemes (and Sciter's `path:`) and `#fragment`s stay as they are.
fn rebase_urls(css: &str, dir: &str) -> String {
    if dir.is_empty() || !css.contains("url(") {
        return css.to_string();
    }
    let mut out = String::with_capacity(css.len() + 32);
    let mut rest = css;
    while let Some(i) = rest.find("url(") {
        let (head, tail) = rest.split_at(i + 4);
        out.push_str(head);
        let Some(close) = tail.find(')') else {
            rest = tail;
            break;
        };
        let inner = tail[..close].trim();
        let (quote, target) = match inner.chars().next() {
            Some(q @ ('"' | '\'')) if inner.len() > 1 && inner.ends_with(q) => (Some(q), &inner[1..inner.len() - 1]),
            _ => (None, inner),
        };
        let statement = out.rfind([';', '}']).map_or(0, |p| p + 1);
        let leave = out[statement..].contains("@import") || target.is_empty() || target.starts_with(['/', '#', '\\']) || target.contains(':');
        if leave {
            out.push_str(&tail[..close]);
        } else {
            let q = quote.map(String::from).unwrap_or_default();
            out.push_str(&format!("{q}{dir}{target}{q}"));
        }
        rest = &tail[close..];
    }
    out.push_str(rest);
    out
}

fn dir_of(path: &str) -> &str {
    path.rfind(['/', '\\']).map_or("", |i| &path[..=i])
}

/// `css` with each `@import "x.css";` / `@import url(x.css) screen;` replaced by that file's text.
pub fn inline_imports(css: &str, dir: &str, load: &mut dyn FnMut(&str) -> Option<String>, depth: usize) -> String {
    if depth >= MAX_DEPTH || !css.contains("@import") {
        return css.to_string();
    }
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("@import") {
        out.push_str(&rest[..i]);
        let after = &rest[i + "@import".len()..];
        let Some(semi) = after.find(';') else {
            out.push_str(&rest[i..]);
            rest = "";
            break;
        };
        let spec = after[..semi].trim();
        rest = &after[semi + 1..];
        // `url("x")`, `url(x)` or a quoted string, then optional media.
        let (target, media) = match spec.strip_prefix("url(") {
            Some(r) => match r.find(')') {
                Some(c) => (r[..c].trim().trim_matches(['"', '\'']), r[c + 1..].trim()),
                None => continue,
            },
            None => match spec.chars().next() {
                Some(q @ ('"' | '\'')) => match spec[1..].find(q) {
                    Some(c) => (&spec[1..1 + c], spec[c + 2..].trim()),
                    None => continue,
                },
                _ => continue,
            },
        };
        let path = format!("{dir}{target}");
        if let Some(text) = load(&path) {
            let text = inline_imports(&rebase_urls(&text, dir_of(&path)), dir_of(&path), load, depth + 1);
            if media.is_empty() {
                out.push_str(&text);
            } else {
                out.push_str(&format!("@media {media} {{\n{text}\n}}"));
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(name: &str) -> Option<String> {
        match name {
            "a.css" => Some("@import \"sub/b.css\";\nbody { color: red }".into()),
            "sub/b.css" => Some("@import url(c.css) screen;\np { margin: 0 }".into()),
            "sub/c.css" => Some("i { color: blue }".into()),
            _ => None,
        }
    }

    #[test]
    fn urls_in_a_sheet_are_relative_to_the_sheet() {
        let css = "@import url(c.css);\n@font-face { src: url(f.ttf) } a { background: url(\"i.png\") } b { background: url(data:x) url(/abs.png) url(#f) url(path:M0 0) }";
        assert_eq!(
            rebase_urls(css, "css/"),
            "@import url(c.css);\n@font-face { src: url(css/f.ttf) } a { background: url(\"css/i.png\") } b { background: url(data:x) url(/abs.png) url(#f) url(path:M0 0) }"
        );
        assert_eq!(rebase_urls(css, ""), css);
    }

    #[test]
    fn style_src_loads_the_file_even_when_self_closed() {
        let html = "<head><style src=\"a.css\" /><style>p{margin:0}</style><style src=sub/c.css></style></head><p>kept</p>";
        let out = inline_links(html, &mut |p| files(p));
        assert!(out.contains("<style data-href=\"a.css\">"), "{out}");
        assert!(out.contains("body { color: red }") && out.contains("<style>p{margin:0}</style>"), "{out}");
        assert!(out.contains("i { color: blue }") && out.ends_with("</head><p>kept</p>"), "{out}");
    }

    #[test]
    fn links_become_styles_and_imports_resolve_relative_to_the_importer() {
        let html = r#"<head><link rel="stylesheet" href="a.css"><link rel=stylesheet href="print.css" media="print"><link rel="icon" href="x.png"><link rel="stylesheet" href="missing.css"></head><!-- <link rel="stylesheet" href="a.css"> -->"#;
        let out = inline_links(html, &mut |p| files(p));
        assert!(out.contains("<style data-href=\"a.css\">"), "{out}");
        assert!(out.contains("body { color: red }") && out.contains("p { margin: 0 }"), "{out}");
        assert!(out.contains("@media screen {\ni { color: blue }"), "{out}");
        assert!(out.contains(r#"<link rel="icon" href="x.png">"#), "other links stay: {out}");
        assert!(!out.contains("missing.css"), "an unloadable sheet is dropped: {out}");
        assert!(out.contains(r#"<!-- <link rel="stylesheet" href="a.css"> -->"#), "{out}");
        assert!(!out.contains("@import"), "{out}");
    }
}
