//! The C API of the Nile engine.
//!
//! Create a view with [`nile_view_create_html`] or [`nile_view_create_file`], feed it input
//! events, call [`nile_view_tick`] regularly, and draw it into your own pixel buffer with
//! [`nile_view_render`]. Window-level requests (close, minimize, drag, title, cursor) come out
//! of the view for your windowing code to act on, and scripts can call functions you register
//! with [`nile_view_register_function`].
//!
//! Single-threaded: use a view from one thread. Pointers that Nile returns (`const char *`) stay
//! valid until the next call on the same view; `char *` results belong to the caller and must be
//! released with [`nile_string_free`]. No function unwinds a panic into C; a failed call returns
//! `false`, null or an error code instead.

use std::{
    ffi::{c_char, c_void, CStr, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
    ptr,
    time::Duration,
};

use nile_engine::{
    window::{self, Frame, InitialState, Material, Resizable, WindowConfig},
    dialogs::{Dialogs, FileFilter, MessageKind},
    Clipboard, ModifiersState, Page,
};
use nile_script::WindowCommand;
use nile_style::Cursor;

/// An open page: its document, styles, layout and script runtime.
pub struct NileView {
    page: Page,
    /// Backing storage for the `const char *` results handed out (title, ...).
    title: CString,
    config_title: CString,
}

/// Shift is held.
pub const NILE_MOD_SHIFT: u8 = 1;
/// Control is held.
pub const NILE_MOD_CONTROL: u8 = 2;
/// Alt/Option is held.
pub const NILE_MOD_ALT: u8 = 4;
/// Command (macOS) / Windows key is held.
pub const NILE_MOD_META: u8 = 8;

/// The mouse pointer shape the page wants over the current position.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NileCursor {
    Default = 0,
    Pointer = 1,
    Text = 2,
    Crosshair = 3,
    Move = 4,
    NotAllowed = 5,
    Wait = 6,
    Progress = 7,
    Help = 8,
    Grab = 9,
    Grabbing = 10,
    /// Horizontal resize (`col-resize`, `ew-resize`).
    ResizeH = 11,
    /// Vertical resize (`row-resize`, `ns-resize`).
    ResizeV = 12,
    /// `cursor: none`: hide the pointer.
    Hidden = 13,
    /// Diagonal resize, top left to bottom right.
    ResizeNwse = 14,
    /// Diagonal resize, top right to bottom left.
    ResizeNesw = 15,
}

/// What [`nile_view_tick`] found.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NileTick {
    /// Something changed; draw again.
    pub redraw: bool,
    /// Call `nile_view_tick` again in this many milliseconds (timers, animations, the caret
    /// blink), or -1 if nothing is scheduled and you can wait for input.
    pub next_ms: i32,
}

/// What a window request asks for.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NileWindowCommandKind {
    /// No request is pending.
    None = 0,
    Close = 1,
    Minimize = 2,
    Maximize = 3,
    /// Un-maximize, un-minimize and leave full screen.
    Restore = 4,
    ToggleMaximize = 5,
    /// `a` is 1 to enter full screen, 0 to leave it.
    Fullscreen = 6,
    /// Move the window's top-left to (`a`, `b`) in screen pixels.
    MoveTo = 7,
    /// Resize the window's content area to `a` x `b` logical pixels.
    ResizeTo = 8,
}

/// A request from the page (a script, or an element with `role="window-close"` etc.).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NileWindowCommand {
    pub kind: NileWindowCommandKind,
    pub a: f32,
    pub b: f32,
}

/// The window settings in the page's root `<html>` element (`window-frame`, `window-width`, ...).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NileWindowConfig {
    /// The title (`window-title` or `<title>`); valid until the next call on the view.
    pub title: *const c_char,
    /// Content size in logical pixels, resolved against the desktop size you passed.
    pub width: f32,
    pub height: f32,
    /// Limits in logical pixels; 0 means none.
    pub min_width: f32,
    pub min_height: f32,
    pub max_width: f32,
    pub max_height: f32,
    pub resizable: bool,
    /// For frameless windows: how far inside the edge the resize handles reach.
    pub resize_margin: f32,
    pub minimizable: bool,
    pub maximizable: bool,
    /// 0 shown, 1 minimized, 2 maximized, 3 full screen, 4 hidden.
    pub state: i32,
    /// 0 none, else the keypad position 1-9 (5 centres) the page asked for.
    pub alignment: i32,
    /// Top-left for that alignment, when `alignment` is not 0.
    pub x: f32,
    pub y: f32,
    /// 0 system frame, 1 extended (content under the title bar), 2 solid (no frame),
    /// 3 transparent (no frame, see-through where the page has no background), 4 solid with a shadow.
    pub frame: i32,
    /// The system draws no frame, so you should honour [`nile_view_caption_press`] and resize edges.
    pub frameless: bool,
    /// The window's pixels carry alpha (`window-frame="transparent"` or a `window-material`): make the
    /// window transparent and present the premultiplied BGRA from [`nile_view_render`] with alpha.
    pub translucent: bool,
    /// The system draws a drop shadow around a frameless window (`solid-with-shadow`).
    pub shadow: bool,
    /// `window-material`: 0 none (or unset), 1 blur, 2 acrylic, 3 mica, 4 tabbed, 5 sidebar, 6 hud. Scripts can
    /// change it (`Window.this.material`): read the config again after ticks and apply a new value.
    pub material: i32,
}

/// A function the host provides for scripts: it receives the call's arguments as a JSON array
/// and returns a JSON value as a NUL-terminated string (copied immediately), or NULL for
/// `undefined`. Scripts call it as `host.name(...)`.
pub type NileHostFn = Option<unsafe extern "C" fn(args_json: *const c_char, user: *mut c_void) -> *const c_char>;

/// Clipboard access. `get` returns the current text (copied immediately) or NULL; `set` stores text.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NileClipboard {
    pub get: Option<unsafe extern "C" fn(user: *mut c_void) -> *const c_char>,
    pub set: Option<unsafe extern "C" fn(text: *const c_char, user: *mut c_void)>,
    pub user: *mut c_void,
    /// Optional: the clipboard's HTML flavour (copied immediately) or NULL; rich paste into editors.
    pub get_html: Option<unsafe extern "C" fn(user: *mut c_void) -> *const c_char>,
    /// Optional: put `html` on the clipboard with `text` as its plain-text form.
    pub set_html: Option<unsafe extern "C" fn(html: *const c_char, text: *const c_char, user: *mut c_void)>,
}

struct CClipboard(NileClipboard);

