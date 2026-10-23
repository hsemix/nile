//! `<include src="fragment.html" media="platform == 'Windows'">fallback</include>`: Sciter assembles
//! a document from fragments while it loads. The expansion is textual, before parsing, so an
//! include may sit anywhere (including `<head>`, where the HTML parser would otherwise close it).

const MAX_DEPTH: usize = 16;

/// Replaces every `<include>` in `html` with the text `load(src)` returns. An include whose `media`
/// is false or whose fragment can't be loaded is replaced by its own content (the fallback).
/// Fragments may include further fragments, up to `MAX_DEPTH` levels (which also stops cycles); a
/// nested `src` is relative to the fragment that names it, so `load` gets the joined path.
pub fn expand(html: &str, load: &mut dyn FnMut(&str) -> Option<String>) -> String {
    expand_at(html, "", load, 0)
}

fn expand_at(html: &str, dir: &str, load: &mut dyn FnMut(&str) -> Option<String>, depth: usize) -> String {
    let lower = html.to_ascii_lowercase();
    if !lower.contains("<include") {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    while at < html.len() {
        let rest = &lower[at..];
        let comment = rest.find("<!--");
        let Some(start) = rest.find("<include").map(|i| i + at) else { break };
        // Includes inside comments stay comments.
        if let Some(c) = comment {
            if c + at < start {
                let end = rest[c..].find("-->").map(|e| c + e + 3).unwrap_or(rest.len());
                out.push_str(&html[at..at + end]);
                at += end;
                continue;
            }
        }
        let after = start + "<include".len();
        match html.as_bytes().get(after) {
            Some(b) if b.is_ascii_whitespace() || *b == b'/' || *b == b'>' => {}
            _ => {
                out.push_str(&html[at..after]);
                at = after;
                continue;
            }
        }
        let Some(tag_end) = tag_end(html, after) else { break };
        let tag = &html[after..tag_end]; // attributes, maybe a trailing '/'
        let self_closing = tag.trim_end().ends_with('/');
        let (inner, resume) = if self_closing {
            ("", tag_end + 1)
        } else {
            match lower[tag_end + 1..].find("</include") {
                Some(close) => {
                    let inner_end = tag_end + 1 + close;
                    let resume = lower[inner_end..].find('>').map(|g| inner_end + g + 1).unwrap_or(html.len());
                    (&html[tag_end + 1..inner_end], resume)
                }
                None => ("", tag_end + 1),
            }
        };
        out.push_str(&html[at..start]);
        let attrs = attributes(tag);
        let get = |name: &str| attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
        let wanted = get("media").is_none_or(media_matches);
        let path = match get("src") {
            Some(src) if wanted && depth < MAX_DEPTH => Some(if src.contains("://") || src.starts_with('/') { src.to_string() } else { format!("{dir}{src}") }),
            _ => None,
        };
        match path.as_deref().and_then(|p| load(p).map(|text| (p, text))) {
            Some((p, text)) => {
                let nested = p.rfind('/').map_or("", |i| &p[..=i]);
                out.push_str(&expand_at(&text, nested, load, depth + 1));
            }
            None => out.push_str(&expand_at(inner, dir, load, depth + 1)),
        }
        at = resume;
    }
    out.push_str(&html[at.min(html.len())..]);
    out
}

/// Index of the `>` that closes the tag whose attributes start at `from`, skipping quoted values.
pub(crate) fn tag_end(html: &str, from: usize) -> Option<usize> {
    let mut quote = None;
    for (i, b) in html.bytes().enumerate().skip(from) {
        match (quote, b) {
            (Some(q), b) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(b),
            (None, b'>') => return Some(i),
            _ => {}
        }
    }
    None
}

pub(crate) fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let chars: Vec<char> = tag.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == '/') {
            i += 1;
        }
        let name_start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' && chars[i] != '/' {
            i += 1;
        }
        if i == name_start {
            break;
        }
        let name: String = chars[name_start..i].iter().collect::<String>().to_ascii_lowercase();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                let q = chars[i];
                i += 1;
                while i < chars.len() && chars[i] != q {
                    value.push(chars[i]);
                    i += 1;
                }
                i += 1;
            } else {
                while i < chars.len() && !chars[i].is_whitespace() && !(chars[i] == '/' && i + 1 == chars.len()) {
                    value.push(chars[i]);
                    i += 1;
                }
            }
        }
        found.push((name, value));
    }
    found
}

