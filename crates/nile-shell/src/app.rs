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
    event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, Window, WindowId, WindowLevel},
};

use crate::{access, dialogs, menu, mods_of};

/// What other threads (the accessibility layer) send the event loop.
pub enum UserEvent {
    Access(accesskit_winit::Event),
    /// A menu bar item was chosen: a page item's node id, or a negative tag for a standard action.
    #[allow(dead_code)]
    Menu(isize),
    /// Windows: a command of window `hwnd`'s menu bar, as the page item's node id.
    #[allow(dead_code)]
    MenuIn(isize, u64),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(e: accesskit_winit::Event) -> Self {
        UserEvent::Access(e)
    }
}

/// What a window's own handling can't do alone.
enum Action {
    Open(WindowSpec),
    Post { from: u32, to: u32, json: String },
    /// Close window `id` (0: the window that asked) with a result.
    Close { id: u32, value: String },
    Focus(u32),
    /// `application.quit(code)`.
    Quit(i32),
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
    /// The window's link to the platform's accessibility API, and a stamp of the page when the
    /// tree was last sent.
    access: accesskit_winit::Adapter,
    access_stamp: u64,
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    wins: Vec<Win>,
    portals: Vec<PortalWin>,
    /// The page that becomes the first window once the event loop is running.
    first: Option<Page>,
    main_id: u32,
    /// The system menu bar (macOS), and the window whose page supplies it.
    menu: Option<menu::NativeMenu>,
    active: u32,
    /// Started from a `run.js`: the first window is a hidden host for the script, and the program ends
    /// when the windows it opened are all closed.
    pub bootstrap: bool,
    /// The exit code `application.quit(code)` asked for.
    pub exit_code: i32,
}

impl App {
    pub fn new(page: Page, proxy: EventLoopProxy<UserEvent>) -> App {
        App { proxy, wins: Vec::new(), portals: Vec::new(), first: Some(page), main_id: 0, menu: None, active: 0, bootstrap: false, exit_code: 0 }
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

/// The window system's pointer shape for a CSS `cursor`.
fn icon_of(c: nile_style::Cursor) -> CursorIcon {
    use nile_style::Cursor as C;
    match c {
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
        C::ResizeNwse => CursorIcon::NwseResize,
        C::ResizeNesw => CursorIcon::NeswResize,
        C::ResizeV => CursorIcon::RowResize,
    }
}

/// A popup the page shows in a window of its own (see `nile_engine::Portal`): the owner's page paints
/// it and gets its pointer and keys, with the pointer translated into the owner's coordinates.
struct PortalWin {
    owner: u32,
    node: nile_dom::NodeId,
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    /// Border box in the owner's viewport coordinates.
    rect: (f32, f32, f32, f32),
}

impl Win {
    /// Send the accessibility tree again if the page has changed (only built when assistive
    /// technology is listening).
    fn sync_access(&mut self) {
        let stamp = {
            let v = self.page.view.borrow();
            let doc = self.page.doc.borrow();
            let mut h = doc.version.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            h ^= (v.scroll_y.to_bits() as u64) << 7;
            h ^= (v.interaction.focus.map_or(0, |f| f.0 as u64 + 1)) << 21;
            h ^= (v.viewport.0.to_bits() as u64) << 3 ^ (v.viewport.1.to_bits() as u64) << 13;
            for slot in self.page.frames.borrow().slots.values() {
                h = h.wrapping_add(slot.page.doc.borrow().version.wrapping_mul(31));
            }
            h
        };
        if stamp == self.access_stamp {
            return;
        }
        self.access_stamp = stamp;
        let scale = self.window.scale_factor();
        let page = &mut self.page;
        self.access.update_if_active(|| access::update(&page.a11y_tree(), scale));
    }

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
                WindowCommand::Quit(code) => actions.push(Action::Quit(code)),
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
        self.window.set_cursor_visible(want != nile_style::Cursor::None);
        self.window.set_cursor(icon_of(want));
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

/// Make `child` an owned window of `owner`: above it, hidden and closed with it. macOS: a child
/// window; Windows: an owner window (set when the window is created); elsewhere winit has no way, so
/// the two stay independent.
#[cfg(target_os = "macos")]
fn attach_to_owner(child: &Window, owner: &Window) {
    use objc2_app_kit::{NSView, NSWindowOrderingMode};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let view = |w: &Window| match w.window_handle().map(|h| h.as_raw()) {
        // SAFETY: winit's AppKit handle is the window's live content view.
        Ok(RawWindowHandle::AppKit(h)) => Some(unsafe { h.ns_view.cast::<NSView>().as_ref() }.window()),
        _ => None,
    };
    if let (Some(Some(child)), Some(Some(owner))) = (view(child), view(owner)) {
        // SAFETY: both are live windows of this process, used on the main thread.
        unsafe { owner.addChildWindow_ordered(&child, NSWindowOrderingMode::Above) };
    }
}

#[cfg(not(target_os = "macos"))]
fn attach_to_owner(_child: &Window, _owner: &Window) {}

/// The Win32 handle of `owner`, for `with_owner_window`.
#[cfg(windows)]
fn owner_hwnd(owner: &Window) -> Option<isize> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match owner.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get()),
        _ => None,
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
        // The window it is owned by: the `parent` it names, else (for dialogs, tool windows, popups
        // and `modal()`) the one that opened it.
        let owner: Option<u32> = spec.and_then(|s| {
            s.get("parent")
                .and_then(|p| p.parse().ok())
                .or_else(|| (s.modal || matches!(s.get("type"), Some("dialog" | "tool" | "popup" | "child"))).then_some(s.opener))
        });
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
            // Shown after the accessibility adapter exists (it must see the window first).
            .with_visible(false)
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
        #[cfg(windows)]
        if let Some(hwnd) = owner.and_then(|o| self.index(o)).and_then(|i| owner_hwnd(&self.wins[i].window)) {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_owner_window(hwnd);
        }
        let window = Rc::new(event_loop.create_window(attrs).expect("create window"));
        if let Some(o) = owner.and_then(|o| self.index(o)) {
            attach_to_owner(&window, &self.wins[o].window);
        }
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
        let access = accesskit_winit::Adapter::with_event_loop_proxy(event_loop, &window, self.proxy.clone());
        window.set_visible(config.state != InitialState::Hidden);
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
            access,
            access_stamp: 0,
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
                let mapped = nile_dom::vfs::url_to_path(url).map(|p| p.to_string_lossy().into_owned());
                let rel = mapped.as_deref().unwrap_or_else(|| nile_script::path_of_url(url));
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
                Action::Quit(code) => {
                    self.exit_code = code;
                    event_loop.exit();
                }
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
        // A bootstrap script's own window is hidden: the program ends with the last window it opened.
        if self.bootstrap && self.wins.iter().all(|w| w.id == self.main_id) {
            event_loop.exit();
        }
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
        self.portals.retain(|p| p.owner != id);
        let win = self.wins.remove(idx);
        if let Some(opener) = win.opener.and_then(|o| self.index(o)) {
            self.wins[opener].page.window_closed(id, value);
            self.wins[opener].window.request_redraw();
        }
    }

