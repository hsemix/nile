//! JavaScript runtime (QuickJS via rquickjs) with a web-style DOM.
//!
//! Rust exposes a small set of native functions over the arena DOM
//! (`__nile`, installed by `install_natives`); `prelude.js` builds the familiar
//! classes on top (`document`, `Element`, `classList`, `style`, events with
//! capture/bubble, timers). Nodes are identified by arena id, so JS wrappers
//! are cheap and detached nodes simply stay in the arena.
//!
//! Layout-dependent APIs (`getBoundingClientRect`, `getComputedStyle`, `offset*`,
//! window size and scrolling) go through [`Environment`], which the embedder
//! implements so it can flush pending style/layout work first.
//!
//! Not yet: `fetch`, modules/JSX in the loader, Sciter's own DOM extensions.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nile_dom::{Document, NodeId, NodeKind};
use nile_style::{InteractionState, Query};
use rquickjs::{
    function::Func,
    loader::{ImportAttributes, Loader, Resolver},
    module::Declared,
    CatchResultExt, Context, Ctx, Exception, Function, Module, Object, Runtime,
};

mod extensions;
mod crypto;
mod desktop;
mod envmod;
mod integration;
pub use integration::mime_of;
mod jsx;
mod keychain;
mod serial;
mod net;
mod sys;

pub use net::{file_url, path_of_url, FetchRequest, FetchResponse, Fetcher, LocalFetcher};

const EDIT: &str = include_str!("edit.js");
const WEB: &str = include_str!("web.js");
const CANVAS: &str = include_str!("canvas.js");
const AUDIO: &str = include_str!("audio.js");
const CRYPTO: &str = include_str!("crypto.js");
const INTL: &str = include_str!("intl.js");
// `@nile/ui`: the component library, whole modules loaded by `import`.
const UI_MODULES: &[(&str, &str)] = &[
    ("@nile/ui", include_str!("ui/index.js")),
    ("@nile/ui/base", include_str!("ui/base.js")),
    ("@nile/ui/barcode", include_str!("ui/barcode.js")),
    ("@nile/ui/drawer", include_str!("ui/drawer.js")),
    ("@nile/ui/splitter", include_str!("ui/splitter.js")),
    ("@nile/ui/grid", include_str!("ui/grid.js")),
    ("@nile/ui/dialog", include_str!("ui/dialog.js")),
    ("@nile/ui/toast", include_str!("ui/toast.js")),
    ("@nile/ui/shortcuts", include_str!("ui/shortcuts.js")),
    ("@nile/ui/tabs", include_str!("ui/tabs.js")),
    ("@nile/ui/accordion", include_str!("ui/accordion.js")),
    ("@nile/ui/combobox", include_str!("ui/combobox.js")),
    ("@nile/ui/ribbon", include_str!("ui/ribbon.js")),
    ("@nile/ui/tiles", include_str!("ui/tiles.js")),
    ("@nile/ui/chart", include_str!("ui/chart.js")),
    ("@nile/ui/tree", include_str!("ui/tree.js")),
    ("@nile/ui/keypad", include_str!("ui/keypad.js")),
    ("@nile/ui/date", include_str!("ui/date.js")),
    ("@nile/ui/menu", include_str!("ui/menu.js")),
    ("@nile/ui/palette", include_str!("ui/palette.js")),
    ("@nile/ui/wizard", include_str!("ui/wizard.js")),
    ("@nile/print", include_str!("ui/print.js")),
    ("@nile/escpos", include_str!("ui/escpos.js")),
    ("@nile/crypto", include_str!("ui/passwords.js")),
    ("@nile/keychain", include_str!("ui/keychain.js")),
    ("@nile/serial", include_str!("ui/serial.js")),
    ("@nile/mail", include_str!("ui/mail.js")),
];

/// A module of the embedded library (`@nile/ui`, `@nile/ui/<name>`, `@nile/print`, ...): a name in [`UI_MODULES`], or any
/// `@nile/ui/...` name (an unknown one then fails to load with a clear error).
fn is_library_module(name: &str) -> bool {
    name.starts_with("@nile/ui/") || UI_MODULES.iter().any(|(m, _)| *m == name)
}

fn prelude() -> String {
    include_str!("prelude.js").replace("/*@EDIT@*/", EDIT).replace("/*@WEB@*/", &WEB.replace("/*@CANVAS@*/", CANVAS).replace("/*@AUDIO@*/", AUDIO).replace("/*@CRYPTO@*/", CRYPTO).replace("/*@INTL@*/", INTL))
}

/// Window ids are unique within the process (the main window, then each `new Window()`).
static NEXT_WINDOW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

thread_local! {
    /// `Window.share`: JSON text by key, shared by the windows of one thread.
    static SHARE: RefCell<std::collections::HashMap<String, String>> = RefCell::default();
}

static APP_ARGUMENTS: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

/// The program's own arguments (what is left after the shell took the page and its flags), which
/// `application.arguments` and `@env`'s `arguments()` return. Unset, they are the process arguments.
pub fn set_app_arguments(args: Vec<String>) {
    *APP_ARGUMENTS.lock().unwrap() = Some(args);
}

pub fn app_arguments() -> Vec<String> {
    APP_ARGUMENTS.lock().unwrap().clone().unwrap_or_else(|| std::env::args().skip(1).collect())
}

pub fn next_window_id() -> u32 {
    NEXT_WINDOW.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A window a script asked for with `new Window({...})` or `Window.this.modal({...})`.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowSpec {
    pub id: u32,
    /// The window that asked.
    pub opener: u32,
    /// Shown as a dialog: its opener is blocked until it closes.
    pub modal: bool,
    /// `type`, `url`, `html`, `caption`, `x`, `y`, `width`, `height`, `alignment`, `state`, `parent`
    /// and the rest, as the script wrote them (`parameters` is JSON text).
    pub fields: Vec<(String, String)>,
}

impl WindowSpec {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// What a window knows about how it was opened (`Window.this.parent`, `parameters`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowInfo {
    pub id: u32,
    pub opener: Option<u32>,
    pub modal: bool,
    /// The `parameters` the opener passed, as JSON text.
    pub parameters: String,
}

/// `a=1&b=%20x` as pairs.
pub fn parse_fields(query: &str) -> Vec<(String, String)> {
    let decode = |s: &str| {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'%' if i + 2 < bytes.len() => {
                    if let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    out.push(b'%');
                    i += 1;
                }
                b'+' => {
                    out.push(b' ');
                    i += 1;
                }
                b => {
                    out.push(b);
                    i += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    query.split('&').filter(|p| !p.is_empty()).map(|p| match p.split_once('=') {
        Some((k, v)) => (decode(k), decode(v)),
        None => (decode(p), String::new()),
    }).collect()
}

/// Something a script asks the host window to do.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowCommand {
    Close,
    /// Close this window; the text is the value for the opener's `modal()` (JSON).
    CloseWith(String),
    /// Open another window.
    Open(WindowSpec),
    /// Deliver `json` to window `to` as a `message` event.
    Post { to: u32, json: String },
    /// Close another window by id.
    CloseId(u32),
    /// Bring another window to the front.
    FocusId(u32),
    /// `application.quit(code)`: end the program with this exit code.
    Quit(i32),
    Minimize,
    Maximize,
    /// Back from maximized, minimized or full screen.
    Restore,
    ToggleMaximize,
    Fullscreen(bool),
    /// Outer position in screen pixels.
    MoveTo(f32, f32),
    /// Inner size in CSS px.
    ResizeTo(f32, f32),
}

/// `application.progress`: the progress shown on the dock icon / taskbar button.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Progress {
    #[default]
    None,
    Indeterminate,
    /// 0.0 to 1.0.
    Value(f32),
}

/// `Window.this.tray = {...}`: the notification-area (system tray) icon the page asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct TraySpec {
    /// The icon file, as the page wrote it (relative to the page).
    pub icon: String,
    pub tooltip: String,
    /// The `<menu>` shown when the icon is clicked with the secondary button.
    pub menu: Option<NodeId>,
    /// A single-colour icon the system may recolour (macOS menu bar).
    pub template: bool,
}

