//! CSS grid values: track lists, line placement, areas. `apply` handles the grid
//! properties; layout turns the results into Taffy's grid style.

use crate::parse::{RepeatKind, Value};
use crate::{length, px, Align, ComputedStyle, Cursor, Length, VerticalAlign};

/// The size of a track's min or max.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Breadth {
    Px(f32),
    /// A fraction (50% = 0.5).
    Percent(f32),
    /// Flexible, `1fr`. Only valid as a maximum.
    Fr(f32),
    Auto,
    MinContent,
    MaxContent,
}

/// One track, `minmax(min, max)`; a plain size is `minmax(size, size)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Track {
    pub min: Breadth,
    pub max: Breadth,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackRepeat {
    Count(u16),
    AutoFill,
    AutoFit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrackItem {
    Track(Track),
    Repeat(TrackRepeat, Vec<Track>),
}

/// One end of an item's placement in a row or column.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum GridLine {
    #[default]
    Auto,
    /// 1-based line number; negative counts from the end.
    Line(i16),
    Span(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GridPlace {
    pub start: GridLine,
    pub end: GridLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridAutoFlow {
    pub column: bool,
    pub dense: bool,
}

fn breadth(v: &Value, fs: f32) -> Option<Breadth> {
    Some(match v {
        Value::Fr(f) => Breadth::Fr(*f),
        Value::Ident(i) => match i.as_str() {
            "auto" => Breadth::Auto,
            "min-content" => Breadth::MinContent,
            "max-content" => Breadth::MaxContent,
            _ => return None,
        },
        other => match length(other, fs)? {
            Length::Px(p) => Breadth::Px(p),
            Length::Percent(p) => Breadth::Percent(p),
            _ => return None,
        },
    })
}

fn track(v: &Value, fs: f32) -> Option<Track> {
    if let Value::Minmax(pair) = v {
        let min = breadth(&pair.0, fs)?;
        let max = breadth(&pair.1, fs)?;
        return Some(Track { min: if matches!(min, Breadth::Fr(_)) { Breadth::Auto } else { min }, max });
    }
    let b = breadth(v, fs)?;
    Some(match b {
        Breadth::Fr(_) => Track { min: Breadth::Auto, max: b },
        _ => Track { min: b, max: b },
    })
}

fn tracks(vals: &[Value], fs: f32) -> Option<Vec<Track>> {
    vals.iter().map(|v| track(v, fs)).collect()
}

fn items(vals: &[Value], fs: f32) -> Option<Vec<TrackItem>> {
    if vals.first().and_then(Value::ident) == Some("none") {
        return Some(Vec::new());
    }
    vals.iter()
        .map(|v| match v {
            Value::Repeat(rep) => {
                let kind = match rep.0 {
                    RepeatKind::Count(n) => TrackRepeat::Count(n),
                    RepeatKind::AutoFill => TrackRepeat::AutoFill,
                    RepeatKind::AutoFit => TrackRepeat::AutoFit,
                };
                Some(TrackItem::Repeat(kind, tracks(&rep.1, fs)?))
            }
            other => track(other, fs).map(TrackItem::Track),
        })
        .collect()
}

/// `1`, `-1`, `span 2`, `auto`.
fn line(part: &[Value]) -> Option<GridLine> {
    Some(match part {
        [Value::Number(n)] if n.fract() == 0.0 && *n != 0.0 => GridLine::Line(*n as i16),
        [Value::Ident(s), Value::Number(n)] | [Value::Number(n), Value::Ident(s)] if s == "span" && *n >= 1.0 => {
            GridLine::Span(*n as u16)
        }
        [Value::Ident(s)] if s == "span" => GridLine::Span(1),
        [Value::Ident(s)] if s == "auto" => GridLine::Auto,
        _ => return None,
    })
}

/// `a`, `a / b`.
fn place(vals: &[Value]) -> Option<GridPlace> {
    let mut parts = vals.split(|v| *v == Value::Slash);
    let start = line(parts.next()?)?;
    let end = match parts.next() {
        Some(p) => line(p)?,
        None => GridLine::Auto,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(GridPlace { start, end })
}

fn align_of(v: Option<&Value>) -> Option<Align> {
    crate::align(v?)
}

/// Applies a grid-related declaration; false if `name` isn't one.
pub(crate) fn apply(s: &mut ComputedStyle, name: &str, v: &[Value], fs: f32) -> bool {
    let first = v.first();
    match name {
        "grid-template-columns" => {
            if let Some(t) = items(v, fs) {
                s.grid_template_columns = t;
            }
        }
        "grid-template-rows" => {
            if let Some(t) = items(v, fs) {
                s.grid_template_rows = t;
            }
        }
        "grid-auto-columns" => {
            if let Some(t) = tracks(v, fs) {
                s.grid_auto_columns = t;
            }
        }
        "grid-auto-rows" => {
            if let Some(t) = tracks(v, fs) {
                s.grid_auto_rows = t;
            }
        }
        "grid-auto-flow" => {
            let mut flow = GridAutoFlow::default();
            for x in v {
                match x.ident() {
                    Some("column") => flow.column = true,
                    Some("row") => {}
                    Some("dense") => flow.dense = true,
                    _ => return true,
                }
            }
            s.grid_auto_flow = flow;
        }
        "grid-template-areas" => {
            if first.and_then(Value::ident) == Some("none") {
                s.grid_areas.clear();
            } else {
                let rows: Vec<Vec<String>> = v
                    .iter()
                    .filter_map(|x| match x {
                        Value::Str(row) => Some(row.split_whitespace().map(str::to_string).collect()),
                        _ => None,
                    })
                    .collect();
                // Every row needs the same number of cells.
                if !rows.is_empty() && rows.iter().all(|r| r.len() == rows[0].len() && !r.is_empty()) {
                    s.grid_areas = rows;
                }
            }
        }
        "grid-column" => {
            if let Some(p) = place(v) {
                s.grid_column = p;
            }
        }
        "grid-row" => {
            if let Some(p) = place(v) {
                s.grid_row = p;
            }
        }
        "grid-column-start" => s.grid_column.start = line(v).unwrap_or(s.grid_column.start),
        "grid-column-end" => s.grid_column.end = line(v).unwrap_or(s.grid_column.end),
        "grid-row-start" => s.grid_row.start = line(v).unwrap_or(s.grid_row.start),
        "grid-row-end" => s.grid_row.end = line(v).unwrap_or(s.grid_row.end),
        // `grid-area: name`, or `row-start / column-start / row-end / column-end`.
        "grid-area" => {
            if let [Value::Ident(n)] = v {
                if n != "auto" && n != "span" {
                    s.grid_area_name = Some(n.clone());
                    return true;
                }
            }
            let parts: Vec<GridLine> = v.split(|x| *x == Value::Slash).map(line).collect::<Option<_>>().unwrap_or_default();
            let at = |i: usize| parts.get(i).copied();
            if !parts.is_empty() {
                s.grid_row.start = parts[0];
                s.grid_column.start = at(1).unwrap_or_default();
                s.grid_row.end = at(2).unwrap_or_default();
                s.grid_column.end = at(3).unwrap_or_default();
            }
        }
        "row-gap" | "grid-row-gap" => s.row_gap = first.and_then(|x| px(x, fs)).unwrap_or(s.row_gap),
        "column-gap" | "grid-column-gap" => s.column_gap = first.and_then(|x| px(x, fs)).unwrap_or(s.column_gap),
        "gap" | "grid-gap" => {
            if let Some(r) = first.and_then(|x| px(x, fs)) {
                s.row_gap = r;
                s.column_gap = v.get(1).and_then(|x| px(x, fs)).unwrap_or(r);
            }
        }
        "border-spacing" => {
            if let Some(h) = first.and_then(|x| px(x, fs)) {
                s.border_spacing = (h, v.get(1).and_then(|x| px(x, fs)).unwrap_or(h));
            }
        }
        "cursor" => {
            // The first keyword we know wins; `url(...)` fallbacks are skipped.
            if let Some(c) = v.iter().filter_map(Value::ident).find_map(cursor_of) {
                s.cursor = c;
            }
        }
        "table-layout" => match first.and_then(Value::ident) {
            Some("fixed") => s.table_fixed = true,
            Some("auto") => s.table_fixed = false,
            _ => {}
        },
        "caption-side" => s.caption_bottom = matches!(first.and_then(Value::ident), Some("bottom" | "bottom-outside")),
        "border-collapse" => match first.and_then(Value::ident) {
            Some("collapse") => s.border_collapse = true,
            Some("separate") => s.border_collapse = false,
            _ => {}
        },
        "vertical-align" => {
            s.vertical_align = match first.and_then(Value::ident) {
                Some("top") => VerticalAlign::Top,
                Some("middle") => VerticalAlign::Middle,
                Some("bottom") => VerticalAlign::Bottom,
                Some("baseline") => VerticalAlign::Baseline,
                Some("sub") => VerticalAlign::Sub,
                Some("super") => VerticalAlign::Super,
                _ => match first.and_then(|v| length(v, s.font_size)) {
                    Some(Length::Px(px)) => VerticalAlign::Shift(px),
                    Some(Length::Percent(p)) => VerticalAlign::Shift(p * s.font_size * s.line_height.max(1.0)),
                    _ => s.vertical_align,
                },
            }
        }
        "justify-items" => s.justify_items = align_of(first).unwrap_or(s.justify_items),
        "justify-self" => s.justify_self = align_of(first).unwrap_or(s.justify_self),
        "align-self" => s.align_self = align_of(first).unwrap_or(s.align_self),
        "align-content" => s.align_content = align_of(first).unwrap_or(s.align_content),
        "place-items" => {
            if let Some(a) = align_of(first) {
                s.align_items = a;
                s.justify_items = align_of(v.get(1)).unwrap_or(a);
            }
        }
        "place-self" => {
            if let Some(a) = align_of(first) {
                s.align_self = a;
                s.justify_self = align_of(v.get(1)).unwrap_or(a);
            }
        }
        "place-content" => {
            if let Some(a) = align_of(first) {
                s.align_content = a;
                s.justify_content = align_of(v.get(1)).unwrap_or(a);
            }
        }
        _ => return false,
    }
    true
}

/// The rows/columns an area name covers, as 1-based lines (row start, row end, column
/// start, column end), from `grid-template-areas`.
pub fn area_lines(areas: &[Vec<String>], name: &str) -> Option<(i16, i16, i16, i16)> {
    let mut found: Option<(usize, usize, usize, usize)> = None;
    for (r, row) in areas.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if cell == name {
                found = Some(match found {
                    None => (r, r + 1, c, c + 1),
                    Some((r0, r1, c0, c1)) => (r0.min(r), r1.max(r + 1), c0.min(c), c1.max(c + 1)),
                });
            }
        }
    }
    found.map(|(r0, r1, c0, c1)| (r0 as i16 + 1, r1 as i16 + 1, c0 as i16 + 1, c1 as i16 + 1))
}

fn cursor_of(name: &str) -> Option<Cursor> {
    Some(match name {
        "auto" => Cursor::Auto,
        "default" => Cursor::Default,
        "pointer" => Cursor::Pointer,
        "text" | "vertical-text" => Cursor::Text,
        "crosshair" => Cursor::Crosshair,
        "move" | "all-scroll" => Cursor::Move,
        "not-allowed" | "no-drop" => Cursor::NotAllowed,
        "wait" => Cursor::Wait,
        "progress" => Cursor::Progress,
        "help" => Cursor::Help,
        "grab" => Cursor::Grab,
        "grabbing" => Cursor::Grabbing,
        "col-resize" | "ew-resize" | "e-resize" | "w-resize" => Cursor::ResizeH,
        "row-resize" | "ns-resize" | "n-resize" | "s-resize" => Cursor::ResizeV,
        "nwse-resize" | "nw-resize" | "se-resize" => Cursor::ResizeNwse,
        "nesw-resize" | "ne-resize" | "sw-resize" => Cursor::ResizeNesw,
        "none" => Cursor::None,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_rectangles() {
        let areas: Vec<Vec<String>> = ["h h h", "s m m", "f f f"].iter().map(|r| r.split(' ').map(String::from).collect()).collect();
        assert_eq!(area_lines(&areas, "h"), Some((1, 2, 1, 4)));
        assert_eq!(area_lines(&areas, "m"), Some((2, 3, 2, 4)));
        assert_eq!(area_lines(&areas, "x"), None);
    }
}
