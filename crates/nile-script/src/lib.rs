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
mod envmod;
mod jsx;
mod net;
mod sys;

pub use net::{file_url, path_of_url, FetchRequest, FetchResponse, Fetcher, LocalFetcher};

const EDIT: &str = include_str!("edit.js");
const WEB: &str = include_str!("web.js");
const CANVAS: &str = include_str!("canvas.js");

fn prelude() -> String {
    include_str!("prelude.js").replace("/*@EDIT@*/", EDIT).replace("/*@WEB@*/", &WEB.replace("/*@CANVAS@*/", CANVAS))
}

/// Window ids are unique within the process (the main window, then each `new Window()`).
static NEXT_WINDOW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

thread_local! {
    /// `Window.share`: JSON text by key, shared by the windows of one thread.
    static SHARE: RefCell<std::collections::HashMap<String, String>> = RefCell::default();
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
    /// Close, move, resize, ... the host window. Embedders without a window ignore it.
    fn window_command(&self, _cmd: WindowCommand) {}
    /// Show the popup element `node` in an OS window of its own (`rect`: its border box in viewport
    /// coordinates), or take it back.
    fn portal_set(&self, _node: NodeId, _rect: [f32; 4]) {}
    /// The `<menu>` element chosen as the application menu bar, or none.
    fn set_app_menu(&self, _node: Option<NodeId>) {}
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
        if matches!(name, "@nile" | "@sciter" | "@env" | "@sys" | "@debug" | "@storage") {
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
                "const m = globalThis.__sciterModule;\nexport const { VERSION, REVISION, QUICKJS_VERSION, setModuleUrlResolver, loadLibrary, parseValue, devicePixels, on, off, once, onGlobalEvent, offGlobalEvent, $, $$, uuid, encode, decode, compress, decompress, toBase64, fromBase64, md5, crc32 } = m;\nconst imp = m.import;\nexport { imp as import };\nexport default m;",
            ),
            "@debug" => Some("const m = globalThis.__debugModule;\nexport const { setUnhandledExceptionHandler, setConsoleOutputHandler, callStackAt, setResourceArrivalHandler, setBreakpointHandler, setBreakpoints, getElementByUID, getUIDofElement, highlightElement, getStyleRulesOfElement, containerId, objectKind, sublimatedValue, sublimatedValueElements, frameVariables } = m;\nexport default m;"),
            "@storage" => Some("const m = globalThis.__storageModule;\nexport const open = m.open;\nexport default m;"),
            "@env" => Some(
                "const m = globalThis.__envModule;\nexport const { language, country, userName, machineName, domainName, arguments: args, launch, home, homeURL, path, pathURL, drives, variable, exec, DEVICE } = m;\nexport { args as arguments };\nexport const OS = m.OS, PLATFORM = m.PLATFORM;\nexport default m;",
            ),
            _ => None,
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
    /// `click`, `mousedown`, `mouseup`, `mousemove`, `mouseover`, `mouseout`.
    Mouse { kind: &'static str, x: f32, y: f32, button: i32 },
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
    native!("menuSet", {
        let env = env.clone();
        move |id: i32| env.set_app_menu(node(id))
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
                UiEvent::Mouse { kind, x, y, button } => {
                    init.set("clientX", *x)?;
                    init.set("clientY", *y)?;
                    init.set("button", *button)?;
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
mod tests {
    use super::*;

    /// Every element is a 100x20 box stacked at y = 20 * id, scrolled by 5.
    struct Fake(RefCell<f32>);

    impl Environment for Fake {
        fn bounding_rect(&self, node: NodeId) -> Option<[f32; 4]> {
            (node.0 != 0).then(|| [10.0, 20.0 * node.0 as f32 - *self.0.borrow(), 100.0, 20.0])
        }
        fn computed_style(&self, _: NodeId, p: &str) -> Option<String> {
            (p == "color").then(|| "rgb(1, 2, 3)".into())
        }
        fn viewport(&self) -> (f32, f32) {
            (640.0, 480.0)
        }
        fn scroll_y(&self) -> f32 {
            *self.0.borrow()
        }
        fn scroll_to(&self, y: f32) {
            *self.0.borrow_mut() = y;
        }
        fn interaction(&self) -> InteractionState {
            InteractionState::default()
        }
        fn set_focus(&self, _: Option<NodeId>) {}
        fn element_scroll(&self, _: NodeId) -> Option<[f32; 4]> {
            None
        }
        fn scroll_element_to(&self, _: NodeId, _: f32, _: f32) {}
        fn selection(&self, _: NodeId) -> (usize, usize) {
            (0, 0)
        }
        fn set_selection(&self, _: NodeId, _: usize, _: usize) {}
    }

    fn host(html: &str) -> (ScriptHost, Rc<RefCell<Document>>) {
        let doc = Rc::new(RefCell::new(Document::parse(html)));
        (ScriptHost::new(doc.clone(), Rc::new(Fake(RefCell::new(0.0)))).unwrap(), doc)
    }

    #[test]
    fn host_functions_eval_and_call_round_trip_json() {
        let (host, _) = host("");
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        host.register_host_fn("add", move |args| {
            log.borrow_mut().push(args.to_string());
            let v: Vec<f64> = args.trim_matches(|c| c == '[' || c == ']').split(',').filter_map(|n| n.trim().parse().ok()).collect();
            format!("{}", v.iter().sum::<f64>())
        });
        host.register_host_fn("nothing", |_| String::new());
        assert_eq!(host.eval("host.add(1, 2, 3.5)").unwrap(), "6.5");
        assert_eq!(host.eval("host.call('add', 4, 5)").unwrap(), "9");
        assert_eq!(*seen.borrow(), ["[1,2,3.5]", "[4,5]"]);
        assert_eq!(host.eval("host.nothing()").unwrap(), "", "empty result is undefined");
        assert_eq!(host.eval("[host.has('add'), host.has('x')]").unwrap(), "[true,false]");
        assert!(host.eval("host.missing(1)").unwrap_err().contains("not registered"));
        // eval: strings raw, objects as JSON.
        assert_eq!(host.eval("'hi'").unwrap(), "hi");
        assert_eq!(host.eval("({a: [1, 2]})").unwrap(), r#"{"a":[1,2]}"#);
        assert_eq!(host.eval("undefined").unwrap(), "");
        assert!(host.eval("(").is_err());
        // The host calls into the page.
        host.run("globalThis.app = { greet(n, x) { return { hello: n, x }; } }; function twice(n) { return n * 2 }").unwrap();
        assert_eq!(host.call("twice", "[21]").unwrap(), "42");
        assert_eq!(host.call("app.greet", r#"["Ada", [1]]"#).unwrap(), r#"{"hello":"Ada","x":[1]}"#);
        assert!(host.call("nope", "[]").unwrap_err().contains("not a function"));
    }

    #[test]
    fn console_log_and_errors() {
        let (host, _) = host("");
        host.run("console.log('sum', 1 + 2)").unwrap();
        assert_eq!(host.console_output(), vec!["sum 3".to_string()]);
        assert!(host.run("throw new Error('boom')").unwrap_err().contains("boom"));
    }

    #[test]
    fn queries_and_mutation_reach_the_document() {
        let (host, doc) = host(r#"<ul id="l"><li class="a">one</li><li>two</li></ul>"#);
        let v0 = doc.borrow().version;
        host.run(
            r#"
            const ul = document.getElementById('l');
            console.log(ul.children.length, ul.querySelectorAll('li.a').length, ul.firstElementChild.textContent);
            const li = document.createElement('LI');
            li.textContent = 'three';
            li.classList.add('x', 'y');
            li.style.backgroundColor = 'red';
            li.dataset.fooBar = '1';
            ul.appendChild(li);
            console.log(ul.lastElementChild === li, li.tagName, li.className, li.getAttribute('style'),
                        li.getAttribute('data-foo-bar'), li.matches('ul > li.x'), li.closest('#l') === ul);
            console.log(ul.innerHTML);
            "#,
        )
        .unwrap();
        let out = host.console_output();
        assert_eq!(out[0], "2 1 one");
        assert_eq!(out[1], "true LI x y background-color: red 1 true true");
        assert_eq!(
            out[2],
            r#"<li class="a">one</li><li>two</li><li class="x y" style="background-color: red" data-foo-bar="1">three</li>"#
        );
        assert!(doc.borrow().version > v0);
    }

    #[test]
    fn web_platform_helpers_work() {
        let (host, _) = host(r#"<ul id="l"><li>a</li></ul>"#);
        host.run(
            r#"
            console.log(btoa('Hello, World!'), atob('SGVsbG8sIFdvcmxkIQ=='), btoa('ab'), atob('YWI'));
            const bytes = new TextEncoder().encode('héllo €😀');
            console.log(bytes.length, new TextDecoder().decode(bytes));
            const u = new URL('../x/y.html?a=1&b=two%20words#top', 'https://user@Example.com:8080/p/q/r.html');
            console.log(u.href, u.origin, u.pathname, u.searchParams.get('b'), u.hash, u.host);
            u.searchParams.set('c', 'a b'); u.searchParams.append('a', '2');
            console.log(u.search, u.searchParams.getAll('a').join('|'));
            console.log(new URLSearchParams({ q: 'x y', n: 1 }).toString());
            const o = { d: new Date(5), m: new Map([[1, { z: [1, 2] }]]), list: [1, { k: 2 }] };
            o.self = o;
            const c = structuredClone(o);
            console.log(c !== o, c.self === c, c.d.getTime(), c.m.get(1).z.length, c.list[1].k);
            localStorage.setItem('k', 'v'); localStorage.n = 5;
            console.log(localStorage.getItem('k'), localStorage.n, localStorage.length, sessionStorage.length);
            console.log(matchMedia('(min-width: 1px)').matches, matchMedia('(max-width: 1px)').matches, matchMedia('(prefers-color-scheme: light)').matches);
            window.seen = [];
            const ul = document.getElementById('l');
            new MutationObserver((recs) => { for (const r of recs) seen.push(r.type + ':' + (r.attributeName || r.addedNodes.length + '/' + r.removedNodes.length)); })
              .observe(ul, { childList: true, attributes: true, subtree: true });
            const li = document.createElement('li');
            ul.appendChild(li);
            ul.setAttribute('data-x', '1');
            li.setAttribute('class', 'new');
            ul.removeChild(li);
            "#,
        )
        .unwrap();
        host.run("console.log(seen.join(','))").unwrap();
        let out = host.console_output();
        assert_eq!(out[0], "SGVsbG8sIFdvcmxkIQ== Hello, World! YWI= ab");
        assert_eq!(out[1], "14 héllo €😀");
        assert_eq!(out[2], "https://user@example.com:8080/p/x/y.html?a=1&b=two%20words#top https://example.com:8080 /p/x/y.html two words #top example.com:8080");
        assert_eq!(out[3], "?a=1&b=two+words&c=a+b&a=2 1|2");
        assert_eq!(out[4], "q=x+y&n=1");
        assert_eq!(out[5], "true true 5 2 2");
        assert_eq!(out[6], "v 5 2 0");
        assert_eq!(out[7], "true false true");
        assert_eq!(out[8], "childList:1/0,attributes:data-x,attributes:class,childList:0/1");
    }

    #[test]
    fn fetch_and_xhr_load_local_files_and_data_urls() {
        let dir = std::env::temp_dir().join(format!("nile-fetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("d.json"), r#"{"n":7,"s":"héllo"}"#).unwrap();
        let (host, _) = host("<p></p>");
        host.set_fetcher(Arc::new(LocalFetcher::new(&dir)));
        host.run(
            r#"
            window.out = [];
            fetch('d.json').then(r => { out.push(r.status + ' ' + r.ok + ' ' + r.headers.get('content-type')); return r.json(); })
              .then(j => out.push(j.n + j.s));
            fetch('nope.txt').then(r => out.push(r.status + ' ' + r.ok));
            fetch('data:text/plain;base64,SGk=').then(r => r.text()).then(t => out.push(t));
            fetch('https://example.com/').catch(e => out.push('rejected'));
            const x = new XMLHttpRequest();
            x.open('GET', 'd.json'); x.responseType = 'json';
            x.onload = () => out.push('xhr ' + x.status + ' ' + x.response.n);
            x.send();
            const ac = new AbortController();
            fetch('d.json', { signal: ac.signal }).catch(e => out.push('aborted'));
            ac.abort();
            "#,
        )
        .unwrap();
        for _ in 0..200 {
            host.tick();
            if !host.fetches_pending() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        host.tick();
        host.run("console.log(out.slice().sort().join('|'))").unwrap();
        let out = host.console_output();
        assert_eq!(out[0], "200 true application/json|404 false|7héllo|Hi|aborted|rejected|xhr 200 7");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn local_storage_persists_to_the_chosen_file() {
        let file = std::env::temp_dir().join(format!("nile-storage-{}", std::process::id())).join("ls.json");
        let _ = std::fs::remove_file(&file);
        {
            let (host, _) = host("<p></p>");
            host.set_storage_file(&file);
            host.run("localStorage.setItem('name', 'Nile'); localStorage.n = 2; localStorage.removeItem('n'); localStorage.k = 'é\"x'").unwrap();
        } // dropping flushes
        assert_eq!(std::fs::read_to_string(&file).unwrap().replace(' ', ""), r#"{"name":"Nile","k":"é\"x"}"#);
        let (host, _) = host("<p></p>");
        host.set_storage_file(&file);
        host.run("console.log(localStorage.name, localStorage.length, localStorage.getItem('k'))").unwrap();
        assert_eq!(host.console_output(), vec!["Nile 2 é\"x"]);
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn sciter_and_env_modules_load() {
        let (host, _) = host(r#"<p id="p" class="a">x</p>"#);
        host.run_module(
            "page.html#module1",
            r##"
            import { $, $$, md5, crc32, toBase64, fromBase64, uuid, encode, decode, parseValue, devicePixels, VERSION } from "@nile";
            import * as env from "@env";
            import sctr, { on } from "@sciter";
            console.log($("#p").className, $$("p").length, typeof VERSION, typeof on, sctr.crc32 === crc32);
            console.log(md5("hello"), crc32("hello"), toBase64(encode("Nile")), decode(fromBase64("TmlsZQ==")), uuid().length);
            console.log(JSON.stringify(parseValue("{foo-bar: zap; n: 0xFF, list: [1, 2n], ok: true; s: 'q'}", 0), (k, v) => typeof v === "bigint" ? v + "n" : v));
            console.log(devicePixels("1in"), devicePixels(10));
            console.log(["OSX", "Windows", "Linux"].includes(env.PLATFORM) || env.PLATFORM, env.DEVICE, env.language().length, env.path("home", "x").endsWith("x"), Array.isArray(env.arguments()), env.drives().length > 0);
            env.variable("NILE_MODULE_TEST", "7");
            console.log(env.variable("NILE_MODULE_TEST"));
            env.variable("NILE_MODULE_TEST", null);
            console.log(JSON.stringify(env.variable("NILE_MODULE_TEST")));
            "##,
        )
        .unwrap();
        let out = host.console_output();
        assert_eq!(out[0], "a 1 string function true");
        assert_eq!(out[1], "5d41402abc4b2a76b9719d911017c592 907060870 TmlsZQ== Nile 36");
        assert_eq!(out[2], r#"{"foo-bar":"zap","n":255,"list":[1,"2n"],"ok":true,"s":"q"}"#);
        assert_eq!(out[3], "96 10");
        assert!(out[4].starts_with("true desktop 2 true true true"), "{}", out[4]);
        assert_eq!(out[5], "7");
        assert_eq!(out[6], "\"\"");
    }

    #[test]
    fn missing_attributes_are_null() {
        let (host, _) = host("<p id='p'></p>");
        host.run("const p = document.getElementById('p'); console.log(p.getAttribute('nope') === null, p.hasAttribute('id'), p.hasAttribute('nope'))").unwrap();
        assert_eq!(host.console_output(), vec!["true true false"]);
    }

    #[test]
    fn selector_errors_throw() {
        let (host, _) = host("<p></p>");
        host.run("try { document.querySelector('p::first-line') } catch (e) { console.log(String(e).includes('SyntaxError')) }")
            .unwrap();
        assert_eq!(host.console_output(), vec!["true"]);
    }

    #[test]
    fn events_capture_and_bubble_in_order() {
        let (host, doc) = host(r#"<div id="outer"><button id="inner">x</button></div>"#);
        host.run(
            r#"
            const log = [];
            const outer = document.getElementById('outer'), inner = document.getElementById('inner');
            outer.addEventListener('click', () => log.push('outer-capture'), true);
            outer.addEventListener('click', () => log.push('outer-bubble'));
            inner.addEventListener('click', (e) => { log.push('inner:' + e.target.id + ':' + e.currentTarget.id); });
            document.addEventListener('click', () => log.push('document'));
            window.addEventListener('click', () => log.push('window'));
            inner.onclick = () => log.push('onclick');
            globalThis.__log = log;
            "#,
        )
        .unwrap();
        let inner = doc.borrow().find(|n| n.attr("id") == Some("inner")).unwrap();
        let ev = UiEvent::Mouse { kind: "click", x: 1.0, y: 2.0, button: 0 };
        assert!(host.dispatch(inner, &ev));
        host.run("console.log(__log.join(','))").unwrap();
        assert_eq!(
            host.console_output()[0],
            "outer-capture,onclick,inner:inner:inner,outer-bubble,document,window"
        );
    }

    #[test]
    fn stop_propagation_prevent_default_and_once() {
        let (host, doc) = host(r##"<div id="o"><a id="i" href="#">x</a></div>"##);
        host.run(
            r#"
            const out = [];
            const o = document.getElementById('o'), i = document.getElementById('i');
            o.addEventListener('click', () => out.push('o'));
            i.addEventListener('click', (e) => { e.preventDefault(); e.stopPropagation(); out.push('i'); }, { once: true });
            globalThis.__out = out;
            "#,
        )
        .unwrap();
        let i = doc.borrow().find(|n| n.attr("id") == Some("i")).unwrap();
        let ev = UiEvent::Mouse { kind: "click", x: 0.0, y: 0.0, button: 0 };
        assert!(!host.dispatch(i, &ev), "preventDefault reports false");
        assert!(host.dispatch(i, &ev), "once listener is gone; click bubbles to o");
        host.run("console.log(__out.join(','))").unwrap();
        assert_eq!(host.console_output()[0], "i,o");
    }

    #[test]
    fn attribute_handlers_and_key_events() {
        let (host, doc) = host(r#"<input id="t" onkeydown="console.log('key:' + event.key + event.ctrlKey)">"#);
        let t = doc.borrow().find(|n| n.attr("id") == Some("t")).unwrap();
        host.dispatch(t, &UiEvent::Key { kind: "keydown", key: "a".into(), ctrl: true, shift: false, alt: false, meta: false });
        assert_eq!(host.console_output(), vec!["key:atrue"]);
    }

    #[test]
    fn timers_and_promises() {
        let (host, _) = host("");
        host.run(
            r#"
            setTimeout((a) => console.log('t', a), 0, 'x');
            const id = setInterval(() => console.log('i'), 1);
            Promise.resolve().then(() => console.log('micro'));
            clearTimeout(setTimeout(() => console.log('never'), 0));
            "#,
        )
        .unwrap();
        assert_eq!(host.console_output(), vec!["micro"]);
        std::thread::sleep(Duration::from_millis(5));
        assert!(host.tick().is_some(), "the interval keeps the loop alive");
        let out = host.console_output();
        assert!(out.contains(&"t x".to_string()) && out.contains(&"i".to_string()));
        assert!(!out.contains(&"never".to_string()));
    }

    #[test]
    fn layout_queries_and_scrolling() {
        let (host, doc) = host(r#"<div id="a"><p id="b" style="border: 2px solid red">x</p></div>"#);
        let b = doc.borrow().find(|n| n.attr("id") == Some("b")).unwrap().0;
        host.run(
            r#"
            const el = document.getElementById('b');
            const r = el.getBoundingClientRect();
            console.log(r.left, r.top, r.width, r.height, r.right, r.bottom, el.offsetWidth, el.offsetHeight);
            console.log(getComputedStyle(el).color, getComputedStyle(el).getPropertyValue('color'), getComputedStyle(el).fontSize === '');
            console.log(innerWidth, innerHeight, scrollY);
            scrollTo(0, 30);
            console.log(scrollY, el.getBoundingClientRect().top, document.documentElement.scrollTop);
            console.log(document.createElement('div').getBoundingClientRect().width);
            "#,
        )
        .unwrap();
        let out = host.console_output();
        assert_eq!(out[0], format!("10 {} 100 20 110 {} 100 20", 20 * b, 20 * b + 20));
        assert_eq!(out[1], "rgb(1, 2, 3) rgb(1, 2, 3) true");
        assert_eq!(out[2], "640 480 0");
        assert_eq!(out[3], format!("30 {} 30", 20 * b - 30));
        assert_eq!(out[4], "100");
    }

    #[test]
    fn jsx_builds_dom_in_classic_scripts() {
        let (host, doc) = host(r#"<div id="root"></div>"#);
        host.run(
            r#"
            const Card = (props, kids) => <section class="card" data-n={props.n}>{kids}</section>;
            let clicked = 0;
            const items = ['a', 'b'];
            const view = (
              <Card n={2}>
                <h1 style={{ color: 'red', fontSize: '20px' }} onclick={() => clicked++}>Title &amp; more</h1>
                <ul class={['x', false, 'y']}>{items.map((t, i) => <li key={i} hidden={i === 1}>{t}</li>)}</ul>
                <>frag</>
                {null}{false}{3}
              </Card>
            );
            document.getElementById('root').append(view);
            document.querySelector('h1').click();
            console.log(clicked, document.getElementById('root').innerHTML);
            "#,
        )
        .unwrap();
        let _ = doc;
        assert_eq!(
            host.console_output()[0],
            r#"1 <section class="card" data-n="2"><h1 style="color: red; font-size: 20px">Title &amp; more</h1><ul class="x y"><li>a</li><li hidden="">b</li></ul>frag3</section>"#
        );
    }

    #[test]
    fn modules_import_relative_files_with_jsx() {
        let dir = std::env::temp_dir().join(format!("nile-mod-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(dir.join("lib/util.js"), "export const twice = (n) => n * 2;\nexport default 'dflt';").unwrap();
        std::fs::write(
            dir.join("lib/view.jsx"),
            "import { twice } from './util.js';\nexport const View = ({ n }) => <b>{twice(n)}</b>;",
        )
        .unwrap();
        let (host, _) = host(r#"<div id="root"></div>"#);
        let page = dir.join("page.html");
        host.run_module(
            &format!("{}#module0", page.display()),
            r#"
            import dflt, { twice } from './lib/util.js';
            import { View } from './lib/view.jsx';
            document.getElementById('root').append(<View n={21} />);
            console.log(dflt, twice(4), document.getElementById('root').innerHTML);
            "#,
        )
        .unwrap();
        assert_eq!(host.console_output(), vec!["dflt 8 <b>42</b>"]);
        // Errors surface as messages.
        let err = host.run_module(&format!("{}#module1", page.display()), "import './missing.js';").unwrap_err();
        assert!(err.contains("missing.js"), "{err}");
        let err = host.run_module(&format!("{}#module2", page.display()), "import 'bare';").unwrap_err();
        assert!(err.contains("bare"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn jsx_compiles_to_vnodes_like_sciter() {
        let (host, _) = host("");
        host.run(
            r#"
            const v = <h1 id="hw">Hello</h1>;
            console.log(Array.isArray(v), v[0], JSON.stringify(v[1]), JSON.stringify(v[2]), JSON.stringify(<br/>));
            "#,
        )
        .unwrap();
        assert_eq!(host.console_output(), vec![r#"true h1 {"id":"hw"} ["Hello"] ["br",null,null]"#]);
    }

    #[test]
    fn sciter_selectors_and_delegation() {
        let (host, _) = host(r#"<ul id="l"><li class="a"><b>1</b></li><li class="a">2</li></ul>"#);
        host.run(
            r#"
            const l = $('#l');
            console.log($$('li').length, l.$$('.a').length, l.$('b').textContent, l.children[1].elementIndex);

            // this = the element handler is attached to; second arg = matched descendant.
            const seen = [];
            l.on('click', 'li', function (evt, li) { seen.push(this.id + ':' + li.textContent + ':' + (li === evt.target.closest('li'))); });
            $$('li')[0].$('b').click();     // bubbles from <b>, delegated to the li
            l.click();                       // not inside an li: ignored

            // namespaces, off by name / namespace / function, once, capture
            const hits = [];
            const h = () => hits.push('h');
            l.on('click.one', h).on('click.two', () => hits.push('two')).once('click', () => hits.push('once'));
            l.click(); l.click();
            l.off('.two'); l.click();
            l.off(h); l.click();
            l.off('click'); l.click();
            const order = [];
            l.on('^click', () => order.push('capture'));
            l.on('click', () => order.push('bubble'));
            $$('li')[1].click();
            console.log(seen.join(','));
            console.log(hits.join(','));
            console.log(order.join(','));
            "#,
        )
        .unwrap();
        let out = host.console_output();
        assert_eq!(out[0], "2 2 1 1");
        assert_eq!(out[1], "l:1:true");
        // 1st click: h,two,once; 2nd: h,two; after off('.two'): h; after off(h): nothing; then off('click').
        assert_eq!(out[2], "h,two,once,h,two,h");
        assert_eq!(out[3], "capture,bubble");
    }

    #[test]
    fn element_state_flags_and_attributes() {
        let (host, _) = host(
            r#"<style>#i:checked { color: red } </style><input id="i" type="checkbox" title="t"><p id="p"></p>"#,
        );
        host.run(
            r#"
            const i = $('#i');
            console.log(i.state.checked, i.state.disabled, i.state.hover, 'checked' in i.state);
            i.state.checked = true;
            console.log(i.state.checked, i.hasAttribute('checked'), i.matches(':checked'), $$('input:checked').length);
            i.state.checked = false;
            console.log($$(':checked').length);
            const names = [...i.attributes].map((a) => a.name + '=' + a.value).join(',');
            i.attributes.title = 'new';
            console.log(names, i.getAttribute('title'), i.attributes.length, i.attributes.type, i.attributes.nope);
            console.log(i.value, $('#p').nodeIndex, $('#p').insertAfter(document.createElement('hr'), $('#p')).tagName, $('#p').insert);
            "#,
        )
        .unwrap();
        let out = host.console_output();
        assert_eq!(out[0], "false false false true");
        assert_eq!(out[1], "true false true 1");
        assert_eq!(out[2], "0");
        assert_eq!(out[3], "id=i,type=checkbox,title=t new 3 checkbox undefined");
        assert_eq!(out[4], "on 1 HR undefined");
    }

    #[test]
    fn content_prepend_and_patch() {
        let (host, doc) = host(r#"<div id="root"><p id="keep" class="old" title="gone">old</p>stray</div>"#);
        host.run(
            r#"
            const root = $('#root');
            const keep = $('#keep');
            const ids = [];
            root.patch(
              <div>
                <p id="keep" class="new" onclick={() => ids.push('clicked')}>fresh</p>
                <ul><li key="a">A</li><li key="b">B</li></ul>
                {['x', 'y']}
              </div>
            );
            console.log(keep === $('#keep'), keep.className, keep.hasAttribute('title'), root.innerHTML);
            keep.click();
            const a = $('li');
            // Re-patch with the items swapped: keyed DOM is reused, not rebuilt.
            root.patch(<div><p id="keep" class="new">fresh</p><ul><li key="b">B</li><li key="a">A</li></ul></div>);
            console.log(ids.join(), $('li') !== a, $$('li')[1] === a, root.innerHTML);
            root.prepend(<hr/>, 'text');
            console.log(root.firstChild.tagName, root.childNodes[1].data, root.children.length);
            root.content(<i>only</i>);
            console.log(root.innerHTML);
            "#,
        )
        .unwrap();
        let _ = doc;
        let out = host.console_output();
        assert_eq!(
            out[0],
            r#"true new false <p id="keep" class="new">fresh</p><ul><li>A</li><li>B</li></ul>xy"#
        );
        assert_eq!(out[1], r#"clicked true true <p id="keep" class="new">fresh</p><ul><li>B</li><li>A</li></ul>"#);
        assert_eq!(out[2], "HR text 3");
        assert_eq!(out[3], "<i>only</i>");
    }

    #[test]
    fn form_control_properties() {
        let (host, _) = host(
            r#"<input id="t" value="hi" placeholder="p"><input id="c" type="checkbox" value="v"><input id="d" disabled><button id="b">x</button><div id="div"></div>"#,
        );
        host.run(
            r#"
            const t = $('#t'), c = $('#c'), d = $('#d');
            console.log(t.value, t.placeholder, t.type, c.type, c.checked, d.disabled, $('#b').disabled, $('#div').value);
            t.value = 'new'; c.checked = true; d.disabled = false; $('#b').disabled = true; t.name = 'n';
            console.log(t.getAttribute('value'), c.checked, c.state.checked, d.disabled, d.hasAttribute('disabled'), $('#b').disabled, t.name);
            c.checked = false;
            const div = $('#div'); div.value = 5;
            console.log(c.checked, div.value, document.activeElement === null || document.activeElement === document.body);
            "#,
        )
        .unwrap();
        let out = host.console_output();
        assert_eq!(out[0], "hi p text checkbox false true false undefined");
        assert_eq!(out[1], "new true true false false true n");
        assert_eq!(out[2], "false 5 true");
    }

    #[test]
    fn select_and_option_properties() {
        let (host, _) = host(
            r#"<select id="s"><option value="r">Red</option><option disabled>Gone</option><option selected> Big   Blue </option></select>
               <select id="d"><option disabled>x</option><option value="y">Why</option></select>"#,
        );
        host.run(
            r#"
            const s = $('#s'), d = $('#d');
            console.log(s.options.length, s.length, s.selectedIndex, s.value, s.options[2].text, s.options[2].selected, s.options[0].selected);
            s.value = 'r';
            console.log(s.selectedIndex, s.options[0].selected, s.options[2].selected, s.options[2].hasAttribute('selected'), s.value);
            s.options[2].selected = true;
            console.log(s.selectedIndex, s.value);
            s.selectedIndex = 0;
            console.log(s.value, s.options[1].disabled, d.selectedIndex, d.value, $('#s').options[0].label);
            s.value = 'nope';
            console.log(s.value === 'r' || s.selectedIndex, document.createElement('p').options);
            "#,
        )
        .unwrap();
        let out = host.console_output();
        assert_eq!(out[0], "3 3 2 Big Blue Big Blue true false");
        assert_eq!(out[1], "0 true false false r");
        assert_eq!(out[2], "2 Big Blue");
        assert_eq!(out[3], "r true 1 y Red");
        // An unknown value selects nothing; the first enabled option shows.
        assert_eq!(out[4], "true undefined");
    }

    #[test]
    fn lifecycle_events_fire() {
        let (host, _) = host("<p></p>");
        host.run(
            "document.addEventListener('DOMContentLoaded', () => console.log('dcl ' + document.readyState));\
             window.addEventListener('load', () => console.log('load ' + document.readyState));",
        )
        .unwrap();
        host.fire_load();
        assert_eq!(host.console_output(), vec!["dcl interactive", "load complete"]);
    }
}