/// One monitor, as the window system reports it (physical pixels; `scale` is device pixels per CSS px).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScreenInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub primary: bool,
}

/// What scripts can ask of the embedder: layout results and the viewport.
/// Implementations must bring styles/layout up to date with the document
/// before answering, since scripts expect to read what they just wrote.
pub trait Environment {
    /// Border box `[x, y, width, height]` in viewport coordinates, or `None`
    /// if the node has no box (`display: none`, detached, ...).
    fn bounding_rect(&self, node: NodeId) -> Option<[f32; 4]>;
    /// The computed value of a CSS property as text, if known.
    fn computed_style(&self, node: NodeId, property: &str) -> Option<String>;
    /// Viewport size in CSS px.
    fn viewport(&self) -> (f32, f32);
    /// The system theme is dark (`prefers-color-scheme: dark`).
    fn system_dark(&self) -> bool {
        false
    }
    /// The encoded bytes of an image the page names (`<img src>`), for `drawImage` and patterns.
    fn image_bytes(&self, _src: &str) -> Option<Vec<u8>> {
        None
    }
    /// Device pixels per logical pixel (`devicePixelRatio`).
    fn scale(&self) -> f32 {
        1.0
    }
    fn scroll_y(&self) -> f32;
    fn scroll_to(&self, y: f32);
    /// Hover/active/focus, for `:hover` etc. in selectors scripts evaluate.
    fn interaction(&self) -> InteractionState;
    /// Move keyboard focus (`el.focus()` / `el.blur()`).
    fn set_focus(&self, node: Option<NodeId>);
    /// A script assigned the focused input's value: that is not an edit to fire `change` for.
    fn value_synced(&self, _node: NodeId) {}
    /// `[scrollLeft, scrollTop, scrollWidth, scrollHeight]` of an element that clips its content.
    fn element_scroll(&self, node: NodeId) -> Option<[f32; 4]>;
    /// `[sheet count, current sheet (1-based)]` of a `page-view: paged` element: the sheet with the
    /// caret, else the one at the middle of the window.
    fn page_info(&self, _node: NodeId) -> Option<[usize; 2]> {
        None
    }
    /// Scroll an element (clamped to its range); fires a `scroll` event if it moved.
    fn scroll_element_to(&self, node: NodeId, x: f32, y: f32);
    /// `(start, end)` of an input's selection, in characters of its value.
    fn selection(&self, node: NodeId) -> (usize, usize);
    fn set_selection(&self, node: NodeId, start: usize, end: usize);
    /// The text selected on the page (outside inputs); empty if none.
    fn selected_text(&self) -> String {
        String::new()
    }
    fn clear_selected_text(&self) {}
    /// The editing selection (anchor, focus) as DOM positions: character offsets in text
    /// nodes, child indices in elements.
    fn edit_selection(&self) -> Option<[(NodeId, usize); 2]> {
        None
    }
    fn set_edit_selection(&self, _anchor: (NodeId, usize), _focus: (NodeId, usize)) {}
    fn clear_edit_selection(&self) {}
    /// `execCommand('copy' | 'cut' | 'paste')`: the host does it with its clipboard.
    fn clipboard_request(&self, _kind: &str) {}
    /// Read one clipboard flavour: `text`, `html`, `json`, or `files` (paths joined by newlines).
    fn clipboard_get(&self, _kind: &str) -> Option<String> {
        None
    }
    /// The clipboard's picture as (width, height, RGBA bytes).
    fn clipboard_get_image(&self) -> Option<(u32, u32, Vec<u8>)> {
        None
    }
    /// Put one flavour on the clipboard: `text` (a), `html` (a, with `b` as its plain text),
    /// `files` (a: paths joined by newlines) or `json` (a, with `b` as its text form).
    fn clipboard_put(&self, _kind: &str, _a: &str, _b: Option<&str>) -> bool {
        false
    }
    fn clipboard_put_image(&self, _width: u32, _height: u32, _rgba: &[u8]) -> bool {
        false
    }
    /// Close, move, resize, ... the host window. Embedders without a window ignore it.
    fn window_command(&self, _cmd: WindowCommand) {}
    /// Show the popup element `node` in an OS window of its own (`rect`: its border box in viewport
    /// coordinates), or take it back.
    fn portal_set(&self, _node: NodeId, _rect: [f32; 4]) {}
    /// `Window.this.print()` / `exportPdf()`: the page should write its PDF to `path` (empty: a temp file) with `title`
    /// and answer with `__printReady(id, path, error)`.
    fn print_request(&self, _id: u32, _path: &str, _title: &str) {}
    /// The `<menu>` element chosen as the application menu bar, or none.
    fn set_app_menu(&self, _node: Option<NodeId>) {}
    /// The host shows `Window.this.menu` as a system menu bar (macOS, Windows). When it doesn't, the
    /// script draws an in-window bar itself.
    fn native_menu(&self) -> bool {
        true
    }
    /// An item of the in-window menu bar was chosen: run it like a native menu choice.
    fn menu_invoke(&self, _id: u64) {}
    /// Show (or with `None` remove) the tray icon.
    fn set_tray(&self, _spec: Option<TraySpec>) {}
    /// The text on the dock / taskbar icon (`application.badge`); `None` clears it.
    fn set_badge(&self, _text: Option<String>) {}
    fn set_progress(&self, _progress: Progress) {}
    /// The global hotkeys the page wants (`application.registerHotkey`): `(id, accelerator)`.
    fn set_hotkeys(&self, _keys: Vec<(u32, String)>) {}
    fn portal_clear(&self, _node: NodeId) {}
    /// How this window was opened.
    fn window_info(&self) -> WindowInfo {
        WindowInfo::default()
    }
    /// A native dialog (`open`, `save`, `folder`, `message`) with four text arguments; the
    /// answer as text, "" when cancelled, `None` when the host has no dialogs.
    fn dialog(&self, _kind: &str, _args: [&str; 4]) -> Option<String> {
        None
    }
    /// The monitors and the index of the one the window is on (`Window.screens`, `screenBox`).
    fn screens(&self) -> (Vec<ScreenInfo>, usize) {
        (Vec::new(), 0)
    }
    /// Window `[screenX, screenY, outerWidth, outerHeight, fullscreen (0/1)]`.
    fn window_rect(&self) -> [f32; 5] {
        let (w, h) = self.viewport();
        [0.0, 0.0, w, h, 0.0]
    }
}

/// Resolves `./` and `../` imports against the importing module's file.
#[derive(Default)]
struct FileResolver { packages: Vec<nile_extensions::Package> }

