//! 2D affine transforms: `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.

use crate::Rect;
use nile_style::{StopPos, TransformFn};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Default for Affine {
    fn default() -> Self {
        Affine::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn translate(x: f32, y: f32) -> Affine {
        Affine { e: x, f: y, ..Affine::IDENTITY }
    }

    pub fn scale(x: f32, y: f32) -> Affine {
        Affine { a: x, d: y, ..Affine::IDENTITY }
    }

    /// Clockwise by `deg` (the y axis points down).
    pub fn rotate(deg: f32) -> Affine {
        let (s, c) = deg.to_radians().sin_cos();
        Affine { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    pub fn skew(x_deg: f32, y_deg: f32) -> Affine {
        Affine { a: 1.0, b: y_deg.to_radians().tan(), c: x_deg.to_radians().tan(), d: 1.0, e: 0.0, f: 0.0 }
    }

    /// The transform that applies `other` first, then `self`.
    pub fn then_after(self, other: Affine) -> Affine {
        Affine {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            e: self.a * other.e + self.c * other.f + self.e,
            f: self.b * other.e + self.d * other.f + self.f,
        }
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-9 {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Affine { a, b, c, d, e: -(a * self.e + c * self.f), f: -(b * self.e + d * self.f) })
    }

    pub fn is_translation(&self) -> bool {
        (self.a - 1.0).abs() < 1e-6 && self.b.abs() < 1e-6 && self.c.abs() < 1e-6 && (self.d - 1.0).abs() < 1e-6
    }

    /// The bounding box of `r` after the transform.
    pub fn bounds(&self, r: Rect) -> Rect {
        let pts = [self.apply(r.x, r.y), self.apply(r.x + r.width, r.y), self.apply(r.x, r.y + r.height), self.apply(r.x + r.width, r.y + r.height)];
        let (mut l, mut t, mut rt, mut b) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for (x, y) in pts {
            l = l.min(x);
            t = t.min(y);
            rt = rt.max(x);
            b = b.max(y);
        }
        Rect { x: l, y: t, width: rt - l, height: b - t }
    }

    /// The transform a `transform` list and origin give a border box.
    pub fn from_style(list: &[TransformFn], origin: (StopPos, StopPos), r: Rect) -> Affine {
        let along = |p: StopPos, whole: f32| match p {
            StopPos::Px(v) => v,
            StopPos::Fraction(f) => f * whole,
        };
        let mut m = Affine::IDENTITY;
        for f in list {
            let step = match *f {
                TransformFn::Matrix([a, b, c, d, e, f]) => Affine { a, b, c, d, e, f },
                TransformFn::Translate(x, y) => Affine::translate(along(x, r.width), along(y, r.height)),
                TransformFn::Scale(x, y) => Affine::scale(x, y),
                TransformFn::Rotate(deg) => Affine::rotate(deg),
                TransformFn::Skew(x, y) => Affine::skew(x, y),
            };
            // Later functions apply first to the point (CSS multiplies left to right).
            m = m.then_after(step);
        }
        let (ox, oy) = (r.x + along(origin.0, r.width), r.y + along(origin.1, r.height));
        Affine::translate(ox, oy).then_after(m).then_after(Affine::translate(-ox, -oy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_and_inverts() {
        let r = Rect { x: 10.0, y: 20.0, width: 100.0, height: 50.0 };
        // Rotating 90deg about the center keeps the center fixed.
        let m = Affine::from_style(&[TransformFn::Rotate(90.0)], (StopPos::Fraction(0.5), StopPos::Fraction(0.5)), r);
        let (cx, cy) = m.apply(60.0, 45.0);
        assert!((cx - 60.0).abs() < 1e-3 && (cy - 45.0).abs() < 1e-3);
        // A point 10 right of the center ends up 10 below it.
        let (x, y) = m.apply(70.0, 45.0);
        assert!((x - 60.0).abs() < 1e-3 && (y - 55.0).abs() < 1e-3);
        let (bx, by) = m.inverse().unwrap().apply(x, y);
        assert!((bx - 70.0).abs() < 1e-3 && (by - 45.0).abs() < 1e-3);
        // translate(50%) moves by half the width; list order: translate then scale applies scale first.
        let t = Affine::from_style(&[TransformFn::Translate(StopPos::Fraction(0.5), StopPos::Px(0.0)), TransformFn::Scale(2.0, 2.0)], (StopPos::Px(0.0), StopPos::Px(0.0)), r);
        // Origin (10, 20) is the box corner, so that point only gets the translation.
        assert_eq!(t.apply(10.0, 20.0), (60.0, 20.0));
        assert_eq!(t.apply(20.0, 30.0), (80.0, 40.0));
    }
}
