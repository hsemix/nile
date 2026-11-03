//! Sciter's CSS spellings, rewritten as text before cssparser sees them:
//! `var(name): value` declares a custom property (`--name`), `var(name)`, `color(name)` and
//! `length(name)` read one, and `url(path:M0 0 L10 10)` (an SVG path with spaces, which is not a
//! valid unquoted URL) gets quotes (docs.sciter.com/docs/CSS/variables-and-attributes).

use std::borrow::Cow;

/// Colour spaces of the standard `color()` function, which must be left alone.
const SPACES: [&str; 10] = ["srgb", "srgb-linear", "display-p3", "a98-rgb", "prophoto-rgb", "rec2020", "xyz", "xyz-d50", "xyz-d65", "from"];

/// Sciter style sheets allow `//` comments to the end of the line; without this the comment and the declaration after it would
/// be read as one broken declaration. Strings, `url(...)` and `/* ... */` comments are left alone (`url(http://x)`).
fn strip_line_comments(css: &str) -> Cow<'_, str> {
    if !css.contains("//") {
        return Cow::Borrowed(css);
    }
    let b = css.as_bytes();
    let mut out = String::with_capacity(css.len());
    let (mut i, mut at) = (0, 0);
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'*') => i = css[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2),
            b'/' if b.get(i + 1) == Some(&b'/') => {
                out.push_str(&css[at..i]);
                i = css[i..].find('\n').map_or(b.len(), |e| i + e);
                at = i;
            }
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'u' | b'U' if css[i..].len() > 4 && css[i..i + 4].eq_ignore_ascii_case("url(") && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'-')) => {
                // An unquoted url runs to its closing parenthesis, whatever it contains.
                i += 4;
                while i < b.len() && b[i] != b')' {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out.push_str(&css[at.min(css.len())..]);
    Cow::Owned(out)
}

pub(crate) fn rewrite(css: &str) -> Cow<'_, str> {
    match strip_line_comments(css) {
        Cow::Borrowed(same) => rewrite_names(same),
        Cow::Owned(stripped) => Cow::Owned(rewrite_names(&stripped).into_owned()),
    }
}

fn rewrite_names(css: &str) -> Cow<'_, str> {
    // `html:theme(dark)` matches the element carrying (or inheriting) that `theme` attribute.
    if css.contains(":theme(") {
        let mut out = String::with_capacity(css.len());
        let mut rest = css;
        while let Some(i) = rest.find(":theme(") {
            out.push_str(&rest[..i]);
            let after = &rest[i + 7..];
            match after.find(')') {
                Some(c) => {
                    out.push_str(&format!("[theme=\"{}\"]", after[..c].trim().trim_matches(['"', '\''])));
                    rest = &after[c + 1..];
                }
                None => {
                    out.push_str(&rest[i..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        return Cow::Owned(rewrite_names(&out).into_owned());
    }
    if !(css.contains("var(") || css.contains("color(") || css.contains("length(") || css.contains("path:") || css.contains("stock:")) {
        return Cow::Borrowed(css);
    }
    let b = css.as_bytes();
    let mut out = String::with_capacity(css.len() + 16);
    let mut i = 0;
    let mut at = 0; // css[at..i] is not yet copied
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'-' || c == b'_';
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = css[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
            }
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            c if c.is_ascii_alphabetic() && (i == 0 || !ident(b[i - 1])) => {
                let start = i;
                while i < b.len() && ident(b[i]) {
                    i += 1;
                }
                if b.get(i) != Some(&b'(') {
                    continue;
                }
                let name = css[start..i].to_ascii_lowercase();
                let Some(close) = css[i..].find(')').map(|c| i + c) else { continue };
                let inner = css[i + 1..close].trim();
                let simple = !inner.is_empty() && inner.bytes().all(ident);
                let replacement = match name.as_str() {
                    "url" if inner.starts_with("path:") || inner.starts_with("stock:") => Some(format!("url(\"{inner}\")")),
                    // `var(name): value` declares, `var(name, fallback)` / `var(name)` reads.
                    "var" if simple && css[close + 1..].trim_start().starts_with(':') => Some(format!("--{inner}")),
                    "var" if !inner.starts_with("--") && inner.as_bytes().first().is_some_and(|c| ident(*c)) => Some(format!("var(--{inner})")),
                    "color" | "length" if simple && !SPACES.contains(&inner.to_ascii_lowercase().as_str()) => Some(format!("var(--{inner})")),
                    _ => None,
                };
                if let Some(r) = replacement {
                    out.push_str(&css[at..start]);
                    out.push_str(&r);
                    at = close + 1;
                    i = close + 1;
                }
            }
            _ => i += 1,
        }
    }
    out.push_str(&css[at..]);
    Cow::Owned(out)
}

/// A named length of the window chrome (`window-caption-height` ...), in px.
pub(crate) fn window_constant(name: &str) -> Option<f32> {
    Some(match name {
        "window-caption-height" | "window-button-height" => 32.0,
        "window-button-width" => 46.0,
        // The resize border is drawn by the window itself, so the page's frame is a hairline; macOS draws none.
        "window-frame-width" => {
            if cfg!(target_os = "macos") {
                0.0
            } else {
                1.0
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sciter_spellings_become_standard_css() {
        assert_eq!(rewrite("html { var(text-color): #fff; color: color(text-color); width: length(w); height: var(h, 80px) }"), "html { --text-color: #fff; color: var(--text-color); width: var(--w); height: var(--h, 80px) }");
        assert_eq!(rewrite("a { background-image: url(path:M0 0 L10 10 z); b: url(stock:arrow-right) }"), "a { background-image: url(\"path:M0 0 L10 10 z\"); b: url(\"stock:arrow-right\") }");
        assert_eq!(rewrite("html:theme(dark) { var(c): #fff }"), "html[theme=\"dark\"] { --c: #fff }");
        let plain = "a { color: color(srgb 1 0 0); b: var(--x); c: url(x.png); /* var(a): b */ d: \"var(a)\" }";
        assert_eq!(rewrite(plain), plain);
    }

    #[test]
    fn line_comments_end_at_the_line_but_not_inside_urls_or_strings() {
        assert_eq!(rewrite("a { color: red; // note\n  width: 5px; // more\n}"), "a { color: red; \n  width: 5px; \n}");
        let kept = "a { background: url(http://x.org/a.png); content: \"//\"; /* // */ b: c }";
        assert_eq!(rewrite(kept), kept);
        assert_eq!(rewrite("// first line\na { b: c }"), "\na { b: c }");
    }
}
