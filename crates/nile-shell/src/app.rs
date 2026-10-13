//! The windowed shell: one winit window per `Win`, each with its own `Page`. The first window is
//! the page given on the command line; scripts open more with `new Window({...})` and
//! `Window.this.modal({...})`, which arrive as `WindowCommand`s and are carried out here. Messages,
//! close results and modal blocking between windows are routed here too.

use std::{num::NonZeroU32, path::PathBuf, rc::Rc, time::Instant};

use nile_engine::{window, window::{Frame, InitialState, Resizable, Size, WindowConfig}, Page, SystemClipboard};
use nile_script::{WindowCommand, WindowInfo, WindowSpec};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, Window, WindowId, WindowLevel},
};

use crate::{dialogs, mods_of};

/// What a window's own handling can't do alone.
enum Action {
    Open(WindowSpec),
    Post { from: u32, to: u32, json: String },
    /// Close window `id` (0: the window that asked) with a result.
    Close { id: u32, value: String },
    Focus(u32),
}

struct Win {
    id: u32,
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    page: Page,
    mods: ModifiersState,
    /// The pointer shape last given to the window.
    cursor: Option<nile_style::Cursor>,
    config: WindowConfig,
    /// The title last given to the window.
    title: String,
    /// Which window edge a frameless window's pointer is over, if any.
    edge: Option<(i8, i8)>,
    /// Whether the window's input method is on, and where its candidate window was last put.
    ime_on: bool,
    ime_area: Option<(f32, f32, f32, f32)>,
    /// The window that opened this one, and whether it is a dialog that blocks it.
    opener: Option<u32>,
    modal: bool,
}

pub struct App {
    wins: Vec<Win>,
    /// The page that becomes the first window once the event loop is running.
    first: Option<Page>,
    main_id: u32,
}

