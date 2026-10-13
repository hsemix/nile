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

mod dialogs;

use std::{num::NonZeroU32, path::PathBuf, rc::Rc, time::{Duration, Instant}};

use nile_engine::{window, window::{Frame, InitialState, Resizable, WindowConfig}, ModifiersState as Mods, Page, SystemClipboard};
use nile_script::WindowCommand;
use skia_safe::{AlphaType, ColorType, EncodedImageFormat, ImageInfo};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, Window, WindowId},
};

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

struct App {
    page: Page,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    mods: ModifiersState,
    /// The pointer shape last given to the window.
    cursor: Option<nile_style::Cursor>,
    close_requested: bool,
    config: WindowConfig,
    /// The title last given to the window.
    title: String,
    /// Which window edge a frameless window's pointer is over, if any.
    edge: Option<(i8, i8)>,
    /// Whether the window's input method is on, and where its candidate window was last put.
    ime_on: bool,
    ime_area: Option<(f32, f32, f32, f32)>,
}

impl App {
    /// Answer a click on `<input type=file>` with the system's file chooser.
    fn sync_files(&mut self) {
        if let Some(request) = self.page.take_file_request() {
            let paths = dialogs::pick_files(request.multiple, &request.accept);
            if !paths.is_empty() {
                self.page.choose_files(request.node, &paths);
            }
        }
    }

    /// Keep the window's input method in step with the page: on while a text field has focus,
    /// with the candidate window next to the caret.
    fn sync_ime(&mut self) {
        let Some(window) = self.window.clone() else { return };
        let wanted = self.page.ime_wanted();
        if wanted != self.ime_on {
            window.set_ime_allowed(wanted);
            self.ime_on = wanted;
        }
        if wanted {
            if let Some((x, y, w, h)) = self.page.ime_cursor_rect() {
                if self.ime_area != Some((x, y, w, h)) {
                    window.set_ime_cursor_area(LogicalPosition::new(x, y), LogicalSize::new(w, h));
                    self.ime_area = Some((x, y, w, h));
                }
            }
        }
    }

    /// Apply window actions the page asked for, follow `document.title`, and tell scripts where
    /// the window is.
    fn sync_window(&mut self) {
        self.sync_files();
        self.sync_ime();
        let Some(window) = self.window.clone() else { return };
        let requests = std::mem::take(&mut self.page.view.borrow_mut().window_requests);
        for cmd in requests {
            match cmd {
                WindowCommand::Close => self.close_requested = true,
                WindowCommand::Minimize => window.set_minimized(true),
                WindowCommand::Maximize => window.set_maximized(true),
                WindowCommand::Restore => {
                    window.set_fullscreen(None);
                    window.set_minimized(false);
                    window.set_maximized(false);
                }
                WindowCommand::ToggleMaximize => {
                    if self.config.maximizable && self.config.resizable != Resizable::No {
                        window.set_maximized(!window.is_maximized());
                    }
                }
                WindowCommand::Fullscreen(on) => {
                    window.set_fullscreen(on.then_some(winit::window::Fullscreen::Borderless(None)));
                }
                WindowCommand::MoveTo(x, y) => window.set_outer_position(winit::dpi::LogicalPosition::new(x, y)),
                WindowCommand::ResizeTo(w, h) => {
                    let _ = window.request_inner_size(LogicalSize::new(w.max(1.0), h.max(1.0)));
                }
            }
        }
        let title = window::current_title(&self.page.doc.borrow(), &self.config);
        if title != self.title {
            window.set_title(&title);
            self.title = title;
        }
        let scale = window.scale_factor();
        let pos = window.outer_position().map(|p| p.to_logical::<f32>(scale)).unwrap_or_default();
        let outer = window.outer_size().to_logical::<f32>(scale);
        self.page.view.borrow_mut().window_rect =
            Some([pos.x, pos.y, outer.width, outer.height, f32::from(window.fullscreen().is_some())]);
    }

    /// For a frameless, resizable window: the margin in which the edges resize it.
    fn resize_margin(&self) -> Option<f32> {
        match (self.config.frameless(), self.config.resizable) {
            (true, Resizable::Yes) => Some(6.0),
            (true, Resizable::Margin(m)) => Some(m),
            _ => None,
        }
    }