impl Clipboard for CClipboard {
    fn text(&mut self) -> Option<String> {
        let get = self.0.get?;
        let p = unsafe { get(self.0.user) };
        (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
    fn set_text(&mut self, text: &str) {
        if let (Some(set), Ok(c)) = (self.0.set, CString::new(text)) {
            unsafe { set(c.as_ptr(), self.0.user) }
        }
    }
    fn html(&mut self) -> Option<String> {
        let p = unsafe { self.0.get_html?(self.0.user) };
        (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
    fn set_html(&mut self, html: &str, text: &str) {
        match (self.0.set_html, CString::new(html), CString::new(text)) {
            (Some(set), Ok(h), Ok(t)) => unsafe { set(h.as_ptr(), t.as_ptr(), self.0.user) },
            _ => self.set_text(text),
        }
    }
}

/// Native dialogs for scripts (`Window.this.selectFile`, `selectFolder`, `modal(<alert>..)`) and
/// `<input type=file>`. Every callback blocks until the user answers; strings you return are copied
/// immediately. Leave a callback NULL to have that dialog unavailable (scripts get `null`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NileDialogs {
    /// Open dialog. `filters` is Sciter's `label|*.ext;*.ext|label|*.*` text (may be empty), `dir`
    /// where to start. Return the chosen paths separated by newlines, or NULL when cancelled.
    pub open_files: Option<unsafe extern "C" fn(multiple: bool, filters: *const c_char, caption: *const c_char, dir: *const c_char, user: *mut c_void) -> *const c_char>,
    /// Save dialog; `extension` is the default one. Return the path or NULL.
    pub save_file: Option<unsafe extern "C" fn(filters: *const c_char, extension: *const c_char, caption: *const c_char, path: *const c_char, user: *mut c_void) -> *const c_char>,
    /// Folder chooser. Return the path or NULL.
    pub pick_folder: Option<unsafe extern "C" fn(caption: *const c_char, dir: *const c_char, user: *mut c_void) -> *const c_char>,
    /// Message box; `kind` 0 info, 1 alert, 2 error, 3 question (the only one with a second button).
    /// Return true for OK / Yes.
    pub message: Option<unsafe extern "C" fn(kind: i32, caption: *const c_char, text: *const c_char, user: *mut c_void) -> bool>,
    pub user: *mut c_void,
}

struct CDialogs(NileDialogs);

fn filters_text(filters: &[FileFilter]) -> CString {
    let parts: Vec<String> = filters
        .iter()
        .map(|f| {
            let exts = if f.extensions.is_empty() { "*.*".to_string() } else { f.extensions.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";") };
            format!("{}|{}", f.label, exts)
        })
        .collect();
    cstring(&parts.join("|"))
}

fn answer(p: *const c_char) -> Option<String> {
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()).filter(|s| !s.is_empty())
}

impl Dialogs for CDialogs {
    fn open_files(&self, multiple: bool, filters: &[FileFilter], caption: &str, dir: &str) -> Vec<String> {
        let Some(f) = self.0.open_files else { return Vec::new() };
        let (flt, cap, dir) = (filters_text(filters), cstring(caption), cstring(dir));
        let p = unsafe { f(multiple, flt.as_ptr(), cap.as_ptr(), dir.as_ptr(), self.0.user) };
        answer(p).map(|s| s.lines().filter(|l| !l.is_empty()).map(str::to_string).collect()).unwrap_or_default()
    }
    fn save_file(&self, filters: &[FileFilter], extension: &str, caption: &str, path: &str) -> Option<String> {
        let f = self.0.save_file?;
        let (flt, ext, cap, path) = (filters_text(filters), cstring(extension), cstring(caption), cstring(path));
        answer(unsafe { f(flt.as_ptr(), ext.as_ptr(), cap.as_ptr(), path.as_ptr(), self.0.user) })
    }
    fn pick_folder(&self, caption: &str, dir: &str) -> Option<String> {
        let f = self.0.pick_folder?;
        let (cap, dir) = (cstring(caption), cstring(dir));
        answer(unsafe { f(cap.as_ptr(), dir.as_ptr(), self.0.user) })
    }
    fn message(&self, kind: MessageKind, caption: &str, text: &str) -> bool {
        let Some(f) = self.0.message else { return true };
        let kind = match kind {
            MessageKind::Info => 0,
            MessageKind::Alert => 1,
            MessageKind::Error => 2,
            MessageKind::Question => 3,
        };
        let (cap, text) = (cstring(caption), cstring(text));
        unsafe { f(kind, cap.as_ptr(), text.as_ptr(), self.0.user) }
    }
}

// ---------------------------------------------------------------------------------------------

fn text<'a>(p: *const c_char) -> Option<&'a str> {
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_str().ok()).flatten()
}

fn cstring(s: &str) -> CString {
    // Interior NULs can't be represented in a C string; drop them.
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

fn into_c(s: String) -> *mut c_char {
    cstring(&s).into_raw()
}

fn view<'a>(v: *mut NileView) -> Option<&'a mut NileView> {
    unsafe { v.as_mut() }
}

fn mods(bits: u8) -> ModifiersState {
    ModifiersState::from_bits(bits)
}

/// Run `f`, turning a panic into `fallback` so it never crosses into C.
fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// What a host's own first window knows about itself: an id of its own, no opener.
fn first_window() -> nile_script::WindowInfo {
    nile_script::WindowInfo { id: nile_script::next_window_id(), ..Default::default() }
}

fn new_view(page: Page) -> *mut NileView {
    Box::into_raw(Box::new(NileView { page, title: CString::default(), config_title: CString::default() }))
}

// ---- lifecycle ------------------------------------------------------------------------------

/// The engine's version, e.g. "0.1.0".
#[no_mangle]
pub extern "C" fn nile_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// Create a view from HTML text. Relative paths (`<img src>`, `<script src>`, `url()`) resolve
/// against `base_dir` (NULL for the current directory). `width` x `height` is the viewport in
/// logical pixels. Returns NULL on bad arguments. Free it with [`nile_view_destroy`].
///
/// # Safety
/// `html` and `base_dir` must be NULL or valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn nile_view_create_html(html: *const c_char, base_dir: *const c_char, width: f32, height: f32) -> *mut NileView {
    guard(ptr::null_mut(), || {
        let Some(html) = text(html) else { return ptr::null_mut() };
        let base = text(base_dir).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        new_view(Page::html_window(html, base, (width.max(1.0), height.max(1.0)), first_window()))
    })
}

/// Create a view from an HTML file. Relative paths resolve against the file's folder. A missing
/// file gives a page that says so. Returns NULL on bad arguments.
///
/// # Safety
/// `path` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nile_view_create_file(path: *const c_char, width: f32, height: f32) -> *mut NileView {
    guard(ptr::null_mut(), || {
        let Some(path) = text(path) else { return ptr::null_mut() };
        new_view(Page::load_window(PathBuf::from(path), (width.max(1.0), height.max(1.0)), first_window()))
    })
}

/// Destroy a view. Passing NULL is fine.
///
/// # Safety
/// `view` must come from a `nile_view_create_*` call and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn nile_view_destroy(view: *mut NileView) {
    if !view.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(view) })));
    }
}

/// Reload the page from its file or HTML text, keeping the size, scroll position and clipboard.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_reload(view: *mut NileView) {
    if let Some(v) = view_ref(view) {
        guard((), || v.page.reload());
    }
}

fn view_ref<'a>(v: *mut NileView) -> Option<&'a mut NileView> {
    view(v)
}

/// Free a string returned as `char *` by this library. Passing NULL is fine.
///
/// # Safety
/// `s` must be a pointer this library returned, not freed before.
#[no_mangle]
pub unsafe extern "C" fn nile_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

// ---- size and painting ----------------------------------------------------------------------

/// Set the viewport size in logical pixels (call when your window resizes).
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_resize(view: *mut NileView, width: f32, height: f32) {
    if let Some(v) = view_ref(view) {
        guard((), || v.page.layout(width.max(1.0), height.max(1.0)));
    }
}

/// The page's full height in logical pixels (for your own scroll bar), or 0 on error.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_content_height(view: *mut NileView) -> f32 {
    match view_ref(view) {
        Some(v) => guard(0.0, || v.page.view.borrow_mut().layout().content_height),
        None => 0.0,
    }
}

