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
mod page;
mod parse;
mod selector;
mod shape;
mod system;
pub use shape::{Dim as ShapeDim, RefBox as ShapeBox, Resolved as ResolvedShape, Shape, ShapeOutside};

use nile_dom::{Document, NodeId, NodeKind};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use selectors::context::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
    SelectorCaches,
};
use selectors::matching::matches_selector;
pub use anim::{AnimEvent, AnimValue, AnimationSpec, Animator, Direction, Fill, Timing, TransitionSpec};
pub use page::{resolve as resolve_page, MarginBox, MarginPlace, PageStyle, Piece};
pub use grid::{area_lines, Breadth, GridAutoFlow, GridLine, GridPlace, Track, TrackItem, TrackRepeat};
pub use gradient::{TransformFn, ColorStop, Extent, Gradient, GradientKind, LinearDir, StopPos};
pub use parse::{color_from_hex, Declaration, FontFace, Rule, Stylesheet, Value};
use parse::Cx;
pub use system::{default_accent, parse_hex, set_system_theme, system_color, system_theme, text_on, ColorScheme, Palette, SystemTheme};
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

/// A value per corner of a box (`border-radius`), clockwise from the top left.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Corners {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

impl Corners {
    pub const fn all(v: f32) -> Corners {
        Corners { tl: v, tr: v, br: v, bl: v }
    }

    pub fn max(self) -> f32 {
        self.tl.max(self.tr).max(self.br).max(self.bl)
    }

    pub fn is_zero(self) -> bool {
        self == Corners::default()
    }
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

/// A `break-before` / `break-after` / `break-inside` value, reduced to what pagination needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Break {
    #[default]
    Auto,
    /// `avoid`, `avoid-page`: keep together if it fits on a page.
    Avoid,
    /// `page`, `always`, `left`, `right`, `recto`, `verso`: force a page break.
    Always,
}

/// `position`. `Fixed` is laid out against the viewport and stays put when the page scrolls;
/// `Sticky` is in flow and slides within its parent while the page scrolls past its inset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
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

/// `scrollbar-width`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ScrollbarWidth {
    #[default]
    Auto,
    Thin,
    None,
}

/// What a scroll container's `::-webkit-scrollbar*` rules set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScrollbarParts {
    /// `::-webkit-scrollbar { width | height }` in px (`display: none` is 0).
    pub thickness: Option<f32>,
    pub thumb: Option<Color>,
    pub thumb_radius: Option<f32>,
    pub track: Option<Color>,
}

/// How a scroll container's scrollbars look: thickness, colours and the thumb's rounding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarLook {
    pub thickness: f32,
    pub thumb: Option<Color>,
    pub thumb_radius: Option<f32>,
    pub track: Option<Color>,
}

/// Thickness of the default overlay scrollbar, and of `scrollbar-width: thin`.
pub const SCROLLBAR_AUTO: f32 = 6.0;
pub const SCROLLBAR_THIN: f32 = 4.0;

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
    /// `<input type=number>`: up/down spin arrows in the right padding.
    Spinner,
    /// `<input type=color>`: a colour swatch in the left padding.
    Swatch,
    /// Date inputs: a calendar glyph in the right padding (it opens the picker).
    Calendar,
    /// `<input type=time>`: a clock glyph in the right padding.
    Clock,
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
    /// `nwse-resize`: the diagonal from top left to bottom right.
    ResizeNwse,
    /// `nesw-resize`.
    ResizeNesw,
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
    /// Inline: lowered / raised by a fraction of the font size.
    Sub,
    Super,
    /// Inline: raised by this many px (negative lowers).
    Shift(f32),
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
    /// `shape-outside`: the outline text wraps around instead of the float's rectangle.
    pub shape_outside: Option<ShapeOutside>,
    /// `shape-margin`.
    pub shape_margin: f32,
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
    /// The colour of an `<input type=color>`.
    pub swatch: Option<Color>,
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
    /// `caption-side: bottom`; inherited.
    pub caption_bottom: bool,
    /// `prototype: Class url(file.js)`: the script class made the element's controller, and the file it
    /// comes from (empty for a class the page already defines).
    pub prototype: Option<(String, String)>,
    /// `font-variation-settings` / `font-feature-settings` as written (`"wght" 650`), for the text engine; inherited.
    pub font_variations: String,
    pub font_features: String,
    /// `list-style-image: url(...)`.
    pub list_style_image: Option<String>,
    /// `backface-visibility: hidden`: not drawn while its transform turns it away.
    pub backface_hidden: bool,
    /// `transform-style: preserve-3d` (accepted; transforms of nested elements already compose in 3D).
    pub preserve_3d: bool,
    /// `scrollbar-width` and `scrollbar-color` (thumb, track) of a scroll container.
    pub scrollbar_width: ScrollbarWidth,
    pub scrollbar_color: Option<(Color, Color)>,
    /// `table-layout: fixed`.
    pub table_fixed: bool,
    /// `vertical-align` for table cells.
    pub vertical_align: VerticalAlign,
    /// `width: fit-content | max-content | min-content` (the box shrinks to its content, up to the
    /// space available; `min-content` is treated like `max-content`).
    pub width_fit: bool,
    /// `cursor`; inherited.
    pub cursor: Cursor,
    /// `color-scheme`; inherited. Only the root's counts for the user-agent sheet.
    pub color_scheme: ColorScheme,
    /// `accent-color` (`None` = auto: the system accent); inherited.
    pub accent_color: Option<Color>,
    /// `popup-position`: which numpad point (1 bottom-left … 9 top-right) of the popup meets which
    /// point of its anchor. Read by scripts (`Element.popup`), not by layout.
    pub popup_position: Option<(u8, u8)>,
    /// `popup-animation(type: slide, axis: vertical, duration: 100ms)`, as written.
    pub popup_animation: Option<String>,
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
    /// Fixed part of each corner's radius, and the fraction of the box's width/height that
    /// `%` radii add (see [`ComputedStyle::corner_radii`]).
    pub border_radius: Corners,
    pub border_radius_pct: Corners,
    pub opacity: f32,
    pub transform: Vec<TransformFn>,
    pub transitions: Vec<TransitionSpec>,
    pub animations: Vec<AnimationSpec>,
    /// Origin of `transform` within the border box.
    pub transform_origin: (StopPos, StopPos),
    /// `perspective` of a container: its children's 3D transforms are seen from this far away.
    pub perspective: Option<f32>,
    pub perspective_origin: (StopPos, StopPos),
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
    /// `list-style-position: inside`.
    pub list_style_inside: bool,
    /// `white-space`; inherited.
    pub white_space: WhiteSpace,
    pub italic: bool,
    pub underline: bool,
    pub line_through: bool,
    pub user_select: UserSelect,
    /// `break-before`, `break-after`, `break-inside` (and the `page-break-*` spellings).
    pub break_before: Break,
    pub break_after: Break,
    pub break_inside: Break,
    /// `page-view: paged` (Nile): the element's content is shown as `@page`-sized sheets, with
    /// the page breaks `paginate` finds. Not inherited.
    pub page_view: bool,
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
            shape_outside: None,
            shape_margin: 0.0,
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
            swatch: None,
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
            caption_bottom: false,
            list_style_image: None,
            prototype: None,
            font_variations: String::new(),
            font_features: String::new(),
            backface_hidden: false,
            preserve_3d: false,
            scrollbar_width: ScrollbarWidth::Auto,
            scrollbar_color: None,
            table_fixed: false,
            vertical_align: VerticalAlign::Baseline,
            cursor: Cursor::Auto,
            width_fit: false,
            color_scheme: ColorScheme::Light,
            accent_color: None,
            popup_position: None,
            popup_animation: None,
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
            border_radius: Corners::all(0.0),
            border_radius_pct: Corners::all(0.0),
            opacity: 1.0,
            transform: Vec::new(),
            transitions: Vec::new(),
            animations: Vec::new(),
            transform_origin: (StopPos::Fraction(0.5), StopPos::Fraction(0.5)),
            perspective: None,
            perspective_origin: (StopPos::Fraction(0.5), StopPos::Fraction(0.5)),
            box_shadow: Vec::new(),
            color: Color::BLACK,
            font_size: 16.0,
            font_weight: 400.0,
            font_family: "sans-serif".into(),
            line_height: 1.2,
            text_align: TextAlign::Start,
            white_space: WhiteSpace::Normal,
            list_style_type: ListStyleType::Disc,
            list_style_inside: false,
            text_ellipsis: false,
            italic: false,
            underline: false,
            line_through: false,
            user_select: UserSelect::None,
            break_before: Break::Auto,
            break_after: Break::Auto,
            break_inside: Break::Auto,
            page_view: false,
            selection_bg: None,
            text_transform: TextTransform::None,
            letter_spacing: 0.0,
            vars: Rc::default(),
        }
    }
}

impl ComputedStyle {
    /// The `(horizontal, vertical)` radius of each corner (top left, top right, bottom right,
    /// bottom left) for a border box of `w` by `h`, `%` resolved.
    pub fn corner_radii(&self, w: f32, h: f32) -> [(f32, f32); 4] {
        let (r, q) = (self.border_radius, self.border_radius_pct);
        [(r.tl + q.tl * w, r.tl + q.tl * h), (r.tr + q.tr * w, r.tr + q.tr * h), (r.br + q.br * w, r.br + q.br * h), (r.bl + q.bl * w, r.bl + q.bl * h)]
    }

