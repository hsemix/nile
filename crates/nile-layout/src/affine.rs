//! Transforms of the page plane: `x' = (a*x + c*y + e) / w`, `y' = (b*x + d*y + f) / w` with
//! `w = g*x + h*y + i`. Plain 2D transforms have `g = h = 0`, `i = 1`; 3D transforms (`rotateX`,
//! `perspective`, ...) are built as 4x4 matrices and flattened onto the plane (z = 0) here.

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
    /// The perspective row.
    pub g: f32,
    pub h: f32,
    pub i: f32,
}

impl Default for Affine {
    fn default() -> Self {
        Affine::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0, g: 0.0, h: 0.0, i: 1.0 };

    pub fn translate(x: f32, y: f32) -> Affine {
        Affine { e: x, f: y, ..Affine::IDENTITY }
    }

    pub fn scale(x: f32, y: f32) -> Affine {
        Affine { a: x, d: y, ..Affine::IDENTITY }
    }

    /// Clockwise by `deg` (the y axis points down).
    pub fn rotate(deg: f32) -> Affine {
        let (s, c) = deg.to_radians().sin_cos();
        Affine { a: c, b: s, c: -s, d: c, ..Affine::IDENTITY }
    }

    pub fn skew(x_deg: f32, y_deg: f32) -> Affine {
        Affine { b: y_deg.to_radians().tan(), c: x_deg.to_radians().tan(), ..Affine::IDENTITY }
    }

    fn rows(&self) -> [[f32; 3]; 3] {
        [[self.a, self.c, self.e], [self.b, self.d, self.f], [self.g, self.h, self.i]]
    }

    fn from_rows(m: [[f32; 3]; 3]) -> Affine {
        Affine { a: m[0][0], c: m[0][1], e: m[0][2], b: m[1][0], d: m[1][1], f: m[1][2], g: m[2][0], h: m[2][1], i: m[2][2] }
    }

    /// Whether a surface mapped by this transform shows its back to the viewer: the map flips
    /// orientation (negative determinant), as after `rotateY(180deg)`.
    pub fn faces_away(&self) -> bool {
        let det = self.a * (self.d * self.i - self.f * self.h) - self.c * (self.b * self.i - self.f * self.g) + self.e * (self.b * self.h - self.d * self.g);
        det * self.i < 0.0
    }

    /// The transform that applies `other` first, then `self`.
    pub fn then_after(self, other: Affine) -> Affine {
        let (x, y) = (self.rows(), other.rows());
        let mut out = [[0.0; 3]; 3];
        for (r, row) in out.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| x[r][k] * y[k][c]).sum();
            }
        }
        Affine::from_rows(out)
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        // Points at or behind the eye (w <= 0) are pushed far away rather than flipped.
        let w = self.g * x + self.h * y + self.i;
        let w = if w.abs() < 1e-6 { 1e-6 } else { w };
        ((self.a * x + self.c * y + self.e) / w, (self.b * x + self.d * y + self.f) / w)
    }

    pub fn inverse(&self) -> Option<Affine> {
        let m = self.rows();
        let cof = |r: usize, c: usize| {
            let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
            let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
            m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]
        };
        let det = m[0][0] * cof(0, 0) + m[0][1] * cof(0, 1) + m[0][2] * cof(0, 2);
        if det.abs() < 1e-9 {
            return None;
        }
        // The inverse is the transposed cofactor matrix over the determinant.
        let mut out = [[0.0; 3]; 3];
        for (r, row) in out.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = cof(c, r) / det;
            }
        }
        // Normalise so the corner is 1 where it can be, keeping plain 2D transforms tidy.
        let k = out[2][2];
        if k.abs() > 1e-9 {
            for row in &mut out {
                for cell in row.iter_mut() {
                    *cell /= k;
                }
            }
        }
        Some(Affine::from_rows(out))
    }

    pub fn is_translation(&self) -> bool {
        (self.a - 1.0).abs() < 1e-6 && self.b.abs() < 1e-6 && self.c.abs() < 1e-6 && (self.d - 1.0).abs() < 1e-6 && self.g.abs() < 1e-9 && self.h.abs() < 1e-9
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

    /// The transform a `transform` list and origin give a border box. `perspective` is the
    /// container's `(distance, origin x, origin y)` when its children are seen in perspective.
    pub fn from_style(list: &[TransformFn], origin: (StopPos, StopPos), r: Rect) -> Affine {
        Affine::from_style_in(list, origin, r, None)
    }

    pub fn from_style_in(list: &[TransformFn], origin: (StopPos, StopPos), r: Rect, perspective: Option<(f32, f32, f32)>) -> Affine {
        let along = |p: StopPos, whole: f32| match p {
            StopPos::Px(v) => v,
            StopPos::Fraction(f) => f * whole,
        };
        let mut m = M4::IDENTITY;
        for f in list {
            let step = match *f {
                TransformFn::Matrix([a, b, c, d, e, f]) => M4::from_cols([a, b, 0.0, 0.0, c, d, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, e, f, 0.0, 1.0]),
                TransformFn::Translate(x, y) => M4::translate(along(x, r.width), along(y, r.height), 0.0),
                TransformFn::Translate3d(x, y, z) => M4::translate(along(x, r.width), along(y, r.height), z),
                TransformFn::Scale(x, y) => M4::scale(x, y, 1.0),
                TransformFn::Scale3d(x, y, z) => M4::scale(x, y, z),
                TransformFn::Rotate(deg) => M4::rotate([0.0, 0.0, 1.0], deg),
                TransformFn::RotateX(deg) => M4::rotate([1.0, 0.0, 0.0], deg),
                TransformFn::RotateY(deg) => M4::rotate([0.0, 1.0, 0.0], deg),
                TransformFn::Rotate3d([x, y, z, deg]) => M4::rotate([x, y, z], deg),
                TransformFn::Skew(x, y) => {
                    let mut k = M4::IDENTITY;
                    k.0[0][1] = x.to_radians().tan();
                    k.0[1][0] = y.to_radians().tan();
                    k
                }
                TransformFn::Perspective(d) => {
                    let mut k = M4::IDENTITY;
                    k.0[3][2] = -1.0 / d;
                    k
                }
                TransformFn::Matrix3d(v) => M4::from_cols(v),
            };
            // Later functions apply first to the point (CSS multiplies left to right).
            m = m.mul(&step);
        }
        let (ox, oy) = (r.x + along(origin.0, r.width), r.y + along(origin.1, r.height));
        let mut full = M4::translate(ox, oy, 0.0).mul(&m).mul(&M4::translate(-ox, -oy, 0.0));
        if let Some((d, px, py)) = perspective {
            let mut p = M4::IDENTITY;
            p.0[3][2] = -1.0 / d;
            full = M4::translate(px, py, 0.0).mul(&p).mul(&M4::translate(-px, -py, 0.0)).mul(&full);
        }
        full.flatten()
    }
}

