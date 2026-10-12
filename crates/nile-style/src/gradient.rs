//! CSS gradients as values: `linear-gradient()`, `radial-gradient()` and the
//! `repeating-` forms. Painting turns them into Skia shaders.

use crate::Color;

/// A position along the gradient line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StopPos {
    Px(f32),
    /// A fraction (50% = 0.5).
    Fraction(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorStop {
    pub color: Color,
    pub pos: Option<StopPos>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinearDir {
    /// Degrees, 0 = up, clockwise.
    Angle(f32),
    /// `to bottom right` is (1, 1): the sign of each axis it points along.
    Corner(i8, i8),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Extent {
    ClosestSide,
    ClosestCorner,
    FarthestSide,
    FarthestCorner,
    /// Explicit radii: one for a circle, two for an ellipse.
    Radii(StopPos, StopPos),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientKind {
    Linear(LinearDir),
    Radial {
        circle: bool,
        extent: Extent,
        /// Center as (x, y) within the box.
        at: (StopPos, StopPos),
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<ColorStop>,
    pub repeating: bool,
}

impl Gradient {
    /// Stops as (position, color) with positions as fractions of `length` px: missing
    /// ends default to 0 and 100%, missing ones in between are spread evenly, and
    /// positions never decrease.
    pub fn resolved_stops(&self, length: f32) -> Vec<(f32, Color)> {
        let n = self.stops.len();
        let mut pos: Vec<Option<f32>> = self
            .stops
            .iter()
            .map(|s| {
                s.pos.map(|p| match p {
                    StopPos::Px(px) => if length > 0.0 { px / length } else { 0.0 },
                    StopPos::Fraction(f) => f,
                })
            })
            .collect();
        if n == 0 {
            return Vec::new();
        }
        pos[0].get_or_insert(0.0);
        pos[n - 1].get_or_insert(1.0);
        let mut max = f32::MIN;
        for p in pos.iter_mut().flatten() {
            max = max.max(*p);
            *p = max;
        }
        let mut i = 0;
        while i < n {
            if pos[i].is_some() {
                i += 1;
                continue;
            }
            let from = i - 1;
            let mut to = i;
            while pos[to].is_none() {
                to += 1;
            }
            let (a, b) = (pos[from].unwrap_or(0.0), pos[to].unwrap_or(1.0));
            for (k, slot) in pos[from + 1..to].iter_mut().enumerate() {
                *slot = Some(a + (b - a) * (k + 1) as f32 / (to - from) as f32);
            }
            i = to;
        }
        self.stops.iter().zip(pos).map(|(s, p)| (p.unwrap_or(0.0), s.color)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(c: u8, pos: Option<StopPos>) -> ColorStop {
        ColorStop { color: Color::rgb(c, c, c), pos }
    }

    #[test]
    fn stops_fill_in_and_never_decrease() {
        let g = Gradient {
            kind: GradientKind::Linear(LinearDir::Angle(180.0)),
            stops: vec![stop(0, None), stop(1, None), stop(2, Some(StopPos::Px(50.0))), stop(3, Some(StopPos::Fraction(0.25))), stop(4, None)],
            repeating: false,
        };
        let r: Vec<f32> = g.resolved_stops(100.0).into_iter().map(|(p, _)| p).collect();
        assert_eq!(r, vec![0.0, 0.25, 0.5, 0.5, 1.0]);
    }
}
