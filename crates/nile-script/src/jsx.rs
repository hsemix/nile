//! JSX to plain JavaScript.
//!
//! `<div class="a" onclick={f}>Hi {name}</div>` becomes
//! `JSX("div", {"class": "a", "onclick": f}, ["Hi ", name])` (Sciter's shape:
//! tag, attributes or `null`, children array or `null`); `<>..</>` becomes
//! `JSX(JSX.Fragment, null, [..])`. The rest of the source is copied through
//! untouched, newlines included (JSX that spans lines is followed by the same
//! number of newlines, so error line numbers stay right).
//!
//! This is a scanner, not a JS parser: it tracks strings, template literals,
//! comments and regex literals so it doesn't misread `<` inside them, and it
//! treats `<` as JSX only where an expression can start. That covers real JSX;
//! the odd ambiguous construct is left alone for the JS engine to report.

/// Transform `src`, or `None` if it contains no JSX (or the JSX is malformed,
/// in which case the engine reports the syntax error on the original source).
pub fn transform(src: &str) -> Option<String> {
    if !src.contains('<') {
        return None;
    }
    let mut t = Scanner { src, b: src.as_bytes(), i: 0, prev: Prev::Start, changed: false };
    let out = t.code(false).ok()?;
    t.changed.then_some(out)
}

#[derive(Clone, Copy, PartialEq)]
enum Prev {
    /// Start of input or after a token that begins an expression: `( , = ; ...`
    Start,
    /// After a value (identifier, number, string, `)`, `]`): `<` is less-than.
    Value,
}

struct Scanner<'a> {
    src: &'a str,
    b: &'a [u8],
    i: usize,
    prev: Prev,
    changed: bool,
}

type R<T> = Result<T, ()>;

const EXPR_KEYWORDS: &[&str] = &[
    "return", "typeof", "case", "default", "in", "of", "yield", "await", "void", "delete", "throw", "else",
    "do", "new", "instanceof",
];

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}

fn is_ident(c: u8) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

impl<'a> Scanner<'a> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn starts(&self, s: &str) -> bool {
        self.b[self.i..].starts_with(s.as_bytes())
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    /// Copy JS code to the output, transforming JSX. If `until_brace`, stops at
    /// the unmatched `}` that ends a `${}` or JSX expression (not consumed).
    fn code(&mut self, until_brace: bool) -> R<String> {
        let mut out = String::new();
        let mut depth = 0usize;
        while let Some(c) = self.peek() {
            let start = self.i;
            match c {
                c if c.is_ascii_whitespace() => self.i += 1,
                b'/' if self.starts("//") => {
                    while self.peek().is_some_and(|c| c != b'\n') {
                        self.i += 1;
                    }
                }
                b'/' if self.starts("/*") => {
                    self.i += 2;
                    while self.i < self.b.len() && !self.starts("*/") {
                        self.i += 1;
                    }
                    self.i = (self.i + 2).min(self.b.len());
                }
                b'\'' | b'"' => {
                    self.i += 1;
                    while let Some(ch) = self.peek() {
                        self.i += 1;
                        if ch == b'\\' {
                            self.i += 1;
                        } else if ch == c {
                            break;
                        }
                    }
                    self.i = self.i.min(self.b.len());
                    self.prev = Prev::Value;
                }
                b'`' => {
                    out.push_str(&self.src[start..start]);
                    let t = self.template()?;
                    out.push_str(&t);
                    self.prev = Prev::Value;
                    continue;
                }
                b'{' => {
                    depth += 1;
                    self.i += 1;
                    self.prev = Prev::Start;
                }
                b'}' => {
                    if depth == 0 && until_brace {
                        return Ok(out);
                    }
                    depth = depth.saturating_sub(1);
                    self.i += 1;
                    // After a block or object literal an expression may follow.
                    self.prev = Prev::Start;
                }
                b'<' if self.prev == Prev::Start && self.b.get(self.i + 1).is_some_and(|n| is_ident_start(*n) || *n == b'>') => {
                    let before = self.i;
                    let expr = self.element()?;
                    let newlines = self.src[before..self.i].matches('\n').count();
                    out.push_str(&expr);
                    out.extend(std::iter::repeat_n('\n', newlines));
                    self.changed = true;
                    self.prev = Prev::Value;
                    continue;
                }
                b'/' if self.prev == Prev::Start => {
                    // Regex literal.
                    self.i += 1;
                    let mut in_class = false;
                    while let Some(ch) = self.peek() {
                        self.i += 1;
                        match ch {
                            b'\\' => self.i += 1,
                            b'[' => in_class = true,
                            b']' => in_class = false,
                            b'/' if !in_class => break,
                            b'\n' => break,
                            _ => {}
                        }
                    }
                    self.prev = Prev::Value;
                }
                c if is_ident_start(c) => {
                    while self.peek().is_some_and(is_ident) {
                        self.i += 1;
                    }
                    let word = &self.src[start..self.i];
                    self.prev = if EXPR_KEYWORDS.contains(&word) { Prev::Start } else { Prev::Value };
                }
                c if c.is_ascii_digit() => {
                    while self.peek().is_some_and(|c| is_ident(c) || c == b'.') {
                        self.i += 1;
                    }
                    self.prev = Prev::Value;
                }
                b')' | b']' => {
                    self.i += 1;
                    self.prev = Prev::Value;
                }
                _ => {
                    // Operators and punctuation. `++`/`--` keep a value a value.
                    let postfix = (self.starts("++") || self.starts("--")) && self.prev == Prev::Value;
                    self.i += if self.starts("=>") || postfix { 2 } else { 1 };
                    self.prev = if postfix { Prev::Value } else { Prev::Start };
                }
            }
            // Copy everything consumed above that wasn't handled specially.
            out.push_str(&self.src[start..self.i]);
        }
        if until_brace {
            Err(()) // unterminated `${` or `{`
        } else {
            Ok(out)
        }
    }