impl App {
    pub fn new(page: Page) -> App {
        App { wins: Vec::new(), first: Some(page), main_id: 0 }
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

impl Win {
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
        let wanted = self.page.ime_wanted();
        if wanted != self.ime_on {
            self.window.set_ime_allowed(wanted);
            self.ime_on = wanted;
        }
        if wanted {
            if let Some((x, y, w, h)) = self.page.ime_cursor_rect() {
                if self.ime_area != Some((x, y, w, h)) {
                    self.window.set_ime_cursor_area(LogicalPosition::new(x, y), LogicalSize::new(w, h));
                    self.ime_area = Some((x, y, w, h));
                }
            }
        }
    }

    /// Apply the window actions the page asked for, follow `document.title`, and tell scripts where
    /// the window is. What needs the other windows comes back as actions.
    fn sync_window(&mut self) -> Vec<Action> {
        self.sync_files();
        self.sync_ime();
        let window = self.window.clone();
        let mut actions = Vec::new();
        let requests = std::mem::take(&mut self.page.view.borrow_mut().window_requests);
        for cmd in requests {
            match cmd {
                WindowCommand::Close => actions.push(Action::Close { id: 0, value: "null".into() }),
                WindowCommand::CloseWith(value) => actions.push(Action::Close { id: 0, value }),
                WindowCommand::CloseId(id) => actions.push(Action::Close { id, value: "null".into() }),
                WindowCommand::Open(spec) => actions.push(Action::Open(spec)),
                WindowCommand::Post { to, json } => actions.push(Action::Post { from: self.id, to: if to == 0 { self.id } else { to }, json }),
                WindowCommand::FocusId(id) => actions.push(Action::Focus(if id == 0 { self.id } else { id })),
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
                WindowCommand::MoveTo(x, y) => window.set_outer_position(LogicalPosition::new(x, y)),
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
        self.page.view.borrow_mut().window_rect = Some([pos.x, pos.y, outer.width, outer.height, f32::from(window.fullscreen().is_some())]);
        actions
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
        if let Some(edge) = self.edge {
            self.window.set_cursor(match edge {
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
        use nile_style::Cursor as C;
        self.window.set_cursor_visible(want != C::None);
        self.window.set_cursor(match want {
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

    /// One winit event for this window. True if the user asked to close it.
    fn handle(&mut self, event: WindowEvent) -> bool {
        let window = self.window.clone();
        match event {
            WindowEvent::CloseRequested => return true,
            WindowEvent::Focused(on) => {
                if self.page.window_activated(on) {
                    window.request_redraw();
                }
            }
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
                    return false;
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
                    return false;
                }
                if state == ElementState::Pressed && self.page.caption_press() {
                    let _ = window.drag_window();
                    return false;
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
                let Some(key) = dom_key(&event.logical_key) else { return false };
                let pressed = event.state == ElementState::Pressed;
                let (proceed, changed) = self.page.key(if pressed { "keydown" } else { "keyup" }, key.clone(), mods_of(self.mods));
                // F5 or Ctrl/Cmd+R reloads, unless the page handled the key.
                let reload = key == "F5" || (key.eq_ignore_ascii_case("r") && (self.mods.control_key() || self.mods.super_key()));
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
                    return false;
                };
                self.surface.resize(w, h).expect("resize surface");
                let mut buffer = self.surface.buffer_mut().expect("surface buffer");
                // softbuffer pixels are u32 0RGB = BGRA bytes on little-endian,
                // which is what Skia's BGRA8888 writes.
                let bytes = unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, buffer.len() * 4) };
                self.page.paint(bytes, size.width, size.height, window.scale_factor() as f32);
                buffer.present().expect("present");
            }
            _ => {}
        }
        false
    }
}

/// `WindowSpec` numbers as Sciter's: where `a` is a numpad digit (7 8 9 / 4 5 6 / 1 2 3), the top-left
/// of a `size` box placed in `area` (x, y, w, h).
fn align(a: u8, size: (f32, f32), area: (f32, f32, f32, f32)) -> (f32, f32) {
    let (col, row) = ((a - 1) % 3, 2 - (a - 1) / 3);
    let at = |i: u8, win: f32, start: f32, len: f32| start + match i { 0 => 0.0, 1 => ((len - win) / 2.0).max(0.0), _ => (len - win).max(0.0) };
    (at(col, size.0, area.0, area.2), at(row, size.1, area.1, area.3))
}

impl App {
    fn index(&self, id: u32) -> Option<usize> {
        self.wins.iter().position(|w| w.id == id)
    }

    /// Whether a dialog opened by this window is open: the window ignores input meanwhile.
    fn blocked(&self, idx: usize) -> bool {
        let id = self.wins[idx].id;
        self.wins.iter().any(|w| w.modal && w.opener == Some(id))
    }

    /// Create a window around `page`. `spec` is what a script asked for (none for the first window).
    fn create(&mut self, event_loop: &ActiveEventLoop, page: Page, id: u32, spec: Option<&WindowSpec>) {
        let mut config = WindowConfig::from_document(&page.doc.borrow());
        let opener_rect = spec.and_then(|s| self.index(s.opener)).and_then(|i| self.wins[i].page.view.borrow().window_rect);
        let monitor = event_loop.primary_monitor();
        let desktop = monitor
            .as_ref()
            .map(|m| {
                let sz = m.size().to_logical::<f32>(m.scale_factor());
                (sz.width, sz.height)
            })
            .unwrap_or((1440.0, 900.0));
        let (mut level, mut explicit_pos, mut parent_align) = (WindowLevel::Normal, None, None);
        if let Some(spec) = spec {
            let num = |k: &str| spec.get(k).and_then(|v| v.parse::<f32>().ok());
            if let Some(c) = spec.get("caption") {
                config.title = c.to_string();
            }
            if let Some(w) = num("width") {
                config.width = Some(Size::Px(w));
            }
            if let Some(h) = num("height") {
                config.height = Some(Size::Px(h));
            }
            match spec.get("type") {
                Some("popup" | "child") => {
                    config.frame = Frame::Solid;
                    level = WindowLevel::AlwaysOnTop;
                }
                Some("tool") => {
                    config.minimizable = false;
                    config.maximizable = false;
                    level = WindowLevel::AlwaysOnTop;
                }
                Some("dialog") => {
                    config.minimizable = false;
                    config.maximizable = false;
                }
                _ => {}
            }
            config.state = match spec.get("state") {
                Some("minimized") => InitialState::Minimized,
                Some("maximized") => InitialState::Maximized,
                Some("hidden") => InitialState::Hidden,
                Some("fullscreen") => InitialState::Fullscreen,
                _ => config.state,
            };
            if let (Some(x), Some(y)) = (num("x"), num("y")) {
                explicit_pos = Some((x, y));
            } else if let Some(a) = num("alignment").map(|a| a as i32).filter(|a| a.abs() >= 1 && a.abs() <= 9) {
                if a < 0 {
                    parent_align = Some((-a) as u8);
                } else {
                    config.alignment = Some(a as u8);
                }
            } else if spec.modal || spec.get("type") == Some("dialog") {
                parent_align = Some(5);
            }
        }
        let (w, h) = config.inner_size(desktop);
        let mut attrs = Window::default_attributes()
            .with_title(config.title.clone())
            .with_inner_size(LogicalSize::new(w, h))
            .with_resizable(config.resizable != Resizable::No)
            .with_decorations(!config.frameless())
            .with_transparent(config.frame == Frame::Transparent)
            .with_visible(config.state != InitialState::Hidden)
            .with_maximized(config.state == InitialState::Maximized)
            .with_window_level(level)
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
            .and_then(|i| page.window_icon(i))
            .and_then(|(rgba, w, h)| winit::window::Icon::from_rgba(rgba, w, h).ok())
        {
            attrs = attrs.with_window_icon(Some(icon));
        }
        let window = Rc::new(event_loop.create_window(attrs).expect("create window"));
        let pos = explicit_pos
            .or_else(|| parent_align.zip(opener_rect).map(|(a, r)| align(a, (w, h), (r[0], r[1], r[2], r[3]))))
            .or_else(|| config.position((w, h), desktop));
        if let Some((x, y)) = pos {
            window.set_outer_position(LogicalPosition::new(x, y));
        }
        match config.state {
            InitialState::Minimized => window.set_minimized(true),
            InitialState::Fullscreen => window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None))),
            _ => {}
        }
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        let title = config.title.clone();
        self.wins.push(Win {
            id,
            window,
            surface,
            page,
            mods: ModifiersState::default(),
            cursor: None,
            config,
            title,
            edge: None,
            ime_on: false,
            ime_area: None,
            opener: spec.map(|s| s.opener),
            modal: spec.is_some_and(|s| s.modal),
        });
        let last = self.wins.len() - 1;
        let actions = self.wins[last].sync_window();
        self.run(event_loop, self.wins[last].id, actions);
    }

    /// A page for a window a script asked for, loaded relative to the window that asked.
    fn page_for(&self, spec: &WindowSpec) -> Page {
        let opener = self.index(spec.opener).map(|i| &self.wins[i]);
        let dir = opener.and_then(|w| w.page.path.parent().map(PathBuf::from)).unwrap_or_default();
        let info = WindowInfo { id: spec.id, opener: Some(spec.opener), modal: spec.modal, parameters: spec.get("parameters").unwrap_or("").to_string() };
        let size = (spec.get("width").and_then(|v| v.parse().ok()).unwrap_or(640.0), spec.get("height").and_then(|v| v.parse().ok()).unwrap_or(480.0));
        let mut page = match (spec.get("html"), spec.get("url")) {
            (Some(html), _) => Page::html_window(html, dir, size, info),
            (None, Some(url)) => {
                let rel = url.strip_prefix("file://").unwrap_or(url);
                let file = if std::path::Path::new(rel).is_absolute() { PathBuf::from(rel) } else { dir.join(rel) };
                Page::load_window(file, size, info)
            }
            (None, None) => Page::html_window("<body></body>", dir, size, info),
        };
        page.clipboard = Box::new(SystemClipboard::default());
        page.set_dialogs(Rc::new(dialogs::NativeDialogs));
        page
    }

    /// Carry out what a window's scripts asked for that touches other windows.
    fn run(&mut self, event_loop: &ActiveEventLoop, from: u32, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Open(spec) => {
                    let page = self.page_for(&spec);
                    self.create(event_loop, page, spec.id, Some(&spec));
                }
                Action::Post { from: sender, to, json } => {
                    if let Some(i) = self.index(to) {
                        self.wins[i].page.window_message(sender, &json);
                        self.wins[i].window.request_redraw();
                    }
                }
                Action::Close { id, value } => self.close(event_loop, if id == 0 { from } else { id }, 1, &value),
                Action::Focus(id) => {
                    if let Some(i) = self.index(id) {
                        self.wins[i].window.focus_window();
                    }
                }
            }
        }
    }