    /// Show the CSS `cursor` of whatever is under the pointer.
    fn sync_cursor(&mut self) {
        if let (Some(edge), Some(w)) = (self.edge, &self.window) {
            w.set_cursor(match edge {
                (-1, -1) | (1, 1) => CursorIcon::NwseResize,
                (1, -1) | (-1, 1) => CursorIcon::NeswResize,
                (0, _) => CursorIcon::NsResize,
                _ => CursorIcon::EwResize,
            });
            self.cursor = None;
            return;
        }
        let want = self.page.cursor();
        if self.cursor == Some(want) {
            return;
        }
        self.cursor = Some(want);
        let Some(w) = &self.window else { return };
        use nile_style::Cursor as C;
        w.set_cursor_visible(want != C::None);
        w.set_cursor(match want {
            C::Auto | C::Default | C::None => CursorIcon::Default,
            C::Pointer => CursorIcon::Pointer,
            C::Text => CursorIcon::Text,
            C::Crosshair => CursorIcon::Crosshair,
            C::Move => CursorIcon::Move,
            C::NotAllowed => CursorIcon::NotAllowed,
            C::Wait => CursorIcon::Wait,
            C::Progress => CursorIcon::Progress,
            C::Help => CursorIcon::Help,
            C::Grab => CursorIcon::Grab,
            C::Grabbing => CursorIcon::Grabbing,
            C::ResizeH => CursorIcon::ColResize,
            C::ResizeV => CursorIcon::RowResize,
        });
    }
}