    /// A template literal, with `${}` holes scanned as code.
    fn template(&mut self) -> R<String> {
        let mut out = String::from("`");
        self.i += 1;
        while let Some(c) = self.peek() {
            if c == b'`' {
                self.i += 1;
                out.push('`');
                return Ok(out);
            }
            if c == b'\\' {
                out.push_str(&self.src[self.i..(self.i + 2).min(self.b.len())]);
                self.i += 2;
            } else if self.starts("${") {
                self.i += 2;
                self.prev = Prev::Start;
                let inner = self.code(true)?;
                self.i += 1; // the closing brace
                out.push_str("${");
                out.push_str(&inner);
                out.push('}');
            } else {
                let ch = self.src[self.i..].chars().next().ok_or(())?;
                out.push(ch);
                self.i += ch.len_utf8();
            }
        }
        Err(())
    }

    /// One JSX element or fragment starting at `<`.
    fn element(&mut self) -> R<String> {
        self.i += 1; // <
        if self.peek() == Some(b'>') {
            self.i += 1;
            let kids = self.children("")?;
            return Ok(call("JSX.Fragment", "null", &kids));
        }
        let name = self.name()?;
        let tag = if name.contains('.') || name.as_bytes()[0].is_ascii_uppercase() || name.starts_with(['_', '$']) {
            name.clone()
        } else {
            js_string(&name)
        };

        // Attributes, as segments so a spread can interleave.
        let mut segments: Vec<String> = Vec::new();
        let mut props: Vec<String> = Vec::new();
        let flush = |props: &mut Vec<String>, segments: &mut Vec<String>| {
            if !props.is_empty() {
                segments.push(format!("{{{}}}", props.join(", ")));
                props.clear();
            }
        };
        let self_closing;
        loop {
            self.skip_ws();
            if self.starts("/>") {
                self.i += 2;
                self_closing = true;
                break;
            }
            if self.peek() == Some(b'>') {
                self.i += 1;
                self_closing = false;
                break;
            }
            if self.peek() == Some(b'{') {
                // {...spread}
                self.i += 1;
                self.skip_ws();
                if !self.starts("...") {
                    return Err(());
                }
                self.i += 3;
                let expr = self.expression()?;
                flush(&mut props, &mut segments);
                segments.push(expr);
                continue;
            }
            let attr = self.name()?;
            self.skip_ws();
            let value = if self.peek() == Some(b'=') {
                self.i += 1;
                self.skip_ws();
                match self.peek() {
                    Some(q @ (b'"' | b'\'')) => {
                        self.i += 1;
                        let s = self.i;
                        while self.peek().is_some_and(|c| c != q) {
                            self.i += 1;
                        }
                        let raw = &self.src[s..self.i];
                        self.i += 1;
                        js_string(&decode_entities(raw))
                    }
                    Some(b'{') => {
                        self.i += 1;
                        self.expression()?
                    }
                    Some(b'<') => self.element()?,
                    _ => return Err(()),
                }
            } else {
                "true".into()
            };
            props.push(format!("{}: {}", js_string(&attr), value));
        }
        let props_js = if segments.is_empty() {
            if props.is_empty() { "null".to_string() } else { format!("{{{}}}", props.join(", ")) }
        } else {
            flush(&mut props, &mut segments);
            format!("Object.assign({{}}, {})", segments.join(", "))
        };
        let kids = if self_closing { Vec::new() } else { self.children(&name)? };
        Ok(call(&tag, &props_js, &kids))
    }

