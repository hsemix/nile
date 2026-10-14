//! CSS transitions and `@keyframes` animations.
//!
//! The cascade produces each element's *target* style. An [`Animator`], which the
//! embedder keeps between restyles, turns changes of that target into transitions and
//! runs animations, writing the in-between values over a copy of the styles. All times
//! are seconds on a clock the embedder chooses.

use std::collections::{HashMap, HashSet};

use crate::gradient::{StopPos, TransformFn};
use crate::parse::{Declaration, Value};
use crate::{apply, Color, ComputedStyle, Corners, Length, NodeId, StyleMap};

/// An easing curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Timing {
    Linear,
    /// `cubic-bezier(x1, y1, x2, y2)`.
    Bezier(f32, f32, f32, f32),
    /// `steps(n)`; true if the jump happens at the start of each step.
    Steps(u32, bool),
}

impl Timing {
    pub const EASE: Timing = Timing::Bezier(0.25, 0.1, 0.25, 1.0);

    /// `ease`, `linear`, `ease-in`, ... by keyword.
    pub fn keyword(name: &str) -> Option<Timing> {
        Some(match name {
            "linear" => Timing::Linear,
            "ease" => Timing::EASE,
            "ease-in" => Timing::Bezier(0.42, 0.0, 1.0, 1.0),
            "ease-out" => Timing::Bezier(0.0, 0.0, 0.58, 1.0),
            "ease-in-out" => Timing::Bezier(0.42, 0.0, 0.58, 1.0),
            "step-start" => Timing::Steps(1, true),
            "step-end" => Timing::Steps(1, false),
            _ => return None,
        })
    }