    /// After anything happened in window `id`: apply its requests and refresh its cursor and popups.
    fn after(&mut self, event_loop: &ActiveEventLoop, id: u32) {
        if let Some(i) = self.index(id) {
            let actions = self.wins[i].sync_window();
            self.wins[i].sync_cursor();
            self.run(event_loop, id, actions);
            self.sync_portals(event_loop, id);
            if let Some(i) = self.index(id) {
                self.wins[i].sync_access();
            }
        }
        self.sync_menu(id);
    }

    /// Show a window's `Window.this.menu` as the system menu bar: the active window's on macOS (one
    /// bar for the app), each window's own on Windows.
    fn sync_menu(&mut self, id: u32) {
        let Some(menu) = self.menu.as_mut() else { return };
        if cfg!(target_os = "macos") && id != self.active {
            return;
        }
        let Some(i) = self.wins.iter().position(|w| w.id == id) else { return };
        let items = self.wins[i].page.app_menu();
        let name = self.wins.first().map(|w| w.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| "Nile".into());
        menu.set(&self.wins[i].window, &name, items);
    }

    /// A menu item with a standard role that the page didn't handle itself.
    fn menu_role(&mut self, event_loop: &ActiveEventLoop, id: u32, role: &str) {
        match role {
            "quit" => self.close(event_loop, self.main_id, 0, "null"),
            "close" => self.close(event_loop, id, 0, "null"),
            "minimize" => {
                if let Some(i) = self.index(id) {
                    self.wins[i].window.set_minimized(true);
                }
            }
            "zoom" | "maximize" => {
                if let Some(i) = self.index(id) {
                    let w = &self.wins[i].window;
                    w.set_maximized(!w.is_maximized());
                }
            }
            _ => {}
        }
    }

    /// Open, move and close the OS windows of `owner`'s popups to match what its page asks for, and
    /// repaint them (they show the owner's live page).
    fn sync_portals(&mut self, event_loop: &ActiveEventLoop, owner: u32) {
        let Some(oi) = self.index(owner) else {
            self.portals.retain(|p| p.owner != owner);
            return;
        };
        let wanted = self.wins[oi].page.view.borrow().portals.clone();
        self.portals.retain(|p| p.owner != owner || wanted.iter().any(|w| w.node == p.node));
        let scale = self.wins[oi].window.scale_factor();
        let origin = self.wins[oi].window.inner_position().map(|p| p.to_logical::<f32>(scale)).unwrap_or_default();
        for w in wanted {
            let (x, y, pw, ph) = w.rect;
            let size = LogicalSize::new(pw.max(1.0), ph.max(1.0));
            let at = LogicalPosition::new(origin.x + x, origin.y + y);
            match self.portals.iter_mut().find(|p| p.owner == owner && p.node == w.node) {
                Some(p) => {
                    if p.rect != w.rect {
                        p.rect = w.rect;
                        let _ = p.window.request_inner_size(size);
                        p.window.set_outer_position(at);
                    }
                    p.window.request_redraw();
                }
                None => {
                    let attrs = Window::default_attributes()
                        .with_decorations(false)
                        .with_resizable(false)
                        .with_active(false)
                        .with_window_level(WindowLevel::AlwaysOnTop)
                        .with_inner_size(size)
                        .with_position(at)
                        .with_title("");
                    let window = Rc::new(event_loop.create_window(attrs).expect("create popup window"));
                    let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
                    let surface = softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
                    window.request_redraw();
                    self.portals.push(PortalWin { owner, node: w.node, window, surface, rect: w.rect });
                }
            }
        }
    }