    /// An expression after `{` up to its matching `}` (consumed).
    fn expression(&mut self) -> R<String> {
        self.prev = Prev::Start;
        let e = self.code(true)?;
        self.i += 1;
        Ok(e.trim().to_string())
    }

    fn name(&mut self) -> R<String> {
        let s = self.i;
        while self.peek().is_some_and(|c| is_ident(c) || c == b'.' || c == b':' || c == b'-') {
            self.i += 1;
        }
        if s == self.i {
            return Err(());
        }
        Ok(self.src[s..self.i].to_string())
    }

    /// Children up to and including the closing tag `</name>` (or `</>`).
    fn children(&mut self, name: &str) -> R<Vec<String>> {
        let mut kids = Vec::new();
        loop {
            match self.peek().ok_or(())? {
                b'<' if self.starts("</") => {
                    self.i += 2;
                    self.skip_ws();
                    let close = if self.peek() == Some(b'>') { String::new() } else { self.name()? };
                    self.skip_ws();
                    if close != name || self.peek() != Some(b'>') {
                        return Err(());
                    }
                    self.i += 1;
                    return Ok(kids);
                }
                b'<' => kids.push(self.element()?),
                b'{' => {
                    self.i += 1;
                    let e = self.expression()?;
                    // `{/* comment */}` leaves nothing.
                    if !only_comments(&e) {
                        kids.push(e);
                    }
                }
                _ => {
                    let s = self.i;
                    while self.peek().is_some_and(|c| c != b'<' && c != b'{') {
                        self.i += 1;
                    }
                    if let Some(text) = jsx_text(&self.src[s..self.i]) {
                        kids.push(js_string(&decode_entities(&text)));
                    }
                }
            }
        }
    }
}

/// True for an expression holding nothing but whitespace and comments.
fn only_comments(e: &str) -> bool {
    let mut rest = e.trim();
    loop {
        if rest.is_empty() {
            return true;
        }
        if let Some(r) = rest.strip_prefix("/*") {
            match r.find("*/") {
                Some(i) => rest = r[i + 2..].trim_start(),
                None => return false,
            }
        } else if let Some(r) = rest.strip_prefix("//") {
            rest = r.find('\n').map_or("", |i| r[i..].trim_start());
        } else {
            return false;
        }
    }
}

fn call(tag: &str, props: &str, kids: &[String]) -> String {
    let kids = if kids.is_empty() { "null".to_string() } else { format!("[{}]", kids.join(", ")) };
    format!("JSX({tag}, {props}, {kids})")
}

