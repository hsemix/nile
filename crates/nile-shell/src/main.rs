//! `nile` — opens an HTML file in a native window, or renders it to PNG.
//!
//!   nile page.html                      open a window (F5 / Ctrl-R reloads, wheel scrolls)
//!   nile page.html --png out.png        render headless
//!        [--width 900] [--height 640] [--scale 2]
//!        [--hover X,Y] [--press X,Y] [--click X,Y]   simulate the pointer (logical px)
//!        [--wheel X,Y,DY]              scroll at a point (negative DY scrolls content up)
//!        [--type TEXT]                 type into the focused input ({Backspace}, {Shift+ArrowLeft}, {Cmd+a}, ... for keys)
//!        [--compose TEXT]              show TEXT as an input method's composition at the caret
//!        [--wait MS]                   let timers run this long before the snapshot

mod app;
mod dialogs;

use std::{path::PathBuf, rc::Rc, time::{Duration, Instant}};

use nile_engine::{ModifiersState as Mods, Page, SystemClipboard};
use skia_safe::{AlphaType, ColorType, EncodedImageFormat, ImageInfo};
use winit::{event_loop::EventLoop, keyboard::ModifiersState};

/// winit's modifier state as the engine's.
fn mods_of(m: ModifiersState) -> Mods {
    let mut out = Mods::default();
    for (on, bit) in [(m.shift_key(), Mods::SHIFT), (m.control_key(), Mods::CONTROL), (m.alt_key(), Mods::ALT), (m.super_key(), Mods::SUPER)] {
        if on {
            out |= bit;
        }
    }
    out
}

/// Pointer simulation for headless renders: hover (and optionally press) at a logical point.
struct Pointer {
    x: f32,
    y: f32,
    press: bool,
    /// Release again after pressing (a full click).
    release: bool,
    /// A right click (the `contextmenu` event).
    context: bool,
    /// Wheel delta to apply at the point (positive scrolls content down, like wheel-up).
    wheel: f32,
}

/// Options for a headless render.
struct Snapshot {
    out: String,
    width: u32,
    height: u32,
    scale: f32,
    pointer: Option<Pointer>,
    typed: Option<String>,
    /// `--compose TEXT`: show TEXT as an input method's composition at the caret.
    compose: Option<String>,
    wait_ms: u64,
    settle_ms: u64,
    /// `--select X1,Y1,X2,Y2`: drag a text selection.
    select: Option<[f32; 4]>,
}