impl Resolver for FileResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        if let Some(package) = self.packages.iter().find(|p| p.import == name) {
            return Ok(package.entry.to_string_lossy().into_owned());
        }
        if name == "@native" {
            let base = std::path::Path::new(base.split('#').next().unwrap_or(base));
            if let Some(package) = self.packages.iter().find(|p| base.starts_with(&p.root)) {
                return Ok(format!("@native:{}", package.host));
            }
            return Err(rquickjs::Error::new_resolving(base.to_string_lossy(), name));
        }
        let path = std::path::Path::new(name);
        if matches!(name, "@nile" | "@sciter" | "@env" | "@sys" | "@debug" | "@storage") || is_library_module(name) {
            // `@sciter` is the same module under the name Sciter apps use.
            return Ok(if name == "@sciter" { "@nile".to_string() } else { name.to_string() });
        }
        // `file://` and `this://app/...` URLs (`__DIR__ + "x.js"`) name files too.
        if name.starts_with("file://") || name.starts_with("this://") {
            let mapped = nile_dom::vfs::url_to_path(name).map(|p| p.to_string_lossy().into_owned());
            let local = mapped.as_deref().unwrap_or_else(|| path_of_url(name));
            return Ok(normalize(std::path::Path::new(local)));
        }
        if name.starts_with("./") || name.starts_with("../") {
            // Inline modules are named `page.html#moduleN`; imports resolve from the page's folder.
            let base_file = base.split('#').next().unwrap_or(base);
            let dir = std::path::Path::new(base_file).parent().unwrap_or(std::path::Path::new(""));
            Ok(normalize(&dir.join(path)))
        } else if path.is_absolute() {
            Ok(normalize(path))
        } else {
            Err(rquickjs::Error::new_resolving(base, name))
        }
    }
}

/// Lexically remove `.` and `..` components.
fn normalize(path: &std::path::Path) -> String {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Loads module files from disk, transforming JSX.
#[derive(Default)]
struct FileLoader { packages: Vec<nile_extensions::Package> }

impl Loader for FileLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, Declared>> {
        if let Some(host) = name.strip_prefix("@native:") {
            if self.packages.iter().any(|p| p.host.to_string() == host) {
                return Module::declare(ctx.clone(), name, format!("export default globalThis.__nileExtensionScope({host:?});"));
            }
            return Err(rquickjs::Error::new_loading(name));
        }
        // Built-in modules are thin wrappers over objects the prelude defines.
        let builtin = match name {
            "@sys" => Some("const m = globalThis.__sysModule;\nexport const fs = m.fs;\nexport const TCP = m.TCP;\nexport const UDP = m.UDP;\nexport const Pipe = m.Pipe;\nexport const TTY = m.TTY;\nexport const Process = m.Process;\nexport const spawn = m.spawn;\nexport const hrtime = m.hrtime;\nexport const gettimeofday = m.gettimeofday;\nexport const uname = m.uname;\nexport const isatty = m.isatty;\nexport const environ = m.environ;\nexport const getenv = m.getenv;\nexport const setenv = m.setenv;\nexport const unsetenv = m.unsetenv;\nexport const cwd = m.cwd;\nexport const chdir = m.chdir;\nexport const homedir = m.homedir;\nexport const tmpdir = m.tmpdir;\nexport const exepath = m.exepath;\nexport const random = m.random;\nexport default m;"),
            "@nile" => Some(
                "const m = globalThis.__sciterModule;\nexport const { Clipboard, VERSION, REVISION, QUICKJS_VERSION, setModuleUrlResolver, loadLibrary, parseValue, devicePixels, on, off, once, onGlobalEvent, offGlobalEvent, $, $$, uuid, encode, decode, compress, decompress, toBase64, fromBase64, md5, crc32 } = m;\nconst imp = m.import;\nexport { imp as import };\nexport default m;",
            ),
            "@debug" => Some("const m = globalThis.__debugModule;\nexport const { setUnhandledExceptionHandler, setConsoleOutputHandler, callStackAt, setResourceArrivalHandler, setBreakpointHandler, setBreakpoints, getElementByUID, getUIDofElement, highlightElement, getStyleRulesOfElement, containerId, objectKind, sublimatedValue, sublimatedValueElements, frameVariables } = m;\nexport default m;"),
            "@storage" => Some("const m = globalThis.__storageModule;\nexport const open = m.open;\nexport default m;"),
            "@env" => Some(
                "const m = globalThis.__envModule;\nexport const { language, country, userName, machineName, domainName, arguments: args, launch, reveal, moveToTrash, openWith, online, power, idleSeconds, home, homeURL, path, pathURL, drives, variable, exec, DEVICE } = m;\nexport { args as arguments };\nexport const OS = m.OS, PLATFORM = m.PLATFORM;\nexport default m;",
            ),
            // Everything else under `@nile/...` is a library module from `UI_MODULES`: the one list to extend.
            n => UI_MODULES.iter().find(|(m, _)| *m == n).map(|(_, src)| *src),
        };
        if let Some(source) = builtin {
            return Module::declare(ctx.clone(), name, source);
        }
        let source = nile_dom::vfs::read_to_string(std::path::Path::new(name)).map_err(|_| rquickjs::Error::new_loading(name))?;
        let source = jsx::transform(&source).unwrap_or(source);
        Module::declare(ctx.clone(), name, with_location(name, source))
    }
}

/// `__DIR__` and `__FILE__` (Sciter): the module's folder (with a trailing slash) and file as URLs,
/// declared on the module's first line so line numbers stay put. Inline modules (`page.html#module0`)
/// keep the page-wide globals.
fn with_location(name: &str, source: String) -> String {
    if name.contains('#') || name.starts_with('@') {
        return source;
    }
    let path = std::path::Path::new(name);
    let absolute = std::fs::canonicalize(path).ok().filter(|_| !nile_dom::vfs::is_virtual(path)).unwrap_or_else(|| path.to_path_buf());
    let url = |p: &std::path::Path| nile_dom::vfs::app_url(p).unwrap_or_else(|| file_url(p));
    let dir = absolute.parent().map(|d| format!("{}/", url(d).trim_end_matches('/'))).unwrap_or_default();
    format!("const __DIR__ = {dir:?}, __FILE__ = {:?}; {source}", url(&absolute))
}