/// Paint the page into `pixels`: `width` x `height` physical pixels, 4 bytes each in BGRA order
/// with premultiplied alpha (the usual native bitmap layout on little-endian machines), rows
/// packed with no padding. `scale` is the device pixel ratio (the logical viewport is
/// `width / scale` by `height / scale`). `len` is the buffer size in bytes and must be at least
/// `width * height * 4`. Returns false if the arguments are wrong.
///
/// # Safety
/// `pixels` must point to `len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn nile_view_render(view: *mut NileView, pixels: *mut u8, len: usize, width: u32, height: u32, scale: f32) -> bool {
    let Some(v) = view_ref(view) else { return false };
    let need = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    if pixels.is_null() || width == 0 || height == 0 || len < need || scale <= 0.0 {
        return false;
    }
    let buf = unsafe { std::slice::from_raw_parts_mut(pixels, need) };
    guard(false, || {
        v.page.paint(buf, width, height, scale);
        true
    })
}

// ---- input ----------------------------------------------------------------------------------

/// The pointer moved to (`x`, `y`) in logical pixels from the view's top-left. Returns true if
/// the page changed (hover styles, selection drag, ...) and needs drawing.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_pointer_move(view: *mut NileView, x: f32, y: f32) -> bool {
    view_ref(view).is_some_and(|v| guard(false, || v.page.pointer_moved(x, y)))
}

/// The pointer left the view.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_pointer_leave(view: *mut NileView) -> bool {
    view_ref(view).is_some_and(|v| guard(false, || v.page.pointer_left()))
}

/// A mouse button went down (`pressed` true) or up at the last pointer position. Only the
/// primary button (0) is used. `modifiers` is a mix of the `NILE_MOD_*` bits (Shift-click
/// extends selections, Cmd/Ctrl-click toggles list items). Returns true if the page changed.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_pointer_button(view: *mut NileView, button: i32, pressed: bool, modifiers: u8) -> bool {
    if button != 0 {
        return false;
    }
    view_ref(view).is_some_and(|v| {
        guard(false, || {
            v.page.mods = mods(modifiers);
            if pressed { v.page.press() } else { v.page.release() }
        })
    })
}

/// Scroll by a wheel or trackpad delta in logical pixels at the pointer. Positive `dy` scrolls
/// the content down the screen (wheel up). Always returns true.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_wheel(view: *mut NileView, dx: f32, dy: f32) -> bool {
    view_ref(view).is_some_and(|v| guard(false, || {
        v.page.wheel(dx, dy);
        true
    }))
}

/// A key went down or up. `key` is the DOM `KeyboardEvent.key` value: "a", "Enter", "ArrowLeft",
/// "Backspace", " ", ... `modifiers` is a mix of the `NILE_MOD_*` bits. A key-down that scripts
/// don't cancel also does the default thing (typing, caret movement, Tab focus, shortcuts such as
/// Cmd/Ctrl+C). Returns true if the page changed.
///
/// # Safety
/// `view` must be a live view and `key` a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_key(view: *mut NileView, key: *const c_char, modifiers: u8, pressed: bool) -> bool {
    let (Some(v), Some(key)) = (view_ref(view), text(key)) else { return false };
    guard(false, || {
        let m = mods(modifiers);
        v.page.mods = m;
        if pressed {
            let (proceed, mut changed) = v.page.key("keydown", key.to_string(), m);
            if proceed {
                changed |= v.page.default_key(key, m);
            }
            changed
        } else {
            v.page.key("keyup", key.to_string(), m).1
        }
    })
}

/// Type text (for example what an input method committed) into the focused input, one character
/// at a time. Returns true if the page changed.
///
/// # Safety
/// `view` must be a live view and `utf8` a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_text_input(view: *mut NileView, utf8: *const c_char) -> bool {
    let (Some(v), Some(s)) = (view_ref(view), text(utf8)) else { return false };
    guard(false, || {
        let mut changed = false;
        for c in s.chars() {
            let key = c.to_string();
            let (proceed, c1) = v.page.key("keydown", key.clone(), ModifiersState::default());
            changed |= c1;
            if proceed {
                changed |= v.page.default_key(&key, ModifiersState::default());
            }
            changed |= v.page.key("keyup", key, ModifiersState::default()).1;
        }
        changed
    })
}

/// Run due timers, animations and the caret blink. Call it when the time from the last
/// result's `next_ms` has passed, and after any call that might start something.
///
/// # Safety
/// `view` must be a live view; `out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn nile_view_tick(view: *mut NileView, out: *mut NileTick) {
    let result = match view_ref(view) {
        Some(v) => guard(NileTick { redraw: false, next_ms: -1 }, || {
            let (redraw, next) = v.page.tick();
            NileTick { redraw, next_ms: next.map_or(-1, |d: Duration| d.as_millis().min(i32::MAX as u128) as i32) }
        }),
        None => NileTick { redraw: false, next_ms: -1 },
    };
    if let Some(out) = unsafe { out.as_mut() } {
        *out = result;
    }
}

/// The pointer shape to show at the last pointer position.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_cursor(view: *mut NileView) -> NileCursor {
    let Some(v) = view_ref(view) else { return NileCursor::Default };
    guard(NileCursor::Default, || match v.page.cursor() {
        Cursor::Auto | Cursor::Default => NileCursor::Default,
        Cursor::Pointer => NileCursor::Pointer,
        Cursor::Text => NileCursor::Text,
        Cursor::Crosshair => NileCursor::Crosshair,
        Cursor::Move => NileCursor::Move,
        Cursor::NotAllowed => NileCursor::NotAllowed,
        Cursor::Wait => NileCursor::Wait,
        Cursor::Progress => NileCursor::Progress,
        Cursor::Help => NileCursor::Help,
        Cursor::Grab => NileCursor::Grab,
        Cursor::Grabbing => NileCursor::Grabbing,
        Cursor::ResizeH => NileCursor::ResizeH,
        Cursor::ResizeV => NileCursor::ResizeV,
        Cursor::ResizeNwse => NileCursor::ResizeNwse,
        Cursor::ResizeNesw => NileCursor::ResizeNesw,
        Cursor::None => NileCursor::Hidden,
    })
}

/// The text selected on the page or in the focused input (for a Copy menu item), as a new string
/// to free with [`nile_string_free`]; empty if nothing is selected.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_selected_text(view: *mut NileView) -> *mut c_char {
    let Some(v) = view_ref(view) else { return ptr::null_mut() };
    guard(ptr::null_mut(), || into_c(v.page.selected_text()))
}

/// Install clipboard callbacks (the default is a private in-memory clipboard).
///
/// # Safety
/// `view` must be a live view; the callbacks and `user` must stay valid for its lifetime.
#[no_mangle]
pub unsafe extern "C" fn nile_view_set_clipboard(view: *mut NileView, clipboard: NileClipboard) {
    if let Some(v) = view_ref(view) {
        v.page.clipboard = Box::new(CClipboard(clipboard));
    }
}

/// Install native dialogs for scripts and `<input type=file>` (without them scripts get `null`
/// and file inputs use [`nile_view_take_file_request`]). Kept across reloads.
///
/// # Safety
/// `view` must be a live view; the callbacks and `user` must stay valid for its lifetime.
#[no_mangle]
pub unsafe extern "C" fn nile_view_set_dialogs(view: *mut NileView, dialogs: NileDialogs) {
    if let Some(v) = view_ref(view) {
        v.page.set_dialogs(std::rc::Rc::new(CDialogs(dialogs)));
    }
}

/// Tell Nile the system's colour theme: `dark` for `prefers-color-scheme: dark` (and the dark
/// look of pages that say `color-scheme: light dark`), `accent_rgb` the system accent colour as
/// `0xRRGGBB` (negative = the platform's stock accent). It is a per-thread setting shared by every
/// view of the thread; call it before creating views and again when the OS changes. Restyles this
/// view (call it on the others too) and returns true if the theme changed.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_set_system_theme(view: *mut NileView, dark: bool, accent_rgb: i32) -> bool {
    let Some(v) = view_ref(view) else { return false };
    guard(false, || {
        let accent = if accent_rgb < 0 { nile_style::default_accent() } else { nile_style::Color::rgb((accent_rgb >> 16) as u8, (accent_rgb >> 8) as u8, accent_rgb as u8) };
        let changed = nile_style::set_system_theme(nile_style::SystemTheme { dark, accent });
        if changed {
            v.page.system_theme_changed();
        }
        changed
    })
}