fn render_png(page: &mut Page, snap: Snapshot) {
    let Snapshot { out, width, height, scale, pointer, typed, compose, wait_ms, settle_ms, select } = snap;
    let out = out.as_str();
    // Let timers run (animations, setTimeout) before the snapshot.
    let wait = |page: &mut Page, ms: u64| {
        let deadline = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < deadline {
            let (_, next) = page.tick();
            let Some(next) = next else { break };
            std::thread::sleep(next.min(deadline.saturating_duration_since(Instant::now())));
        }
        page.run_timers();
    };
    wait(page, wait_ms);
    if let Some(p) = pointer {
        // Lay out first so there is something to hit test.
        page.layout(width as f32, height as f32);
        page.pointer_moved(p.x, p.y);
        if p.press {
            page.press();
        }
        if p.release {
            page.release();
        }
        if p.context {
            page.context_menu();
        }
        if p.wheel != 0.0 {
            page.wheel(0.0, p.wheel);
        }
    }
    if let Some([x1, y1, x2, y2]) = select {
        page.layout(width as f32, height as f32);
        page.pointer_moved(x1, y1);
        page.press();
        page.pointer_moved(x2, y2);
        page.release();
    }
    if let Some(text) = typed {
        let mut rest = text.as_str();
        while !rest.is_empty() {
            let (key, tail) = match rest.strip_prefix('{').and_then(|r| r.split_once('}')) {
                Some((name, tail)) => (name.to_string(), tail),
                None => {
                    let c = rest.chars().next().unwrap();
                    (c.to_string(), &rest[c.len_utf8()..])
                }
            };
            rest = tail;
            // `{Shift+ArrowRight}`, `{Cmd+a}`, `{Ctrl+Shift+End}`: modifiers before the key.
            let mut parts: Vec<&str> = key.split('+').collect();
            let name = parts.pop().unwrap_or("");
            let key = if name == "Space" { " ".to_string() } else { name.to_string() };
            let mut mods = Mods::default();
            for m in parts {
                match m.to_ascii_lowercase().as_str() {
                    "shift" => mods |= Mods::SHIFT,
                    "ctrl" | "control" => mods |= Mods::CONTROL,
                    "alt" => mods |= Mods::ALT,
                    "cmd" | "super" | "meta" => mods |= Mods::SUPER,
                    _ => {}
                }
            }
            let (proceed, _) = page.key("keydown", key.clone(), mods);
            if proceed {
                page.default_key(&key, mods);
            }
            page.key("keyup", key, mods);
        }
    }
    if let Some(text) = compose {
        page.ime_preedit(&text, None);
    }
    // After the input: transitions it started, and anything it scheduled.
    wait(page, settle_ms);
    let (pw, ph) = ((width as f32 * scale) as u32, (height as f32 * scale) as u32);
    let mut pixels = vec![0u8; (pw * ph * 4) as usize];
    page.paint(&mut pixels, pw, ph, scale);

    let info = ImageInfo::new((pw as i32, ph as i32), ColorType::BGRA8888, AlphaType::Premul, None);
    let image = skia_safe::images::raster_from_data(
        &info,
        skia_safe::Data::new_copy(&pixels),
        pw as usize * 4,
    )
    .expect("pixel buffer matches image info");
    let png = image.encode(None, EncodedImageFormat::PNG, None).expect("PNG encoding");
    std::fs::write(out, png.as_bytes()).expect("writing PNG");
    println!("wrote {out} ({pw}x{ph})");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut file = None;
    let mut png = None;
    let mut pointer = None;
    let mut wait_ms = 0u64;
    let mut settle_ms = 0u64;
    let mut select = None;
    let mut typed = None;
    let mut compose = None;
    let (mut width, mut height, mut scale) = (900u32, 640u32, 1.0f32);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--png" => png = args.next(),
            "--width" => width = args.next().and_then(|v| v.parse().ok()).unwrap_or(width),
            "--height" => height = args.next().and_then(|v| v.parse().ok()).unwrap_or(height),
            "--hover" | "--press" | "--click" | "--rclick" | "--wheel" => {
                // X,Y (or X,Y,DY for --wheel), in logical px.
                let nums: Vec<f32> = args.next().map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect()).unwrap_or_default();
                if let [x, y, rest @ ..] = nums.as_slice() {
                    pointer = Some(Pointer {
                        x: *x,
                        y: *y,
                        press: arg == "--press" || arg == "--click",
                        release: arg == "--click",
                        context: arg == "--rclick",
                        wheel: if arg == "--wheel" { rest.first().copied().unwrap_or(0.0) } else { 0.0 },
                    });
                }
            }
            "--type" => typed = args.next(),
            "--compose" => compose = args.next(),
            "--select" => {
                let v: Vec<f32> = args.next().unwrap_or_default().split(',').filter_map(|n| n.parse().ok()).collect();
                if let [a, b, c, d] = v[..] {
                    select = Some([a, b, c, d]);
                }
            }
            "--settle" => settle_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--wait" => wait_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--scale" => scale = args.next().and_then(|v| v.parse().ok()).unwrap_or(scale),
            _ => file = Some(PathBuf::from(arg)),
        }
    }
    let Some(file) = file else {
        eprintln!("usage: nile <file.html> [--png out.png] [--width N] [--height N] [--scale N] [--hover X,Y] [--press X,Y] [--click X,Y] [--wheel X,Y,DY] [--type TEXT] [--wait MS]");
        std::process::exit(2);
    };

    // A window knows its id before its scripts run, so `new Window()` at startup can name its opener.
    let info = nile_script::WindowInfo { id: nile_script::next_window_id(), ..Default::default() };
    let mut page = if png.is_none() { Page::load_window(file, (width as f32, height as f32), info) } else { Page::load(file, (width as f32, height as f32), 0.0) };
    if png.is_none() {
        page.clipboard = Box::new(SystemClipboard::default());
        page.set_dialogs(Rc::new(dialogs::NativeDialogs));
    }
    if let Some(out) = png {
        render_png(&mut page, Snapshot { out, width, height, scale, pointer, typed, compose, wait_ms, settle_ms, select });
        return;
    }

    let event_loop = EventLoop::new().expect("event loop");
    let mut app = app::App::new(page);
    event_loop.run_app(&mut app).expect("run app");
}