/// An input event for [`ScriptHost::dispatch`].
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    /// `click`, `mousedown`, `mouseup`, `mousemove`, `mouseover`, `mouseout`. `mods` is the held modifier keys as
    /// bits: 1 shift, 2 control, 4 alt, 8 meta.
    Mouse { kind: &'static str, x: f32, y: f32, button: i32, mods: u8 },
    /// `keydown`, `keyup`.
    Key { kind: &'static str, key: String, ctrl: bool, shift: bool, alt: bool, meta: bool },
    /// `focus`, `blur`, ... with no payload.
    Plain(&'static str),
    /// `transitionend` (`name` = property), `animationstart`/`animationend` (`name` = keyframes).
    Animation { kind: &'static str, name: String, elapsed: f64 },
}

pub struct ScriptHost {
    extensions: Rc<RefCell<nile_extensions::Extensions>>,
    runtime: Runtime,
    context: Context,
    log: Arc<Mutex<Vec<String>>>,
    start: Instant,
    host_fns: HostFns,
    net: net::NetState,
    sys: sys::SysState,
    observing: Rc<std::cell::Cell<bool>>,
    storage: Rc<RefCell<Option<std::path::PathBuf>>>,
}

/// Functions the embedder exposes to scripts as `host.name(...)`: each takes the call's
/// arguments as a JSON array and returns a JSON value (or an empty string for `undefined`).
type HostFnMap = std::collections::HashMap<String, Rc<dyn Fn(&str) -> String>>;
type HostFns = Rc<RefCell<HostFnMap>>;

thread_local! {
    static DEFAULT_HOST_FNS: RefCell<HostFnMap> = RefCell::default();
}

/// Expose `f` to the scripts of every page this thread creates from now on, as `host.<name>(...)`,
/// before their first script runs (what a standalone program needs, since it makes the pages
/// itself). A page can still register its own with [`ScriptHost::register_host_fn`].
pub fn add_default_host_fn(name: &str, f: impl Fn(&str) -> String + 'static) {
    DEFAULT_HOST_FNS.with(|m| m.borrow_mut().insert(name.to_string(), Rc::new(f)));
}

/// Character offset `chars` of a text node as UTF-16 units (element offsets are child indices).
fn to_utf16(doc: &Document, node: NodeId, chars: usize) -> usize {
    match &doc.node(node).kind {
        nile_dom::NodeKind::Text(t) => t.chars().take(chars).map(char::len_utf16).sum(),
        _ => chars,
    }
}

/// The inverse of [`to_utf16`]: a position inside a surrogate pair rounds down.
fn from_utf16(doc: &Document, node: NodeId, units: usize) -> usize {
    match &doc.node(node).kind {
        nile_dom::NodeKind::Text(t) => {
            let (mut chars, mut u) = (0, 0);
            for c in t.chars() {
                if u + c.len_utf16() > units {
                    break;
                }
                u += c.len_utf16();
                chars += 1;
            }
            chars
        }
        _ => units,
    }
}

fn node(id: i32) -> Option<NodeId> {
    usize::try_from(id).ok().map(NodeId)
}

fn id_out(id: Option<NodeId>) -> i32 {
    id.map_or(-1, |n| n.0 as i32)
}

/// State the natives share with the [`ScriptHost`].
struct Shared {
    host_fns: HostFns,
    net: net::NetState,
    sys: sys::SysState,
    observing: Rc<std::cell::Cell<bool>>,
    storage: Rc<RefCell<Option<std::path::PathBuf>>>,
}

fn install_natives(
    ctx: &Ctx<'_>,
    doc: Rc<RefCell<Document>>,
    log: Arc<Mutex<Vec<String>>>,
    start: Instant,
    env: Rc<dyn Environment>,
    shared: Shared,
) -> rquickjs::Result<()> {
    let Shared { host_fns, net, sys, observing, storage } = shared;
    let n = Object::new(ctx.clone())?;

    macro_rules! native {
        ($name:literal, $f:expr) => {
            n.set($name, Func::from($f))?;
        };
    }
    // Clone `doc` into each closure under the name `d`.
    macro_rules! with_doc {
        ($d:ident => $f:expr) => {{
            let $d = doc.clone();
            $f
        }};
    }

    native!("root", || 0i32);
    native!("nodeType", with_doc!(d => move |id: i32| -> i32 {
        let d = d.borrow();
        match node(id).filter(|n| n.0 < d.nodes.len()).map(|n| &d.node(n).kind) {
            Some(NodeKind::Element { .. }) => 1,
            Some(NodeKind::Text(_)) => 3,
            _ => 9,
        }
    }));
    native!("tag", with_doc!(d => move |id: i32| -> Option<String> {
        let d = d.borrow();
        node(id).and_then(|n| d.node(n).tag().map(str::to_string))
    }));
    native!("parent", with_doc!(d => move |id: i32| -> i32 {
        let d = d.borrow();
        id_out(node(id).and_then(|n| d.node(n).parent))
    }));
    native!("childNodes", with_doc!(d => move |id: i32| -> Vec<i32> {
        let d = d.borrow();
        node(id).map(|n| d.node(n).children.iter().map(|c| c.0 as i32).collect()).unwrap_or_default()
    }));
    native!("docElement", with_doc!(d => move || -> i32 {
        let d = d.borrow();
        id_out(d.node(Document::ROOT).children.iter().copied().find(|c| d.node(*c).tag() == Some("html")))
    }));
    native!("bodyEl", with_doc!(d => move || -> i32 {
        let d = d.borrow();
        id_out(d.body().filter(|b| d.is_attached(*b)))
    }));
    native!("headEl", with_doc!(d => move || -> i32 {
        let d = d.borrow();
        id_out(d.descendants(Document::ROOT).into_iter().find(|c| d.node(*c).tag() == Some("head")))
    }));
    native!("getAttr", with_doc!(d => move |id: i32, name: String| -> Option<String> {
        let d = d.borrow();
        node(id).and_then(|n| d.node(n).attr(&name.to_ascii_lowercase()).map(str::to_string))
    }));
    native!("setAttr", with_doc!(d => move |id: i32, name: String, value: String| {
        if let Some(n) = node(id) {
            d.borrow_mut().set_attr(n, &name, &value);
        }
    }));
    native!("removeAttr", with_doc!(d => move |id: i32, name: String| {
        if let Some(n) = node(id) {
            d.borrow_mut().remove_attr(n, &name);
        }
    }));
    native!("attrNames", with_doc!(d => move |id: i32| -> Vec<String> {
        let d = d.borrow();
        match node(id).map(|n| &d.node(n).kind) {
            Some(NodeKind::Element { attrs, .. }) => attrs.iter().map(|(k, _)| k.clone()).collect(),
            _ => Vec::new(),
        }
    }));
    native!("text", with_doc!(d => move |id: i32| -> String {
        let d = d.borrow();
        node(id).map(|n| d.text_content(n)).unwrap_or_default()
    }));
    native!("setText", with_doc!(d => move |id: i32, text: String| {
        if let Some(n) = node(id) {
            d.borrow_mut().set_text_content(n, &text);
        }
    }));
    native!("innerHTML", with_doc!(d => move |id: i32| -> String {
        let d = d.borrow();
        node(id).map(|n| d.inner_html(n)).unwrap_or_default()
    }));
    native!("setInnerHTML", with_doc!(d => move |id: i32, html: String| {
        if let Some(n) = node(id) {
            d.borrow_mut().set_inner_html(n, &html);
        }
    }));
    native!("createElement", with_doc!(d => move |tag: String| -> i32 {
        d.borrow_mut().create_element(&tag).0 as i32
    }));
    native!("createText", with_doc!(d => move |text: String| -> i32 {
        d.borrow_mut().create_text(&text).0 as i32
    }));
    native!("append", with_doc!(d => move |ctx: Ctx<'_>, parent: i32, child: i32| -> rquickjs::Result<()> {
        match (node(parent), node(child)) {
            (Some(p), Some(c)) => d
                .borrow_mut()
                .append_child(p, c)
                .map_err(|e| Exception::throw_message(&ctx, &format!("HierarchyRequestError: {e:?}"))),
            _ => Err(Exception::throw_message(&ctx, "invalid node")),
        }
    }));
    native!("insertBefore", with_doc!(d => move |ctx: Ctx<'_>, parent: i32, child: i32, before: i32| -> rquickjs::Result<()> {
        match (node(parent), node(child)) {
            (Some(p), Some(c)) => d
                .borrow_mut()
                .insert_before(p, c, node(before))
                .map_err(|e| Exception::throw_message(&ctx, &format!("HierarchyRequestError: {e:?}"))),
            _ => Err(Exception::throw_message(&ctx, "invalid node")),
        }
    }));
    native!("remove", with_doc!(d => move |id: i32| {
        if let Some(n) = node(id) {
            d.borrow_mut().detach(n);
        }
    }));
    native!("getState", with_doc!(d => move |id: i32, name: String| -> bool {
        node(id).is_some_and(|n| d.borrow().has_state(n, &name))
    }));
    native!("setState", with_doc!(d => move |id: i32, name: String, on: bool| {
        if let Some(n) = node(id) {
            d.borrow_mut().set_state(n, &name, on);
        }
    }));
    native!("byId", with_doc!(d => move |id: String| -> i32 {
        let d = d.borrow();
        id_out(d.descendants(Document::ROOT).into_iter().find(|n| d.node(*n).attr("id") == Some(id.as_str())))
    }));
    native!("select", with_doc!(d => { let env = env.clone(); move |ctx: Ctx<'_>, root: i32, css: String| -> rquickjs::Result<Vec<i32>> {
        let q = Query::parse(&css).map_err(|e| Exception::throw_message(&ctx, &format!("SyntaxError: {e}")))?;
        let state = env.interaction();
        let d = d.borrow();
        Ok(node(root).map(|r| q.select_all_with(&d, r, &state).into_iter().map(|n| n.0 as i32).collect()).unwrap_or_default())
    } }));
    native!("matches", with_doc!(d => { let env = env.clone(); move |ctx: Ctx<'_>, id: i32, css: String| -> rquickjs::Result<bool> {
        let q = Query::parse(&css).map_err(|e| Exception::throw_message(&ctx, &format!("SyntaxError: {e}")))?;
        let d = d.borrow();
        let state = env.interaction();
        Ok(node(id).is_some_and(|n| matches!(d.node(n).kind, NodeKind::Element { .. }) && q.matches_with(&d, n, &state)))
    } }));
    // `matchesIn(el, scope, css)`: `css` may start with a combinator or use `:root`, both relative to `scope`.
    native!("matchesIn", with_doc!(d => { let env = env.clone(); move |ctx: Ctx<'_>, id: i32, scope: i32, css: String| -> rquickjs::Result<bool> {
        let q = Query::parse(&css).map_err(|e| Exception::throw_message(&ctx, &format!("SyntaxError: {e}")))?;
        let d = d.borrow();
        let state = env.interaction();
        Ok(match (node(id), node(scope)) {
            (Some(n), Some(s)) => matches!(d.node(n).kind, NodeKind::Element { .. }) && q.matches_in_scope(&d, n, s, &state),
            _ => false,
        })
    } }));
    native!("rect", {
        let env = env.clone();
        move |id: i32| -> Option<Vec<f32>> { node(id).and_then(|n| env.bounding_rect(n)).map(|r| r.to_vec()) }
    });
    native!("computed", {
        let env = env.clone();
        move |id: i32, prop: String| -> Option<String> { node(id).and_then(|n| env.computed_style(n, &prop)) }
    });
    native!("systemDark", {
        let env = env.clone();
        move || -> bool { env.system_dark() }
    });
    // <canvas>: the surfaces and the 2D operations live in nile-canvas; the page's images come
    // from the embedder through `Environment::image_bytes`.
    native!("canvasCreate", |w: f64, h: f64| -> i32 { nile_canvas::create(w as u32, h as u32).map_or(0, |id| id as i32) });
    native!("canvasResize", |id: i32, w: f64, h: f64| nile_canvas::resize(id as u32, w as u32, h as u32));
    native!("canvasRemove", |id: i32| nile_canvas::remove(id as u32));
    native!("canvasCall", {
        let env = env.clone();
        move |id: i32, op: String, args: Vec<f64>, text: String| -> String {
            struct EnvImages<'a>(&'a dyn Environment);
            impl nile_canvas::Images for EnvImages<'_> {
                fn bytes(&self, src: &str) -> Option<Vec<u8>> {
                    self.0.image_bytes(src)
                }
            }
            // An error comes back as "\u{1}Name: message" for the wrapper to throw.
            match nile_canvas::call(id as u32, &op, &args, &text, &EnvImages(&*env)) {
                Ok(s) => s,
                Err(e) => format!("\u{1}{e}"),
            }
        }
    });
    // Audio: `audioLoad(src)` registers a file as "id,duration" (an error starts with \u{1}), `audioOp` drives it.
    #[cfg(feature = "audio")]
    {
        native!("audioLoad", {
            let env = env.clone();
            move |src: String| -> String {
                let Some(bytes) = env.image_bytes(&src) else { return format!("\u{1}{src}: not found") };
                match nile_audio::load(bytes) {
                    Ok((id, duration)) => format!("{id},{}", duration.map_or(String::new(), |d| d.to_string())),
                    Err(e) => format!("\u{1}{e}"),
                }
            }
        });
        native!("audioOp", |op: String, id: i32, arg: f64| -> String {
            let id = id as u32;
            let done = |r: Result<(), String>| r.map_or_else(|e| format!("\u{1}{e}"), |()| String::new());
            match op.as_str() {
                "play" => done(nile_audio::play(id)),
                "pause" => done(nile_audio::pause(id)),
                "stop" => done(nile_audio::stop(id)),
                "seek" => done(nile_audio::seek(id, arg)),
                "volume" => done(nile_audio::set_volume(id, arg as f32)),
                "muted" => done(nile_audio::set_muted(id, arg != 0.0)),
                "loop" => done(nile_audio::set_loop(id, arg != 0.0)),
                "rate" => done(nile_audio::set_rate(id, arg as f32)),
                "unload" => {
                    nile_audio::unload(id);
                    String::new()
                }
                "state" => match nile_audio::state(id) {
                    Some(s) => format!(
                        "{{\"position\":{},\"playing\":{},\"ended\":{},\"duration\":{}}}",
                        s.position,
                        s.playing,
                        s.ended,
                        s.duration.map_or("null".to_string(), |d| d.to_string())
                    ),
                    None => "\u{1}no such audio".to_string(),
                },
                _ => String::new(),
            }
        });
    }
    #[cfg(not(feature = "audio"))]
    {
        native!("audioLoad", |_src: String| -> String { "\u{1}audio support is not built into this program".to_string() });
        native!("audioOp", |_op: String, _id: i32, _arg: f64| -> String { String::new() });
    }
    native!("viewport", {
        let env = env.clone();
        move || -> Vec<f32> {
            let (w, h) = env.viewport();
            vec![w, h]
        }
    });
    // The editing selection. JS counts text offsets in UTF-16 units, the engine in characters.
    native!("selGet", with_doc!(d => { let env = env.clone(); move || -> Vec<f64> {
        let Some([(an, ao), (fnode, fo)]) = env.edit_selection() else { return Vec::new() };
        let d = d.borrow();
        vec![an.0 as f64, to_utf16(&d, an, ao) as f64, fnode.0 as f64, to_utf16(&d, fnode, fo) as f64]
    } }));
    native!("valueSynced", with_doc!(_d => { let env = env.clone(); move |id: i32| {
        if let Some(n) = node(id) {
            env.value_synced(n);
        }
    } }));
    native!("selSet", with_doc!(d => { let env = env.clone(); move |an: i32, ao: f64, fnode: i32, fo: f64| {
        let (Some(an), Some(fnode)) = (node(an), node(fnode)) else { return };
        let d = d.borrow();
        let (ao, fo) = (from_utf16(&d, an, ao.max(0.0) as usize), from_utf16(&d, fnode, fo.max(0.0) as usize));
        drop(d);
        env.set_edit_selection((an, ao), (fnode, fo));
    } }));
    native!("clipboardRequest", {
        let env = env.clone();
        move |kind: String| env.clipboard_request(&kind)
    });
    native!("clipGet", {
        let env = env.clone();
        move |kind: String| env.clipboard_get(&kind)
    });
    native!("clipGetImage", {
        let env = env.clone();
        move || {
            env.clipboard_get_image().map(|(w, h, rgba)| {
                use base64::Engine as _;
                format!("{w},{h},{}", base64::engine::general_purpose::STANDARD.encode(rgba))
            })
        }
    });
    native!("clipPut", {
        let env = env.clone();
        move |kind: String, a: String, b: Option<String>| env.clipboard_put(&kind, &a, b.as_deref())
    });
    native!("clipPutImage", {
        let env = env.clone();
        move |w: u32, h: u32, data: String| {
            use base64::Engine as _;
            match base64::engine::general_purpose::STANDARD.decode(data.as_bytes()) {
                Ok(rgba) if rgba.len() == w as usize * h as usize * 4 => env.clipboard_put_image(w, h, &rgba),
                _ => false,
            }
        }
    });
    native!("selClear", {
        let env = env.clone();
        move || env.clear_edit_selection()
    });
    native!("windowCommand", {
        let env = env.clone();
        move |name: String, a: f64, b: f64| {
            let (a, b) = (a as f32, b as f32);
            let cmd = match name.as_str() {
                "close" => WindowCommand::Close,
                "minimize" => WindowCommand::Minimize,
                "maximize" => WindowCommand::Maximize,
                "restore" => WindowCommand::Restore,
                "quit" => WindowCommand::Quit(a as i32),
                "toggleMaximize" => WindowCommand::ToggleMaximize,
                "fullscreen" => WindowCommand::Fullscreen(a != 0.0),
                "moveTo" => WindowCommand::MoveTo(a, b),
                "resizeTo" => WindowCommand::ResizeTo(a, b),
                _ => return,
            };
            env.window_command(cmd);
        }
    });
    native!("dialog", {
        let env = env.clone();
        move |kind: String, a: String, b: String, c: String, d: String| -> Option<String> { env.dialog(&kind, [&a, &b, &c, &d]) }
    });
    native!("traySet", {
        let env = env.clone();
        move |on: bool, icon: String, tooltip: String, menu: i32, template: bool| {
            env.set_tray(on.then(|| TraySpec { icon, tooltip, menu: node(menu), template }))
        }
    });
    native!("hotkeysSet", {
        let env = env.clone();
        move |json: String| {
            let list: Vec<(u32, String)> = serde_json::from_str(&json).unwrap_or_default();
            env.set_hotkeys(list)
        }
    });
    native!("progressSet", {
        let env = env.clone();
        // kind: 0 none, 1 indeterminate, 2 a value
        move |kind: i32, value: f64| {
            env.set_progress(match kind {
                1 => Progress::Indeterminate,
                2 => Progress::Value(value.clamp(0.0, 1.0) as f32),
                _ => Progress::None,
            })
        }
    });
    native!("badgeSet", {
        let env = env.clone();
        move |text: String, on: bool| env.set_badge(on.then_some(text))
    });
    native!("menuNative", {
        let env = env.clone();
        move || env.native_menu()
    });
    native!("menuInvoke", {
        let env = env.clone();
        move |id: f64| env.menu_invoke(id as u64)
    });
    native!("menuSet", {
        let env = env.clone();
        move |id: i32| env.set_app_menu(node(id))
    });
    native!("printRequest", {
        let env = env.clone();
        move |id: i32, path: String, title: String| env.print_request(id as u32, &path, &title)
    });
    native!("portalSet", {
        let env = env.clone();
        move |id: i32, x: f32, y: f32, w: f32, h: f32| {
            if let Some(n) = node(id) {
                env.portal_set(n, [x, y, w, h]);
            }
        }
    });
    native!("portalClear", {
        let env = env.clone();
        move |id: i32| {
            if let Some(n) = node(id) {
                env.portal_clear(n);
            }
        }
    });
    native!("windowInfo", {
        let env = env.clone();
        move || -> String {
            let i = env.window_info();
            format!("{}\t{}\t{}\t{}", i.id, i.opener.map_or(String::new(), |o| o.to_string()), i32::from(i.modal), i.parameters)
        }
    });
    // `new Window(...)`: ask for a window and get its id at once.
    native!("windowOpen", {
        let env = env.clone();
        move |query: String, modal: bool| -> u32 {
            let id = next_window_id();
            let opener = env.window_info().id;
            env.window_command(WindowCommand::Open(WindowSpec { id, opener, modal, fields: parse_fields(&query) }));
            id
        }
    });
    native!("windowClose", {
        let env = env.clone();
        move |id: u32, value: String| {
            let me = env.window_info().id;
            env.window_command(if id == 0 || id == me { WindowCommand::CloseWith(value) } else { WindowCommand::CloseId(id) });
        }
    });
    native!("windowPost", {
        let env = env.clone();
        move |to: u32, json: String| env.window_command(WindowCommand::Post { to, json })
    });
    native!("shareGet", move |k: String| -> Option<String> { SHARE.with(|s| s.borrow().get(&k).cloned()) });
    native!("shareSet", move |k: String, v: String| { SHARE.with(|s| s.borrow_mut().insert(k, v)); });
    native!("shareDel", move |k: String| { SHARE.with(|s| s.borrow_mut().remove(&k)); });
    native!("shareKeys", move || -> String { SHARE.with(|s| s.borrow().keys().cloned().collect::<Vec<_>>().join("\n")) });
    native!("windowFocus", {
        let env = env.clone();
        move |id: u32| env.window_command(WindowCommand::FocusId(id))
    });
    native!("screensInfo", {
        let env = env.clone();
        // [[name, x, y, w, h, scale, primary], ...] plus the window's index, as JSON.
        move || -> String {
            let (list, at) = env.screens();
            let rows: Vec<serde_json::Value> = list.iter().map(|s| serde_json::json!([s.name, s.x, s.y, s.width, s.height, s.scale, s.primary])).collect();
            serde_json::json!({ "screens": rows, "at": at }).to_string()
        }
    });
    native!("windowRect", {
        let env = env.clone();
        move || -> Vec<f32> { env.window_rect().to_vec() }
    });
    native!("scrollY", {
        let env = env.clone();
        move || -> f32 { env.scroll_y() }
    });
    native!("scrollTo", {
        let env = env.clone();
        move |y: f32| env.scroll_to(y)
    });
    native!("focus", {
        let env = env.clone();
        move |id: i32| env.set_focus(node(id))
    });
    native!("activeElement", {
        let env = env.clone();
        move || -> i32 { id_out(env.interaction().focus) }
    });
    native!("elScroll", {
        let env = env.clone();
        move |id: i32| -> Option<Vec<f32>> { node(id).and_then(|n| env.element_scroll(n)).map(|r| r.to_vec()) }
    });
    native!("pageInfo", {
        let env = env.clone();
        move |id: i32| -> Option<Vec<i32>> { node(id).and_then(|n| env.page_info(n)).map(|r| r.iter().map(|&v| v as i32).collect()) }
    });
    native!("elScrollTo", {
        let env = env.clone();
        move |id: i32, x: f32, y: f32| {
            if let Some(n) = node(id) {
                env.scroll_element_to(n, x, y);
            }
        }
    });
    native!("selection", {
        let env = env.clone();
        move |id: i32| -> Vec<f64> {
            let (a, b) = node(id).map_or((0, 0), |n| env.selection(n));
            vec![a as f64, b as f64]
        }
    });
    native!("setSelection", {
        let env = env.clone();
        move |id: i32, start: f64, end: f64| {
            if let Some(n) = node(id) {
                env.set_selection(n, start.max(0.0) as usize, end.max(0.0) as usize);
            }
        }
    });
    native!("pageSelection", {
        let env = env.clone();
        move || env.selected_text()
    });
    native!("clearPageSelection", {
        let env = env.clone();
        move || env.clear_selected_text()
    });
    native!("hostCall", {
        let host_fns = host_fns.clone();
        move |ctx: Ctx<'_>, name: String, args: String| -> rquickjs::Result<String> {
            // Clone the handle out first: the callee may register more functions.
            let f = host_fns.borrow().get(&name).cloned();
            match f {
                Some(f) => Ok(f(&args)),
                None => Err(Exception::throw_message(&ctx, &format!("host function '{name}' is not registered"))),
            }
        }
    });
    native!("hostHas", {
        move |name: String| host_fns.borrow().contains_key(&name)
    });
    native!("storageSave", move |json: String| {
        if let Some(path) = storage.borrow().as_ref() {
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, json).and_then(|()| std::fs::rename(&tmp, path)).is_err() {
                eprintln!("[js] could not save localStorage to {}", path.display());
            }
        }
    });
    native!("envCall", |op: String, args: Vec<String>| envmod::call(&op, &args));
    // `@sys.fs`: "O:" + result, or "E:" + error.
    native!("fsCall", |op: String, args: Vec<String>| match envmod::fs(&op, &args) {
        Ok(v) => format!("O:{v}"),
        Err(e) => format!("E:{e}"),
    });
    // `@sys` resources: results are "O:" + text or "E:" + error; blocking operations answer later (`__sysDone`).
    native!("sysCall", |op: String, args: Vec<String>| match sys::call(&op, &args) {
        Ok(v) => format!("O:{v}"),
        Err(e) => format!("E:{e}"),
    });
    native!("sysStart", {
        let sys = sys.clone();
        move |id: u32, op: String, args: Vec<String>| sys::start(&sys, id, op, args)
    });
    native!("cryptoCall", |op: String, args: Vec<String>| match crypto::call(&op, &args) {
        Ok(v) => format!("O:{v}"),
        Err(e) => format!("E:{e}"),
    });
    native!("fileSize", |path: String| std::fs::metadata(path).map_or(-1.0, |m| m.len() as f64));
    native!("scale", {
        let env = env.clone();
        move || env.scale()
    });
    native!("observing", {
        let flag = observing.clone();
        move |on: bool| flag.set(on)
    });
    native!("fetchStart", move |id: u32, method: String, url: String, headers: Vec<String>, body: String| {
        let headers = headers.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0].clone(), c[1].clone())).collect();
        net.start(id, FetchRequest { method, url, headers, body: net::unb64(&body) });
    });
    native!("now", move || start.elapsed().as_secs_f64() * 1000.0);
    native!("log", move |level: String, msg: String| {
        match level.as_str() {
            "warn" | "error" => eprintln!("[js {level}] {msg}"),
            _ => println!("[js] {msg}"),
        }
        log.lock().unwrap().push(msg);
    });

    ctx.globals().set("__nile", n)?;
    Ok(())
}

