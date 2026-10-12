//! Style resolution: parse CSS, cascade it over the DOM, produce computed styles.
//!
//! Scope:
//! - `<style>` blocks; selectors matched by Servo's `selectors` crate (see
//!   `selector.rs` for what's supported)
//! - inline `style="..."` attributes
//! - a small user-agent stylesheet
//! - inheritance of text properties

mod parse;
mod selector;

use nile_dom::{Document, NodeId, NodeKind};
use selectors::context::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
    SelectorCaches,
};
use selectors::matching::matches_selector;
pub use parse::{Declaration, Rule, Stylesheet, Value};
pub use selector::{ElementRef, InteractionState, NileImpl, PseudoClass, Query};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const TRANSPARENT: Color = Color { r: 0, g: 0, b: 0, a: 0 };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::BLACK
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    #[default]
    Auto,
    Px(f32),
    Percent(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Edges<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> Edges<T> {
    pub fn all(v: T) -> Self {
        Edges { top: v, right: v, bottom: v, left: v }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Display {
    #[default]
    Block,
    Inline,
    /// A block container laid out as one atomic box inside an inline run.
    InlineBlock,
    Flex,
    None,
}

/// Native-looking control drawn by the renderer (`appearance`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Appearance {
    #[default]
    None,
    Checkbox,
    Radio,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FlexDirection {
    #[default]
    Row,
    Column,
    RowReverse,
    ColumnReverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Align {
    #[default]
    Normal,
    Start,
    End,
    Center,
    Stretch,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    // Box model
    pub display: Display,
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub min_height: Length,
    pub max_width: Length,
    pub max_height: Length,
    pub margin: Edges<Length>,
    pub padding: Edges<f32>,
    pub border_width: Edges<f32>,
    /// `box-sizing: border-box` (the CSS default is `content-box`).
    pub border_box: bool,
    pub appearance: Appearance,
    /// `overflow: hidden | scroll | auto` on either axis: content is clipped to the padding box.
    // TODO: scrolling; for now every non-visible overflow just clips.
    pub overflow_hidden: bool,
    /// The element's `checked` state flag (checkbox/radio marks), resolved at cascade time.
    pub checked: bool,

    // Flex
    pub flex_direction: FlexDirection,
    pub flex_wrap: bool,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Length,
    pub justify_content: Align,
    pub align_items: Align,
    pub gap: f32,

    // Paint
    pub background: Option<Color>,
    pub border_color: Color,
    pub border_radius: f32,
    pub opacity: f32,

    // Text (inherited)
    pub color: Color,
    pub font_size: f32,
    pub font_weight: f32,
    pub font_family: String,
    pub line_height: f32,
    pub text_align: TextAlign,
    pub italic: bool,
    pub underline: bool,
    pub line_through: bool,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        ComputedStyle {
            display: Display::Block,
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            min_height: Length::Auto,
            max_width: Length::Auto,
            max_height: Length::Auto,
            margin: Edges::all(Length::Px(0.0)),
            padding: Edges::all(0.0),
            border_width: Edges::all(0.0),
            border_box: false,
            appearance: Appearance::None,
            overflow_hidden: false,
            checked: false,
            flex_direction: FlexDirection::Row,
            flex_wrap: false,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Length::Auto,
            justify_content: Align::Normal,
            align_items: Align::Normal,
            gap: 0.0,
            background: None,
            border_color: Color::BLACK,
            border_radius: 0.0,
            opacity: 1.0,
            color: Color::BLACK,
            font_size: 16.0,
            font_weight: 400.0,
            font_family: "sans-serif".into(),
            line_height: 1.2,
            text_align: TextAlign::Start,
            italic: false,
            underline: false,
            line_through: false,
        }
    }
}

impl ComputedStyle {
    /// The CSS text of a property's computed value, as `getComputedStyle`
    /// reports it. `None` for properties we don't track. Sizes are in px;
    /// `width`/`height` report `auto` here (the used size comes from layout).
    pub fn css_value(&self, prop: &str) -> Option<String> {
        let px = |v: f32| format!("{}px", (v * 100.0).round() / 100.0);
        let len = |l: Length| match l {
            Length::Auto => "auto".to_string(),
            Length::Px(v) => px(v),
            Length::Percent(p) => format!("{}%", (p * 10000.0).round() / 100.0),
        };
        let color = |c: Color| {
            if c.a == 255 {
                format!("rgb({}, {}, {})", c.r, c.g, c.b)
            } else {
                format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, (f32::from(c.a) / 255.0 * 1000.0).round() / 1000.0)
            }
        };
        Some(match prop {
            "display" => match self.display {
                Display::Block => "block",
                Display::Inline => "inline",
                Display::InlineBlock => "inline-block",
                Display::Flex => "flex",
                Display::None => "none",
            }
            .into(),
            "overflow" => if self.overflow_hidden { "hidden" } else { "visible" }.into(),
            "appearance" => match self.appearance {
                Appearance::None => "none",
                Appearance::Checkbox => "checkbox",
                Appearance::Radio => "radio",
            }
            .into(),
            "box-sizing" => if self.border_box { "border-box" } else { "content-box" }.into(),
            "width" => len(self.width),
            "height" => len(self.height),
            "min-width" => len(self.min_width),
            "min-height" => len(self.min_height),
            "max-width" => len(self.max_width),
            "max-height" => len(self.max_height),
            "margin-top" => len(self.margin.top),
            "margin-right" => len(self.margin.right),
            "margin-bottom" => len(self.margin.bottom),
            "margin-left" => len(self.margin.left),
            "padding-top" => px(self.padding.top),
            "padding-right" => px(self.padding.right),
            "padding-bottom" => px(self.padding.bottom),
            "padding-left" => px(self.padding.left),
            "border-top-width" => px(self.border_width.top),
            "border-right-width" => px(self.border_width.right),
            "border-bottom-width" => px(self.border_width.bottom),
            "border-left-width" => px(self.border_width.left),
            "border-color" | "border-top-color" => color(self.border_color),
            "border-radius" => px(self.border_radius),
            "background-color" => color(self.background.unwrap_or(Color::TRANSPARENT)),
            "color" => color(self.color),
            "opacity" => self.opacity.to_string(),
            "font-size" => px(self.font_size),
            "font-weight" => (self.font_weight as i32).to_string(),
            "font-family" => self.font_family.clone(),
            "font-style" => if self.italic { "italic" } else { "normal" }.into(),
            "line-height" => px(self.line_height * self.font_size),
            "text-align" => match self.text_align {
                TextAlign::Start => "start",
                TextAlign::Center => "center",
                TextAlign::End => "end",
                TextAlign::Justify => "justify",
            }
            .into(),
            "text-decoration" | "text-decoration-line" => match (self.underline, self.line_through) {
                (true, true) => "underline line-through",
                (true, false) => "underline",
                (false, true) => "line-through",
                (false, false) => "none",
            }
            .into(),
            "flex-direction" => match self.flex_direction {
                FlexDirection::Row => "row",
                FlexDirection::Column => "column",
                FlexDirection::RowReverse => "row-reverse",
                FlexDirection::ColumnReverse => "column-reverse",
            }
            .into(),
            "flex-grow" => self.flex_grow.to_string(),
            "flex-shrink" => self.flex_shrink.to_string(),
            "gap" => px(self.gap),
            _ => return None,
        })
    }

    /// A fresh style for a child: inherited properties copied, the rest reset.
    fn inherit(parent: &ComputedStyle) -> ComputedStyle {
        ComputedStyle {
            color: parent.color,
            font_size: parent.font_size,
            font_weight: parent.font_weight,
            font_family: parent.font_family.clone(),
            line_height: parent.line_height,
            text_align: parent.text_align,
            italic: parent.italic,
            // TODO: decorations aren't really inherited in CSS; they propagate
            // to inline descendants, which this approximates.
            underline: parent.underline,
            line_through: parent.line_through,
            ..Default::default()
        }
    }
}

const ROOT_FONT_SIZE: f32 = 16.0;

const UA_STYLESHEET: &str = r#"
head, script, style, title, meta, link { display: none }
html, body, div, p, h1, h2, h3, section, header, footer, main, nav, ul, li { display: block }
span, a, b, strong, em, i, u, s, del, br, label { display: inline }
form { display: block }
body { margin: 8px }
h1 { font-size: 2em; font-weight: bold; margin: 0.67em 0 }
h2 { font-size: 1.5em; font-weight: bold; margin: 0.83em 0 }
h3 { font-size: 1.17em; font-weight: bold; margin: 1em 0 }
p { margin: 1em 0 }
b, strong { font-weight: bold }
em, i { font-style: italic }
u, a { text-decoration: underline }
s, del { text-decoration: line-through }
a { color: #0000ee }
button, input { display: inline-block; font-size: 13px; color: #000000; padding: 3px 8px;
  border: 1px solid #767676; border-radius: 3px; background: #efefef }
input { background: #ffffff; width: 160px; min-height: 16px; margin: 2px; overflow: hidden }
textarea { display: inline-block; font-size: 13px; color: #000000; padding: 3px 4px; border: 1px solid #767676;
  border-radius: 3px; background: #ffffff; width: 200px; height: 60px; margin: 2px; overflow: hidden }
input:focus, textarea:focus { border-color: #1d4ed8 }
button:disabled, input:disabled, textarea:disabled { color: #8a8a8a; background: #f0f0f0; border-color: #c4c4c4 }
input[type=checkbox], input[type=radio] { width: 13px; height: 13px; min-height: 0; padding: 0;
  margin: 3px 4px; background: #ffffff; appearance: checkbox }
input[type=radio] { border-radius: 7px; appearance: radio }
input[type=checkbox]:checked, input[type=radio]:checked { background: #1d4ed8; border-color: #1d4ed8; color: #ffffff }
input[type=button], input[type=submit], input[type=reset] { width: auto; background: #efefef }
"#;

/// Computed styles for every node, indexed by `NodeId.0`.
pub struct StyleMap {
    pub styles: Vec<ComputedStyle>,
}

impl StyleMap {
    pub fn get(&self, id: NodeId) -> &ComputedStyle {
        &self.styles[id.0]
    }
}

pub fn compute_styles(doc: &Document) -> StyleMap {
    compute_styles_with(doc, &InteractionState::default())
}

/// Like [`compute_styles`], with `:hover` / `:active` / `:focus` resolved
/// against `state`.
pub fn compute_styles_with(doc: &Document, state: &InteractionState) -> StyleMap {
    let ua = Stylesheet::parse(UA_STYLESHEET);
    let author: Vec<Stylesheet> = doc
        .nodes
        .iter()
        .filter(|n| n.tag() == Some("style"))
        .map(|n| {
            let css: String = n
                .children
                .iter()
                .filter_map(|c| match &doc.node(*c).kind {
                    NodeKind::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect();
            Stylesheet::parse(&css)
        })
        .collect();

    let mut styles = vec![ComputedStyle::default(); doc.nodes.len()];
    let root_style = ComputedStyle::default();
    let mut caches = SelectorCaches::default();
    let mut ctx = MatchingContext::new(
        MatchingMode::Normal,
        None,
        &mut caches,
        QuirksMode::NoQuirks,
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );
    let cx = CascadeInput { doc, state, ua: &ua, author: &author };
    cascade(&cx, &mut ctx, Document::ROOT, &root_style, &mut styles);
    StyleMap { styles }
}

struct CascadeInput<'a> {
    doc: &'a Document,
    state: &'a InteractionState,
    ua: &'a Stylesheet,
    author: &'a [Stylesheet],
}

fn cascade(
    cx: &CascadeInput,
    ctx: &mut MatchingContext<NileImpl>,
    id: NodeId,
    parent: &ComputedStyle,
    out: &mut Vec<ComputedStyle>,
) {
    let doc = cx.doc;
    let node = doc.node(id);
    let mut style = ComputedStyle::inherit(parent);

    if let NodeKind::Element { .. } = &node.kind {
        let el = ElementRef::new(doc, id, cx.state);

        // Origin order: UA first, then author sheets; within an origin, by
        // specificity, then source order.
        let mut decls: Vec<&Declaration> = Vec::new();
        for sheet in std::iter::once(cx.ua).chain(cx.author.iter()) {
            let mut matched: Vec<(u32, usize, &Rule)> = sheet
                .rules
                .iter()
                .enumerate()
                .filter_map(|(i, rule)| {
                    rule.selectors
                        .slice()
                        .iter()
                        .filter(|s| matches_selector(s, 0, None, &el, ctx))
                        .map(|s| s.specificity())
                        .max()
                        .map(|spec| (spec, i, rule))
                })
                .collect();
            matched.sort_by_key(|(spec, i, _)| (*spec, *i));
            decls.extend(matched.iter().flat_map(|(_, _, r)| r.declarations.iter()));
        }
        let inline = node.attr("style").map(parse::parse_declarations).unwrap_or_default();
        decls.extend(inline.iter());

        style.checked = doc.has_state(id, "checked");

        // font-size first, so `em` in everything else resolves against it.
        for d in decls.iter().filter(|d| d.name == "font-size") {
            apply(&mut style, d, parent.font_size);
        }
        for d in decls.iter().filter(|d| d.name != "font-size") {
            apply(&mut style, d, parent.font_size);
        }
    }

    out[id.0] = style.clone();
    for child in &node.children {
        cascade(cx, ctx, *child, &style, out);
    }
}

fn length(v: &Value, font_size: f32) -> Option<Length> {
    Some(match v {
        Value::Px(px) => Length::Px(*px),
        Value::Number(n) if *n == 0.0 => Length::Px(0.0),
        Value::Em(e) => Length::Px(e * font_size),
        Value::Rem(e) => Length::Px(e * ROOT_FONT_SIZE),
        Value::Percent(p) => Length::Percent(p / 100.0),
        Value::Ident(i) if i == "auto" => Length::Auto,
        _ => return None,
    })
}

fn px(v: &Value, font_size: f32) -> Option<f32> {
    match length(v, font_size)? {
        Length::Px(p) => Some(p),
        _ => None,
    }
}

/// Expand 1–4 value box shorthands (`margin`, `padding`, ...).
fn edges<T: Copy>(vals: &[T]) -> Option<Edges<T>> {
    Some(match *vals {
        [a] => Edges::all(a),
        [v, h] => Edges { top: v, right: h, bottom: v, left: h },
        [t, h, b] => Edges { top: t, right: h, bottom: b, left: h },
        [t, r, b, l] => Edges { top: t, right: r, bottom: b, left: l },
        _ => return None,
    })
}

fn align(v: &Value) -> Option<Align> {
    Some(match v.ident()? {
        "flex-start" | "start" => Align::Start,
        "flex-end" | "end" => Align::End,
        "center" => Align::Center,
        "stretch" => Align::Stretch,
        "space-between" => Align::SpaceBetween,
        "space-around" => Align::SpaceAround,
        "space-evenly" => Align::SpaceEvenly,
        "normal" => Align::Normal,
        _ => return None,
    })
}

fn apply(s: &mut ComputedStyle, d: &Declaration, parent_font_size: f32) {
    let fs = if d.name == "font-size" { parent_font_size } else { s.font_size };
    let v = &d.values;
    let first = v.first();
    let l = |val: Option<&Value>| val.and_then(|x| length(x, fs));
    let p = |val: Option<&Value>| val.and_then(|x| px(x, fs));

    match d.name.as_str() {
        "display" => {
            s.display = match first.and_then(Value::ident) {
                Some("block") => Display::Block,
                Some("inline") => Display::Inline,
                Some("inline-block") => Display::InlineBlock,
                Some("flex") | Some("inline-flex") => Display::Flex,
                Some("none") => Display::None,
                _ => return,
            }
        }
        "overflow" | "overflow-x" | "overflow-y" => {
            // Either axis being non-visible clips (we clip both).
            s.overflow_hidden = match first.and_then(Value::ident) {
                Some("hidden" | "scroll" | "auto" | "clip") => true,
                Some("visible") => false,
                _ => return,
            }
        }
        "appearance" | "-webkit-appearance" => {
            s.appearance = match first.and_then(Value::ident) {
                Some("checkbox") => Appearance::Checkbox,
                Some("radio") => Appearance::Radio,
                Some("none") | Some("auto") => Appearance::None,
                _ => return,
            }
        }
        "box-sizing" => {
            s.border_box = match first.and_then(Value::ident) {
                Some("border-box") => true,
                Some("content-box") => false,
                _ => return,
            }
        }
        "width" => s.width = l(first).unwrap_or(s.width),
        "height" => s.height = l(first).unwrap_or(s.height),
        "min-width" => s.min_width = l(first).unwrap_or(s.min_width),
        "min-height" => s.min_height = l(first).unwrap_or(s.min_height),
        "max-width" => s.max_width = l(first).unwrap_or(s.max_width),
        "max-height" => s.max_height = l(first).unwrap_or(s.max_height),
        "margin" => {
            let vals: Option<Vec<Length>> = v.iter().map(|x| length(x, fs)).collect();
            if let Some(e) = vals.as_deref().and_then(edges) {
                s.margin = e;
            }
        }
        "margin-top" => s.margin.top = l(first).unwrap_or(s.margin.top),
        "margin-right" => s.margin.right = l(first).unwrap_or(s.margin.right),
        "margin-bottom" => s.margin.bottom = l(first).unwrap_or(s.margin.bottom),
        "margin-left" => s.margin.left = l(first).unwrap_or(s.margin.left),
        "padding" => {
            let vals: Option<Vec<f32>> = v.iter().map(|x| px(x, fs)).collect();
            if let Some(e) = vals.as_deref().and_then(edges) {
                s.padding = e;
            }
        }
        "padding-top" => s.padding.top = p(first).unwrap_or(s.padding.top),
        "padding-right" => s.padding.right = p(first).unwrap_or(s.padding.right),
        "padding-bottom" => s.padding.bottom = p(first).unwrap_or(s.padding.bottom),
        "padding-left" => s.padding.left = p(first).unwrap_or(s.padding.left),
        "border-width" => {
            let vals: Option<Vec<f32>> = v.iter().map(|x| px(x, fs)).collect();
            if let Some(e) = vals.as_deref().and_then(edges) {
                s.border_width = e;
            }
        }
        "border-color" => s.border_color = first.and_then(Value::color).unwrap_or(s.border_color),
        // `border: 1px solid #ccc` — order-independent.
        "border" => {
            for x in v {
                if let Some(w) = px(x, fs) {
                    s.border_width = Edges::all(w);
                } else if let Some(c) = x.color() {
                    s.border_color = c;
                } else if x.ident() == Some("none") {
                    s.border_width = Edges::all(0.0);
                }
            }
        }
        "border-radius" => s.border_radius = p(first).unwrap_or(s.border_radius),
        "background" | "background-color" => {
            if let Some(c) = v.iter().find_map(Value::color) {
                s.background = Some(c);
            }
        }
        "opacity" => {
            if let Some(Value::Number(n)) = first {
                s.opacity = n.clamp(0.0, 1.0);
            }
        }
        "color" => s.color = first.and_then(Value::color).unwrap_or(s.color),
        "font-size" => s.font_size = p(first).unwrap_or(s.font_size),
        "font-weight" => {
            s.font_weight = match first {
                Some(Value::Number(n)) => *n,
                Some(x) if x.ident() == Some("bold") => 700.0,
                Some(x) if x.ident() == Some("normal") => 400.0,
                _ => return,
            }
        }
        "font-family" => {
            if let Some(name) = v.iter().find_map(|x| match x {
                Value::Ident(i) | Value::Str(i) => Some(i.clone()),
                _ => None,
            }) {
                s.font_family = name;
            }
        }
        "line-height" => {
            s.line_height = match first {
                Some(Value::Number(n)) => *n,
                Some(x) => match p(Some(x)) {
                    Some(px) if s.font_size > 0.0 => px / s.font_size,
                    _ => return,
                },
                None => return,
            }
        }
        "font-style" => {
            s.italic = match first.and_then(Value::ident) {
                Some("italic") | Some("oblique") => true,
                Some("normal") => false,
                _ => return,
            }
        }
        "text-decoration" | "text-decoration-line" => {
            s.underline = v.iter().any(|x| x.ident() == Some("underline"));
            s.line_through = v.iter().any(|x| x.ident() == Some("line-through"));
        }
        "text-align" => {
            s.text_align = match first.and_then(Value::ident) {
                Some("left") | Some("start") => TextAlign::Start,
                Some("center") => TextAlign::Center,
                Some("right") | Some("end") => TextAlign::End,
                Some("justify") => TextAlign::Justify,
                _ => return,
            }
        }
        "flex-direction" => {
            s.flex_direction = match first.and_then(Value::ident) {
                Some("row") => FlexDirection::Row,
                Some("column") => FlexDirection::Column,
                Some("row-reverse") => FlexDirection::RowReverse,
                Some("column-reverse") => FlexDirection::ColumnReverse,
                _ => return,
            }
        }
        "flex-wrap" => s.flex_wrap = first.and_then(Value::ident) == Some("wrap"),
        "flex-grow" => {
            if let Some(Value::Number(n)) = first {
                s.flex_grow = *n;
            }
        }
        "flex-shrink" => {
            if let Some(Value::Number(n)) = first {
                s.flex_shrink = *n;
            }
        }
        "flex-basis" => s.flex_basis = l(first).unwrap_or(s.flex_basis),
        // `flex: <grow> [<shrink>] [<basis>]`; a bare number means basis 0.
        "flex" => match v.as_slice() {
            [x] if x.ident() == Some("none") => {
                s.flex_grow = 0.0;
                s.flex_shrink = 0.0;
                s.flex_basis = Length::Auto;
            }
            [x] if x.ident() == Some("auto") => {
                s.flex_grow = 1.0;
                s.flex_shrink = 1.0;
                s.flex_basis = Length::Auto;
            }
            [Value::Number(g), rest @ ..] => {
                s.flex_grow = *g;
                s.flex_shrink = match rest.first() {
                    Some(Value::Number(sh)) => *sh,
                    _ => 1.0,
                };
                s.flex_basis = rest.iter().find_map(|x| length(x, fs)).unwrap_or(Length::Px(0.0));
            }
            _ => {}
        },
        "justify-content" => s.justify_content = first.and_then(align).unwrap_or(s.justify_content),
        "align-items" => s.align_items = first.and_then(align).unwrap_or(s.align_items),
        "gap" => s.gap = p(first).unwrap_or(s.gap),
        _ => {} // Unsupported properties are ignored, like a browser would.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_order_and_inheritance() {
        let doc = Document::parse(
            r#"<style>.card { color: #ff0000; padding: 4px 8px } #x { color: blue }</style>
               <div class="card" id="x" style="font-size: 20px"><p>t</p></div>"#,
        );
        let styles = compute_styles(&doc);
        let div = doc.find(|n| n.tag() == Some("div")).unwrap();
        let p = doc.find(|n| n.tag() == Some("p")).unwrap();
        let ds = styles.get(div);
        assert_eq!(ds.color, Color::rgb(0, 0, 255)); // id beats class
        assert_eq!(ds.padding.left, 8.0);
        assert_eq!(ds.font_size, 20.0);
        assert_eq!(styles.get(p).font_size, 20.0); // inherited
        assert_eq!(styles.get(p).margin.top, Length::Px(20.0)); // 1em of 20px
    }

    fn color_of(doc: &Document, styles: &StyleMap, id: &str) -> Color {
        let n = doc.find(|n| n.attr("id") == Some(id)).unwrap();
        styles.get(n).color
    }

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    #[test]
    fn combinators() {
        let doc = Document::parse(
            r#"<style>
                 .a p { color: red }
                 .a > .direct { color: blue }
                 h1 + p { color: red }
                 h1 ~ span { color: blue }
               </style>
               <div class="a"><section><p id="deep">x</p></section><p id="direct" class="direct">y</p></div>
               <h1>t</h1><p id="adj">z</p><b></b><span id="sib">w</span>"#,
        );
        let st = compute_styles(&doc);
        assert_eq!(color_of(&doc, &st, "deep"), RED);
        // `.a p` (0,1,1) beats `.a > .direct`? No: (0,2,0) > (0,1,1).
        assert_eq!(color_of(&doc, &st, "direct"), BLUE);
        assert_eq!(color_of(&doc, &st, "adj"), RED);
        assert_eq!(color_of(&doc, &st, "sib"), BLUE);
    }

    #[test]
    fn structural_and_attribute_selectors() {
        let doc = Document::parse(
            r#"<style>
                 li:first-child { color: red }
                 li:nth-child(2n) { color: blue }
                 li:not(.skip):last-child { color: red }
                 a[href^="https"] { color: blue }
                 input[disabled] { color: red }
               </style>
               <ul><li id="l1">1</li><li id="l2">2</li><li id="l3" class="skip">3</li><li id="l4">4</li><li id="l5">5</li></ul>
               <a id="sec" href="https://x">s</a><a id="ins" href="http://x">i</a><input id="in" disabled>"#,
        );
        let st = compute_styles(&doc);
        assert_eq!(color_of(&doc, &st, "l1"), RED);
        assert_eq!(color_of(&doc, &st, "l2"), BLUE);
        assert_eq!(color_of(&doc, &st, "l3"), Color::BLACK);
        assert_eq!(color_of(&doc, &st, "l4"), BLUE);
        assert_eq!(color_of(&doc, &st, "l5"), RED);
        assert_eq!(color_of(&doc, &st, "sec"), BLUE);
        assert_eq!(color_of(&doc, &st, "ins"), Color::rgb(0, 0, 238)); // UA link blue
        assert_eq!(color_of(&doc, &st, "in"), RED);
    }

    #[test]
    fn hover_follows_interaction_state() {
        let doc = Document::parse(
            r#"<style>.b { color: blue } .b:hover { color: red } .b:hover span { color: red }</style>
               <div class="b" id="btn"><span id="inner">x</span></div><div id="other"></div>"#,
        );
        let btn = doc.find(|n| n.attr("id") == Some("btn")).unwrap();
        let inner = doc.find(|n| n.attr("id") == Some("inner")).unwrap();

        let idle = compute_styles(&doc);
        assert_eq!(color_of(&doc, &idle, "btn"), BLUE);

        // Hovering a descendant hovers its ancestors too.
        let state = InteractionState { hover: Some(inner), ..Default::default() };
        let hovered = compute_styles_with(&doc, &state);
        assert_eq!(hovered.get(btn).color, RED);
        assert_eq!(color_of(&doc, &hovered, "inner"), RED);
    }

    #[test]
    fn computed_values_serialize_like_css() {
        let doc = Document::parse(
            r#"<style>p { color: #ff8000; padding: 4px 8px; font-size: 20px; margin-top: 0.5em; text-decoration: underline }</style><p id="p">x</p>"#,
        );
        let st = compute_styles(&doc);
        let s = st.get(doc.find(|n| n.tag() == Some("p")).unwrap());
        assert_eq!(s.css_value("color").as_deref(), Some("rgb(255, 128, 0)"));
        assert_eq!(s.css_value("padding-left").as_deref(), Some("8px"));
        assert_eq!(s.css_value("margin-top").as_deref(), Some("10px"));
        assert_eq!(s.css_value("line-height").as_deref(), Some("24px"));
        assert_eq!(s.css_value("text-decoration").as_deref(), Some("underline"));
        assert_eq!(s.css_value("background-color").as_deref(), Some("rgba(0, 0, 0, 0)"));
        assert_eq!(s.css_value("nonsense"), None);
    }

    #[test]
    fn state_flag_pseudo_classes() {
        let mut doc = Document::parse(
            r#"<style>input:checked { color: red } input:disabled { color: blue } li:not(.x):expanded { color: red }</style>
               <input id="a" disabled><input id="b"><input id="c" checked><ul><li id="d">x</li></ul>"#,
        );
        let id = |d: &Document, s: &str| d.find(|n| n.attr("id") == Some(s)).unwrap();
        let st = compute_styles(&doc);
        assert_eq!(color_of(&doc, &st, "a"), BLUE); // from the attribute
        assert_eq!(color_of(&doc, &st, "b"), Color::BLACK);
        assert_eq!(color_of(&doc, &st, "c"), RED);
        let (b, d) = (id(&doc, "b"), id(&doc, "d"));
        doc.set_state(b, "checked", true);
        doc.set_state(d, "expanded", true);
        let st = compute_styles(&doc);
        assert_eq!(color_of(&doc, &st, "b"), RED);
        assert_eq!(color_of(&doc, &st, "d"), RED);
    }

    #[test]
    fn query_selects_in_document_order() {
        let doc = Document::parse(r#"<ul id="u"><li class="a">1</li><li>2</li><li class="a">3</li></ul><p class="a">x</p>"#);
        let ul = doc.find(|n| n.tag() == Some("ul")).unwrap();
        let q = Query::parse("ul > .a, p.a").unwrap();
        let all = q.select_all(&doc, Document::ROOT);
        assert_eq!(all.len(), 3);
        assert_eq!(Query::parse("li.a").unwrap().select_all(&doc, ul).len(), 2);
        assert!(Query::parse("li:hover").unwrap().select_all(&doc, ul).is_empty());
        assert!(Query::parse("li::before").is_err());
        assert!(Query::parse("<<").is_err());
    }

    #[test]
    fn unsupported_selectors_drop_only_their_rule() {
        let doc = Document::parse(
            r#"<style>p::first-line { color: red } p:visited { color: red } p { color: blue }</style><p id="p">x</p>"#,
        );
        assert_eq!(color_of(&doc, &compute_styles(&doc), "p"), BLUE);
    }
}