/// Files were dropped on the window at the pointer's last position (tell the page where with
/// [`nile_pointer_move`] first). `paths` are UTF-8 paths separated by newlines. A file input under
/// the pointer takes them; elsewhere the target gets a `drop` event. Returns true if the page changed.
///
/// # Safety
/// `view` must be a live view and `paths` a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_view_drop_files(view: *mut NileView, paths: *const c_char) -> bool {
    let (Some(v), Some(paths)) = (view_ref(view), text(paths)) else { return false };
    guard(false, || {
        let list: Vec<String> = paths.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
        v.page.drop_files(&list)
    })
}

/// The window became active (`active` true) or inactive: fires `activate` on `Window.this`.
/// Returns true if the page changed.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_window_activated(view: *mut NileView, active: bool) -> bool {
    view_ref(view).is_some_and(|v| guard(false, || v.page.window_activated(active)))
}

/// Popups that want an OS window of their own (`popup({window: "popup"})`), as JSON
/// `[{"id":N,"x":..,"y":..,"w":..,"h":..}]` with the border box in this view's logical coordinates (it
/// may lie outside the view). Free the string with [`nile_string_free`]. For each: make a frameless,
/// always-on-top window at that rectangle, draw it with [`nile_view_render_portal`], and send its
/// pointer, wheel and key input to this same view with `x`/`y` offset by the rectangle's origin.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_portals(view: *mut NileView) -> *mut c_char {
    let Some(v) = view_ref(view) else { return ptr::null_mut() };
    guard(ptr::null_mut(), || {
        v.page.sync();
        let items: Vec<String> = v
            .page
            .view
            .borrow()
            .portals
            .iter()
            .map(|p| format!("{{\"id\":{},\"x\":{},\"y\":{},\"w\":{},\"h\":{}}}", p.node.0, p.rect.0, p.rect.1, p.rect.2, p.rect.3))
            .collect();
        into_c(format!("[{}]", items.join(",")))
    })
}

/// Draw one popup from [`nile_view_portals`] into its window's buffer (same format as
/// [`nile_view_render`]): `width` x `height` physical pixels at `scale`, the popup's top-left
/// corner at the buffer's. Returns false if `id` isn't a current popup or the arguments are wrong.
///
/// # Safety
/// `pixels` must point to `len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn nile_view_render_portal(view: *mut NileView, id: usize, pixels: *mut u8, len: usize, width: u32, height: u32, scale: f32) -> bool {
    let Some(v) = view_ref(view) else { return false };
    let need = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    if pixels.is_null() || width == 0 || height == 0 || len < need || scale <= 0.0 {
        return false;
    }
    let buf = unsafe { std::slice::from_raw_parts_mut(pixels, need) };
    guard(false, || {
        let rect = v.page.view.borrow().portals.iter().find(|p| p.node.0 == id).map(|p| (p.node, p.rect));
        let Some((node, rect)) = rect else { return false };
        v.page.paint_portal(buf, width, height, scale, node, (rect.0, rect.1));
        true
    })
}

// ---- accessibility ----------------------------------------------------------------------------

/// What an assistive-technology request asks an element to do (`nile_view_accessibility_action`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NileA11yAction {
    Click = 0,
    Focus = 1,
    /// Replace the text of an input with `text`.
    SetValue = 2,
    Increment = 3,
    Decrement = 4,
    ScrollIntoView = 5,
}

/// The page's accessibility tree as JSON (`{"root", "focus", "nodes": [{"id", "role", "name",
/// "bounds": [x, y, w, h], "children": [ids], ...}]}`, window coordinates in logical pixels), as a
/// new string to free with [`nile_string_free`]. Build the platform's accessibility objects from it
/// (UI Automation, NSAccessibility, AT-SPI) and call it again after the page changes.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_accessibility_tree(view: *mut NileView) -> *mut c_char {
    let Some(v) = view_ref(view) else { return ptr::null_mut() };
    guard(ptr::null_mut(), || into_c(v.page.a11y_tree().to_json()))
}

/// Carry out a request from assistive technology on node `id` of the tree last returned. `value` is
/// used by `SetValue` (NULL otherwise). Returns true if the page did something (draw again).
///
/// # Safety
/// `view` must be a live view; `value` NULL or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn nile_view_accessibility_action(view: *mut NileView, id: u64, action: NileA11yAction, value: *const c_char) -> bool {
    use nile_engine::a11y::A11yAction as A;
    let Some(v) = view_ref(view) else { return false };
    let action = match action {
        NileA11yAction::Click => A::Click,
        NileA11yAction::Focus => A::Focus,
        NileA11yAction::SetValue => A::SetValue(text(value).unwrap_or_default().to_string()),
        NileA11yAction::Increment => A::Increment,
        NileA11yAction::Decrement => A::Decrement,
        NileA11yAction::ScrollIntoView => A::ScrollIntoView,
    };
    guard(false, || v.page.a11y_action(id, action))
}

// ---- application menu -------------------------------------------------------------------------

/// The page's application menu (`Window.this.menu = element`) as JSON, or NULL when it has none:
/// `[{"id", "label", "accel"?, "enabled", "checked", "role"?, "items"?}, {"separator": true}, ...]`.
/// Build the platform's menu bar from it and call [`nile_view_menu_click`] with an item's `id` when it
/// is chosen. Call again after the page changes (`checked` and `enabled` follow its attributes).
/// Free the string with [`nile_string_free`].
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_menu(view: *mut NileView) -> *mut c_char {
    let Some(v) = view_ref(view) else { return ptr::null_mut() };
    guard(ptr::null_mut(), || match v.page.app_menu() {
        Some(items) => into_c(nile_engine::menus::menu_json(&items)),
        None => ptr::null_mut(),
    })
}

/// A menu item was chosen: fires its `click` (or does a clipboard/editing role). Returns NULL, or for a
/// window role the host must carry out (`quit`, `close`, `minimize`, `zoom`, `about`, ...) a string
/// to free with [`nile_string_free`].
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_menu_click(view: *mut NileView, id: u64) -> *mut c_char {
    let Some(v) = view_ref(view) else { return ptr::null_mut() };
    guard(ptr::null_mut(), || v.page.menu_click(id).map_or(ptr::null_mut(), into_c))
}

/// Run the menu item whose shortcut is `key` (a DOM key such as `"s"` or `"F5"`) with the same modifier
/// bits as `nile_view_key`, for hosts whose menu bar doesn't handle shortcuts. Returns whether one ran.
///
/// # Safety
/// `view` must be a live view and `key` a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_view_menu_accelerator(view: *mut NileView, key: *const c_char, modifiers: u8) -> bool {
    let (Some(v), Some(key)) = (view_ref(view), text(key)) else { return false };
    guard(false, || v.page.menu_accelerator(key, mods(modifiers)))
}

// ---- paged output -----------------------------------------------------------------------------

/// Cut the document into pages (`@page` size and margins, `break-*`) and write them to a PDF at
/// `path`. Returns the page count, or -1 on failure. The view's own layout is redone on the next draw.
///
/// # Safety
/// `view` must be a live view and `path` a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_view_write_pdf(view: *mut NileView, path: *const c_char) -> i32 {
    let (Some(v), Some(path)) = (view_ref(view), text(path)) else { return -1 };
    guard(-1, || {
        let mut bytes = Vec::new();
        let title = v.page.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let pages = v.page.write_pdf(&mut bytes, &title);
        match std::fs::write(path, bytes) {
            Ok(()) => pages as i32,
            Err(_) => -1,
        }
    })
}