/// JSX text whitespace rules: lines are trimmed, blank lines dropped, and the
/// rest joined with single spaces. Text on one line keeps its inner spacing.
fn jsx_text(raw: &str) -> Option<String> {
    if !raw.contains('\n') {
        return (!raw.is_empty()).then(|| raw.to_string());
    }
    let lines: Vec<&str> = raw.split('\n').collect();
    let last = lines.len() - 1;
    let parts: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let mut l = *l;
            if i != 0 {
                l = l.trim_start();
            }
            if i != last {
                l = l.trim_end();
            }
            (!l.is_empty()).then_some(l)
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|e| *e <= 8);
        let decoded = end.and_then(|e| match &rest[1..e] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            n if n.starts_with("#x") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        });
        match (decoded, end) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A JS string literal for `s`.
fn js_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(src: &str) -> String {
        transform(src).expect("expected JSX to be found")
    }

    #[test]
    fn elements_attributes_and_children() {
        assert_eq!(t(r#"const a = <p class="x" hidden>hi</p>;"#), r#"const a = JSX("p", {"class": "x", "hidden": true}, ["hi"]);"#);
        assert_eq!(t("x = <br/>"), r#"x = JSX("br", null, null)"#);
        assert_eq!(
            t("render(<ul><li id={id}>{n + 1}</li><Item a={1}/></ul>)"),
            r#"render(JSX("ul", null, [JSX("li", {"id": id}, [n + 1]), JSX(Item, {"a": 1}, null)]))"#
        );
    }

    #[test]
    fn fragments_spread_and_member_tags() {
        assert_eq!(t("f(<><a/>text</>)"), r#"f(JSX(JSX.Fragment, null, [JSX("a", null, null), "text"]))"#);
        assert_eq!(t("x = <div {...p} b={2}/>"), r#"x = JSX("div", Object.assign({}, p, {"b": 2}), null)"#);
        assert_eq!(t("x = <ui.Panel/>"), r#"x = JSX(ui.Panel, null, null)"#);
    }

    #[test]
    fn text_whitespace_follows_jsx_rules() {
        let out = t("x = (\n  <p>\n    Hello,\n    world &amp; co\n  </p>\n)");
        assert!(out.contains(r#"JSX("p", null, ["Hello, world & co"])"#), "{out}");
        // Lines are preserved.
        assert_eq!(out.matches('\n').count(), 5);
        assert_eq!(t("x = <b> a  b </b>"), "x = JSX(\"b\", null, [\" a  b \"])");
    }

    #[test]
    fn nested_expressions_templates_and_arrows() {
        assert_eq!(
            t("items.map((i) => <li key={i}>{`#${i}`}</li>)"),
            r#"items.map((i) => JSX("li", {"key": i}, [`#${i}`]))"#
        );
        assert_eq!(
            t("x = <a>{cond ? <b/> : <i/>}</a>"),
            r#"x = JSX("a", null, [cond ? JSX("b", null, null) : JSX("i", null, null)])"#
        );
        assert_eq!(t("x = <a>{/* note */}ok</a>"), r#"x = JSX("a", null, ["ok"])"#);
        assert_eq!(t("x = `${<b/>}`"), r#"x = `${JSX("b", null, null)}`"#);
    }

    #[test]
    fn leaves_non_jsx_alone() {
        for src in [
            "if (a < b && c > d) {}",
            "const s = '<div>' + \"<p/>\"; // <b>",
            "let re = /<a>/g; x = y < z;",
            "i++ < n; a[0] <b",
            "/* <div/> */ const t = `<span>`;",
            "for (let i = 0; i < n; i++) {}",
        ] {
            assert_eq!(transform(src), None, "{src}");
        }
    }

    #[test]
    fn malformed_jsx_is_left_for_the_engine_to_report() {
        assert_eq!(transform("x = <div>unclosed"), None);
        assert_eq!(transform("x = <a></b>"), None);
    }
}