/// The current platform as Sciter names it: `Windows`, `OSX` or `Linux`.
pub fn platform() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "OSX"
    } else {
        "Linux"
    }
}

/// A small media expression: `platform == "Windows"`, `!=`, joined by `and` / `or` (`and` binds
/// tighter). Anything it doesn't understand counts as true, so a fragment isn't silently lost.
pub(crate) fn media_matches(expr: &str) -> bool {
    expr.split(" or ").any(|alt| alt.split(" and ").all(comparison))
}

fn comparison(term: &str) -> bool {
    let term = term.trim();
    let (negate, parts) = match term.split_once("!=") {
        Some(p) => (true, p),
        None => match term.split_once("==") {
            Some(p) => (false, p),
            None => return true,
        },
    };
    let value = parts.1.trim().trim_matches(|c| c == '"' || c == '\'');
    match parts.0.trim() {
        "platform" => (value.eq_ignore_ascii_case(platform())) != negate,
        _ => true,
    }
}

/// Sciter's tag shorthands (docs.sciter.com/docs/HTML, "syntax shortcuts"): `<menu.context>` and
/// `<div #title>` / `<input|text(user).wide>` become `class`, `id`, `type` and `name` attributes
/// (`|type`, `#id`, `.class`, `(name)`, glued to the tag name or standing alone as attributes).
/// Raw text (`<script>`, `<style>`) and comments are left alone.
pub(crate) fn expand_class_tags(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len() + 16);
    let mut at = 0; // how much of `html` is already in `out`
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if lower[i..].starts_with("<!--") {
            i = lower[i..].find("-->").map_or(bytes.len(), |e| i + e + 3);
            continue;
        }
        for raw in ["script", "style"] {
            if lower[i + 1..].starts_with(raw) && !bytes.get(i + 1 + raw.len()).is_some_and(|b| b.is_ascii_alphanumeric()) {
                // Skip to the closing tag: its content is not markup.
                let close = format!("</{raw}");
                let body_start = tag_end(html, i + 1).unwrap_or(bytes.len());
                i = lower[body_start..].find(&close).map_or(bytes.len(), |e| body_start + e);
                break;
            }
        }
        if i >= bytes.len() || bytes[i] != b'<' {
            continue;
        }
        let closing = bytes.get(i + 1) == Some(&b'/');
        let name_start = i + 1 + closing as usize;
        let mut j = name_start;
        while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-') {
            j += 1;
        }
        if j == name_start || !bytes[name_start].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'-' || c == b'_';
        if closing {
            // `</div.a.b>`: drop the class list.
            if bytes.get(j) == Some(&b'.') {
                let mut k = j;
                while k < bytes.len() && !bytes[k].is_ascii_whitespace() && bytes[k] != b'>' && bytes[k] != b'/' {
                    k += 1;
                }
                if html[j..k].split('.').filter(|c| !c.is_empty()).all(|c| c.bytes().all(ident)) {
                    out.push_str(&html[at..j]);
                    at = k;
                }
            }
            i = at.max(i + 1);
            continue;
        }
        let Some(end) = tag_end(html, j) else {
            i += 1;
            continue;
        };
        // Walk the attribute area: shorthands are collected, everything else is copied as it is.
        let (mut id, mut ty, mut name) = (None, None, None);
        let mut classes: Vec<&str> = Vec::new();
        let mut rest = String::new();
        let mut p = j;
        let mut found = false;
        while p < end {
            let c = bytes[p];
            let take = |from: usize| {
                let mut q = from;
                while q < end && ident(bytes[q]) {
                    q += 1;
                }
                q
            };
            // A shorthand starts a token (right after the tag name, or after whitespace/another shorthand).
            if matches!(c, b'#' | b'.' | b'|') && take(p + 1) > p + 1 && (c != b'|' || p == j) {
                let q = take(p + 1);
                let word = &html[p + 1..q];
                match c {
                    b'#' => id = Some(word),
                    b'|' => ty = Some(word),
                    _ => classes.push(word),
                }
                found = true;
                p = q;
            } else if c == b'(' && bytes[p..end].contains(&b')') {
                let q = p + bytes[p..end].iter().position(|b| *b == b')').unwrap_or(0);
                name = Some(&html[p + 1..q]);
                found = true;
                p = q + 1;
            } else if c.is_ascii_whitespace() {
                rest.push(c as char);
                p += 1;
            } else {
                // An ordinary attribute (or the closing `/`): copy it whole, quotes included.
                let mut quote = None;
                while p < end {
                    let ch = html[p..].chars().next().unwrap_or(' ');
                    match quote {
                        Some(q) if ch == q => quote = None,
                        None if ch == '"' || ch == '\'' => quote = Some(ch),
                        None if ch.is_ascii_whitespace() => break,
                        _ => {}
                    }
                    rest.push(ch);
                    p += ch.len_utf8();
                }
            }
        }
        if !found {
            i = j.max(i + 1);
            continue;
        }
        out.push_str(&html[at..j]);
        let mut add = String::new();
        for (key, v) in [("type", ty), ("id", id), ("name", name)] {
            if let Some(v) = v.filter(|v| !v.is_empty()) {
                add.push_str(&format!(" {key}=\"{v}\""));
            }
        }
        if !classes.is_empty() {
            let list = classes.join(" ");
            // Merge with an existing quoted `class="..."` on the same tag, else add one.
            match rest.find("class=\"") {
                Some(q) => rest.insert_str(q + 7, &format!("{list} ")),
                None => add.push_str(&format!(" class=\"{list}\"")),
            }
        }
        out.push_str(&add);
        let rest = rest.trim_end();
        if !rest.is_empty() && !rest.starts_with(|c: char| c.is_ascii_whitespace() || c == '/') {
            out.push(' ');
        }
        out.push_str(rest);
        at = end;
        i = at.max(i + 1);
    }
    out.push_str(&html[at..]);
    out
}