// ---- window ---------------------------------------------------------------------------------

/// The page's current title (`document.title`); valid until the next call on the view.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_title(view: *mut NileView) -> *const c_char {
    let Some(v) = view_ref(view) else { return ptr::null() };
    guard(ptr::null(), || {
        let doc = v.page.doc.borrow();
        v.title = cstring(&window::current_title(&doc, &WindowConfig::from_document(&doc)));
        v.title.as_ptr()
    })
}

/// Read the window settings from the page's root element. `desktop_width`/`desktop_height` (the
/// screen's logical size) resolve percentage sizes and alignment. Returns false on error.
///
/// # Safety
/// `view` must be a live view and `out` writable.
#[no_mangle]
pub unsafe extern "C" fn nile_view_window_config(view: *mut NileView, desktop_width: f32, desktop_height: f32, out: *mut NileWindowConfig) -> bool {
    let (Some(v), Some(out)) = (view_ref(view), unsafe { out.as_mut() }) else { return false };
    guard(false, || {
        let c = WindowConfig::from_document(&v.page.doc.borrow());
        let desktop = (desktop_width, desktop_height);
        let (width, height) = c.inner_size(desktop);
        let pos = c.position((width, height), desktop);
        v.config_title = cstring(&c.title);
        *out = NileWindowConfig {
            title: v.config_title.as_ptr(),
            width,
            height,
            min_width: c.min_width.unwrap_or(0.0),
            min_height: c.min_height.unwrap_or(0.0),
            max_width: c.max_width.unwrap_or(0.0),
            max_height: c.max_height.unwrap_or(0.0),
            resizable: c.resizable != Resizable::No,
            resize_margin: match c.resizable {
                Resizable::Margin(m) => m,
                _ => 6.0,
            },
            minimizable: c.minimizable,
            maximizable: c.maximizable,
            state: match c.state {
                InitialState::Shown => 0,
                InitialState::Minimized => 1,
                InitialState::Maximized => 2,
                InitialState::Fullscreen => 3,
                InitialState::Hidden => 4,
            },
            alignment: c.alignment.map_or(0, i32::from),
            x: pos.map_or(0.0, |p| p.0),
            y: pos.map_or(0.0, |p| p.1),
            frame: match c.frame {
                Frame::Native => 0,
                Frame::Extended => 1,
                Frame::Solid => 2,
                Frame::Transparent => 3,
                Frame::SolidShadow => 4,
            },
            frameless: c.frameless(),
            translucent: c.translucent(),
            shadow: c.shadow(),
            material: match c.material {
                Material::Auto | Material::None => 0,
                Material::Blur => 1,
                Material::Acrylic => 2,
                Material::Mica => 3,
                Material::Tabbed => 4,
                Material::Sidebar => 5,
                Material::Hud => 6,
            },
        };
        true
    })
}

/// Take the next pending window request (close, minimize, move, ...). Returns false, with `out`
/// set to `NILE_WINDOW_COMMAND_KIND_NONE`, when there are none; call it in a loop after every
/// input call and tick.
///
/// # Safety
/// `view` must be a live view; `out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn nile_view_take_window_command(view: *mut NileView, out: *mut NileWindowCommand) -> bool {
    let none = NileWindowCommand { kind: NileWindowCommandKind::None, a: 0.0, b: 0.0 };
    // Requests about other windows stay queued for `nile_view_take_window_request`.
    let cmd = view_ref(view).and_then(|v| {
        let mut view = v.page.view.borrow_mut();
        let at = view.window_requests.iter().position(|c| !is_request(c))?;
        match view.window_requests.remove(at) {
            WindowCommand::CloseWith(_) => Some(WindowCommand::Close),
            other => Some(other),
        }
    });
    let (found, result) = match cmd {
        None => (false, none),
        Some(c) => {
            use NileWindowCommandKind as K;
            let (kind, a, b) = match c {
                WindowCommand::Close => (K::Close, 0.0, 0.0),
                WindowCommand::Minimize => (K::Minimize, 0.0, 0.0),
                WindowCommand::Maximize => (K::Maximize, 0.0, 0.0),
                WindowCommand::Restore => (K::Restore, 0.0, 0.0),
                WindowCommand::ToggleMaximize => (K::ToggleMaximize, 0.0, 0.0),
                WindowCommand::Fullscreen(on) => (K::Fullscreen, f32::from(on), 0.0),
                WindowCommand::MoveTo(x, y) => (K::MoveTo, x, y),
                WindowCommand::ResizeTo(w, h) => (K::ResizeTo, w, h),
                // Filtered out above.
                WindowCommand::CloseWith(_) | WindowCommand::Open(_) | WindowCommand::Post { .. } | WindowCommand::CloseId(_) | WindowCommand::FocusId(_) | WindowCommand::Quit(_) => (K::None, 0.0, 0.0),
            };
            (true, NileWindowCommand { kind, a, b })
        }
    };
    if let Some(out) = unsafe { out.as_mut() } {
        *out = result;
    }
    found
}

/// Whether a command concerns the program or other windows (answered by `nile_view_take_window_request`)
/// rather than this window itself.
fn is_request(c: &WindowCommand) -> bool {
    matches!(c, WindowCommand::Open(_) | WindowCommand::Post { .. } | WindowCommand::CloseId(_) | WindowCommand::FocusId(_) | WindowCommand::Quit(_))
}

/// The id this view's window has (what other windows address it by, and `Window.this.id`).
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_window_id(view: *mut NileView) -> u32 {
    view_ref(view).map_or(0, |v| v.page.view.borrow().window_info.id)
}

/// The next thing this window's scripts asked of the host that involves other windows or the
/// program, as JSON, or NULL when there is none. Free it with [`nile_string_free`]. Kinds:
///
/// - `{"kind":"open","id":3,"opener":1,"modal":false,"fields":{"url":"a.html","width":"400",...}}`:
///   `new Window({...})` / `modal({...})`. Create a window and call [`nile_view_create_window`] with
///   the `id`, `opener`, `modal` and the `url`, `html`, `width`, `height` and `parameters` fields; the
///   other fields (`type`, `caption`, `x`, `y`, `alignment`, `state`, ...) say how to show it.
/// - `{"kind":"post","from":1,"to":3,"json":{...}}`: deliver with [`nile_view_window_message`] to
///   the view whose [`nile_view_window_id`] is `to`.
/// - `{"kind":"close","id":3,"value":null}`: close that window (ask [`nile_view_close_request`] first;
///   tell its opener with [`nile_view_window_closed`] afterwards).
/// - `{"kind":"focus","id":3}`: bring it to the front.
/// - `{"kind":"quit","code":0}`: `application.quit()`.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_take_window_request(view: *mut NileView) -> *mut c_char {
    let Some(v) = view_ref(view) else { return ptr::null_mut() };
    guard(ptr::null_mut(), || {
        let id = v.page.view.borrow().window_info.id;
        let q = nile_engine::editing::json_string;
        let cmd = {
            let mut view = v.page.view.borrow_mut();
            let at = view.window_requests.iter().position(is_request);
            at.map(|i| view.window_requests.remove(i))
        };
        match cmd {
            Some(WindowCommand::Open(spec)) => {
                let fields: Vec<String> = spec.fields.iter().map(|(k, v)| format!("{}:{}", q(k), q(v))).collect();
                into_c(format!("{{\"kind\":\"open\",\"id\":{},\"opener\":{},\"modal\":{},\"fields\":{{{}}}}}", spec.id, spec.opener, spec.modal, fields.join(",")))
            }
            Some(WindowCommand::Post { to, json }) => into_c(format!("{{\"kind\":\"post\",\"from\":{id},\"to\":{},\"json\":{json}}}", if to == 0 { id } else { to })),
            Some(WindowCommand::CloseId(to)) => into_c(format!("{{\"kind\":\"close\",\"id\":{},\"value\":null}}", if to == 0 { id } else { to })),
            Some(WindowCommand::FocusId(to)) => into_c(format!("{{\"kind\":\"focus\",\"id\":{}}}", if to == 0 { id } else { to })),
            Some(WindowCommand::Quit(code)) => into_c(format!("{{\"kind\":\"quit\",\"code\":{code}}}")),
            _ => ptr::null_mut(),
        }
    })
}