impl Drop for ScriptHost {
    fn drop(&mut self) {
        self.extensions.borrow().stop_accepting();
        self.call_global("__nativeExtDispose", |_, f| f.call::<_, ()>(()));
        // Stop/join native code before releasing QuickJS or unloading libraries.
        *self.extensions.borrow_mut() = nile_extensions::Extensions::default();
        // A change still waiting for its save timer.
        self.call_global("__storageFlush", |_, f| f.call::<_, ()>(()));
    }
}

impl ScriptHost {
    /// A runtime whose `document` is `doc`. Scripts mutate it in place; the
    /// owner compares `doc.version` after each call to see if anything changed.
    pub fn new(doc: Rc<RefCell<Document>>, env: Rc<dyn Environment>) -> rquickjs::Result<Self> {
        let runtime = Runtime::new()?;
        runtime.set_loader(FileResolver::default(), FileLoader::default());
        let context = Context::full(&runtime)?;
        let log = Arc::new(Mutex::new(Vec::new()));
        let start = Instant::now();
        let host_fns: HostFns = Rc::new(RefCell::new(DEFAULT_HOST_FNS.with(|m| m.borrow().clone())));
        let net = net::NetState::new();
        let sys = sys::SysState::default();
        let observing: Rc<std::cell::Cell<bool>> = Rc::default();
        let storage: Rc<RefCell<Option<std::path::PathBuf>>> = Rc::default();

        context.with(|ctx| -> rquickjs::Result<()> {
            install_natives(&ctx, doc, log.clone(), start, env, Shared { host_fns: host_fns.clone(), net: net.clone(), sys: sys.clone(), observing: observing.clone(), storage: storage.clone() })?;
            ctx.eval::<(), _>(prelude()).catch(&ctx).map_err(|e| {
                eprintln!("[js] prelude failed: {e}");
                rquickjs::Error::Unknown
            })
        })?;
        let host = ScriptHost { extensions: Rc::default(), runtime, context, log, start, host_fns, net, sys, observing, storage };
        host.install_extensions()?;
        Ok(host)
    }