/// `<splitter>` and `<splitter/>` become `<splitter></splitter>`.
fn close_splitters(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    while let Some(i) = lower[at..].find("<splitter").map(|i| i + at) {
        let after = i + "<splitter".len();
        let delimited = html.as_bytes().get(after).is_some_and(|b| b.is_ascii_whitespace() || *b == b'/' || *b == b'>');
        let Some(end) = tag_end(html, after).filter(|_| delimited) else {
            out.push_str(&html[at..after]);
            at = after;
            continue;
        };
        out.push_str(&html[at..i]);
        out.push_str("<splitter");
        out.push_str(html[after..end].trim_end().trim_end_matches('/'));
        out.push_str("></splitter>");
        at = end + 1;
        let rest = &lower[at..];
        let trimmed = rest.trim_start();
        if trimmed.starts_with("</splitter>") {
            at += rest.len() - trimmed.len() + "</splitter>".len();
        }
    }
    out.push_str(&html[at..]);
    out
}

/// The HTML parser drops `<frame>` outside a `<frameset>`, but Sciter allows it wherever a block
/// goes (and `<frame src=...>` needs no closing tag). So `<frame ...>` becomes
/// `<nile-frame ...></nile-frame>`, which `Document::parse` turns back into a `frame` element.
pub(crate) fn rewrite_frames(html: &str) -> String {
    let html = &expand_class_tags(html);
    let lower = html.to_ascii_lowercase();
    if !lower.contains("<frame") && !lower.contains("<splitter") {
        return html.to_string();
    }
    // `<frameset>` is dropped by the parser in a body too; `<splitter/>` would swallow its siblings.
    let html = &html.replace("<frameset", "<nile-frameset").replace("</frameset", "</nile-frameset").replace("<FRAMESET", "<nile-frameset").replace("</FRAMESET", "</nile-frameset");
    let html = &close_splitters(html);
    let lower = html.to_ascii_lowercase();
    if !lower.contains("<frame") {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len() + 32);
    let mut at = 0;
    while let Some(i) = lower[at..].find("<frame").map(|i| i + at) {
        let after = i + "<frame".len();
        let delimited = html.as_bytes().get(after).is_some_and(|b| b.is_ascii_whitespace() || *b == b'/' || *b == b'>');
        let Some(end) = tag_end(html, after).filter(|_| delimited) else {
            out.push_str(&html[at..after]);
            at = after;
            continue;
        };
        out.push_str(&html[at..i]);
        out.push_str("<nile-frame");
        out.push_str(html[after..end].trim_end().trim_end_matches('/'));
        out.push_str("></nile-frame>");
        at = end + 1;
        // An explicit `</frame>` right after is the same element, not another one.
        let rest = &lower[at..];
        let trimmed = rest.trim_start();
        if trimmed.starts_with("</frame>") {
            at += rest.len() - trimmed.len() + "</frame>".len();
        }
    }
    out.push_str(&html[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(html: &str) -> String {
        expand(html, &mut |src| match src {
            "a.htm" => Some("<p>A</p>".into()),
            "nested.htm" => Some("[<include src=a.htm></include>]".into()),
            "sub/n.htm" => Some("<include src=b.htm />".into()),
            "sub/b.htm" => Some("B".into()),
            "loop.htm" => Some("<include src=loop.htm />".into()),
            _ => None,
        })
    }

    #[test]
    fn includes_are_spliced_in_place() {
        assert_eq!(run("<body><include src=\"a.htm\"></include>!</body>"), "<body><p>A</p>!</body>");
        assert_eq!(run("x<INCLUDE SRC='a.htm'/>y"), "x<p>A</p>y");
        assert_eq!(run("<include src=nested.htm></include>"), "[<p>A</p>]");
        assert_eq!(run("<include src=sub/n.htm></include>"), "B");
    }

    #[test]
    fn missing_fragments_and_false_media_use_the_fallback() {
        assert_eq!(run("<include src=\"nope.htm\">fallback</include>"), "fallback");
        assert_eq!(run("<include src=\"a.htm\" media=\"platform == 'Nope'\">fb</include>"), "fb");
        let here = format!("<include src=\"a.htm\" media=\"platform == '{}'\">fb</include>", platform());
        assert_eq!(run(&here), "<p>A</p>");
        let not = format!("<include src=\"a.htm\" media=\"platform != '{}'\">fb</include>", platform());
        assert_eq!(run(&not), "fb");
    }

    #[test]
    fn frames_become_closed_custom_elements() {
        assert_eq!(rewrite_frames("<div><frame src=a.htm><p>x</p></div>"), "<div><nile-frame src=a.htm></nile-frame><p>x</p></div>");
        assert_eq!(rewrite_frames("<FRAME src='a'/> </frame><frameset>"), "<nile-frame src='a'></nile-frame><nile-frameset>");
        assert_eq!(rewrite_frames("<frame src=a></frame>z"), "<nile-frame src=a></nile-frame>z");
    }

    #[test]
    fn framesets_and_splitters_survive_the_parser() {
        assert_eq!(rewrite_frames("<frameset cols='1,*'><div></div><splitter/><frame src=a></frameset>"), "<nile-frameset cols='1,*'><div></div><splitter></splitter><nile-frame src=a></nile-frame></nile-frameset>");
        assert_eq!(rewrite_frames("<splitter></splitter>"), "<splitter></splitter>");
    }

    #[test]
    fn class_shorthand_on_tags() {
        assert_eq!(expand_class_tags("<menu.context id=x><li>a</li></menu>"), "<menu class=\"context\" id=x><li>a</li></menu>");
        assert_eq!(expand_class_tags("<div.a.b>x</div.a.b>"), "<div class=\"a b\">x</div>");
        assert_eq!(expand_class_tags("<p.x class=\"y\">"), "<p class=\"x y\">");
        // Not markup: comparisons in scripts, comments, plain tags.
        assert_eq!(expand_class_tags("<script>if (a<b.c) {}</script><p>1 < 2.5</p>"), "<script>if (a<b.c) {}</script><p>1 < 2.5</p>");
        assert_eq!(expand_class_tags("<!-- <a.b> --><a href=x.y>"), "<!-- <a.b> --><a href=x.y>");
        assert_eq!(expand_class_tags("<div #title>x</div>"), "<div id=\"title\">x</div>");
        assert_eq!(expand_class_tags("<input|text(user) value=\"a.b #c\" />"), "<input type=\"text\" name=\"user\" value=\"a.b #c\" />");
        assert_eq!(expand_class_tags("<button|radio(g).first class=\"x\">"), "<button type=\"radio\" name=\"g\" class=\"first x\">");
        assert_eq!(expand_class_tags("<a href=a.b#c>"), "<a href=a.b#c>");
    }

    #[test]
    fn cycles_stop_and_comments_and_lookalikes_are_left_alone() {
        assert_eq!(run("<include src=loop.htm></include>"), "");
        assert_eq!(run("<!-- <include src=a.htm> --><p/>"), "<!-- <include src=a.htm> --><p/>");
        assert_eq!(run("<includes src=a.htm>"), "<includes src=a.htm>");
    }
}
