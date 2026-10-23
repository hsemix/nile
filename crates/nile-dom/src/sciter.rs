//! Sciter's looser document structure, handled as text before the HTML parser sees it:
//! custom `<!ENTITY name "value">` declarations and content standing between `</head>` and an
//! explicit `<body>` (the usual `<header>` caption of an extended window), which HTML5 parsing
//! would pull into an implicit body but Sciter leaves as children of `<html>`.

use crate::include::tag_end;
use std::collections::HashMap;

/// Reads each `<!ENTITY name "value">` out of `html` and replaces `&name;` with the value.
pub(crate) fn expand_entities(html: &str) -> String {
    if !html.contains("<!ENTITY") {
        return html.to_string();
    }
    let mut map: HashMap<&str, &str> = HashMap::new();
    let mut body = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find("<!ENTITY") {
        body.push_str(&rest[..i]);
        let decl = &rest[i + "<!ENTITY".len()..];
        let Some(end) = decl.find('>') else {
            rest = &decl[decl.len()..];
            break;
        };
        let spec = decl[..end].trim();
        if let Some((name, value)) = spec.split_once(char::is_whitespace) {
            let value = value.trim();
            if let Some(q) = value.chars().next().filter(|c| *c == '"' || *c == '\'') {
                if let Some(close) = value[1..].rfind(q) {
                    map.insert(name, &value[1..1 + close]);
                }
            }
        }
        rest = &decl[end + 1..];
    }
    body.push_str(rest);
    if map.is_empty() {
        return body;
    }
    let mut out = String::with_capacity(body.len());
    let mut tail = body.as_str();
    while let Some(i) = tail.find('&') {
        out.push_str(&tail[..i]);
        let after = &tail[i + 1..];
        let name_len = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(after.len());
        match (after[name_len..].starts_with(';'), map.get(&after[..name_len])) {
            (true, Some(value)) => {
                out.push_str(value);
                tail = &after[name_len + 1..];
            }
            _ => {
                out.push('&');
                tail = after;
            }
        }
    }
    out.push_str(tail);
    out
}

/// Wraps the markup between `<html ...>` (past its `</head>`) and the first explicit `<body>` in a
/// `<nile-hoist>` element; `Document::convert` re-attaches that element's children to `<html>`.
pub(crate) fn hoist_before_body(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let delimited = |at: usize, len: usize| matches!(lower.as_bytes().get(at + len), Some(b) if b.is_ascii_whitespace() || *b == b'>');
    let Some(body) = lower.find("<body").filter(|b| delimited(*b, 5)) else { return html.to_string() };
    let Some(open) = lower[..body].find("<html").filter(|h| delimited(*h, 5)) else { return html.to_string() };
    let Some(open_end) = tag_end(html, open) else { return html.to_string() };
    // The stretches that would otherwise land in an implicit body: before the head, if there is
    // one, and after it.
    let head = (open_end..body).find(|&i| lower[i..].starts_with("<head") && matches!(lower.as_bytes().get(i + 5), Some(b) if b.is_ascii_whitespace() || *b == b'>'));
    let head_end = head.and_then(|h| lower[h..body].find("</head>").map(|e| h + e + "</head>".len()));
    let mut spans: Vec<(usize, usize)> = Vec::new();
    match (head, head_end) {
        (Some(h), Some(e)) => spans.extend([(open_end + 1, h), (e, body)]),
        (None, _) => spans.push((open_end + 1, body)),
        _ => return html.to_string(),
    }
    let needs = |text: &str| {
        // Comments alone, or only head-type elements, don't need it.
        let mut rest = text;
        while let Some(i) = rest.find('<') {
            let tag: String = rest[i + 1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>().to_ascii_lowercase();
            if !tag.is_empty() && !matches!(tag.as_str(), "style" | "script" | "title" | "meta" | "link" | "base" | "include" | "head") {
                return true;
            }
            rest = &rest[i + 1..];
        }
        false
    };
    if !spans.iter().any(|(a, b)| needs(&html[*a..*b])) {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len() + 32);
    let mut at = 0;
    for (a, b) in spans {
        out.push_str(&html[at..a]);
        out.push_str("<nile-hoist>");
        out.push_str(&html[a..b]);
        out.push_str("</nile-hoist>");
        at = b;
    }
    out.push_str(&html[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_are_declared_and_replaced() {
        let html = "<!ENTITY fa-x \"&#xf000;\">\n<!ENTITY y 'Y'><p>&fa-x; &y; &amp; &nope;</p>";
        assert_eq!(expand_entities(html), "\n<p>&#xf000; Y &amp; &nope;</p>");
        assert_eq!(expand_entities("<p>&lt;</p>"), "<p>&lt;</p>");
    }

    #[test]
    fn markup_before_an_explicit_body_is_wrapped() {
        let html = "<html a=1><head><title>t</title></head><header>H</header><div>x</div><body>b</body></html>";
        assert_eq!(hoist_before_body(html), "<html a=1><nile-hoist></nile-hoist><head><title>t</title></head><nile-hoist><header>H</header><div>x</div></nile-hoist><body>b</body></html>");
        let plain = "<html><head></head><body>b</body></html>";
        assert_eq!(hoist_before_body(plain), plain);
        let heads = "<html><title>t</title><style>a{}</style><body>b</body></html>";
        assert_eq!(hoist_before_body(heads), heads);
    }
}