/// A 4x4 matrix acting on column vectors `[x, y, z, 1]`; `self.0[row][col]`.
#[derive(Clone, Copy)]
struct M4([[f32; 4]; 4]);

impl M4 {
    const IDENTITY: M4 = M4([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);

    /// From CSS's column-major order.
    fn from_cols(v: [f32; 16]) -> M4 {
        let mut m = [[0.0; 4]; 4];
        for (i, x) in v.iter().enumerate() {
            m[i % 4][i / 4] = *x;
        }
        M4(m)
    }

    fn translate(x: f32, y: f32, z: f32) -> M4 {
        let mut m = M4::IDENTITY;
        m.0[0][3] = x;
        m.0[1][3] = y;
        m.0[2][3] = z;
        m
    }

    fn scale(x: f32, y: f32, z: f32) -> M4 {
        let mut m = M4::IDENTITY;
        m.0[0][0] = x;
        m.0[1][1] = y;
        m.0[2][2] = z;
        m
    }

    /// `rotate3d`: clockwise (as seen on screen, y down) about `axis` by `deg`.
    fn rotate(axis: [f32; 3], deg: f32) -> M4 {
        let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
        if len < 1e-9 {
            return M4::IDENTITY;
        }
        let [x, y, z] = [axis[0] / len, axis[1] / len, axis[2] / len];
        let half = deg.to_radians() / 2.0;
        let (sc, sq) = (half.sin() * half.cos(), half.sin() * half.sin());
        M4::from_cols([
            1.0 - 2.0 * (y * y + z * z) * sq, 2.0 * (x * y * sq + z * sc), 2.0 * (x * z * sq - y * sc), 0.0,
            2.0 * (x * y * sq - z * sc), 1.0 - 2.0 * (x * x + z * z) * sq, 2.0 * (y * z * sq + x * sc), 0.0,
            2.0 * (x * z * sq + y * sc), 2.0 * (y * z * sq - x * sc), 1.0 - 2.0 * (x * x + y * y) * sq, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// `self` applied after `other`.
    fn mul(&self, other: &M4) -> M4 {
        let mut out = [[0.0; 4]; 4];
        for (r, row) in out.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = (0..4).map(|k| self.0[r][k] * other.0[k][c]).sum();
            }
        }
        M4(out)
    }

    /// What this does to points of the page plane (z = 0): columns x, y and the translation.
    fn flatten(&self) -> Affine {
        let m = &self.0;
        Affine { a: m[0][0], c: m[0][1], e: m[0][3], b: m[1][0], d: m[1][1], f: m[1][3], g: m[3][0], h: m[3][1], i: m[3][3] }
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

    #[test]
    fn three_d_transforms_flatten_with_perspective() {
        let r = Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };
        let center = (StopPos::Fraction(0.5), StopPos::Fraction(0.5));
        // Turning a square 60deg about y makes it narrower.
        let m = Affine::from_style(&[TransformFn::RotateY(60.0)], center, r);
        let ((x0, _), (x1, _)) = (m.apply(0.0, 50.0), m.apply(100.0, 50.0));
        assert!((x1 - x0 - 50.0).abs() < 0.1, "{x0} {x1}");
        // With perspective the near edge grows and the far one shrinks.
        let p = Affine::from_style_in(&[TransformFn::RotateY(60.0)], center, r, Some((200.0, 50.0, 50.0)));
        let (_, h_left) = (p.apply(0.0, 0.0).1, p.apply(0.0, 100.0).1 - p.apply(0.0, 0.0).1);
        let h_right = p.apply(100.0, 100.0).1 - p.apply(100.0, 0.0).1;
        assert!(h_left != h_right && (h_left - h_right).abs() > 5.0, "{h_left} {h_right}");
        // Hit testing needs the inverse to bring the point back.
        let (sx, sy) = p.apply(30.0, 70.0);
        let (bx, by) = p.inverse().unwrap().apply(sx, sy);
        assert!((bx - 30.0).abs() < 0.05 && (by - 70.0).abs() < 0.05, "{bx} {by}");
        // translateZ under perspective scales the plane.
        let z = Affine::from_style_in(&[TransformFn::Translate3d(StopPos::Px(0.0), StopPos::Px(0.0), 100.0)], center, r, Some((200.0, 50.0, 50.0)));
        assert!((z.apply(100.0, 50.0).0 - 50.0 - 100.0).abs() < 0.1, "{:?}", z.apply(100.0, 50.0));
    }
}
