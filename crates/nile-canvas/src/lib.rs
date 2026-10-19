//! The 2D drawing context behind `<canvas>`: Skia raster surfaces kept per canvas, and one entry
//! point ([`call`]) that scripts drive with the Canvas 2D API's operation names.
//!
//! The renderer reads a canvas back with [`snapshot`]; [`take_dirty`] tells the page that a canvas
//! changed so it repaints without a new layout. Everything is per thread (the script runtime and
//! the renderer share the thread).

mod text;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use base64::Engine;
use skia_safe::{
    gradient::{interpolation::InPremul, Colors, Gradient as SkGradient, Interpolation},
    surfaces, AlphaType, BlendMode, Canvas, Color4f, ColorType, Data, EncodedImageFormat, FilterMode, Image, ImageInfo, MipmapMode, Paint,
    PaintCap, PaintJoin, PaintStyle, Path, PathBuilder, PathFillType, Point, RRect, Rect, SamplingOptions, Shader, Surface, TileMode,
};

/// Where `drawImage` and `createPattern` get an `<img>`'s encoded bytes from.
pub trait Images {
    fn bytes(&self, src: &str) -> Option<Vec<u8>>;
}

/// No images: `drawImage` of an `<img>` draws nothing.
pub struct NoImages;

impl Images for NoImages {
    fn bytes(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

/// A fill or stroke source.
#[derive(Clone)]
enum Source {
    Color(Color4f),
    Gradient(u32),
    Pattern(u32),
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Start,
    End,
    Left,
    Right,
    Center,
}

#[derive(Clone, Copy, PartialEq)]
enum Baseline {
    Top,
    Hanging,
    Middle,
    Alphabetic,
    Ideographic,
    Bottom,
}

/// The parts of the context that `save()` and `restore()` keep (the transform and the clip live in
/// Skia's own state).
#[derive(Clone)]
struct State {
    fill: Source,
    stroke: Source,
    alpha: f32,
    blend: BlendMode,
    line_width: f32,
    cap: PaintCap,
    join: PaintJoin,
    miter: f32,
    dash: Vec<f32>,
    dash_offset: f32,
    shadow: Color4f,
    shadow_blur: f32,
    shadow_dx: f32,
    shadow_dy: f32,
    smoothing: bool,
    font: text::FontSpec,
    align: Align,
    baseline: Baseline,
}

impl Default for State {
    fn default() -> Self {
        let black = Color4f::new(0.0, 0.0, 0.0, 1.0);
        State {
            fill: Source::Color(black),
            stroke: Source::Color(black),
            alpha: 1.0,
            blend: BlendMode::SrcOver,
            line_width: 1.0,
            cap: PaintCap::Butt,
            join: PaintJoin::Miter,
            miter: 10.0,
            dash: Vec::new(),
            dash_offset: 0.0,
            shadow: Color4f::new(0.0, 0.0, 0.0, 0.0),
            shadow_blur: 0.0,
            shadow_dx: 0.0,
            shadow_dy: 0.0,
            smoothing: true,
            font: text::FontSpec::default(),
            align: Align::Start,
            baseline: Baseline::Alphabetic,
        }
    }
}

struct Canvas2D {
    surface: Surface,
    width: u32,
    height: u32,
    state: State,
    stack: Vec<State>,
    /// The current path, its points already in device space (transformed when they were added).
    path: PathBuilder,
}

struct Gradient {
    kind: GradientKind,
    stops: Vec<(f32, Color4f)>,
}

enum GradientKind {
    Linear([f32; 4]),
    Radial([f32; 6]),
    Conic(f32, f32, f32),
}

struct Pattern {
    image: Image,
    repeat_x: bool,
    repeat_y: bool,
}

thread_local! {
    static CANVASES: RefCell<HashMap<u32, Canvas2D>> = RefCell::default();
    static GRADIENTS: RefCell<HashMap<u32, Gradient>> = RefCell::default();
    static PATTERNS: RefCell<HashMap<u32, Pattern>> = RefCell::default();
    static PATHS: RefCell<HashMap<u32, PathBuilder>> = RefCell::default();
    static DECODED: RefCell<HashMap<String, Option<Image>>> = RefCell::default();
    static NEXT: Cell<u32> = const { Cell::new(1) };
    static DIRTY: Cell<bool> = const { Cell::new(false) };
}

fn next_id() -> u32 {
    NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

fn dirty() {
    DIRTY.with(|d| d.set(true));
}

/// Whether any canvas was drawn on since the last call (the page then repaints).
pub fn take_dirty() -> bool {
    DIRTY.with(|d| d.replace(false))
}

/// A new canvas of `width` x `height` pixels (cleared, transparent black); its id.
pub fn create(width: u32, height: u32) -> Option<u32> {
    let (w, h) = (width.clamp(1, 16384), height.clamp(1, 16384));
    let surface = surfaces::raster_n32_premul((w as i32, h as i32))?;
    let id = next_id();
    CANVASES.with(|c| c.borrow_mut().insert(id, Canvas2D { surface, width: w, height: h, state: State::default(), stack: Vec::new(), path: PathBuilder::new() }));
    dirty();
    Some(id)
}

/// Change a canvas's size: the bitmap is cleared and the context's state reset, as the spec says.
pub fn resize(id: u32, width: u32, height: u32) {
    CANVASES.with(|c| {
        if let Some(cv) = c.borrow_mut().get_mut(&id) {
            let (w, h) = (width.clamp(1, 16384), height.clamp(1, 16384));
            if let Some(surface) = surfaces::raster_n32_premul((w as i32, h as i32)) {
                *cv = Canvas2D { surface, width: w, height: h, state: State::default(), stack: Vec::new(), path: PathBuilder::new() };
                dirty();
            }
        }
    });
}

/// Forget a canvas (its element went away).
pub fn remove(id: u32) {
    CANVASES.with(|c| c.borrow_mut().remove(&id));
}

pub fn size(id: u32) -> Option<(u32, u32)> {
    CANVASES.with(|c| c.borrow().get(&id).map(|cv| (cv.width, cv.height)))
}

/// A copy of the canvas's pixels as an image, for the renderer to draw.
pub fn snapshot(id: u32) -> Option<Image> {
    CANVASES.with(|c| c.borrow_mut().get_mut(&id).map(|cv| cv.surface.image_snapshot()))
}

fn color4f(c: nile_style::Color) -> Color4f {
    Color4f::new(f32::from(c.r) / 255.0, f32::from(c.g) / 255.0, f32::from(c.b) / 255.0, f32::from(c.a) / 255.0)
}

/// How `fillStyle` reads back: `#rrggbb`, or `rgba(...)` when translucent.
fn canonical(c: nile_style::Color) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        let a = (f32::from(c.a) / 255.0 * 100.0).round() / 100.0;
        format!("rgba({}, {}, {}, {a})", c.r, c.g, c.b)
    }
}

fn blend_mode(name: &str) -> Option<BlendMode> {
    Some(match name {
        "source-over" => BlendMode::SrcOver,
        "source-in" => BlendMode::SrcIn,
        "source-out" => BlendMode::SrcOut,
        "source-atop" => BlendMode::SrcATop,
        "destination-over" => BlendMode::DstOver,
        "destination-in" => BlendMode::DstIn,
        "destination-out" => BlendMode::DstOut,
        "destination-atop" => BlendMode::DstATop,
        "lighter" => BlendMode::Plus,
        "copy" => BlendMode::Src,
        "xor" => BlendMode::Xor,
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        "overlay" => BlendMode::Overlay,
        "darken" => BlendMode::Darken,
        "lighten" => BlendMode::Lighten,
        "color-dodge" => BlendMode::ColorDodge,
        "color-burn" => BlendMode::ColorBurn,
        "hard-light" => BlendMode::HardLight,
        "soft-light" => BlendMode::SoftLight,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        "hue" => BlendMode::Hue,
        "saturation" => BlendMode::Saturation,
        "color" => BlendMode::Color,
        "luminosity" => BlendMode::Luminosity,
        _ => return None,
    })
}

fn fill_type(rule: f64) -> PathFillType {
    if rule == 1.0 {
        PathFillType::EvenOdd
    } else {
        PathFillType::Winding
    }
}

/// The shader for a gradient: its stops, in the user space the canvas has now.
fn gradient_shader(g: &Gradient) -> Option<Shader> {
    if g.stops.is_empty() {
        return None;
    }
    let mut stops = g.stops.clone();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let colors: Vec<Color4f> = stops.iter().map(|s| s.1).collect();
    let positions: Vec<f32> = stops.iter().map(|s| s.0).collect();
    let sk = SkGradient::new(Colors::new(&colors, Some(&positions), TileMode::Clamp, None), Interpolation { in_premul: InPremul::Yes, ..Interpolation::default() });
    match g.kind {
        GradientKind::Linear([x0, y0, x1, y1]) => skia_safe::shaders::linear_gradient(((x0, y0), (x1, y1)), &sk, None),
        GradientKind::Radial([x0, y0, r0, x1, y1, r1]) => skia_safe::shaders::two_point_conical_gradient(((x0, y0), r0), ((x1, y1), r1), &sk, None),
        GradientKind::Conic(angle, x, y) => {
            let m = skia_safe::Matrix::rotate_deg_pivot(angle.to_degrees() - 90.0, (x, y));
            skia_safe::shaders::sweep_gradient((x, y), (0.0, 360.0), &sk, Some(&m))
        }
    }
}

fn pattern_shader(p: &Pattern) -> Option<Shader> {
    let mode = |repeat: bool| if repeat { TileMode::Repeat } else { TileMode::Decal };
    p.image.to_shader((mode(p.repeat_x), mode(p.repeat_y)), SamplingOptions::new(FilterMode::Linear, MipmapMode::None), None)
}

impl Canvas2D {
    fn canvas(&mut self) -> &Canvas {
        self.surface.canvas()
    }