/// Create the view for a window the page asked to open (an `"open"` request from
/// [`nile_view_take_window_request`]). `url` is a file or `this://` URL resolved against `base_dir`
/// (normally the opener's folder), or NULL when `html` has the markup; both NULL gives an empty page.
/// `parameters_json` is the `parameters` field (NULL for none). Returns NULL on bad arguments.
///
/// # Safety
/// The string arguments must be NULL or valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn nile_view_create_window(url: *const c_char, html: *const c_char, base_dir: *const c_char, width: f32, height: f32, id: u32, opener: u32, modal: bool, parameters_json: *const c_char) -> *mut NileView {
    guard(ptr::null_mut(), || {
        let dir = text(base_dir).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let info = nile_script::WindowInfo { id, opener: (opener != 0).then_some(opener), modal, parameters: text(parameters_json).unwrap_or_default().to_string() };
        let size = (width.max(1.0), height.max(1.0));
        let page = match (text(html), text(url)) {
            (Some(html), _) => Page::html_window(html, dir, size, info),
            (None, Some(url)) => {
                let mapped = nile_dom::vfs::url_to_path(url).map(|p| p.to_string_lossy().into_owned());
                let rel = mapped.as_deref().unwrap_or_else(|| nile_script::path_of_url(url));
                let file = if std::path::Path::new(rel).is_absolute() { PathBuf::from(rel) } else { dir.join(rel) };
                Page::load_window(file, size, info)
            }
            (None, None) => Page::html_window("<body></body>", dir, size, info),
        };
        new_view(page)
    })
}

/// A message another window posted (`Window.post`): fires `message` with `data` (`json_data`) and
/// the sender's id as `source`. Returns whether the page changed.
///
/// # Safety
/// `view` must be a live view; `json_data` a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_view_window_message(view: *mut NileView, from: u32, json_data: *const c_char) -> bool {
    let (Some(v), Some(json)) = (view_ref(view), text(json_data)) else { return false };
    guard(false, || v.page.window_message(from, json))
}

/// A window this one opened has closed; resolves its `Window.closed` / `modal()` promise with
/// `value_json` (JSON text, `"null"` for none). Returns whether the page changed.
///
/// # Safety
/// `view` must be a live view; `value_json` a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_view_window_closed(view: *mut NileView, id: u32, value_json: *const c_char) -> bool {
    let Some(v) = view_ref(view) else { return false };
    let value = text(value_json).unwrap_or("null");
    guard(false, || v.page.window_closed(id, value))
}

/// The user (or a script) wants this window closed: fires `closerequest`. Returns whether it may close
/// (false if a listener called `preventDefault()`). `reason` is 0 for the title bar's button, 1 for a script.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_close_request(view: *mut NileView, reason: u32) -> bool {
    let Some(v) = view_ref(view) else { return true };
    guard(true, || v.page.close_request(reason))
}

/// Tell scripts where the window is (`window.screenX`, `outerWidth`, ...): top-left in screen
/// pixels, outer size, and whether it is full screen.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_set_window_rect(view: *mut NileView, x: f32, y: f32, width: f32, height: f32, fullscreen: bool) {
    if let Some(v) = view_ref(view) {
        v.page.view.borrow_mut().window_rect = Some([x, y, width, height, f32::from(fullscreen)]);
    }
}

/// For a frameless window: call after the pointer is pressed over the view. Returns true if the
/// press landed on a `role="window-caption"` element and you should start dragging the window
/// (a quick second press asks to toggle maximize instead, via the window request queue); in
/// that case don't pass the press on with [`nile_pointer_button`].
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_caption_press(view: *mut NileView) -> bool {
    view_ref(view).is_some_and(|v| guard(false, || v.page.caption_press()))
}

// ---- scripting ------------------------------------------------------------------------------

fn finish(result: Result<String, String>, out: *mut *mut c_char) -> i32 {
    let (code, s) = match result {
        Ok(s) => (0, s),
        Err(e) => (1, e),
    };
    if let Some(out) = unsafe { out.as_mut() } {
        *out = into_c(s);
    }
    code
}

/// Evaluate JavaScript in the page. On success returns 0 and stores the result in `*out`:
/// strings as they are, `undefined` as an empty string, anything else as JSON. On a script error
/// returns 1 and stores the message. `out` may be NULL. Free `*out` with [`nile_string_free`].
/// After it returns, call [`nile_view_tick`] or draw again, as scripts may have changed the page.
///
/// # Safety
/// `view` must be a live view, `code` a valid string and `out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn nile_view_eval(view: *mut NileView, code: *const c_char, out: *mut *mut c_char) -> i32 {
    let (Some(v), Some(code)) = (view_ref(view), text(code)) else { return 2 };
    guard(2, || finish(v.page.eval(code).0, out))
}

/// Call a JavaScript function by name (`"save"` or dotted, `"app.save"`) with arguments given as
/// a JSON array (`"[1, \"two\"]"`; NULL means no arguments). Returns 0 and the JSON result in
/// `*out`, or 1 and an error message. Free `*out` with [`nile_string_free`].
///
/// # Safety
/// `view` must be a live view, `name` a valid string, `args_json` NULL or valid, `out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn nile_view_call(view: *mut NileView, name: *const c_char, args_json: *const c_char, out: *mut *mut c_char) -> i32 {
    let (Some(v), Some(name)) = (view_ref(view), text(name)) else { return 2 };
    let args = text(args_json).unwrap_or("[]");
    guard(2, || finish(v.page.call(name, args).0, out))
}

/// Expose `callback` to scripts as `host.<name>(...)`. Registering a name again replaces it.
///
/// # Safety
/// `view` must be a live view and `name` a valid string; `callback` and `user` must stay valid
/// for the life of the view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_register_function(view: *mut NileView, name: *const c_char, callback: NileHostFn, user: *mut c_void) -> bool {
    let (Some(v), Some(name), Some(callback)) = (view_ref(view), text(name), callback) else { return false };
    let user = user as usize;
    guard(false, || {
        v.page.register_host_fn(name, move |args| {
            let Ok(args) = CString::new(args) else { return String::new() };
            let p = unsafe { callback(args.as_ptr(), user as *mut c_void) };
            if p.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
            }
        });
        true
    })
}

/// Keep the page's `localStorage` in the file at `path` (created if missing). Pages created from
/// a file already persist into the user's data directory; set `NILE_STORAGE_DIR` (`off` to
/// disable) to move that. Returns 0 on success.
///
/// # Safety
/// `view` must be a live view and `path` a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_view_set_storage_file(view: *mut NileView, path: *const c_char) -> i32 {
    let (Some(v), Some(path)) = (view_ref(view), text(path)) else { return 2 };
    guard(2, || {
        v.page.set_storage_file(std::path::Path::new(path));
        0
    })
}