/// DOM `KeyboardEvent.key` for a winit key.
fn dom_key(key: &Key) -> Option<String> {
    Some(match key {
        Key::Character(c) => c.to_string(),
        Key::Named(NamedKey::Space) => " ".into(),
        Key::Named(n) => format!("{n:?}"),
        _ => return None,
    })
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let config = WindowConfig::from_document(&self.page.doc.borrow());
        let desktop = event_loop
            .primary_monitor()
            .map(|m| {
                let sz = m.size().to_logical::<f32>(m.scale_factor());
                (sz.width, sz.height)
            })
            .unwrap_or((1440.0, 900.0));
        let (w, h) = config.inner_size(desktop);
        let mut attrs = Window::default_attributes()
            .with_title(config.title.clone())
            .with_inner_size(LogicalSize::new(w, h))
            .with_resizable(config.resizable != Resizable::No)
            .with_decorations(!config.frameless())
            .with_transparent(config.frame == Frame::Transparent)
            .with_visible(config.state != InitialState::Hidden)
            .with_maximized(config.state == InitialState::Maximized)
            .with_enabled_buttons({
                let mut b = winit::window::WindowButtons::CLOSE;
                if config.minimizable {
                    b |= winit::window::WindowButtons::MINIMIZE;
                }
                if config.maximizable && config.resizable != Resizable::No {
                    b |= winit::window::WindowButtons::MAXIMIZE;
                }
                b
            });
        if let (Some(min_w), Some(min_h)) = (config.min_width, config.min_height) {
            attrs = attrs.with_min_inner_size(LogicalSize::new(min_w, min_h));
        } else if config.min_width.is_some() || config.min_height.is_some() {
            attrs = attrs.with_min_inner_size(LogicalSize::new(config.min_width.unwrap_or(0.0), config.min_height.unwrap_or(0.0)));
        }
        if config.max_width.is_some() || config.max_height.is_some() {
            attrs = attrs.with_max_inner_size(LogicalSize::new(config.max_width.unwrap_or(f32::MAX / 4.0), config.max_height.unwrap_or(f32::MAX / 4.0)));
        }
        #[cfg(target_os = "macos")]
        if config.frame == Frame::Extended {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs = attrs.with_titlebar_transparent(true).with_title_hidden(true).with_fullsize_content_view(true);
        }
        if let Some(icon) = config
            .icon
            .as_deref()
            .and_then(|i| self.page.window_icon(i))
            .and_then(|(rgba, w, h)| winit::window::Icon::from_rgba(rgba, w, h).ok())
        {
            attrs = attrs.with_window_icon(Some(icon));
        }
        let window = Rc::new(event_loop.create_window(attrs).expect("create window"));
        if let Some((x, y)) = config.position((w, h), desktop) {
            window.set_outer_position(winit::dpi::LogicalPosition::new(x, y));
        }
        match config.state {
            InitialState::Minimized => window.set_minimized(true),
            InitialState::Fullscreen => window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None))),
            _ => {}
        }
        self.title = config.title.clone();
        self.config = config;
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        self.surface = Some(surface);
        self.window = Some(window);
        self.sync_window();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let (changed, next) = self.page.tick();
        self.sync_window();
        if self.close_requested {
            event_loop.exit();
        }
        if changed {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        event_loop.set_control_flow(match next {
            Some(d) => ControlFlow::WaitUntil(Instant::now() + d),
            None => ControlFlow::Wait,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.handle_window_event(event_loop, id, event);
        self.sync_window();
        self.sync_cursor();
        if self.close_requested {
            event_loop.exit();
        }
    }
}

impl App {
    fn handle_window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => window.request_redraw(),
            WindowEvent::CursorMoved { position, .. } => {
                let scale = window.scale_factor();
                // A frameless window's edges resize it before the page sees the pointer.
                self.edge = self.resize_margin().and_then(|m| {
                    let inner = window.inner_size().to_logical::<f32>(scale);
                    window::edge_at((position.x / scale) as f32, (position.y / scale) as f32, (inner.width, inner.height), m)
                });
                if self.edge.is_some() {
                    if self.page.pointer_left() {
                        window.request_redraw();
                    }
                    return;
                }
                if self.page.pointer_moved((position.x / scale) as f32, (position.y / scale) as f32) {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.page.pointer_left() {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                if let (Some(edge), ElementState::Pressed) = (self.edge, state) {
                    use winit::window::ResizeDirection as D;
                    let _ = window.drag_resize_window(match edge {
                        (-1, -1) => D::NorthWest,
                        (1, -1) => D::NorthEast,
                        (-1, 1) => D::SouthWest,
                        (1, 1) => D::SouthEast,
                        (0, -1) => D::North,
                        (0, 1) => D::South,
                        (-1, _) => D::West,
                        _ => D::East,
                    });
                    return;
                }
                if state == ElementState::Pressed && self.page.caption_press() {
                    let _ = window.drag_window();
                    return;
                }
                let changed = match state {
                    ElementState::Pressed => self.page.press(),
                    ElementState::Released => self.page.release(),
                };
                if changed {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } => {
                if self.page.context_menu() {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 40.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / window.scale_factor() as f32,
                };
                let dx = match delta {
                    MouseScrollDelta::LineDelta(x, _) => x * 40.0,
                    MouseScrollDelta::PixelDelta(p) => p.x as f32 / window.scale_factor() as f32,
                };
                self.page.wheel(dx, dy);
                window.request_redraw();
            }
            WindowEvent::Ime(ime) => {
                use winit::event::Ime;
                match ime {
                    Ime::Preedit(text, cursor) => {
                        self.page.ime_preedit(&text, cursor);
                        window.request_redraw();
                    }
                    Ime::Commit(text) => {
                        self.page.ime_commit(&text);
                        window.request_redraw();
                    }
                    Ime::Disabled => {
                        self.page.ime_cancel();
                        self.ime_on = false;
                        window.request_redraw();
                    }
                    Ime::Enabled => {}
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
                self.page.mods = mods_of(self.mods);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let Some(key) = dom_key(&event.logical_key) else { return };
                let pressed = event.state == ElementState::Pressed;
                let (proceed, changed) = self.page.key(if pressed { "keydown" } else { "keyup" }, key.clone(), mods_of(self.mods));
                // F5 or Ctrl/Cmd+R reloads, unless the page handled the key.
                let reload = key == "F5"
                    || (key.eq_ignore_ascii_case("r") && (self.mods.control_key() || self.mods.super_key()));
                let edited = pressed && proceed && !reload && self.page.default_key(&key, mods_of(self.mods));
                if pressed && proceed && reload {
                    self.page.reload();
                    window.request_redraw();
                } else if changed || edited {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                let size = window.inner_size();
                let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
                    return;
                };
                let surface = self.surface.as_mut().unwrap();
                surface.resize(w, h).expect("resize surface");
                let mut buffer = surface.buffer_mut().expect("surface buffer");

                // softbuffer pixels are u32 0RGB = BGRA bytes on little-endian,
                // which is what Skia's BGRA8888 writes.
                let bytes = unsafe {
                    std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, buffer.len() * 4)
                };
                self.page.paint(bytes, size.width, size.height, window.scale_factor() as f32);
                buffer.present().expect("present");
            }
            _ => {}
        }
    }
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

    let mut page = Page::load(file, (width as f32, height as f32), 0.0);
    if png.is_none() {
        page.clipboard = Box::new(SystemClipboard::default());
        page.set_dialogs(Rc::new(dialogs::NativeDialogs));
    }
    if let Some(out) = png {
        render_png(&mut page, Snapshot { out, width, height, scale, pointer, typed, compose, wait_ms, settle_ms, select });
        return;
    }

    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App { page, window: None, surface: None, mods: ModifiersState::default(), cursor: None, close_requested: false, config: WindowConfig::default(), title: String::new(), edge: None, ime_on: false, ime_area: None };
    event_loop.run_app(&mut app).expect("run app");
}
