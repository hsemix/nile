//! `shape-outside`: the outline text wraps around instead of a float's rectangle.
//!
//! Supports the basic shapes `circle()`, `ellipse()`, `inset()` and `polygon()` (with an optional
//! reference box keyword). Layout resolves one against the float's reference box and asks it how far
//! into a text line's band the float reaches.

/// A length or a percentage of the reference box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dim {
    Px(f32),
    Pct(f32),
}

impl Dim {
    fn of(self, whole: f32) -> f32 {
        match self {
            Dim::Px(p) => p,
            Dim::Pct(p) => p / 100.0 * whole,
        }
    }
}

/// A circle or ellipse radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Radius {
    Len(Dim),
    ClosestSide,
    FarthestSide,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Circle { r: Radius, at: (Dim, Dim) },
    Ellipse { rx: Radius, ry: Radius, at: (Dim, Dim) },
    /// Offsets from the top, right, bottom and left edges.
    Inset([Dim; 4]),
    Polygon(Vec<(Dim, Dim)>),
}

/// Which box the shape's percentages and coordinates refer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RefBox {
    ContentBox,
    PaddingBox,
    BorderBox,
    #[default]
    MarginBox,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeOutside {
    pub shape: Shape,
    pub reference: RefBox,
}

/// A shape in pixels, relative to the reference box's top-left corner.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32 },
    Rect { x0: f32, y0: f32, x1: f32, y1: f32 },
    Polygon(Vec<(f32, f32)>),
}

impl ShapeOutside {
    /// Parse a `shape-outside` value; `None` for `none`, boxes alone, images and anything unusable.
    pub fn parse(text: &str) -> Option<ShapeOutside> {
        let text = text.trim().to_ascii_lowercase();
        let mut reference = None;
        let mut shape = None;
        let mut rest = text.as_str();
        while !rest.trim().is_empty() {
            rest = rest.trim_start();
            let end = rest.find(['(', ' ']).unwrap_or(rest.len());
            if rest[end..].starts_with('(') {
                let close = rest.find(')')?;
                let (name, args) = (&rest[..end], &rest[end + 1..close]);
                shape = Some(parse_shape(name, args)?);
                rest = &rest[close + 1..];
            } else {
                reference = Some(match &rest[..end] {
                    "content-box" => RefBox::ContentBox,
                    "padding-box" => RefBox::PaddingBox,
                    "border-box" => RefBox::BorderBox,
                    "margin-box" => RefBox::MarginBox,
                    _ => return None,
                });
                rest = &rest[end..];
            }
        }
        Some(ShapeOutside { shape: shape?, reference: reference.unwrap_or_default() })
    }
}

fn dim(token: &str) -> Option<Dim> {
    let t = token.trim();
    if let Some(p) = t.strip_suffix('%') {
        return p.trim().parse().ok().map(Dim::Pct);
    }
    if let Some(p) = t.strip_suffix("px") {
        return p.trim().parse().ok().map(Dim::Px);
    }
    (t == "0").then_some(Dim::Px(0.0))
}

fn position(text: &str) -> Option<(Dim, Dim)> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let keyword = |k: &str| match k {
        "left" | "top" => Some(Dim::Pct(0.0)),
        "center" => Some(Dim::Pct(50.0)),
        "right" | "bottom" => Some(Dim::Pct(100.0)),
        other => dim(other),
    };
    match parts.as_slice() {
        [] => Some((Dim::Pct(50.0), Dim::Pct(50.0))),
        [a] => match *a {
            "top" | "bottom" => Some((Dim::Pct(50.0), keyword(a)?)),
            _ => Some((keyword(a)?, Dim::Pct(50.0))),
        },
        [a, b] => match (*a, *b) {
            ("top" | "bottom", _) => Some((keyword(b)?, keyword(a)?)),
            _ => Some((keyword(a)?, keyword(b)?)),
        },
        _ => None,
    }
}

fn radius(token: &str) -> Option<Radius> {
    match token {
        "closest-side" => Some(Radius::ClosestSide),
        "farthest-side" => Some(Radius::FarthestSide),
        t => dim(t).map(Radius::Len),
    }
}

