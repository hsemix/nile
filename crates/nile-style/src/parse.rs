//! CSS parsing on top of `cssparser`'s tokenizer.

use crate::selector::{NileImpl, NileParser};
use crate::anim::Timing;
use crate::gradient::{TransformFn, ColorStop, Extent, Gradient, GradientKind, LinearDir, StopPos};
use crate::{colors, Color};
use std::cell::Cell;
use cssparser::{Delimiter, ParseError, Parser, Token};
use selectors::parser::{ParseRelative, SelectorList};

type PResult<T> = Result<T, ParseError<()>>;

/// What the parser needs to know about the environment: the viewport (for `vw`, `vh`
/// and `@media`) and a flag it sets when the result depends on it.
#[derive(Clone, Copy)]
pub struct Cx<'a> {
    pub viewport: (f32, f32),
    pub uses_viewport: &'a Cell<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f32),
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    /// `calc()` and friends reduced to `px + em * font-size + pct * 100%`. Only one
    /// of the percentage and the length parts can be non-zero when it is used.
    Calc { px: f32, em: f32, pct: f32 },
    Ident(String),
    Str(String),
    Color(Color),
    /// A top-level `,` (lists such as `box-shadow`).
    Comma,
    /// `attr(name)`, in `content`.
    Attr(String),
    /// An angle in degrees.
    Angle(f32),
    Gradient(Box<Gradient>),
    /// A `fr` track size.
    Fr(f32),
    /// `url(...)`.
    Url(String),
    /// A duration in seconds.
    Time(f32),
    /// `cubic-bezier()` or `steps()`.
    Timing(Timing),
    Transform(TransformFn),
    /// A top-level `/` (grid lines).
    Slash,
    /// `minmax(min, max)`, and `fit-content(x)` as `minmax(auto, x)`.
    Minmax(Box<(Value, Value)>),
    /// `repeat(count, tracks...)`.
    Repeat(Box<(RepeatKind, Vec<Value>)>),
    /// `light-dark(light, dark)`: picked by the element's colour scheme when the style is applied.
    LightDark(Color, Color),
    /// `counter(name, style)` (separator `None`) or `counters(name, separator, style)`.
    Counter(Box<(String, Option<String>, String)>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RepeatKind {
    Count(u16),
    AutoFill,
    AutoFit,
}

impl Value {
    pub fn ident(&self) -> Option<&str> {
        match self {
            Value::Ident(s) => Some(s),
            _ => None,
        }
    }

    pub fn color(&self) -> Option<Color> {
        match self {
            Value::Color(c) => Some(*c),
            Value::LightDark(light, dark) => Some(if crate::system::current_dark() { *dark } else { *light }),
            Value::Ident(name) => colors::named(name),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub values: Vec<Value>,
    pub important: bool,
    /// The value's source text, kept only for custom properties and values using `var()`;
    /// those are resolved per element during the cascade.
    pub raw: String,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub selectors: SelectorList<NileImpl>,
    pub declarations: Vec<Declaration>,
    /// `@layer` the rule is in: 0 for none, else 1 + the layer's place in `Stylesheet::layers`.
    /// Unlayered rules beat layered ones whatever their specificity; later layers beat earlier ones.
    pub layer: u32,
}

/// Properties whose value is kept as text for the cascade to hand on (scripts read `popup-*`; the
/// others are parsed by their own modules or the text engine).
pub(crate) fn keeps_raw(name: &str) -> bool {
    name.starts_with("popup-") || matches!(name, "filter" | "backdrop-filter" | "-webkit-backdrop-filter" | "clip-path" | "shape-outside" | "font-variation-settings" | "font-feature-settings" | "prototype" | "nile-password-char")
}

/// An `@keyframes` rule: the declarations at each offset (0..=1), in offset order.
#[derive(Debug, Clone)]
pub struct Keyframes {
    pub name: String,
    pub frames: Vec<Frame>,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub offset: f32,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
    pub keyframes: Vec<Keyframes>,
    /// `@page` rules: the selector (`:first`, `:left`, ..., or empty) and the body text.
    pub pages: Vec<(String, String)>,
    /// `@font-face` rules.
    pub font_faces: Vec<FontFace>,
    /// Some rule or value depended on the viewport size (`@media`, `vw`, ...).
    pub uses_viewport: bool,
    /// `@layer` names (nested ones as `outer.inner`) in the order they were first seen.
    pub layers: Vec<String>,
    /// The `@layer` blocks being parsed, innermost last.
    open_layers: Vec<String>,
}

impl Stylesheet {
    /// The layer a rule parsed now belongs to.
    fn current_layer(&self) -> u32 {
        match self.open_layers.last() {
            Some(name) => self.layers.iter().position(|l| l == name).map_or(0, |i| i as u32 + 1),
            None => 0,
        }
    }

    fn declare_layer(&mut self, name: &str) -> String {
        let full = match self.open_layers.last() {
            Some(outer) => format!("{outer}.{name}"),
            None => name.to_string(),
        };
        if !self.layers.contains(&full) {
            self.layers.push(full.clone());
        }
        full
    }
}

/// One `@font-face`: a family name and where its font can be loaded from.
#[derive(Debug, Clone, PartialEq)]
pub struct FontFace {
    pub family: String,
    /// `url(...)` sources in order (`local(...)` ones are skipped).
    pub urls: Vec<String>,
    /// `font-weight`: `None` for a range (a variable font) or when unspecified.
    pub weight: Option<f32>,
    /// `font-style`: `None` when unspecified.
    pub italic: Option<bool>,
}

const DEFAULT_VIEWPORT: (f32, f32) = (800.0, 600.0);

impl Stylesheet {
    pub fn parse(css: &str) -> Stylesheet {
        Stylesheet::parse_with(css, DEFAULT_VIEWPORT)
    }

    pub fn parse_with(css: &str, viewport: (f32, f32)) -> Stylesheet {
        let used = Cell::new(false);
        let cx = Cx { viewport, uses_viewport: &used };
        let mut parser = Parser::new(css);
        let mut sheet = Stylesheet::default();
        rules_in(&mut parser, &cx, &mut sheet);
        sheet.uses_viewport = used.get();
        sheet
    }
}

/// Parses rules and at-rules until the input ends.
fn rules_in(p: &mut Parser<'_>, cx: &Cx, sheet: &mut Stylesheet) {
    while !p.is_exhausted() {
        let state = p.state();
        let at = match p.next() {
            Ok(Token::AtKeyword(name)) => Some(name.to_ascii_lowercase()),
            _ => None,
        };
        if let Some(name) = at {
            at_rule(p, cx, &name, sheet);
            continue;
        }
        p.reset(&state);
        match parse_rule(p, cx, sheet) {
            Ok(()) => {}
            // Skip the broken rule and keep going, as browsers do.
            Err(_) => {
                let _ = p.parse_until_after(Delimiter::CurlyBracketBlock, |p| {
                    while p.next().is_ok() {}
                    Ok::<_, ParseError<()>>(())
                });
            }
        }
    }
}

/// `@media` rules are evaluated now and their rules kept or dropped; every other
/// at-rule (`@import`, `@font-face`, `@keyframes`, ...) is skipped.
fn at_rule(p: &mut Parser<'_>, cx: &Cx, name: &str, sheet: &mut Stylesheet) {
    let start = p.position();
    let _ = p.parse_until_before(Delimiter::CurlyBracketBlock | Delimiter::Semicolon, |p| {
        while p.next().is_ok() {}
        Ok::<_, ParseError<()>>(())
    });
    let raw_prelude = p.slice_from(start).trim().to_string();
    let prelude = raw_prelude.to_ascii_lowercase();
    let has_block = matches!(p.next(), Ok(Token::CurlyBracketBlock));
    // `@layer a, b;` only fixes the order of the layers.
    if name == "layer" && !has_block {
        for n in prelude.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            sheet.declare_layer(n);
        }
        return;
    }
    if has_block {
        if name == "keyframes" || name == "-webkit-keyframes" {
            let frames = p.parse_nested_block(|p| Ok::<_, ParseError<()>>(keyframes_in(p, cx))).unwrap_or_default();
            let name = raw_prelude.trim_matches(|c| c == '"' || c == '\'').to_string();
            if !name.is_empty() && !frames.is_empty() {
                sheet.keyframes.push(Keyframes { name, frames });
            }
        } else if name == "page" {
            let body = p
                .parse_nested_block(|p| {
                    let start = p.position();
                    while p.next().is_ok() {}
                    Ok::<_, ParseError<()>>(p.slice_from(start).to_string())
                })
                .unwrap_or_default();
            sheet.pages.push((raw_prelude.clone(), body));
        } else if name == "font-face" {
            let face = p.parse_nested_block(|p| Ok::<_, ParseError<()>>(font_face_in(p))).unwrap_or_default();
            if let Some(face) = face {
                sheet.font_faces.push(face);
            }
        } else if name == "layer" {
            // `@layer name { ... }`: the rules inside belong to the layer (an unnamed block gets its own).
            let first = prelude.split(',').next().unwrap_or("").trim().to_string();
            let name = if first.is_empty() { format!("<anonymous {}>", sheet.layers.len()) } else { first };
            let full = sheet.declare_layer(&name);
            sheet.open_layers.push(full);
            let _ = p.parse_nested_block(|p| {
                rules_in(p, cx, sheet);
                Ok::<_, ParseError<()>>(())
            });
            sheet.open_layers.pop();
        } else if name == "supports" {
            if supports_matches(&prelude, cx.viewport) {
                let _ = p.parse_nested_block(|p| {
                    rules_in(p, cx, sheet);
                    Ok::<_, ParseError<()>>(())
                });
            }
        } else if name == "media" {
            cx.uses_viewport.set(true);
            if media_matches(&prelude, cx.viewport) {
                let _ = p.parse_nested_block(|p| {
                    rules_in(p, cx, sheet);
                    Ok::<_, ParseError<()>>(())
                });
            }
        }
    }
}

/// The descriptors of an `@font-face` block.
fn font_face_in(p: &mut Parser<'_>) -> Option<FontFace> {
    let (mut family, mut src, mut weight, mut style) = (None, String::new(), None, None);
    while !p.is_exhausted() {
        let entry = p.parse_until_after(Delimiter::Semicolon, |p| {
            let name = p.expect_ident()?.to_ascii_lowercase();
            p.expect_colon()?;
            let start = p.position();
            while p.next().is_ok() {}
            Ok::<_, ParseError<()>>((name, p.slice_from(start).trim().to_string()))
        });
        let Ok((name, value)) = entry else { continue };
        match name.as_str() {
            "font-family" => family = Some(value.trim_matches(|c| c == '"' || c == '\'').to_string()),
            "src" => src = value,
            "font-weight" => weight = Some(value),
            "font-style" => style = Some(value),
            _ => {}
        }
    }
    let family = family.filter(|f| !f.is_empty())?;
    let urls = font_urls(&src);
    if urls.is_empty() {
        return None;
    }
    let weight = weight.and_then(|w| match w.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(400.0),
        "bold" => Some(700.0),
        w => w.parse::<f32>().ok(),
    });
    let italic = style.map(|s| s.trim().to_ascii_lowercase()).map(|s| s.starts_with("italic") || s.starts_with("oblique"));
    Some(FontFace { family, urls, weight, italic })
}

/// The `url(...)` entries of a `src` descriptor, in order.
fn font_urls(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = src.to_ascii_lowercase();
    let mut at = 0;
    while let Some(i) = lower[at..].find("url(") {
        let start = at + i + 4;
        let rest = &src[start..];
        let rest_trim = rest.trim_start();
        let lead = rest.len() - rest_trim.len();
        let (url, used) = match rest_trim.chars().next() {
            Some(q @ ('"' | '\'')) => match rest_trim[1..].find(q) {
                Some(e) => (rest_trim[1..1 + e].to_string(), lead + e + 2),
                None => break,
            },
            _ => match rest_trim.find(')') {
                Some(e) => (rest_trim[..e].trim().to_string(), lead + e),
                None => break,
            },
        };
        if !url.is_empty() {
            out.push(url);
        }
        at = start + used;
    }
    out
}

/// `@supports` conditions: `(prop: value)`, `not`, `and`, `or`, parentheses, `selector(...)` (true).
fn supports_matches(cond: &str, viewport: (f32, f32)) -> bool {
    fn split_top<'a>(s: &'a str, word: &str) -> Option<(&'a str, &'a str)> {
        let mut depth = 0;
        let pat = format!(" {word} ");
        for (i, c) in s.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ if depth == 0 && s[i..].starts_with(&pat) => return Some((&s[..i], &s[i + pat.len()..])),
                _ => {}
            }
        }
        None
    }
    let s = cond.trim();
    if let Some(rest) = s.strip_prefix("not ") {
        return !supports_matches(rest, viewport);
    }
    if let Some((a, b)) = split_top(s, "or") {
        return supports_matches(a, viewport) || supports_matches(b, viewport);
    }
    if let Some((a, b)) = split_top(s, "and") {
        return supports_matches(a, viewport) && supports_matches(b, viewport);
    }
    if s.starts_with("selector(") {
        return true;
    }
    let Some(inner) = s.strip_prefix('(').and_then(|r| r.strip_suffix(')')) else { return false };
    let inner = inner.trim();
    if inner.starts_with('(') || inner.starts_with("not ") {
        return supports_matches(inner, viewport);
    }
    match inner.split_once(':') {
        Some((name, value)) => crate::supports_declaration(name.trim(), value.trim(), viewport),
        None => false,
    }
}

