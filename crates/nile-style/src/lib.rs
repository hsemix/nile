//! Style resolution: parse CSS, cascade it over the DOM, produce computed styles.
//!
//! Scope:
//! - `<style>` blocks; selectors matched by Servo's `selectors` crate (see
//!   `selector.rs` for what's supported)
//! - inline `style="..."` attributes
//! - a small user-agent stylesheet
//! - inheritance of text properties

mod anim;
mod colors;
mod gradient;
mod grid;
mod parse;
mod selector;

use nile_dom::{Document, NodeId, NodeKind};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use selectors::context::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
    SelectorCaches,
};
use selectors::matching::matches_selector;
pub use anim::{AnimValue, AnimationSpec, Animator, Direction, Fill, Timing, TransitionSpec};
pub use grid::{area_lines, Breadth, GridAutoFlow, GridLine, GridPlace, Track, TrackItem, TrackRepeat};
pub use gradient::{TransformFn, ColorStop, Extent, Gradient, GradientKind, LinearDir, StopPos};
pub use parse::{Declaration, Rule, Stylesheet, Value};
use parse::Cx;
pub use selector::{ElementRef, InteractionState, NileImpl, PseudoClass, PseudoElement, Query};

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
    /// `calc()` mixing a length and a percentage: `px + pct * basis` (`pct` is a fraction).
    /// Layout resolves it once the containing block's size is known.
    Calc { px: f32, pct: f32 },
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

/// Border line style. `none`/`hidden` give the side a zero width whatever its
/// `border-width`; `double`, `groove` and the like draw solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
}

/// `background-size`. `None` in `Explicit` is `auto` for that axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum BgSize {
    #[default]
    Auto,
    Cover,
    Contain,
    Explicit(Option<StopPos>, Option<StopPos>),
}

/// One `box-shadow` layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
    pub inset: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Float {
    #[default]
    None,
    Left,
    Right,
}

/// Which floats a box moves below (`clear`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clear {
    #[default]
    None,
    Left,
    Right,
    Both,
}

/// Whether the user can select an element's text. Off unless the page asks for it,
/// as in desktop UIs (and Sciter, where it is `behavior: htmlarea`); inherited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UserSelect {
    #[default]
    None,
    Text,
    /// Selectable, and a click selects the whole run.
    All,
}

/// `position`. `Fixed` is laid out like `Absolute` (against the parent, scrolling with the page).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
}

impl Position {
    /// Taken out of the normal flow.
    pub fn out_of_flow(self) -> bool {
        matches!(self, Position::Absolute | Position::Fixed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Display {
    #[default]
    Block,
    Inline,
    /// A block container laid out as one atomic box inside an inline run.
    InlineBlock,
    Flex,
    Grid,
    /// `list-item`: a block with a marker (`<li>`).
    ListItem,
    /// `table` and its parts; layout turns a table into a grid.
    Table,
    TableRowGroup,
    TableRow,
    TableCell,
    TableCaption,
    None,
}

/// `overflow` on one axis. `Scroll` covers both `scroll` and `auto`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum OverflowKind {
    #[default]
    Visible,
    Hidden,
    Scroll,
}

/// Native-looking control drawn by the renderer (`appearance`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Appearance {
    #[default]
    None,
    Checkbox,
    Radio,
    /// A `<select>` drop-down: draws its chevron.
    Select,
    /// `<progress>`, `<meter>` and `<input type=range>`: a bar filled to `control_fraction`.
    Progress,
    Meter,
    Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FlexDirection {
    #[default]
    Row,
    Column,
    RowReverse,
    ColumnReverse,
}

/// `list-style-type`: what a list item's marker looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListStyleType {
    #[default]
    Disc,
    Circle,
    Square,
    Decimal,
    DecimalLeadingZero,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
    LowerGreek,
    /// `<summary>`'s triangle, pointing right (closed) or down (open).
    DisclosureClosed,
    DisclosureOpen,
    None,
}

impl ListStyleType {
    fn parse(name: &str) -> Option<ListStyleType> {
        Some(match name {
            "disc" => ListStyleType::Disc,
            "circle" => ListStyleType::Circle,
            "square" => ListStyleType::Square,
            "decimal" => ListStyleType::Decimal,
            "decimal-leading-zero" => ListStyleType::DecimalLeadingZero,
            "lower-alpha" | "lower-latin" => ListStyleType::LowerAlpha,
            "upper-alpha" | "upper-latin" => ListStyleType::UpperAlpha,
            "lower-roman" => ListStyleType::LowerRoman,
            "upper-roman" => ListStyleType::UpperRoman,
            "lower-greek" => ListStyleType::LowerGreek,
            "disclosure-closed" => ListStyleType::DisclosureClosed,
            "disclosure-open" => ListStyleType::DisclosureOpen,
            "none" => ListStyleType::None,
            _ => return None,
        })
    }
}

/// `white-space`: whether runs of spaces collapse, newlines break lines, and long lines wrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Nowrap,
    Pre,
    PreWrap,
    PreLine,
}

impl WhiteSpace {
    /// Lines may wrap at the box edge.
    pub fn wraps(self) -> bool {
        !matches!(self, WhiteSpace::Nowrap | WhiteSpace::Pre)
    }
    /// Runs of spaces and tabs are kept.
    pub fn keeps_spaces(self) -> bool {
        matches!(self, WhiteSpace::Pre | WhiteSpace::PreWrap)
    }
    /// A newline in the source is a line break.
    pub fn keeps_newlines(self) -> bool {
        !matches!(self, WhiteSpace::Normal | WhiteSpace::Nowrap)
    }
}

/// The mouse pointer shape over an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cursor {
    /// The platform picks: an I-beam over text and inputs, else the arrow.
    #[default]
    Auto,
    Default,
    Pointer,
    Text,
    Crosshair,
    Move,
    NotAllowed,
    Wait,
    Progress,
    Help,
    Grab,
    Grabbing,
    /// `col-resize`, `ew-resize`
    ResizeH,
    /// `row-resize`, `ns-resize`
    ResizeV,
    None,
}

/// `vertical-align`; only table cells act on it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Top,
    Middle,
    Bottom,
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
    pub position: Position,
    pub float: Float,
    pub clear: Clear,
    /// `z-index`; `None` is `auto`.
    pub z_index: Option<i32>,
    /// `top`, `right`, `bottom`, `left`.
    pub inset: Edges<Length>,
    pub padding: Edges<f32>,
    pub border_width: Edges<f32>,
    /// `box-sizing: border-box` (the CSS default is `content-box`).
    pub border_box: bool,
    pub appearance: Appearance,
    /// Non-visible overflow clips content to the padding box; `Scroll` also lets the
    /// wheel scroll it (`Hidden` can still be scrolled from script).
    pub overflow_x: OverflowKind,
    pub overflow_y: OverflowKind,
    /// The element's `checked` state flag (checkbox/radio marks), resolved at cascade time.
    pub checked: bool,
    /// How full a progress bar, meter or slider is (0..=1); `None` for an indeterminate progress.
    pub control_fraction: Option<f32>,
    /// A `<meter>` outside its low/high range, drawn in a warning color.
    pub meter_warn: bool,

    // Flex
    pub flex_direction: FlexDirection,
    pub flex_wrap: bool,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Length,
    pub justify_content: Align,
    pub align_items: Align,
    pub row_gap: f32,
    pub column_gap: f32,
    /// `border-spacing` (horizontal, vertical) and `border-collapse: collapse`; inherited.
    pub border_spacing: (f32, f32),
    pub border_collapse: bool,
    /// `vertical-align` for table cells.
    pub vertical_align: VerticalAlign,
    /// `cursor`; inherited.
    pub cursor: Cursor,
    pub align_self: Align,
    pub align_content: Align,

    // Grid
    pub justify_items: Align,
    pub justify_self: Align,
    pub grid_template_columns: Vec<TrackItem>,
    pub grid_template_rows: Vec<TrackItem>,
    pub grid_auto_columns: Vec<Track>,
    pub grid_auto_rows: Vec<Track>,
    pub grid_auto_flow: GridAutoFlow,
    pub grid_column: GridPlace,
    pub grid_row: GridPlace,
    /// `grid-template-areas`: the area name of each cell, by row.
    pub grid_areas: Vec<Vec<String>>,
    /// `grid-area: <name>` of an item.
    pub grid_area_name: Option<String>,

    // Paint
    pub background: Option<Color>,
    /// The first gradient of `background` / `background-image`, painted over the color.
    pub background_image: Option<Gradient>,
    /// `background-image: url(...)`, as written.
    pub background_url: Option<String>,
    pub background_size: BgSize,
    /// Where the image sits in the box: (x, y), a fraction of the free space or px.
    pub background_position: (StopPos, StopPos),
    /// Whether the image tiles horizontally, vertically.
    pub background_repeat: (bool, bool),
    /// Resolved per-side border colors (top, right, bottom, left).
    pub border_color: Edges<Color>,
    /// What the author set: `None` follows `color` (CSS `currentColor`, the initial value).
    pub border_color_set: Edges<Option<Color>>,
    pub border_style: Edges<BorderStyle>,
    pub border_radius: f32,
    pub opacity: f32,
    pub transform: Vec<TransformFn>,
    pub transitions: Vec<TransitionSpec>,
    pub animations: Vec<AnimationSpec>,
    /// Origin of `transform` within the border box.
    pub transform_origin: (StopPos, StopPos),
    /// Outer shadows, first = topmost. `inset` ones are parsed but not painted yet.
    pub box_shadow: Vec<BoxShadow>,

    // Text (inherited)
    pub color: Color,
    pub font_size: f32,
    pub font_weight: f32,
    pub font_family: String,
    pub line_height: f32,
    pub text_align: TextAlign,
    /// `text-overflow: ellipsis` (not inherited).
    pub text_ellipsis: bool,
    /// `list-style-type`; inherited.
    pub list_style_type: ListStyleType,
    /// `white-space`; inherited.
    pub white_space: WhiteSpace,
    pub italic: bool,
    pub underline: bool,
    pub line_through: bool,
    pub user_select: UserSelect,
    /// `text-selection-background-color` (Sciter); inherited.
    pub selection_bg: Option<Color>,
    pub text_transform: TextTransform,
    /// Extra space after each character, in px.
    pub letter_spacing: f32,
    /// Custom properties (`--name`), inherited. Values are source text, resolved on use.
    pub vars: Rc<BTreeMap<String, String>>,
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
            position: Position::Static,
            float: Float::None,
            clear: Clear::None,
            z_index: None,
            inset: Edges::all(Length::Auto),
            padding: Edges::all(0.0),
            border_width: Edges::all(0.0),
            border_box: false,
            appearance: Appearance::None,
            overflow_x: OverflowKind::Visible,
            overflow_y: OverflowKind::Visible,
            checked: false,
            control_fraction: None,
            meter_warn: false,
            flex_direction: FlexDirection::Row,
            flex_wrap: false,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Length::Auto,
            justify_content: Align::Normal,
            align_items: Align::Normal,
            row_gap: 0.0,
            column_gap: 0.0,
            border_spacing: (0.0, 0.0),
            border_collapse: false,
            vertical_align: VerticalAlign::Baseline,
            cursor: Cursor::Auto,
            align_self: Align::Normal,
            align_content: Align::Normal,
            justify_items: Align::Normal,
            justify_self: Align::Normal,
            grid_template_columns: Vec::new(),
            grid_template_rows: Vec::new(),
            grid_auto_columns: Vec::new(),
            grid_auto_rows: Vec::new(),
            grid_auto_flow: GridAutoFlow::default(),
            grid_column: GridPlace::default(),
            grid_row: GridPlace::default(),
            grid_areas: Vec::new(),
            grid_area_name: None,
            background: None,
            background_image: None,
            background_url: None,
            background_size: BgSize::Auto,
            background_position: (StopPos::Fraction(0.0), StopPos::Fraction(0.0)),
            background_repeat: (true, true),
            border_color: Edges::all(Color::BLACK),
            border_color_set: Edges::all(None),
            border_style: Edges::all(BorderStyle::None),
            border_radius: 0.0,
            opacity: 1.0,
            transform: Vec::new(),
            transitions: Vec::new(),
            animations: Vec::new(),
            transform_origin: (StopPos::Fraction(0.5), StopPos::Fraction(0.5)),
            box_shadow: Vec::new(),
            color: Color::BLACK,
            font_size: 16.0,
            font_weight: 400.0,
            font_family: "sans-serif".into(),
            line_height: 1.2,
            text_align: TextAlign::Start,
            white_space: WhiteSpace::Normal,
            list_style_type: ListStyleType::Disc,
            text_ellipsis: false,
            italic: false,
            underline: false,
            line_through: false,
            user_select: UserSelect::None,
            selection_bg: None,
            text_transform: TextTransform::None,
            letter_spacing: 0.0,
            vars: Rc::default(),
        }
    }
}