    /// Progress (0..=1) of the animation -> progress of the value.
    pub fn ease(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Timing::Linear => t,
            Timing::Steps(n, start) => {
                let n = n as f32;
                let step = if start { (t * n).ceil() } else { (t * n).floor() };
                (step / n).clamp(0.0, 1.0)
            }
            Timing::Bezier(x1, y1, x2, y2) => {
                let bez = |a: f32, b: f32, s: f32| 3.0 * (1.0 - s).powi(2) * s * a + 3.0 * (1.0 - s) * s * s * b + s.powi(3);
                // Find the curve parameter whose x is `t`: bisection, which is plenty
                // fast for a handful of values per frame.
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                for _ in 0..24 {
                    let mid = (lo + hi) / 2.0;
                    if bez(x1, x2, mid) < t {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                bez(y1, y2, (lo + hi) / 2.0)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransitionSpec {
    /// A property name, or `all`.
    pub property: String,
    pub duration: f32,
    pub delay: f32,
    pub timing: Timing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fill {
    #[default]
    None,
    Forwards,
    Backwards,
    Both,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimationSpec {
    pub name: String,
    pub duration: f32,
    pub delay: f32,
    pub timing: Timing,
    /// `f32::INFINITY` for `infinite`.
    pub iterations: f32,
    pub direction: Direction,
    pub fill: Fill,
}

impl AnimationSpec {
    fn new(name: &str) -> AnimationSpec {
        AnimationSpec {
            name: name.to_string(),
            duration: 0.0,
            delay: 0.0,
            timing: Timing::EASE,
            iterations: 1.0,
            direction: Direction::Normal,
            fill: Fill::None,
        }
    }
}

fn timing_of(v: &Value) -> Option<Timing> {
    match v {
        Value::Timing(t) => Some(*t),
        Value::Ident(i) => Timing::keyword(i),
        _ => None,
    }
}

fn groups(v: &[Value]) -> Vec<&[Value]> {
    v.split(|x| *x == Value::Comma).collect()
}

/// The `transition-*` and `animation-*` properties; false if `name` isn't one.
pub(crate) fn apply_properties(s: &mut ComputedStyle, name: &str, v: &[Value]) -> bool {
    match name {
        "transition" => {
            if v.first().and_then(Value::ident) == Some("none") {
                s.transitions.clear();
                return true;
            }
            let mut specs = Vec::new();
            for g in groups(v) {
                let mut spec = TransitionSpec { property: "all".into(), duration: 0.0, delay: 0.0, timing: Timing::EASE };
                let mut times = 0;
                for x in g {
                    if let Value::Time(t) = x {
                        if times == 0 {
                            spec.duration = t.max(0.0);
                        } else {
                            spec.delay = *t;
                        }
                        times += 1;
                    } else if let Some(t) = timing_of(x) {
                        spec.timing = t;
                    } else if let Some(i) = x.ident() {
                        spec.property = i.to_string();
                    }
                }
                specs.push(spec);
            }
            s.transitions = specs;
        }
        "transition-property" => {
            let props: Vec<&str> = v.iter().filter_map(Value::ident).collect();
            if props == ["none"] {
                s.transitions.clear();
            } else {
                let old = std::mem::take(&mut s.transitions);
                s.transitions = props
                    .iter()
                    .enumerate()
                    .map(|(i, p)| TransitionSpec { property: p.to_string(), ..old.get(i).or(old.last()).cloned().unwrap_or(TransitionSpec { property: String::new(), duration: 0.0, delay: 0.0, timing: Timing::EASE }) })
                    .collect();
            }
        }
        "transition-duration" | "transition-delay" | "transition-timing-function" => {
            if s.transitions.is_empty() {
                s.transitions.push(TransitionSpec { property: "all".into(), duration: 0.0, delay: 0.0, timing: Timing::EASE });
            }
            let items = groups(v);
            for (i, spec) in s.transitions.iter_mut().enumerate() {
                let item = items[i % items.len()].first();
                match (name, item) {
                    ("transition-duration", Some(Value::Time(t))) => spec.duration = t.max(0.0),
                    ("transition-delay", Some(Value::Time(t))) => spec.delay = *t,
                    ("transition-timing-function", Some(x)) => spec.timing = timing_of(x).unwrap_or(spec.timing),
                    _ => {}
                }
            }
        }
        "animation" => {
            if v.first().and_then(Value::ident) == Some("none") {
                s.animations.clear();
                return true;
            }
            let mut specs = Vec::new();
            for g in groups(v) {
                let mut spec = AnimationSpec::new("");
                let mut times = 0;
                for x in g {
                    match x {
                        Value::Time(t) => {
                            if times == 0 {
                                spec.duration = t.max(0.0);
                            } else {
                                spec.delay = *t;
                            }
                            times += 1;
                        }
                        Value::Number(n) => spec.iterations = n.max(0.0),
                        Value::Ident(i) => match i.as_str() {
                            "infinite" => spec.iterations = f32::INFINITY,
                            "normal" => spec.direction = Direction::Normal,
                            "reverse" => spec.direction = Direction::Reverse,
                            "alternate" => spec.direction = Direction::Alternate,
                            "alternate-reverse" => spec.direction = Direction::AlternateReverse,
                            "none" if spec.name.is_empty() => {}
                            "forwards" => spec.fill = Fill::Forwards,
                            "backwards" => spec.fill = Fill::Backwards,
                            "both" => spec.fill = Fill::Both,
                            "running" | "paused" => {}
                            other => match Timing::keyword(other) {
                                Some(t) => spec.timing = t,
                                None => spec.name = other.to_string(),
                            },
                        },
                        Value::Timing(t) => spec.timing = *t,
                        _ => {}
                    }
                }
                if !spec.name.is_empty() {
                    specs.push(spec);
                }
            }
            s.animations = specs;
        }
        "animation-name" => {
            let names: Vec<&str> = v.iter().filter_map(Value::ident).filter(|n| *n != "none").collect();
            let old = std::mem::take(&mut s.animations);
            s.animations = names
                .iter()
                .enumerate()
                .map(|(i, n)| AnimationSpec { name: n.to_string(), ..old.get(i).cloned().unwrap_or_else(|| AnimationSpec::new(n)) })
                .collect();
        }
        "animation-duration" | "animation-delay" | "animation-timing-function" | "animation-iteration-count" | "animation-direction" | "animation-fill-mode" => {
            let items = groups(v);
            for (i, spec) in s.animations.iter_mut().enumerate() {
                let item = items[i % items.len()].first();
                match (name, item) {
                    ("animation-duration", Some(Value::Time(t))) => spec.duration = t.max(0.0),
                    ("animation-delay", Some(Value::Time(t))) => spec.delay = *t,
                    ("animation-timing-function", Some(x)) => spec.timing = timing_of(x).unwrap_or(spec.timing),
                    ("animation-iteration-count", Some(Value::Number(n))) => spec.iterations = n.max(0.0),
                    ("animation-iteration-count", Some(Value::Ident(i))) if i == "infinite" => spec.iterations = f32::INFINITY,
                    ("animation-direction", Some(Value::Ident(i))) => {
                        spec.direction = match i.as_str() {
                            "reverse" => Direction::Reverse,
                            "alternate" => Direction::Alternate,
                            "alternate-reverse" => Direction::AlternateReverse,
                            _ => Direction::Normal,
                        }
                    }
                    ("animation-fill-mode", Some(Value::Ident(i))) => {
                        spec.fill = match i.as_str() {
                            "forwards" => Fill::Forwards,
                            "backwards" => Fill::Backwards,
                            "both" => Fill::Both,
                            _ => Fill::None,
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => return false,
    }
    true
}

/// A value of an animatable property.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimValue {
    Number(f32),
    Color(Color),
    Length(Length),
    Transform(Vec<TransformFn>),
}

const PROPS: &[&str] = &[
    "opacity", "color", "background-color", "border-top-color", "border-right-color", "border-bottom-color", "border-left-color",
    "width", "height", "min-width", "min-height", "max-width", "max-height",
    "margin-top", "margin-right", "margin-bottom", "margin-left",
    "padding-top", "padding-right", "padding-bottom", "padding-left",
    "border-top-width", "border-right-width", "border-bottom-width", "border-left-width", "border-radius",
    "top", "right", "bottom", "left", "font-size", "letter-spacing", "flex-grow", "flex-shrink", "row-gap", "column-gap", "transform",
];

/// Whether a `transition-property` entry covers the longhand `prop`: `all`, the same name, or a
/// shorthand (`background`, `margin`, `border-color`, ...) that includes it.
fn transitions_property(listed: &str, prop: &str) -> bool {
    listed == "all"
        || listed == prop
        || match listed {
            "background" => prop == "background-color",
            "margin" | "padding" => prop.starts_with(&format!("{listed}-")),
            "border-color" => prop.starts_with("border-") && prop.ends_with("-color"),
            "border-width" => prop.starts_with("border-") && prop.ends_with("-width"),
            "border" => prop.starts_with("border-") && (prop.ends_with("-color") || prop.ends_with("-width") || prop == "border-radius"),
            "gap" => prop == "row-gap" || prop == "column-gap",
            _ => false,
        }
}

fn get(s: &ComputedStyle, prop: &str) -> Option<AnimValue> {
    use AnimValue::{Color as C, Length as L, Number as N};
    Some(match prop {
        "opacity" => N(s.opacity),
        "color" => C(s.color),
        "background-color" => C(s.background.unwrap_or(Color::TRANSPARENT)),
        "border-top-color" => C(s.border_color.top),
        "border-right-color" => C(s.border_color.right),
        "border-bottom-color" => C(s.border_color.bottom),
        "border-left-color" => C(s.border_color.left),
        "width" => L(s.width),
        "height" => L(s.height),
        "min-width" => L(s.min_width),
        "min-height" => L(s.min_height),
        "max-width" => L(s.max_width),
        "max-height" => L(s.max_height),
        "margin-top" => L(s.margin.top),
        "margin-right" => L(s.margin.right),
        "margin-bottom" => L(s.margin.bottom),
        "margin-left" => L(s.margin.left),
        "padding-top" => N(s.padding.top),
        "padding-right" => N(s.padding.right),
        "padding-bottom" => N(s.padding.bottom),
        "padding-left" => N(s.padding.left),
        "border-top-width" => N(s.border_width.top),
        "border-right-width" => N(s.border_width.right),
        "border-bottom-width" => N(s.border_width.bottom),
        "border-left-width" => N(s.border_width.left),
        "border-radius" => N(s.border_radius.max()),
        "top" => L(s.inset.top),
        "right" => L(s.inset.right),
        "bottom" => L(s.inset.bottom),
        "left" => L(s.inset.left),
        "font-size" => N(s.font_size),
        "letter-spacing" => N(s.letter_spacing),
        "flex-grow" => N(s.flex_grow),
        "flex-shrink" => N(s.flex_shrink),
        "row-gap" => N(s.row_gap),
        "column-gap" => N(s.column_gap),
        "transform" => AnimValue::Transform(s.transform.clone()),
        _ => return None,
    })
}

fn set(s: &mut ComputedStyle, prop: &str, v: AnimValue) {
    match (prop, v) {
        ("opacity", AnimValue::Number(n)) => s.opacity = n.clamp(0.0, 1.0),
        ("color", AnimValue::Color(c)) => s.color = c,
        ("background-color", AnimValue::Color(c)) => s.background = (c.a > 0).then_some(c),
        ("border-top-color", AnimValue::Color(c)) => s.border_color.top = c,
        ("border-right-color", AnimValue::Color(c)) => s.border_color.right = c,
        ("border-bottom-color", AnimValue::Color(c)) => s.border_color.bottom = c,
        ("border-left-color", AnimValue::Color(c)) => s.border_color.left = c,
        ("width", AnimValue::Length(l)) => s.width = l,
        ("height", AnimValue::Length(l)) => s.height = l,
        ("min-width", AnimValue::Length(l)) => s.min_width = l,
        ("min-height", AnimValue::Length(l)) => s.min_height = l,
        ("max-width", AnimValue::Length(l)) => s.max_width = l,
        ("max-height", AnimValue::Length(l)) => s.max_height = l,
        ("margin-top", AnimValue::Length(l)) => s.margin.top = l,
        ("margin-right", AnimValue::Length(l)) => s.margin.right = l,
        ("margin-bottom", AnimValue::Length(l)) => s.margin.bottom = l,
        ("margin-left", AnimValue::Length(l)) => s.margin.left = l,
        ("padding-top", AnimValue::Number(n)) => s.padding.top = n.max(0.0),
        ("padding-right", AnimValue::Number(n)) => s.padding.right = n.max(0.0),
        ("padding-bottom", AnimValue::Number(n)) => s.padding.bottom = n.max(0.0),
        ("padding-left", AnimValue::Number(n)) => s.padding.left = n.max(0.0),
        ("border-top-width", AnimValue::Number(n)) => s.border_width.top = n.max(0.0),
        ("border-right-width", AnimValue::Number(n)) => s.border_width.right = n.max(0.0),
        ("border-bottom-width", AnimValue::Number(n)) => s.border_width.bottom = n.max(0.0),
        ("border-left-width", AnimValue::Number(n)) => s.border_width.left = n.max(0.0),
        ("border-radius", AnimValue::Number(n)) => s.border_radius = Corners::all(n.max(0.0)),
        ("top", AnimValue::Length(l)) => s.inset.top = l,
        ("right", AnimValue::Length(l)) => s.inset.right = l,
        ("bottom", AnimValue::Length(l)) => s.inset.bottom = l,
        ("left", AnimValue::Length(l)) => s.inset.left = l,
        ("font-size", AnimValue::Number(n)) => s.font_size = n.max(0.0),
        ("letter-spacing", AnimValue::Number(n)) => s.letter_spacing = n,
        ("flex-grow", AnimValue::Number(n)) => s.flex_grow = n.max(0.0),
        ("flex-shrink", AnimValue::Number(n)) => s.flex_shrink = n.max(0.0),
        ("row-gap", AnimValue::Number(n)) => s.row_gap = n.max(0.0),
        ("column-gap", AnimValue::Number(n)) => s.column_gap = n.max(0.0),
        ("transform", AnimValue::Transform(t)) => s.transform = t,
        _ => {}
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    // Interpolate premultiplied, so fading to transparent doesn't shift the hue.
    let (aa, ba) = (f32::from(a.a) / 255.0, f32::from(b.a) / 255.0);
    let alpha = lerp(aa, ba, t);
    if alpha <= 0.0 {
        return Color::TRANSPARENT;
    }
    let ch = |x: u8, y: u8| ((lerp(f32::from(x) * aa, f32::from(y) * ba, t) / alpha).round()).clamp(0.0, 255.0) as u8;
    Color { r: ch(a.r, b.r), g: ch(a.g, b.g), b: ch(a.b, b.b), a: (alpha * 255.0).round() as u8 }
}

fn lerp_stop(a: StopPos, b: StopPos, t: f32) -> Option<StopPos> {
    match (a, b) {
        (StopPos::Px(x), StopPos::Px(y)) => Some(StopPos::Px(lerp(x, y, t))),
        (StopPos::Fraction(x), StopPos::Fraction(y)) => Some(StopPos::Fraction(lerp(x, y, t))),
        _ => None,
    }
}

fn identity_like(f: &TransformFn) -> TransformFn {
    match f {
        TransformFn::Matrix(_) => TransformFn::Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        TransformFn::Translate(x, y) => TransformFn::Translate(
            match x { StopPos::Px(_) => StopPos::Px(0.0), StopPos::Fraction(_) => StopPos::Fraction(0.0) },
            match y { StopPos::Px(_) => StopPos::Px(0.0), StopPos::Fraction(_) => StopPos::Fraction(0.0) },
        ),
        TransformFn::Scale(..) => TransformFn::Scale(1.0, 1.0),
        TransformFn::Rotate(_) => TransformFn::Rotate(0.0),
        TransformFn::Skew(..) => TransformFn::Skew(0.0, 0.0),
        TransformFn::Translate3d(x, y, _) => TransformFn::Translate3d(
            match x { StopPos::Px(_) => StopPos::Px(0.0), StopPos::Fraction(_) => StopPos::Fraction(0.0) },
            match y { StopPos::Px(_) => StopPos::Px(0.0), StopPos::Fraction(_) => StopPos::Fraction(0.0) },
            0.0,
        ),
        TransformFn::Scale3d(..) => TransformFn::Scale3d(1.0, 1.0, 1.0),
        TransformFn::RotateX(_) => TransformFn::RotateX(0.0),
        TransformFn::RotateY(_) => TransformFn::RotateY(0.0),
        TransformFn::Rotate3d([x, y, z, _]) => TransformFn::Rotate3d([*x, *y, *z, 0.0]),
        TransformFn::Perspective(_) => TransformFn::Perspective(1e9),
        TransformFn::Matrix3d(_) => TransformFn::Matrix3d([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]),
    }
}

fn lerp_fn(a: &TransformFn, b: &TransformFn, t: f32) -> Option<TransformFn> {
    Some(match (a, b) {
        (TransformFn::Matrix(x), TransformFn::Matrix(y)) => {
            let mut m = [0.0; 6];
            for i in 0..6 {
                m[i] = lerp(x[i], y[i], t);
            }
            TransformFn::Matrix(m)
        }
        (TransformFn::Translate(ax, ay), TransformFn::Translate(bx, by)) => TransformFn::Translate(lerp_stop(*ax, *bx, t)?, lerp_stop(*ay, *by, t)?),
        (TransformFn::Scale(ax, ay), TransformFn::Scale(bx, by)) => TransformFn::Scale(lerp(*ax, *bx, t), lerp(*ay, *by, t)),
        (TransformFn::Rotate(x), TransformFn::Rotate(y)) => TransformFn::Rotate(lerp(*x, *y, t)),
        (TransformFn::Skew(ax, ay), TransformFn::Skew(bx, by)) => TransformFn::Skew(lerp(*ax, *bx, t), lerp(*ay, *by, t)),
        (TransformFn::Translate3d(ax, ay, az), TransformFn::Translate3d(bx, by, bz)) => {
            TransformFn::Translate3d(lerp_stop(*ax, *bx, t)?, lerp_stop(*ay, *by, t)?, lerp(*az, *bz, t))
        }
        (TransformFn::Scale3d(ax, ay, az), TransformFn::Scale3d(bx, by, bz)) => TransformFn::Scale3d(lerp(*ax, *bx, t), lerp(*ay, *by, t), lerp(*az, *bz, t)),
        (TransformFn::RotateX(x), TransformFn::RotateX(y)) => TransformFn::RotateX(lerp(*x, *y, t)),
        (TransformFn::RotateY(x), TransformFn::RotateY(y)) => TransformFn::RotateY(lerp(*x, *y, t)),
        (TransformFn::Rotate3d(x), TransformFn::Rotate3d(y)) if x[..3] == y[..3] => TransformFn::Rotate3d([x[0], x[1], x[2], lerp(x[3], y[3], t)]),
        (TransformFn::Perspective(x), TransformFn::Perspective(y)) => TransformFn::Perspective(lerp(*x, *y, t)),
        (TransformFn::Matrix3d(x), TransformFn::Matrix3d(y)) => {
            let mut m = [0.0; 16];
            for (i, slot) in m.iter_mut().enumerate() {
                *slot = lerp(x[i], y[i], t);
            }
            TransformFn::Matrix3d(m)
        }
        _ => return None,
    })
}

/// The value `t` of the way from `a` to `b`; values that can't blend flip halfway.
fn interpolate(a: &AnimValue, b: &AnimValue, t: f32) -> AnimValue {
    let discrete = || if t < 0.5 { a.clone() } else { b.clone() };
    match (a, b) {
        (AnimValue::Number(x), AnimValue::Number(y)) => AnimValue::Number(lerp(*x, *y, t)),
        (AnimValue::Color(x), AnimValue::Color(y)) => AnimValue::Color(lerp_color(*x, *y, t)),
        (AnimValue::Length(x), AnimValue::Length(y)) => match (x, y) {
            (Length::Px(p), Length::Px(q)) => AnimValue::Length(Length::Px(lerp(*p, *q, t))),
            (Length::Percent(p), Length::Percent(q)) => AnimValue::Length(Length::Percent(lerp(*p, *q, t))),
            _ => discrete(),
        },
        (AnimValue::Transform(x), AnimValue::Transform(y)) => {
            let n = x.len().max(y.len());
            let pad = |list: &Vec<TransformFn>, other: &Vec<TransformFn>, i: usize| list.get(i).copied().or_else(|| other.get(i).map(identity_like));
            let blended: Option<Vec<TransformFn>> = (0..n)
                .map(|i| lerp_fn(&pad(x, y, i)?, &pad(y, x, i)?, t))
                .collect();
            match blended {
                Some(list) => AnimValue::Transform(list),
                None => discrete(),
            }
        }
        _ => discrete(),
    }
}

/// The animatable properties a declaration of `name` sets (shorthands set several).
fn props_for(name: &str) -> Vec<&'static str> {
    let direct = |n: &str| PROPS.iter().copied().filter(|p| *p == n).collect::<Vec<_>>();
    let sides = |prefix: &str, suffix: &str| ["top", "right", "bottom", "left"].iter().filter_map(|s| {
        let want = if prefix.is_empty() { format!("{s}{suffix}") } else { format!("{prefix}-{s}{suffix}") };
        PROPS.iter().copied().find(|p| *p == want)
    }).collect::<Vec<_>>();
    match name {
        "margin" => sides("margin", ""),
        "padding" => sides("padding", ""),
        "border-width" => sides("border", "-width"),
        "border-color" => sides("border", "-color"),
        "border" => [sides("border", "-width"), sides("border", "-color")].concat(),
        "border-top" | "border-right" | "border-bottom" | "border-left" => {
            [format!("{name}-width"), format!("{name}-color")].iter().filter_map(|w| PROPS.iter().copied().find(|p| p == w)).collect()
        }
        "inset" => sides("", ""),
        "background" => vec!["background-color"],
        "gap" => vec!["row-gap", "column-gap"],
        other => direct(other),
    }
}

/// The properties a keyframe's declarations set, with their values.
fn frame_values(base: &ComputedStyle, declarations: &[Declaration]) -> Vec<(&'static str, AnimValue)> {
    let mut st = base.clone();
    let mut set_props: Vec<&'static str> = Vec::new();
    for d in declarations.iter().filter(|d| d.raw.is_empty()) {
        apply(&mut st, d, base.font_size);
        set_props.extend(props_for(&d.name));
    }
    set_props.sort_unstable();
    set_props.dedup();
    set_props.into_iter().filter_map(|p| Some((p, get(&st, p)?))).collect()
}

#[derive(Debug)]
struct Running {
    from: AnimValue,
    to: AnimValue,
    start: f64,
    duration: f64,
    delay: f64,
    timing: Timing,
}

/// A lifecycle event for the page: `transitionend`, `animationstart` or `animationend`.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimEvent {
    pub node: NodeId,
    pub kind: &'static str,
    /// The property (transitions) or `@keyframes` name (animations).
    pub name: String,
    /// Seconds into the transition or animation when it happened.
    pub elapsed: f64,
}

/// Carries transitions and animations across restyles.
#[derive(Default)]
pub struct Animator {
    events: Vec<AnimEvent>,
    begun: HashSet<(NodeId, String)>,
    ended: HashSet<(NodeId, String)>,
    /// Each transitioning node's target values at the previous restyle.
    last: HashMap<NodeId, Vec<(&'static str, AnimValue)>>,
    transitions: HashMap<(NodeId, &'static str), Running>,
    /// When each (node, animation name) started.
    started: HashMap<(NodeId, String), f64>,
    /// Nodes whose style the last `update` overwrote.
    touched: HashSet<NodeId>,
    active: bool,
}

impl Animator {
    pub fn new() -> Self {
        Self::default()
    }

    /// It holds state from earlier restyles, so it must see the next one even if the
    /// page no longer animates anything.
    pub fn is_tracking(&self) -> bool {
        !self.last.is_empty() || !self.transitions.is_empty() || !self.started.is_empty()
    }

    /// The `transitionend`/`animationstart`/`animationend` events since the last call.
    pub fn take_events(&mut self) -> Vec<AnimEvent> {
        std::mem::take(&mut self.events)
    }

    /// Something is still moving: the embedder should keep calling [`Animator::update`].
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Bring `styles` (a copy of `base`, or its previous output) to time `now`.
    /// `restyled` is true when `base` was just recomputed, which is when changes of
    /// target values start transitions.
    pub fn update(&mut self, base: &StyleMap, styles: &mut StyleMap, now: f64, restyled: bool) {
        let real = base.styles.len() - base.origins().len();
        if restyled {
            self.start_transitions(base, 0..real, now);
        }
        self.advance(base, styles, now);
    }

    /// [`update`](Self::update) after only the targets of `nodes` were recomputed: transitions start
    /// for those, everything else just moves on to `now`.
    pub fn update_nodes(&mut self, base: &StyleMap, styles: &mut StyleMap, now: f64, nodes: &[NodeId]) {
        self.start_transitions(base, nodes.iter().map(|n| n.0), now);
        self.advance(base, styles, now);
    }

    fn advance(&mut self, base: &StyleMap, styles: &mut StyleMap, now: f64) {
        let real = base.styles.len() - base.origins().len();
        // Undo last frame's overrides, then lay this frame's over the target.
        for n in self.touched.drain() {
            if let (Some(b), Some(s)) = (base.styles.get(n.0), styles.styles.get_mut(n.0)) {
                *s = b.clone();
            }
        }
        let mut active = false;
        let mut seen = HashSet::new();
        let mut done = Vec::new();
        for ((node, prop), run) in &self.transitions {
            let Some(style) = styles.styles.get_mut(node.0) else { continue };
            let t = if run.duration > 0.0 { (now - run.start - run.delay) / run.duration } else { 1.0 };
            if t >= 1.0 {
                done.push((*node, *prop));
                continue;
            }
            active = true;
            let value = if t <= 0.0 { run.from.clone() } else { interpolate(&run.from, &run.to, run.timing.ease(t as f32)) };
            set(style, prop, value);
            self.touched.insert(*node);
        }
        for key in done {
            if let Some(run) = self.transitions.remove(&key) {
                self.events.push(AnimEvent { node: key.0, kind: "transitionend", name: key.1.to_string(), elapsed: run.duration });
            }
        }
        for n in 0..real {
            let node = NodeId(n);
            let specs = &base.styles[n].animations;
            if specs.is_empty() {
                continue;
            }
            for spec in specs {
                let Some(frames) = base.keyframes.get(&spec.name) else { continue };
                let key = (node, spec.name.clone());
                let start = *self.started.entry(key.clone()).or_insert(now);
                seen.insert(key.clone());
                let local = now - start - f64::from(spec.delay);
                let total = f64::from(spec.duration) * f64::from(spec.iterations);
                if local >= 0.0 && self.begun.insert(key.clone()) {
                    self.events.push(AnimEvent { node, kind: "animationstart", name: spec.name.clone(), elapsed: 0.0 });
                }
                if local >= total && total.is_finite() && self.ended.insert(key.clone()) {
                    self.events.push(AnimEvent { node, kind: "animationend", name: spec.name.clone(), elapsed: total });
                }
                let Some((progress, running)) = animation_progress(spec, now - start) else { continue };
                active |= running;
                let style = &mut styles.styles[n];
                for (prop, value) in sample(&base.styles[n], frames, spec, progress) {
                    set(style, prop, value);
                }
                self.touched.insert(node);
            }
        }
        self.started.retain(|k, _| seen.contains(k));
        self.begun.retain(|k| seen.contains(k));
        self.ended.retain(|k| seen.contains(k));
        self.active = active;
    }

    fn start_transitions(&mut self, base: &StyleMap, nodes: impl Iterator<Item = usize>, now: f64) {
        for n in nodes {
            let node = NodeId(n);
            let st = &base.styles[n];
            if st.transitions.is_empty() {
                self.last.remove(&node);
                self.transitions.retain(|(k, _), _| *k != node);
                continue;
            }
            let now_values: Vec<(&'static str, AnimValue)> = PROPS.iter().filter_map(|p| Some((*p, get(st, p)?))).collect();
            if let Some(before) = self.last.get(&node) {
                for ((prop, old), (_, new)) in before.iter().zip(&now_values) {
                    if old == new {
                        continue;
                    }
                    let spec = st.transitions.iter().rev().find(|s| transitions_property(&s.property, prop));
                    let Some(spec) = spec.filter(|s| s.duration + s.delay > 0.0) else {
                        self.transitions.remove(&(node, prop));
                        continue;
                    };
                    // Start from what is showing now if it was mid-transition.
                    let from = match self.transitions.get(&(node, *prop)) {
                        Some(run) => {
                            let t = if run.duration > 0.0 { ((now - run.start - run.delay) / run.duration).clamp(0.0, 1.0) } else { 1.0 };
                            interpolate(&run.from, &run.to, run.timing.ease(t as f32))
                        }
                        None => old.clone(),
                    };
                    self.transitions.insert(
                        (node, prop),
                        Running { from, to: new.clone(), start: now, duration: f64::from(spec.duration), delay: f64::from(spec.delay), timing: spec.timing },
                    );
                }
            }
            self.last.insert(node, now_values);
        }
    }
}

/// (progress 0..=1 through the keyframes, still running) for an animation `elapsed`
/// seconds after it was set up; `None` when it has no effect right now.
fn animation_progress(spec: &AnimationSpec, elapsed: f64) -> Option<(f32, bool)> {
    let (dur, delay) = (f64::from(spec.duration), f64::from(spec.delay));
    let local = elapsed - delay;
    let directed = |iteration: f64, frac: f64| -> f32 {
        let reversed = match spec.direction {
            Direction::Normal => false,
            Direction::Reverse => true,
            Direction::Alternate => iteration % 2.0 >= 1.0,
            Direction::AlternateReverse => iteration % 2.0 < 1.0,
        };
        (if reversed { 1.0 - frac } else { frac }) as f32
    };
    if local < 0.0 {
        return matches!(spec.fill, Fill::Backwards | Fill::Both).then(|| (directed(0.0, 0.0), true));
    }
    let total = dur * f64::from(spec.iterations);
    if dur <= 0.0 || local >= total {
        // Finished: the last iteration's end state, if it fills forwards.
        if !matches!(spec.fill, Fill::Forwards | Fill::Both) {
            return None;
        }
        let iterations = f64::from(spec.iterations);
        let (iteration, frac) = if iterations.fract() == 0.0 { ((iterations - 1.0).max(0.0), 1.0) } else { (iterations.floor(), iterations.fract()) };
        return Some((directed(iteration, frac), false));
    }
    let iteration = (local / dur).floor();
    Some((directed(iteration, local / dur - iteration), true))
}

/// Property values at `progress` of the keyframes, eased per interval.
fn sample(base: &ComputedStyle, frames: &[crate::parse::Frame], spec: &AnimationSpec, progress: f32) -> Vec<(&'static str, AnimValue)> {
    let per_frame: Vec<(f32, Vec<(&'static str, AnimValue)>)> = frames.iter().map(|f| (f.offset, frame_values(base, &f.declarations))).collect();
    let mut out = Vec::new();
    for prop in PROPS {
        let Some(own) = get(base, prop) else { continue };
        let mut points: Vec<(f32, AnimValue)> = per_frame
            .iter()
            .filter_map(|(o, vals)| vals.iter().rev().find(|(p, _)| p == prop).map(|(_, v)| (*o, v.clone())))
            .collect();
        if points.is_empty() {
            continue;
        }
        if points.first().is_none_or(|p| p.0 > 0.0) {
            points.insert(0, (0.0, own.clone()));
        }
        if points.last().is_none_or(|p| p.0 < 1.0) {
            points.push((1.0, own));
        }
        let value = points
            .windows(2)
            .find(|w| progress >= w[0].0 && progress <= w[1].0)
            .map(|w| {
                let span = w[1].0 - w[0].0;
                let t = if span > 0.0 { (progress - w[0].0) / span } else { 1.0 };
                interpolate(&w[0].1, &w[1].1, spec.timing.ease(t))
            })
            .unwrap_or_else(|| points[points.len() - 1].1.clone());
        out.push((*prop, value));
    }
    out
}