fn parse_shape(name: &str, args: &str) -> Option<Shape> {
    match name {
        "circle" | "ellipse" => {
            let (radii, at) = match args.split_once(" at ") {
                Some((r, p)) => (r.trim(), position(p)?),
                None => match args.trim().strip_prefix("at ") {
                    Some(p) => ("", position(p)?),
                    None => (args.trim(), (Dim::Pct(50.0), Dim::Pct(50.0))),
                },
            };
            let toks: Vec<&str> = radii.split_whitespace().collect();
            if name == "circle" {
                let r = match toks.as_slice() {
                    [] => Radius::ClosestSide,
                    [r] => radius(r)?,
                    _ => return None,
                };
                Some(Shape::Circle { r, at })
            } else {
                let (rx, ry) = match toks.as_slice() {
                    [] => (Radius::ClosestSide, Radius::ClosestSide),
                    [a, b] => (radius(a)?, radius(b)?),
                    _ => return None,
                };
                Some(Shape::Ellipse { rx, ry, at })
            }
        }
        "inset" => {
            // `round` (corner radii) is not supported: the rectangle is used as is.
            let sides = args.split(" round ").next()?;
            let v: Vec<Dim> = sides.split_whitespace().map(dim).collect::<Option<_>>()?;
            let [t, r, b, l] = match v.as_slice() {
                [a] => [*a; 4],
                [a, b] => [*a, *b, *a, *b],
                [a, b, c] => [*a, *b, *c, *b],
                [a, b, c, d] => [*a, *b, *c, *d],
                _ => return None,
            };
            Some(Shape::Inset([t, r, b, l]))
        }
        "polygon" => {
            let mut points = Vec::new();
            for (i, part) in args.split(',').enumerate() {
                let part = part.trim();
                if i == 0 && (part == "nonzero" || part == "evenodd") {
                    continue;
                }
                let mut xy = part.split_whitespace();
                let (x, y) = (dim(xy.next()?)?, dim(xy.next()?)?);
                if xy.next().is_some() {
                    return None;
                }
                points.push((x, y));
            }
            (points.len() >= 3).then_some(Shape::Polygon(points))
        }
        _ => None,
    }
}

impl Shape {
    /// The shape in pixels for a reference box `w` x `h`, grown by `margin` (`shape-margin`).
    pub fn resolve(&self, w: f32, h: f32, margin: f32) -> Resolved {
        match self {
            Shape::Circle { r, at } => {
                let (cx, cy) = (at.0.of(w), at.1.of(h));
                let side = |near: bool| {
                    let xs = [cx.abs(), (w - cx).abs()];
                    let ys = [cy.abs(), (h - cy).abs()];
                    let all = [xs[0], xs[1], ys[0], ys[1]];
                    if near { all.into_iter().fold(f32::MAX, f32::min) } else { all.into_iter().fold(0.0, f32::max) }
                };
                let r = match r {
                    Radius::Len(Dim::Px(p)) => *p,
                    // Percentages of a circle's radius are of sqrt((w² + h²) / 2).
                    Radius::Len(Dim::Pct(p)) => p / 100.0 * ((w * w + h * h) / 2.0).sqrt(),
                    Radius::ClosestSide => side(true),
                    Radius::FarthestSide => side(false),
                } + margin;
                Resolved::Ellipse { cx, cy, rx: r, ry: r }
            }
            Shape::Ellipse { rx, ry, at } => {
                let (cx, cy) = (at.0.of(w), at.1.of(h));
                let one = |r: &Radius, c: f32, whole: f32| match r {
                    Radius::Len(d) => d.of(whole),
                    Radius::ClosestSide => c.abs().min((whole - c).abs()),
                    Radius::FarthestSide => c.abs().max((whole - c).abs()),
                } + margin;
                Resolved::Ellipse { cx, cy, rx: one(rx, cx, w), ry: one(ry, cy, h) }
            }
            Shape::Inset([t, r, b, l]) => Resolved::Rect { x0: l.of(w) - margin, y0: t.of(h) - margin, x1: w - r.of(w) + margin, y1: h - b.of(h) + margin },
            Shape::Polygon(points) => Resolved::Polygon(points.iter().map(|(x, y)| (x.of(w), y.of(h))).collect()),
        }
    }
}