    /// The loader behind `fetch` and `XMLHttpRequest` (by default: files next to the page and
    /// `data:` URLs). It runs on a worker thread per request.
    pub fn set_fetcher(&self, fetcher: Arc<dyn Fetcher>) {
        *self.net.fetcher.borrow_mut() = fetcher;
    }

    /// Keep `localStorage` in this file: read now, written shortly after each change and on drop.
    pub fn set_storage_file(&self, path: impl Into<std::path::PathBuf>) {
        let path = path.into();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let saved = std::fs::read_to_string(&path).ok();
        *self.storage.borrow_mut() = Some(path);
        if let Some(json) = saved {
            self.call_global("__storageLoad", |_, f| f.call::<_, ()>((json.as_str(),)));
        }
    }

    /// Tell `window.location` where the page lives.
    pub fn set_location(&self, url: &str) {
        self.call_global("__setLocation", |_, f| f.call::<_, ()>((url,)));
    }

    /// Some script watches sizes or visibility (`ResizeObserver`, `IntersectionObserver`).
    pub fn observing(&self) -> bool {
        self.observing.get()
    }

    /// Let those observers look at the current layout.
    pub fn check_observers(&self) {
        self.call_global("__observeCheck", |_, f| f.call::<_, ()>(()));
    }

    /// Requests still in flight; the embedder should keep ticking while there are any.
    pub fn fetches_pending(&self) -> bool {
        self.net.pending.get() > 0 || self.sys.pending() || self.extensions.borrow().pending()
    }