impl ComputedStyle {
    /// True if content is clipped (and so can be scrolled) on either axis.
    pub fn clips(&self) -> bool {
        self.overflow_x != OverflowKind::Visible || self.overflow_y != OverflowKind::Visible
    }

    /// The CSS text of a property's computed value, as `getComputedStyle`
    /// reports it. `None` for properties we don't track. Sizes are in px;
    /// `width`/`height` report `auto` here (the used size comes from layout).
    pub fn css_value(&self, prop: &str) -> Option<String> {
        let px = |v: f32| format!("{}px", (v * 100.0).round() / 100.0);
        let len = |l: Length| match l {
            Length::Auto => "auto".to_string(),
            Length::Px(v) => px(v),
            Length::Percent(p) => format!("{}%", (p * 10000.0).round() / 100.0),
            Length::Calc { px: v, pct } => format!("calc({}% + {})", (pct * 10000.0).round() / 100.0, px(v)),
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
                Display::Grid => "grid",
                Display::ListItem => "list-item",
                Display::Table => "table",
                Display::TableRowGroup => "table-row-group",
                Display::TableRow => "table-row",
                Display::TableCell => "table-cell",
                Display::TableCaption => "table-caption",
                Display::None => "none",
            }
            .into(),
            "overflow" | "overflow-x" | "overflow-y" => {
                let name = |k: OverflowKind| match k {
                    OverflowKind::Visible => "visible",
                    OverflowKind::Hidden => "hidden",
                    OverflowKind::Scroll => "auto",
                };
                match prop {
                    "overflow-x" => name(self.overflow_x).into(),
                    "overflow-y" => name(self.overflow_y).into(),
                    _ if self.overflow_x == self.overflow_y => name(self.overflow_x).into(),
                    _ => format!("{} {}", name(self.overflow_x), name(self.overflow_y)),
                }
            }
            "appearance" => match self.appearance {
                Appearance::None => "none",
                Appearance::Checkbox => "checkbox",
                Appearance::Radio => "radio",
                Appearance::Select => "select",
                Appearance::Progress => "progress",
                Appearance::Meter => "meter",
                Appearance::Range => "range",
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
            "border-color" | "border-top-color" => color(self.border_color.top),
            "border-right-color" => color(self.border_color.right),
            "border-bottom-color" => color(self.border_color.bottom),
            "border-left-color" => color(self.border_color.left),
            "border-radius" => px(self.border_radius),
            "background-color" => color(self.background.unwrap_or(Color::TRANSPARENT)),
            "color" => color(self.color),
            "opacity" => self.opacity.to_string(),
            "font-size" => px(self.font_size),
            "font-weight" => (self.font_weight as i32).to_string(),
            "font-family" => self.font_family.clone(),
            "white-space" => match self.white_space {
                WhiteSpace::Normal => "normal",
                WhiteSpace::Nowrap => "nowrap",
                WhiteSpace::Pre => "pre",
                WhiteSpace::PreWrap => "pre-wrap",
                WhiteSpace::PreLine => "pre-line",
            }
            .into(),
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
            "gap" | "row-gap" => px(self.row_gap),
            "column-gap" => px(self.column_gap),
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
            white_space: parent.white_space,
            list_style_type: parent.list_style_type,
            italic: parent.italic,
            // TODO: decorations aren't really inherited in CSS; they propagate
            // to inline descendants, which this approximates.
            underline: parent.underline,
            line_through: parent.line_through,
            text_transform: parent.text_transform,
            user_select: parent.user_select,
            border_spacing: parent.border_spacing,
            border_collapse: parent.border_collapse,
            cursor: parent.cursor,
            selection_bg: parent.selection_bg,
            letter_spacing: parent.letter_spacing,
            vars: parent.vars.clone(),
            ..Default::default()
        }
    }
}

pub(crate) const ROOT_FONT_SIZE: f32 = 16.0;

const UA_STYLESHEET: &str = r#"
head, script, style, title, meta, link, option, optgroup { display: none }
img { display: inline-block }
[selectable] { user-select: text }
[contenteditable]:not([contenteditable=false]) { user-select: text; cursor: text }
html, body, div, p, h1, h2, h3, section, header, footer, main, nav, ul { display: block }
li { display: list-item }
ul, ol, menu { margin: 1em 0; padding-left: 40px }
ul, menu { list-style-type: disc }
ol { list-style-type: decimal }
ul ul, ol ul, ul ol, ol ol { margin: 0 }
ul ul, ol ul { list-style-type: circle }
ul ul ul, ol ul ul, ul ol ul, ol ol ul { list-style-type: square }
table { display: table; border-spacing: 2px; border-collapse: separate; box-sizing: border-box }
thead, tbody, tfoot { display: table-row-group }
tr { display: table-row }
td, th { display: table-cell; padding: 1px; vertical-align: middle }
th { font-weight: bold; text-align: center }
caption { display: table-caption; text-align: center }
col, colgroup { display: none }
article, aside, address, blockquote, figure, figcaption, details, summary, fieldset, legend, dl, dt, dd, ol, pre,
  h4, h5, h6, hgroup, menu { display: block }
span, a, b, strong, em, i, u, s, del, br, label, code, kbd, samp, var, tt, small, big, sub, sup, mark, abbr, cite, q,
  dfn, time, ins, font, bdi, bdo, data, output { display: inline }