impl Resolved {
    /// How far the shape reaches sideways inside the band of y values `[y0, y1]`: the leftmost and
    /// rightmost x it occupies there, or `None` if it doesn't touch the band. `margin` widens a
    /// polygon's reach (circles, ellipses and rectangles were already grown when resolved).
    pub fn span(&self, y0: f32, y1: f32, margin: f32) -> Option<(f32, f32)> {
        match self {
            Resolved::Ellipse { cx, cy, rx, ry } => {
                if *rx <= 0.0 || *ry <= 0.0 {
                    return None;
                }
                // The widest slice in the band is the one nearest the centre.
                let dy = if *cy < y0 { y0 - cy } else if *cy > y1 { cy - y1 } else { 0.0 };
                if dy >= *ry {
                    return None;
                }
                let half = rx * (1.0 - (dy / ry).powi(2)).sqrt();
                Some((cx - half, cx + half))
            }
            Resolved::Rect { x0, y0: top, x1, y1: bottom } => (y1 > *top && y0 < *bottom && x1 > x0).then_some((*x0, *x1)),
            Resolved::Polygon(points) => {
                let mut samples = vec![y0, y1];
                samples.extend(points.iter().map(|p| p.1).filter(|y| *y > y0 && *y < y1));
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for y in samples {
                    let n = points.len();
                    for i in 0..n {
                        let ((ax, ay), (bx, by)) = (points[i], points[(i + 1) % n]);
                        if (ay <= y && by >= y) || (by <= y && ay >= y) {
                            let x = if (by - ay).abs() < 1e-6 { ax.min(bx) } else { ax + (y - ay) / (by - ay) * (bx - ax) };
                            lo = lo.min(x);
                            hi = hi.max(x);
                            if (by - ay).abs() < 1e-6 {
                                hi = hi.max(ax.max(bx));
                            }
                        }
                    }
                }
                (lo <= hi).then_some((lo - margin, hi + margin))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_shapes_and_reference_boxes() {
        let c = ShapeOutside::parse("circle(50% at 30% 40%) border-box").unwrap();
        assert_eq!(c.reference, RefBox::BorderBox);
        assert_eq!(c.shape, Shape::Circle { r: Radius::Len(Dim::Pct(50.0)), at: (Dim::Pct(30.0), Dim::Pct(40.0)) });
        assert!(matches!(ShapeOutside::parse("circle()").unwrap().shape, Shape::Circle { r: Radius::ClosestSide, .. }));
        assert!(matches!(ShapeOutside::parse("ellipse(40px 20px at center)").unwrap().shape, Shape::Ellipse { .. }));
        assert!(matches!(ShapeOutside::parse("inset(10px 20px)").unwrap().shape, Shape::Inset(_)));
        assert!(matches!(ShapeOutside::parse("polygon(0 0, 100% 0, 0 100%)").unwrap().shape, Shape::Polygon(p) if p.len() == 3));
        assert!(ShapeOutside::parse("none").is_none());
        assert!(ShapeOutside::parse("margin-box").is_none());
        assert!(ShapeOutside::parse("url(a.png)").is_none());
    }

    #[test]
    fn a_circle_reaches_furthest_at_its_middle() {
        let s = ShapeOutside::parse("circle(50%)").unwrap().shape.resolve(100.0, 100.0, 0.0);
        let (l, r) = s.span(45.0, 55.0, 0.0).unwrap();
        assert!((l - 0.0).abs() < 0.01 && (r - 100.0).abs() < 0.01, "{l} {r}");
        // Near the top the slice is much narrower.
        let (l, r) = s.span(0.0, 4.0, 0.0).unwrap();
        assert!(l > 25.0 && r < 75.0, "{l} {r}");
        assert!(s.span(120.0, 130.0, 0.0).is_none());
    }

    #[test]
    fn a_triangle_widens_downwards() {
        let s = ShapeOutside::parse("polygon(0 0, 100% 0, 0 100%)").unwrap().shape.resolve(100.0, 100.0, 0.0);
        let (_, top) = s.span(0.0, 10.0, 0.0).unwrap();
        let (_, lower) = s.span(60.0, 70.0, 0.0).unwrap();
        assert!(top > lower, "{top} {lower}");
    }
}