    /// One winit event for a popup window.
    fn portal_event(&mut self, event_loop: &ActiveEventLoop, pi: usize, event: WindowEvent) {
        let (owner, rect) = (self.portals[pi].owner, self.portals[pi].rect);
        let Some(oi) = self.index(owner) else { return };
        match event {
            WindowEvent::RedrawRequested => {
                let portal = &mut self.portals[pi];
                let size = portal.window.inner_size();
                let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
                portal.surface.resize(w, h).expect("resize surface");
                let mut buffer = portal.surface.buffer_mut().expect("surface buffer");
                let bytes = unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, buffer.len() * 4) };
                self.wins[oi].page.paint_portal(bytes, size.width, size.height, portal.window.scale_factor() as f32, portal.node, (rect.0, rect.1));
                buffer.present().expect("present");
                return;
            }
            WindowEvent::CursorMoved { position, .. } => {
                let s = self.portals[pi].window.scale_factor();
                if self.wins[oi].page.pointer_moved((position.x / s) as f32 + rect.0, (position.y / s) as f32 + rect.1) {
                    self.wins[oi].window.request_redraw();
                }
                self.portals[pi].window.set_cursor(icon_of(self.wins[oi].page.cursor()));
            }
            WindowEvent::CursorLeft { .. } => {
                self.wins[oi].page.pointer_left();
                self.wins[oi].window.request_redraw();
            }
            // Presses, wheel and keys go to the owner as if they had happened there.
            e @ (WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } | WindowEvent::KeyboardInput { .. } | WindowEvent::ModifiersChanged(_) | WindowEvent::Ime(_)) => {
                self.wins[oi].handle(e);
            }
            _ => {}
        }
        self.after(event_loop, owner);
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Some(page) = self.first.take() else { return };
        let id = page.view.borrow().window_info.id;
        self.main_id = id;
        self.active = id;
        self.menu = menu::NativeMenu::new(self.proxy.clone());
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

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let event = match event {
            UserEvent::Access(event) => event,
            UserEvent::MenuIn(hwnd, node) => {
                let Some(idx) = self.wins.iter().position(|w| w.window.id() == WindowId::from(hwnd as u64)) else { return };
                let id = self.wins[idx].id;
                let role = self.wins[idx].page.menu_click(node);
                self.wins[idx].window.request_redraw();
                if let Some(role) = role {
                    self.menu_role(event_loop, id, &role);
                }
                if self.index(id).is_some() {
                    self.after(event_loop, id);
                }
                return;
            }
            UserEvent::Menu(tag) => {
                let id = self.active;
                let Some(idx) = self.index(id) else { return };
                let role = match menu::role_of_tag(tag) {
                    Some(role) => self.wins[idx].page.menu_role(role),
                    None => self.wins[idx].page.menu_click(tag as u64),
                };
                self.wins[idx].window.request_redraw();
                if let Some(role) = role {
                    self.menu_role(event_loop, id, &role);
                }
                if self.index(id).is_some() {
                    self.after(event_loop, id);
                }
                return;
            }
        };
        let Some(idx) = self.wins.iter().position(|w| w.window.id() == event.window_id) else { return };
        let id = self.wins[idx].id;
        match event.window_event {
            accesskit_winit::WindowEvent::InitialTreeRequested => {
                let win = &mut self.wins[idx];
                win.access_stamp = 0;
                let scale = win.window.scale_factor();
                let page = &mut win.page;
                win.access.update_if_active(|| access::update(&page.a11y_tree(), scale));
            }
            accesskit_winit::WindowEvent::ActionRequested(request) => {
                if let Some((node, action)) = access::action(&request) {
                    if self.wins[idx].page.a11y_action(node, action) {
                        self.wins[idx].window.request_redraw();
                    }
                    self.after(event_loop, id);
                }
            }
            accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, wid: WindowId, event: WindowEvent) {
        if let Some(pi) = self.portals.iter().position(|p| p.window.id() == wid) {
            self.portal_event(event_loop, pi, event);
            return;
        }
        let Some(idx) = self.wins.iter().position(|w| w.window.id() == wid) else { return };
        let id = self.wins[idx].id;
        if matches!(event, WindowEvent::Focused(true)) {
            self.active = id;
        }
        {
            let win = &mut self.wins[idx];
            win.access.process_event(&win.window.clone(), &event);
        }
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