    /// A paint for the current fill or stroke source, with alpha, blend mode and shadow applied.
    fn paint(&self, source: &Source, style: PaintStyle) -> Option<Paint> {
        let st = &self.state;
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_style(style);
        paint.set_blend_mode(st.blend);
        match source {
            Source::Color(c) => {
                paint.set_color4f(Color4f::new(c.r, c.g, c.b, c.a * st.alpha), None);
            }
            Source::Gradient(id) => {
                let shader = GRADIENTS.with(|g| g.borrow().get(id).and_then(gradient_shader))?;
                paint.set_shader(shader);
                paint.set_alpha_f(st.alpha);
            }
            Source::Pattern(id) => {
                let shader = PATTERNS.with(|p| p.borrow().get(id).and_then(pattern_shader))?;
                paint.set_shader(shader);
                paint.set_alpha_f(st.alpha);
            }
        }
        if style == PaintStyle::Stroke {
            paint.set_stroke_width(st.line_width);
            paint.set_stroke_cap(st.cap);
            paint.set_stroke_join(st.join);
            paint.set_stroke_miter(st.miter);
            if st.dash.len() >= 2 && st.dash.len() & 1 == 0 {
                paint.set_path_effect(skia_safe::PathEffect::dash(&st.dash, st.dash_offset));
            }
        }
        if st.shadow.a > 0.0 && (st.shadow_blur > 0.0 || st.shadow_dx != 0.0 || st.shadow_dy != 0.0) {
            let sigma = st.shadow_blur / 2.0;
            paint.set_image_filter(skia_safe::image_filters::drop_shadow((st.shadow_dx, st.shadow_dy), (sigma, sigma), st.shadow, None, None, None));
        }
        Some(paint)
    }

    fn fill_paint(&self) -> Option<Paint> {
        self.paint(&self.state.fill.clone(), PaintStyle::Fill)
    }

    fn stroke_paint(&self) -> Option<Paint> {
        self.paint(&self.state.stroke.clone(), PaintStyle::Stroke)
    }

    /// The current transform as a matrix.
    fn ctm(&mut self) -> skia_safe::Matrix {
        self.canvas().local_to_device_as_3x3()
    }

    /// Fill `path` (in the space `device` says) with the fill source.
    fn fill_path(&mut self, path: &Path, device: bool) {
        let Some(mut paint) = self.fill_paint() else { return };
        let ctm = self.ctm();
        self.set_local_matrix(&mut paint, &self.state.fill.clone(), &ctm, device);
        let canvas = self.surface.canvas();
        if device {
            canvas.save();
            canvas.reset_matrix();
            canvas.draw_path(path, &paint);
            canvas.restore();
        } else {
            canvas.draw_path(path, &paint);
        }
        dirty();
    }