    /// Close window `id` unless a `closerequest` listener refuses. Its dialogs and children go with
    /// it, the opener hears the result, and closing the first window ends the program.
    fn close(&mut self, event_loop: &ActiveEventLoop, id: u32, reason: u32, value: &str) {
        let Some(idx) = self.index(id) else { return };
        if !self.wins[idx].page.close_request(reason) {
            return;
        }
        let children: Vec<u32> = self.wins.iter().filter(|w| w.opener == Some(id)).map(|w| w.id).collect();
        for child in children {
            self.close_now(child, "null");
        }
        if id == self.main_id {
            event_loop.exit();
        }
        self.close_now(id, value);
    }

    fn close_now(&mut self, id: u32, value: &str) {
        if self.index(id).is_none() {
            return;
        }
        let children: Vec<u32> = self.wins.iter().filter(|w| w.opener == Some(id)).map(|w| w.id).collect();
        for child in children {
            self.close_now(child, "null");
        }
        let Some(idx) = self.index(id) else { return };
        let win = self.wins.remove(idx);
        if let Some(opener) = win.opener.and_then(|o| self.index(o)) {
            self.wins[opener].page.window_closed(id, value);
            self.wins[opener].window.request_redraw();
        }
    }

    /// After anything happened in window `id`: apply its requests and refresh its cursor.
    fn after(&mut self, event_loop: &ActiveEventLoop, id: u32) {
        if let Some(i) = self.index(id) {
            let actions = self.wins[i].sync_window();
            self.wins[i].sync_cursor();
            self.run(event_loop, id, actions);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Some(page) = self.first.take() else { return };
        let id = page.view.borrow().window_info.id;
        self.main_id = id;
        self.create(event_loop, page, id, None);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let mut next: Option<std::time::Duration> = None;
        for id in self.wins.iter().map(|w| w.id).collect::<Vec<_>>() {
            let Some(i) = self.index(id) else { continue };
            let (changed, n) = self.wins[i].page.tick();
            if changed {
                self.wins[i].window.request_redraw();
            }
            next = [next, n].into_iter().flatten().min();
            self.after(event_loop, id);
        }
        event_loop.set_control_flow(match next {
            Some(d) => ControlFlow::WaitUntil(Instant::now() + d),
            None => ControlFlow::Wait,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, wid: WindowId, event: WindowEvent) {
        let Some(idx) = self.wins.iter().position(|w| w.window.id() == wid) else { return };
        let id = self.wins[idx].id;
        // A window with an open dialog ignores the pointer and keyboard until it closes.
        let input = matches!(
            event,
            WindowEvent::CursorMoved { .. } | WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } | WindowEvent::KeyboardInput { .. } | WindowEvent::Ime(_)
        );
        if input && self.blocked(idx) {
            return;
        }
        if self.wins[idx].handle(event) {
            self.close(event_loop, id, 0, "null");
        }
        self.after(event_loop, id);
    }
}