    /// Run promise jobs queued by the last call.
    fn drain_jobs(&self) {
        while matches!(self.runtime.execute_pending_job(), Ok(true)) {}
    }

    /// Make `f` callable from scripts as `host.<name>(...args)` (and `host.call(name, ...args)`).
    /// `f` gets the arguments as a JSON array and returns a JSON value, or "" for `undefined`.
    pub fn register_host_fn(&self, name: &str, f: impl Fn(&str) -> String + 'static) {
        self.host_fns.borrow_mut().insert(name.to_string(), Rc::new(f));
    }

    /// Evaluate a script and return its value as text: strings as they are, `undefined` as "",
    /// anything else as JSON.
    pub fn eval(&self, source: &str) -> Result<String, String> {
        let source = jsx::transform(source).unwrap_or_else(|| source.to_string());
        let result = self.context.with(|ctx| {
            let value: rquickjs::Value = ctx.eval(source).catch(&ctx).map_err(|e| e.to_string())?;
            let show: Function = ctx.globals().get("__show").map_err(|e| e.to_string())?;
            show.call::<_, String>((value,)).catch(&ctx).map_err(|e| e.to_string())
        });
        self.drain_jobs();
        result
    }

    /// Call a global (or dotted, like `app.save`) function with a JSON array of arguments and
    /// return its result as JSON ("" for `undefined`).
    pub fn call(&self, name: &str, args_json: &str) -> Result<String, String> {
        let result = self.context.with(|ctx| {
            let invoke: Function = ctx.globals().get("__invoke").map_err(|e| e.to_string())?;
            invoke.call::<_, String>((name, args_json)).catch(&ctx).map_err(|e| e.to_string())
        });
        self.drain_jobs();
        result
    }