    /// Whether any corner is rounded.
    pub fn has_rounding(&self) -> bool {
        !self.border_radius.is_zero() || !self.border_radius_pct.is_zero()
    }

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
                Appearance::Spinner => "spinner",
                Appearance::Swatch => "swatch",
                Appearance::Calendar => "calendar",
                Appearance::Clock => "clock",
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
            "popup-position" => {
                let name = |p: u8| ["bottom-left", "bottom-center", "bottom-right", "middle-left", "middle-center", "middle-right", "top-left", "top-center", "top-right"][p as usize - 1];
                self.popup_position.map(|(a, b)| format!("{} {}", name(a), name(b)))?
            }
            "popup-animation" => self.popup_animation.clone()?,
            "border-top-width" => px(self.border_width.top),
            "border-right-width" => px(self.border_width.right),
            "border-bottom-width" => px(self.border_width.bottom),
            "border-left-width" => px(self.border_width.left),
            "border-color" | "border-top-color" => color(self.border_color.top),
            "border-right-color" => color(self.border_color.right),
            "border-bottom-color" => color(self.border_color.bottom),
            "border-left-color" => color(self.border_color.left),
            "border-radius" => {
                let r = self.border_radius;
                if r.tl == r.tr && r.tl == r.br && r.tl == r.bl && self.border_radius_pct.is_zero() {
                    px(r.tl)
                } else {
                    format!("{} {} {} {}", px(r.tl), px(r.tr), px(r.br), px(r.bl))
                }
            }
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
            list_style_image: parent.list_style_image.clone(),
            font_variations: parent.font_variations.clone(),
            font_features: parent.font_features.clone(),
            list_style_inside: parent.list_style_inside,
            italic: parent.italic,
            // TODO: decorations aren't really inherited in CSS; they propagate
            // to inline descendants, which this approximates.
            underline: parent.underline,
            line_through: parent.line_through,
            text_transform: parent.text_transform,
            user_select: parent.user_select,
            border_spacing: parent.border_spacing,
            border_collapse: parent.border_collapse,
            caption_bottom: parent.caption_bottom,
            cursor: parent.cursor,
            color_scheme: parent.color_scheme,
            accent_color: parent.accent_color,
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
popup { display: none; background: #fff; color: #222; border: 1px solid #b4b8c0; box-shadow: 0 4px 14px rgba(0,0,0,0.25); padding: 4px }
popup[role=tooltip] { background: #fffbe6; padding: 3px 7px; font-size: 13px }
menu.popup, menu.context { display: none }
menu.popup, menu.context, popup > menu { list-style: none; margin: 0; padding: 4px 0; background: #fff; border: 1px solid #b4b8c0; box-shadow: 0 4px 14px rgba(0,0,0,0.25); min-width: 140px }
popup > menu, popup > menu.popup, popup > menu.context { display: block; border: 0; box-shadow: none }
menu.popup > li, menu.context > li, popup > menu > li { display: block; list-style: none; padding: 4px 18px; cursor: default }
menu.popup > li.nile-hover, menu.context > li.nile-hover, popup > menu > li.nile-hover { background: @ACCENT@; color: @ACCENT_TEXT@ }
menu.popup > li[disabled], menu.context > li[disabled], popup > menu > li[disabled] { color: #999; background: transparent !important }
.nile-picker { display: block; background: #fff; color: #222; border: 1px solid #b4b8c0; border-radius: 6px; box-shadow: 0 6px 18px rgba(0,0,0,0.25); padding: 8px; font: 13px sans-serif }
.nile-picker button { font: inherit; color: inherit; background: transparent; border: 0; border-radius: 4px; padding: 0; margin: 0; box-shadow: none; min-height: 0; cursor: default }
.nile-picker .np-head { display: flex; align-items: center; margin-bottom: 4px }
.nile-picker .np-title { flex: 1; text-align: center; font-weight: bold }
.nile-picker .np-nav { width: 26px; height: 24px; font-size: 18px; line-height: 20px }
.nile-picker .np-grid { display: grid; grid-template-columns: repeat(7, 30px); grid-auto-rows: 28px }
.nile-picker .np-dow { text-align: center; color: #777; font-size: 11px; line-height: 28px }
.nile-picker .np-day { text-align: center }
.nile-picker .np-out { color: #aaa }
.nile-picker .np-today { border: 1px solid @ACCENT@ }
.nile-picker .np-sel { background: @ACCENT@; color: @ACCENT_TEXT@ }
.nile-picker button[disabled] { color: #ccc }
.nile-picker .np-months { display: grid; grid-template-columns: repeat(3, 62px); grid-auto-rows: 34px; gap: 2px }
.nile-picker .np-foot { display: flex; justify-content: space-between; margin-top: 6px; padding-top: 6px; border-top: 1px solid #e3e5e8 }
.nile-picker .np-link { color: #1d4ed8; padding: 3px 6px }
.nile-picker .np-split { display: flex; gap: 10px }
.nile-picker .np-clock { display: flex; gap: 4px }
.nile-picker .np-col { display: block; width: 48px; height: 212px; overflow: auto }
.nile-picker .np-cell { display: block; width: 100%; height: 26px; text-align: center }
.nile-picker .np-color { width: 216px }
.nile-picker .np-sv { position: relative; width: 216px; height: 150px; border-radius: 4px; overflow: hidden }
.nile-picker .np-white { position: absolute; left: 0; top: 0; right: 0; bottom: 0; background: linear-gradient(to right, #ffffff, rgba(255,255,255,0)) }
.nile-picker .np-black { position: absolute; left: 0; top: 0; right: 0; bottom: 0; background: linear-gradient(to bottom, rgba(0,0,0,0), #000000) }
.nile-picker .np-knob { position: absolute; width: 12px; height: 12px; margin: -6px 0 0 -6px; border: 2px solid #fff; border-radius: 6px; box-shadow: 0 0 2px #000 }
.nile-picker .np-hue { position: relative; height: 12px; margin: 10px 0; border-radius: 6px; background: linear-gradient(to right, #ff0000, #ffff00, #00ff00, #00ffff, #0000ff, #ff00ff, #ff0000) }
.nile-picker .np-hueknob { position: absolute; top: -2px; width: 4px; height: 16px; margin-left: -2px; background: #fff; border: 1px solid #555; border-radius: 2px }
.nile-picker .np-row { display: flex; gap: 8px; align-items: center }
.nile-picker .np-preview { width: 28px; height: 24px; border: 1px solid #b4b8c0; border-radius: 4px }
.nile-picker .np-hex { width: 90px }
.nile-picker .np-swatches { display: grid; grid-template-columns: repeat(6, 28px); gap: 6px; margin-top: 10px }
.nile-picker .np-swatch { width: 28px; height: 20px; border: 1px solid rgba(0,0,0,0.25) }
frame { display: block; height: 150px; border: 0 }
.nile-modal { position: fixed; left: 0; top: 0; right: 0; bottom: 0; z-index: 2147483600; background: rgba(0,0,0,0.35); display: flex; align-items: center; justify-content: center }
.nile-messagebox { min-width: 280px; max-width: 460px; background: #fff; color: #222; border: 1px solid #b4b8c0; border-radius: 8px; box-shadow: 0 10px 34px rgba(0,0,0,0.4); overflow: hidden }
.nile-messagebox-title { padding: 8px 14px; font-weight: bold; background: #f1f3f6; border-bottom: 1px solid #dde0e6 }
.nile-messagebox-body { display: flex; padding: 16px 14px }
.nile-messagebox-icon { width: 30px; height: 30px; border-radius: 15px; margin-right: 12px; text-align: center; line-height: 30px; font-weight: bold; color: @ACCENT_TEXT@; background: @ACCENT@; flex: none }
.nile-messagebox.alert .nile-messagebox-icon { background: #e0a100 }
.nile-messagebox.error .nile-messagebox-icon { background: #d33a3a }
.nile-messagebox.question .nile-messagebox-icon { background: @ACCENT@ }
.nile-messagebox-text { flex: 1; align-self: center }
.nile-messagebox-buttons { display: flex; justify-content: flex-end; padding: 8px 14px 12px }
.nile-messagebox-buttons > button { margin-left: 8px; padding: 4px 16px }
.nile-messagebox-buttons > button.default { border-color: @ACCENT@; font-weight: bold }
frameset { display: grid; height: 100%; min-height: 150px }
frameset > * { min-width: 0; min-height: 0 }
frameset > frame { height: auto }
splitter { display: block; background: #c8c8c8; cursor: col-resize }
frameset[rows] > splitter { cursor: row-resize }
iframe { display: inline-block; width: 300px; height: 150px; border: 0 }
article, aside, address, blockquote, figure, figcaption, details, summary, fieldset, legend, dl, dt, dd, ol, pre,
  h4, h5, h6, hgroup, menu { display: block }
span, a, b, strong, em, i, u, s, del, br, label, code, kbd, samp, var, tt, small, big, sub, sup, mark, abbr, cite, q,
  dfn, time, ins, font, bdi, bdo, data, output { display: inline }
code, kbd, samp, tt, pre { font-family: monospace }
small, sub, sup { font-size: 0.83em }
sub { vertical-align: sub }
sup { vertical-align: super }
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
legend { display: block; padding: 0 3px; margin: -1em 0 0.35em 0.5em; line-height: 1.2; width: fit-content }
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
a:visited { color: #551a8b }
a[href] { cursor: pointer }
input, textarea { cursor: text }
button, select, input[type=checkbox], input[type=radio], input[type=button], input[type=submit], input[type=reset], input[type=range], input[type=file], progress, meter { cursor: default }
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
input:focus, textarea:focus, select:focus { border-color: @ACCENT@ }
button:disabled, input:disabled, textarea:disabled, select:disabled { color: #8a8a8a; background: #f0f0f0; border-color: #c4c4c4 }
input[type=checkbox], input[type=radio] { width: 13px; height: 13px; min-height: 0; padding: 0;
  margin: 3px 4px; background: #ffffff; appearance: checkbox }
input[type=radio] { border-radius: 7px; appearance: radio }
progress { display: inline-block; width: 160px; height: 16px; margin: 2px; border: 0; padding: 0; background: transparent; appearance: progress }
meter { display: inline-block; width: 80px; height: 16px; margin: 2px; border: 0; padding: 0; background: transparent; appearance: meter }
input[type=range] { width: 129px; height: 16px; min-height: 0; margin: 2px; padding: 0; border: 0; background: transparent; appearance: range }
input[type=file] { width: auto; background: #efefef; text-align: left; cursor: default }
input[type=number] { padding-right: 20px; appearance: spinner }
input[type=color] { width: 86px; padding-left: 30px; appearance: swatch }
input[type=date], input[type=month], input[type=week], input[type=datetime-local] { padding-right: 26px; appearance: calendar }
input[type=time] { padding-right: 26px; appearance: clock }
input[type=checkbox]:checked, input[type=radio]:checked { background: @ACCENT@; border-color: @ACCENT@; color: @ACCENT_TEXT@ }
input[type=button], input[type=submit], input[type=reset] { width: auto; background: #efefef }
"#;

/// The look of the platform's own controls, layered over the neutral base sheet (later rules win at
/// equal specificity, so each repeats the base selectors it changes). The `:hover`/`:active`/`:focus-visible`
/// rules mean every page restyles on pointer moves, which the partial restyle keeps cheap (see `StateDeps`).
const THEME_WINDOWS: &str = r#"
html { font-family: "Segoe UI", sans-serif }
button, input, textarea, select { font-family: "Segoe UI", sans-serif }
button, input { padding: 5px 12px; border: 1px solid #d1d1d1; border-bottom-color: #b8b8b8; border-radius: 4px; background: #fdfdfd }
input { background: #fdfdfd; border-color: #e5e5e5; border-bottom-color: #8a8a8a; padding: 5px 10px; min-height: 18px }
textarea { padding: 5px 10px; border: 1px solid #e5e5e5; border-bottom-color: #8a8a8a; border-radius: 4px; background: #fdfdfd }
select { padding: 5px 28px 5px 10px; border: 1px solid #d1d1d1; border-bottom-color: #b8b8b8; border-radius: 4px; background: #fdfdfd }
input:focus, textarea:focus, select:focus { border-color: #e5e5e5; border-bottom-color: @ACCENT@; box-shadow: 0 1px 0 @ACCENT@ }
button:disabled, input:disabled, textarea:disabled, select:disabled { color: #a3a3a3; background: #f6f6f6; border-color: #e6e6e6 }
input[type=checkbox], input[type=radio] { width: 18px; height: 18px; margin: 3px 4px; padding: 0; border: 1px solid #616161; border-radius: 4px; background: #fdfdfd }
input[type=radio] { border-radius: 9px }
input[type=checkbox]:checked, input[type=radio]:checked { background: @ACCENT@; border-color: @ACCENT@; color: @ACCENT_TEXT@ }
input[type=file] { background: #fdfdfd }
input[type=button], input[type=submit], input[type=reset] { background: #fdfdfd; border-color: #d1d1d1; border-bottom-color: #b8b8b8 }
button:hover:not(:disabled), select:hover:not(:disabled), input[type=button]:hover:not(:disabled), input[type=submit]:hover:not(:disabled), input[type=reset]:hover:not(:disabled), input[type=file]:hover:not(:disabled) { background: #f6f6f6; border-color: #d1d1d1; border-bottom-color: #b8b8b8 }
button:active:not(:disabled), select:active:not(:disabled), input[type=button]:active:not(:disabled), input[type=submit]:active:not(:disabled), input[type=reset]:active:not(:disabled), input[type=file]:active:not(:disabled) { background: #f0f0f0; color: #5d5d5d }
input:hover:not(:disabled):not(:focus):not([type=checkbox]):not([type=radio]):not([type=button]):not([type=submit]):not([type=reset]):not([type=range]):not([type=file]), textarea:hover:not(:disabled):not(:focus) { background: #f9f9f9 }
input[type=checkbox]:hover:not(:checked):not(:disabled), input[type=radio]:hover:not(:checked):not(:disabled) { border-color: #3d3d3d }
button:focus-visible, select:focus-visible, input[type=checkbox]:focus-visible, input[type=radio]:focus-visible, input[type=button]:focus-visible, input[type=submit]:focus-visible, input[type=file]:focus-visible { box-shadow: 0 0 0 2px #ffffff, 0 0 0 4px #1a1a1a }
"#;

const THEME_MACOS: &str = r#"
button, input { padding: 3px 12px; border: 1px solid #d6d6d6; border-bottom-color: #c2c2c2; border-radius: 6px; background: #ffffff; box-shadow: 0 1px 1px rgba(0,0,0,0.08) }
input { padding: 3px 7px; border-color: #cfcfcf; border-bottom-color: #bdbdbd; border-radius: 5px; box-shadow: none }
textarea { padding: 3px 7px; border: 1px solid #cfcfcf; border-radius: 5px }
select { border: 1px solid #d6d6d6; border-bottom-color: #c2c2c2; border-radius: 6px; box-shadow: 0 1px 1px rgba(0,0,0,0.08) }
input:focus, textarea:focus, select:focus { border-color: @ACCENT@; box-shadow: 0 0 0 3px @ACCENT_A@ }
button:disabled, input:disabled, textarea:disabled, select:disabled { color: #a0a0a0; background: #f5f5f5; border-color: #e2e2e2; box-shadow: none }
input[type=checkbox], input[type=radio] { width: 14px; height: 14px; margin: 3px 5px; padding: 0; border: 1px solid #b0b0b0; border-radius: 4px; background: #ffffff; box-shadow: none }
input[type=radio] { border-radius: 7px }
input[type=checkbox]:checked, input[type=radio]:checked { background: @ACCENT@; border-color: @ACCENT@; color: @ACCENT_TEXT@ }
input[type=file] { background: #ffffff }
input[type=button], input[type=submit], input[type=reset] { background: #ffffff }
button:hover:not(:disabled), select:hover:not(:disabled), input[type=button]:hover:not(:disabled), input[type=submit]:hover:not(:disabled), input[type=reset]:hover:not(:disabled), input[type=file]:hover:not(:disabled) { background: #f7f7f7 }
button:active:not(:disabled), select:active:not(:disabled), input[type=button]:active:not(:disabled), input[type=submit]:active:not(:disabled), input[type=reset]:active:not(:disabled), input[type=file]:active:not(:disabled) { background: #e6e6e6 }
input[type=checkbox]:hover:not(:checked):not(:disabled), input[type=radio]:hover:not(:checked):not(:disabled) { border-color: #8e8e8e }
button:focus-visible, select:focus-visible, input[type=checkbox]:focus-visible, input[type=radio]:focus-visible, input[type=button]:focus-visible, input[type=submit]:focus-visible, input[type=file]:focus-visible { box-shadow: 0 0 0 3px @ACCENT_A@ }
"#;

const THEME_LINUX: &str = r#"
button, input { padding: 5px 12px; border: 1px solid #cdc7c2; border-bottom-color: #bfb8b1; border-radius: 6px; background: #f6f5f4 }
input { background: #ffffff; padding: 5px 8px; border-bottom-color: #cdc7c2 }
textarea { padding: 5px 8px; border: 1px solid #cdc7c2; border-radius: 6px }
select { padding: 5px 28px 5px 10px; border: 1px solid #cdc7c2; border-bottom-color: #bfb8b1; border-radius: 6px; background: #f6f5f4 }
input:focus, textarea:focus, select:focus { border-color: @ACCENT@; box-shadow: 0 0 0 2px @ACCENT_A@ }
button:disabled, input:disabled, textarea:disabled, select:disabled { color: #9a9996; background: #faf9f8; border-color: #e0dcd8 }
input[type=checkbox], input[type=radio] { width: 16px; height: 16px; margin: 3px 4px; padding: 0; border: 1px solid #b5afa9; border-radius: 4px; background: #ffffff }
input[type=radio] { border-radius: 8px }
input[type=checkbox]:checked, input[type=radio]:checked { background: @ACCENT@; border-color: @ACCENT@; color: @ACCENT_TEXT@ }
input[type=file] { background: #f6f5f4 }
input[type=button], input[type=submit], input[type=reset] { background: #f6f5f4 }
button:hover:not(:disabled), select:hover:not(:disabled), input[type=button]:hover:not(:disabled), input[type=submit]:hover:not(:disabled), input[type=reset]:hover:not(:disabled), input[type=file]:hover:not(:disabled) { background: #fafaf9; border-color: #c4beb8 }
button:active:not(:disabled), select:active:not(:disabled), input[type=button]:active:not(:disabled), input[type=submit]:active:not(:disabled), input[type=reset]:active:not(:disabled), input[type=file]:active:not(:disabled) { background: #e4e0dc }
input[type=checkbox]:hover:not(:checked):not(:disabled), input[type=radio]:hover:not(:checked):not(:disabled) { border-color: #8e8984 }
button:focus-visible, select:focus-visible, input[type=checkbox]:focus-visible, input[type=radio]:focus-visible, input[type=button]:focus-visible, input[type=submit]:focus-visible, input[type=file]:focus-visible { box-shadow: 0 0 0 2px @ACCENT@ }
"#;

/// Applied over the light sheets when the system is dark and the page says it supports it
/// (`color-scheme: dark` or `light dark` on the root, or `<meta name="color-scheme">`): the canvas,
/// text, links, popups, pickers and message boxes.
const DARK_BASE: &str = r#"
html { background: #1f1f1f; color: #f0f0f0 }
a { color: #8ab4f8 }
a:visited { color: #c58af9 }
popup, menu.popup, menu.context, popup > menu { background: #2b2b2b; color: #f0f0f0; border-color: #4a4a4a }
popup[role=tooltip] { background: #3a3a3a }
popup > menu, popup > menu.popup, popup > menu.context { border: 0 }
menu.popup > li[disabled], menu.context > li[disabled], popup > menu > li[disabled] { color: #888 }
.nile-picker { background: #2b2b2b; color: #f0f0f0; border-color: #4a4a4a }
.nile-picker .np-dow { color: #aaa }
.nile-picker .np-out { color: #777 }
.nile-picker button[disabled] { color: #666 }
.nile-picker .np-foot { border-top-color: #444 }
.nile-picker .np-link { color: #8ab4f8 }
.nile-picker .np-preview { border-color: #555 }
.nile-messagebox { background: #2b2b2b; color: #f0f0f0; border-color: #4a4a4a }
.nile-messagebox-title { background: #333333; border-bottom-color: #444444 }
hr { border-top-color: #666666; border-bottom-color: #333333 }
fieldset { border-color: #555555 }
mark { background: #665c00; color: #ffffff }
splitter { background: #444444 }
"#;

const DARK_WINDOWS: &str = r#"
button, input, textarea, select { color: #ffffff }
button, input, select { background: #2d2d2d; border-color: #3a3a3a; border-bottom-color: #4d4d4d }
input, textarea { background: #2d2d2d; border-color: #3a3a3a; border-bottom-color: #9a9a9a }
input:focus, textarea:focus, select:focus { border-color: #3a3a3a; border-bottom-color: @ACCENT@; box-shadow: 0 1px 0 @ACCENT@ }
button:disabled, input:disabled, textarea:disabled, select:disabled { color: #7a7a7a; background: #262626; border-color: #303030 }
input[type=checkbox], input[type=radio] { background: #2d2d2d; border-color: #9a9a9a }
input[type=checkbox]:checked, input[type=radio]:checked { background: @ACCENT@; border-color: @ACCENT@; color: @ACCENT_TEXT@ }
input[type=file], input[type=button], input[type=submit], input[type=reset] { background: #2d2d2d; border-color: #3a3a3a; border-bottom-color: #4d4d4d }
button:hover:not(:disabled), select:hover:not(:disabled), input[type=button]:hover:not(:disabled), input[type=submit]:hover:not(:disabled), input[type=reset]:hover:not(:disabled), input[type=file]:hover:not(:disabled) { background: #323232; border-color: #3a3a3a; border-bottom-color: #4d4d4d }
button:active:not(:disabled), select:active:not(:disabled), input[type=button]:active:not(:disabled), input[type=submit]:active:not(:disabled), input[type=reset]:active:not(:disabled), input[type=file]:active:not(:disabled) { background: #272727; color: #cfcfcf }
input:hover:not(:disabled):not(:focus):not([type=checkbox]):not([type=radio]):not([type=button]):not([type=submit]):not([type=reset]):not([type=range]):not([type=file]), textarea:hover:not(:disabled):not(:focus) { background: #323232 }
input[type=checkbox]:hover:not(:checked):not(:disabled), input[type=radio]:hover:not(:checked):not(:disabled) { border-color: #ffffff }
button:focus-visible, select:focus-visible, input[type=checkbox]:focus-visible, input[type=radio]:focus-visible, input[type=button]:focus-visible, input[type=submit]:focus-visible, input[type=file]:focus-visible { box-shadow: 0 0 0 2px #202020, 0 0 0 4px #ffffff }
"#;

const DARK_MACOS: &str = r#"
button, input, textarea, select { color: #ffffff }
button, select { background: #636366; border-color: #6e6e72; border-bottom-color: #6e6e72; box-shadow: 0 1px 1px rgba(0,0,0,0.35) }
input, textarea { background: #1e1e1e; border-color: #4a4a4a; border-bottom-color: #4a4a4a }
input:focus, textarea:focus, select:focus { border-color: @ACCENT@; box-shadow: 0 0 0 3px @ACCENT_A@ }
button:disabled, input:disabled, textarea:disabled, select:disabled { color: #777777; background: #2a2a2a; border-color: #3a3a3a; box-shadow: none }
input[type=checkbox], input[type=radio] { background: #1e1e1e; border-color: #6a6a6a }
input[type=checkbox]:checked, input[type=radio]:checked { background: @ACCENT@; border-color: @ACCENT@; color: @ACCENT_TEXT@ }
input[type=file], input[type=button], input[type=submit], input[type=reset] { background: #636366 }
button:hover:not(:disabled), select:hover:not(:disabled), input[type=button]:hover:not(:disabled), input[type=submit]:hover:not(:disabled), input[type=reset]:hover:not(:disabled), input[type=file]:hover:not(:disabled) { background: #6c6c70 }
button:active:not(:disabled), select:active:not(:disabled), input[type=button]:active:not(:disabled), input[type=submit]:active:not(:disabled), input[type=reset]:active:not(:disabled), input[type=file]:active:not(:disabled) { background: #58585b }
input[type=checkbox]:hover:not(:checked):not(:disabled), input[type=radio]:hover:not(:checked):not(:disabled) { border-color: #9a9a9a }
button:focus-visible, select:focus-visible, input[type=checkbox]:focus-visible, input[type=radio]:focus-visible, input[type=button]:focus-visible, input[type=submit]:focus-visible, input[type=file]:focus-visible { box-shadow: 0 0 0 3px @ACCENT_A@ }
"#;

const DARK_LINUX: &str = r#"
button, input, textarea, select { color: #ffffff }
button, select { background: #3d3d3d; border-color: #1b1b1b; border-bottom-color: #1b1b1b }
input, textarea { background: #2b2b2b; border-color: #1b1b1b; border-bottom-color: #1b1b1b }
input:focus, textarea:focus, select:focus { border-color: @ACCENT@; box-shadow: 0 0 0 2px @ACCENT_A@ }
button:disabled, input:disabled, textarea:disabled, select:disabled { color: #8a8a8a; background: #333333; border-color: #262626 }
input[type=checkbox], input[type=radio] { background: #2b2b2b; border-color: #5a5a5a }
input[type=checkbox]:checked, input[type=radio]:checked { background: @ACCENT@; border-color: @ACCENT@; color: @ACCENT_TEXT@ }
input[type=file], input[type=button], input[type=submit], input[type=reset] { background: #3d3d3d }
button:hover:not(:disabled), select:hover:not(:disabled), input[type=button]:hover:not(:disabled), input[type=submit]:hover:not(:disabled), input[type=reset]:hover:not(:disabled), input[type=file]:hover:not(:disabled) { background: #454545 }
button:active:not(:disabled), select:active:not(:disabled), input[type=button]:active:not(:disabled), input[type=submit]:active:not(:disabled), input[type=reset]:active:not(:disabled), input[type=file]:active:not(:disabled) { background: #333333 }
input[type=checkbox]:hover:not(:checked):not(:disabled), input[type=radio]:hover:not(:checked):not(:disabled) { border-color: #8a8a8a }
button:focus-visible, select:focus-visible, input[type=checkbox]:focus-visible, input[type=radio]:focus-visible, input[type=button]:focus-visible, input[type=submit]:focus-visible, input[type=file]:focus-visible { box-shadow: 0 0 0 2px @ACCENT@ }
"#;

/// Which platform's look is in use: the running platform's, or `NILE_THEME=windows|macos|linux`.
fn theme_platform() -> &'static str {
    static NAME: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    NAME.get_or_init(|| match std::env::var("NILE_THEME").ok().map(|t| t.to_ascii_lowercase()).as_deref() {
        Some("windows") => "windows",
        Some("macos") => "macos",
        Some("linux") => "linux",
        _ if cfg!(windows) => "windows",
        _ if cfg!(target_os = "macos") => "macos",
        _ => "linux",
    })
}

/// The user-agent sheet for the system theme: the base, the platform's controls, and when
/// `dark_scheme` the dark layer over both; `@ACCENT@` tokens become the system accent.
fn ua_sheet(dark_scheme: bool) -> Rc<Stylesheet> {
    type Key = (bool, bool, [u8; 3], &'static str);
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<Key, Rc<Stylesheet>>> = std::cell::RefCell::default();
    }
    let theme = system_theme();
    let platform = theme_platform();
    let key: Key = (dark_scheme, theme.dark, [theme.accent.r, theme.accent.g, theme.accent.b], platform);
    CACHE.with(|cache| {
        if let Some(hit) = cache.borrow().get(&key) {
            return hit.clone();
        }
        let (light, dark) = match platform {
            "windows" => (THEME_WINDOWS, DARK_WINDOWS),
            "macos" => (THEME_MACOS, DARK_MACOS),
            _ => (THEME_LINUX, DARK_LINUX),
        };
        let mut css = format!("{UA_STYLESHEET}{light}");
        if dark_scheme {
            css.push_str(DARK_BASE);
            css.push_str(dark);
        }
        let a = theme.accent;
        let css = css
            .replace("@ACCENT_TEXT@", &format!("#{:02x}{:02x}{:02x}", text_on(a).r, text_on(a).g, text_on(a).b))
            .replace("@ACCENT_A@", &format!("rgba({},{},{},0.4)", a.r, a.g, a.b))
            .replace("@ACCENT@", &format!("#{:02x}{:02x}{:02x}", a.r, a.g, a.b));
        let sheet = Rc::new(Stylesheet::parse(&css));
        cache.borrow_mut().insert(key, sheet.clone());
        sheet
    })
}

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
    /// `::marker` styles of list items that have one.
    markers: HashMap<NodeId, MarkerStyle>,
    /// `::-webkit-scrollbar*` rules of scroll containers that have any.
    scrollbars: HashMap<NodeId, ScrollbarParts>,
    page_bar: ScrollbarLook,
    /// Some element has a `prototype` (a script class bound from CSS).
    pub has_prototypes: bool,
    /// The dark user-agent sheet is in use (the system is dark and the page supports it).
    pub dark: bool,
    /// `@keyframes` by name.
    pub(crate) keyframes: HashMap<String, Vec<parse::Frame>>,
    /// `@page` rules (selector, body) in source order.
    pages: Vec<(String, String)>,
    /// `@font-face` rules in source order.
    pub font_faces: Vec<FontFace>,
}

/// Which of `:hover`, `:active` and `:focus` appear in the stylesheets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateDeps {
    pub hover: bool,
    pub active: bool,
    pub focus: bool,
    /// A rule has a state selector followed by a sibling combinator (`:hover + b`): a change can reach
    /// the element's later siblings.
    pub sibling: bool,
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
                    d.focus |= css.contains(":focus") || css.contains(":owns-focus");
                    d.sibling |= (css.contains(":hover") || css.contains(":active") || css.contains(":focus")) && (css.contains(" + ") || css.contains(" ~ "));
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
    markers: HashMap<NodeId, MarkerStyle>,
    scrollbars: HashMap<NodeId, ScrollbarParts>,
}

/// What a `::marker` rule sets for a list item's marker.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerStyle {
    /// The marker's own style: the item's, with the rule's declarations on top.
    pub style: ComputedStyle,
    /// Colour set by the rule (otherwise the item's live colour is used).
    pub color: Option<Color>,
    /// `content`: `Some("")` hides the marker, text replaces it.
    pub content: Option<String>,
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
            a.border_radius_pct = other.border_radius_pct;
        }
        if color_is_paint {
            a.color = other.color;
        }
        a.background_size = other.background_size;
        a.background_position = other.background_position;
        a.background_repeat = other.background_repeat;
        a.box_shadow = other.box_shadow.clone();
        a.selection_bg = other.selection_bg;
        a.accent_color = other.accent_color;
        a.color_scheme = other.color_scheme;
        a.scrollbar_color = other.scrollbar_color;
        a.prototype = other.prototype.clone();
        a.transitions = other.transitions.clone();
        a.animations = other.animations.clone();
        a.transform_origin = other.transform_origin;
        a.cursor = other.cursor;
        a.popup_position = other.popup_position;
        a.break_before = other.break_before;
        a.break_after = other.break_after;
        a.break_inside = other.break_inside;
        a.popup_animation = other.popup_animation.clone();
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
            && self.markers == other.markers
            && self.scrollbars == other.scrollbars
            && self.styles.iter().zip(&other.styles).all(|(a, b)| a.same_layout_with(b, color_is_paint))
    }

    /// The page has `::before`/`::after` boxes.
    pub fn has_generated(&self) -> bool {
        !self.pseudo.is_empty()
    }

    /// The `@page` style of page `index` (0-based).
    pub fn page_style(&self, index: usize) -> PageStyle {
        page::resolve(&self.pages, index)
    }

    /// Some element has a `transition` or `animation`.
    pub fn animates(&self) -> bool {
        self.styles.iter().any(|s| !s.transitions.is_empty() || !s.animations.is_empty())
    }

    /// Cascade again only the subtrees under `roots`, in place, for a change of `state` that can't
    /// reach anywhere else (a pointer or focus change: the parent of each element whose state changed
    /// is a root, so siblings and descendants that selectors like `:hover + b` reach are included).
    /// Returns the styles the subtrees had before, or `None` (nothing touched) when this isn't
    /// possible: the document changed size or there are generated `::before`/`::after`/marker boxes
    /// (the caller does the full cascade then).
    pub fn recascade(&mut self, doc: &Document, state: &InteractionState, viewport: (f32, f32), roots: &[NodeId]) -> Option<Vec<(NodeId, ComputedStyle)>> {
        if self.styles.len() != doc.nodes.len() || !self.pseudo.is_empty() || !self.markers.is_empty() || !self.scrollbars.is_empty() {
            return None;
        }
        let (ua, author) = sheets_of(doc, viewport, self.dark);
        let mut caches = SelectorCaches::default();
        let mut ctx = MatchingContext::new(MatchingMode::Normal, None, &mut caches, QuirksMode::NoQuirks, NeedsSelectorFlags::No, MatchingForInvalidation::No);
        let uses_viewport = Cell::new(self.uses_viewport);
        let cx = CascadeInput { doc, state, ua: &ua, author: &author, viewport, uses_viewport: &uses_viewport, has_pseudo: false };
        let mut before = Vec::new();
        let mut generated = Generated::default();
        for &root in roots {
            before.extend(doc.descendants(root).into_iter().chain(std::iter::once(root)).map(|n| (n, self.styles[n.0].clone())));
            let inherited = match doc.node(root).parent {
                Some(p) => self.styles[p.0].clone(),
                None => ComputedStyle::default(),
            };
            cascade(&cx, &mut ctx, root, &inherited, &mut self.styles, &mut generated);
        }
        Some(before)
    }

    /// The generated `::before` / `::after` box of `element`, if it has one.
    /// How the scrollbars of `node` look: `scrollbar-width`/`scrollbar-color` and any
    /// `::-webkit-scrollbar*` rules (which win).
    pub fn scrollbar_look(&self, node: NodeId) -> ScrollbarLook {
        let s = &self.styles[node.0];
        let parts = self.scrollbars.get(&node);
        let thickness = parts.and_then(|p| p.thickness).unwrap_or(match s.scrollbar_width {
            ScrollbarWidth::Auto => SCROLLBAR_AUTO,
            ScrollbarWidth::Thin => SCROLLBAR_THIN,
            ScrollbarWidth::None => 0.0,
        });
        ScrollbarLook {
            thickness,
            thumb: parts.and_then(|p| p.thumb).or(s.scrollbar_color.map(|c| c.0)),
            thumb_radius: parts.and_then(|p| p.thumb_radius),
            track: parts.and_then(|p| p.track).or(s.scrollbar_color.map(|c| c.1)),
        }
    }

    /// Elements whose style names a `prototype`: `(element, class, file)`.
    pub fn prototypes(&self, doc: &Document) -> Vec<(NodeId, String, String)> {
        if !self.has_prototypes {
            return Vec::new();
        }
        doc.nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let (class, url) = self.styles.get(i)?.prototype.clone()?;
                matches!(n.kind, NodeKind::Element { .. }).then_some((NodeId(i), class, url))
            })
            .collect()
    }

    /// The look of the page's own scrollbar: `<html>`'s, else `<body>`'s when `<html>` has none set.
    pub fn page_scrollbar(&self) -> ScrollbarLook {
        self.page_bar
    }

    /// The `::marker` rules of a list item, if any.
    pub fn marker(&self, item: NodeId) -> Option<&MarkerStyle> {
        self.markers.get(&item)
    }

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

    /// The colours for what no stylesheet reaches (menus, bars, scroll thumbs).
    pub fn palette(&self) -> Palette {
        Palette::new(self.dark, system_theme().accent)
    }

    pub fn get(&self, id: NodeId) -> &ComputedStyle {
        &self.styles[id.0]
    }

    /// Like [`StyleMap::get`], `None` while there are no styles (or none for `id`).
    pub fn try_get(&self, id: NodeId) -> Option<&ComputedStyle> {
        self.styles.get(id.0)
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
    let map = compute_with_scheme(doc, state, viewport, false);
    // A dark system and a page that supports dark (the root's `color-scheme`, or a `<meta>`): the
    // dark user-agent sheet replaces the light one, so everything is computed again, only for such pages.
    if system_theme().dark {
        let root_dark = doc.find(|n| n.tag() == Some("html")).is_some_and(|h| map.styles[h.0].color_scheme.allows_dark());
        if root_dark || meta_allows_dark(doc) {
            return compute_with_scheme(doc, state, viewport, true);
        }
    }
    map
}

/// `<meta name="color-scheme" content="light dark">`.
fn meta_allows_dark(doc: &Document) -> bool {
    doc.nodes.iter().any(|n| {
        n.tag() == Some("meta")
            && n.attr("name").is_some_and(|v| v.trim().eq_ignore_ascii_case("color-scheme"))
            && n.attr("content").is_some_and(|c| ColorScheme::parse(c.split_whitespace().map(|w| w.to_ascii_lowercase()).collect::<Vec<_>>().iter().map(String::as_str)).allows_dark())
    })
}

fn compute_with_scheme(doc: &Document, state: &InteractionState, viewport: (f32, f32), dark: bool) -> StyleMap {
    let (ua, author) = sheets_of(doc, viewport, dark);

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
    let pages: Vec<(String, String)> = author.iter().flat_map(|s| s.pages.iter().cloned()).collect();
    let font_faces: Vec<FontFace> = author.iter().flat_map(|s| s.font_faces.iter().cloned()).collect();
    let has_pseudo = author.iter().any(|s| s.rules.iter().any(|r| r.selectors.slice().iter().any(|sel| sel.has_pseudo_element())));
    let state_deps = StateDeps::of(author.iter().map(|s| &**s).chain(std::iter::once(&*ua)));
    let cx = CascadeInput { doc, state, ua: &ua, author: &author, viewport, uses_viewport: &uses_viewport, has_pseudo };
    let mut generated = Generated::default();
    cascade(&cx, &mut ctx, Document::ROOT, &root_style, &mut styles, &mut generated);
    let mut map = StyleMap {
        styles,
        uses_viewport: uses_viewport.get(),
        state_deps,
        pseudo: generated.pseudo,
        origins: generated.origins,
        contents: generated.contents,
        markers: generated.markers,
        scrollbars: generated.scrollbars,
        keyframes,
        pages,
        font_faces,
        page_bar: ScrollbarLook { thickness: SCROLLBAR_AUTO, thumb: None, thumb_radius: None, track: None },
        has_prototypes: false,
        dark,
    };
    map.has_prototypes = map.styles.iter().any(|s| s.prototype.is_some());
    let default = map.page_bar;
    map.page_bar = [doc.find(|n| n.tag() == Some("html")), doc.body()].into_iter().flatten().map(|n| map.scrollbar_look(n)).find(|l| *l != default).unwrap_or(default);
    map
}

/// A parsed author sheet's cache key: source text, viewport size, dark system theme.
type SheetKey = (String, u32, u32, bool);

thread_local! {
    /// Parsed sheets by source text and viewport (`@media` is evaluated while parsing): a restyle
    /// after a hover shouldn't parse the same CSS again.
    static SHEETS: std::cell::RefCell<HashMap<SheetKey, Rc<Stylesheet>>> = std::cell::RefCell::default();
}

/// The user-agent sheet and the document's `<style>` sheets, parsed (or taken from the cache).
fn sheets_of(doc: &Document, viewport: (f32, f32), dark_scheme: bool) -> (Rc<Stylesheet>, Vec<Rc<Stylesheet>>) {
    let ua = ua_sheet(dark_scheme);
    let author = doc
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
            // `@media (prefers-color-scheme)` is evaluated while parsing, so the theme is part of the key.
            let key = (css, viewport.0.to_bits(), viewport.1.to_bits(), system_theme().dark);
            SHEETS.with(|cache| {
                let mut cache = cache.borrow_mut();
                if let Some(hit) = cache.get(&key) {
                    return hit.clone();
                }
                if cache.len() >= 64 {
                    cache.clear();
                }
                let sheet = Rc::new(Stylesheet::parse_with(&key.0, viewport));
                cache.insert(key, sheet.clone());
                sheet
            })
        })
        .collect();
    (ua, author)
}

struct CascadeInput<'a> {
    doc: &'a Document,
    state: &'a InteractionState,
    ua: &'a Stylesheet,
    author: &'a [Rc<Stylesheet>],
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
    for sheet in std::iter::once(cx.ua).chain(cx.author.iter().map(|s| &**s)) {
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

/// The `font` shorthand as its longhands: `[style] [weight] size[/line-height] family[, ...]`
/// (a declaration that isn't `font`, or one that doesn't parse, comes back unchanged / dropped).
fn expand_font(d: Declaration) -> Vec<Declaration> {
    if d.name != "font" {
        return vec![d];
    }
    let part = |name: &str, values: Vec<Value>| Declaration { name: name.into(), values, important: d.important, raw: String::new() };
    let (mut style, mut weight) = (None, None);
    let mut i = 0;
    let v = &d.values;
    let size = loop {
        let Some(x) = v.get(i) else { return Vec::new() };
        i += 1;
        match x {
            Value::Ident(k) if k == "normal" => {}
            Value::Ident(k) if k == "italic" || k == "oblique" => style = Some(x.clone()),
            Value::Ident(k) if matches!(k.as_str(), "bold" | "bolder" | "lighter") => weight = Some(x.clone()),
            Value::Ident(k) if matches!(k.as_str(), "small-caps" | "ultra-condensed" | "extra-condensed" | "condensed" | "semi-condensed" | "semi-expanded" | "expanded" | "extra-expanded" | "ultra-expanded") => {}
            Value::Number(n) if (1.0..=1000.0).contains(n) => weight = Some(x.clone()),
            Value::Px(_) | Value::Em(_) | Value::Rem(_) | Value::Percent(_) => break x.clone(),
            Value::Ident(k) => {
                let px = match k.as_str() {
                    "xx-small" => 9.0,
                    "x-small" => 10.0,
                    "small" => 13.0,
                    "medium" => 16.0,
                    "large" => 18.0,
                    "x-large" => 24.0,
                    "xx-large" => 32.0,
                    _ => return Vec::new(),
                };
                break Value::Px(px);
            }
            _ => return Vec::new(),
        }
    };
    let mut out = Vec::new();
    if let Some(x) = style {
        out.push(part("font-style", vec![x]));
    }
    out.push(part("font-weight", vec![weight.unwrap_or(Value::Ident("normal".into()))]));
    out.push(part("font-size", vec![size]));
    if matches!(v.get(i), Some(Value::Slash)) {
        if let Some(h) = v.get(i + 1) {
            out.push(part("line-height", vec![h.clone()]));
        }
        i += 2;
    }
    let family: Vec<Value> = v.get(i..).unwrap_or(&[]).to_vec();
    if !family.is_empty() {
        out.push(part("font-family", family));
    }
    out
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
            // `popup-*` values are kept as text for scripts; everything else is parsed now.
            let raw = if parse::keeps_raw(&d.name) { text.clone() } else { String::new() };
            Some(Declaration { values: parse::parse_values(&text, pcx), raw, ..(*d).clone() })
        })
        .flat_map(expand_font)
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
/// A list item's `::marker` rules, if any apply.
fn marker_style(cx: &CascadeInput, id: NodeId, element: &ComputedStyle, extra: &mut Generated) {
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
    let wanted = |p: &PseudoElement| *p == PseudoElement::Marker;
    ctx.pseudo_element_matching_fn = Some(&wanted);
    let decls = matching_decls(cx, &el, &mut ctx, true);
    if decls.is_empty() {
        return;
    }
    let mut style = ComputedStyle::inherit(element);
    style.display = Display::Inline;
    let pcx = Cx { viewport: cx.viewport, uses_viewport: cx.uses_viewport };
    let color_set = decls.iter().any(|d| d.name == "color");
    let applied = apply_declarations(&mut style, decls, element.font_size, &pcx);
    let declared = applied.iter().rev().find(|d| d.name == "content");
    let content = declared.map(|_| content_text(&applied, cx.doc, id).unwrap_or_default());
    let color = color_set.then_some(style.color);
    extra.markers.insert(id, MarkerStyle { style, color, content });
}

/// The `::-webkit-scrollbar*` rules of a scroll container (or the root, for the page's bars).
fn scrollbar_parts(cx: &CascadeInput, id: NodeId, element: &ComputedStyle, extra: &mut Generated) {
    let el = ElementRef::new(cx.doc, id, cx.state);
    let mut parts = ScrollbarParts::default();
    let mut any = false;
    for kind in [PseudoElement::Scrollbar, PseudoElement::ScrollbarThumb, PseudoElement::ScrollbarTrack] {
        let mut caches = SelectorCaches::default();
        let mut ctx = MatchingContext::new(MatchingMode::ForStatelessPseudoElement, None, &mut caches, QuirksMode::NoQuirks, NeedsSelectorFlags::No, MatchingForInvalidation::No);
        let wanted = |p: &PseudoElement| *p == kind;
        ctx.pseudo_element_matching_fn = Some(&wanted);
        let decls = matching_decls(cx, &el, &mut ctx, true);
        if decls.is_empty() {
            continue;
        }
        any = true;
        let mut style = ComputedStyle::inherit(element);
        style.display = Display::Block;
        let pcx = Cx { viewport: cx.viewport, uses_viewport: cx.uses_viewport };
        apply_declarations(&mut style, decls, element.font_size, &pcx);
        let px = |l: Length| if let Length::Px(v) = l { Some(v) } else { None };
        match kind {
            PseudoElement::Scrollbar => {
                // A vertical bar's thickness is its width, a horizontal one's its height.
                parts.thickness = if style.display == Display::None { Some(0.0) } else { px(style.width).or(px(style.height)) };
            }
            PseudoElement::ScrollbarThumb => {
                parts.thumb = style.background.filter(|c| c.a > 0);
                parts.thumb_radius = Some(style.border_radius.tl).filter(|r| *r > 0.0);
            }
            _ => parts.track = style.background.filter(|c| c.a > 0),
        }
    }
    if any {
        extra.scrollbars.insert(id, parts);
    }
}

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
/// The colour an `<input type=color>` holds (`#rrggbb`, black when unset or invalid).
fn swatch_color(doc: &Document, id: NodeId) -> Option<Color> {
    let n = doc.node(id);
    if n.tag() != Some("input") || !n.attr("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("color")) {
        return None;
    }
    let v = n.attr("value").unwrap_or("#000000").trim();
    Some(v.strip_prefix('#').and_then(color_from_hex).unwrap_or(Color::BLACK))
}

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
        style.swatch = swatch_color(doc, id);
        apply_declarations(&mut style, decls, parent.font_size, &pcx);
    }

    out[id.0] = style.clone();
    if cx.has_pseudo && matches!(node.kind, NodeKind::Element { .. }) {
        if style.display == Display::ListItem {
            marker_style(cx, id, &style, extra);
        }
        if style.overflow_x == OverflowKind::Scroll || style.overflow_y == OverflowKind::Scroll || matches!(node.tag(), Some("html" | "body")) {
            scrollbar_parts(cx, id, &style, extra);
        }
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

/// A reference point of `popup-position`: `top-left` … `bottom-right` or a numpad digit.
fn point_of(v: &Value) -> Option<u8> {
    match v {
        Value::Ident(i) => ["bottom-left", "bottom-center", "bottom-right", "middle-left", "middle-center", "middle-right", "top-left", "top-center", "top-right"]
            .iter()
            .position(|n| n == i)
            .map(|p| p as u8 + 1),
        Value::Number(n) if (1.0..=9.0).contains(n) && n.fract() == 0.0 => Some(*n as u8),
        _ => None,
    }
}

thread_local! {
    /// Set by [`apply`] when a declaration names a property it doesn't know (for `@supports`).
    static UNRECOGNISED: Cell<bool> = const { Cell::new(false) };
}

/// Whether `name: value` is a declaration this engine understands (what `@supports (name: value)` asks).
pub(crate) fn supports_declaration(name: &str, value: &str, viewport: (f32, f32)) -> bool {
    let used = Cell::new(false);
    let pcx = Cx { viewport, uses_viewport: &used };
    let decls = parse::parse_declarations(&format!("{name}: {value}"), &pcx);
    let Some(d) = decls.iter().find(|d| d.name == name.trim().to_ascii_lowercase()) else { return false };
    let mut style = ComputedStyle::default();
    UNRECOGNISED.with(|u| u.set(false));
    let fs = style.font_size;
    apply(&mut style, d, fs);
    !UNRECOGNISED.with(Cell::get)
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
                Some("scroll" | "auto" | "scroll-indicator") => Some(OverflowKind::Scroll),
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
                Some("spinner" | "number-input") => Appearance::Spinner,
                Some("swatch" | "color-well") => Appearance::Swatch,
                Some("calendar") => Appearance::Calendar,
                Some("clock") => Appearance::Clock,
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
        "width" => {
            if matches!(first.and_then(Value::ident), Some("fit-content" | "max-content" | "min-content")) {
                s.width_fit = true;
                s.width = Length::Auto;
            } else if let Some(v) = l(first) {
                s.width = v;
                s.width_fit = false;
            }
        }
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
                Some("relative") => Position::Relative,
                Some("sticky") => Position::Sticky,
                Some("absolute") => Position::Absolute,
                Some("fixed") => Position::Fixed,
                _ => return,
            }
        }
        "shape-outside" => s.shape_outside = ShapeOutside::parse(&d.raw),
        "shape-margin" => {
            if let Some(m) = first.and_then(|x| px(x, fs)).filter(|m| *m >= 0.0) {
                s.shape_margin = m;
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
        // `border-radius: <h-radii> [/ <v-radii>]`: one to four values, px or %; the vertical
        // radii after a `/` are read as the same as the horizontal ones.
        "border-radius" => {
            let parts: Vec<Option<(f32, f32)>> = v
                .iter()
                .take_while(|x| !matches!(x, Value::Slash))
                .map(|x| match x {
                    Value::Percent(pc) => Some((0.0, pc / 100.0)),
                    other => px(other, fs).map(|p| (p.max(0.0), 0.0)),
                })
                .collect();
            if let Some(all) = parts.iter().copied().collect::<Option<Vec<_>>>() {
                if let Some(e) = edges(&all) {
                    // `edges` maps top/right/bottom/left; corners go tl, tr, br, bl the same way.
                    s.border_radius = Corners { tl: e.top.0, tr: e.right.0, br: e.bottom.0, bl: e.left.0 };
                    s.border_radius_pct = Corners { tl: e.top.1, tr: e.right.1, br: e.bottom.1, bl: e.left.1 };
                }
            }
        }
        "border-top-left-radius" | "border-top-right-radius" | "border-bottom-right-radius" | "border-bottom-left-radius" => {
            let one = match first {
                Some(Value::Percent(pc)) => Some((0.0, pc / 100.0)),
                other => p(other).map(|x| (x.max(0.0), 0.0)),
            };
            if let Some((fixed, pct)) = one {
                let (r, q) = (&mut s.border_radius, &mut s.border_radius_pct);
                let (a, b) = match d.name.as_str() {
                    "border-top-left-radius" => (&mut r.tl, &mut q.tl),
                    "border-top-right-radius" => (&mut r.tr, &mut q.tr),
                    "border-bottom-right-radius" => (&mut r.br, &mut q.br),
                    _ => (&mut r.bl, &mut q.bl),
                };
                *a = fixed;
                *b = pct;
            }
        }
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
        "break-before" | "page-break-before" | "break-after" | "page-break-after" | "break-inside" | "page-break-inside" => {
            let kind = match first.and_then(Value::ident) {
                Some("avoid" | "avoid-page") => Break::Avoid,
                Some("page" | "always" | "left" | "right" | "recto" | "verso") => Break::Always,
                _ => Break::Auto,
            };
            if d.name.ends_with("before") {
                s.break_before = kind;
            } else if d.name.ends_with("after") {
                s.break_after = kind;
            } else {
                s.break_inside = kind;
            }
        }
        "page-view" => s.page_view = first.and_then(Value::ident) == Some("paged"),
        // Sciter's way to make an element's text selectable.
        "behavior" => {
            if v.iter().any(|x| x.ident() == Some("htmlarea")) {
                s.user_select = UserSelect::Text;
            }
        }
        "backface-visibility" => s.backface_hidden = first.and_then(Value::ident) == Some("hidden"),
        "transform-style" => s.preserve_3d = first.and_then(Value::ident) == Some("preserve-3d"),
        "scrollbar-width" => {
            s.scrollbar_width = match first.and_then(Value::ident) {
                Some("thin") => ScrollbarWidth::Thin,
                Some("none") => ScrollbarWidth::None,
                _ => ScrollbarWidth::Auto,
            }
        }
        "scrollbar-color" => {
            s.scrollbar_color = match (v.first().and_then(color), v.get(1).and_then(color)) {
                (Some(thumb), Some(track)) => Some((thumb, track)),
                _ => None,
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
        "perspective" => match first {
            Some(Value::Ident(i)) if i == "none" => s.perspective = None,
            Some(x) => {
                if let Some(d) = px(x, fs).filter(|d| *d > 0.0) {
                    s.perspective = Some(d);
                }
            }
            None => {}
        },
        "popup-position" => {
            if let (Some(a), Some(b)) = (v.first().and_then(point_of), v.get(1).and_then(point_of)) {
                s.popup_position = Some((a, b));
            }
        }
        "popup-reference-point" => {
            if let Some(a) = first.and_then(point_of) {
                s.popup_position = Some((a, s.popup_position.map_or(1, |p| p.1)));
            }
        }
        "popup-anchor-reference-point" => {
            if let Some(b) = first.and_then(point_of) {
                s.popup_position = Some((s.popup_position.map_or(7, |p| p.0), b));
            }
        }
        "popup-animation" => {
            s.popup_animation = (!d.raw.is_empty() && d.raw != "none").then(|| d.raw.clone());
        }
        "perspective-origin" => {
            if let Some(o) = bg_position(&v[..v.len().min(2)], fs) {
                s.perspective_origin = o;
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
        "color-scheme" => s.color_scheme = ColorScheme::parse(v.iter().filter_map(Value::ident)),
        "accent-color" => {
            s.accent_color = match first {
                Some(x) if x.ident() == Some("auto") => None,
                Some(x) => x.color().or(s.accent_color),
                None => s.accent_color,
            }
        }
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
            // The whole list, as CSS text Parley understands (a missing family falls through to the next).
            let mut families: Vec<String> = Vec::new();
            let mut words: Vec<&str> = Vec::new();
            let quote = |n: &str| format!("\"{}\"", n.replace('\\', "\\\\").replace('"', "\\\""));
            let flush = |words: &mut Vec<&str>, families: &mut Vec<String>| {
                if !words.is_empty() {
                    let name = words.join(" ");
                    families.push(if words.len() > 1 { quote(&name) } else { name });
                    words.clear();
                }
            };
            for x in v {
                match x {
                    Value::Ident(i) => words.push(i),
                    Value::Str(n) => {
                        flush(&mut words, &mut families);
                        families.push(quote(n));
                    }
                    Value::Comma => flush(&mut words, &mut families),
                    _ => {}
                }
            }
            flush(&mut words, &mut families);
            if !families.is_empty() {
                s.font_family = families.join(", ");
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
        // The class name keeps its case, so it is read from the text: `Tabs url(tabs.js)`.
        "prototype" => {
            let text = d.raw.trim();
            let class = text.split_whitespace().next().filter(|c| !c.starts_with("url(") && *c != "none");
            let url = text.find("url(").and_then(|i| text[i + 4..].split(')').next()).map(|u| u.trim().trim_matches(|c| c == '"' || c == '\'').to_string()).unwrap_or_default();
            s.prototype = class.map(|c| (c.to_string(), url));
        }
        "font-variation-settings" => s.font_variations = if d.raw.trim() == "normal" { String::new() } else { d.raw.trim().to_string() },
        "font-feature-settings" => s.font_features = if d.raw.trim() == "normal" { String::new() } else { d.raw.trim().to_string() },
        "list-style-image" => {
            s.list_style_image = v.iter().find_map(|x| match x {
                Value::Url(u) => Some(u.clone()),
                _ => None,
            })
        }
        "list-style-position" => match first.and_then(Value::ident) {
            Some("inside") => s.list_style_inside = true,
            Some("outside") => s.list_style_inside = false,
            _ => {}
        },
        // `list-style: <type> || <position> || <image>`.
        "list-style" => {
            s.list_style_image = v.iter().find_map(|x| match x {
                Value::Url(u) => Some(u.clone()),
                _ => None,
            });
            for pos in v.iter().filter_map(Value::ident) {
                match pos {
                    "inside" => s.list_style_inside = true,
                    "outside" => s.list_style_inside = false,
                    _ => {}
                }
            }
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
            if !(anim::apply_properties(s, &d.name, v) || grid::apply(s, &d.name, v, fs)) {
                UNRECOGNISED.with(|u| u.set(true));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_faces_supports_and_family_lists_parse() {
        let css = r#"
            @font-face { font-family: "My Font"; src: local(Arial), url("a/b.woff2") format("woff2"), url(c.ttf); font-weight: bold; font-style: italic }
            @font-face { font-family: Var; src: url('v.woff2'); font-weight: 100 900 }
            @font-face { font-family: NoSource; src: local(Arial) }
            @font-face { src: url(x.ttf) }
            @supports (display: grid) { .a { color: red } }
            @supports not (display: grid) { .b { color: red } }
            @supports (display: flex) and (not-a-property: 1) { .c { color: red } }
            @supports (not-a-property: 1) or (position: sticky) { .d { color: red } }
            @supports not ((not-a-property: 1) or (also-not: x)) { .e { color: red } }
            .f { font-family: 'Segoe UI', "Helvetica Neue", Arial Black, sans-serif }
        "#;
        let sheet = Stylesheet::parse(css);
        assert_eq!(sheet.font_faces.len(), 2, "{:?}", sheet.font_faces);
        assert_eq!(sheet.font_faces[0], parse::FontFace { family: "My Font".into(), urls: vec!["a/b.woff2".into(), "c.ttf".into()], weight: Some(700.0), italic: Some(true) });
        assert_eq!((sheet.font_faces[1].weight, sheet.font_faces[1].italic), (None, None));
        let selectors: Vec<String> = sheet.rules.iter().map(|r| format!("{:?}", r.declarations.first().map(|d| d.name.clone()))).collect();
        // `.a`, `.d` and `.e` hold; `.b` and `.c` don't; `.f` is always there.
        assert_eq!(selectors.len(), 4, "{selectors:?}");
        let doc = Document::parse(r#"<style>.f { font-family: 'Segoe UI', "Helvetica Neue", Arial Black, sans-serif }</style><p class="f" id="p"></p>"#);
        let styles = compute_styles(&doc);
        let p = styles.get(doc.find(|n| n.attr("id") == Some("p")).unwrap());
        assert_eq!(p.font_family, r#""Segoe UI", "Helvetica Neue", "arial black", sans-serif"#);
    }

    #[test]
    fn list_style_image_and_backface_properties_parse_and_inherit() {
        let doc = Document::parse(
            r#"<style>ul { list-style-image: url(a.png) } .s { list-style: square inside url('b.png') } .n { list-style-image: none }
               .h { backface-visibility: hidden; transform-style: preserve-3d }</style>
               <ul id="u"><li id="i"></li><li id="s" class="s"></li><li id="n" class="n"></li></ul><div id="h" class="h"></div>"#,
        );
        let styles = compute_styles(&doc);
        let image = |id: &str| style_of(&doc, &styles, id).list_style_image.clone();
        assert_eq!(image("u").as_deref(), Some("a.png"));
        assert_eq!(image("i").as_deref(), Some("a.png"), "inherited");
        assert_eq!(image("s").as_deref(), Some("b.png"), "the shorthand's picture");
        assert_eq!(image("n"), None, "none clears it");
        let h = style_of(&doc, &styles, "h");
        assert!(h.backface_hidden && h.preserve_3d);
    }

    #[test]
    fn prototype_names_a_class_and_a_file() {
        let doc = Document::parse(r#"<style>tabs { prototype: Tabs url(tabs.js) } .n { prototype: MyThing } .x { prototype: none }</style><tabs id="t" class="x"></tabs><b id="b" class="n"></b><i id="i"></i>"#);
        let styles = compute_styles(&doc);
        assert_eq!(style_of(&doc, &styles, "b").prototype, Some(("MyThing".into(), String::new())));
        assert_eq!(style_of(&doc, &styles, "t").prototype, None, "a later rule's none wins");
        assert!(styles.has_prototypes && style_of(&doc, &styles, "i").prototype.is_none());
        let doc = Document::parse(r#"<style>tabs { prototype: Tabs url(tabs.js) }</style><tabs id="t"></tabs>"#);
        let styles = compute_styles(&doc);
        assert_eq!(style_of(&doc, &styles, "t").prototype, Some(("Tabs".into(), "tabs.js".into())));
        assert_eq!(styles.prototypes(&doc).len(), 1);
    }

    #[test]
    fn the_font_shorthand_sets_its_longhands() {
        let doc = Document::parse(
            r#"<i id=a style="font: italic bold 20px/1.5 'My Font', serif"></i><i id=b style="font: 700 250px/1 sans-serif"></i>
               <i id=c style="font-size: 30px; font: small Arial"></i><i id=d style="font: 14px sans-serif; font-weight: 300"></i><i id=e style="font: bogus"></i>"#,
        );
        let styles = compute_styles(&doc);
        let st = |id: &str| styles.get(doc.find(|n| n.attr("id") == Some(id)).unwrap()).clone();
        let a = st("a");
        assert!(a.italic && a.font_weight == 700.0 && a.font_size == 20.0 && (a.line_height - 1.5).abs() < 1e-6, "{:?}", (a.italic, a.font_weight, a.font_size, a.line_height));
        assert_eq!(a.font_family, r#""My Font", serif"#);
        let b = st("b");
        assert_eq!((b.font_weight, b.font_size, b.line_height, b.font_family.as_str()), (700.0, 250.0, 1.0, "sans-serif"));
        let c = st("c");
        assert_eq!((c.font_size, c.font_family.as_str()), (13.0, "arial"));
        assert_eq!(st("d").font_weight, 300.0, "a later longhand wins");
        assert_eq!(st("e").font_size, ComputedStyle::default().font_size, "an invalid shorthand is ignored");
    }

    #[test]
    fn border_radius_takes_one_to_four_corners_and_percent() {
        let doc = Document::parse(
            r#"<i id=a style="border-radius: 1px 2px 3px 4px"></i><i id=b style="border-radius: 5px 6px"></i>
               <i id=c style="border-radius: 50%; border-bottom-left-radius: 9px"></i><i id=d style="border-radius: 3px / 8px"></i>"#,
        );
        let styles = compute_styles(&doc);
        let r = |id: &str| styles.get(doc.find(|n| n.attr("id") == Some(id)).unwrap()).clone();
        assert_eq!(r("a").border_radius, Corners { tl: 1.0, tr: 2.0, br: 3.0, bl: 4.0 });
        assert_eq!(r("b").border_radius, Corners { tl: 5.0, tr: 6.0, br: 5.0, bl: 6.0 });
        let c = r("c");
        assert_eq!(c.corner_radii(100.0, 50.0), [(50.0, 25.0), (50.0, 25.0), (50.0, 25.0), (9.0, 9.0)]);
        assert_eq!(r("d").border_radius, Corners::all(3.0));
    }

    #[test]
    fn inline_vertical_align_parses() {
        let doc = Document::parse(r#"<sup id=a>1</sup><sub id=b>2</sub><span id=c style="vertical-align: 3px">3</span>"#);
        let styles = compute_styles(&doc);
        let va = |tag: &str| styles.get(doc.find(|n| n.attr("id") == Some(tag)).unwrap()).vertical_align;
        assert_eq!(va("a"), VerticalAlign::Super);
        assert_eq!(va("b"), VerticalAlign::Sub);
        assert_eq!(va("c"), VerticalAlign::Shift(3.0));
    }

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
        assert!(Query::parse("li::first-line").is_err());
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
    fn popup_properties_are_kept_for_scripts() {
        let doc = Document::parse(r#"<style>#a { popup-position: top-right bottom-center; popup-animation: popup-animation(type:slide, duration:100ms) } #b { popup-reference-point: 5 }</style><p id="a"></p><p id="b"></p>"#);
        let s = compute_styles(&doc);
        let a = s.get(doc.find(|n| n.attr("id") == Some("a")).unwrap()).clone();
        assert_eq!(a.popup_position, Some((9, 2)));
        assert_eq!(a.css_value("popup-position").as_deref(), Some("top-right bottom-center"));
        assert!(a.popup_animation.as_deref().is_some_and(|t| t.contains("type:slide")), "{:?}", a.popup_animation);
        let b = s.get(doc.find(|n| n.attr("id") == Some("b")).unwrap()).clone();
        assert_eq!(b.popup_position, Some((5, 1)));
    }

    #[test]
    fn state_deps_find_the_pseudo_classes_rules_mention() {
        let deps = |css: &str| compute_styles(&Document::parse(&format!("<style>{css}</style><p>x</p>"))).state_deps;
        // The UA sheet mentions :hover, :active and :focus (the controls' states), so every page has them.
        assert_eq!(deps("p { color: red }"), StateDeps { hover: true, active: true, focus: true, sibling: false });
        assert!(deps("a:hover { color: red }").hover);
        assert!(deps(".x:not(:hover) b { color: red }").hover);
        assert!(deps("button:active { color: red }").active);
        let d = StateDeps { hover: false, active: false, focus: true, sibling: false };
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

#[cfg(test)]
mod theme_tests {
    use super::*;

    fn style_of(html: &str, id: &str) -> ComputedStyle {
        let doc = Document::parse(html);
        let styles = compute_styles(&doc);
        let node = doc.find(|n| n.attr("id") == Some(id)).expect("node");
        styles.get(node).clone()
    }

    #[test]
    fn system_colours_and_prefers_color_scheme_follow_the_theme() {
        let page = r#"<style>
            #a { color: CanvasText; background: Canvas }
            #b { color: AccentColor }
            @media (prefers-color-scheme: dark) { #c { color: #123456 } }
            #c { background: ButtonFace }
        </style><body><p id="a"></p><p id="b"></p><p id="c"></p></body>"#;
        set_system_theme(SystemTheme { dark: false, accent: Color::rgb(10, 20, 30) });
        assert_eq!(style_of(page, "a").color, Color::rgb(0, 0, 0));
        assert_eq!(style_of(page, "a").background, Some(Color::rgb(255, 255, 255)));
        assert_eq!(style_of(page, "b").color, Color::rgb(10, 20, 30));
        assert_ne!(style_of(page, "c").color, Color::rgb(0x12, 0x34, 0x56));
        set_system_theme(SystemTheme { dark: true, accent: Color::rgb(10, 20, 30) });
        assert_eq!(style_of(page, "a").color, Color::rgb(240, 240, 240));
        assert_eq!(style_of(page, "c").color, Color::rgb(0x12, 0x34, 0x56));
        assert_eq!(style_of(page, "c").background, Some(Color::rgb(58, 58, 58)));
        set_system_theme(SystemTheme::default());
    }

    #[test]
    fn a_page_that_supports_dark_gets_the_dark_user_agent_sheet() {
        set_system_theme(SystemTheme { dark: true, accent: Color::rgb(200, 30, 30) });
        let plain = "<body><button id=\"b\">x</button><input id=\"i\" type=\"checkbox\" checked></body>";
        let opted = "<style>:root { color-scheme: light dark }</style><body><button id=\"b\">x</button><input id=\"i\" type=\"checkbox\" checked></body>";
        let meta = "<meta name=\"color-scheme\" content=\"dark\"><body><button id=\"b\">x</button></body>";
        // Without the opt-in the page stays light, even on a dark system.
        assert_eq!(style_of(plain, "b").color, Color::rgb(0, 0, 0));
        // With it the controls turn dark and the checked box takes the system accent.
        assert_eq!(style_of(opted, "b").color, Color::rgb(255, 255, 255));
        assert_eq!(style_of(opted, "i").background, Some(Color::rgb(200, 30, 30)));
        assert_eq!(style_of(meta, "b").color, Color::rgb(255, 255, 255));
        // A light system ignores the opt-in.
        set_system_theme(SystemTheme { dark: false, accent: Color::rgb(200, 30, 30) });
        assert_eq!(style_of(opted, "b").color, Color::rgb(0, 0, 0));
        assert_eq!(style_of(opted, "i").background, Some(Color::rgb(200, 30, 30)));
        set_system_theme(SystemTheme::default());
    }

    #[test]
    fn accent_color_and_color_scheme_are_inherited() {
        let page = r#"<style>#p { accent-color: #ff0000; color-scheme: dark } #c { accent-color: auto }</style><body><div id="p"><span id="k"></span><span id="c"></span></div></body>"#;
        assert_eq!(style_of(page, "k").accent_color, Some(Color::rgb(255, 0, 0)));
        assert_eq!(style_of(page, "k").color_scheme, ColorScheme::Dark);
        assert_eq!(style_of(page, "c").accent_color, None);
    }
}