/// The frames inside `@keyframes`: `from`, `to` and percentages, comma separated.
fn keyframes_in(p: &mut Parser<'_>, cx: &Cx) -> Vec<Frame> {
    let mut frames = Vec::new();
    while !p.is_exhausted() {
        let offsets: PResult<Vec<f32>> = p.parse_until_before(Delimiter::CurlyBracketBlock, |p| {
            let mut offsets = Vec::new();
            while !p.is_exhausted() {
                match p.next()? {
                    Token::Ident(i) if i.eq_ignore_ascii_case("from") => offsets.push(0.0),
                    Token::Ident(i) if i.eq_ignore_ascii_case("to") => offsets.push(1.0),
                    Token::Percentage { unit_value, .. } => offsets.push(unit_value.clamp(0.0, 1.0)),
                    Token::Comma => {}
                    _ => return Err(ParseError::custom(())),
                }
            }
            Ok(offsets)
        });
        match (p.next(), offsets) {
            (Ok(Token::CurlyBracketBlock), Ok(offsets)) => {
                let declarations = p.parse_nested_block(|p| Ok::<_, ParseError<()>>(declarations_in(p, cx))).unwrap_or_default();
                for offset in offsets {
                    frames.push(Frame { offset, declarations: declarations.clone() });
                }
            }
            (Ok(_), _) => {}
            (Err(_), _) => break,
        }
    }
    frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    frames
}