    /// Stroke `path`: the stroke is made in user space, so the line width follows the transform.
    fn stroke_path(&mut self, path: &Path, device: bool) {
        let Some(paint) = self.stroke_paint() else { return };
        let ctm = self.ctm();
        let user = if device { ctm.invert().map(|inv| path.with_transform(&inv)) } else { Some(path.clone()) };
        let Some(user) = user else { return };
        self.surface.canvas().draw_path(&user, &paint);
        dirty();
    }

    /// Gradients and patterns are defined in user space: they take the transform in force.
    fn set_local_matrix(&self, paint: &mut Paint, source: &Source, ctm: &skia_safe::Matrix, device: bool) {
        if !matches!(source, Source::Gradient(_) | Source::Pattern(_)) {
            return;
        }
        // A device-space path is drawn with an identity matrix, so the shader needs the transform itself.
        if device {
            if let Some(shader) = paint.shader() {
                paint.set_shader(shader.with_local_matrix(ctm));
            }
        }
    }
}

fn arg(args: &[f64], i: usize) -> f32 {
    args.get(i).copied().unwrap_or(0.0) as f32
}

fn rect_of(args: &[f64]) -> Rect {
    let (x, y, w, h) = (arg(args, 0), arg(args, 1), arg(args, 2), arg(args, 3));
    Rect::from_xywh(x.min(x + w), y.min(y + h), w.abs(), h.abs())
}

fn map(m: &skia_safe::Matrix, x: f32, y: f32) -> Point {
    m.map_point((x, y))
}

/// The arc of an ellipse as a path piece: spec angles are clockwise from +x in a y-down space.
fn ellipse_path((x, y): (f32, f32), (rx, ry): (f32, f32), rotation: f32, (start, end): (f64, f64), ccw: bool) -> Path {
    let tau = std::f64::consts::TAU;
    let mut sweep = end - start;
    if !ccw && sweep >= tau || ccw && -sweep >= tau {
        sweep = if ccw { -tau } else { tau };
    } else if !ccw {
        sweep = sweep.rem_euclid(tau);
    } else {
        sweep = -((-sweep).rem_euclid(tau));
    }
    let oval = Rect::from_ltrb(-rx, -ry, rx, ry);
    let mut p = PathBuilder::new();
    let start_deg = start.to_degrees() as f32;
    let half = 180.0 * sweep.signum() as f32;
    if sweep.abs() >= tau - 1e-9 {
        // A full ellipse is two half turns (Skia's arc_to can't do 360 in one go).
        p.arc_to(oval, start_deg, half, true);
        p.arc_to(oval, start_deg + half, half, false);
    } else {
        p.arc_to(oval, start_deg, sweep.to_degrees() as f32, true);
    }
    let mut m = skia_safe::Matrix::translate((x, y));
    m.pre_rotate(rotation.to_degrees(), None);
    p.snapshot_and_transform(&m)
}

/// Run one canvas operation. `args` and `text` carry the operation's numbers and string; the
/// result is the operation's answer as text (empty when there is none).
pub fn call(id: u32, op: &str, args: &[f64], text: &str, images: &dyn Images) -> Result<String, String> {
    // Objects that aren't a context: gradients, patterns, Path2D, the image decoder.
    match op {
        "createLinearGradient" | "createRadialGradient" | "createConicGradient" => {
            let kind = match op {
                "createLinearGradient" => GradientKind::Linear([arg(args, 0), arg(args, 1), arg(args, 2), arg(args, 3)]),
                "createRadialGradient" => GradientKind::Radial([arg(args, 0), arg(args, 1), arg(args, 2), arg(args, 3), arg(args, 4), arg(args, 5)]),
                _ => GradientKind::Conic(arg(args, 0), arg(args, 1), arg(args, 2)),
            };
            let gid = next_id();
            GRADIENTS.with(|g| g.borrow_mut().insert(gid, Gradient { kind, stops: Vec::new() }));
            return Ok(gid.to_string());
        }
        "addColorStop" => {
            let Some(color) = nile_style::parse_color(text) else { return Err("SyntaxError: invalid color".into()) };
            let offset = arg(args, 1);
            if !(0.0..=1.0).contains(&offset) {
                return Err("IndexSizeError: offset must be between 0 and 1".into());
            }
            GRADIENTS.with(|g| {
                if let Some(grad) = g.borrow_mut().get_mut(&(arg(args, 0) as u32)) {
                    grad.stops.push((offset, color4f(color)));
                }
            });
            return Ok(String::new());
        }
        "createPattern" => {
            let (source, repetition) = text.split_once('|').unwrap_or((text, "repeat"));
            let Some(image) = source_image(source, images) else { return Ok(String::new()) };
            let (repeat_x, repeat_y) = match repetition {
                "repeat-x" => (true, false),
                "repeat-y" => (false, true),
                "no-repeat" => (false, false),
                _ => (true, true),
            };
            let pid = next_id();
            PATTERNS.with(|p| p.borrow_mut().insert(pid, Pattern { image, repeat_x, repeat_y }));
            return Ok(pid.to_string());
        }
        "newPath" => {
            let pid = next_id();
            PATHS.with(|p| p.borrow_mut().insert(pid, PathBuilder::new()));
            return Ok(pid.to_string());
        }
        "pathFromSvg" => {
            let pid = next_id();
            PATHS.with(|p| p.borrow_mut().insert(pid, Path::from_svg(text).map_or_else(PathBuilder::new, |path| PathBuilder::new_path(&path))));
            return Ok(pid.to_string());
        }
        "pathOp" => return path_op(args, text),
        "imageSize" => {
            return Ok(source_image(text, images).map(|i| format!("[{},{}]", i.width(), i.height())).unwrap_or_default());
        }
        _ => {}
    }
    // The source of a drawImage may be another canvas, so it is read before this one is borrowed.
    let image = if op == "drawImage" { source_image(text, images) } else { None };
    CANVASES.with(|all| {
        let mut all = all.borrow_mut();
        let cv = all.get_mut(&id).ok_or_else(|| format!("unknown canvas {id}"))?;
        run(cv, op, args, text, image)
    })
}

/// Methods on a `Path2D` object (`text` is `"<path id>:<method>"`).
fn path_op(args: &[f64], text: &str) -> Result<String, String> {
    let (pid, name) = text.split_once(':').unwrap_or((text, ""));
    let pid: u32 = pid.parse().map_err(|_| "bad path".to_string())?;
    PATHS.with(|paths| {
        let mut paths = paths.borrow_mut();
        if name == "addPath" {
            // args = [other id, a, b, c, d, e, f]
            let other = paths.get(&(arg(args, 0) as u32)).map(PathBuilder::snapshot).ok_or("unknown path")?;
            let m = skia_safe::Matrix::new_all(arg(args, 1), arg(args, 3), arg(args, 5), arg(args, 2), arg(args, 4), arg(args, 6), 0.0, 0.0, 1.0);
            paths.get_mut(&pid).ok_or("unknown path")?.add_path_with_transform(&other, &m, skia_safe::path::AddPathMode::Append);
            return Ok(String::new());
        }
        let path = paths.get_mut(&pid).ok_or_else(|| "unknown path".to_string())?;
        build(path, &skia_safe::Matrix::new_identity(), name, args)
    })
}

/// Path construction shared by the context's current path and `Path2D`, with `m` taking the
/// operation's coordinates to the path's space.
fn build(path: &mut PathBuilder, m: &skia_safe::Matrix, op: &str, a: &[f64]) -> Result<String, String> {
    let append = skia_safe::path::AddPathMode::Append;
    let extend = skia_safe::path::AddPathMode::Extend;
    match op {
        "closePath" => {
            path.close();
        }
        "moveTo" => {
            path.move_to(map(m, arg(a, 0), arg(a, 1)));
        }
        "lineTo" => {
            let p = map(m, arg(a, 0), arg(a, 1));
            if path.is_empty() {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        "quadraticCurveTo" => {
            if path.is_empty() {
                path.move_to(map(m, arg(a, 0), arg(a, 1)));
            }
            path.quad_to(map(m, arg(a, 0), arg(a, 1)), map(m, arg(a, 2), arg(a, 3)));
        }
        "bezierCurveTo" => {
            if path.is_empty() {
                path.move_to(map(m, arg(a, 0), arg(a, 1)));
            }
            path.cubic_to(map(m, arg(a, 0), arg(a, 1)), map(m, arg(a, 2), arg(a, 3)), map(m, arg(a, 4), arg(a, 5)));
        }
        "rect" => {
            let mut seg = PathBuilder::new();
            seg.add_rect(rect_of(a), None, None);
            path.add_path_with_transform(&seg.snapshot(), m, append);
        }
        "roundRect" => {
            let radii = [Point::new(arg(a, 4), arg(a, 4)), Point::new(arg(a, 5), arg(a, 5)), Point::new(arg(a, 6), arg(a, 6)), Point::new(arg(a, 7), arg(a, 7))];
            let mut seg = PathBuilder::new();
            seg.add_rrect(RRect::new_rect_radii(rect_of(a), &radii), None, None);
            path.add_path_with_transform(&seg.snapshot(), m, append);
        }
        "arc" | "ellipse" => {
            let (x, y) = (arg(a, 0), arg(a, 1));
            let flag = |i: usize| a.get(i).copied().unwrap_or(0.0) != 0.0;
            let (rx, ry, rot, start, end, ccw) = if op == "arc" { (arg(a, 2), arg(a, 2), 0.0, a[3], a[4], flag(5)) } else { (arg(a, 2), arg(a, 3), arg(a, 4), a[5], a[6], flag(7)) };
            if rx < 0.0 || ry < 0.0 {
                return Err("IndexSizeError: negative radius".into());
            }
            // Joined to the path's last point with a line, as the spec says.
            path.add_path_with_transform(&ellipse_path((x, y), (rx, ry), rot, (start, end), ccw), m, extend);
        }
        "arcTo" => {
            let (x1, y1, x2, y2, r) = (arg(a, 0), arg(a, 1), arg(a, 2), arg(a, 3), arg(a, 4));
            if r < 0.0 {
                return Err("IndexSizeError: negative radius".into());
            }
            if path.is_empty() {
                path.move_to(map(m, x1, y1));
            }
            // The tangent arc is worked out in user space, where the coordinates were given.
            if let Some(inv) = m.invert() {
                let mut user = PathBuilder::new_path(&path.snapshot_and_transform(&inv));
                user.arc_to_tangent((x1, y1), (x2, y2), r);
                *path = PathBuilder::new_path(&user.snapshot_and_transform(m));
            }
        }
        other => return Err(format!("unknown path operation {other}")),
    }
    Ok(String::new())
}

/// The image behind a drawImage/createPattern source: `img:<src>` (decoded from the page's
/// bytes, cached) or `cv:<id>` (another canvas's current pixels).
fn source_image(source: &str, images: &dyn Images) -> Option<Image> {
    if let Some(id) = source.strip_prefix("cv:") {
        return snapshot(id.parse().ok()?);
    }
    let src = source.strip_prefix("img:")?;
    if let Some(hit) = DECODED.with(|d| d.borrow().get(src).cloned()) {
        return hit;
    }
    let image = images.bytes(src).and_then(|b| Image::from_encoded(Data::new_copy(&b)));
    DECODED.with(|d| {
        let mut d = d.borrow_mut();
        if d.len() > 64 {
            d.clear();
        }
        d.insert(src.to_string(), image.clone());
    });
    image
}

fn run(cv: &mut Canvas2D, op: &str, a: &[f64], text: &str, source: Option<Image>) -> Result<String, String> {
    match op {
        // ----- state -----
        "save" => {
            cv.stack.push(cv.state.clone());
            cv.canvas().save();
        }
        "restore" => {
            if let Some(s) = cv.stack.pop() {
                cv.state = s;
                cv.canvas().restore();
            }
        }
        "reset" => {
            let (w, h) = (cv.width, cv.height);
            if let Some(surface) = surfaces::raster_n32_premul((w as i32, h as i32)) {
                *cv = Canvas2D { surface, width: w, height: h, state: State::default(), stack: Vec::new(), path: PathBuilder::new() };
                dirty();
            }
        }
        "scale" => {
            cv.canvas().scale((arg(a, 0), arg(a, 1)));
        }
        "rotate" => {
            cv.canvas().rotate((arg(a, 0)).to_degrees(), None);
        }
        "translate" => {
            cv.canvas().translate((arg(a, 0), arg(a, 1)));
        }
        "transform" | "setTransform" => {
            let m = skia_safe::Matrix::new_all(arg(a, 0), arg(a, 2), arg(a, 4), arg(a, 1), arg(a, 3), arg(a, 5), 0.0, 0.0, 1.0);
            if op == "setTransform" {
                cv.canvas().reset_matrix();
            }
            cv.canvas().concat(&m);
        }
        "resetTransform" => {
            cv.canvas().reset_matrix();
        }
        "getTransform" => {
            let m = cv.ctm();
            return Ok(format!("[{},{},{},{},{},{}]", m.scale_x(), m.skew_y(), m.skew_x(), m.scale_y(), m.translate_x(), m.translate_y()));
        }
        // ----- state properties -----
        "set:globalAlpha" => {
            let v = arg(a, 0);
            if (0.0..=1.0).contains(&v) {
                cv.state.alpha = v;
            }
        }
        "set:globalCompositeOperation" => {
            if let Some(b) = blend_mode(text) {
                cv.state.blend = b;
                return Ok(text.to_string());
            }
        }
        "set:lineWidth" => {
            let v = arg(a, 0);
            if v > 0.0 && v.is_finite() {
                cv.state.line_width = v;
            }
        }
        "set:lineCap" => {
            cv.state.cap = match text {
                "butt" => PaintCap::Butt,
                "round" => PaintCap::Round,
                "square" => PaintCap::Square,
                _ => return Ok(String::new()),
            };
            return Ok(text.to_string());
        }
        "set:lineJoin" => {
            cv.state.join = match text {
                "miter" => PaintJoin::Miter,
                "round" => PaintJoin::Round,
                "bevel" => PaintJoin::Bevel,
                _ => return Ok(String::new()),
            };
            return Ok(text.to_string());
        }
        "set:miterLimit" => {
            let v = arg(a, 0);
            if v > 0.0 && v.is_finite() {
                cv.state.miter = v;
            }
        }
        "setLineDash" => {
            let dash: Vec<f32> = a.iter().map(|v| *v as f32).collect();
            if dash.iter().all(|v| *v >= 0.0 && v.is_finite()) {
                // An odd list repeats itself to make an even one.
                cv.state.dash = if dash.len() % 2 == 1 { dash.iter().chain(dash.iter()).copied().collect() } else { dash };
            }
        }
        "set:lineDashOffset" => cv.state.dash_offset = arg(a, 0),
        "set:shadowBlur" => {
            if arg(a, 0) >= 0.0 {
                cv.state.shadow_blur = arg(a, 0);
            }
        }
        "set:shadowOffsetX" => cv.state.shadow_dx = arg(a, 0),
        "set:shadowOffsetY" => cv.state.shadow_dy = arg(a, 0),
        "set:shadowColor" => {
            if let Some(c) = nile_style::parse_color(text) {
                cv.state.shadow = color4f(c);
                return Ok(canonical(c));
            }
        }
        "set:imageSmoothingEnabled" => cv.state.smoothing = arg(a, 0) != 0.0,
        "set:fillStyle" | "set:strokeStyle" => {
            let source = if let Some(id) = text.strip_prefix("grad:") {
                Source::Gradient(id.parse().map_err(|_| "bad gradient")?)
            } else if let Some(id) = text.strip_prefix("pat:") {
                Source::Pattern(id.parse().map_err(|_| "bad pattern")?)
            } else if let Some(c) = nile_style::parse_color(text) {
                if op == "set:fillStyle" {
                    cv.state.fill = Source::Color(color4f(c));
                } else {
                    cv.state.stroke = Source::Color(color4f(c));
                }
                return Ok(canonical(c));
            } else {
                return Ok(String::new());
            };
            if op == "set:fillStyle" {
                cv.state.fill = source;
            } else {
                cv.state.stroke = source;
            }
            return Ok("ok".into());
        }
        "set:font" => {
            return Ok(match text::FontSpec::parse(text) {
                Some(f) => {
                    let canon = f.canonical();
                    cv.state.font = f;
                    canon
                }
                None => String::new(),
            });
        }
        "set:textAlign" => {
            cv.state.align = match text {
                "start" => Align::Start,
                "end" => Align::End,
                "left" => Align::Left,
                "right" => Align::Right,
                "center" => Align::Center,
                _ => return Ok(String::new()),
            };
            return Ok(text.to_string());
        }
        "set:textBaseline" => {
            cv.state.baseline = match text {
                "top" => Baseline::Top,
                "hanging" => Baseline::Hanging,
                "middle" => Baseline::Middle,
                "alphabetic" => Baseline::Alphabetic,
                "ideographic" => Baseline::Ideographic,
                "bottom" => Baseline::Bottom,
                _ => return Ok(String::new()),
            };
            return Ok(text.to_string());
        }
        // ----- rectangles -----
        "clearRect" => {
            let mut paint = Paint::default();
            paint.set_blend_mode(BlendMode::Clear);
            cv.canvas().draw_rect(rect_of(a), &paint);
            dirty();
        }
        "fillRect" => {
            let mut p = PathBuilder::new();
            p.add_rect(rect_of(a), None, None);
            if let Some(paint) = cv.fill_paint() {
                cv.canvas().draw_path(&p.snapshot(), &paint);
                dirty();
            }
        }
        "strokeRect" => {
            let mut p = PathBuilder::new();
            p.add_rect(rect_of(a), None, None);
            cv.stroke_path(&p.snapshot(), false);
        }
        // ----- paths -----
        "beginPath" => cv.path = PathBuilder::new(),
        "closePath" | "moveTo" | "lineTo" | "quadraticCurveTo" | "bezierCurveTo" | "rect" | "roundRect" | "arc" | "ellipse" | "arcTo" => {
            let m = cv.ctm();
            return build(&mut cv.path, &m, op, a);
        }
        "fill" | "stroke" | "clip" => {
            // `text` names a Path2D ("" = the current path); a[0] is the fill rule.
            let (mut path, device) = match text.parse::<u32>().ok().and_then(|pid| PATHS.with(|p| p.borrow().get(&pid).map(PathBuilder::snapshot))) {
                Some(p) => (p, false),
                None => (cv.path.snapshot(), true),
            };
            match op {
                "fill" => {
                    path = path.with_fill_type(fill_type(a.first().copied().unwrap_or(0.0)));
                    cv.fill_path(&path, device);
                }
                "stroke" => cv.stroke_path(&path, device),
                _ => {
                    path = path.with_fill_type(fill_type(a.first().copied().unwrap_or(0.0)));
                    let ctm = cv.ctm();
                    // A device-space path is clipped through the inverse of the transform in force.
                    let user = if device { ctm.invert().map(|inv| path.with_transform(&inv)) } else { Some(path) };
                    if let Some(user) = user {
                        cv.canvas().clip_path(&user, skia_safe::ClipOp::Intersect, true);
                    }
                }
            }
        }
        "isPointInPath" | "isPointInStroke" => {
            let (x, y) = (arg(a, 0), arg(a, 1));
            let (mut path, device) = match text.parse::<u32>().ok().and_then(|pid| PATHS.with(|p| p.borrow().get(&pid).map(PathBuilder::snapshot))) {
                Some(p) => (p, false),
                None => (cv.path.snapshot(), true),
            };
            let ctm = cv.ctm();
            let point = if device { map(&ctm, x, y) } else { Point::new(x, y) };
            if !device {
                path = path.with_transform(&ctm);
            }
            let hit = if op == "isPointInPath" {
                path = path.with_fill_type(fill_type(f64::from(arg(a, 2))));
                path.contains(point)
            } else {
                let mut paint = Paint::default();
                paint.set_style(PaintStyle::Stroke);
                paint.set_stroke_width(cv.state.line_width * ctm.scale_x().abs().max(ctm.scale_y().abs()));
                let mut outline = Path::new();
                skia_safe::path_utils::fill_path_with_paint(&path, &paint, &mut outline, None, None) && outline.contains(point)
            };
            return Ok(if hit { "1" } else { "0" }.into());
        }
        // ----- text -----
        "fillText" | "strokeText" => {
            let (x, y) = (arg(a, 0), arg(a, 1));
            let max = a.get(2).copied().filter(|m| m.is_finite() && *m >= 0.0).map(|m| m as f32);
            let style = if op == "fillText" { PaintStyle::Fill } else { PaintStyle::Stroke };
            let source = if op == "fillText" { cv.state.fill.clone() } else { cv.state.stroke.clone() };
            if let Some(paint) = cv.paint(&source, style) {
                text::draw(cv, text, x, y, max, &paint);
                dirty();
            }
        }
        "measureText" => return Ok(text::measure(&cv.state, text)),
        // ----- images -----
        "drawImage" => {
            let Some(image) = source else { return Ok(String::new()) };
            let (iw, ih) = (image.width() as f32, image.height() as f32);
            // a = [sx, sy, sw, sh, dx, dy, dw, dh]; NaN source = the whole image, NaN size = natural.
            let (mut sx, mut sy, mut sw, mut sh) = (arg(a, 0), arg(a, 1), arg(a, 2), arg(a, 3));
            if sw.is_nan() || sh.is_nan() {
                (sx, sy, sw, sh) = (0.0, 0.0, iw, ih);
            }
            let (dx, dy) = (arg(a, 4), arg(a, 5));
            let (dw, dh) = (if arg(a, 6).is_nan() { sw } else { arg(a, 6) }, if arg(a, 7).is_nan() { sh } else { arg(a, 7) });
            let Some(mut paint) = cv.paint(&Source::Color(Color4f::new(0.0, 0.0, 0.0, 1.0)), PaintStyle::Fill) else { return Ok(String::new()) };
            paint.set_alpha_f(cv.state.alpha);
            let sampling = if cv.state.smoothing { SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear) } else { SamplingOptions::new(FilterMode::Nearest, MipmapMode::None) };
            let src = Rect::from_xywh(sx, sy, sw, sh);
            let dst = Rect::from_xywh(dx, dy, dw, dh);
            cv.canvas().draw_image_rect_with_sampling_options(&image, Some((&src, skia_safe::canvas::SrcRectConstraint::Strict)), dst, sampling, &paint);
            dirty();
        }
        "getImageData" => {
            let (sx, sy, sw, sh) = (arg(a, 0) as i32, arg(a, 1) as i32, arg(a, 2) as i32, arg(a, 3) as i32);
            if sw <= 0 || sh <= 0 {
                return Err("IndexSizeError: the source width and height must be nonzero".into());
            }
            let info = ImageInfo::new((sw, sh), ColorType::RGBA8888, AlphaType::Unpremul, None);
            let mut bytes = vec![0u8; sw as usize * sh as usize * 4];
            // Pixels outside the canvas stay transparent black.
            cv.surface.read_pixels(&info, &mut bytes, sw as usize * 4, (sx, sy));
            return Ok(base64::engine::general_purpose::STANDARD.encode(bytes));
        }
        "putImageData" => {
            // text = base64 RGBA; a = [w, h, dx, dy].
            let bytes = base64::engine::general_purpose::STANDARD.decode(text).map_err(|e| e.to_string())?;
            let (w, h) = (arg(a, 0) as i32, arg(a, 1) as i32);
            if w <= 0 || h <= 0 || bytes.len() < w as usize * h as usize * 4 {
                return Err("InvalidStateError: bad image data".into());
            }
            let info = ImageInfo::new((w, h), ColorType::RGBA8888, AlphaType::Unpremul, None);
            // putImageData replaces pixels: no transform, clip, alpha or compositing applies.
            let _ = cv.canvas().write_pixels(&info, &bytes, w as usize * 4, (arg(a, 2) as i32, arg(a, 3) as i32));
            dirty();
        }
        "toDataURL" => {
            let (format, mime) = match text.split(';').next().unwrap_or("") {
                "image/jpeg" => (EncodedImageFormat::JPEG, "image/jpeg"),
                "image/webp" => (EncodedImageFormat::WEBP, "image/webp"),
                _ => (EncodedImageFormat::PNG, "image/png"),
            };
            let quality = if a.first().is_some_and(|q| (0.0..=1.0).contains(q)) { (a[0] * 100.0) as u32 } else { 92 };
            let image = cv.surface.image_snapshot();
            #[allow(deprecated)]
            let data = image.encode_to_data_with_quality(format, quality).ok_or("encoding failed")?;
            return Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(data.as_bytes())));
        }
        other => return Err(format!("unknown canvas operation {other}")),
    }
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(id: u32, name: &str, args: &[f64]) -> String {
        call(id, name, args, "", &NoImages).unwrap()
    }
    fn set(id: u32, name: &str, value: &str) -> String {
        call(id, name, &[], value, &NoImages).unwrap()
    }
    fn px(id: u32, x: i32, y: i32) -> [u8; 4] {
        let data = op(id, "getImageData", &[f64::from(x), f64::from(y), 1.0, 1.0]);
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).unwrap();
        [bytes[0], bytes[1], bytes[2], bytes[3]]
    }

    #[test]
    fn rectangles_and_colours() {
        let id = create(20, 20).unwrap();
        assert_eq!(px(id, 5, 5), [0, 0, 0, 0], "a new canvas is transparent");
        assert_eq!(set(id, "set:fillStyle", "red"), "#ff0000");
        assert_eq!(set(id, "set:fillStyle", "rgba(0, 0, 255, 0.5)"), "rgba(0, 0, 255, 0.5)");
        assert_eq!(set(id, "set:fillStyle", "not a colour"), "", "an invalid colour is ignored");
        set(id, "set:fillStyle", "#00ff00");
        op(id, "fillRect", &[2.0, 2.0, 10.0, 10.0]);
        assert_eq!(px(id, 5, 5), [0, 255, 0, 255]);
        assert_eq!(px(id, 15, 15), [0, 0, 0, 0]);
        op(id, "clearRect", &[0.0, 0.0, 6.0, 6.0]);
        assert_eq!(px(id, 5, 5), [0, 0, 0, 0]);
        assert_eq!(px(id, 8, 8), [0, 255, 0, 255]);
        assert!(take_dirty());
    }

    #[test]
    fn transforms_save_and_restore() {
        let id = create(40, 40).unwrap();
        set(id, "set:fillStyle", "#ff0000");
        op(id, "save", &[]);
        op(id, "translate", &[20.0, 20.0]);
        op(id, "scale", &[2.0, 2.0]);
        op(id, "fillRect", &[0.0, 0.0, 5.0, 5.0]);
        assert_eq!(px(id, 25, 25), [255, 0, 0, 255]);
        assert_eq!(px(id, 35, 35), [0, 0, 0, 0]);
        assert_eq!(op(id, "getTransform", &[]), "[2,0,0,2,20,20]");
        op(id, "restore", &[]);
        assert_eq!(op(id, "getTransform", &[]), "[1,0,0,1,0,0]");
        op(id, "fillRect", &[0.0, 0.0, 3.0, 3.0]);
        assert_eq!(px(id, 1, 1), [255, 0, 0, 255]);
        op(id, "setTransform", &[1.0, 0.0, 0.0, 1.0, 30.0, 30.0]);
        op(id, "fillRect", &[0.0, 0.0, 5.0, 5.0]);
        assert_eq!(px(id, 32, 32), [255, 0, 0, 255]);
    }

    #[test]
    fn paths_fill_stroke_and_clip() {
        let id = create(60, 60).unwrap();
        set(id, "set:fillStyle", "#0000ff");
        op(id, "beginPath", &[]);
        op(id, "moveTo", &[10.0, 10.0]);
        op(id, "lineTo", &[50.0, 10.0]);
        op(id, "lineTo", &[10.0, 50.0]);
        op(id, "closePath", &[]);
        op(id, "fill", &[0.0]);
        assert_eq!(px(id, 15, 15), [0, 0, 255, 255]);
        assert_eq!(px(id, 45, 45), [0, 0, 0, 0], "outside the triangle");
        // A full circle, then the stroke of a line with a width.
        op(id, "beginPath", &[]);
        op(id, "arc", &[30.0, 30.0, 8.0, 0.0, std::f64::consts::TAU, 0.0]);
        set(id, "set:fillStyle", "#ffff00");
        op(id, "fill", &[0.0]);
        assert_eq!(px(id, 30, 30), [255, 255, 0, 255]);
        assert_eq!(px(id, 30, 40), [0, 0, 0, 0]);
        set(id, "set:strokeStyle", "#ff0000");
        set(id, "set:lineWidth", "");
        op(id, "set:lineWidth", &[6.0]);
        op(id, "beginPath", &[]);
        op(id, "moveTo", &[5.0, 55.0]);
        op(id, "lineTo", &[55.0, 55.0]);
        op(id, "stroke", &[]);
        assert_eq!(px(id, 30, 55), [255, 0, 0, 255]);
        assert_eq!(px(id, 30, 52), [255, 0, 0, 255], "the stroke is 6 wide");
        assert_eq!(px(id, 30, 48), [0, 0, 0, 0]);
        // A clip limits later drawing.
        op(id, "save", &[]);
        op(id, "beginPath", &[]);
        op(id, "rect", &[0.0, 0.0, 5.0, 5.0]);
        op(id, "clip", &[0.0]);
        set(id, "set:fillStyle", "#00ff00");
        op(id, "fillRect", &[0.0, 0.0, 20.0, 20.0]);
        assert_eq!(px(id, 2, 2), [0, 255, 0, 255]);
        assert_eq!(px(id, 8, 2), [0, 0, 0, 0]);
        op(id, "restore", &[]);
    }

    #[test]
    fn gradients_alpha_and_compositing() {
        let id = create(40, 10).unwrap();
        let g = op(id, "createLinearGradient", &[0.0, 0.0, 40.0, 0.0]);
        let gid: f64 = g.parse().unwrap();
        call(id, "addColorStop", &[gid, 0.0], "#000000", &NoImages).unwrap();
        call(id, "addColorStop", &[gid, 1.0], "#ffffff", &NoImages).unwrap();
        assert!(call(id, "addColorStop", &[gid, 2.0], "#fff", &NoImages).is_err());
        assert!(call(id, "addColorStop", &[gid, 0.5], "nope", &NoImages).is_err());
        assert_eq!(set(id, "set:fillStyle", &format!("grad:{g}")), "ok");
        op(id, "fillRect", &[0.0, 0.0, 40.0, 10.0]);
        let (a, b) = (px(id, 4, 5)[0], px(id, 36, 5)[0]);
        assert!(a < 60 && b > 195, "left dark, right light: {a} {b}");
        // globalAlpha and destination-out.
        let id = create(10, 10).unwrap();
        set(id, "set:fillStyle", "#ff0000");
        op(id, "set:globalAlpha", &[0.5]);
        op(id, "fillRect", &[0.0, 0.0, 10.0, 10.0]);
        let p = px(id, 5, 5);
        assert!((120..=135).contains(&p[3]) && p[0] >= 250, "{p:?}");
        op(id, "set:globalAlpha", &[1.0]);
        assert_eq!(set(id, "set:globalCompositeOperation", "destination-out"), "destination-out");
        op(id, "fillRect", &[0.0, 0.0, 5.0, 10.0]);
        assert_eq!(px(id, 2, 5)[3], 0);
        assert_eq!(set(id, "set:globalCompositeOperation", "bogus"), "");
    }

    #[test]
    fn image_data_round_trips_and_encodes() {
        let id = create(4, 4).unwrap();
        let rgba: Vec<u8> = (0..16).flat_map(|i| [i as u8 * 10, 100, 200, 255]).collect();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&rgba);
        call(id, "putImageData", &[4.0, 4.0, 0.0, 0.0], &b64, &NoImages).unwrap();
        assert_eq!(px(id, 1, 0), [10, 100, 200, 255]);
        assert_eq!(px(id, 3, 3), [150, 100, 200, 255]);
        assert_eq!(px(id, 10, 10), [0, 0, 0, 0], "outside the canvas reads transparent");
        let url = call(id, "toDataURL", &[], "image/png", &NoImages).unwrap();
        assert!(url.starts_with("data:image/png;base64,iVBOR"), "{url}");
        assert!(call(id, "getImageData", &[0.0, 0.0, 0.0, 4.0], "", &NoImages).is_err());
    }

    #[test]
    fn draw_image_between_canvases_and_patterns() {
        let src = create(4, 4).unwrap();
        set(src, "set:fillStyle", "#ff00ff");
        op(src, "fillRect", &[0.0, 0.0, 4.0, 4.0]);
        let dst = create(20, 20).unwrap();
        // Whole image scaled up to 8x8 at (2, 2): a = [sx, sy, sw, sh, dx, dy, dw, dh].
        call(dst, "drawImage", &[f64::NAN, f64::NAN, f64::NAN, f64::NAN, 2.0, 2.0, 8.0, 8.0], &format!("cv:{src}"), &NoImages).unwrap();
        assert_eq!(px(dst, 5, 5), [255, 0, 255, 255]);
        assert_eq!(px(dst, 12, 12), [0, 0, 0, 0]);
        let pat = call(dst, "createPattern", &[], &format!("cv:{src}|repeat"), &NoImages).unwrap();
        set(dst, "set:fillStyle", &format!("pat:{pat}"));
        op(dst, "fillRect", &[10.0, 10.0, 10.0, 10.0]);
        assert_eq!(px(dst, 15, 15), [255, 0, 255, 255]);
    }

    #[test]
    fn path2d_and_hit_testing() {
        let id = create(30, 30).unwrap();
        let pid = call(id, "pathFromSvg", &[], "M2 2 L22 2 L22 22 L2 22 Z", &NoImages).unwrap();
        set(id, "set:fillStyle", "#123456");
        call(id, "fill", &[0.0], &pid, &NoImages).unwrap();
        assert_eq!(px(id, 10, 10), [0x12, 0x34, 0x56, 255]);
        assert_eq!(call(id, "isPointInPath", &[10.0, 10.0, 0.0], &pid, &NoImages).unwrap(), "1");
        assert_eq!(call(id, "isPointInPath", &[28.0, 28.0, 0.0], &pid, &NoImages).unwrap(), "0");
        let built = call(id, "newPath", &[], "", &NoImages).unwrap();
        for (name, args) in [("moveTo", [0.0, 0.0]), ("lineTo", [10.0, 0.0])] {
            call(id, "pathOp", &args, &format!("{built}:{name}"), &NoImages).unwrap();
        }
        assert!(call(id, "pathOp", &[], &format!("{built}:nonsense"), &NoImages).is_err());
    }

    #[test]
    fn text_draws_and_measures() {
        let id = create(120, 40).unwrap();
        assert_eq!(set(id, "set:font", "bold 20px sans-serif"), "bold 20px sans-serif");
        assert_eq!(set(id, "set:font", "garbage"), "");
        set(id, "set:fillStyle", "#000000");
        call(id, "fillText", &[4.0, 28.0], "Hello", &NoImages).unwrap();
        let drawn = base64::engine::general_purpose::STANDARD.decode(op(id, "getImageData", &[0.0, 0.0, 120.0, 40.0])).unwrap();
        assert!(drawn.chunks(4).any(|p| p[3] > 200), "some pixels were inked");
        let m = call(id, "measureText", &[], "Hello", &NoImages).unwrap();
        let width: f64 = m.split("\"width\":").nth(1).unwrap().split(',').next().unwrap().parse().unwrap();
        assert!((30.0..100.0).contains(&width), "{m}");
        let wider = call(id, "measureText", &[], "Hello Hello", &NoImages).unwrap();
        let w2: f64 = wider.split("\"width\":").nth(1).unwrap().split(',').next().unwrap().parse().unwrap();
        assert!(w2 > width * 1.8);
    }

    #[test]
    fn resizing_clears_and_resets() {
        let id = create(10, 10).unwrap();
        set(id, "set:fillStyle", "#ff0000");
        op(id, "fillRect", &[0.0, 0.0, 10.0, 10.0]);
        resize(id, 20, 5);
        assert_eq!(size(id), Some((20, 5)));
        assert_eq!(px(id, 2, 2), [0, 0, 0, 0]);
        // The fill style went back to its default (black), too.
        op(id, "fillRect", &[0.0, 0.0, 2.0, 2.0]);
        assert_eq!(px(id, 0, 0), [0, 0, 0, 255]);
        remove(id);
        assert!(call(id, "fillRect", &[0.0, 0.0, 1.0, 1.0], "", &NoImages).is_err());
    }
}