code, kbd, samp, tt, pre { font-family: monospace }
small, sub, sup { font-size: 0.83em }
big { font-size: 1.17em }
mark { background: #ffff00; color: #000000 }
ins { text-decoration: underline }
cite, dfn, var { font-style: italic }
h4 { font-weight: bold; margin: 1.33em 0 }
h5 { font-size: 0.83em; font-weight: bold; margin: 1.67em 0 }
h6 { font-size: 0.67em; font-weight: bold; margin: 2.33em 0 }
blockquote { margin: 1em 40px }
dd { margin-left: 40px }
pre { margin: 1em 0; white-space: pre }
hr { display: block; height: 0; margin: 8px 0; border-top: 1px solid #9a9a9a; border-bottom: 1px solid #e5e5e5 }
fieldset { margin: 0 2px; padding: 0.35em 0.75em 0.625em; border: 1px solid #c0c0c0 }
legend { padding: 0 2px }
summary { display: list-item; list-style-type: disclosure-closed; padding-left: 1.4em; cursor: pointer }
details[open] > summary { list-style-type: disclosure-open }
details:not([open]) > :not(summary) { display: none }
textarea { white-space: pre-wrap }
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
a[href] { cursor: pointer }
input, textarea { cursor: text }
button, select, input[type=checkbox], input[type=radio], input[type=button], input[type=submit] { cursor: default }
:disabled { cursor: not-allowed }
button { text-align: center }
button, input { display: inline-block; font-size: 13px; color: #000000; padding: 3px 8px;
  border: 1px solid #767676; border-radius: 3px; background: #efefef }
input { background: #ffffff; width: 160px; min-height: 16px; margin: 2px; overflow: hidden }
textarea { display: inline-block; font-size: 13px; color: #000000; padding: 3px 4px; border: 1px solid #767676;
  border-radius: 3px; background: #ffffff; width: 200px; height: 60px; margin: 2px; overflow: auto }
select { display: inline-block; font-size: 13px; color: #000000; padding: 3px 26px 3px 8px; border: 1px solid #767676;
  border-radius: 3px; background: #ffffff; min-height: 16px; margin: 2px; overflow: hidden; appearance: select }
select[multiple], select[size]:not([size="1"]) { appearance: none; padding: 1px 0; min-width: 60px }
input:focus, textarea:focus, select:focus { border-color: #1d4ed8 }
button:disabled, input:disabled, textarea:disabled, select:disabled { color: #8a8a8a; background: #f0f0f0; border-color: #c4c4c4 }
input[type=checkbox], input[type=radio] { width: 13px; height: 13px; min-height: 0; padding: 0;
  margin: 3px 4px; background: #ffffff; appearance: checkbox }
input[type=radio] { border-radius: 7px; appearance: radio }
progress { display: inline-block; width: 160px; height: 16px; margin: 2px; border: 0; padding: 0; background: transparent; appearance: progress }
meter { display: inline-block; width: 80px; height: 16px; margin: 2px; border: 0; padding: 0; background: transparent; appearance: meter }
input[type=range] { width: 129px; height: 16px; min-height: 0; margin: 2px; padding: 0; border: 0; background: transparent; appearance: range }
input[type=checkbox]:checked, input[type=radio]:checked { background: #1d4ed8; border-color: #1d4ed8; color: #ffffff }
input[type=button], input[type=submit], input[type=reset] { width: auto; background: #efefef }
"#;

/// Computed styles for every node, indexed by `NodeId.0`.
#[derive(Clone)]
pub struct StyleMap {
    pub styles: Vec<ComputedStyle>,
    /// Styles depend on the viewport size (`@media`, `vw`, ...): restyle when it changes.
    pub uses_viewport: bool,
    /// Which pointer/focus states some rule's selector mentions. A change in a state nothing
    /// mentions can't change any style.
    pub state_deps: StateDeps,
    pseudo: HashMap<(NodeId, PseudoElement), NodeId>,
    origins: HashMap<NodeId, NodeId>,
    contents: HashMap<NodeId, String>,
    /// `@keyframes` by name.
    pub(crate) keyframes: HashMap<String, Vec<parse::Frame>>,
}

/// Which of `:hover`, `:active` and `:focus` appear in the stylesheets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateDeps {
    pub hover: bool,
    pub active: bool,
    pub focus: bool,
}

impl StateDeps {
    fn of<'a>(sheets: impl Iterator<Item = &'a Stylesheet>) -> StateDeps {
        use cssparser::ToCss;
        let mut d = StateDeps::default();
        for sheet in sheets {
            for rule in &sheet.rules {
                for sel in rule.selectors.slice() {
                    let css = sel.to_css_string();
                    d.hover |= css.contains(":hover");
                    d.active |= css.contains(":active");
                    d.focus |= css.contains(":focus");
                }
            }
        }
        d
    }

    /// Whether a change from `old` to `new` interaction state could restyle anything.
    pub fn affected_by(&self, old: &InteractionState, new: &InteractionState) -> bool {
        (self.hover && old.hover != new.hover) || (self.active && old.active != new.active) || (self.focus && old.focus != new.focus)
    }
}

/// `::before`/`::after` boxes found while styling. Each gets an id past the end of the
/// document's nodes, with a style in [`StyleMap::styles`] like any element.
#[derive(Default)]
struct Generated {
    pseudo: HashMap<(NodeId, PseudoElement), NodeId>,
    origins: HashMap<NodeId, NodeId>,
    contents: HashMap<NodeId, String>,
}

impl ComputedStyle {
    /// Whether `other` differs from `self` only in properties that change how a box is painted,
    /// not where it goes or how it is shaped: colors, shadows, radius, backgrounds, transforms
    /// and opacity. Laid-out boxes stay valid across such a change (see `LayoutTree::refresh_paint`).
    pub fn same_layout(&self, other: &ComputedStyle) -> bool {
        self.same_layout_with(other, false)
    }

    /// [`same_layout`](Self::same_layout), optionally also treating the text `color` as a paint
    /// property (the renderer reads it from the styles, not from the shaped text).
    pub fn same_layout_with(&self, other: &ComputedStyle, color_is_paint: bool) -> bool {
        let mut a = self.clone();
        // Inline boxes' backgrounds and borders are drawn from copies baked into their text run.
        if self.display != Display::Inline || other.display != Display::Inline {
            a.background = other.background;
            a.background_image = other.background_image.clone();
            a.border_color = other.border_color;
            a.border_color_set = other.border_color_set;
            a.border_style = other.border_style;
            a.border_radius = other.border_radius;
        }
        if color_is_paint {
            a.color = other.color;
        }
        a.background_size = other.background_size;
        a.background_position = other.background_position;
        a.background_repeat = other.background_repeat;
        a.box_shadow = other.box_shadow.clone();
        a.selection_bg = other.selection_bg;
        a.transitions = other.transitions.clone();
        a.animations = other.animations.clone();
        a.transform_origin = other.transform_origin;
        a.cursor = other.cursor;
        // Whether a box is faded or transformed decides its paint order; how much doesn't.
        if (a.opacity < 1.0) == (other.opacity < 1.0) {
            a.opacity = other.opacity;
        }
        if a.transform.is_empty() == other.transform.is_empty() {
            a.transform = other.transform.clone();
        }
        a == *other
    }
}

impl StyleMap {
    /// Whether every box would lay out as it did under `other`: same nodes and generated
    /// boxes, and only paint-level differences in the styles.
    pub fn same_layout(&self, other: &StyleMap) -> bool {
        // Text color is read at paint time, except for generated (`::before`/`::after`) text,
        // whose color the renderer can't tell apart from its element's.
        let color_is_paint = !self.has_generated() && !other.has_generated();
        self.styles.len() == other.styles.len()
            && self.pseudo == other.pseudo
            && self.contents == other.contents
            && self.styles.iter().zip(&other.styles).all(|(a, b)| a.same_layout_with(b, color_is_paint))
    }

    /// The page has `::before`/`::after` boxes.
    pub fn has_generated(&self) -> bool {
        !self.pseudo.is_empty()
    }

    /// Some element has a `transition` or `animation`.
    pub fn animates(&self) -> bool {
        self.styles.iter().any(|s| !s.transitions.is_empty() || !s.animations.is_empty())
    }

    /// The generated `::before` / `::after` box of `element`, if it has one.
    pub fn pseudo(&self, element: NodeId, kind: PseudoElement) -> Option<NodeId> {
        self.pseudo.get(&(element, kind)).copied()
    }

    /// True for ids of generated boxes (they have no node in the document).
    pub fn is_pseudo(&self, id: NodeId) -> bool {
        self.origins.contains_key(&id)
    }

    /// The element a generated box belongs to; any other id maps to itself.
    pub fn origin(&self, id: NodeId) -> NodeId {
        self.origins.get(&id).copied().unwrap_or(id)
    }

    /// The text a generated box contains.
    pub fn content(&self, id: NodeId) -> &str {
        self.contents.get(&id).map_or("", String::as_str)
    }

    /// Generated ids and the elements they belong to.
    pub fn origins(&self) -> &HashMap<NodeId, NodeId> {
        &self.origins
    }

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
    compute_styles_in(doc, state, (800.0, 600.0))
}

/// Like [`compute_styles_with`], with `@media` queries and `vw`/`vh` units resolved
/// against a `viewport` of the given size in px.
pub fn compute_styles_in(doc: &Document, state: &InteractionState, viewport: (f32, f32)) -> StyleMap {
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
            Stylesheet::parse_with(&css, viewport)
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
    let uses_viewport = Cell::new(author.iter().any(|s| s.uses_viewport));
    let mut keyframes: HashMap<String, Vec<parse::Frame>> = HashMap::new();
    for sheet in &author {
        for k in &sheet.keyframes {
            keyframes.insert(k.name.clone(), k.frames.clone());
        }
    }
    let has_pseudo = author.iter().any(|s| s.rules.iter().any(|r| r.selectors.slice().iter().any(|sel| sel.has_pseudo_element())));
    let state_deps = StateDeps::of(author.iter().chain(std::iter::once(&ua)));
    let cx = CascadeInput { doc, state, ua: &ua, author: &author, viewport, uses_viewport: &uses_viewport, has_pseudo };
    let mut generated = Generated::default();
    cascade(&cx, &mut ctx, Document::ROOT, &root_style, &mut styles, &mut generated);
    StyleMap {
        styles,
        uses_viewport: uses_viewport.get(),
        state_deps,
        pseudo: generated.pseudo,
        origins: generated.origins,
        contents: generated.contents,
        keyframes,
    }
}

struct CascadeInput<'a> {
    doc: &'a Document,
    state: &'a InteractionState,
    ua: &'a Stylesheet,
    author: &'a [Stylesheet],
    viewport: (f32, f32),
    uses_viewport: &'a Cell<bool>,
    /// Some stylesheet has a `::before`/`::after` rule.
    has_pseudo: bool,
}

/// Declarations from `sheets` matching `el`, in cascade order (specificity, then source
/// order, per origin). With `pseudo`, the selectors that end in that pseudo-element
/// are matched instead of the element's own rules.
fn matching_decls<'a>(
    cx: &'a CascadeInput,
    el: &ElementRef,
    ctx: &mut MatchingContext<NileImpl>,
    pseudo: bool,
) -> Vec<&'a Declaration> {
    let mut decls = Vec::new();
    for sheet in std::iter::once(cx.ua).chain(cx.author.iter()) {
        let mut matched: Vec<(u32, usize, &Rule)> = sheet
            .rules
            .iter()
            .enumerate()
            .filter_map(|(i, rule)| {
                rule.selectors
                    .slice()
                    .iter()
                    .filter(|s| s.has_pseudo_element() == pseudo && matches_selector(s, 0, None, el, ctx))
                    .map(|s| s.specificity())
                    .max()
                    .map(|spec| (spec, i, rule))
            })
            .collect();
        matched.sort_by_key(|(spec, i, _)| (*spec, *i));
        decls.extend(matched.iter().flat_map(|(_, _, r)| r.declarations.iter()));
    }
    decls
}

/// Applies `decls` to `style` (already inheriting from its parent): `!important` last,
/// custom properties and `var()` resolved, borders finalised. Returns the declarations
/// as applied, so callers can read properties that aren't part of `ComputedStyle`.
fn apply_declarations(
    style: &mut ComputedStyle,
    mut decls: Vec<&Declaration>,
    parent_font_size: f32,
    pcx: &Cx,
) -> Vec<Declaration> {
    // `!important` declarations win over normal ones from any origin, keeping
    // their relative order (the sort is stable).
    decls.sort_by_key(|d| d.important);

    // Custom properties first, in order, so `var()` sees the final values.
    if decls.iter().any(|d| d.name.starts_with("--")) {
        let mut vars = (*style.vars).clone();
        for d in decls.iter().filter(|d| d.name.starts_with("--")) {
            match substitute(&d.raw, &vars, 0) {
                Some(v) => vars.insert(d.name.clone(), v),
                None => vars.remove(&d.name),
            };
        }
        style.vars = Rc::new(vars);
    }

    // font-size first, so `em` in everything else resolves against it.
    let vars = style.vars.clone();
    let resolved: Vec<Declaration> = decls
        .iter()
        .filter(|d| !d.name.starts_with("--"))
        .filter_map(|d| {
            if d.raw.is_empty() {
                return Some((*d).clone());
            }
            // A `var()` that can't be resolved makes the declaration invalid.
            let text = substitute(&d.raw, &vars, 0)?;
            Some(Declaration { values: parse::parse_values(&text, pcx), raw: String::new(), ..(*d).clone() })
        })
        .collect();
    for d in resolved.iter().filter(|d| d.name == "font-size") {
        apply(style, d, parent_font_size);
    }
    for d in resolved.iter().filter(|d| d.name != "font-size") {
        apply(style, d, parent_font_size);
    }
    for i in 0..4 {
        if *side(&mut style.border_style, i) == BorderStyle::None {
            *side(&mut style.border_width, i) = 0.0;
        }
    }
    let own = style.color;
    let set = style.border_color_set;
    let pick = |c: Option<Color>| c.unwrap_or(own);
    style.border_color = Edges { top: pick(set.top), right: pick(set.right), bottom: pick(set.bottom), left: pick(set.left) };
    resolved
}

/// The text a `content` value generates; `None` for `none`/`normal` or nothing usable.
fn content_text(decls: &[Declaration], doc: &Document, id: NodeId) -> Option<String> {
    let values = &decls.iter().rev().find(|d| d.name == "content")?.values;
    if values.is_empty() || matches!(values.first().and_then(Value::ident), Some("none" | "normal")) {
        return None;
    }
    let mut out = String::new();
    let mut any = false;
    for v in values {
        match v {
            Value::Str(t) => {
                out.push_str(t);
                any = true;
            }
            Value::Attr(name) => {
                out.push_str(doc.node(id).attr(name).unwrap_or(""));
                any = true;
            }
            Value::Ident(i) if i == "open-quote" => {
                out.push('\u{201c}');
                any = true;
            }
            Value::Ident(i) if i == "close-quote" => {
                out.push('\u{201d}');
                any = true;
            }
            _ => {}
        }
    }
    any.then_some(out)
}

/// Style a `::before`/`::after` of `id`, if some rule gives it `content`. Its style
/// goes at the end of `out`, and the new id is returned.
fn pseudo_style(
    cx: &CascadeInput,
    id: NodeId,
    element: &ComputedStyle,
    kind: PseudoElement,
    out: &mut Vec<ComputedStyle>,
    extra: &mut Generated,
) {
    let el = ElementRef::new(cx.doc, id, cx.state);
    let mut caches = SelectorCaches::default();
    let mut ctx = MatchingContext::new(
        MatchingMode::ForStatelessPseudoElement,
        None,
        &mut caches,
        QuirksMode::NoQuirks,
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );
    let wanted = move |p: &PseudoElement| *p == kind;
    ctx.pseudo_element_matching_fn = Some(&wanted);
    let decls = matching_decls(cx, &el, &mut ctx, true);
    if decls.is_empty() {
        return;
    }
    let mut style = ComputedStyle::inherit(element);
    style.display = Display::Inline;
    let pcx = Cx { viewport: cx.viewport, uses_viewport: cx.uses_viewport };
    let applied = apply_declarations(&mut style, decls, element.font_size, &pcx);
    let Some(content) = content_text(&applied, cx.doc, id) else { return };
    let new = NodeId(out.len());
    out.push(style);
    extra.pseudo.insert((id, kind), new);
    extra.origins.insert(new, id);
    extra.contents.insert(new, content);
}

/// The fill of a `<progress>`, `<meter>` or `<input type=range>`, and whether a meter is out of
/// its comfortable range.
pub fn control_fraction(doc: &Document, id: NodeId) -> (Option<f32>, bool) {
    let n = doc.node(id);
    let num = |name: &str| n.attr(name).and_then(|v| v.trim().parse::<f32>().ok());
    match n.tag() {
        Some("progress") => {
            let max = num("max").filter(|m| *m > 0.0).unwrap_or(1.0);
            (num("value").map(|v| (v / max).clamp(0.0, 1.0)), false)
        }
        Some("meter") => {
            let (min, max) = (num("min").unwrap_or(0.0), num("max").unwrap_or(1.0));
            let v = num("value").unwrap_or(0.0).clamp(min, max.max(min));
            let (low, high) = (num("low").unwrap_or(min), num("high").unwrap_or(max));
            let frac = if max > min { (v - min) / (max - min) } else { 0.0 };
            (Some(frac), v < low || v > high)
        }
        Some("input") if n.attr("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("range")) => {
            let (min, max) = (num("min").unwrap_or(0.0), num("max").unwrap_or(100.0));
            let v = num("value").unwrap_or((min + max) / 2.0).clamp(min, max.max(min));
            (Some(if max > min { (v - min) / (max - min) } else { 0.0 }), false)
        }
        _ => (None, false),
    }
}

fn cascade(
    cx: &CascadeInput,
    ctx: &mut MatchingContext<NileImpl>,
    id: NodeId,
    parent: &ComputedStyle,
    out: &mut Vec<ComputedStyle>,
    extra: &mut Generated,
) {
    let doc = cx.doc;
    let node = doc.node(id);
    let mut style = ComputedStyle::inherit(parent);

    if let NodeKind::Element { .. } = &node.kind {
        let el = ElementRef::new(doc, id, cx.state);
        let mut decls = matching_decls(cx, &el, ctx, false);
        let pcx = Cx { viewport: cx.viewport, uses_viewport: cx.uses_viewport };
        let inline = node.attr("style").map(|css| parse::parse_declarations(css, &pcx)).unwrap_or_default();
        decls.extend(inline.iter());
        style.checked = doc.has_state(id, "checked");
        (style.control_fraction, style.meter_warn) = control_fraction(doc, id);
        apply_declarations(&mut style, decls, parent.font_size, &pcx);
    }

    out[id.0] = style.clone();
    if cx.has_pseudo && matches!(node.kind, NodeKind::Element { .. }) {
        pseudo_style(cx, id, &style, PseudoElement::Before, out, extra);
        pseudo_style(cx, id, &style, PseudoElement::After, out, extra);
    }
    for child in &node.children {
        cascade(cx, ctx, *child, &style, out, extra);
    }
}

/// Replaces every `var(--name[, fallback])` in `text`. `None` if a variable is
/// undefined without a fallback, or substitution nests too deeply (a cycle).
fn substitute(text: &str, vars: &BTreeMap<String, String>, depth: u32) -> Option<String> {
    if depth > 16 {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("var(") {
        out.push_str(&rest[..at]);
        let body = &rest[at + 4..];
        let mut level = 1;
        let mut end = None;
        let mut comma = None;
        for (i, c) in body.char_indices() {
            match c {
                '(' => level += 1,
                ')' => {
                    level -= 1;
                    if level == 0 {
                        end = Some(i);
                        break;
                    }
                }
                ',' if level == 1 && comma.is_none() => comma = Some(i),
                _ => {}
            }
        }
        let end = end?;
        let name = body[..comma.unwrap_or(end)].trim();
        let value = match vars.get(name) {
            Some(v) => substitute(v, vars, depth + 1)?,
            None => substitute(body[comma? + 1..end].trim(), vars, depth + 1)?,
        };
        out.push_str(&value);
        rest = &body[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

fn stop_len(v: &Value, fs: f32) -> Option<StopPos> {
    match length(v, fs)? {
        Length::Px(p) => Some(StopPos::Px(p)),
        Length::Percent(p) => Some(StopPos::Fraction(p)),
        _ => None,
    }
}

/// `background-position`: one or two keywords/lengths.
fn bg_position(v: &[Value], fs: f32) -> Option<(StopPos, StopPos)> {
    let half = StopPos::Fraction(0.5);
    let (mut x, mut y) = (None, None);
    let mut free = Vec::new();
    for item in v {
        match item.ident() {
            Some("left") => x = Some(StopPos::Fraction(0.0)),
            Some("right") => x = Some(StopPos::Fraction(1.0)),
            Some("top") => y = Some(StopPos::Fraction(0.0)),
            Some("bottom") => y = Some(StopPos::Fraction(1.0)),
            Some("center") => free.push(half),
            Some(_) => return None,
            None => free.push(stop_len(item, fs)?),
        }
    }
    if v.is_empty() || v.len() > 2 {
        return None;
    }
    let mut free = free.into_iter();
    let x = x.or_else(|| free.next()).unwrap_or(half);
    let y = y.or_else(|| free.next()).unwrap_or(half);
    Some((x, y))
}

fn bg_size(v: &[Value], fs: f32) -> Option<BgSize> {
    let part = |x: &Value| -> Option<Option<StopPos>> {
        if x.ident() == Some("auto") {
            Some(None)
        } else {
            stop_len(x, fs).map(Some)
        }
    };
    match v {
        [x] if x.ident() == Some("cover") => Some(BgSize::Cover),
        [x] if x.ident() == Some("contain") => Some(BgSize::Contain),
        [w] => Some(BgSize::Explicit(part(w)?, None)),
        [w, h] => Some(BgSize::Explicit(part(w)?, part(h)?)),
        _ => None,
    }
}

fn bg_repeat(v: &[Value]) -> Option<(bool, bool)> {
    let words: Vec<&str> = v.iter().filter_map(Value::ident).filter(|i| i.contains("repeat")).collect();
    match words.as_slice() {
        ["repeat"] => Some((true, true)),
        ["no-repeat"] => Some((false, false)),
        ["repeat-x"] => Some((true, false)),
        ["repeat-y"] => Some((false, true)),
        [x, y] => Some((*x != "no-repeat", *y != "no-repeat")),
        _ => None,
    }
}

fn color_of(x: &Value, current: Color) -> Option<Color> {
    x.color().or_else(|| (x.ident() == Some("currentcolor")).then_some(current))
}

fn side<T>(e: &mut Edges<T>, i: usize) -> &mut T {
    match i {
        0 => &mut e.top,
        1 => &mut e.right,
        2 => &mut e.bottom,
        _ => &mut e.left,
    }
}

/// Which side a `border-top-*` style property names; `None` for the all-sides ones.
fn side_index(name: &str) -> Option<usize> {
    ["-top", "-right", "-bottom", "-left"].iter().position(|s| name.contains(s))
}

fn border_style(v: &Value) -> Option<BorderStyle> {
    Some(match v.ident()? {
        "none" | "hidden" => BorderStyle::None,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "solid" | "double" | "groove" | "ridge" | "inset" | "outset" => BorderStyle::Solid,
        _ => return None,
    })
}

pub(crate) fn length(v: &Value, font_size: f32) -> Option<Length> {
    Some(match v {
        Value::Px(px) => Length::Px(*px),
        Value::Number(n) if *n == 0.0 => Length::Px(0.0),
        Value::Em(e) => Length::Px(e * font_size),
        Value::Rem(e) => Length::Px(e * ROOT_FONT_SIZE),
        Value::Percent(p) => Length::Percent(p / 100.0),
        Value::Calc { px, em, pct } if *pct == 0.0 => Length::Px(px + em * font_size),
        Value::Calc { px, em, pct } if *px == 0.0 && *em == 0.0 => Length::Percent(pct / 100.0),
        Value::Calc { px, em, pct } => Length::Calc { px: px + em * font_size, pct: pct / 100.0 },
        Value::Ident(i) if i == "auto" => Length::Auto,
        _ => return None,
    })
}

pub(crate) fn px(v: &Value, font_size: f32) -> Option<f32> {
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

pub(crate) fn align(v: &Value) -> Option<Align> {
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
    // `currentcolor` takes the element's own text color (as of this declaration).
    let current = s.color;
    let color = |x: &Value| x.color().or_else(|| (x.ident() == Some("currentcolor")).then_some(current));
    // A border color as the author set it: `Some(None)` is `currentColor`.
    let spec = |x: &Value| -> Option<Option<Color>> {
        if x.ident() == Some("currentcolor") {
            Some(None)
        } else {
            x.color().map(Some)
        }
    };

    match d.name.as_str() {
        "display" => {
            s.display = match first.and_then(Value::ident) {
                Some("block") => Display::Block,
                Some("inline") => Display::Inline,
                Some("inline-block") => Display::InlineBlock,
                Some("flex") | Some("inline-flex") => Display::Flex,
                Some("grid") | Some("inline-grid") => Display::Grid,
                Some("list-item") => Display::ListItem,
                Some("table" | "inline-table") => Display::Table,
                Some("table-row-group" | "table-header-group" | "table-footer-group") => Display::TableRowGroup,
                Some("table-row") => Display::TableRow,
                Some("table-cell") => Display::TableCell,
                Some("table-caption") => Display::TableCaption,
                Some("none") => Display::None,
                _ => return,
            }
        }
        "overflow" | "overflow-x" | "overflow-y" => {
            let kind = |v: Option<&Value>| match v.and_then(Value::ident) {
                Some("hidden" | "clip") => Some(OverflowKind::Hidden),
                Some("scroll" | "auto") => Some(OverflowKind::Scroll),
                Some("visible") => Some(OverflowKind::Visible),
                _ => None,
            };
            let Some(first_kind) = kind(first) else { return };
            match d.name.as_str() {
                "overflow-x" => s.overflow_x = first_kind,
                "overflow-y" => s.overflow_y = first_kind,
                // `overflow: <x> [<y>]`
                _ => {
                    s.overflow_x = first_kind;
                    s.overflow_y = kind(v.get(1)).unwrap_or(first_kind);
                }
            }
        }
        "appearance" | "-webkit-appearance" => {
            s.appearance = match first.and_then(Value::ident) {
                Some("checkbox") => Appearance::Checkbox,
                Some("radio") => Appearance::Radio,
                Some("select" | "menulist") => Appearance::Select,
                Some("progress-bar" | "progress") => Appearance::Progress,
                Some("meter") => Appearance::Meter,
                Some("slider-horizontal" | "range") => Appearance::Range,
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
        "position" => {
            s.position = match first.and_then(Value::ident) {
                Some("static") => Position::Static,
                Some("relative" | "sticky") => Position::Relative,
                Some("absolute") => Position::Absolute,
                Some("fixed") => Position::Fixed,
                _ => return,
            }
        }
        "float" => {
            s.float = match first.and_then(Value::ident) {
                Some("left" | "inline-start") => Float::Left,
                Some("right" | "inline-end") => Float::Right,
                Some("none") => Float::None,
                _ => return,
            }
        }
        "clear" => {
            s.clear = match first.and_then(Value::ident) {
                Some("left") => Clear::Left,
                Some("right") => Clear::Right,
                Some("both") => Clear::Both,
                Some("none") => Clear::None,
                _ => return,
            }
        }
        "z-index" => match first {
            Some(Value::Number(n)) if n.fract() == 0.0 => s.z_index = Some(*n as i32),
            Some(Value::Ident(i)) if i == "auto" => s.z_index = None,
            _ => {}
        },
        "top" => s.inset.top = l(first).unwrap_or(s.inset.top),
        "right" => s.inset.right = l(first).unwrap_or(s.inset.right),
        "bottom" => s.inset.bottom = l(first).unwrap_or(s.inset.bottom),
        "left" => s.inset.left = l(first).unwrap_or(s.inset.left),
        "inset" => {
            let vals: Option<Vec<Length>> = v.iter().map(|x| length(x, fs)).collect();
            if let Some(e) = vals.as_deref().and_then(edges) {
                s.inset = e;
            }
        }
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
        "border-color" => {
            let vals: Option<Vec<Option<Color>>> = v.iter().map(spec).collect();
            if let Some(e) = vals.as_deref().and_then(edges) {
                s.border_color_set = e;
            }
        }
        "border-style" => {
            let vals: Option<Vec<BorderStyle>> = v.iter().map(border_style).collect();
            if let Some(e) = vals.as_deref().and_then(edges) {
                s.border_style = e;
            }
        }
        "border-top-color" | "border-right-color" | "border-bottom-color" | "border-left-color" => {
            if let (Some(c), Some(i)) = (first.and_then(spec), side_index(&d.name)) {
                *side(&mut s.border_color_set, i) = c;
            }
        }
        "border-top-style" | "border-right-style" | "border-bottom-style" | "border-left-style" => {
            if let (Some(st), Some(i)) = (first.and_then(border_style), side_index(&d.name)) {
                *side(&mut s.border_style, i) = st;
            }
        }
        // `border: 1px solid #ccc` — order-independent. Without a color the border
        // follows `color`.
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let mut width = None;
            // A shorthand without a style is drawn solid (browsers draw nothing).
            let mut line = BorderStyle::Solid;
            let mut colored = None;
            for x in v {
                if let Some(w) = px(x, fs) {
                    width = Some(w);
                } else if let Some(c) = spec(x) {
                    colored = Some(c);
                } else {
                    match x.ident() {
                        Some("none" | "hidden") => line = BorderStyle::None,
                        Some("thin") => width = Some(1.0),
                        Some("medium") => width = Some(3.0),
                        Some("thick") => width = Some(5.0),
                        Some("dashed") => line = BorderStyle::Dashed,
                        Some("dotted") => line = BorderStyle::Dotted,
                        _ => {}
                    }
                }
            }
            let sides: Vec<usize> = match side_index(&d.name) {
                Some(i) => vec![i],
                None => vec![0, 1, 2, 3],
            };
            for i in sides {
                *side(&mut s.border_color_set, i) = colored.unwrap_or(None);
                *side(&mut s.border_style, i) = line;
                if let Some(w) = width {
                    *side(&mut s.border_width, i) = w;
                }
            }
        }
        "border-top-width" => s.border_width.top = p(first).unwrap_or(s.border_width.top),
        "border-right-width" => s.border_width.right = p(first).unwrap_or(s.border_width.right),
        "border-bottom-width" => s.border_width.bottom = p(first).unwrap_or(s.border_width.bottom),
        "border-left-width" => s.border_width.left = p(first).unwrap_or(s.border_width.left),
        "border-radius" => s.border_radius = p(first).unwrap_or(s.border_radius),
        "background" | "background-color" | "background-image" => {
            if d.name != "background-image" {
                if let Some(c) = v.iter().find_map(&color) {
                    s.background = Some(c);
                }
            }
            if d.name != "background-color" {
                // The shorthand resets the image; `none` and unsupported images clear it.
                s.background_image = v.iter().find_map(|x| match x {
                    Value::Gradient(g) => Some((**g).clone()),
                    _ => None,
                });
                s.background_url = v.iter().find_map(|x| match x {
                    Value::Url(u) => Some(u.clone()),
                    _ => None,
                });
            }
            if d.name == "background" {
                // `<position> [/ <size>] <repeat>`, in any order around the image and color.
                let (before, after) = match v.iter().position(|x| *x == Value::Slash) {
                    Some(i) => (&v[..i], &v[i + 1..]),
                    None => (&v[..], &v[..0]),
                };
                let pos_vals: Vec<Value> = before
                    .iter()
                    .filter(|x| matches!(x, Value::Percent(_) | Value::Px(_) | Value::Em(_) | Value::Rem(_)) || matches!(x.ident(), Some("left" | "right" | "top" | "bottom" | "center")))
                    .cloned()
                    .collect();
                s.background_position = bg_position(&pos_vals, fs).unwrap_or((StopPos::Fraction(0.0), StopPos::Fraction(0.0)));
                s.background_size = bg_size(after, fs).unwrap_or(BgSize::Auto);
                s.background_repeat = bg_repeat(before).unwrap_or((true, true));
            }
        }
        "user-select" | "-webkit-user-select" | "-moz-user-select" => {
            s.user_select = match first.and_then(Value::ident) {
                Some("none") => UserSelect::None,
                Some("text" | "contain") => UserSelect::Text,
                Some("all") => UserSelect::All,
                // `auto` takes the parent's value, which is already inherited.
                _ => return,
            }
        }
        // Sciter's way to make an element's text selectable.
        "behavior" => {
            if v.iter().any(|x| x.ident() == Some("htmlarea")) {
                s.user_select = UserSelect::Text;
            }
        }
        "text-selection-background-color" => s.selection_bg = first.and_then(color).or(s.selection_bg),
        "text-selection" => s.selection_bg = v.get(1).and_then(color).or(s.selection_bg),
        "transform" => {
            let list: Vec<TransformFn> = v
                .iter()
                .filter_map(|x| if let Value::Transform(t) = x { Some(*t) } else { None })
                .collect();
            // A list with anything unusable in it is invalid as a whole.
            if first.and_then(Value::ident) == Some("none") {
                s.transform.clear();
            } else if !list.is_empty() && list.len() == v.len() {
                s.transform = list;
            }
        }
        "transform-origin" => {
            if let Some(o) = bg_position(&v[..v.len().min(2)], fs) {
                s.transform_origin = o;
            }
        }
        "background-position" => {
            if let Some(p) = bg_position(v, fs) {
                s.background_position = p;
            }
        }
        "background-size" => {
            if let Some(z) = bg_size(v, fs) {
                s.background_size = z;
            }
        }
        "background-repeat" => {
            if let Some(r) = bg_repeat(v) {
                s.background_repeat = r;
            }
        }
        "box-shadow" => {
            let mut shadows = Vec::new();
            for group in v.split(|x| *x == Value::Comma) {
                let mut lens = Vec::new();
                let (mut color, mut inset) = (current, false);
                for x in group {
                    if let Some(n) = px(x, fs) {
                        lens.push(n);
                    } else if let Some(c) = self::color_of(x, current) {
                        color = c;
                    } else if x.ident() == Some("inset") {
                        inset = true;
                    }
                }
                if let [dx, dy, rest @ ..] = lens.as_slice() {
                    let blur = rest.first().copied().unwrap_or(0.0).max(0.0);
                    let spread = rest.get(1).copied().unwrap_or(0.0);
                    shadows.push(BoxShadow { dx: *dx, dy: *dy, blur, spread, color, inset });
                }
            }
            if !shadows.is_empty() || first.and_then(Value::ident) == Some("none") {
                s.box_shadow = shadows;
            }
        }
        "opacity" => {
            if let Some(Value::Number(n)) = first {
                s.opacity = n.clamp(0.0, 1.0);
            }
        }
        "color" => s.color = first.and_then(color).unwrap_or(s.color),
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
        "text-overflow" => match first.and_then(Value::ident) {
            Some("ellipsis") => s.text_ellipsis = true,
            Some("clip") => s.text_ellipsis = false,
            _ => {}
        },
        "list-style-type" => {
            if let Some(t) = first.and_then(Value::ident).and_then(ListStyleType::parse) {
                s.list_style_type = t;
            }
        }
        // `list-style: <type> || <position> || <image>`: only the type is used.
        "list-style" => {
            if let Some(t) = v.iter().filter_map(Value::ident).find_map(ListStyleType::parse) {
                s.list_style_type = t;
            }
        }
        "white-space" => {
            s.white_space = match first.and_then(Value::ident) {
                Some("normal") => WhiteSpace::Normal,
                Some("nowrap") => WhiteSpace::Nowrap,
                Some("pre") => WhiteSpace::Pre,
                Some("pre-wrap") => WhiteSpace::PreWrap,
                Some("pre-line") => WhiteSpace::PreLine,
                _ => return,
            }
        }
        "text-transform" => {
            s.text_transform = match first.and_then(Value::ident) {
                Some("uppercase") => TextTransform::Uppercase,
                Some("lowercase") => TextTransform::Lowercase,
                Some("capitalize") => TextTransform::Capitalize,
                Some("none") => TextTransform::None,
                _ => return,
            }
        }
        "letter-spacing" => match first {
            Some(Value::Ident(i)) if i == "normal" => s.letter_spacing = 0.0,
            other => s.letter_spacing = p(other).unwrap_or(s.letter_spacing),
        },
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
        _ => {
            // Grid and gap properties; anything else is unsupported and ignored, like a browser would.
            if !anim::apply_properties(s, &d.name, v) {
                grid::apply(s, &d.name, v, fs);
            }
        }
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
        assert!(Query::parse("li::marker").is_err());
        assert!(Query::parse("<<").is_err());
    }

    #[test]
    fn unsupported_selectors_drop_only_their_rule() {
        let doc = Document::parse(
            r#"<style>p::first-line { color: red } p:visited { color: red } p { color: blue }</style><p id="p">x</p>"#,
        );
        assert_eq!(color_of(&doc, &compute_styles(&doc), "p"), BLUE);
    }

    fn style_of(doc: &Document, styles: &StyleMap, id: &str) -> ComputedStyle {
        styles.get(doc.find(|n| n.attr("id") == Some(id)).unwrap()).clone()
    }

    #[test]
    fn important_beats_specificity_and_inline() {
        let doc = Document::parse(
            r#"<style>.a { color: red !important } #a { color: blue }</style>
               <p class="a" id="a" style="color: green">x</p>
               <p id="b" style="color: green !important; color: blue">y</p>"#,
        );
        let styles = compute_styles(&doc);
        assert_eq!(color_of(&doc, &styles, "a"), RED);
        assert_eq!(color_of(&doc, &styles, "b"), Color::rgb(0, 128, 0));
    }

    #[test]
    fn custom_properties_inherit_and_fall_back() {
        let doc = Document::parse(
            r#"<style>:root { --c: #ff0000; --pad: 6px; --both: var(--c) }
               #a { color: var(--c); padding: var(--pad) 2px }
               #b { --c: blue; color: var(--c) }
               #c { color: var(--none, blue); margin: 3px }
               #d { color: var(--none) }
               #e { color: var(--both) }</style>
               <div id="a"></div><div id="b"><span id="b2" style="color: var(--c)">x</span></div>
               <div id="c"></div><div id="d"></div><div id="e"></div>"#,
        );
        let styles = compute_styles(&doc);
        assert_eq!(color_of(&doc, &styles, "a"), RED);
        assert_eq!(style_of(&doc, &styles, "a").padding.top, 6.0);
        assert_eq!(style_of(&doc, &styles, "a").padding.left, 2.0);
        assert_eq!(color_of(&doc, &styles, "b"), BLUE);
        assert_eq!(color_of(&doc, &styles, "b2"), BLUE); // inherits the override
        assert_eq!(color_of(&doc, &styles, "c"), BLUE);
        assert_eq!(color_of(&doc, &styles, "d"), Color::BLACK); // unresolved: declaration dropped
        assert_eq!(color_of(&doc, &styles, "e"), RED);
    }

    #[test]
    fn calc_and_viewport_units() {
        let doc = Document::parse(
            r#"<style>#a { width: calc(10px + 2 * 5px); height: calc(1em + 4px); font-size: 10px }
               #b { width: calc(100% / 4); height: 10vh; margin-left: min(5px, 9px); padding-top: clamp(2px, 1px, 8px) }
               #c { width: calc(100% - 4px) }</style>
               <div id="a"></div><div id="b"></div><div id="c"></div>"#,
        );
        let styles = compute_styles_in(&doc, &InteractionState::default(), (500.0, 400.0));
        assert_eq!(style_of(&doc, &styles, "a").width, Length::Px(20.0));
        assert_eq!(style_of(&doc, &styles, "a").height, Length::Px(14.0));
        let b = style_of(&doc, &styles, "b");
        assert_eq!(b.width, Length::Percent(0.25));
        assert_eq!(b.height, Length::Px(40.0));
        assert_eq!(b.margin.left, Length::Px(5.0));
        assert_eq!(b.padding.top, 2.0);
        // `%` mixed with `px` is resolved by layout.
        assert_eq!(style_of(&doc, &styles, "c").width, Length::Calc { px: -4.0, pct: 1.0 });
        assert!(styles.uses_viewport);
    }

    #[test]
    fn media_queries_follow_the_viewport() {
        let css = r#"<style>#a { color: red }
            @media (min-width: 600px) { #a { color: blue } }
            @media screen and (max-width: 300px), print { #a { color: green } }
            @media not (min-width: 100px) { #a { color: yellow } }
            @media (400px <= width < 700px) { #a { padding-top: 7px } }
            @font-face { font-family: x; src: url(x.ttf) }</style><p id="a">x</p>"#;
        let doc = Document::parse(css);
        let at = |w: f32| {
            let styles = compute_styles_in(&doc, &InteractionState::default(), (w, 500.0));
            style_of(&doc, &styles, "a")
        };
        assert_eq!(at(800.0).color, BLUE);
        assert_eq!(at(500.0).color, RED);
        assert_eq!(at(500.0).padding.top, 0.0); // `a < b < c` chains aren't supported
        assert_eq!(at(250.0).color, Color::rgb(0, 128, 0));
    }

    #[test]
    fn colors_and_borders() {
        let doc = Document::parse(
            r#"<style>#a { background: hsl(120, 100%, 25%); color: rebeccapurple; border: 2px solid }
               #b { border-bottom: 1px solid #888; color: red }
               #c { background: rgb(0 128 255 / 50%) }</style>
               <div id="a"></div><div id="b"></div><div id="c"></div>"#,
        );
        let styles = compute_styles(&doc);
        let a = style_of(&doc, &styles, "a");
        assert_eq!(a.background, Some(Color::rgb(0, 128, 0)));
        assert_eq!(a.border_color.left, Color::rgb(102, 51, 153)); // follows `color`
        let b = style_of(&doc, &styles, "b");
        assert_eq!((b.border_width.bottom, b.border_width.top), (1.0, 0.0));
        assert_eq!(b.border_color.bottom, Color::rgb(136, 136, 136));
        assert_eq!(b.border_color.top, RED); // not set: follows `color`
        assert_eq!(style_of(&doc, &styles, "c").background, Some(Color { r: 0, g: 128, b: 255, a: 127 }));
    }

    #[test]
    fn per_side_border_colors_and_styles() {
        let doc = Document::parse(
            r#"<style>#a { border: 2px dashed red; border-left-color: blue; border-bottom-style: dotted }
               #b { border-style: solid none; border-width: 3px; border-color: red green }</style>
               <div id="a"></div><div id="b"></div>"#,
        );
        let styles = compute_styles(&doc);
        let a = style_of(&doc, &styles, "a");
        assert_eq!((a.border_color.top, a.border_color.left), (RED, BLUE));
        assert_eq!((a.border_style.top, a.border_style.bottom), (BorderStyle::Dashed, BorderStyle::Dotted));
        let b = style_of(&doc, &styles, "b");
        assert_eq!((b.border_width.top, b.border_width.right), (3.0, 0.0));
        assert_eq!(b.border_color.right, Color::rgb(0, 128, 0));
    }

    #[test]
    fn box_shadow_lists() {
        let doc = Document::parse(
            r#"<div id="a" style="color: blue; box-shadow: 2px 3px 4px 1px rgba(0,0,0,0.5), inset 0 0 0 red, 1px 1px"></div>
               <div id="b" style="box-shadow: none"></div>"#,
        );
        let styles = compute_styles(&doc);
        let a = style_of(&doc, &styles, "a").box_shadow;
        assert_eq!(a.len(), 3);
        assert_eq!((a[0].dx, a[0].dy, a[0].blur, a[0].spread), (2.0, 3.0, 4.0, 1.0));
        assert_eq!(a[0].color.a, 127);
        assert!(a[1].inset && a[1].color == RED);
        assert_eq!(a[2].color, BLUE); // no color: currentColor
        assert!(style_of(&doc, &styles, "b").box_shadow.is_empty());
    }

    #[test]
    fn generated_content_boxes() {
        let doc = Document::parse(
            r#"<style>.a::before { content: "> "; color: red }
               .a::after { content: attr(data-x) "!"; display: block }
               .b::before { color: blue }
               .c:hover::before, .c::after { content: "" } .c { color: blue }</style>
               <p class="a" data-x="hi">t</p><p class="b">u</p><p class="c">v</p>"#,
        );
        let styles = compute_styles(&doc);
        let node = |c: &str| doc.find(|n| n.attr("class") == Some(c)).unwrap();
        let before = styles.pseudo(node("a"), PseudoElement::Before).unwrap();
        assert_eq!(styles.content(before), "> ");
        assert_eq!(styles.get(before).color, RED);
        assert_eq!(styles.get(before).display, Display::Inline);
        assert_eq!(styles.origin(before), node("a"));
        let after = styles.pseudo(node("a"), PseudoElement::After).unwrap();
        assert_eq!((styles.content(after), styles.get(after).display), ("hi!", Display::Block));
        assert!(styles.pseudo(node("b"), PseudoElement::Before).is_none()); // no `content`
        assert!(styles.pseudo(node("c"), PseudoElement::Before).is_none()); // :hover not active
        let c_after = styles.pseudo(node("c"), PseudoElement::After).unwrap();
        assert_eq!(styles.get(c_after).color, BLUE); // inherited from the element
        assert!(styles.is_pseudo(c_after) && !styles.is_pseudo(node("c")));
    }

    #[test]
    fn gradients_parse() {
        let doc = Document::parse(
            r#"<div id="a" style="background: #fff linear-gradient(to bottom right, red, blue 50%, green 60% 70%)"></div>
               <div id="b" style="background-image: repeating-radial-gradient(circle at left 20%, #000 0 4px, #fff 4px 8px)"></div>
               <div id="c" style="background: linear-gradient(45deg, red)"></div>
               <div id="d" style="background-image: linear-gradient(0.25turn, red, blue); background-image: none"></div>
               <div id="e" style="background-image: linear-gradient(0.25turn, red, blue)"></div>"#,
        );
        let styles = compute_styles(&doc);
        let a = style_of(&doc, &styles, "a");
        assert_eq!(a.background, Some(Color::rgb(255, 255, 255)));
        let g = a.background_image.unwrap();
        assert_eq!(g.kind, GradientKind::Linear(LinearDir::Corner(1, 1)));
        assert_eq!(g.stops.len(), 4); // `green 60% 70%` is two stops
        assert_eq!(g.stops[2].pos, Some(StopPos::Fraction(0.6)));
        let b = style_of(&doc, &styles, "b").background_image.unwrap();
        assert!(b.repeating);
        assert_eq!(b.kind, GradientKind::Radial { circle: true, extent: Extent::FarthestCorner, at: (StopPos::Fraction(0.0), StopPos::Fraction(0.2)) });
        assert!(style_of(&doc, &styles, "c").background_image.is_none()); // needs two stops
        assert!(style_of(&doc, &styles, "d").background_image.is_none());
        assert_eq!(style_of(&doc, &styles, "e").background_image.unwrap().kind, GradientKind::Linear(LinearDir::Angle(90.0)));
    }

    #[test]
    fn grid_properties_parse() {
        let doc = Document::parse(
            r#"<div id="a" style="display: grid; grid-template-columns: 100px repeat(2, minmax(10px, 1fr)) auto; grid-auto-flow: column dense; gap: 4px 8px; grid-template-areas: 'a b' 'c c'"></div>
               <div id="b" style="grid-column: 2 / span 3; grid-row: span 2; align-self: center; grid-area: hero"></div>
               <div id="c" style="grid-area: 1 / 2 / 3 / 4; place-items: center end"></div>"#,
        );
        let styles = compute_styles(&doc);
        let a = style_of(&doc, &styles, "a");
        assert_eq!(a.display, Display::Grid);
        let fixed = |p: f32| Track { min: Breadth::Px(p), max: Breadth::Px(p) };
        assert_eq!(
            a.grid_template_columns,
            vec![
                TrackItem::Track(fixed(100.0)),
                TrackItem::Repeat(TrackRepeat::Count(2), vec![Track { min: Breadth::Px(10.0), max: Breadth::Fr(1.0) }]),
                TrackItem::Track(Track { min: Breadth::Auto, max: Breadth::Auto }),
            ]
        );
        assert_eq!(a.grid_auto_flow, GridAutoFlow { column: true, dense: true });
        assert_eq!((a.row_gap, a.column_gap), (4.0, 8.0));
        assert_eq!(a.grid_areas, vec![vec!["a", "b"], vec!["c", "c"]]);
        let b = style_of(&doc, &styles, "b");
        assert_eq!(b.grid_column, GridPlace { start: GridLine::Line(2), end: GridLine::Span(3) });
        assert_eq!(b.grid_row.start, GridLine::Span(2));
        assert_eq!((b.align_self, b.grid_area_name.as_deref()), (Align::Center, Some("hero")));
        let c = style_of(&doc, &styles, "c");
        assert_eq!(c.grid_row, GridPlace { start: GridLine::Line(1), end: GridLine::Line(3) });
        assert_eq!(c.grid_column, GridPlace { start: GridLine::Line(2), end: GridLine::Line(4) });
        assert_eq!((c.align_items, c.justify_items), (Align::Center, Align::End));
    }

    #[test]
    fn background_image_properties() {
        let doc = Document::parse(
            r#"<div id="a" style="background: #fff url('a b.png') no-repeat right 10px / 50% auto"></div>
               <div id="b" style="background-image: url(x.png); background-size: cover; background-position: center top; background-repeat: repeat-x"></div>
               <div id="c" style="background-image: url(x.png); background: red"></div>"#,
        );
        let styles = compute_styles(&doc);
        let a = style_of(&doc, &styles, "a");
        assert_eq!(a.background_url.as_deref(), Some("a b.png"));
        assert_eq!(a.background_repeat, (false, false));
        assert_eq!(a.background_position, (StopPos::Fraction(1.0), StopPos::Px(10.0)));
        assert_eq!(a.background_size, BgSize::Explicit(Some(StopPos::Fraction(0.5)), None));
        let b = style_of(&doc, &styles, "b");
        assert_eq!((b.background_size, b.background_repeat), (BgSize::Cover, (true, false)));
        assert_eq!(b.background_position, (StopPos::Fraction(0.5), StopPos::Fraction(0.0)));
        assert!(style_of(&doc, &styles, "c").background_url.is_none()); // the shorthand reset it
    }

    #[test]
    fn text_selection_is_opt_in() {
        let doc = Document::parse(
            r#"<style>.t { user-select: text } .k { behavior: htmlarea; text-selection-background-color: #ff0 }</style>
               <div id="a">x</div><div id="b" class="t"><p id="b2" style="user-select: none">y</p><p id="b3" style="user-select: auto">z</p></div>
               <div id="c" selectable>w</div><div id="d" class="k"><span id="d2">v</span></div><div id="e" style="user-select: all"></div>"#,
        );
        let styles = compute_styles(&doc);
        let us = |id: &str| style_of(&doc, &styles, id).user_select;
        assert_eq!((us("a"), us("b"), us("b2"), us("b3")), (UserSelect::None, UserSelect::Text, UserSelect::None, UserSelect::Text));
        assert_eq!((us("c"), us("d"), us("d2"), us("e")), (UserSelect::Text, UserSelect::Text, UserSelect::Text, UserSelect::All));
        assert_eq!(style_of(&doc, &styles, "d2").selection_bg, Some(Color::rgb(255, 255, 0))); // inherited
    }

    #[test]
    fn transform_lists_parse() {
        let doc = Document::parse(
            r#"<div id="a" style="transform: translate(10px, 50%) rotate(0.25turn) scale(2) skewX(10deg) matrix(1,0,0,1,5,6); transform-origin: left 10px"></div>
               <div id="b" style="transform: rotate(10deg) 5"></div>
               <div id="c" style="transform: rotate(10deg); transform: none"></div>"#,
        );
        let styles = compute_styles(&doc);
        let a = style_of(&doc, &styles, "a");
        assert_eq!(
            a.transform,
            vec![
                TransformFn::Translate(StopPos::Px(10.0), StopPos::Fraction(0.5)),
                TransformFn::Rotate(90.0),
                TransformFn::Scale(2.0, 2.0),
                TransformFn::Skew(10.0, 0.0),
                TransformFn::Matrix([1.0, 0.0, 0.0, 1.0, 5.0, 6.0]),
            ]
        );
        assert_eq!(a.transform_origin, (StopPos::Fraction(0.0), StopPos::Px(10.0)));
        assert!(style_of(&doc, &styles, "b").transform.is_empty());
        assert!(style_of(&doc, &styles, "c").transform.is_empty());
    }

    fn opacity_at(animator: &mut Animator, base: &StyleMap, styles: &mut StyleMap, doc: &Document, now: f64, restyled: bool) -> f32 {
        animator.update(base, styles, now, restyled);
        let n = doc.find(|n| n.attr("id") == Some("a")).unwrap();
        styles.get(n).opacity
    }

    #[test]
    fn transitions_start_on_target_changes() {
        let mut doc = Document::parse(
            r#"<style>#a { opacity: 1; width: 100px; transition: opacity 1s linear, width 2s linear 1s } #a.on { opacity: 0; width: 200px }</style><div id="a"></div>"#,
        );
        let a = doc.find(|n| n.attr("id") == Some("a")).unwrap();
        let mut animator = Animator::new();
        let base = compute_styles(&doc);
        let mut styles = base.clone();
        assert_eq!(opacity_at(&mut animator, &base, &mut styles, &doc, 0.0, true), 1.0);
        assert!(!animator.is_active());

        doc.set_attr(a, "class", "on");
        let base = compute_styles(&doc);
        let mut styles = base.clone();
        assert_eq!(opacity_at(&mut animator, &base, &mut styles, &doc, 10.0, true), 1.0); // just started
        assert!(animator.is_active());
        assert!((opacity_at(&mut animator, &base, &mut styles, &doc, 10.25, false) - 0.75).abs() < 1e-4);
        assert_eq!(styles.get(a).width, Length::Px(100.0)); // still inside its 1s delay
        // Reversing midway starts from where it is now.
        doc.set_attr(a, "class", "");
        let base = compute_styles(&doc);
        let mut styles = base.clone();
        let mid = opacity_at(&mut animator, &base, &mut styles, &doc, 10.5, true);
        assert!((mid - 0.5).abs() < 1e-4, "{mid}");
        assert!((opacity_at(&mut animator, &base, &mut styles, &doc, 11.0, false) - 0.75).abs() < 1e-4);
        assert_eq!(opacity_at(&mut animator, &base, &mut styles, &doc, 12.0, false), 1.0);
        // The width never moved: it was still waiting out its 1s delay when reversed.
        assert_eq!(styles.get(a).width, Length::Px(100.0));
        animator.update(&base, &mut styles, 20.0, false);
        assert!(!animator.is_active());
        assert_eq!(styles.get(a).width, Length::Px(100.0));
    }

    #[test]
    fn keyframe_animations_run_repeat_and_fill() {
        let doc = Document::parse(
            r#"<style>@keyframes fade { from { opacity: 0 } 50% { opacity: 1 } to { opacity: 0.5; transform: translateX(10px) } }
               #a { animation: fade 2s linear 1s 2 alternate both } </style><div id="a"></div>"#,
        );
        let a = doc.find(|n| n.attr("id") == Some("a")).unwrap();
        let mut animator = Animator::new();
        let base = compute_styles(&doc);
        let mut styles = base.clone();
        // Before its delay, `backwards` fill holds the first frame.
        assert_eq!(opacity_at(&mut animator, &base, &mut styles, &doc, 0.0, true), 0.0);
        assert!((opacity_at(&mut animator, &base, &mut styles, &doc, 1.5, false) - 0.5).abs() < 1e-4);
        assert!((opacity_at(&mut animator, &base, &mut styles, &doc, 2.0, false) - 1.0).abs() < 1e-4);
        assert_eq!(styles.get(a).transform, vec![TransformFn::Translate(StopPos::Px(5.0), StopPos::Px(0.0))]);
        // The second iteration runs backwards: 1s in is the same as 1s from the end.
        assert!((opacity_at(&mut animator, &base, &mut styles, &doc, 4.0, false) - 1.0).abs() < 1e-4);
        // Done at 1 + 2*2 = 5s; `forwards` keeps the last (reversed) frame: from { opacity: 0 }.
        assert_eq!(opacity_at(&mut animator, &base, &mut styles, &doc, 6.0, false), 0.0);
        assert!(!animator.is_active());
    }

    #[test]
    fn same_layout_ignores_paint_properties_only() {
        let doc = Document::parse(
            r#"<div id="a" style="width:10px; transform:scale(1)"></div><div id="b" style="width:10px; background:red; box-shadow:0 0 2px red; transform:rotate(1deg); cursor:pointer"></div>
               <div id="c" style="width:20px"></div><div id="d" style="width:10px; opacity:0; color:red"></div>"#,
        );
        let s = compute_styles(&doc);
        let st = |id: &str| s.get(doc.find(|n| n.attr("id") == Some(id)).unwrap()).clone();
        assert!(st("a").same_layout(&st("b")), "background, shadow, transform amount and cursor are paint-level");
        assert!(!st("a").same_layout(&st("c")), "width is not");
        assert!(!st("a").same_layout(&st("d")), "text color is baked into text layouts");
        let mut faded = st("a");
        faded.opacity = 0.5;
        assert!(!st("a").same_layout(&faded), "becoming translucent changes paint order");
        let mut fainter = faded.clone();
        fainter.opacity = 0.2;
        assert!(faded.same_layout(&fainter), "but fading further doesn't");
    }

    #[test]
    fn state_deps_find_the_pseudo_classes_rules_mention() {
        let deps = |css: &str| compute_styles(&Document::parse(&format!("<style>{css}</style><p>x</p>"))).state_deps;
        // The UA sheet already mentions :focus (input borders).
        assert_eq!(deps("p { color: red }"), StateDeps { hover: false, active: false, focus: true });
        assert!(deps("a:hover { color: red }").hover);
        assert!(deps(".x:not(:hover) b { color: red }").hover);
        assert!(deps("button:active { color: red }").active);
        let d = StateDeps { hover: false, active: false, focus: true };
        let (a, b) = (InteractionState::default(), InteractionState { hover: Some(NodeId(1)), ..Default::default() });
        assert!(!d.affected_by(&a, &b), "a hover change is invisible to a sheet with no :hover");
        assert!(d.affected_by(&a, &InteractionState { focus: Some(NodeId(1)), ..Default::default() }));
    }

    #[test]
    fn has_selects_by_descendants_children_and_following_siblings() {
        let doc = Document::parse(
            r#"<style>
                 .card:has(img) { color: red }
                 .row:has(> input:checked) { color: blue }
                 h2:has(+ p) { color: green }
               </style>
               <div id="a" class="card"><p><img></p></div><div id="b" class="card"><p>no</p></div>
               <div id="c" class="row"><input type="checkbox" checked></div><div id="d" class="row"><input type="checkbox"></div>
               <h2 id="e">t</h2><p>x</p><h2 id="f">t</h2><div>y</div>"#,
        );
        let s = compute_styles(&doc);
        let color = |id: &str| s.get(doc.find(|n| n.attr("id") == Some(id)).unwrap()).color;
        let default = color("b");
        assert_eq!(color("a"), Color::rgb(255, 0, 0), ":has(descendant)");
        assert_eq!(color("c"), Color::rgb(0, 0, 255), ":has(> child:state)");
        assert_ne!(color("d"), Color::rgb(0, 0, 255));
        assert_eq!(color("e"), Color::rgb(0, 128, 0), ":has(+ sibling)");
        assert_eq!(color("f"), default);
    }
}