/// Evaluates a media query list against the viewport: width/height features
/// (`min-`/`max-`, plain, and range syntax), `orientation`, `screen`/`all`, `and`,
/// `not`, and commas as "or". The window is a light-theme screen with a mouse.
fn media_matches(query: &str, (vw, vh): (f32, f32)) -> bool {
    let list = query.trim();
    if list.is_empty() {
        return true;
    }
    list.split(',').any(|q| {
        let mut q = q.trim();
        let negate = q.strip_prefix("not ").map(|r| q = r.trim()).is_some();
        if let Some(r) = q.strip_prefix("only ") {
            q = r.trim();
        }
        let ok = q.split(" and ").all(|part| {
            let part = part.trim();
            match part.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
                Some(feature) => media_feature(feature.trim(), vw, vh),
                None => matches!(part, "all" | "screen" | ""),
            }
        });
        ok != negate
    })
}

fn media_feature(f: &str, vw: f32, vh: f32) -> bool {
    let length = |v: &str| -> Option<f32> {
        let v = v.trim();
        if let Some(n) = v.strip_suffix("px") {
            n.trim().parse().ok()
        } else if let Some(n) = v.strip_suffix("rem").or_else(|| v.strip_suffix("em")) {
            n.trim().parse::<f32>().ok().map(|n| n * 16.0)
        } else {
            v.parse().ok()
        }
    };
    // Range syntax: `width >= 600px`, `400px <= width`.
    for op in ["<=", ">=", "<", ">"] {
        if let Some((l, r)) = f.split_once(op) {
            let (l, r) = (l.trim(), r.trim());
            let (dim, val, flipped) = match (l, r) {
                ("width" | "height", v) => (l, v, false),
                (v, "width" | "height") => (r, v, true),
                _ => return false,
            };
            let cur = if dim == "width" { vw } else { vh };
            let Some(v) = length(val) else { return false };
            let (a, b) = if flipped { (v, cur) } else { (cur, v) };
            return match op {
                "<=" => a <= b,
                ">=" => a >= b,
                "<" => a < b,
                _ => a > b,
            };
        }
    }
    let (name, value) = match f.split_once(':') {
        Some((n, v)) => (n.trim(), Some(v.trim())),
        None => (f, None),
    };
    let Some(value) = value else { return matches!(name, "color" | "hover" | "pointer") };
    match name {
        "orientation" => (value == "landscape") == (vw >= vh) && matches!(value, "landscape" | "portrait"),
        "prefers-color-scheme" => value == if crate::system::system_theme().dark { "dark" } else { "light" },
        "prefers-reduced-motion" => value == "no-preference",
        "hover" => value == "hover",
        "pointer" => value == "fine",
        _ => {
            let Some(v) = length(value) else { return false };
            match name {
                "min-width" => vw >= v,
                "max-width" => vw <= v,
                "width" => vw == v,
                "min-height" => vh >= v,
                "max-height" => vh <= v,
                "height" => vh == v,
                _ => false,
            }
        }
    }
}