/// A click on `<input type=file>` asks the host for a file chooser. If one is waiting, returns 1,
/// sets `*multiple` (0 or 1) and `*accept` (the `accept` attribute, free it with
/// [`nile_string_free`]; either may be NULL), and remembers the input; answer with
/// [`nile_view_choose_files`]. Returns 0 if nothing is waiting.
///
/// # Safety
/// `view` must be a live view; `multiple` and `accept` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn nile_view_take_file_request(view: *mut NileView, multiple: *mut i32, accept: *mut *mut c_char) -> i32 {
    let Some(v) = view_ref(view) else { return 0 };
    guard(0, || match v.page.take_file_request() {
        Some(r) => {
            if !multiple.is_null() {
                unsafe { *multiple = i32::from(r.multiple) };
            }
            if !accept.is_null() {
                unsafe { *accept = into_c(r.accept) };
            }
            1
        }
        None => 0,
    })
}

/// Answer the request from [`nile_view_take_file_request`] with the chosen files, one path per
/// line (`"a.txt\nb.txt"`); an empty string clears the input. Returns 0 on success, 1 if no
/// request was taken.
///
/// # Safety
/// `view` must be a live view and `paths` a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_view_choose_files(view: *mut NileView, paths: *const c_char) -> i32 {
    let (Some(v), Some(paths)) = (view_ref(view), text(paths)) else { return 2 };
    guard(2, || {
        let list: Vec<String> = paths.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
        i32::from(!v.page.choose_files_for_request(&list))
    })
}

/// Whether the focused element takes text, so the host should enable its input method (IME).
/// Returns 1 or 0.
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_ime_wanted(view: *mut NileView) -> i32 {
    let Some(v) = view_ref(view) else { return 0 };
    guard(0, || i32::from(v.page.ime_wanted()))
}

/// Where the input method's candidate window should go: the caret, in logical pixels of the
/// view. Returns 1 and fills the four outputs (any may be NULL), or 0 if there is no caret.
///
/// # Safety
/// `view` must be a live view; the outputs NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn nile_view_ime_cursor_area(view: *mut NileView, x: *mut f32, y: *mut f32, w: *mut f32, h: *mut f32) -> i32 {
    let Some(v) = view_ref(view) else { return 0 };
    guard(0, || match v.page.ime_cursor_rect() {
        Some((rx, ry, rw, rh)) => {
            for (out, val) in [(x, rx), (y, ry), (w, rw), (h, rh)] {
                if !out.is_null() {
                    unsafe { *out = val };
                }
            }
            1
        }
        None => 0,
    })
}

/// The input method is composing `utf8` text. `cursor_start`/`cursor_end` are the composition's
/// own caret as byte offsets into the text, or both -1 for none. An empty string ends the
/// composition. Returns 1 if the page changed.
///
/// # Safety
/// `view` must be a live view and `utf8` a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_view_ime_preedit(view: *mut NileView, utf8: *const c_char, cursor_start: i32, cursor_end: i32) -> i32 {
    let (Some(v), Some(text)) = (view_ref(view), text(utf8)) else { return 0 };
    let cursor = (cursor_start >= 0 && cursor_end >= 0).then_some((cursor_start as usize, cursor_end as usize));
    guard(0, || i32::from(v.page.ime_preedit(text, cursor)))
}

/// The input method finished composing: insert `utf8` text where the composition was.
/// Returns 1 if the page changed.
///
/// # Safety
/// `view` must be a live view and `utf8` a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_view_ime_commit(view: *mut NileView, utf8: *const c_char) -> i32 {
    let (Some(v), Some(text)) = (view_ref(view), text(utf8)) else { return 0 };
    guard(0, || i32::from(v.page.ime_commit(text)))
}

/// Abandon the composition (the input method was switched off or lost focus).
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_ime_cancel(view: *mut NileView) {
    let Some(v) = view_ref(view) else { return };
    guard((), || v.page.ime_cancel());
}