    /// Run a classic script. Errors are returned as readable strings.
    pub fn run(&self, source: &str) -> Result<(), String> {
        let source = jsx::transform(source).unwrap_or_else(|| source.to_string());
        let result = self
            .context
            .with(|ctx| ctx.eval::<(), _>(source).catch(&ctx).map_err(|e| e.to_string()));
        self.drain_jobs();
        result
    }

    /// Run an ES module. `name` is its file path (or `page.html#module0` for an
    /// inline module); relative `import`s resolve from its folder.
    pub fn run_module(&self, name: &str, source: &str) -> Result<(), String> {
        let source = with_location(name, jsx::transform(source).unwrap_or_else(|| source.to_string()));
        let result = self.context.with(|ctx| {
            let promise = Module::evaluate(ctx.clone(), name, source).catch(&ctx).map_err(|e| e.to_string())?;
            match promise.finish::<()>().catch(&ctx) {
                // Still waiting on a timer or similar (top-level await): fine.
                Err(rquickjs::CaughtError::Error(rquickjs::Error::WouldBlock)) => Ok(()),
                Err(e) => Err(e.to_string()),
                Ok(()) => Ok(()),
            }
        });
        self.drain_jobs();
        result
    }

    /// Fire `DOMContentLoaded` on the document, then `load` on the window.
    pub fn fire_load(&self) {
        self.call_global("__lifecycle", |_, f| f.call::<_, ()>(()));
    }

    /// Fire an event such as `resize` at the window.
    pub fn fire_window_event(&self, kind: &str) {
        self.call_global("__windowEvent", |_, f| f.call::<_, ()>((kind,)));
    }

    /// Dispatch `event` at `target` with capture and bubble phases. Returns
    /// false if a listener called `preventDefault()`.
    pub fn dispatch(&self, target: NodeId, event: &UiEvent) -> bool {
        let mut proceed = true;
        self.call_global("__dispatch", |ctx, f| {
            let init = Object::new(ctx.clone())?;
            let kind = match event {
                UiEvent::Mouse { kind, x, y, button, mods } => {
                    init.set("clientX", *x)?;
                    init.set("clientY", *y)?;
                    init.set("button", *button)?;
                    init.set("shiftKey", mods & 1 != 0)?;
                    init.set("ctrlKey", mods & 2 != 0)?;
                    init.set("altKey", mods & 4 != 0)?;
                    init.set("metaKey", mods & 8 != 0)?;
                    kind
                }
                UiEvent::Key { kind, key, ctrl, shift, alt, meta } => {
                    init.set("key", key.as_str())?;
                    init.set("ctrlKey", *ctrl)?;
                    init.set("shiftKey", *shift)?;
                    init.set("altKey", *alt)?;
                    init.set("metaKey", *meta)?;
                    kind
                }
                UiEvent::Plain(kind) => kind,
                UiEvent::Animation { kind, name, elapsed } => {
                    init.set(if *kind == "transitionend" { "propertyName" } else { "animationName" }, name.as_str())?;
                    init.set("elapsedTime", *elapsed)?;
                    init.set("pseudoElement", "")?;
                    kind
                }
            };
            proceed = f.call::<_, bool>((*kind, target.0 as i32, init))?;
            Ok(())
        });
        proceed
    }

    /// Run timers that are due. Returns how long until the next one, if any.
    pub fn tick(&self) -> Option<Duration> {
        self.tick_extensions();
        let now = self.start.elapsed().as_secs_f64() * 1000.0;
        let mut next = -1.0f64;
        self.call_global("__tick", |_, f| {
            next = f.call::<_, f64>((now,))?;
            Ok(())
        });
        // Hand finished requests to their promises.
        for (id, result) in self.net.take_done() {
            self.call_global("__fetchDone", |_, f| {
                match &result {
                    Ok(r) => {
                        let headers: Vec<String> = r.headers.iter().flat_map(|(k, v)| [k.clone(), v.clone()]).collect();
                        f.call::<_, ()>((id, r.status, r.status_text.as_str(), r.url.as_str(), headers, net::b64(&r.body), ""))?;
                    }
                    Err(e) => f.call::<_, ()>((id, 0u16, "", "", Vec::<String>::new(), "", e.as_str()))?,
                }
                Ok(())
            });
        }
        for (id, result) in self.sys.take_done() {
            self.call_global("__sysDone", |_, f| {
                match &result {
                    Ok(v) => f.call::<_, ()>((id, true, v.as_str()))?,
                    Err(e) => f.call::<_, ()>((id, false, e.as_str()))?,
                }
                Ok(())
            });
        }
        // Fetches are quick: look again at frame rate. A socket or process can wait for a long time (an idle
        // WebSocket always has a read pending), so those are looked at less often.
        let poll = if self.net.pending.get() > 0 {
            Some(Duration::from_millis(16))
        } else if self.sys.pending() || self.extensions.borrow().pending() {
            Some(Duration::from_millis(30))
        } else {
            None
        };
        // Sounds: notice the ones that ended and let the page know; look again soon while any plays.
        #[cfg(feature = "audio")]
        let (ended, playing) = {
            nile_audio::poll();
            (nile_audio::take_events().into_iter().map(|(id, _)| id as i32).collect::<Vec<i32>>(), nile_audio::busy())
        };
        #[cfg(not(feature = "audio"))]
        let (ended, playing): (Vec<i32>, bool) = (Vec::new(), false);
        if playing || !ended.is_empty() {
            self.call_global("__audioTick", |_, f| {
                f.call::<_, ()>((ended.clone(),))?;
                Ok(())
            });
        }
        let poll = if playing { Some(poll.map_or(Duration::from_millis(100), |p| p.min(Duration::from_millis(100)))) } else { poll };
        let timer = (next >= 0.0).then(|| Duration::from_secs_f64(next / 1000.0));
        match (timer, poll) {
            (Some(t), Some(p)) => Some(t.min(p)),
            (t, p) => t.or(p),
        }
    }

    /// Everything passed to `console.*` so far.
    pub fn console_output(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn call_global(
        &self,
        name: &str,
        f: impl for<'js> FnOnce(&Ctx<'js>, Function<'js>) -> rquickjs::Result<()>,
    ) {
        let result = self.context.with(|ctx| {
            let func: Function = ctx.globals().get(name)?;
            f(&ctx, func)
        });
        if let Err(e) = result {
            // Listener errors are reported inside the prelude; this is a host-side failure.
            self.context.with(|ctx| eprintln!("[js] {name} failed: {}", describe(&ctx, e)));
        }
        self.drain_jobs();
    }
}

fn describe(ctx: &Ctx<'_>, e: rquickjs::Error) -> String {
    if matches!(e, rquickjs::Error::Exception) {
        let ex = ctx.catch();
        ex.into_exception().map(|x| x.to_string()).unwrap_or_else(|| "exception".into())
    } else {
        e.to_string()
    }
}

#[cfg(test)]
mod tests;