/// Parses `selector, selector { decls }`. Returns `Ok(None)` for rules we
/// recognise but don't support (unsupported selectors, at-rules).
fn parse_rule<'i>(p: &mut Parser<'i>, cx: &Cx, sheet: &mut Stylesheet) -> PResult<()> {
    let start = p.position();
    let selectors = p.parse_until_before(Delimiter::CurlyBracketBlock | Delimiter::Semicolon, |p| {
        SelectorList::parse(&NileParser, p, ParseRelative::No).map_err(|_| ParseError::<()>::custom(()))
    });
    let selector_text = p.slice_from(start).trim().to_string();
    match p.next()? {
        Token::CurlyBracketBlock => {
            let body = p.parse_nested_block(|p| {
                let from = p.position();
                while p.next().is_ok() {}
                Ok::<_, ParseError<()>>(p.slice_from(from).to_string())
            })?;
            // A failed selector list drops the rule (its block is skipped above).
            if let Ok(selectors) = selectors {
                add_rule(&selector_text, selectors, &body, cx, sheet, 0);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// One item of a rule's block: declarations, or a nested rule / at-rule with its block.
enum BodyItem {
    Declarations(String),
    Nested(String, String),
}

/// Splits a style rule's block (CSS nesting) into declarations and nested rules.
fn split_body(body: &str) -> Vec<BodyItem> {
    let mut items = Vec::new();
    let (mut depth, mut start, mut quote) = (0_i32, 0, None::<char>);
    let mut decls = String::new();
    let mut prelude: Option<(usize, usize)> = None;
    let mut chars = body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if let Some(q) = quote {
            if c == '\\' {
                chars.next();
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ';' if depth == 0 => {
                decls.push_str(&body[start..=i]);
                start = i + 1;
            }
            '{' if depth == 0 => {
                prelude = Some((start, i));
                depth = 1_000;
                start = i + 1;
            }
            '{' => depth += 1,
            '}' if depth > 1_000 => depth -= 1,
            '}' if depth == 1_000 => {
                let (a, b) = prelude.take().unwrap_or((0, 0));
                items.push(BodyItem::Nested(body[a..b].trim().to_string(), body[start..i].to_string()));
                depth = 0;
                start = i + 1;
            }
            _ => {}
        }
    }
    decls.push_str(&body[start.min(body.len())..]);
    if !decls.trim().is_empty() {
        items.insert(0, BodyItem::Declarations(decls));
    }
    items
}

/// Adds a rule for `selectors` with the declarations in `body`, and the rules nested in it.
fn add_rule(selector_text: &str, selectors: SelectorList<NileImpl>, body: &str, cx: &Cx, sheet: &mut Stylesheet, depth: u32) {
    let layer = sheet.current_layer();
    if !body.contains('{') {
        let declarations = declarations_in(&mut Parser::new(body), cx);
        sheet.rules.push(Rule { selectors, declarations, layer });
        return;
    }
    let mut nested = Vec::new();
    let mut declarations = Vec::new();
    for item in split_body(body) {
        match item {
            BodyItem::Declarations(text) => declarations.extend(declarations_in(&mut Parser::new(&text), cx)),
            BodyItem::Nested(prelude, block) => nested.push((prelude, block)),
        }
    }
    if !declarations.is_empty() {
        sheet.rules.push(Rule { selectors: selectors.clone(), declarations, layer });
    }
    if depth > 16 {
        return;
    }
    for (prelude, block) in nested {
        let lower = prelude.to_ascii_lowercase();
        if let Some(at) = lower.strip_prefix('@') {
            // Conditional rules nested in a rule apply their declarations to the same selector.
            let (kind, condition) = at.split_once(char::is_whitespace).unwrap_or((at, ""));
            let holds = match kind {
                "media" => {
                    cx.uses_viewport.set(true);
                    media_matches(condition, cx.viewport)
                }
                "supports" => supports_matches(condition, cx.viewport),
                "layer" | "scope" | "container" => true,
                _ => false,
            };
            if holds {
                add_rule(selector_text, selectors.clone(), &block, cx, sheet, depth + 1);
            }
            continue;
        }
        // `&` stands for the parent; without one the nested selector is a descendant of it.
        let parent = if selector_text.contains(',') { format!(":is({selector_text})") } else { selector_text.to_string() };
        let combined: Vec<String> = prelude
            .split(',')
            .map(|part| {
                let part = part.trim();
                if part.contains('&') {
                    part.replace('&', &parent)
                } else {
                    // A descendant, or related through a leading combinator (`> p`).
                    format!("{parent} {part}")
                }
            })
            .collect();
        let text = combined.join(", ");
        let mut parser = Parser::new(&text);
        if let Ok(list) = SelectorList::parse(&NileParser, &mut parser, ParseRelative::No) {
            add_rule(&text, list, &block, cx, sheet, depth + 1);
        }
    }
}

/// Parses the contents of a `style="..."` attribute.
pub fn parse_declarations(css: &str, cx: &Cx) -> Vec<Declaration> {
    let mut parser = Parser::new(css);
    declarations_in(&mut parser, cx)
}

/// Parses a property's value text (after `var()` substitution) into values.
pub fn parse_values(css: &str, cx: &Cx) -> Vec<Value> {
    let mut parser = Parser::new(css);
    let mut values = Vec::new();
    while !parser.is_exhausted() {
        match parse_value(&mut parser, cx) {
            Ok(Some(v)) => values.push(v),
            Ok(None) => {}
            Err(_) => break,
        }
    }
    values
}

fn declarations_in(p: &mut Parser<'_>, cx: &Cx) -> Vec<Declaration> {
    let mut out = Vec::new();
    while !p.is_exhausted() {
        let result = p.parse_until_after(Delimiter::Semicolon, |p| {
            let ident = p.expect_ident()?.clone();
            let custom = ident.starts_with("--");
            let name = if custom { ident.to_string() } else { ident.to_ascii_lowercase() };
            p.expect_colon()?;
            let start = p.position();
            let mut values = Vec::new();
            let mut important = false;
            while !p.is_exhausted() {
                if custom {
                    let opens_block = matches!(
                        p.next_including_whitespace()?,
                        Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock
                    );
                    if opens_block {
                        p.parse_nested_block(|p| {
                            while p.next_including_whitespace().is_ok() {}
                            Ok::<_, ParseError<()>>(())
                        })?;
                    }
                    continue;
                }
                let state = p.state();
                if matches!(p.next(), Ok(Token::Delim('!'))) {
                    p.expect_ident_matching("important")?;
                    important = true;
                    continue;
                }
                p.reset(&state);
                if let Some(v) = parse_value(p, cx)? {
                    values.push(v);
                }
            }
            let mut raw = p.slice_from(start).trim().to_string();
            if custom {
                if let Some(rest) = raw.strip_suffix("important") {
                    if let Some(rest) = rest.trim_end().strip_suffix('!') {
                        important = true;
                        raw = rest.trim_end().to_string();
                    }
                }
            } else if !raw.contains("var(") && !keeps_raw(&name) {
                raw.clear();
            }
            Ok::<_, ParseError<()>>(Declaration { name, values, important, raw })
        });
        if let Ok(decl) = result {
            out.push(decl);
        }
    }
    out
}

const ABSOLUTE_UNITS: [(&str, f32); 5] = [("cm", 96.0 / 2.54), ("mm", 96.0 / 25.4), ("in", 96.0), ("pc", 16.0), ("q", 96.0 / 101.6)];

fn parse_value<'i>(p: &mut Parser<'i>, cx: &Cx) -> PResult<Option<Value>> {
    let token = p.next()?.clone();
    Ok(Some(match token {
        Token::Number { value, .. } => Value::Number(value),
        Token::Percentage { unit_value, .. } => Value::Percent(unit_value * 100.0),
        Token::Dimension { value, ref unit, .. } => {
            let unit = unit.to_ascii_lowercase();
            if let Some((px, _)) = viewport_units(value, &unit, cx) {
                Value::Px(px)
            } else {
                match unit.as_str() {
                    "px" => Value::Px(value),
                    "em" => Value::Em(value),
                    "rem" => Value::Rem(value),
                    // Font-relative units that need font metrics are approximated from the em.
                    "ch" | "ex" => Value::Em(value * 0.5),
                    "cap" => Value::Em(value * 0.7),
                    "ic" => Value::Em(value),
                    "lh" | "rlh" => Value::Em(value * 1.2),
                    "pt" => Value::Px(value * 4.0 / 3.0),
                    "fr" => Value::Fr(value),
                    "s" => Value::Time(value),
                    "ms" => Value::Time(value / 1000.0),
                    "deg" => Value::Angle(value),
                    "turn" => Value::Angle(value * 360.0),
                    "rad" => Value::Angle(value.to_degrees()),
                    "grad" => Value::Angle(value * 0.9),
                    u => match ABSOLUTE_UNITS.iter().find(|(n, _)| *n == u) {
                        Some((_, k)) => Value::Px(value * k),
                        None => return Ok(None),
                    },
                }
            }
        }
        Token::Comma => Value::Comma,
        Token::UnquotedUrl(ref u) => Value::Url(u.to_string()),
        Token::Delim('/') => Value::Slash,
        Token::Ident(ref s) => Value::Ident(s.to_ascii_lowercase()),
        Token::QuotedString(ref s) => Value::Str(s.to_string()),
        Token::Hash(ref h) | Token::IDHash(ref h) => match hex_color(h) {
            Some(c) => Value::Color(c),
            None => return Ok(None),
        },
        Token::Function(ref name) => {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "rgb" | "rgba" | "hsl" | "hsla" => {
                    let args = p.parse_nested_block(color_args)?;
                    match color_function(&name, &args) {
                        Some(c) => Value::Color(c),
                        None => return Ok(None),
                    }
                }
                "oklab" | "oklch" | "lab" | "lch" | "hwb" => {
                    let args = p.parse_nested_block(color_args)?;
                    match modern_color(&name, &args) {
                        Some(c) => Value::Color(c),
                        None => return Ok(None),
                    }
                }
                "color" => {
                    let parsed = p.parse_nested_block(|p| {
                        let space = p.expect_ident()?.to_ascii_lowercase();
                        Ok::<_, ParseError<()>>((space, color_args(p)?))
                    })?;
                    let (space, args) = parsed;
                    let [r, g, b, rest @ ..] = args.as_slice() else { return Ok(None) };
                    let ch = |x: &Arg| if x.pct { x.v / 100.0 } else { x.v };
                    let alpha = rest.first().map_or(1.0, ch);
                    match crate::color::in_space(&space, [ch(r), ch(g), ch(b)], alpha) {
                        Some(c) => Value::Color(c),
                        None => return Ok(None),
                    }
                }
                "color-mix" => {
                    let items = p.parse_nested_block(|p| gradient_items(p, cx))?;
                    let [head, a, b] = items.as_slice() else { return Ok(None) };
                    let space = match head.as_slice() {
                        [Value::Ident(i), Value::Ident(space), ..] if i == "in" => space.clone(),
                        _ => return Ok(None),
                    };
                    let side = |v: &Vec<Value>| -> Option<(Color, Option<f32>)> {
                        let color = v.iter().find_map(Value::color)?;
                        let pct = v.iter().find_map(|x| if let Value::Percent(p) = x { Some(*p) } else { None });
                        Some((color, pct))
                    };
                    match (side(a), side(b)) {
                        (Some(a), Some(b)) => match crate::color::mix(&space, a, b) {
                            Some(c) => Value::Color(c),
                            None => return Ok(None),
                        },
                        _ => return Ok(None),
                    }
                }
                "light-dark" => {
                    let items = p.parse_nested_block(|p| gradient_items(p, cx))?;
                    match items.as_slice() {
                        [a, b] => match (a.iter().find_map(Value::color), b.iter().find_map(Value::color)) {
                            (Some(l), Some(d)) => Value::LightDark(l, d),
                            _ => return Ok(None),
                        },
                        _ => return Ok(None),
                    }
                }
                "calc" | "-webkit-calc" | "min" | "max" | "clamp" | "abs" | "sign" | "sqrt" | "pow" | "hypot" | "log" | "exp" | "sin" | "cos" | "tan" | "asin"
                | "acos" | "atan" | "atan2" | "round" | "mod" | "rem" => {
                    let q = p.parse_nested_block(|p| math_function(&name, p, cx))?;
                    q.into_value()
                }
                "linear-gradient" | "repeating-linear-gradient" | "radial-gradient" | "repeating-radial-gradient" => {
                    let items = p.parse_nested_block(|p| gradient_items(p, cx))?;
                    match build_gradient(&name, items) {
                        Some(g) => Value::Gradient(Box::new(g)),
                        None => return Ok(None),
                    }
                }
                "minmax" | "fit-content" => {
                    let args = p.parse_nested_block(|p| gradient_items(p, cx))?;
                    let flat: Vec<&Vec<Value>> = args.iter().collect();
                    match (name.as_str(), flat.as_slice()) {
                        ("minmax", [a, b]) if a.len() == 1 && b.len() == 1 => Value::Minmax(Box::new((a[0].clone(), b[0].clone()))),
                        ("fit-content", [a]) if a.len() == 1 => Value::Minmax(Box::new((Value::Ident("auto".into()), a[0].clone()))),
                        _ => return Ok(None),
                    }
                }
                "repeat" => {
                    let args = p.parse_nested_block(|p| gradient_items(p, cx))?;
                    let Some((count, rest)) = args.split_first() else { return Ok(None) };
                    let kind = match count.as_slice() {
                        [Value::Number(n)] if n.fract() == 0.0 && *n >= 1.0 => RepeatKind::Count(*n as u16),
                        [Value::Ident(i)] if i == "auto-fill" => RepeatKind::AutoFill,
                        [Value::Ident(i)] if i == "auto-fit" => RepeatKind::AutoFit,
                        _ => return Ok(None),
                    };
                    let tracks: Vec<Value> = rest.iter().flatten().cloned().collect();
                    if tracks.is_empty() {
                        return Ok(None);
                    }
                    Value::Repeat(Box::new((kind, tracks)))
                }
                "url" => {
                    let url = p.parse_nested_block(|p| Ok::<_, ParseError<()>>(p.expect_string()?.to_string()))?;
                    Value::Url(url)
                }
                "matrix" | "translate" | "translatex" | "translatey" | "translate3d" | "scale" | "scalex" | "scaley" | "scale3d"
                | "rotate" | "rotatez" | "skew" | "skewx" | "skewy" | "rotatex" | "rotatey" | "rotate3d" | "translatez" | "scalez"
                | "perspective" | "matrix3d" => {
                    let args = p.parse_nested_block(|p| gradient_items(p, cx))?;
                    match transform_fn(&name, &args) {
                        Some(t) => Value::Transform(t),
                        None => return Ok(None),
                    }
                }
                "cubic-bezier" | "steps" => {
                    let args = p.parse_nested_block(|p| gradient_items(p, cx))?;
                    let nums: Vec<f32> = args.iter().filter_map(|a| if let [Value::Number(n)] = a.as_slice() { Some(*n) } else { None }).collect();
                    match (name.as_str(), nums.as_slice()) {
                        ("cubic-bezier", [a, b, c, d]) if (0.0..=1.0).contains(a) && (0.0..=1.0).contains(c) => Value::Timing(Timing::Bezier(*a, *b, *c, *d)),
                        ("steps", [n]) if *n >= 1.0 => {
                            let start = args.get(1).and_then(|a| a.first()).and_then(Value::ident).is_some_and(|i| i == "start" || i == "jump-start");
                            Value::Timing(Timing::Steps(*n as u32, start))
                        }
                        _ => return Ok(None),
                    }
                }
                "counter" | "counters" => {
                    let plural = name == "counters";
                    let args = p.parse_nested_block(|p| gradient_items(p, cx))?;
                    let at = |i: usize| args.get(i).and_then(|a| a.first());
                    let Some(Value::Ident(counter)) = at(0) else { return Ok(None) };
                    let (sep, style_at) = if plural {
                        let Some(Value::Str(sep)) = at(1) else { return Ok(None) };
                        (Some(sep.clone()), 2)
                    } else {
                        (None, 1)
                    };
                    let style = match at(style_at) {
                        Some(Value::Ident(i)) => i.clone(),
                        _ => "decimal".to_string(),
                    };
                    Value::Counter(Box::new((counter.clone(), sep, style)))
                }
                // `env(safe-area-inset-top, 0px)`: a desktop window has no insets; other names use the fallback.
                "env" => {
                    let items = p.parse_nested_block(|p| gradient_items(p, cx))?;
                    let known = matches!(items.first().and_then(|i| i.first()), Some(Value::Ident(n)) if n.starts_with("safe-area-inset-"));
                    match (known, items.get(1).and_then(|i| i.first())) {
                        (true, _) => Value::Px(0.0),
                        (false, Some(fallback)) => fallback.clone(),
                        _ => return Ok(None),
                    }
                }
                "attr" => {
                    let name = p.parse_nested_block(|p| Ok::<_, ParseError<()>>(p.expect_ident()?.to_string()))?;
                    Value::Attr(name)
                }
                // `var()`, `url()`, gradients, ...: not handled here.
                _ => {
                    p.parse_nested_block(|p| {
                        while p.next().is_ok() {}
                        Ok::<_, ParseError<()>>(())
                    })?;
                    return Ok(None);
                }
            }
        }
        _ => return Ok(None),
    }))
}

/// The comma-separated items inside a gradient function.
fn gradient_items(p: &mut Parser<'_>, cx: &Cx) -> PResult<Vec<Vec<Value>>> {
    let mut items = Vec::new();
    let mut cur = Vec::new();
    while !p.is_exhausted() {
        match parse_value(p, cx)? {
            Some(Value::Comma) => items.push(std::mem::take(&mut cur)),
            Some(v) => cur.push(v),
            None => {}
        }
    }
    items.push(cur);
    Ok(items)
}

fn stop_pos(v: &Value) -> Option<StopPos> {
    match v {
        Value::Px(px) => Some(StopPos::Px(*px)),
        Value::Percent(p) => Some(StopPos::Fraction(p / 100.0)),
        Value::Number(n) if *n == 0.0 => Some(StopPos::Px(0.0)),
        _ => None,
    }
}

/// Builds a gradient from its items: an optional first item that configures the
/// direction or shape, then color stops (a color with up to two positions).
fn build_gradient(name: &str, mut items: Vec<Vec<Value>>) -> Option<Gradient> {
    let repeating = name.starts_with("repeating-");
    let radial = name.ends_with("radial-gradient");
    let config = if items.first()?.iter().any(|v| v.color().is_some()) { None } else { Some(items.remove(0)) };
    let mut stops = Vec::new();
    for item in &items {
        let color = item.iter().find_map(Value::color);
        let mut positions = item.iter().filter_map(stop_pos);
        // An item with no color is a color hint (`50%`), which isn't supported.
        let Some(color) = color else { continue };
        match (positions.next(), positions.next()) {
            (Some(a), Some(b)) => {
                stops.push(ColorStop { color, pos: Some(a) });
                stops.push(ColorStop { color, pos: Some(b) });
            }
            (pos, _) => stops.push(ColorStop { color, pos }),
        }
    }
    if stops.len() < 2 {
        return None;
    }
    let config = config.unwrap_or_default();
    let kind = if radial { radial_kind(&config)? } else { GradientKind::Linear(linear_dir(&config)?) };
    Some(Gradient { kind, stops, repeating })
}

fn linear_dir(config: &[Value]) -> Option<LinearDir> {
    if config.is_empty() {
        return Some(LinearDir::Angle(180.0));
    }
    if let [Value::Angle(a)] = config {
        return Some(LinearDir::Angle(*a));
    }
    if config.first().and_then(Value::ident) != Some("to") {
        return None;
    }
    let (mut x, mut y) = (0i8, 0i8);
    for v in &config[1..] {
        match v.ident()? {
            "left" => x = -1,
            "right" => x = 1,
            "top" => y = -1,
            "bottom" => y = 1,
            _ => return None,
        }
    }
    Some(match (x, y) {
        (0, -1) => LinearDir::Angle(0.0),
        (1, 0) => LinearDir::Angle(90.0),
        (0, 1) => LinearDir::Angle(180.0),
        (-1, 0) => LinearDir::Angle(270.0),
        (0, 0) => return None,
        (x, y) => LinearDir::Corner(x, y),
    })
}

fn radial_kind(config: &[Value]) -> Option<GradientKind> {
    let center = (StopPos::Fraction(0.5), StopPos::Fraction(0.5));
    let (shape_part, at_part) = match config.iter().position(|v| v.ident() == Some("at")) {
        Some(i) => (&config[..i], &config[i + 1..]),
        None => (config, &[][..]),
    };
    let mut circle = None;
    let mut extent = None;
    let mut radii = Vec::new();
    for v in shape_part {
        match v.ident() {
            Some("circle") => circle = Some(true),
            Some("ellipse") => circle = Some(false),
            Some("closest-side") => extent = Some(Extent::ClosestSide),
            Some("closest-corner") => extent = Some(Extent::ClosestCorner),
            Some("farthest-side") => extent = Some(Extent::FarthestSide),
            Some("farthest-corner") => extent = Some(Extent::FarthestCorner),
            Some(_) => return None,
            None => radii.push(stop_pos(v)?),
        }
    }
    let circle = circle.unwrap_or(radii.len() == 1);
    let extent = match radii.as_slice() {
        [] => extent.unwrap_or(Extent::FarthestCorner),
        [r] => Extent::Radii(*r, *r),
        [rx, ry, ..] => Extent::Radii(*rx, *ry),
    };
    // `at <x> [<y>]`, with keywords or lengths.
    let mut x = None;
    let mut y = None;
    let mut free = Vec::new();
    for v in at_part {
        let half = StopPos::Fraction(0.5);
        match v.ident() {
            Some("left") => x = Some(StopPos::Fraction(0.0)),
            Some("right") => x = Some(StopPos::Fraction(1.0)),
            Some("top") => y = Some(StopPos::Fraction(0.0)),
            Some("bottom") => y = Some(StopPos::Fraction(1.0)),
            Some("center") => free.push(half),
            Some(_) => return None,
            None => free.push(stop_pos(v)?),
        }
    }
    let mut free = free.into_iter();
    let x = x.or_else(|| free.next()).unwrap_or(center.0);
    let y = y.or_else(|| free.next()).unwrap_or(center.1);
    Some(GradientKind::Radial { circle, extent, at: (x, y) })
}

fn transform_fn(name: &str, args: &[Vec<Value>]) -> Option<TransformFn> {
    let one = |i: usize| -> Option<&Value> { match args.get(i)?.as_slice() { [v] => Some(v), _ => None } };
    let len = |v: &Value| -> Option<StopPos> {
        match v {
            Value::Px(p) => Some(StopPos::Px(*p)),
            Value::Percent(p) => Some(StopPos::Fraction(p / 100.0)),
            Value::Number(n) if *n == 0.0 => Some(StopPos::Px(0.0)),
            // `em`/`rem` need the font size; treated like the default 16px.
            Value::Em(e) | Value::Rem(e) => Some(StopPos::Px(e * 16.0)),
            _ => None,
        }
    };
    let angle = |v: &Value| -> Option<f32> {
        match v {
            Value::Angle(a) => Some(*a),
            Value::Number(n) if *n == 0.0 => Some(0.0),
            _ => None,
        }
    };
    let num = |v: &Value| if let Value::Number(n) = v { Some(*n) } else { None };
    let zero = StopPos::Px(0.0);
    Some(match name {
        "matrix" if args.len() == 6 => {
            let mut m = [0.0; 6];
            for (i, slot) in m.iter_mut().enumerate() {
                *slot = num(one(i)?)?;
            }
            TransformFn::Matrix(m)
        }
        "matrix3d" if args.len() == 16 => {
            let mut m = [0.0; 16];
            for (i, slot) in m.iter_mut().enumerate() {
                *slot = num(one(i)?)?;
            }
            TransformFn::Matrix3d(m)
        }
        "translate3d" if args.len() == 3 => {
            let z = match len(one(2)?)? {
                StopPos::Px(p) => p,
                StopPos::Fraction(_) => return None,
            };
            TransformFn::Translate3d(len(one(0)?)?, len(one(1)?)?, z)
        }
        "translatez" => match len(one(0)?)? {
            StopPos::Px(p) => TransformFn::Translate3d(zero, zero, p),
            StopPos::Fraction(_) => return None,
        },
        "scale3d" if args.len() == 3 => TransformFn::Scale3d(num(one(0)?)?, num(one(1)?)?, num(one(2)?)?),
        "scalez" => TransformFn::Scale3d(1.0, 1.0, num(one(0)?)?),
        "rotatex" => TransformFn::RotateX(angle(one(0)?)?),
        "rotatey" => TransformFn::RotateY(angle(one(0)?)?),
        "rotate3d" if args.len() == 4 => TransformFn::Rotate3d([num(one(0)?)?, num(one(1)?)?, num(one(2)?)?, angle(one(3)?)?]),
        "perspective" => match len(one(0)?)? {
            StopPos::Px(p) if p > 0.0 => TransformFn::Perspective(p),
            _ => return None,
        },
        "translate" => TransformFn::Translate(len(one(0)?)?, one(1).map_or(Some(zero), len)?),
        "translatex" => TransformFn::Translate(len(one(0)?)?, zero),
        "translatey" => TransformFn::Translate(zero, len(one(0)?)?),
        "scale" => {
            let x = num(one(0)?)?;
            TransformFn::Scale(x, one(1).map_or(Some(x), num)?)
        }
        "scalex" => TransformFn::Scale(num(one(0)?)?, 1.0),
        "scaley" => TransformFn::Scale(1.0, num(one(0)?)?),
        "rotate" | "rotatez" => TransformFn::Rotate(angle(one(0)?)?),
        "skew" => TransformFn::Skew(angle(one(0)?)?, one(1).map_or(Some(0.0), angle)?),
        "skewx" => TransformFn::Skew(angle(one(0)?)?, 0.0),
        "skewy" => TransformFn::Skew(0.0, angle(one(0)?)?),
        _ => return None,
    })
}

/// `vw`, `vh`, `vmin`, `vmax` as pixels of the current viewport.
fn viewport_units(value: f32, unit: &str, cx: &Cx) -> Option<(f32, ())> {
    let (w, h) = cx.viewport;
    let base = match unit {
        "vw" | "dvw" | "svw" | "lvw" => w,
        "vh" | "dvh" | "svh" | "lvh" => h,
        "vmin" | "dvmin" | "svmin" | "lvmin" => w.min(h),
        "vmax" | "dvmax" | "svmax" | "lvmax" => w.max(h),
        _ => return None,
    };
    cx.uses_viewport.set(true);
    Some((value * base / 100.0, ()))
}

/// One argument of a color function: a number, or a percentage (`pct`), or an angle.
struct Arg {
    v: f32,
    pct: bool,
}

fn color_args(p: &mut Parser<'_>) -> PResult<Vec<Arg>> {
    let mut args = Vec::new();
    while !p.is_exhausted() {
        match p.next()? {
            Token::Number { value, .. } => args.push(Arg { v: *value, pct: false }),
            Token::Percentage { unit_value, .. } => args.push(Arg { v: *unit_value * 100.0, pct: true }),
            Token::Dimension { value, unit, .. } => {
                let deg = match unit.to_ascii_lowercase().as_str() {
                    "deg" => *value,
                    "turn" => *value * 360.0,
                    "rad" => value.to_degrees(),
                    "grad" => *value * 0.9,
                    _ => continue,
                };
                args.push(Arg { v: deg, pct: false });
            }
            Token::Ident(i) if i.eq_ignore_ascii_case("none") => args.push(Arg { v: 0.0, pct: false }),
            _ => {}
        }
    }
    Ok(args)
}

/// `oklab()`, `oklch()`, `lab()`, `lch()` and `hwb()`.
fn modern_color(name: &str, args: &[Arg]) -> Option<Color> {
    let [a, b, c, rest @ ..] = args else { return None };
    let alpha = rest.first().map_or(1.0, |x| if x.pct { x.v / 100.0 } else { x.v });
    let ch = |x: &Arg, full: f32| if x.pct { x.v / 100.0 * full } else { x.v };
    Some(match name {
        "oklab" => crate::color::oklab(ch(a, 1.0), ch(b, 0.4), ch(c, 0.4), alpha),
        "oklch" => crate::color::oklch(ch(a, 1.0), ch(b, 0.4), c.v, alpha),
        "lab" => crate::color::lab(ch(a, 100.0), ch(b, 125.0), ch(c, 125.0), alpha),
        "lch" => crate::color::lch(ch(a, 100.0), ch(b, 150.0), c.v, alpha),
        "hwb" => crate::color::hwb(a.v, b.v / 100.0, c.v / 100.0, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8),
        _ => return None,
    })
}

fn color_function(name: &str, args: &[Arg]) -> Option<Color> {
    let [a, b, c, rest @ ..] = args else { return None };
    let alpha = rest.first().map_or(255, |x| {
        let f = if x.pct { x.v / 100.0 } else { x.v };
        (f.clamp(0.0, 1.0) * 255.0) as u8
    });
    match name {
        "rgb" | "rgba" => {
            let ch = |x: &Arg| (if x.pct { x.v * 2.55 } else { x.v }).clamp(0.0, 255.0).round() as u8;
            Some(Color { r: ch(a), g: ch(b), b: ch(c), a: alpha })
        }
        _ => Some(colors::hsl(a.v, b.v / 100.0, c.v / 100.0, alpha)),
    }
}

/// A `calc()` quantity: a plain number, or a length/percentage mix.
#[derive(Clone, Copy)]
struct Quantity {
    num: f32,
    is_num: bool,
    px: f32,
    em: f32,
    pct: f32,
}

impl Quantity {
    fn number(num: f32) -> Quantity {
        Quantity { num, is_num: true, px: 0.0, em: 0.0, pct: 0.0 }
    }

    fn length(px: f32, em: f32, pct: f32) -> Quantity {
        Quantity { num: 0.0, is_num: false, px, em, pct }
    }

    fn scale(self, k: f32) -> Quantity {
        Quantity { num: self.num * k, px: self.px * k, em: self.em * k, pct: self.pct * k, ..self }
    }

    fn add(self, o: Quantity, sign: f32) -> Option<Quantity> {
        if self.is_num != o.is_num {
            return None;
        }
        Some(Quantity {
            num: self.num + sign * o.num,
            px: self.px + sign * o.px,
            em: self.em + sign * o.em,
            pct: self.pct + sign * o.pct,
            ..self
        })
    }

    /// Orders two quantities of the same kind; `None` if they can't be compared
    /// before layout (an `em` and a `px`, or a `%` and a `px`).
    fn compare(&self, o: &Quantity) -> Option<std::cmp::Ordering> {
        let key = |q: &Quantity| -> Option<f32> {
            if q.is_num {
                Some(q.num)
            } else if q.em == 0.0 && q.pct == 0.0 {
                Some(q.px)
            } else if q.px == 0.0 && q.em == 0.0 {
                Some(q.pct)
            } else {
                None
            }
        };
        let same_kind = self.is_num == o.is_num && (self.pct == 0.0) == (o.pct == 0.0);
        if !same_kind {
            return None;
        }
        key(self)?.partial_cmp(&key(o)?)
    }

    fn into_value(self) -> Value {
        if self.is_num {
            Value::Number(self.num)
        } else {
            Value::Calc { px: self.px, em: self.em, pct: self.pct }
        }
    }
}

fn bad<T>() -> PResult<T> {
    Err(ParseError::custom(()))
}

fn math_function(name: &str, p: &mut Parser<'_>, cx: &Cx) -> PResult<Quantity> {
    if name == "calc" || name == "-webkit-calc" {
        let q = calc_sum(p, cx)?;
        p.expect_exhausted()?;
        return Ok(q);
    }
    // `round(<strategy>, a, b)`.
    let strategy = if name == "round" {
        p.try_parse(|p| {
            let i = p.expect_ident()?.to_ascii_lowercase();
            p.expect_comma()?;
            Ok::<_, ParseError<()>>(i)
        })
        .ok()
    } else {
        None
    };
    let mut args = Vec::new();
    loop {
        args.push(calc_sum(p, cx)?);
        if p.try_parse(|p| p.expect_comma()).is_err() {
            break;
        }
    }
    p.expect_exhausted()?;
    if let Some(q) = fold_math(name, strategy.as_deref(), &args) {
        return Ok(q);
    }
    let pick = |args: &[Quantity], want: std::cmp::Ordering| -> PResult<Quantity> {
        let mut best = args[0];
        for a in &args[1..] {
            match a.compare(&best) {
                Some(o) if o == want => best = *a,
                Some(_) => {}
                None => return bad(),
            }
        }
        Ok(best)
    };
    match (name, args.as_slice()) {
        ("min", [_, ..]) => pick(&args, std::cmp::Ordering::Less),
        ("max", [_, ..]) => pick(&args, std::cmp::Ordering::Greater),
        // clamp(min, value, max) = max(min, min(value, max))
        ("clamp", [lo, v, hi]) => {
            let capped = pick(&[*v, *hi], std::cmp::Ordering::Less)?;
            pick(&[*lo, capped], std::cmp::Ordering::Greater)
        }
        _ => bad(),
    }
}

/// The single number a quantity stands for when it needs no layout: a plain number or a `px` length.
fn scalar(q: &Quantity) -> Option<f32> {
    if q.is_num {
        Some(q.num)
    } else if q.em == 0.0 && q.pct == 0.0 {
        Some(q.px)
    } else {
        None
    }
}

/// `abs`, `sign`, `sqrt`, `pow`, `hypot`, `log`, `exp`, the trigonometric functions, `round`, `mod` and
/// `rem` of quantities that are known now (numbers or `px`); `None` for anything else.
fn fold_math(name: &str, strategy: Option<&str>, args: &[Quantity]) -> Option<Quantity> {
    let vals: Vec<f32> = args.iter().map(scalar).collect::<Option<_>>()?;
    let kind_num = args.iter().all(|a| a.is_num);
    let same_kind = args.iter().all(|a| a.is_num == args[0].is_num);
    let like = |v: f32| if args[0].is_num { Quantity::number(v) } else { Quantity::length(v, 0.0, 0.0) };
    let number = Quantity::number;
    Some(match (name, vals.as_slice()) {
        ("abs", [a]) => like(a.abs()),
        ("sign", [a]) => number(if *a == 0.0 { 0.0 } else { a.signum() }),
        ("sqrt", [a]) if kind_num => number(a.sqrt()),
        ("pow", [a, b]) if kind_num => number(a.powf(*b)),
        ("hypot", v) if same_kind && !v.is_empty() => like(v.iter().map(|x| x * x).sum::<f32>().sqrt()),
        ("log", [a]) if kind_num => number(a.ln()),
        ("log", [a, b]) if kind_num => number(a.ln() / b.ln()),
        ("exp", [a]) if kind_num => number(a.exp()),
        ("sin", [a]) if kind_num => number(a.sin()),
        ("cos", [a]) if kind_num => number(a.cos()),
        ("tan", [a]) if kind_num => number(a.tan()),
        ("asin", [a]) if kind_num => number(a.asin()),
        ("acos", [a]) if kind_num => number(a.acos()),
        ("atan", [a]) if kind_num => number(a.atan()),
        ("atan2", [a, b]) if same_kind => number(a.atan2(*b)),
        ("round", [a]) if kind_num => number(a.round()),
        ("round", [a, b]) if same_kind && *b != 0.0 => {
            let q = a / b;
            let n = match strategy.unwrap_or("nearest") {
                "up" => q.ceil(),
                "down" => q.floor(),
                "to-zero" => q.trunc(),
                _ => q.round(),
            };
            like(n * b)
        }
        ("mod", [a, b]) if same_kind && *b != 0.0 => like(a - b * (a / b).floor()),
        ("rem", [a, b]) if same_kind && *b != 0.0 => like(a - b * (a / b).trunc()),
        _ => return None,
    })
}

fn calc_sum(p: &mut Parser<'_>, cx: &Cx) -> PResult<Quantity> {
    let mut acc = calc_product(p, cx)?;
    loop {
        let state = p.state();
        let sign = match p.next() {
            Ok(Token::Delim('+')) => 1.0,
            Ok(Token::Delim('-')) => -1.0,
            _ => {
                p.reset(&state);
                return Ok(acc);
            }
        };
        let rhs = calc_product(p, cx)?;
        acc = match acc.add(rhs, sign) {
            Some(q) => q,
            None => return bad(),
        };
    }
}

fn calc_product(p: &mut Parser<'_>, cx: &Cx) -> PResult<Quantity> {
    let mut acc = calc_factor(p, cx)?;
    loop {
        let state = p.state();
        let divide = match p.next() {
            Ok(Token::Delim('*')) => false,
            Ok(Token::Delim('/')) => true,
            _ => {
                p.reset(&state);
                return Ok(acc);
            }
        };
        let rhs = calc_factor(p, cx)?;
        acc = match (divide, acc.is_num, rhs.is_num) {
            (false, _, true) => acc.scale(rhs.num),
            (false, true, false) => rhs.scale(acc.num),
            (true, _, true) if rhs.num != 0.0 => acc.scale(1.0 / rhs.num),
            _ => return bad(),
        };
    }
}

fn calc_factor(p: &mut Parser<'_>, cx: &Cx) -> PResult<Quantity> {
    let token = p.next()?.clone();
    Ok(match token {
        Token::Number { value, .. } => Quantity::number(value),
        Token::Percentage { unit_value, .. } => Quantity::length(0.0, 0.0, unit_value * 100.0),
        Token::Dimension { value, ref unit, .. } => {
            let unit = unit.to_ascii_lowercase();
            if let Some((px, _)) = viewport_units(value, &unit, cx) {
                Quantity::length(px, 0.0, 0.0)
            } else {
                match unit.as_str() {
                    "px" => Quantity::length(value, 0.0, 0.0),
                    "em" => Quantity::length(0.0, value, 0.0),
                    "ch" | "ex" => Quantity::length(0.0, value * 0.5, 0.0),
                    "cap" => Quantity::length(0.0, value * 0.7, 0.0),
                    "ic" => Quantity::length(0.0, value, 0.0),
                    "lh" | "rlh" => Quantity::length(0.0, value * 1.2, 0.0),
                    "rem" => Quantity::length(value * crate::ROOT_FONT_SIZE, 0.0, 0.0),
                    "deg" => Quantity::number(value.to_radians()),
                    "turn" => Quantity::number(value * std::f32::consts::TAU),
                    "rad" => Quantity::number(value),
                    "grad" => Quantity::number(value * std::f32::consts::PI / 200.0),
                    "pt" => Quantity::length(value * 4.0 / 3.0, 0.0, 0.0),
                    u => match ABSOLUTE_UNITS.iter().find(|(n, _)| *n == u) {
                        Some((_, k)) => Quantity::length(value * k, 0.0, 0.0),
                        None => return bad(),
                    },
                }
            }
        }
        Token::Ident(ref i) if i.eq_ignore_ascii_case("pi") => Quantity::number(std::f32::consts::PI),
        Token::Ident(ref i) if i.eq_ignore_ascii_case("e") => Quantity::number(std::f32::consts::E),
        Token::ParenthesisBlock => p.parse_nested_block(|p| {
            let q = calc_sum(p, cx)?;
            p.expect_exhausted()?;
            Ok(q)
        })?,
        Token::Function(ref name) => {
            let name = name.to_ascii_lowercase();
            p.parse_nested_block(|p| math_function(&name, p, cx))?
        }
        _ => return bad(),
    })
}

/// `rrggbb`/`rgb` (and the alpha forms) without the `#`.
pub fn color_from_hex(h: &str) -> Option<Color> {
    h.is_ascii().then(|| hex_color(h)).flatten()
}

fn hex_color(h: &str) -> Option<Color> {
    let digit = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok();
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    match h.len() {
        3 | 4 => {
            let r = digit(0)? * 17;
            let g = digit(1)? * 17;
            let b = digit(2)? * 17;
            let a = if h.len() == 4 { digit(3)? * 17 } else { 255 };
            Some(Color { r, g, b, a })
        }
        6 | 8 => {
            let a = if h.len() == 8 { byte(6)? } else { 255 };
            Some(Color { r: byte(0)?, g: byte(2)?, b: byte(4)?, a })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rules_and_skips_unsupported() {
        let sheet = Stylesheet::parse(
            "h1, .title { color: #333; margin: 0 auto } div > p:first-child { color: red } \
             a:hover { color: blue } p::first-line { color: red } @import url(x.css); \
             li:unknown { color: red } #x.y[data-a=b] { gap: 4px }",
        );
        assert_eq!(sheet.rules.len(), 4);
        assert_eq!(sheet.rules[0].selectors.slice().len(), 2);
        assert_eq!(sheet.rules[0].declarations[1].values.len(), 2);
        assert_eq!(sheet.rules[3].declarations[0].name, "gap");
    }

    #[test]
    fn parses_colors() {
        let used = Cell::new(false);
        let cx = Cx { viewport: DEFAULT_VIEWPORT, uses_viewport: &used };
        let d = parse_declarations("color: rgba(10, 20, 30, 0.5); background: #fff", &cx);
        assert_eq!(d[0].values[0], Value::Color(Color { r: 10, g: 20, b: 30, a: 127 }));
        assert_eq!(d[1].values[0].color(), Some(Color::rgb(255, 255, 255)));
    }
}