/// The `console.log`/`warn`/`error` lines written since the last call, separated by newlines
/// (empty if none), as a new string to free with [`nile_string_free`].
///
/// # Safety
/// `view` must be a live view.
#[no_mangle]
pub unsafe extern "C" fn nile_view_take_console(view: *mut NileView) -> *mut c_char {
    let Some(v) = view_ref(view) else { return ptr::null_mut() };
    guard(ptr::null_mut(), || into_c(v.page.take_console().join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> CString {
        CString::new(x).unwrap()
    }

    unsafe fn take(p: *mut c_char) -> String {
        let out = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        unsafe { nile_string_free(p) };
        out
    }

    const PAGE: &str = r#"<html window-title="Demo" window-frame="solid" window-width="320"><body style="margin:0">
        <button id="b" style="width:100px;height:30px;margin:0" role="window-close">x</button>
        <input id="i" style="margin:0;width:100px">
        <script>
          document.$('#i').addEventListener('input', () => console.log('typed:' + document.$('#i').value));
          function greet(n) { return { hello: n, sum: host.add(1, 2) }; }
        </script></body></html>"#;

    unsafe extern "C" fn add(args: *const c_char, user: *mut c_void) -> *const c_char {
        let calls = unsafe { &mut *(user as *mut Vec<String>) };
        calls.push(unsafe { CStr::from_ptr(args) }.to_string_lossy().into_owned());
        c"3".as_ptr()
    }

    #[test]
    fn a_c_host_can_open_windows_and_pass_messages() {
        unsafe {
            let main = nile_view_create_html(
                s(r#"<body><script>
                     globalThis.got = [];
                     globalThis.w = new Window({ html: "<body><p id=x>child</p><script>window.addEventListener('message', e => document.$('#x').text = 'got ' + e.data.hi);<\/script></body>", width: 300, height: 200, parameters: { n: 7 } });
                     w.closed.then(v => got.push('closed:' + JSON.stringify(v)));
                     Window.this.post(1, {}); </script></body>"#)
                .as_ptr(),
                ptr::null(),
                400.0,
                300.0,
            );
            let mut open = None;
            while let p @ 1.. = nile_view_take_window_request(main) as usize {
                let json = take(p as *mut c_char);
                if json.contains("\"kind\":\"open\"") {
                    open = Some(json);
                }
            }
            let open = open.expect("new Window() is an open request");
            assert!(open.contains("\"width\":\"300\"") && open.contains("\"opener\":"), "{open}");
            let id: u32 = open.split("\"id\":").nth(1).unwrap().split(',').next().unwrap().parse().unwrap();
            let me = nile_view_window_id(main);
            assert_ne!(id, me);
            let child = nile_view_create_window(ptr::null(), s("<body><p id=x>child</p><script>window.addEventListener('message', e => document.$('#x').text = 'got ' + e.data.hi);</script></body>").as_ptr(), ptr::null(), 300.0, 200.0, id, me, false, s("{\"n\":7}").as_ptr());
            assert_eq!(nile_view_window_id(child), id);
            assert!(nile_view_window_message(child, me, s("{\"hi\":\"there\"}").as_ptr()));
            let mut out = ptr::null_mut();
            nile_view_eval(child, s("document.$('#x').text + ':' + Window.this.parameters.n").as_ptr(), &mut out);
            assert_eq!(take(out), "got there:7");
            assert!(nile_view_close_request(child, 1));
            nile_view_window_closed(main, id, s("\"ok\"").as_ptr());
            let mut out = ptr::null_mut();
            nile_view_eval(main, s("got.join()").as_ptr(), &mut out);
            assert_eq!(take(out), "closed:\"ok\"");
            nile_view_destroy(child);
            nile_view_destroy(main);
        }
    }

    #[test]
    fn a_c_host_can_load_render_type_script_and_get_window_requests() {
        unsafe {
            let v = nile_view_create_html(s(PAGE).as_ptr(), ptr::null(), 400.0, 200.0);
            assert!(!v.is_null());
            assert_eq!(CStr::from_ptr(nile_version()).to_str().unwrap(), env!("CARGO_PKG_VERSION"));

            // Render into our own buffer; the white-ish page background and the button show up.
            let (w, h) = (400u32, 200u32);
            let mut px = vec![0u8; (w * h * 4) as usize];
            assert!(nile_view_render(v, px.as_mut_ptr(), px.len(), w, h, 1.0));
            assert!(px.chunks(4).any(|p| p != [0, 0, 0, 0]), "something was painted");
            assert!(!nile_view_render(v, px.as_mut_ptr(), 10, w, h, 1.0), "short buffers are rejected");

            // Type into the input and watch scripts react.
            nile_pointer_move(v, 150.0, 12.0);
            nile_pointer_button(v, 0, true, 0);
            nile_pointer_button(v, 0, false, 0);
            assert!(nile_text_input(v, s("hi").as_ptr()));
            assert_eq!(take(nile_view_take_console(v)), "typed:h\ntyped:hi");
            assert_eq!(take(nile_view_take_console(v)), "", "console lines are delivered once");

            // Eval, call, and a host function the script calls back.
            let mut out = ptr::null_mut();
            assert_eq!(nile_view_eval(v, s("document.$('#i').value.length").as_ptr(), &mut out), 0);
            assert_eq!(take(out), "2");
            assert_eq!(nile_view_eval(v, s("throw new Error('boom')").as_ptr(), &mut out), 1);
            assert!(take(out).contains("boom"));
            let mut calls: Vec<String> = Vec::new();
            assert!(nile_view_register_function(v, s("add").as_ptr(), Some(add), &mut calls as *mut _ as *mut c_void));
            assert_eq!(nile_view_call(v, s("greet").as_ptr(), s(r#"["Ada"]"#).as_ptr(), &mut out), 0);
            assert_eq!(take(out), r#"{"hello":"Ada","sum":3}"#);
            assert_eq!(calls, ["[1,2]"]);

            // Window: config, title, and the close button queued as a request.
            let mut cfg = std::mem::zeroed::<NileWindowConfig>();
            assert!(nile_view_window_config(v, 1000.0, 800.0, &mut cfg));
            assert_eq!((cfg.width, cfg.frame, cfg.frameless), (320.0, 2, true));
            assert_eq!(CStr::from_ptr(cfg.title).to_str().unwrap(), "Demo");
            nile_pointer_move(v, 20.0, 10.0);
            nile_pointer_button(v, 0, true, 0);
            nile_pointer_button(v, 0, false, 0);
            let mut cmd = NileWindowCommand { kind: NileWindowCommandKind::None, a: 0.0, b: 0.0 };
            assert!(nile_view_take_window_command(v, &mut cmd));
            assert_eq!(cmd.kind, NileWindowCommandKind::Close);
            assert!(!nile_view_take_window_command(v, &mut cmd));
            assert_eq!(cmd.kind, NileWindowCommandKind::None);

            let mut tick = NileTick { redraw: false, next_ms: 0 };
            nile_view_tick(v, &mut tick);
            assert!(tick.next_ms == -1 || tick.next_ms >= 0);
            nile_view_destroy(v);
        }
    }

    #[test]
    fn translucent_config_dialogs_drops_and_portals_reach_c() {
        unsafe {
            let html = s(r#"<html window-frame="solid-with-shadow" window-material="mica"><body><input id="f" type="file" style="width:200px;height:30px"><script>
                window.dropped = ""; window.active = "";
                document.getElementById("f").addEventListener("change", () => { window.dropped = this_files(); });
                function this_files() { return document.getElementById("f").files.length + ":" + document.getElementById("f").files[0].name; }
                Window.this.on("activate", (e) => { window.active += e.reason; });
            </script></body></html>"#);
            let v = nile_view_create_html(html.as_ptr(), ptr::null(), 300.0, 200.0);
            let mut cfg = std::mem::zeroed::<NileWindowConfig>();
            assert!(nile_view_window_config(v, 1000.0, 800.0, &mut cfg));
            assert_eq!((cfg.frame, cfg.shadow, cfg.translucent, cfg.material), (4, true, true, 3));

            // Dialogs: a host that answers every open dialog with one file.
            unsafe extern "C" fn open(_m: bool, _f: *const c_char, caption: *const c_char, _d: *const c_char, _u: *mut c_void) -> *const c_char {
                assert_eq!(CStr::from_ptr(caption).to_str().unwrap(), "Pick");
                c"/tmp/nile-capi-dialog.txt".as_ptr()
            }
            nile_view_set_dialogs(v, NileDialogs { open_files: Some(open), save_file: None, pick_folder: None, message: None, user: ptr::null_mut() });
            let mut tick = NileTick { redraw: false, next_ms: 0 };
            nile_view_tick(v, &mut tick);
            assert!(nile_view_eval_ok(v, "String(Window.this.selectFile({ mode: 'open', caption: 'Pick' })).endsWith('nile-capi-dialog.txt')"));

            // Activation and file drops (the pointer says where).
            assert!(nile_view_eval_ok(v, "true"));
            nile_view_window_activated(v, true);
            assert!(nile_view_eval_ok(v, "window.active === '1'"));
            nile_pointer_move(v, 20.0, 15.0);
            assert!(nile_view_drop_files(v, s("/tmp/nile-capi-drop.txt").as_ptr()));
            assert!(nile_view_eval_ok(v, "window.dropped === '1:nile-capi-drop.txt'"));

            // Portals: none yet; an unknown id is refused.
            assert_eq!(take(nile_view_portals(v)), "[]");
            let mut px = [0u8; 16];
            assert!(!nile_view_render_portal(v, 12345, px.as_mut_ptr(), px.len(), 2, 2, 1.0));
            nile_view_destroy(v);
        }
    }

    unsafe fn nile_view_eval_ok(v: *mut NileView, code: &str) -> bool {
        let mut out = ptr::null_mut();
        let code = unsafe { nile_view_eval(v, s(code).as_ptr(), &mut out) };
        code == 0 && !out.is_null() && unsafe { take(out) } == "true"
    }

    #[test]
    fn a_c_host_can_set_the_system_theme() {
        unsafe {
            let html = s(r#"<html><head><style>:root { color-scheme: light dark }</style></head><body><button id="b">x</button></body></html>"#);
            let v = nile_view_create_html(html.as_ptr(), ptr::null(), 200.0, 100.0);
            assert!(nile_view_set_system_theme(v, true, 0x336699));
            assert!(!nile_view_set_system_theme(v, true, 0x336699), "unchanged");
            let mut out = ptr::null_mut();
            assert_eq!(nile_view_eval(v, s("getComputedStyle(document.getElementById('b')).color").as_ptr(), &mut out), 0);
            assert_eq!(take(out), "rgb(255, 255, 255)", "the dark controls");
            assert!(nile_view_set_system_theme(v, false, -1));
            nile_view_destroy(v);
        }
    }

    #[test]
    fn bad_arguments_fail_softly() {
        unsafe {
            assert!(nile_view_create_html(ptr::null(), ptr::null(), 10.0, 10.0).is_null());
            assert!(!nile_pointer_move(ptr::null_mut(), 0.0, 0.0));
            assert_eq!(nile_view_eval(ptr::null_mut(), s("1").as_ptr(), ptr::null_mut()), 2);
            nile_view_destroy(ptr::null_mut());
            nile_string_free(ptr::null_mut());
        }
    }
}
