//! The Nile engine without a window: parse a page, run its scripts, feed it input, and paint it
//! into a pixel buffer. The `nile` shell wraps it in a winit window and `nile-capi` exposes it as a
//! C library. Everything here is single-threaded.
//!
//! `Page` is the entry point. Its fields and methods are public for those two embedders; most of
//! them are internal plumbing, so prefer the C API (`nile-capi`) for anything else.

pub mod editing;
mod net;
pub mod window;

use std::{cell::RefCell, path::PathBuf, rc::Rc, time::{Duration, Instant}};

use nile_dom::{Document, NodeId, ScriptSource};
use nile_layout::{DomPoint, TextPoint, FileImages, LayoutOptions, LayoutTree, MenuRequest, ScrollOffsets};
use nile_render::{Overlay, Renderer};
use nile_script::{Environment, ScriptHost, UiEvent, WindowCommand};
use nile_style::{compute_styles_in, Animator, InteractionState, StyleMap};
use nile_text::TextEngine;
use skia_safe::{surfaces, AlphaType, Color, ColorType, ImageInfo};
use window::{Frame, Role, WindowConfig};

/// Keyboard modifiers held during a key or click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModifiersState(u8);

impl ModifiersState {
    pub const SHIFT: Self = Self(1);
    pub const CONTROL: Self = Self(2);
    pub const ALT: Self = Self(4);
    pub const SUPER: Self = Self(8);

    pub fn from_bits(bits: u8) -> Self {
        Self(bits & 15)
    }
    pub fn shift_key(self) -> bool {
        self.0 & 1 != 0
    }
    pub fn control_key(self) -> bool {
        self.0 & 2 != 0
    }
    pub fn alt_key(self) -> bool {
        self.0 & 4 != 0
    }
    pub fn super_key(self) -> bool {
        self.0 & 8 != 0
    }
}

impl std::ops::BitOr for ModifiersState {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

impl std::ops::BitOrAssign for ModifiersState {
    fn bitor_assign(&mut self, o: Self) {
        self.0 |= o.0;
    }
}

/// Text clipboard. The system one in the window; an in-memory one for headless
/// runs and tests, so they never touch the user's clipboard.
pub trait Clipboard {
    fn text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: &str);
}

#[derive(Default)]
pub struct MemoryClipboard(Option<String>);

impl Clipboard for MemoryClipboard {
    fn text(&mut self) -> Option<String> {
        self.0.clone()
    }
    fn set_text(&mut self, text: &str) {
        self.0 = Some(text.to_string());
    }
}

/// The OS clipboard, opened on first use (it can fail, e.g. without a display).
#[cfg(feature = "system-clipboard")]
#[derive(Default)]
pub struct SystemClipboard(Option<arboard::Clipboard>);

#[cfg(feature = "system-clipboard")]
impl SystemClipboard {
    fn get(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.0.is_none() {
            self.0 = arboard::Clipboard::new().ok();
        }
        self.0.as_mut()
    }
}

#[cfg(feature = "system-clipboard")]
impl Clipboard for SystemClipboard {
    fn text(&mut self) -> Option<String> {
        self.get()?.get_text().ok()
    }
    fn set_text(&mut self, text: &str) {
        if let Some(c) = self.get() {
            let _ = c.set_text(text.to_string());
        }
    }
}

/// Half the caret's blink period.
pub const CARET_BLINK: Duration = Duration::from_millis(530);

/// Style, layout and scroll state. Shared (`Rc<RefCell<..>>`) between the page
/// and the script host, so scripts can read layout results. Never hold a borrow
/// across a call into scripts.
pub struct View {
    pub doc: Rc<RefCell<Document>>,
    /// `doc.version` that `styles` were computed for.
    pub seen_version: u64,
    /// What the styles are right now: `base` with transitions and animations applied.
    pub styles: StyleMap,
    /// The cascaded target styles, kept while the page animates.
    pub base: Option<StyleMap>,
    pub animator: Animator,
    /// Time zero of transitions and animations.
    pub clock: Instant,
    /// Device pixels per logical pixel of the last paint (`devicePixelRatio`).
    pub scale: f32,
    pub text: TextEngine,
    pub layout: Option<(f32, f32, LayoutTree)>,
    /// Logical viewport size, so hit testing and scripts can lay out on demand.
    pub viewport: (f32, f32),
    pub scroll_y: f32,
    /// A script moved the scroll position; the window needs repainting.
    pub scroll_dirty: bool,
    /// Scroll offsets of `overflow` elements.
    pub scroll_offsets: ScrollOffsets,
    /// Elements scrolled since the page last looked, to fire `scroll` events.
    pub scrolled: Vec<NodeId>,
    pub interaction: InteractionState,
    /// Text selected on the page itself (not in an input).
    pub page_sel: Option<PageSel>,
    /// The caret and selection inside `contenteditable` content: (anchor, focus).
    pub edit_sel: Option<(DomPoint, DomPoint)>,
    /// `copy`/`cut`/`paste` asked for by scripts, for the page to do with its clipboard.
    pub clipboard_requests: Vec<String>,
    /// Where `<img>` and `url()` backgrounds are read from: the page's folder.
    pub images: FileImages,
    /// The open `<select>` drop-down and its highlighted row.
    pub menu: Option<MenuRequest>,
    /// Selection in the focused text input, in characters of its value: the fixed
    /// end and the moving end (the caret).
    pub anchor: usize,
    pub caret: usize,
    /// Selections scripts set on inputs that don't have focus yet.
    pub pending_sel: std::collections::HashMap<NodeId, (usize, usize)>,
    /// Focus moves made by scripts, so the page can fire `blur`/`focus` for them.
    pub focus_changes: Vec<(Option<NodeId>, Option<NodeId>)>,
    /// Window actions scripts and `role="window-*"` elements asked for; the window loop applies them.
    pub window_requests: Vec<WindowCommand>,
    /// The viewport changed since `resize` last fired.
    pub resized: bool,
    /// Where the window is: `[screenX, screenY, outerWidth, outerHeight, fullscreen]`, kept
    /// current by the window loop. `None` in headless runs.
    pub window_rect: Option<[f32; 5]>,
}

/// A selection over the page's text, valid for one version of the document.
#[derive(Clone, Copy)]
pub struct PageSel {
    anchor: TextPoint,
    focus: TextPoint,
    version: u64,
}

impl View {
    /// The selected text runs and byte ranges, if the selection is still current.
    pub fn page_ranges(&mut self) -> Vec<(usize, usize, usize)> {
        if let Some((a, f)) = self.edit_sel {
            if a == f {
                return Vec::new();
            }
            let doc = self.doc.clone();
            let doc = doc.borrow();
            let tree = self.layout();
            return match (tree.text_point_of(&doc, a), tree.text_point_of(&doc, f)) {
                (Some(a), Some(f)) if a != f => tree.selection_ranges(a, f),
                _ => Vec::new(),
            };
        }
        let Some(sel) = self.page_sel else { return Vec::new() };
        if sel.version != self.doc.borrow().version {
            self.page_sel = None;
            return Vec::new();
        }
        if sel.anchor == sel.focus {
            return Vec::new();
        }
        self.layout().selection_ranges(sel.anchor, sel.focus)
    }

    pub fn page_selected_text(&mut self) -> String {
        let ranges = self.page_ranges();
        self.layout().selection_text(&ranges)
    }

    pub fn set_page_sel(&mut self, anchor: TextPoint, focus: TextPoint) {
        self.page_sel = Some(PageSel { anchor, focus, version: self.doc.borrow().version });
    }
}

/// Print how long a phase took when `NILE_TRACE` is set (for finding what to optimise).
fn traced<T>(label: &str, f: impl FnOnce() -> T) -> T {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("NILE_TRACE").is_some()) {
        return f();
    }
    let t = Instant::now();
    let out = f();
    eprintln!("[trace] {label}: {:.2} ms", t.elapsed().as_secs_f64() * 1e3);
    out
}

impl View {
    /// Recompute styles (after an interaction change or a DOM mutation).
    // TODO: restyle only the affected elements and relayout only if a
    // layout-affecting property changed.
    pub fn restyle(&mut self) {
        let doc = self.doc.borrow();
        let fresh = traced("style", || compute_styles_in(&doc, &self.interaction, self.viewport));
        let dom_changed = doc.version != self.seen_version;
        self.seen_version = doc.version;
        drop(doc);
        let old = self.adopt(fresh);
        self.reuse_layout_if_paint_only(&old, dom_changed);
    }

    /// Keep the laid-out boxes when a restyle only changed how things are painted (hover
    /// colors, fades, ...); otherwise throw them away.
    pub fn reuse_layout_if_paint_only(&mut self, old: &StyleMap, dom_changed: bool) {
        let keep = !dom_changed && self.layout.is_some() && self.styles.same_layout(old);
        match (&mut self.layout, keep) {
            (Some((_, _, tree)), true) => tree.refresh_paint(&self.styles),
            _ => self.layout = None,
        }
    }

    /// Take freshly cascaded styles, starting transitions for what changed.
    pub fn adopt(&mut self, fresh: StyleMap) -> StyleMap {
        if fresh.animates() || self.animator.is_tracking() {
            let now = self.clock.elapsed().as_secs_f64();
            let old = std::mem::replace(&mut self.styles, fresh.clone());
            let base = self.base.insert(fresh);
            self.animator.update(base, &mut self.styles, now, true);
            old
        } else {
            self.base = None;
            std::mem::replace(&mut self.styles, fresh)
        }
    }

    /// Advance transitions and animations to now. True if styles changed.
    pub fn animate(&mut self) -> bool {
        let Some(base) = &self.base else { return false };
        if !self.animator.is_active() {
            return false;
        }
        let now = self.clock.elapsed().as_secs_f64();
        let old = traced("animate: snapshot", || self.layout.is_some().then(|| self.styles.clone()));
        traced("animate: update", || self.animator.update(base, &mut self.styles, now, false));
        match old {
            Some(old) => traced("animate: compare", || self.reuse_layout_if_paint_only(&old, false)),
            None => self.layout = None,
        }
        true
    }

    /// Restyle if scripts changed the document. True if they did.
    pub fn sync(&mut self) -> bool {
        let changed = self.doc.borrow().version != self.seen_version;
        if changed {
            self.restyle();
        }
        changed
    }

    /// Scroll an `overflow` element, clamped to its range. True if it moved.
    pub fn set_element_scroll(&mut self, node: NodeId, x: f32, y: f32) -> bool {
        self.sync();
        let max = {
            let l = self.layout();
            l.scroll_box(node).map(|sb| sb.max_offset()).or_else(|| l.list_box(node).map(|b| (0.0, b.max_scroll)))
        };
        let Some((mx, my)) = max else { return false };
        let new = (x.clamp(0.0, mx), y.clamp(0.0, my));
        let old = self.scroll_offsets.get(&node).copied().unwrap_or_default();
        if new == old {
            return false;
        }
        self.scroll_offsets.insert(node, new);
        self.scrolled.push(node);
        self.scroll_dirty = true;
        // The drop-down is positioned for the old scroll position.
        self.menu = None;
        self.layout = None;
        true
    }

    /// Selection for an input that just got focus: a range a script set earlier,
    /// else the caret at the end of its value.
    pub fn select_for_focus(&mut self, node: NodeId) {
        let len = nile_layout::control_value(&self.doc.borrow(), node).chars().count();
        let (a, c) = self.pending_sel.remove(&node).unwrap_or((len, len));
        self.anchor = a.min(len);
        self.caret = c.min(len);
    }

    /// Open, move or close the `<select>` drop-down.
    pub fn set_menu(&mut self, menu: Option<MenuRequest>) {
        self.menu = menu;
        self.layout = None;
        self.scroll_dirty = true;
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        let changed = self.viewport != (width, height);
        // The first layout isn't a resize; later ones are.
        self.resized |= changed && self.viewport.0 > 0.0;
        self.viewport = (width, height);
        if changed && self.styles.uses_viewport {
            self.restyle();
        }
    }

    /// Layout at the current viewport, reusing the last result if unchanged.
    pub fn layout(&mut self) -> &LayoutTree {
        let (width, height) = self.viewport;
        let stale = !matches!(&self.layout, Some((w, h, _)) if *w == width && *h == height);
        if stale {
            let options = LayoutOptions { menu: self.menu, scroll_y: self.scroll_y, offsets: Some(&self.scroll_offsets), images: Some(&self.images) };
            let tree = traced("layout", || nile_layout::layout_with(&self.doc.borrow(), &self.styles, &mut self.text, width, height, &options));
            self.layout = Some((width, height, tree));
        }
        let y = self.scroll_y;
        let tree = &mut self.layout.as_mut().unwrap().2;
        tree.set_page_scroll(y);
        tree
    }
}

/// What scripts see of the view. Each query first brings styles and layout up
/// to date, so a script reads what it just wrote.
pub struct Env(Rc<RefCell<View>>);

impl Environment for Env {
    fn bounding_rect(&self, node: NodeId) -> Option<[f32; 4]> {
        let mut v = self.0.borrow_mut();
        v.sync();
        let scroll = v.scroll_y;
        let offsets = v.scroll_offsets.clone();
        let r = v.layout().node_rect_scrolled(node, &offsets)?;
        Some([r.x, r.y - scroll, r.width, r.height])
    }

    fn computed_style(&self, node: NodeId, property: &str) -> Option<String> {
        let mut v = self.0.borrow_mut();
        v.sync();
        v.styles.styles.get(node.0)?.css_value(property)
    }

    fn viewport(&self) -> (f32, f32) {
        self.0.borrow().viewport
    }

    fn scale(&self) -> f32 {
        self.0.borrow().scale
    }

    fn scroll_y(&self) -> f32 {
        self.0.borrow().scroll_y
    }

    fn scroll_to(&self, y: f32) {
        let mut v = self.0.borrow_mut();
        v.scroll_y = y.max(0.0);
        v.scroll_dirty = true;
    }

    fn interaction(&self) -> InteractionState {
        self.0.borrow().interaction
    }

    fn element_scroll(&self, node: NodeId) -> Option<[f32; 4]> {
        let mut v = self.0.borrow_mut();
        v.sync();
        let (ox, oy) = v.scroll_offsets.get(&node).copied().unwrap_or_default();
        let sb = *v.layout().scroll_box(node)?;
        Some([ox, oy, sb.content_width, sb.content_height])
    }

    fn scroll_element_to(&self, node: NodeId, x: f32, y: f32) {
        self.0.borrow_mut().set_element_scroll(node, x, y);
    }

    fn selection(&self, node: NodeId) -> (usize, usize) {
        let mut v = self.0.borrow_mut();
        v.sync();
        let len = nile_layout::control_value(&v.doc.borrow(), node).chars().count();
        let (a, c) = if v.interaction.focus == Some(node) {
            (v.anchor, v.caret)
        } else {
            v.pending_sel.get(&node).copied().unwrap_or((len, len))
        };
        (a.min(c).min(len), a.max(c).min(len))
    }

    fn selected_text(&self) -> String {
        let mut v = self.0.borrow_mut();
        v.sync();
        v.page_selected_text()
    }

    fn clear_selected_text(&self) {
        let mut v = self.0.borrow_mut();
        v.page_sel = None;
        v.edit_sel = None;
    }

    fn edit_selection(&self) -> Option<[(NodeId, usize); 2]> {
        self.0.borrow().edit_sel.map(|(a, f)| [(a.node, a.offset), (f.node, f.offset)])
    }

    fn set_edit_selection(&self, anchor: (NodeId, usize), focus: (NodeId, usize)) {
        let mut v = self.0.borrow_mut();
        let n = v.doc.borrow().nodes.len();
        if anchor.0 .0 < n && focus.0 .0 < n {
            v.edit_sel = Some((DomPoint { node: anchor.0, offset: anchor.1 }, DomPoint { node: focus.0, offset: focus.1 }));
            v.page_sel = None;
        }
    }

    fn clear_edit_selection(&self) {
        self.0.borrow_mut().edit_sel = None;
    }

    fn clipboard_request(&self, kind: &str) {
        self.0.borrow_mut().clipboard_requests.push(kind.to_string());
    }

    fn window_command(&self, cmd: WindowCommand) {
        self.0.borrow_mut().window_requests.push(cmd);
    }

    fn window_rect(&self) -> [f32; 5] {
        let v = self.0.borrow();
        v.window_rect.unwrap_or([0.0, 0.0, v.viewport.0, v.viewport.1, 0.0])
    }

    fn set_selection(&self, node: NodeId, start: usize, end: usize) {
        let mut v = self.0.borrow_mut();
        let len = nile_layout::control_value(&v.doc.borrow(), node).chars().count();
        let (a, c) = (start.min(end).min(len), start.max(end).min(len));
        if v.interaction.focus == Some(node) {
            v.anchor = a;
            v.caret = c;
            v.scroll_dirty = true;
        } else {
            v.pending_sel.insert(node, (a, c));
        }
    }

    fn set_focus(&self, node: Option<NodeId>) {
        let mut v = self.0.borrow_mut();
        if v.interaction.focus != node {
            let old = v.interaction.focus;
            v.interaction.focus = node;
            if let Some(n) = node {
                v.select_for_focus(n);
            }
            v.focus_changes.push((old, node));
            v.restyle();
        }
    }
}

/// One loaded document plus everything needed to paint and interact with it.
pub struct Page {
    pub path: PathBuf,
    /// The HTML text, for pages that weren't loaded from a file (so reload can redo them).
    pub memory: Option<String>,
    /// How much of the console output `take_console` has returned.
    pub console_read: usize,
    /// Shared with the script host, which mutates it.
    pub doc: Rc<RefCell<Document>>,
    pub view: Rc<RefCell<View>>,
    pub renderer: Renderer,
    /// Last pointer position in logical viewport pixels.
    pub pointer: Option<(f32, f32)>,
    /// Element the primary button went down on, for deriving `click`.
    pub pressed: Option<NodeId>,
    pub scripts: Option<ScriptHost>,
    /// Modifier keys currently held (for shift-click etc.).
    pub mods: ModifiersState,
    /// Text input a mouse drag is selecting in.
    pub drag: Option<NodeId>,
    /// A range slider being dragged, and its value when the drag began.
    pub range_drag: Option<(NodeId, String)>,
    /// An editing-host drag selection: its anchor and whether it began with a double-click.
    pub edit_drag: Option<(DomPoint, bool)>,
    /// The x the caret keeps while moving up and down.
    pub edit_x: Option<f32>,
    /// List box row the shift-range started from, and the row keys moved to last.
    pub list_anchor: Option<(NodeId, usize)>,
    pub list_cursor: Option<(NodeId, usize)>,
    /// Time, position and count of the last primary click, for double/triple-click.
    pub last_click: Option<(Instant, (f32, f32), u32)>,
    pub clipboard: Box<dyn Clipboard>,
    /// Value when the focused input got focus, to decide whether `change` fires.
    pub value_at_focus: Option<String>,
    /// A mouse drag extending the page selection.
    pub page_drag: bool,
    /// A scrollbar thumb being dragged.
    pub bar_drag: Option<BarDrag>,
    /// Undo/redo stacks of the text inputs edited so far.
    pub history: std::collections::HashMap<NodeId, History>,
    /// Blink phase of the caret and when it flips next.
    pub caret_visible: bool,
    pub blink_at: Instant,
}

/// A scrollbar thumb held by the pointer.
pub struct BarDrag {
    /// The scrolling element, or `None` for the page.
    node: Option<NodeId>,
    /// The scrollbar where it was grabbed, in painted coordinates.
    thumb: nile_layout::Thumb,
    /// How far into the thumb (along the track) the pointer grabbed it.
    grab: f32,
}

/// What an edit did, so runs of the same kind (typing, repeated Backspace) undo as one step.
#[derive(Clone, Copy, PartialEq)]
pub enum EditKind {
    Insert,
    Backspace,
    Delete,
    /// Paste, cut, a replaced selection: always its own step.
    Other,
}

/// An input's value and selection (anchor, caret).
#[derive(Clone)]
pub struct EditState {
    value: String,
    anchor: usize,
    caret: usize,
}

#[derive(Default)]
pub struct History {
    undo: Vec<EditState>,
    redo: Vec<EditState>,
    /// The last edit's kind and where the caret ended, for coalescing.
    last: Option<(EditKind, usize)>,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Start of the word before `at`, skipping separators first.
fn word_start(chars: &[char], mut at: usize) -> usize {
    while at > 0 && !is_word_char(chars[at - 1]) {
        at -= 1;
    }
    while at > 0 && is_word_char(chars[at - 1]) {
        at -= 1;
    }
    at
}

/// End of the word after `at`, skipping separators first.
fn word_end(chars: &[char], mut at: usize) -> usize {
    while at < chars.len() && !is_word_char(chars[at]) {
        at += 1;
    }
    while at < chars.len() && is_word_char(chars[at]) {
        at += 1;
    }
    at
}

impl Page {
    pub fn load(path: PathBuf, viewport: (f32, f32), scroll_y: f32) -> Page {
        let html = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            format!("<body><h1>Could not open file</h1><p>{}: {e}</p></body>", path.display())
        });
        Page::from_source(html, path, None, viewport, scroll_y)
    }

    /// A page from HTML text. Relative `src`, `href` and `url()` paths resolve against `base_dir`.
    pub fn load_html(html: &str, base_dir: PathBuf, viewport: (f32, f32)) -> Page {
        Page::from_source(html.to_string(), base_dir.join("index.html"), Some(html.to_string()), viewport, 0.0)
    }

    /// `memory`: the HTML to use again on reload, for pages that didn't come from a file.
    fn from_source(html: String, path: PathBuf, memory: Option<String>, viewport: (f32, f32), scroll_y: f32) -> Page {
        let doc = Rc::new(RefCell::new(Document::parse(&html)));
        let interaction = InteractionState::default();
        let styles = compute_styles_in(&doc.borrow(), &interaction, viewport);
        let view = Rc::new(RefCell::new(View {
            doc: doc.clone(),
            seen_version: doc.borrow().version,
            styles: StyleMap::clone(&styles),
            base: None,
            animator: Animator::new(),
            clock: Instant::now(),
            scale: 1.0,
            text: TextEngine::new(),
            layout: None,
            viewport,
            scroll_y,
            scroll_dirty: false,
            scroll_offsets: ScrollOffsets::new(),
            scrolled: Vec::new(),
            menu: None,
            anchor: 0,
            caret: 0,
            pending_sel: std::collections::HashMap::new(),
            interaction,
            page_sel: None,
            edit_sel: None,
            clipboard_requests: Vec::new(),
            images: FileImages::new(path.parent().map(PathBuf::from).unwrap_or_default()),
            focus_changes: Vec::new(),
            window_requests: Vec::new(),
            resized: false,
            window_rect: None,
        }));
        view.borrow_mut().adopt(styles);

        let scripts = match ScriptHost::new(doc.clone(), Rc::new(Env(view.clone()))) {
            Ok(host) => {
                let sources = doc.borrow().script_sources();
                let dir = path.parent().unwrap_or(std::path::Path::new("."));
                host.set_fetcher(std::sync::Arc::new(net::PageFetcher::new(dir)));
                let url = std::fs::canonicalize(&path).ok().map(|p| format!("file://{}", p.to_string_lossy().replace('\\', "/")));
                host.set_location(url.as_deref().unwrap_or("about:blank"));
                let read = |rel: &str| -> Option<(String, String)> {
                    let file = dir.join(rel);
                    match std::fs::read_to_string(&file) {
                        Ok(code) => Some((file.to_string_lossy().into_owned(), code)),
                        Err(e) => {
                            eprintln!("[js] could not load {}: {e}", file.display());
                            None
                        }
                    }
                };
                let report = |r: Result<(), String>| {
                    if let Err(e) = r {
                        eprintln!("[js error] {e}");
                    }
                };
                // Classic scripts run in order; modules are deferred until after them.
                let mut modules = Vec::new();
                for src in sources {
                    match src {
                        ScriptSource::Inline(code) => report(host.run(&code)),
                        ScriptSource::External(rel) => {
                            if let Some((_, code)) = read(&rel) {
                                report(host.run(&code));
                            }
                        }
                        ScriptSource::ModuleInline(code) => {
                            // Named like a file in the page's folder so relative imports resolve.
                            modules.push((format!("{}#module{}", path.display(), modules.len()), code));
                        }
                        ScriptSource::ModuleExternal(rel) => modules.extend(read(&rel)),
                    }
                }
                for (name, code) in modules {
                    report(host.run_module(&name, &code));
                }
                host.fire_load();
                Some(host)
            }
            Err(e) => {
                eprintln!("[js] runtime failed to start: {e}");
                None
            }
        };
        // Scripts may have changed the document.
        view.borrow_mut().sync();

        Page {
            path,
            memory,
            console_read: 0,
            doc,
            view,
            renderer: Renderer::new(),
            pointer: None,
            pressed: None,
            scripts,
            mods: ModifiersState::default(),
            drag: None,
            range_drag: None,
            edit_drag: None,
            edit_x: None,
            list_anchor: None,
            list_cursor: None,
            last_click: None,
            clipboard: Box::new(MemoryClipboard::default()),
            value_at_focus: None,
            history: std::collections::HashMap::new(),
            bar_drag: None,
            page_drag: false,
            caret_visible: true,
            blink_at: Instant::now() + CARET_BLINK,
        }
    }

    pub fn reload(&mut self) {
        let (viewport, scroll) = {
            let v = self.view.borrow();
            (v.viewport, v.scroll_y)
        };
        let clipboard = std::mem::replace(&mut self.clipboard, Box::new(MemoryClipboard::default()));
        *self = match self.memory.take() {
            Some(html) => Page::from_source(html.clone(), self.path.clone(), Some(html), viewport, scroll),
            None => Page::load(self.path.clone(), viewport, scroll),
        };
        self.clipboard = clipboard;
    }

    /// Evaluate script text in the page and return the value as text (strings as they are,
    /// `undefined` as "", anything else as JSON). Also returns whether the page changed.
    pub fn eval(&mut self, source: &str) -> (Result<String, String>, bool) {
        let Some(host) = &self.scripts else { return (Err("scripting is unavailable".into()), false) };
        let r = host.eval(source);
        (r, self.sync())
    }

    /// Call a global function (`name` may be dotted) with a JSON array of arguments.
    pub fn call(&mut self, name: &str, args_json: &str) -> (Result<String, String>, bool) {
        let Some(host) = &self.scripts else { return (Err("scripting is unavailable".into()), false) };
        let r = host.call(name, args_json);
        (r, self.sync())
    }

    /// Expose `f` to scripts as `host.<name>(...)`; see [`ScriptHost::register_host_fn`].
    pub fn register_host_fn(&self, name: &str, f: impl Fn(&str) -> String + 'static) {
        if let Some(host) = &self.scripts {
            host.register_host_fn(name, f);
        }
    }

    /// The selected text: the focused input's selection, else the page's.
    pub fn selected_text(&mut self) -> String {
        if let Some(f) = self.interaction().focus.filter(|f| self.is_text_input(*f)) {
            let chars: Vec<char> = self.value_of(f).chars().collect();
            let (a, c) = (self.anchor().min(chars.len()), self.caret().min(chars.len()));
            return chars[a.min(c)..a.max(c)].iter().collect();
        }
        self.view.borrow_mut().page_selected_text()
    }

    /// `console.*` lines written since the last call.
    pub fn take_console(&mut self) -> Vec<String> {
        let Some(host) = &self.scripts else { return Vec::new() };
        let all = host.console_output();
        let new = all[self.console_read.min(all.len())..].to_vec();
        self.console_read = all.len();
        new
    }

    /// Lay out at the given logical viewport size (a no-op if unchanged).
    pub fn layout(&mut self, width: f32, height: f32) {
        let mut v = self.view.borrow_mut();
        v.set_viewport(width, height);
        v.layout();
    }

    /// Restyle after an interaction state change.
    pub fn restyle(&mut self) {
        self.view.borrow_mut().restyle();
    }

    /// Pick up DOM changes and script-driven scrolling. True if the page
    /// needs repainting.
    pub fn sync(&mut self) -> bool {
        if let Some(host) = self.scripts.as_ref().filter(|h| h.observing()) {
            host.check_observers();
        }
        let (changed, moves, scrolled) = {
            let mut v = self.view.borrow_mut();
            let changed = v.sync();
            (
                std::mem::take(&mut v.scroll_dirty) | changed,
                std::mem::take(&mut v.focus_changes),
                std::mem::take(&mut v.scrolled),
            )
        };
        let moved = !moves.is_empty();
        for (old, new) in moves {
            self.focus_moved(old, new);
        }
        let requests = std::mem::take(&mut self.view.borrow_mut().clipboard_requests);
        for kind in requests {
            self.clipboard_command(&kind);
        }
        for node in scrolled {
            self.emit(node, UiEvent::Plain("scroll"));
        }
        changed | moved
    }

    /// Focus moved from `old` to `new`: end the old input's edit (firing
    /// `change` if its value changed), start the new one, and fire `blur`/`focus`.
    pub fn focus_moved(&mut self, old: Option<NodeId>, new: Option<NodeId>) {
        if let Some(o) = old {
            if self.value_at_focus.take().is_some_and(|v| v != self.value_of(o)) {
                self.emit(o, UiEvent::Plain("change"));
            }
            self.emit(o, UiEvent::Plain("blur"));
        }
        // A host focused without a click (Tab, `focus()`) puts the caret at its start.
        if let Some(n) = new.filter(|_| self.focused_host() == new) {
            let inside = self.edit_selection().is_some_and(|(_, f)| editing::editing_host_of(&self.doc.borrow(), f.node) == Some(n));
            if !inside {
                let start = DomPoint { node: n, offset: 0 };
                self.set_edit_selection(start, start);
            }
        }
        if self.view.borrow().menu.is_some_and(|m| Some(m.select) != new) {
            self.close_menu();
        }
        self.show_caret();
        if let Some(n) = new {
            self.value_at_focus = self.is_text_input(n).then(|| self.value_of(n));
            self.emit(n, UiEvent::Plain("focus"));
        }
    }

    /// Selection anchor and caret (characters of the focused input's value).
    pub fn caret(&self) -> usize {
        self.view.borrow().caret
    }

    pub fn anchor(&self) -> usize {
        self.view.borrow().anchor
    }

    pub fn set_sel(&mut self, anchor: usize, caret: usize) {
        let mut v = self.view.borrow_mut();
        v.anchor = anchor;
        v.caret = caret;
    }

    pub fn interaction(&self) -> InteractionState {
        self.view.borrow().interaction
    }

    /// Dispatch an event to scripts, then pick up any changes. Returns
    /// (not default-prevented, page changed).
    pub fn emit(&mut self, target: NodeId, event: UiEvent) -> (bool, bool) {
        let Some(host) = &self.scripts else { return (true, false) };
        let proceed = host.dispatch(target, &event);
        (proceed, self.sync())
    }

    /// Run due timers. Returns whether the page changed and when to wake next.
    pub fn run_timers(&mut self) -> (bool, Option<Duration>) {
        let Some(host) = &self.scripts else { return (false, None) };
        if std::mem::take(&mut self.view.borrow_mut().resized) {
            host.fire_window_event("resize");
        }
        let next = host.tick();
        (self.sync(), next)
    }

    /// The element under a viewport point, if the page has been laid out.
    pub fn hit_test(&mut self, x: f32, y: f32) -> Option<NodeId> {
        let mut v = self.view.borrow_mut();
        if v.viewport.0 <= 0.0 {
            return None;
        }
        let scroll = v.scroll_y;
        let offsets = v.scroll_offsets.clone();
        v.layout().hit_test_scrolled(x, y + scroll, &offsets)
    }

    /// Scroll under the pointer by a wheel delta (positive scrolls content up/left,
    /// i.e. `dy > 0` is wheel-up): the innermost `overflow: auto | scroll` ancestor
    /// that can still move that way takes it, else the page does.
    pub fn wheel(&mut self, dx: f32, dy: f32) {
        if let Some((x, y)) = self.pointer {
            if self.menu_row_at(x, y).is_some() {
                self.scroll_menu(-dy);
                return;
            }
        }
        self.close_menu();
        let target = self.pointer.and_then(|(x, y)| self.hit_test(x, y));
        self.scroll_chain(target, dx, dy);
    }

    /// Scroll by a wheel-style delta starting at `start` and chaining outwards to the page.
    pub fn scroll_chain(&mut self, start: Option<NodeId>, dx: f32, dy: f32) {
        let mut cur = start;
        while let Some(n) = cur {
            let moved = {
                let mut v = self.view.borrow_mut();
                let sb = v.layout().scroll_box(n).copied();
                let list = v.layout().list_box(n).map(|l| l.max_scroll);
                let (ox, oy) = v.scroll_offsets.get(&n).copied().unwrap_or_default();
                match sb {
                    None if list.is_some() => {
                        let my = list.unwrap_or(0.0);
                        ((dy > 0.0 && oy > 0.0) || (dy < 0.0 && oy < my)) && v.set_element_scroll(n, 0.0, oy - dy)
                    }
                    Some(sb) => {
                        let (mx, my) = sb.max_offset();
                        let can_y = sb.wheel_y && ((dy > 0.0 && oy > 0.0) || (dy < 0.0 && oy < my));
                        let can_x = sb.wheel_x && ((dx > 0.0 && ox > 0.0) || (dx < 0.0 && ox < mx));
                        (can_x || can_y)
                            && v.set_element_scroll(n, if can_x { ox - dx } else { ox }, if can_y { oy - dy } else { oy })
                    }
                    None => false,
                }
            };
            if moved {
                self.sync();
                self.refresh_hover();
                return;
            }
            cur = self.doc.borrow().node(n).parent;
        }
        {
            let mut v = self.view.borrow_mut();
            let max = (v.layout().content_height - v.viewport.1).max(0.0);
            v.scroll_y = (v.scroll_y - dy).clamp(0.0, max);
        }
        self.refresh_hover();
    }

    /// Keyboard scrolling when no text is being edited: arrows, Page Up/Down, Space
    /// (Shift reverses), Home and End. Scrolls the scroller under the focus or pointer,
    /// then the page. True if the key was a scroll key.
    pub fn scroll_key(&mut self, key: &str, mods: ModifiersState) -> bool {
        if mods.control_key() || mods.alt_key() || mods.super_key() {
            return false;
        }
        let start = self.interaction().focus.or_else(|| self.pointer.and_then(|(x, y)| self.hit_test(x, y)));
        // The scroller the key applies to, to size a "page" and tell if it can scroll at all.
        let page = {
            let mut v = self.view.borrow_mut();
            let mut cur = start;
            let mut found = None;
            while let Some(n) = cur {
                if let Some(sb) = v.layout().scroll_box(n).copied() {
                    if sb.wheel_y {
                        found = Some(sb.clip.height);
                        break;
                    }
                }
                cur = self.doc.borrow().node(n).parent;
            }
            found.unwrap_or(v.viewport.1) * 0.875
        };
        let (dx, dy) = match key {
            "ArrowDown" => (0.0, -40.0),
            "ArrowUp" => (0.0, 40.0),
            "ArrowRight" => (-40.0, 0.0),
            "ArrowLeft" => (40.0, 0.0),
            "PageDown" => (0.0, -page),
            "PageUp" => (0.0, page),
            " " => (0.0, if mods.shift_key() { page } else { -page }),
            "End" => (0.0, -1.0e7),
            "Home" => (0.0, 1.0e7),
            _ => return false,
        };
        self.scroll_chain(start, dx, dy);
        true
    }

    /// The scrollbar under a viewport point: who scrolls, its geometry in painted
    /// coordinates (document coordinates for elements), and whether the thumb itself
    /// (rather than the track) is under the point.
    pub fn scrollbar_at(&mut self, x: f32, y: f32) -> Option<(Option<NodeId>, nile_layout::Thumb, bool)> {
        let mut v = self.view.borrow_mut();
        if v.viewport.0 <= 0.0 {
            return None;
        }
        // A little slack either side of the thin bar makes it easy to grab.
        let near = |r: nile_layout::Rect, px: f32, py: f32| px >= r.x - 4.0 && px <= r.x + r.width + 4.0 && py >= r.y - 2.0 && py <= r.y + r.height + 2.0;
        let (scroll, viewport) = (v.scroll_y, v.viewport);
        let content = v.layout().content_height;
        if let Some(t) = nile_layout::page_thumb(viewport, content, scroll) {
            if near(t.track, x, y) {
                return Some((None, t, near(t.rect, x, y)));
            }
        }
        let offsets = v.scroll_offsets.clone();
        let (dx, dy) = (x, y + scroll);
        let tree = v.layout();
        let places = tree.placements(&offsets);
        for (b, p) in tree.boxes.iter().zip(&places).rev() {
            let Some(sb) = &b.scroll else { continue };
            let (ox, oy) = offsets.get(&b.node).copied().unwrap_or_default();
            for t in sb.thumbs(ox, oy) {
                let t = nile_layout::Thumb { rect: p.xf.bounds(t.rect), track: p.xf.bounds(t.track), ..t };
                if near(t.track, dx, dy) {
                    return Some((Some(b.node), t, near(t.rect, dx, dy)));
                }
            }
        }
        None
    }

    /// Start dragging a scrollbar under the pointer (a click on its track jumps there).
    pub fn scrollbar_press(&mut self, x: f32, y: f32) -> bool {
        if self.view.borrow().menu.is_some() {
            return false;
        }
        let Some((node, thumb, on_thumb)) = self.scrollbar_at(x, y) else { return false };
        let pos = self.bar_axis(node, thumb.vertical, x, y);
        let start = if thumb.vertical { thumb.rect.y } else { thumb.rect.x };
        let len = if thumb.vertical { thumb.rect.height } else { thumb.rect.width };
        let grab = if on_thumb { pos - start } else { len / 2.0 };
        self.bar_drag = Some(BarDrag { node, thumb, grab });
        self.bar_drag_to(x, y);
        true
    }

    /// The pointer's coordinate along a scrollbar's axis, in that scrollbar's space.
    pub fn bar_axis(&self, node: Option<NodeId>, vertical: bool, x: f32, y: f32) -> f32 {
        if vertical {
            if node.is_some() { y + self.view.borrow().scroll_y } else { y }
        } else {
            x
        }
    }

    pub fn bar_drag_to(&mut self, x: f32, y: f32) -> bool {
        let Some(drag) = &self.bar_drag else { return false };
        let (node, thumb, grab) = (drag.node, drag.thumb, drag.grab);
        let offset = thumb.offset_for(self.bar_axis(node, thumb.vertical, x, y) - grab);
        match node {
            None => self.view.borrow_mut().scroll_y = offset,
            Some(n) => {
                let moved = {
                    let mut v = self.view.borrow_mut();
                    let (ox, oy) = v.scroll_offsets.get(&n).copied().unwrap_or_default();
                    if thumb.vertical { v.set_element_scroll(n, ox, offset) } else { v.set_element_scroll(n, offset, oy) }
                };
                if moved {
                    self.sync();
                }
            }
        }
        true
    }

    /// Scroll the focused input/textarea so its caret is visible.
    pub fn reveal_caret(&mut self) {
        let Some(f) = self.interaction().focus.filter(|f| self.is_text_input(*f)) else { return };
        let at = nile_layout::caret_byte(&self.doc.borrow(), f, self.caret());
        let mut v = self.view.borrow_mut();
        v.sync();
        let tree = v.layout();
        let (Some(c), Some(sb)) = (tree.caret_rect(f, at), tree.scroll_box(f).copied()) else { return };
        let (ox, oy) = v.scroll_offsets.get(&f).copied().unwrap_or_default();
        // Reveal the caret plus the box's padding, so the first line scrolls fully to the top.
        let pad = v.styles.get(f).padding;
        let (clip, cx, cy) = (sb.clip, c.x - sb.clip.x, c.y - sb.clip.y);
        let nx = if cx - pad.left < ox {
            (cx - pad.left).max(0.0)
        } else if cx + c.width + 1.0 + pad.right > ox + clip.width {
            cx + c.width + 1.0 + pad.right - clip.width
        } else {
            ox
        };
        let ny = if cy - pad.top < oy {
            (cy - pad.top).max(0.0)
        } else if cy + c.height + pad.bottom > oy + clip.height {
            cy + c.height + pad.bottom - clip.height
        } else {
            oy
        };
        v.set_element_scroll(f, nx, ny);
    }

    /// Nearest ancestor-or-self that can take focus (links, form controls, `tabindex`).
    pub fn focusable(&self, mut id: NodeId) -> Option<NodeId> {
        loop {
            let doc = self.doc.borrow();
            let n = doc.node(id);
            let focusable = match n.tag() {
                Some("a") => n.attr("href").is_some(),
                Some("input" | "button" | "select" | "textarea") => true,
                _ => {
                    n.attr("tabindex").is_some()
                        || n.attr("contenteditable").is_some_and(|v| !v.trim().eq_ignore_ascii_case("false"))
                }
            };
            if focusable {
                return Some(id);
            }
            id = n.parent?;
        }
    }

    /// The picture named by `window-icon` (a file next to the page) as unpremultiplied RGBA
    /// pixels with its width and height.
    pub fn window_icon(&self, src: &str) -> Option<(Vec<u8>, u32, u32)> {
        let bytes = std::fs::read(self.path.parent()?.join(src)).ok()?;
        let img = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes))?;
        let (w, h) = (img.width(), img.height());
        let info = ImageInfo::new((w, h), ColorType::RGBA8888, AlphaType::Unpremul, None);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        img.read_pixels(&info, &mut rgba, w as usize * 4, (0, 0), skia_safe::image::CachingHint::Allow).then_some(())?;
        Some((rgba, w as u32, h as u32))
    }

    pub fn is_range(&self, id: NodeId) -> bool {
        let doc = self.doc.borrow();
        let n = doc.node(id);
        n.tag() == Some("input") && n.attr("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("range"))
    }

    pub fn is_number(&self, id: NodeId) -> bool {
        let doc = self.doc.borrow();
        let n = doc.node(id);
        n.tag() == Some("input") && n.attr("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("number"))
    }

    /// Step a number input up (`dir > 0`) or down by its `step`, clamped to `min`/`max`;
    /// fires `input` and `change`.
    fn number_step(&mut self, id: NodeId, dir: f64) -> bool {
        if self.is_disabled(id) || self.doc.borrow().node(id).attr("readonly").is_some() {
            return false;
        }
        let (num, current) = {
            let doc = self.doc.borrow();
            let n = doc.node(id);
            let num = |name: &str| n.attr(name).and_then(|v| v.trim().parse::<f64>().ok());
            let step = match n.attr("step").map(str::trim) {
                Some("any") => 1.0,
                _ => num("step").filter(|s| *s > 0.0).unwrap_or(1.0),
            };
            ((num("min"), num("max"), step), n.attr("value").and_then(|v| v.trim().parse::<f64>().ok()))
        };
        let (min, max, step) = num;
        let mut v = match current {
            Some(v) => {
                // Land on the next multiple of `step` (from `min`) in that direction.
                let base = min.unwrap_or(0.0);
                let k = (v - base) / step;
                let k = if dir > 0.0 { (k + 1e-9).floor() + 1.0 } else { (k - 1e-9).ceil() - 1.0 };
                base + k * step
            }
            // Empty: the minimum, or one step from zero.
            None => min.unwrap_or(dir * step),
        };
        if let Some(lo) = min {
            v = v.max(lo);
        }
        if let Some(hi) = max {
            v = v.min(hi);
        }
        let text = format!("{v:.10}");
        let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text };
        if current.is_some_and(|c| format!("{c}") == text) {
            return false;
        }
        self.doc.borrow_mut().set_attr(id, "value", &text);
        let len = text.chars().count();
        self.set_sel(len, len);
        self.value_at_focus = Some(text);
        self.sync();
        self.emit(id, UiEvent::Plain("input"));
        self.emit(id, UiEvent::Plain("change"));
        true
    }

    /// Which spin arrow is under the pointer: `Some(1.0)` up, `Some(-1.0)` down.
    fn number_arrow_at_pointer(&mut self, id: NodeId) -> Option<f64> {
        let (x, y) = self.pointer?;
        let (rect, scroll) = {
            let mut v = self.view.borrow_mut();
            let offsets = v.scroll_offsets.clone();
            let scroll = v.scroll_y;
            (v.layout().node_rect_scrolled(id, &offsets)?, scroll)
        };
        let y = y + scroll;
        (x >= rect.x + rect.width - 18.0 && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height).then(|| if y < rect.y + rect.height / 2.0 { 1.0 } else { -1.0 })
    }

    /// `(min, max, step)` of a range input; step 0 means "any".
    fn range_bounds(&self, id: NodeId) -> (f32, f32, f32) {
        let doc = self.doc.borrow();
        let n = doc.node(id);
        let num = |name: &str| n.attr(name).and_then(|v| v.trim().parse::<f32>().ok());
        let (min, max) = (num("min").unwrap_or(0.0), num("max").unwrap_or(100.0));
        let step = match n.attr("step").map(str::trim) {
            Some("any") => 0.0,
            _ => num("step").filter(|s| *s > 0.0).unwrap_or(1.0),
        };
        (min, max.max(min), step)
    }

    /// The slider's current value, clamped into its range.
    pub fn range_value(&self, id: NodeId) -> f32 {
        let (min, max, _) = self.range_bounds(id);
        let v = self.doc.borrow().node(id).attr("value").and_then(|v| v.trim().parse::<f32>().ok());
        v.unwrap_or((min + max) / 2.0).clamp(min, max)
    }

    /// Set a slider's value (snapped to `step`), firing `input` if it changed.
    fn range_set(&mut self, id: NodeId, value: f32) -> bool {
        let (min, max, step) = self.range_bounds(id);
        let mut v = value.clamp(min, max);
        if step > 0.0 {
            v = (min + ((v - min) / step).round() * step).min(max);
        }
        if v == self.range_value(id) && self.doc.borrow().node(id).attr("value").is_some() {
            return false;
        }
        self.doc.borrow_mut().set_attr(id, "value", &v.to_string());
        self.sync();
        self.emit(id, UiEvent::Plain("input"));
        true
    }

    /// Put a slider's thumb where the pointer is.
    fn range_from_pointer(&mut self, id: NodeId) -> bool {
        let Some((x, _)) = self.pointer else { return false };
        let rect = {
            let mut v = self.view.borrow_mut();
            let offsets = v.scroll_offsets.clone();
            v.layout().node_rect_scrolled(id, &offsets)
        };
        let Some(r) = rect else { return false };
        let (min, max, _) = self.range_bounds(id);
        let radius = (r.height / 2.0).min(8.0);
        let span = (r.width - 2.0 * radius).max(1.0);
        let f = ((x - r.x - radius) / span).clamp(0.0, 1.0);
        self.range_set(id, min + f * (max - min))
    }

    /// Arrow keys, Page Up/Down, Home and End on a focused slider.
    fn range_key(&mut self, id: NodeId, key: &str) -> bool {
        let (min, max, step) = self.range_bounds(id);
        let unit = if step > 0.0 { step } else { (max - min) / 100.0 };
        let v = self.range_value(id);
        let target = match key {
            "ArrowRight" | "ArrowUp" => v + unit,
            "ArrowLeft" | "ArrowDown" => v - unit,
            "PageUp" => v + unit * 10.0,
            "PageDown" => v - unit * 10.0,
            "Home" => min,
            "End" => max,
            _ => return false,
        };
        let changed = self.range_set(id, target);
        if changed {
            self.emit(id, UiEvent::Plain("change"));
        }
        changed
    }

    /// The `<details>` whose `<summary>` is at or above `node`.
    fn summary_details(&self, node: NodeId) -> Option<NodeId> {
        let doc = self.doc.borrow();
        let mut cur = Some(node);
        while let Some(n) = cur {
            if doc.node(n).tag() == Some("summary") {
                return doc.node(n).parent.filter(|p| doc.node(*p).tag() == Some("details"));
            }
            cur = doc.node(n).parent;
        }
        None
    }

    /// A press on a `role="window-caption"` element: drag the window, or on a double click
    /// toggle maximize. Returns true when the window loop should start the system drag.
    pub fn caption_press(&mut self) -> bool {
        let Some(hover) = self.interaction().hover else { return false };
        if window::role_at(&self.doc.borrow(), hover).map(|r| r.1) != Some(Role::Caption) {
            return false;
        }
        if self.register_click() == 2 {
            self.view.borrow_mut().window_requests.push(WindowCommand::ToggleMaximize);
            return false;
        }
        true
    }

    /// The pointer shape for what's under the pointer: a pressed scrollbar thumb or an open
    /// drop-down use the arrow; otherwise the hovered element's `cursor`.
    pub fn cursor(&self) -> nile_style::Cursor {
        use nile_style::Cursor;
        if self.bar_drag.is_some() || self.view.borrow().menu.is_some() {
            return Cursor::Default;
        }
        match self.interaction().hover {
            Some(n) => self.view.borrow().styles.get(n).cursor,
            None => Cursor::Auto,
        }
    }

    /// Re-run hit testing for the current pointer and scroll, firing
    /// `mouseout`/`mouseover` when the hovered element changes. True if the
    /// page needs repainting.
    pub fn refresh_hover(&mut self) -> bool {
        let hover = self.pointer.and_then(|(x, y)| self.hit_test(x, y));
        let old = self.interaction().hover;
        if hover == old {
            return false;
        }
        self.view.borrow_mut().interaction.hover = hover;
        self.restyle();
        let (x, y) = self.pointer.unwrap_or_default();
        let mouse = |kind| UiEvent::Mouse { kind, x, y, button: 0 };
        if let Some(o) = old {
            self.emit(o, mouse("mouseout"));
        }
        if let Some(n) = hover {
            self.emit(n, mouse("mouseover"));
        }
        true
    }

    pub fn pointer_moved(&mut self, x: f32, y: f32) -> bool {
        self.pointer = Some((x, y));
        if self.bar_drag.is_some() {
            return self.bar_drag_to(x, y);
        }
        if let Some((node, _)) = self.range_drag.clone() {
            return self.range_from_pointer(node);
        }
        if self.edit_drag.is_some() {
            self.refresh_hover();
            return self.edit_drag_to_pointer();
        }
        if self.page_drag {
            if let Some(p) = self.page_text_point() {
                let mut v = self.view.borrow_mut();
                if let Some(s) = v.page_sel {
                    v.set_page_sel(s.anchor, p);
                }
            }
            self.refresh_hover();
            return true;
        }
        // With a drop-down open the pointer belongs to it.
        if self.view.borrow().menu.is_some() {
            return self.menu_hover(x, y);
        }
        let mut changed = self.refresh_hover();
        // Dragging inside a text input extends its selection.
        if let Some(f) = self.drag {
            if let Some(c) = self.char_at_pointer(f) {
                changed |= c != self.caret();
                let a = self.anchor();
                self.set_sel(a, c);
                self.reveal_caret();
            }
        }
        if let Some(t) = self.interaction().hover {
            changed |= self.emit(t, UiEvent::Mouse { kind: "mousemove", x, y, button: 0 }).1;
        }
        changed
    }

    pub fn pointer_left(&mut self) -> bool {
        self.pointer = None;
        self.refresh_hover()
    }

    /// Count consecutive clicks close in time and place (1, 2, 3).
    pub fn register_click(&mut self) -> u32 {
        let now = Instant::now();
        let pos = self.pointer.unwrap_or_default();
        let count = match self.last_click {
            Some((t, (x, y), n))
                if now.duration_since(t) < Duration::from_millis(500)
                    && (x - pos.0).abs() < 4.0
                    && (y - pos.1).abs() < 4.0 =>
            {
                (n + 1).min(3)
            }
            _ => 1,
        };
        self.last_click = Some((now, pos, count));
        count
    }

    /// The character index in input `f`'s value nearest the pointer.
    pub fn char_at_pointer(&mut self, f: NodeId) -> Option<usize> {
        let (x, y) = self.pointer?;
        let at = {
            let mut v = self.view.borrow_mut();
            let scroll = v.scroll_y;
            let offsets = v.scroll_offsets.clone();
            v.layout().text_index_at_scrolled(f, x, y + scroll, &offsets)?
        };
        Some(nile_layout::caret_char(&self.doc.borrow(), f, at))
    }

    /// The word under the pointer, as a character range of `f`'s value.
    pub fn word_at_pointer(&mut self, f: NodeId) -> Option<std::ops::Range<usize>> {
        let (x, y) = self.pointer?;
        let range = {
            let mut v = self.view.borrow_mut();
            let scroll = v.scroll_y;
            let offsets = v.scroll_offsets.clone();
            v.layout().word_range_at(f, x, y + scroll, &offsets)?
        };
        let doc = self.doc.borrow();
        Some(nile_layout::caret_char(&doc, f, range.start)..nile_layout::caret_char(&doc, f, range.end))
    }

    pub fn mouse_event(&self, kind: &'static str) -> UiEvent {
        let (x, y) = self.pointer.unwrap_or_default();
        UiEvent::Mouse { kind, x, y, button: 0 }
    }

    /// Primary button down: the hovered element becomes active and focus moves
    /// to its nearest focusable ancestor (or is cleared).
    pub fn press(&mut self) -> bool {
        if let Some((x, y)) = self.pointer {
            if self.scrollbar_press(x, y) {
                return true;
            }
        }
        // While a drop-down is open the click is decided on release.
        if self.view.borrow().menu.is_some() {
            return false;
        }
        let target = self.interaction().hover;
        self.pressed = target;
        let mut changed = false;
        // A script that cancels `mousedown` keeps the focus and selection where they are (how
        // toolbar buttons avoid taking them from an editor).
        let mut keep_focus = false;
        if let Some(t) = target {
            let (proceed, c) = self.emit(t, self.mouse_event("mousedown"));
            changed |= c;
            keep_focus = !proceed;
        }
        let next = InteractionState {
            active: target,
            focus: if keep_focus { self.interaction().focus } else { target.and_then(|n| self.focusable(n)) },
            ..self.interaction()
        };
        changed |= self.set_interaction(next);
        if keep_focus {
            return changed;
        }
        // Pressing plain page content (not a control) drops an editing selection.
        if self.interaction().focus.is_none() && self.view.borrow_mut().edit_sel.take().is_some() {
            changed = true;
        }
        // Clicking inside a text input places the caret; shift extends the selection,
        // a double-click selects a word and a triple-click everything.
        let spin = self.interaction().focus.filter(|f| self.is_number(*f)).and_then(|f| Some((f, self.number_arrow_at_pointer(f)?)));
        if let Some((f, dir)) = spin {
            self.number_step(f, dir);
            changed = true;
        } else if let Some(f) = self.interaction().focus.filter(|f| self.is_text_input(*f)) {
            let clicks = self.register_click();
            if let Some(c) = self.char_at_pointer(f) {
                let len = self.value_of(f).chars().count();
                let before = (self.anchor(), self.caret());
                match clicks {
                    1 if self.mods.shift_key() => {
                        let a = self.anchor();
                        self.set_sel(a, c);
                        self.drag = Some(f);
                    }
                    1 => {
                        self.set_sel(c, c);
                        self.drag = Some(f);
                    }
                    2 => {
                        let word = self.word_at_pointer(f).unwrap_or(c..c);
                        self.set_sel(word.start, word.end);
                    }
                    _ => self.set_sel(0, len),
                }
                changed |= before != (self.anchor(), self.caret());
            }
        }
        if let Some(host) = self.focused_host() {
            changed |= self.begin_edit_selection(host);
        }
        if let Some(f) = self.interaction().focus.filter(|f| self.is_range(*f) && !self.is_disabled(*f)) {
            self.range_drag = Some((f, self.range_value(f).to_string()));
            changed |= self.range_from_pointer(f);
        }
        if self.interaction().focus.is_some_and(|f| self.is_text_input(f)) {
            self.view.borrow_mut().page_sel = None;
        } else if self.interaction().focus.is_none() {
            changed |= self.begin_page_selection();
        }
        self.show_caret();
        changed
    }

    /// The page text position nearest the pointer.
    pub fn page_text_point(&mut self) -> Option<TextPoint> {
        let (x, y) = self.pointer?;
        let mut v = self.view.borrow_mut();
        let scroll = v.scroll_y;
        let offsets = v.scroll_offsets.clone();
        v.layout().text_point_at(x, y + scroll, &offsets)
    }

    /// A press on plain page content: start (or, with shift, extend) a text selection;
    /// double-click picks a word and triple-click a whole run.
    pub fn begin_page_selection(&mut self) -> bool {
        let clicks = self.register_click();
        // The press has to land on something selectable, not merely near some text.
        let hit_selectable = self
            .interaction()
            .hover
            .is_some_and(|n| self.view.borrow().styles.get(n).user_select != nile_style::UserSelect::None);
        let point = if hit_selectable { self.page_text_point() } else { None };
        let Some(p) = point else {
            let had = self.view.borrow_mut().page_sel.take().is_some();
            return had;
        };
        let shift = self.mods.shift_key();
        let mut v = self.view.borrow_mut();
        let current = v.page_sel.filter(|s| s.version == v.doc.borrow().version);
        let whole = v.layout().selects_whole_run(p.run);
        match (clicks, current) {
            (_, _) if whole => {
                if let Some((a, b)) = v.layout().word_at(p, true) {
                    v.set_page_sel(a, b);
                }
            }
            (1, Some(s)) if shift => v.set_page_sel(s.anchor, p),
            (1, _) => {
                v.set_page_sel(p, p);
                drop(v);
                self.page_drag = true;
                return true;
            }
            (n, _) => {
                if let Some((a, b)) = v.layout().word_at(p, n >= 3) {
                    v.set_page_sel(a, b);
                }
            }
        }
        true
    }

    /// Keys for the page selection when no input is being edited: copy and select all.
    pub fn page_key(&mut self, key: &str, mods: ModifiersState) -> bool {
        if self.interaction().focus.is_some_and(|f| self.is_text_input(f)) || self.focused_host().is_some() || !(mods.control_key() || mods.super_key()) {
            return false;
        }
        if key.eq_ignore_ascii_case("c") {
            let text = self.view.borrow_mut().page_selected_text();
            if text.is_empty() {
                return false;
            }
            self.clipboard.set_text(&text);
            return true;
        }
        if key.eq_ignore_ascii_case("a") {
            let mut v = self.view.borrow_mut();
            let Some((a, b)) = v.layout().text_extent() else { return false };
            v.set_page_sel(a, b);
            return true;
        }
        false
    }

    /// Primary button up: `mouseup`, then `click` on the common ancestor of the
    /// pressed and released elements.
    pub fn release(&mut self) -> bool {
        if self.bar_drag.take().is_some() {
            return true;
        }
        self.drag = None;
        self.page_drag = false;
        if self.edit_drag.take().is_some() {
            let next = InteractionState { active: None, ..self.interaction() };
            self.set_interaction(next);
            return true;
        }
        if let Some((node, at_press)) = self.range_drag.take() {
            if self.range_value(node).to_string() != at_press {
                self.emit(node, UiEvent::Plain("change"));
            }
            let next = InteractionState { active: None, ..self.interaction() };
            self.set_interaction(next);
            return true;
        }
        if self.view.borrow().menu.is_some() {
            return self.menu_release();
        }
        let mut changed = false;
        let target = self.interaction().hover;
        if let Some(t) = target {
            changed |= self.emit(t, self.mouse_event("mouseup")).1;
        }
        if let (Some(down), Some(up)) = (self.pressed.take(), target) {
            if let Some(common) = self.common_ancestor(down, up) {
                let (proceed, c) = self.emit(common, self.mouse_event("click"));
                changed |= c;
                if proceed {
                    changed |= self.click_default(common);
                }
            }
        }
        let next = InteractionState { active: None, ..self.interaction() };
        changed | self.set_interaction(next)
    }

    /// Make the caret visible and restart its blink (after typing, clicking, ...).
    pub fn show_caret(&mut self) {
        self.caret_visible = true;
        self.blink_at = Instant::now() + CARET_BLINK;
        self.reveal_caret();
    }

    /// Flip the caret when its blink is due. Returns whether the page changed
    /// and when the next flip is due (only while a text input has focus).
    pub fn blink(&mut self) -> (bool, Option<Duration>) {
        let editing = self.interaction().focus.is_some_and(|f| self.is_text_input(f)) || self.focused_host().is_some();
        if !editing {
            self.caret_visible = true;
            return (false, None);
        }
        let now = Instant::now();
        let mut changed = false;
        if now >= self.blink_at {
            self.caret_visible = !self.caret_visible;
            self.blink_at = now + CARET_BLINK;
            changed = true;
        }
        (changed, Some(self.blink_at.saturating_duration_since(now)))
    }

    /// Timers and caret blink together: whether to repaint, and when to wake next.
    pub fn tick(&mut self) -> (bool, Option<Duration>) {
        let (c1, n1) = self.run_timers();
        let (c2, n2) = self.blink();
        let (c3, n3) = self.animate();
        (c1 | c2 | c3, [n1, n2, n3].into_iter().flatten().min())
    }

    /// Step transitions and animations; wake again in a frame while any is running.
    pub fn animate(&mut self) -> (bool, Option<Duration>) {
        let (changed, events, active) = {
            let mut v = self.view.borrow_mut();
            let changed = v.animate();
            (changed, v.animator.take_events(), v.animator.is_active())
        };
        let fired = !events.is_empty();
        for e in events {
            self.emit(e.node, UiEvent::Animation { kind: e.kind, name: e.name, elapsed: e.elapsed });
        }
        (changed | fired, active.then_some(Duration::from_millis(16)))
    }

    /// Elements Tab visits: positive `tabindex` first (ascending), then the
    /// rest in document order. Skips disabled, hidden and `tabindex="-1"`.
    pub fn tab_order(&mut self) -> Vec<NodeId> {
        let doc = self.doc.borrow();
        let mut view = self.view.borrow_mut();
        let tree = view.layout();
        let mut positive: Vec<(i32, NodeId)> = Vec::new();
        let mut rest = Vec::new();
        for id in doc.descendants(Document::ROOT) {
            let n = doc.node(id);
            let natural = match n.tag() {
                Some("a") => n.attr("href").is_some(),
                Some("input") => n.attr("type").is_none_or(|t| !t.eq_ignore_ascii_case("hidden")),
                Some("button" | "select" | "textarea") => true,
                _ => false,
            };
            let tabindex = n.attr("tabindex").and_then(|t| t.trim().parse::<i32>().ok());
            if tabindex.is_some_and(|t| t < 0) || !(natural || tabindex.is_some()) {
                continue;
            }
            if doc.has_state(id, "disabled") || tree.node_rect(id).is_none() {
                continue;
            }
            match tabindex {
                Some(t) if t > 0 => positive.push((t, id)),
                _ => rest.push(id),
            }
        }
        positive.sort_by_key(|(t, _)| *t);
        positive.into_iter().map(|(_, id)| id).chain(rest).collect()
    }

    /// Tab (or Shift+Tab) to the next focusable element, wrapping around.
    pub fn focus_next(&mut self, forward: bool) -> bool {
        let order = self.tab_order();
        if order.is_empty() {
            return false;
        }
        let at = self.interaction().focus.and_then(|f| order.iter().position(|n| *n == f));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => order.len() - 1,
            (Some(i), true) => (i + 1) % order.len(),
            (Some(i), false) => (i + order.len() - 1) % order.len(),
        };
        let state = InteractionState { focus: Some(order[next]), ..self.interaction() };
        let changed = self.set_interaction(state);
        // Tabbing into a text input selects its contents, as browsers do.
        if self.is_text_input(order[next]) {
            let len = self.value_of(order[next]).chars().count();
            self.set_sel(0, len);
        }
        changed
    }

    /// Default action for a key the page didn't cancel: Tab moves focus,
    /// anything else edits the focused text input.
    pub fn default_key(&mut self, key: &str, mods: ModifiersState) -> bool {
        if self.view.borrow().menu.is_none() && self.page_key(key, mods) {
            return true;
        }
        let changed = if self.view.borrow().menu.is_some() {
            self.menu_key(key)
        } else if key == "Tab" {
            self.focus_next(!mods.shift_key())
        } else if self.interaction().focus.is_some_and(|f| self.is_select(f)) {
            self.select_key(key, mods)
        } else if let Some(f) = self.interaction().focus.filter(|f| self.is_range(*f)) {
            self.range_key(f, key)
        } else if let Some(host) = self.focused_host().filter(|_| key != "Tab") {
            self.edit_dom_key(host, key, mods)
        } else {
            let editing = self.interaction().focus.is_some_and(|f| self.is_text_input(f));
            self.edit_key(key, mods) || (!editing && self.scroll_key(key, mods))
        };
        if changed {
            self.show_caret();
        }
        changed
    }

    pub fn is_select(&self, id: NodeId) -> bool {
        self.doc.borrow().node(id).tag() == Some("select")
    }

    /// The nearest `<select>` at or above `id`.
    pub fn select_at_or_above(&self, id: NodeId) -> Option<NodeId> {
        let doc = self.doc.borrow();
        let mut cur = Some(id);
        while let Some(n) = cur {
            if doc.node(n).tag() == Some("select") {
                return Some(n);
            }
            cur = doc.node(n).parent;
        }
        None
    }

    pub fn open_menu(&mut self, select: NodeId) {
        let selected = nile_layout::selected_index(&self.doc.borrow(), select);
        self.view.borrow_mut().set_menu(Some(MenuRequest { select, highlight: selected, scroll: 0.0 }));
        if let Some(row) = selected {
            self.reveal_menu_row(row);
        }
    }

    pub fn close_menu(&mut self) {
        if self.view.borrow().menu.is_some() {
            self.view.borrow_mut().set_menu(None);
        }
    }

    /// Options of the open menu's select as (node, disabled).
    pub fn menu_options(&self) -> Vec<(NodeId, bool)> {
        let Some(m) = self.view.borrow().menu else { return Vec::new() };
        let doc = self.doc.borrow();
        nile_layout::select_options(&doc, m.select).into_iter().map(|o| (o, doc.has_state(o, "disabled"))).collect()
    }

    /// The menu row at a viewport point, if the point is inside the menu.
    pub fn menu_row_at(&mut self, x: f32, y: f32) -> Option<Option<usize>> {
        let mut v = self.view.borrow_mut();
        let y = y + v.scroll_y;
        let menu = v.layout().menu.as_ref()?;
        menu.contains(x, y).then(|| menu.row_at(x, y))
    }

    pub fn set_highlight(&mut self, row: Option<usize>) -> bool {
        let mut v = self.view.borrow_mut();
        match v.menu {
            Some(m) if m.highlight != row => {
                v.set_menu(Some(MenuRequest { highlight: row, ..m }));
                true
            }
            _ => false,
        }
    }

    /// Scroll the open drop-down's list by `dy` pixels.
    pub fn scroll_menu(&mut self, dy: f32) {
        let mut v = self.view.borrow_mut();
        let Some(m) = v.menu else { return };
        let Some(max) = v.layout().menu.as_ref().map(|b| b.max_scroll) else { return };
        let scroll = (m.scroll + dy).clamp(0.0, max);
        if scroll != m.scroll {
            v.set_menu(Some(MenuRequest { scroll, ..m }));
        }
    }

    /// Scroll the open drop-down just enough to show `row`.
    pub fn reveal_menu_row(&mut self, row: usize) {
        let mut v = self.view.borrow_mut();
        let Some(m) = v.menu else { return };
        let Some((top, bottom, view, scroll)) = v.layout().menu.as_ref().map(|b| {
            (b.row_height * row as f32, b.row_height * (row + 1) as f32, b.rect.height - 2.0, b.scroll)
        }) else {
            return;
        };
        let want = if top < scroll { top } else if bottom > scroll + view { bottom - view } else { scroll };
        if want != scroll {
            v.set_menu(Some(MenuRequest { scroll: want, ..m }));
        }
    }

    /// Pointer over the open menu: highlight the enabled row under it.
    pub fn menu_hover(&mut self, x: f32, y: f32) -> bool {
        let options = self.menu_options();
        match self.menu_row_at(x, y) {
            Some(Some(row)) if !options.get(row).is_some_and(|o| o.1) => self.set_highlight(Some(row)),
            _ => false,
        }
    }

    /// Button released with the menu open: pick the row under the pointer, or close.
    pub fn menu_release(&mut self) -> bool {
        let Some(select) = self.view.borrow().menu.map(|m| m.select) else { return false };
        let (x, y) = self.pointer.unwrap_or_default();
        let options = self.menu_options();
        match self.menu_row_at(x, y) {
            Some(Some(row)) if !options.get(row).is_some_and(|o| o.1) => {
                self.choose_option(select, row);
            }
            // On a disabled row: stay open.
            Some(_) => return false,
            None => self.close_menu(),
        }
        true
    }

    /// Make option `index` of `select` the choice, firing `input` and `change` if it differs.
    pub fn choose_option(&mut self, select: NodeId, index: usize) {
        let before = nile_layout::selected_index(&self.doc.borrow(), select);
        {
            let mut doc = self.doc.borrow_mut();
            for (i, o) in nile_layout::select_options(&doc, select).into_iter().enumerate() {
                doc.set_state(o, "selected", i == index);
                if i != index {
                    doc.remove_attr(o, "selected");
                }
            }
        }
        self.close_menu();
        self.sync();
        if before != Some(index) {
            self.emit(select, UiEvent::Plain("input"));
            self.emit(select, UiEvent::Plain("change"));
        }
    }

    /// Options of a list box as (node, disabled).
    pub fn list_options(&self, select: NodeId) -> Vec<(NodeId, bool)> {
        let doc = self.doc.borrow();
        nile_layout::select_options(&doc, select).into_iter().map(|o| (o, doc.has_state(o, "disabled"))).collect()
    }

    /// Make exactly the options for which `want` is true the selected ones, firing
    /// `input` and `change` if that differs from now. True if anything changed.
    pub fn set_list_selection(&mut self, select: NodeId, want: impl Fn(usize) -> bool) -> bool {
        let mut changed = false;
        {
            let mut doc = self.doc.borrow_mut();
            for (i, o) in nile_layout::select_options(&doc, select).into_iter().enumerate() {
                let on = want(i);
                if doc.has_state(o, "selected") != on {
                    changed = true;
                    doc.set_state(o, "selected", on);
                }
                if !on {
                    doc.remove_attr(o, "selected");
                }
            }
        }
        if changed {
            self.sync();
            self.emit(select, UiEvent::Plain("input"));
            self.emit(select, UiEvent::Plain("change"));
        }
        changed
    }

    /// Scroll a list box just enough to show `row`.
    pub fn reveal_list_row(&mut self, select: NodeId, row: usize) {
        let mut v = self.view.borrow_mut();
        let Some((rh, view, scroll)) = v.layout().list_box(select).map(|l| (l.row_height, l.view_height, l.scroll)) else {
            return;
        };
        let (top, bottom) = (rh * row as f32, rh * (row + 1) as f32);
        let want = if top < scroll { top } else if bottom > scroll + view { bottom - view } else { scroll };
        if want != scroll {
            v.set_element_scroll(select, 0.0, want);
        }
    }

    /// A click on a list box: select the row under the pointer (Cmd/Ctrl toggles, Shift extends).
    pub fn list_click(&mut self, select: NodeId) -> bool {
        let Some((_, y)) = self.pointer else { return false };
        let row = {
            let mut v = self.view.borrow_mut();
            let offsets = v.scroll_offsets.clone();
            let top = v.scroll_y;
            let Some(rect) = v.layout().node_rect_scrolled(select, &offsets) else { return false };
            v.layout().list_box(select).and_then(|l| l.row_at(y + top - rect.y))
        };
        let Some(row) = row else { return false };
        if self.list_options(select)[row].1 {
            return false;
        }
        let multiple = self.doc.borrow().node(select).attr("multiple").is_some();
        let (shift, toggle) = (self.mods.shift_key(), self.mods.control_key() || self.mods.super_key());
        let anchor = self.list_anchor.filter(|a| a.0 == select).map_or(row, |a| a.1);
        let before: Vec<bool> = {
            let doc = self.doc.borrow();
            nile_layout::select_options(&doc, select).iter().map(|o| doc.has_state(*o, "selected")).collect()
        };
        let disabled: Vec<bool> = self.list_options(select).iter().map(|o| o.1).collect();
        let changed = if multiple && shift {
            let (lo, hi) = (anchor.min(row), anchor.max(row));
            self.set_list_selection(select, |i| i >= lo && i <= hi && !disabled[i])
        } else if multiple && toggle {
            self.list_anchor = Some((select, row));
            self.set_list_selection(select, |i| if i == row { !before[i] } else { before[i] })
        } else {
            self.list_anchor = Some((select, row));
            self.set_list_selection(select, |i| i == row)
        };
        self.list_cursor = Some((select, row));
        changed
    }

    /// Keys in a focused list box: arrows, Home/End, Page Up/Down and type-ahead move the
    /// selection (Shift extends it in a `multiple` list); Cmd/Ctrl+A selects everything.
    pub fn list_key(&mut self, select: NodeId, key: &str, mods: ModifiersState) -> bool {
        let options = self.list_options(select);
        let multiple = self.doc.borrow().node(select).attr("multiple").is_some();
        if multiple && key.eq_ignore_ascii_case("a") && (mods.control_key() || mods.super_key()) {
            let disabled: Vec<bool> = options.iter().map(|o| o.1).collect();
            return self.set_list_selection(select, |i| !disabled[i]);
        }
        let current = self
            .list_cursor
            .filter(|c| c.0 == select)
            .map(|c| c.1)
            .or_else(|| options.iter().position(|o| self.doc.borrow().has_state(o.0, "selected")));
        let page = self.view.borrow_mut().layout().list_box(select).map_or(1, |l| (l.view_height / l.row_height) as i32);
        let target = match key {
            "ArrowDown" => self.step_option(current, 1, &options),
            "ArrowUp" => self.step_option(current, -1, &options),
            "PageDown" => (0..page.max(1)).fold(current, |c, _| self.step_option(c, 1, &options)),
            "PageUp" => (0..page.max(1)).fold(current, |c, _| self.step_option(c, -1, &options)),
            "Home" => self.step_option(None, 1, &options),
            "End" => self.step_option(None, -1, &options),
            k if k.chars().count() == 1 && !mods.control_key() && !mods.super_key() => {
                self.find_option_starting(select, current, k.chars().next().unwrap())
            }
            _ => return false,
        };
        let Some(row) = target else { return false };
        let anchor = self.list_anchor.filter(|a| a.0 == select).map_or(row, |a| a.1);
        let changed = if multiple && mods.shift_key() {
            let (lo, hi) = (anchor.min(row), anchor.max(row));
            let disabled: Vec<bool> = options.iter().map(|o| o.1).collect();
            self.set_list_selection(select, |i| i >= lo && i <= hi && !disabled[i])
        } else {
            self.list_anchor = Some((select, row));
            self.set_list_selection(select, |i| i == row)
        };
        self.list_cursor = Some((select, row));
        self.reveal_list_row(select, row);
        changed || Some(row) != current
    }

    /// Next enabled option from `from` in direction `dir`, stopping at the ends.
    pub fn step_option(&self, from: Option<usize>, dir: i32, options: &[(NodeId, bool)]) -> Option<usize> {
        let mut i = from.map_or(if dir > 0 { -1 } else { options.len() as i32 }, |f| f as i32);
        loop {
            i += dir;
            if i < 0 || i >= options.len() as i32 {
                return from;
            }
            if !options[i as usize].1 {
                return Some(i as usize);
            }
        }
    }

    /// First enabled option after `from` whose label starts with `c` (type-ahead).
    pub fn find_option_starting(&self, select: NodeId, from: Option<usize>, c: char) -> Option<usize> {
        let doc = self.doc.borrow();
        let options = nile_layout::select_options(&doc, select);
        let n = options.len();
        (1..=n).map(|k| (from.map_or(0, |f| f + k)) % n.max(1)).find(|&i| {
            !doc.has_state(options[i], "disabled")
                && nile_layout::option_label(&doc, options[i]).chars().next().is_some_and(|f| f.eq_ignore_ascii_case(&c))
        })
    }

    /// Keys while the drop-down is open.
    pub fn menu_key(&mut self, key: &str) -> bool {
        let Some(m) = self.view.borrow().menu else { return false };
        let options = self.menu_options();
        match key {
            "ArrowDown" | "ArrowUp" => {
                let next = self.step_option(m.highlight, if key == "ArrowDown" { 1 } else { -1 }, &options);
                self.highlight_and_reveal(next)
            }
            "Home" => {
                let first = self.step_option(None, 1, &options);
                self.highlight_and_reveal(first)
            }
            "End" => {
                let last = self.step_option(None, -1, &options);
                self.highlight_and_reveal(last)
            }
            "Enter" | " " => {
                if let Some(row) = m.highlight {
                    self.choose_option(m.select, row);
                }
                true
            }
            "Escape" | "Tab" => {
                self.close_menu();
                true
            }
            k if k.chars().count() == 1 => match self.find_option_starting(m.select, m.highlight, k.chars().next().unwrap()) {
                Some(i) => self.highlight_and_reveal(Some(i)),
                None => false,
            },
            _ => false,
        }
    }

    pub fn highlight_and_reveal(&mut self, row: Option<usize>) -> bool {
        let changed = self.set_highlight(row);
        if let Some(r) = row {
            self.reveal_menu_row(r);
        }
        changed
    }

    /// Keys on a focused, closed `<select>`: arrows change the choice, space/Enter open it.
    pub fn select_key(&mut self, key: &str, mods: ModifiersState) -> bool {
        let Some(select) = self.interaction().focus.filter(|f| self.is_select(*f)) else { return false };
        if self.is_disabled(select) {
            return false;
        }
        if nile_layout::list_rows(&self.doc.borrow(), select).is_some() {
            return self.list_key(select, key, mods);
        }
        let options: Vec<(NodeId, bool)> = {
            let doc = self.doc.borrow();
            nile_layout::select_options(&doc, select).into_iter().map(|o| (o, doc.has_state(o, "disabled"))).collect()
        };
        let current = nile_layout::selected_index(&self.doc.borrow(), select);
        let target = match key {
            "ArrowDown" if mods.alt_key() => {
                self.open_menu(select);
                return true;
            }
            " " | "Enter" => {
                self.open_menu(select);
                return true;
            }
            "ArrowDown" => self.step_option(current, 1, &options),
            "ArrowUp" => self.step_option(current, -1, &options),
            "Home" => self.step_option(None, 1, &options),
            "End" => self.step_option(None, -1, &options),
            k if k.chars().count() == 1 && !mods.control_key() && !mods.super_key() => {
                self.find_option_starting(select, current, k.chars().next().unwrap())
            }
            _ => return false,
        };
        match target {
            Some(i) if Some(i) != current => {
                self.choose_option(select, i);
                true
            }
            _ => false,
        }
    }

    pub fn value_of(&self, id: NodeId) -> String {
        nile_layout::control_value(&self.doc.borrow(), id)
    }

    pub fn is_textarea(&self, id: NodeId) -> bool {
        self.doc.borrow().node(id).tag() == Some("textarea")
    }

    pub fn is_text_input(&self, id: NodeId) -> bool {
        let doc = self.doc.borrow();
        let n = doc.node(id);
        n.tag() == Some("textarea")
            || n.tag() == Some("input")
                && !matches!(
                    n.attr("type").map(str::to_ascii_lowercase).as_deref(),
                    Some("checkbox" | "radio" | "hidden" | "file" | "image" | "button" | "submit" | "reset" | "range")
                )
    }

    pub fn is_disabled(&self, id: NodeId) -> bool {
        self.doc.borrow().has_state(id, "disabled")
    }

    /// Default action of a click on `target`: toggle a checkbox, select a
    /// radio, or forward a `<label>` click to its control. True if the page changed.
    pub fn click_default(&mut self, target: NodeId) -> bool {
        // A click on a <summary> opens or closes its <details>.
        if let Some(details) = self.summary_details(target) {
            {
                let mut doc = self.doc.borrow_mut();
                if doc.node(details).attr("open").is_some() {
                    doc.remove_attr(details, "open");
                } else {
                    doc.set_attr(details, "open", "");
                }
            }
            self.sync();
            self.emit(details, UiEvent::Plain("toggle"));
            return true;
        }
        // Elements acting as window buttons.
        let role = window::role_at(&self.doc.borrow(), target).map(|r| r.1);
        let cmd = match role {
            Some(Role::Minimize) => Some(WindowCommand::Minimize),
            Some(Role::Maximize) => Some(WindowCommand::ToggleMaximize),
            Some(Role::Close) => Some(WindowCommand::Close),
            _ => None,
        };
        if let Some(cmd) = cmd {
            self.view.borrow_mut().window_requests.push(cmd);
            return false;
        }
        // The nearest control or label at or above the click.
        let found = {
            let doc = self.doc.borrow();
            let mut cur = Some(target);
            let mut found = None;
            while let Some(n) = cur {
                let node = doc.node(n);
                let ty = node.attr("type").map(str::to_ascii_lowercase);
                match node.tag() {
                    Some("input") if matches!(ty.as_deref(), Some("checkbox" | "radio")) => {
                        found = Some((n, false));
                        break;
                    }
                    Some("label") => {
                        found = Some((n, true));
                        break;
                    }
                    _ => {}
                }
                cur = node.parent;
            }
            found
        };
        // A click on a <select> opens its drop-down.
        if let Some(select) = self.select_at_or_above(target) {
            if nile_layout::list_rows(&self.doc.borrow(), select).is_some() {
                return !self.is_disabled(select) && self.list_click(select);
            }
            if !self.is_disabled(select) {
                self.open_menu(select);
                return true;
            }
            return false;
        }
        let Some((n, is_label)) = found else { return false };
        if is_label {
            let control = {
                let doc = self.doc.borrow();
                match doc.node(n).attr("for") {
                    Some(id) => doc.descendants(Document::ROOT).into_iter().find(|c| doc.node(*c).attr("id") == Some(id)),
                    None => doc.descendants(n).into_iter().find(|c| doc.node(*c).tag() == Some("input")),
                }
            };
            let Some(control) = control else { return false };
            // A click on the control itself already toggled it.
            let inside = {
                let doc = self.doc.borrow();
                let mut cur = Some(target);
                let mut hit = false;
                while let Some(c) = cur {
                    if c == control {
                        hit = true;
                        break;
                    }
                    cur = doc.node(c).parent;
                }
                hit
            };
            if inside {
                return false;
            }
            let (proceed, mut changed) = self.emit(control, self.mouse_event("click"));
            if proceed {
                changed |= self.click_default(control);
            }
            return changed;
        }
        if self.is_disabled(n) {
            return false;
        }
        let (is_radio, was_checked, name) = {
            let doc = self.doc.borrow();
            let node = doc.node(n);
            (
                node.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("radio")),
                doc.has_state(n, "checked"),
                node.attr("name").map(str::to_string),
            )
        };
        if is_radio {
            if was_checked {
                return false;
            }
            // Uncheck the rest of the group: same name under the same form (or the page).
            let group: Vec<NodeId> = {
                let doc = self.doc.borrow();
                let form = {
                    let mut cur = doc.node(n).parent;
                    while let Some(p) = cur {
                        if doc.node(p).tag() == Some("form") {
                            break;
                        }
                        cur = doc.node(p).parent;
                    }
                    cur.unwrap_or(Document::ROOT)
                };
                match &name {
                    Some(name) => doc
                        .descendants(form)
                        .into_iter()
                        .filter(|c| *c != n && doc.node(*c).tag() == Some("input") && doc.node(*c).attr("name") == Some(name.as_str()))
                        .collect(),
                    None => Vec::new(),
                }
            };
            let mut doc = self.doc.borrow_mut();
            for other in group {
                doc.set_state(other, "checked", false);
                doc.remove_attr(other, "checked");
            }
            doc.set_state(n, "checked", true);
        } else {
            let mut doc = self.doc.borrow_mut();
            doc.set_state(n, "checked", !was_checked);
            if was_checked {
                doc.remove_attr(n, "checked");
            }
        }
        self.sync();
        self.emit(n, UiEvent::Plain("input"));
        self.emit(n, UiEvent::Plain("change"));
        true
    }

    /// Edit the focused text input for a key press: typing, deleting, caret and
    /// selection movement, select-all, copy/cut/paste. True if the page needs repainting.
    pub fn edit_key(&mut self, key: &str, mods: ModifiersState) -> bool {
        let Some(focus) = self.interaction().focus.filter(|f| self.is_text_input(*f)) else { return false };
        let readonly = self.is_disabled(focus) || self.doc.borrow().node(focus).attr("readonly").is_some();
        let multiline = self.is_textarea(focus);
        let (command, shift) = (mods.control_key() || mods.super_key(), mods.shift_key());
        let word_mod = mods.alt_key() || (mods.control_key() && !cfg!(target_os = "macos"));
        let mut chars: Vec<char> = self.value_of(focus).chars().collect();
        let len = chars.len();
        let (anchor, caret) = (self.anchor().min(len), self.caret().min(len));
        let (lo, hi) = (anchor.min(caret), anchor.max(caret));
        let selected = |chars: &[char]| chars[lo..hi].iter().collect::<String>();

        // Either move the caret (extending the selection with shift)...
        let mut target: Option<usize> = None;
        // ...or replace chars[from..to] with some text.
        let mut edit: Option<(usize, usize, String)> = None;
        match key {
            k if command && k.eq_ignore_ascii_case("a") => {
                self.set_sel(0, len);
                return true;
            }
            k if command && k.eq_ignore_ascii_case("c") => {
                if lo != hi {
                    let text = selected(&chars);
                    self.clipboard.set_text(&text);
                }
                return false;
            }
            k if command && k.eq_ignore_ascii_case("x") => {
                if lo != hi && !readonly {
                    let text = selected(&chars);
                    self.clipboard.set_text(&text);
                    edit = Some((lo, hi, String::new()));
                }
            }
            k if command && k.eq_ignore_ascii_case("v") => {
                if let (false, Some(text)) = (readonly, self.clipboard.text()) {
                    let text = if multiline { text.replace("\r\n", "\n") } else { text.replace("\r\n", " ").replace(['\r', '\n'], " ") };
                    let text = if self.is_number(focus) { text.chars().filter(|c| "0123456789+-.eE".contains(*c)).collect() } else { text };
                    edit = Some((lo, hi, text));
                }
            }
            k if command && k.eq_ignore_ascii_case("z") && !readonly => {
                return if shift { self.redo(focus) } else { self.undo(focus) };
            }
            k if mods.control_key() && !cfg!(target_os = "macos") && k.eq_ignore_ascii_case("y") && !readonly => {
                return self.redo(focus);
            }
            // Alt+Backspace / Ctrl+Backspace (and Cmd+Backspace on macOS, to the line start).
            "Backspace" if !readonly && (word_mod || mods.super_key()) && lo == hi && caret > 0 => {
                let from = if mods.super_key() { chars[..caret].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1) } else { word_start(&chars, caret) };
                edit = Some((from, caret, String::new()));
            }
            "Delete" if !readonly && word_mod && lo == hi && caret < len => edit = Some((caret, word_end(&chars, caret), String::new())),
            "Backspace" if !readonly => {
                if lo != hi {
                    edit = Some((lo, hi, String::new()));
                } else if caret > 0 {
                    edit = Some((caret - 1, caret, String::new()));
                }
            }
            "Delete" if !readonly => {
                if lo != hi {
                    edit = Some((lo, hi, String::new()));
                } else if caret < len {
                    edit = Some((caret, caret + 1, String::new()));
                }
            }
            // Alt (macOS) or Ctrl moves by word; Cmd+Arrow goes to the line's ends.
            "ArrowLeft" if mods.super_key() => target = Some(chars[..caret].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1)),
            "ArrowRight" if mods.super_key() => target = Some(chars[caret..].iter().position(|c| *c == '\n').map_or(len, |i| caret + i)),
            "ArrowLeft" if word_mod => target = Some(word_start(&chars, caret)),
            "ArrowRight" if word_mod => target = Some(word_end(&chars, caret)),
            "ArrowLeft" => target = Some(if !shift && lo != hi { lo } else { caret.saturating_sub(1) }),
            "ArrowRight" => target = Some(if !shift && lo != hi { hi } else { (caret + 1).min(len) }),
            // Multi-line: Home/End go to the line's ends (hard breaks only).
            "Home" => target = Some(chars[..caret].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1)),
            "End" => target = Some(chars[caret..].iter().position(|c| *c == '\n').map_or(len, |i| caret + i)),
            "ArrowUp" | "ArrowDown" if multiline => {
                let up = key == "ArrowUp";
                let moved = {
                    let at = nile_layout::caret_byte(&self.doc.borrow(), focus, caret);
                    self.view.borrow_mut().layout().move_caret_vertically(focus, at, if up { -1 } else { 1 })
                };
                target = Some(match moved {
                    Some(b) => nile_layout::caret_char(&self.doc.borrow(), focus, b),
                    // Past the first or last line: go to the start or end.
                    None if up => 0,
                    None => len,
                });
            }
            "ArrowUp" | "ArrowDown" if !multiline && self.is_number(focus) => {
                return self.number_step(focus, if key == "ArrowUp" { 1.0 } else { -1.0 });
            }
            "Enter" if multiline && !readonly => edit = Some((lo, hi, "\n".into())),
            "Enter" => {
                // Commit: fire `change` if the value moved since focus.
                if self.value_at_focus.as_ref().is_some_and(|v| *v != self.value_of(focus)) {
                    self.value_at_focus = Some(self.value_of(focus));
                    return self.emit(focus, UiEvent::Plain("change")).1 | true;
                }
                return false;
            }
            k if !readonly && !command && k.chars().count() == 1 => {
                let c = k.chars().next().unwrap();
                if !c.is_control() && (!self.is_number(focus) || "0123456789+-.eE".contains(c)) {
                    edit = Some((lo, hi, c.to_string()));
                }
            }
            _ => return false,
        }
        if let Some(t) = target {
            let new_anchor = if shift { anchor } else { t };
            self.set_sel(new_anchor, t);
            return (new_anchor, t) != (anchor, caret);
        }
        let Some((from, to, text)) = edit else { return false };
        let inserted = text.chars().count();
        let kind = match key {
            _ if lo != hi || command => EditKind::Other,
            "Backspace" if !word_mod => EditKind::Backspace,
            "Delete" if !word_mod => EditKind::Delete,
            k if k.chars().count() == 1 && inserted == 1 => EditKind::Insert,
            _ => EditKind::Other,
        };
        self.record_edit(focus, kind, EditState { value: chars.iter().collect(), anchor, caret });
        chars.splice(from..to, text.chars());
        let value: String = chars.iter().collect();
        self.doc.borrow_mut().set_attr(focus, "value", &value);
        self.set_sel(from + inserted, from + inserted);
        if let Some(h) = self.history.get_mut(&focus) {
            h.last = Some((kind, from + inserted));
        }
        self.sync();
        self.emit(focus, UiEvent::Plain("input"));
        true
    }

    /// Remember the state before an edit, unless it continues the previous one.
    pub fn record_edit(&mut self, node: NodeId, kind: EditKind, before: EditState) {
        let h = self.history.entry(node).or_default();
        let continues = kind != EditKind::Other && h.last == Some((kind, before.caret)) && before.anchor == before.caret;
        if !continues {
            h.undo.push(before);
            if h.undo.len() > 200 {
                h.undo.remove(0);
            }
        }
        h.redo.clear();
    }

    pub fn current_state(&self, node: NodeId) -> EditState {
        let len = self.value_of(node).chars().count();
        EditState { value: self.value_of(node), anchor: self.anchor().min(len), caret: self.caret().min(len) }
    }

    pub fn apply_state(&mut self, node: NodeId, state: EditState) -> bool {
        self.doc.borrow_mut().set_attr(node, "value", &state.value);
        self.set_sel(state.anchor, state.caret);
        self.sync();
        self.emit(node, UiEvent::Plain("input"));
        true
    }

    pub fn undo(&mut self, node: NodeId) -> bool {
        let now = self.current_state(node);
        let Some(h) = self.history.get_mut(&node) else { return false };
        let Some(prev) = h.undo.pop() else { return false };
        h.redo.push(now);
        h.last = None;
        self.apply_state(node, prev)
    }

    pub fn redo(&mut self, node: NodeId) -> bool {
        let now = self.current_state(node);
        let Some(h) = self.history.get_mut(&node) else { return false };
        let Some(next) = h.redo.pop() else { return false };
        h.undo.push(now);
        h.last = None;
        self.apply_state(node, next)
    }

    /// Caret to draw: the focused text input and the caret's byte offset in its displayed text.
    pub fn caret_for_paint(&self) -> Option<(NodeId, usize)> {
        let focus = self.interaction().focus.filter(|f| self.is_text_input(*f))?;
        if !self.caret_visible {
            return None;
        }
        let doc = self.doc.borrow();
        Some((focus, nile_layout::caret_byte(&doc, focus, self.caret())))
    }

    /// The focused input's selected range as byte offsets in its displayed text.
    pub fn selection_for_paint(&self) -> Option<(NodeId, usize, usize)> {
        let focus = self.interaction().focus.filter(|f| self.is_text_input(*f))?;
        let len = self.value_of(focus).chars().count();
        let (a, c) = (self.anchor().min(len), self.caret().min(len));
        if a == c {
            return None;
        }
        let doc = self.doc.borrow();
        Some((focus, nile_layout::caret_byte(&doc, focus, a.min(c)), nile_layout::caret_byte(&doc, focus, a.max(c))))
    }

    pub fn common_ancestor(&self, a: NodeId, b: NodeId) -> Option<NodeId> {
        let doc = self.doc.borrow();
        let mut chain = std::collections::HashSet::new();
        let mut cur = Some(a);
        while let Some(n) = cur {
            chain.insert(n);
            cur = doc.node(n).parent;
        }
        let mut cur = Some(b);
        while let Some(n) = cur {
            if chain.contains(&n) {
                return Some(n);
            }
            cur = doc.node(n).parent;
        }
        None
    }

    /// A key event goes to the focused element, or `<body>`. Returns whether
    /// the page should keep its default handling (no `preventDefault`).
    pub fn key(&mut self, kind: &'static str, key: String, mods: ModifiersState) -> (bool, bool) {
        let target = self.interaction().focus.or_else(|| self.doc.borrow().body());
        let Some(target) = target else { return (true, false) };
        self.emit(
            target,
            UiEvent::Key {
                kind,
                key,
                ctrl: mods.control_key(),
                shift: mods.shift_key(),
                alt: mods.alt_key(),
                meta: mods.super_key(),
            },
        )
    }

    pub fn set_interaction(&mut self, next: InteractionState) -> bool {
        let current = self.interaction();
        if next == current {
            return false;
        }
        let (old_focus, new_focus) = (current.focus, next.focus);
        {
            let mut v = self.view.borrow_mut();
            v.interaction = next;
            if old_focus != new_focus {
                if let Some(n) = new_focus {
                    v.select_for_focus(n);
                }
            }
        }
        // A state no selector mentions can't change a style: skip the cascade (and keep the layout).
        if self.view.borrow().styles.state_deps.affected_by(&current, &next) {
            self.restyle();
        }
        if old_focus != new_focus {
            self.focus_moved(old_focus, new_focus);
        }
        true
    }

    /// CSS canvas background: the root element's background, or the body's if
    /// the root has none, fills the whole viewport. Defaults to white.
    pub fn canvas_background(&self) -> Color {
        ["html", "body"]
            .iter()
            .filter_map(|tag| self.doc.borrow().find(|n| n.tag() == Some(tag)))
            .find_map(|id| self.view.borrow().styles.get(id).background)
            .map(|c| Color::from_argb(255, c.r, c.g, c.b))
            .unwrap_or_else(|| {
                // A `window-frame: transparent` window shows the desktop where the page has no background.
                if WindowConfig::from_document(&self.doc.borrow()).frame == Frame::Transparent { Color::TRANSPARENT } else { Color::WHITE }
            })
    }

    /// Paint into a BGRA pixel buffer of `width`×`height` physical pixels.
    pub fn paint(&mut self, pixels: &mut [u8], width: u32, height: u32, scale: f32) {
        self.view.borrow_mut().scale = scale;
        let info = ImageInfo::new(
            (width as i32, height as i32),
            ColorType::BGRA8888,
            AlphaType::Premul,
            None,
        );
        let row_bytes = width as usize * 4;
        let Some(mut surface) = surfaces::wrap_pixels(&info, pixels, row_bytes, None) else {
            return;
        };

        let (lw, lh) = (width as f32 / scale, height as f32 / scale);
        self.layout(lw, lh);
        let bg = self.canvas_background();
        let caret = self.caret_for_paint();
        let selection = self.selection_for_paint();
        let dom_caret = self.dom_caret_for_paint();
        let mut view = self.view.borrow_mut();
        view.layout();
        let page_selection = view.page_ranges();
        let max_scroll = (view.layout().content_height - lh).max(0.0);
        view.scroll_y = view.scroll_y.clamp(0.0, max_scroll);
        let scroll = view.scroll_y;
        let view = &*view;

        let canvas = surface.canvas();
        canvas.clear(bg);
        canvas.save();
        canvas.scale((scale, scale));
        let tree = &view.layout.as_ref().expect("laid out above").2;
        traced("paint", || {
            self.renderer.paint(
                canvas,
                tree,
                &view.styles,
                &Overlay { scroll_y: scroll, offsets: &view.scroll_offsets, caret, selection, viewport: view.viewport, page_selection: &page_selection, dom_caret },
            )
        });
        canvas.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page loaded from a temp file, laid out at 600x300.
    fn page(html: &str, name: &str) -> (Page, PathBuf) {
        let path = std::env::temp_dir().join(format!("nile-shell-{}-{name}.html", std::process::id()));
        std::fs::write(&path, html).unwrap();
        let mut p = Page::load(path.clone(), (600.0, 300.0), 0.0);
        p.layout(600.0, 300.0);
        (p, path)
    }

    fn center(p: &Page, id: &str) -> (f32, f32) {
        let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        let r = p.view.borrow_mut().layout().node_rect(node).unwrap();
        (r.x + r.width / 2.0, r.y + r.height / 2.0)
    }

    fn click(p: &mut Page, id: &str) {
        let (x, y) = center(p, id);
        p.pointer_moved(x, y);
        p.press();
        p.release();
    }

    fn log(p: &Page) -> Vec<String> {
        p.scripts.as_ref().unwrap().console_output()
    }

    #[test]
    fn checkbox_radio_and_label_clicks() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><input id="c" type="checkbox"><label id="lab" for="c">Check</label>
               <input id="r1" type="radio" name="g" checked><input id="r2" type="radio" name="g"><input id="off" type="checkbox" disabled>
               <script>
                 for (const t of ['input', 'change']) document.addEventListener(t, (e) => console.log(t + ':' + e.target.id));
               </script></body>"#,
            "clicks",
        );
        let checked = |p: &Page, id: &str| {
            let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
            p.doc.borrow().has_state(n, "checked")
        };
        click(&mut p, "c");
        assert!(checked(&p, "c"));
        click(&mut p, "lab"); // forwarded to the checkbox: toggles it back
        assert!(!checked(&p, "c"));
        click(&mut p, "r2");
        assert!(checked(&p, "r2") && !checked(&p, "r1"));
        click(&mut p, "r2"); // already selected: no event
        click(&mut p, "off");
        assert!(!checked(&p, "off"));
        assert_eq!(log(&p), ["input:c", "change:c", "input:c", "change:c", "input:r2", "change:r2"]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn typing_edits_value_and_fires_events() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><input id="t" value="ab"><input id="u">
               <script>
                 const t = document.getElementById('t');
                 t.addEventListener('input', () => console.log('input:' + t.value));
                 t.addEventListener('change', () => console.log('change:' + t.value));
                 t.addEventListener('keydown', (e) => { if (e.key === 'q') e.preventDefault(); });
               </script></body>"#,
            "typing",
        );
        let key = |p: &mut Page, k: &str| {
            let (proceed, _) = p.key("keydown", k.to_string(), ModifiersState::default());
            if proceed {
                p.default_key(k, ModifiersState::default());
            }
        };
        let (x, y) = center(&p, "t");
        p.pointer_moved(x + 60.0, y); // inside the input, right of the text: caret goes to the end
        p.press();
        p.release();
        assert_eq!(p.caret(), 2);
        key(&mut p, "c");
        key(&mut p, "q"); // prevented by the page
        key(&mut p, "ArrowLeft");
        key(&mut p, "Backspace");
        key(&mut p, "Home");
        key(&mut p, "Delete");
        assert_eq!(p.value_of(p.interaction().focus.unwrap()), "c");
        key(&mut p, "Enter");
        // Moving focus away fires `change` only if the value changed since the last commit.
        click(&mut p, "u");
        assert_eq!(log(&p), ["input:abc", "input:ac", "input:c", "change:c"]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn script_focus_moves_the_caret_target() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><input id="a" value="xy"><input id="b" value="z">
               <script>
                 $('#a').addEventListener('focus', () => console.log('focus a'));
                 $('#a').addEventListener('blur', () => console.log('blur a'));
                 $('#b').focus();
                 $('#a').focus();
                 console.log(document.activeElement.id);
               </script></body>"#,
            "focus",
        );
        // Focus calls made while scripts load are applied once the page syncs.
        p.sync();
        assert_eq!(p.interaction().focus.map(|f| p.doc.borrow().node(f).attr("id").map(str::to_string)), Some(Some("a".into())));
        assert_eq!(p.caret(), 2);
        assert!(log(&p).contains(&"a".to_string()), "{:?}", log(&p));
        let _ = std::fs::remove_file(path);
    }

    fn focused_id(p: &Page) -> Option<String> {
        p.interaction().focus.and_then(|f| p.doc.borrow().node(f).attr("id").map(str::to_string))
    }

    #[test]
    fn tab_visits_focusable_elements_in_order() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><input id="a"><input id="off" disabled><a id="link" href="#">x</a><a id="plain">y</a>
               <div id="ti" tabindex="0">t</div><div id="skip" tabindex="-1">s</div><input id="hid" type="hidden">
               <button id="late" tabindex="2">b</button><button id="early" tabindex="1">c</button>
               <div style="display:none"><input id="none"></div></body>"##,
            "tab",
        );
        let mut seen = Vec::new();
        for _ in 0..7 {
            p.default_key("Tab", ModifiersState::default());
            seen.push(focused_id(&p).unwrap());
        }
        // Positive tabindex first (ascending), then document order; wraps around.
        assert_eq!(seen, ["early", "late", "a", "link", "ti", "early", "late"]);
        let shift = ModifiersState::SHIFT;
        p.default_key("Tab", shift);
        assert_eq!(focused_id(&p).as_deref(), Some("early"));
        p.default_key("Tab", shift);
        assert_eq!(focused_id(&p).as_deref(), Some("ti"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn caret_blinks_only_while_editing() {
        let (mut p, path) = page(r#"<body style="margin:0"><input id="t"><p id="x">x</p></body>"#, "blink");
        assert_eq!(p.blink(), (false, None), "nothing focused: no blink timer");
        click(&mut p, "t");
        assert!(p.caret_visible);
        assert!(p.caret_for_paint().is_some());
        p.blink_at = Instant::now() - Duration::from_millis(1);
        let (changed, next) = p.blink();
        assert!(changed && !p.caret_visible && next.is_some());
        assert!(p.caret_for_paint().is_none(), "caret hidden in the off phase");
        p.default_key("a", ModifiersState::default());
        assert!(p.caret_visible, "typing shows the caret again");
        click(&mut p, "x");
        assert_eq!(p.blink(), (false, None));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn textarea_editing_with_newlines_and_vertical_moves() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><textarea id="t" style="width:200px">ab
cd</textarea>
               <script>$('#t').addEventListener('input', () => console.log(JSON.stringify($('#t').value)));</script></body>"#,
            "textarea",
        );
        let key = |p: &mut Page, k: &str| {
            p.default_key(k, ModifiersState::default());
        };
        click(&mut p, "t");
        // The click lands at the middle of the box, below the text: caret goes to the end.
        assert_eq!(p.value_of(p.interaction().focus.unwrap()), "ab\ncd");
        key(&mut p, "ArrowUp"); // up from the second line keeps the column
        assert!(p.caret() <= 2, "caret on the first line, got {}", p.caret());
        key(&mut p, "End");
        key(&mut p, "Enter"); // newline after "ab"
        key(&mut p, "X");
        key(&mut p, "ArrowDown");
        key(&mut p, "ArrowDown");
        key(&mut p, "ArrowDown"); // past the last line: end of text
        key(&mut p, "Y");
        assert_eq!(log(&p).last().map(String::as_str), Some("\"ab\\nX\\ncdY\""));
        let _ = std::fs::remove_file(path);
    }

    fn offset(p: &Page, id: &str) -> (f32, f32) {
        let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        p.view.borrow().scroll_offsets.get(&n).copied().unwrap_or_default()
    }

    #[test]
    fn wheel_scrolls_the_innermost_box_then_chains_to_the_page() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="box" style="width:100px; height:50px; overflow:auto">
                 <div style="height:40px">a</div><div style="height:40px">b</div><div style="height:40px">c</div></div>
               <div style="height:600px">tall</div>
               <script>
                 $('#box').addEventListener('scroll', () => console.log('scroll:' + $('#box').scrollTop));
               </script></body>"#,
            "wheel",
        );
        let (x, y) = center(&p, "box");
        p.pointer_moved(x, y);
        p.wheel(0.0, -30.0); // wheel down: scroll content up by 30
        assert_eq!(offset(&p, "box"), (0.0, 30.0));
        p.wheel(0.0, -100.0); // clamps at the end (120 - 50 = 70)
        assert_eq!(offset(&p, "box"), (0.0, 70.0));
        assert_eq!(p.view.borrow().scroll_y, 0.0);
        p.wheel(0.0, -40.0); // box is at its limit: the page takes it
        assert_eq!(offset(&p, "box"), (0.0, 70.0));
        assert_eq!(p.view.borrow().scroll_y, 40.0);
        p.pointer_moved(x, 5.0); // the page moved under the pointer: back over the box
        p.wheel(0.0, 100.0); // wheel up: the box scrolls back first
        assert_eq!(offset(&p, "box"), (0.0, 0.0));
        assert_eq!(log(&p), ["scroll:30", "scroll:70", "scroll:0"]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn scripts_scroll_elements_and_the_caret_stays_visible() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="box" style="width:100px; height:50px; overflow:hidden">
                 <div style="height:200px; width:300px">x</div></div>
               <textarea id="t" style="width:100px; height:40px"></textarea>
               <script>
                 const b = $('#box');
                 b.scrollTop = 80; b.scrollLeft = 500;
                 console.log(b.scrollTop, b.scrollLeft, b.scrollWidth, b.scrollHeight, b.clientHeight);
                 b.scrollBy(-30, 0); b.scrollTo({ top: 5 });
                 console.log(b.scrollLeft, b.scrollTop);
                 console.log(document.createElement('p').scrollTop);
               </script></body>"#,
            "scrolljs",
        );
        assert_eq!(log(&p)[0], "80 200 300 200 50");
        assert_eq!(log(&p)[1], "170 5");
        assert_eq!(log(&p)[2], "0");
        // Typing past the bottom of the textarea scrolls it to keep the caret in view.
        click(&mut p, "t");
        for k in ["a", "Enter", "b", "Enter", "c", "Enter", "d", "Enter", "e"] {
            p.default_key(k, ModifiersState::default());
        }
        assert!(offset(&p, "t").1 > 0.0, "scrolled to the caret: {:?}", offset(&p, "t"));
        p.default_key("Home", ModifiersState::default());
        for _ in 0..4 {
            p.default_key("ArrowUp", ModifiersState::default());
        }
        assert_eq!(offset(&p, "t").1, 0.0, "back at the top");
        let _ = std::fs::remove_file(path);
    }

    /// Press a key (keydown + default action) with modifiers.
    fn press_key(p: &mut Page, k: &str, mods: ModifiersState) {
        let (proceed, _) = p.key("keydown", k.to_string(), mods);
        if proceed {
            p.default_key(k, mods);
        }
    }

    fn val(p: &Page, id: &str) -> String {
        let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        p.value_of(n)
    }

    #[test]
    fn selection_with_keyboard_and_typing_over_it() {
        let (mut p, path) = page(r#"<body style="margin:0"><input id="t" value="hello world"></body>"#, "sel-keys");
        let none = ModifiersState::default();
        click(&mut p, "t"); // caret at the end (click is right of the text)
        press_key(&mut p, "Home", none);
        for _ in 0..5 {
            press_key(&mut p, "ArrowRight", ModifiersState::SHIFT);
        }
        assert_eq!((p.anchor(), p.caret()), (0, 5));
        assert!(p.selection_for_paint().is_some());
        press_key(&mut p, "ArrowRight", none); // collapses to the end of the selection
        assert_eq!((p.anchor(), p.caret()), (5, 5));
        press_key(&mut p, "End", ModifiersState::SHIFT);
        assert_eq!((p.anchor(), p.caret()), (5, 11));
        press_key(&mut p, "X", none); // typing replaces the selection
        assert_eq!(val(&p, "t"), "helloX");
        press_key(&mut p, "ArrowLeft", ModifiersState::SHIFT);
        press_key(&mut p, "Backspace", none); // deletes just the selection
        assert_eq!(val(&p, "t"), "hello");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn undo_redo_groups_typing_and_backspace_runs() {
        let (mut p, path) = page(r#"<body style="margin:0"><input id="t"><input id="u" value="hello"></body>"#, "undo");
        let none = ModifiersState::default();
        let ctrl = ModifiersState::CONTROL;
        click(&mut p, "t");
        for c in ["a", "b", "c"] {
            press_key(&mut p, c, none);
        }
        assert_eq!(val(&p, "t"), "abc");
        press_key(&mut p, "z", ctrl); // the whole run is one step
        assert_eq!(val(&p, "t"), "");
        press_key(&mut p, "z", ctrl | ModifiersState::SHIFT);
        assert_eq!((val(&p, "t").as_str(), p.caret()), ("abc", 3));
        // Moving the caret ends a run.
        press_key(&mut p, "ArrowLeft", none);
        press_key(&mut p, "X", none);
        assert_eq!(val(&p, "t"), "abXc");
        press_key(&mut p, "z", ctrl);
        assert_eq!((val(&p, "t").as_str(), p.caret()), ("abc", 2));
        press_key(&mut p, "z", ctrl);
        assert_eq!(val(&p, "t"), "");
        press_key(&mut p, "z", ctrl); // nothing left to undo
        assert_eq!(val(&p, "t"), "");
        // A new edit drops what could be redone.
        press_key(&mut p, "q", none);
        press_key(&mut p, "z", ctrl | ModifiersState::SHIFT);
        assert_eq!(val(&p, "t"), "q");

        click(&mut p, "u");
        press_key(&mut p, "End", none);
        for _ in 0..3 {
            press_key(&mut p, "Backspace", none);
        }
        assert_eq!(val(&p, "u"), "he");
        press_key(&mut p, "z", ctrl);
        assert_eq!(val(&p, "u"), "hello");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn word_wise_movement_and_deletion() {
        let (mut p, path) = page(r#"<body style="margin:0"><input id="t" value="one two, three"></body>"#, "words");
        let none = ModifiersState::default();
        let alt = ModifiersState::ALT;
        click(&mut p, "t");
        press_key(&mut p, "End", none);
        press_key(&mut p, "ArrowLeft", alt);
        assert_eq!(p.caret(), 9); // start of "three"
        press_key(&mut p, "ArrowLeft", alt);
        assert_eq!(p.caret(), 4);
        press_key(&mut p, "ArrowRight", alt);
        assert_eq!(p.caret(), 7); // end of "two"
        press_key(&mut p, "ArrowRight", alt | ModifiersState::SHIFT);
        assert_eq!((p.anchor(), p.caret()), (7, 14));
        press_key(&mut p, "ArrowLeft", none);
        press_key(&mut p, "End", none);
        press_key(&mut p, "Backspace", alt);
        assert_eq!(val(&p, "t"), "one two, ");
        press_key(&mut p, "Home", none);
        press_key(&mut p, "Delete", alt);
        assert_eq!(val(&p, "t"), " two, ");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn keyboard_scrolls_the_page_and_scrollers() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="box" tabindex="0" style="width:100px; height:80px; overflow:auto"><div style="height:400px">in</div></div>
               <input id="t" value="x"><div style="height:1000px">tall</div></body>"#,
            "keyscroll",
        );
        let none = ModifiersState::default();
        // Nothing focused or hovered: the page scrolls.
        press_key(&mut p, "ArrowDown", none);
        assert_eq!(p.view.borrow().scroll_y, 40.0);
        press_key(&mut p, "PageDown", none); // 0.875 of the 300px viewport
        assert_eq!(p.view.borrow().scroll_y, 40.0 + 262.5);
        press_key(&mut p, " ", ModifiersState::SHIFT);
        assert_eq!(p.view.borrow().scroll_y, 40.0);
        press_key(&mut p, "End", none);
        let end = p.view.borrow().scroll_y;
        assert!(end > 700.0 && end < 900.0, "{end}");
        press_key(&mut p, "ArrowDown", none); // already at the end
        assert_eq!(p.view.borrow().scroll_y, end);
        press_key(&mut p, "Home", none);
        assert_eq!(p.view.borrow().scroll_y, 0.0);
        // A focused scroller takes the keys first.
        click(&mut p, "box");
        press_key(&mut p, "PageDown", none);
        assert_eq!(offset(&p, "box"), (0.0, 70.0));
        assert_eq!(p.view.borrow().scroll_y, 0.0);
        // In a text input the keys are for editing.
        click(&mut p, "t");
        press_key(&mut p, "ArrowDown", none);
        assert_eq!(p.view.borrow().scroll_y, 0.0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn scrollbars_drag_and_jump() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="box" style="width:100px; height:80px; overflow:auto"><div style="height:400px">in</div></div>
               <div style="height:1000px">tall</div></body>"#,
            "bars",
        );
        // The box's thumb: 16px long at the top of a 80px track (x 92..98); 320px scroll range.
        p.pointer_moved(95.0, 8.0);
        assert!(p.press());
        p.pointer_moved(95.0, 40.0); // 32 of the 64 free px
        assert_eq!(offset(&p, "box"), (0.0, 160.0));
        p.pointer_moved(95.0, 500.0); // dragging past the end clamps
        assert_eq!(offset(&p, "box"), (0.0, 320.0));
        p.pointer_moved(10.0, 10.0); // hover doesn't change while a thumb is held
        assert!(p.release());
        // Clicking its track jumps there (thumb centred on the pointer).
        p.pointer_moved(95.0, 48.0);
        p.press();
        p.release();
        let y = offset(&p, "box").1;
        assert!((y - 160.0).abs() < 8.0 * 5.0 + 1.0, "{y}"); // within a few px of the middle
        // The page's own bar sits at the right edge of the 600x300 viewport.
        let before = p.view.borrow().scroll_y;
        p.pointer_moved(595.0, 150.0);
        assert!(p.press());
        p.pointer_moved(595.0, 290.0);
        p.release();
        let after = p.view.borrow().scroll_y;
        assert!(after > before + 300.0, "{after}");
        let _ = std::fs::remove_file(path);
    }

    fn selected(p: &Page) -> String {
        p.view.borrow_mut().page_selected_text()
    }

    #[test]
    fn page_text_is_only_selectable_when_asked_for() {
        let (mut p, path) = page(
            r#"<body style="margin:0; font-size:20px"><p id="plain" style="margin:0">not selectable</p>
               <p id="attr" selectable style="margin:0">with attribute</p>
               <div style="behavior: htmlarea"><p id="beh" style="margin:0">sciter behavior</p></div>
               <p id="all" style="margin:0; user-select: all">click selects everything here</p>
               <p id="off" selectable style="margin:0"><span id="inner" style="user-select: none">locked</span></p></body>"#,
            "userselect",
        );
        let drag = |p: &mut Page, id: &str| {
            let (_, y) = center(p, id);
            p.pointer_moved(1.0, y);
            p.press();
            p.pointer_moved(590.0, y);
            p.release();
        };
        drag(&mut p, "plain");
        assert_eq!(selected(&p), "");
        drag(&mut p, "attr");
        assert_eq!(selected(&p), "with attribute");
        drag(&mut p, "beh");
        assert_eq!(selected(&p), "sciter behavior");
        // `user-select: all`: one click takes the whole run.
        let (_, y) = center(&p, "all");
        p.pointer_moved(40.0, y);
        p.press();
        p.release();
        assert_eq!(selected(&p), "click selects everything here");
        // Select all skips what can't be selected.
        press_key(&mut p, "a", ModifiersState::CONTROL);
        let all = selected(&p);
        assert!(all.contains("with attribute") && all.contains("sciter behavior") && !all.contains("not selectable"), "{all}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn page_text_selection_by_drag_word_run_and_copy() {
        let (mut p, path) = page(
            r#"<body style="margin:0; font-size:20px; user-select:text"><p id="a" style="margin:0">alpha beta gamma</p>
               <p id="b" style="margin:0">second <b>bold</b> tail</p><input id="t" value="inside">
               <script>document.addEventListener('click', () => console.log('sel:' + window.getSelection().toString()))</script></body>"#,
            "pagesel",
        );
        // Drag from the first paragraph's left edge to the second's right side.
        let (_, ay) = center(&p, "a");
        let (_, by) = center(&p, "b");
        p.pointer_moved(1.0, ay);
        p.press();
        p.pointer_moved(560.0, by);
        p.release();
        assert_eq!(selected(&p), "alpha beta gamma\nsecond bold tail");
        // Copying puts it on the clipboard.
        press_key(&mut p, "c", ModifiersState::CONTROL);
        assert_eq!(p.clipboard.text().as_deref(), Some("alpha beta gamma\nsecond bold tail"));
        // A double-click picks the word under the pointer, a triple-click the run.
        p.pointer_moved(80.0, ay);
        p.press();
        p.release();
        p.press();
        p.release();
        assert_eq!(selected(&p), "beta");
        p.press();
        p.release();
        assert_eq!(selected(&p), "alpha beta gamma");
        // Select all, then clicking an input drops the page selection.
        press_key(&mut p, "a", ModifiersState::CONTROL);
        assert_eq!(selected(&p), "alpha beta gamma\nsecond bold tail");
        click(&mut p, "t");
        assert_eq!(selected(&p), "");
        press_key(&mut p, "a", ModifiersState::CONTROL); // now it selects the input's text
        assert_eq!(selected(&p), "");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn select_all_copy_cut_and_paste() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><input id="t" value="abc def"><textarea id="m"></textarea>
               <script>$('#t').addEventListener('input', () => console.log('input:' + $('#t').value));</script></body>"#,
            "clipboard",
        );
        let cmd = ModifiersState::SUPER;
        click(&mut p, "t");
        press_key(&mut p, "a", cmd);
        assert_eq!((p.anchor(), p.caret()), (0, 7));
        press_key(&mut p, "c", cmd);
        assert_eq!(p.clipboard.text().as_deref(), Some("abc def"));
        press_key(&mut p, "Home", ModifiersState::default());
        press_key(&mut p, "ArrowRight", ModifiersState::SHIFT);
        press_key(&mut p, "ArrowRight", ModifiersState::SHIFT);
        press_key(&mut p, "ArrowRight", ModifiersState::SHIFT);
        press_key(&mut p, "x", cmd); // cut "abc"
        assert_eq!(val(&p, "t"), " def");
        assert_eq!(p.clipboard.text().as_deref(), Some("abc"));
        press_key(&mut p, "End", ModifiersState::default());
        press_key(&mut p, "v", cmd);
        assert_eq!(val(&p, "t"), " defabc");
        // Single-line inputs flatten pasted newlines; textareas keep them.
        p.clipboard.set_text("l1\r\nl2");
        press_key(&mut p, "v", cmd);
        assert_eq!(val(&p, "t"), " defabcl1 l2");
        click(&mut p, "m");
        press_key(&mut p, "v", cmd);
        assert_eq!(val(&p, "m"), "l1\nl2");
        assert!(log(&p).iter().filter(|l| l.starts_with("input:")).count() >= 3);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn mouse_selection_drag_double_and_triple_click() {
        let (mut p, path) = page(r#"<body style="margin:0"><input id="t" value="one two three" style="width:300px"></body>"#, "sel-mouse");
        let r = {
            let n = p.doc.borrow().find(|n| n.attr("id") == Some("t")).unwrap();
            p.view.borrow_mut().layout().node_rect(n).unwrap()
        };
        let y = r.y + r.height / 2.0;
        // Drag from the left edge of the text to the right: selects a prefix.
        p.pointer_moved(r.x + 6.0, y);
        p.press();
        p.pointer_moved(r.x + 40.0, y);
        p.release();
        let (a, c) = (p.anchor(), p.caret());
        assert!(a == 0 && c > 2 && c < 8, "dragged selection {a}..{c}");
        // Double-click selects the word under the pointer (a new click far enough away).
        p.last_click = None;
        p.pointer_moved(r.x + 38.0, y);
        p.press();
        p.release();
        p.press();
        p.release();
        assert_eq!(val(&p, "t")[p.anchor()..p.caret()].trim(), "two", "word at {}..{}", p.anchor(), p.caret());
        // Triple-click selects everything.
        p.press();
        p.release();
        assert_eq!((p.anchor(), p.caret()), (0, 13));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn script_selection_api_and_tab_selects_all() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><input id="a" value="abcdef"><input id="b" value="xyz">
               <script>
                 const a = $('#a'), b = $('#b');
                 b.setSelectionRange(1, 2);              // before b has focus
                 console.log(a.selectionStart, a.selectionEnd);   // unfocused: collapsed at the end
                 a.focus();
                 a.setSelectionRange(2, 4);
                 console.log(a.selectionStart, a.selectionEnd);
                 a.select();
                 console.log(a.selectionStart, a.selectionEnd, document.createElement('p').selectionStart);
                 b.focus();
                 console.log(b.selectionStart, b.selectionEnd);   // the pending range applied on focus
               </script></body>"#,
            "sel-js",
        );
        p.sync();
        assert_eq!(log(&p), ["6 6", "2 4", "0 6 null", "1 2"]);
        // Tab into an input selects its contents.
        p.default_key("Tab", ModifiersState::default());
        assert_eq!((p.anchor(), p.caret()), (0, 6), "wrapped to the first input, all selected");
        p.default_key("Tab", ModifiersState::default());
        assert_eq!((p.anchor(), p.caret()), (0, 3), "then the second");
        let _ = std::fs::remove_file(path);
    }

    fn sel_index(p: &Page, id: &str) -> usize {
        let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        nile_layout::selected_index(&p.doc.borrow(), n).unwrap()
    }

    /// Viewport point at the middle of row `row` of the open menu.
    fn row_point(p: &mut Page, row: usize) -> (f32, f32) {
        let mut v = p.view.borrow_mut();
        let m = v.layout().menu.as_ref().expect("menu is open");
        (m.rect.x + 10.0, m.rect.y + 1.0 + m.row_height * (row as f32 + 0.5))
    }

    const SELECT_PAGE: &str = r#"<body style="margin:0"><select id="s"><option value="r">Red</option><option value="g" disabled>Green</option><option value="b">Blue</option><option value="y">Yellow</option></select><p id="below">below</p>
        <script>
          for (const t of ['input', 'change']) $('#s').addEventListener(t, () => console.log(t + ':' + $('#s').value));
        </script></body>"#;

    #[test]
    fn select_opens_chooses_with_the_mouse_and_closes() {
        let (mut p, path) = page(SELECT_PAGE, "select-mouse");
        click(&mut p, "s");
        assert!(p.view.borrow().menu.is_some(), "click opens the drop-down");
        assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(0));
        // Hovering a row highlights it; a disabled row doesn't take it.
        let (x, y) = row_point(&mut p, 2);
        p.pointer_moved(x, y);
        assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(2));
        let (x, y) = row_point(&mut p, 1);
        p.pointer_moved(x, y);
        assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(2), "disabled row can't be highlighted");
        p.press();
        p.release();
        assert!(p.view.borrow().menu.is_some(), "releasing on a disabled row keeps it open");
        // Choose Blue.
        let (x, y) = row_point(&mut p, 2);
        p.pointer_moved(x, y);
        p.press();
        p.release();
        assert!(p.view.borrow().menu.is_none());
        assert_eq!(sel_index(&p, "s"), 2);
        // Opening and clicking outside closes without changing anything.
        click(&mut p, "s");
        let (bx, by) = center(&p, "below");
        p.pointer_moved(bx + 200.0, by + 100.0);
        p.press();
        p.release();
        assert!(p.view.borrow().menu.is_none());
        assert_eq!(sel_index(&p, "s"), 2);
        assert_eq!(log(&p), ["input:b", "change:b"]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn select_keyboard_navigation() {
        let (mut p, path) = page(SELECT_PAGE, "select-keys");
        let none = ModifiersState::default();
        click(&mut p, "s"); // open
        press_key(&mut p, "ArrowDown", none); // skips the disabled Green
        assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(2));
        press_key(&mut p, "End", none);
        assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(3));
        press_key(&mut p, "ArrowDown", none); // stays at the end
        assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(3));
        press_key(&mut p, "Escape", none);
        assert!(p.view.borrow().menu.is_none());
        assert_eq!(sel_index(&p, "s"), 0, "escape keeps the choice");
        // Closed: arrows change the choice directly, with events.
        press_key(&mut p, "ArrowDown", none);
        assert_eq!(sel_index(&p, "s"), 2);
        press_key(&mut p, "y", none); // type-ahead
        assert_eq!(sel_index(&p, "s"), 3);
        press_key(&mut p, "ArrowDown", none);
        assert_eq!(sel_index(&p, "s"), 3, "no wrap at the end");
        press_key(&mut p, " ", none); // opens
        assert!(p.view.borrow().menu.is_some());
        press_key(&mut p, "Home", none);
        press_key(&mut p, "Enter", none); // choose Red
        assert!(p.view.borrow().menu.is_none());
        assert_eq!(sel_index(&p, "s"), 0);
        assert_eq!(log(&p), ["input:b", "change:b", "input:y", "change:y", "input:r", "change:r"]);
        let _ = std::fs::remove_file(path);
    }

    const LIST_PAGE: &str = r#"<body style="margin:0"><select id="l" multiple size="4"><option>A</option><option>B</option><option disabled>C</option><option>D</option><option>E</option><option>F</option></select>
        <script>$('#l').addEventListener('change', () => console.log('change:' + $('#l').selectedOptions.map((o) => o.text).join('')));</script></body>"#;

    /// Viewport point in row `row` of the list box.
    fn list_row_point(p: &mut Page, row: usize) -> (f32, f32) {
        let id = p.doc.borrow().find(|n| n.attr("id") == Some("l")).unwrap();
        let mut v = p.view.borrow_mut();
        let r = v.layout().node_rect(id).unwrap();
        let l = v.layout().list_box(id).unwrap();
        (r.x + 10.0, r.y + l.inset_top - l.scroll + l.row_height * (row as f32 + 0.5))
    }

    fn selected_rows(p: &Page) -> Vec<usize> {
        let doc = p.doc.borrow();
        let id = doc.find(|n| n.attr("id") == Some("l")).unwrap();
        nile_layout::select_options(&doc, id).iter().enumerate().filter(|(_, o)| doc.has_state(**o, "selected")).map(|(i, _)| i).collect()
    }

    #[test]
    fn list_box_selects_rows_with_modifiers_keys_and_the_wheel() {
        let (mut p, path) = page(LIST_PAGE, "list-box");
        let click_row = |p: &mut Page, row: usize, mods: ModifiersState| {
            let (x, y) = list_row_point(p, row);
            p.mods = mods;
            p.pointer_moved(x, y);
            p.press();
            p.release();
            p.mods = ModifiersState::default();
        };
        let none = ModifiersState::default();
        click_row(&mut p, 1, none);
        assert_eq!(selected_rows(&p), [1]);
        assert!(p.view.borrow().menu.is_none(), "a list box has no drop-down");
        click_row(&mut p, 3, ModifiersState::SUPER);
        assert_eq!(selected_rows(&p), [1, 3], "cmd-click adds");
        click_row(&mut p, 1, ModifiersState::SUPER);
        assert_eq!(selected_rows(&p), [3], "cmd-click toggles off");
        click_row(&mut p, 0, ModifiersState::SHIFT);
        assert_eq!(selected_rows(&p), [0, 1], "shift-click replaces the selection with anchor..row");
        click_row(&mut p, 2, none);
        assert_eq!(selected_rows(&p), [0, 1], "disabled rows can't be chosen");
        // Keys, with the list still focused.
        press_key(&mut p, "ArrowDown", none);
        assert_eq!(selected_rows(&p), [1]);
        press_key(&mut p, "ArrowDown", none);
        assert_eq!(selected_rows(&p), [3], "moves past the disabled row");
        press_key(&mut p, "ArrowDown", ModifiersState::SHIFT);
        assert_eq!(selected_rows(&p), [3, 4]);
        press_key(&mut p, "a", ModifiersState::SUPER);
        assert_eq!(selected_rows(&p), [0, 1, 3, 4, 5]);
        press_key(&mut p, "Home", none);
        assert_eq!(selected_rows(&p), [0]);
        // The wheel scrolls the rows (4 visible of 6).
        let (x, y) = list_row_point(&mut p, 1);
        p.pointer_moved(x, y);
        p.wheel(0.0, -30.0);
        let id = p.doc.borrow().find(|n| n.attr("id") == Some("l")).unwrap();
        assert!(p.view.borrow_mut().layout().list_box(id).unwrap().scroll > 0.0);
        press_key(&mut p, "End", none);
        let l = p.view.borrow_mut().layout().list_box(id).unwrap().scroll;
        assert!(l > 0.0 && selected_rows(&p) == [5], "End selects and scrolls to the last row");
        assert_eq!(log(&p).last().map(String::as_str), Some("change:F"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn paint_only_restyles_keep_the_layout_and_layout_changes_drop_it() {
        let (mut p, path) = page(
            r#"<style>
                 #a { width: 100px; height: 40px; background: #eee; transform: rotate(0deg) }
                 #a:hover { background: #09f; box-shadow: 0 0 4px #000; transform: rotate(3deg) }
                 #b { width: 100px; height: 40px }
                 #b:hover { height: 80px }
               </style><body style="margin:0"><div id="a"></div><div id="b"></div></body>"#,
            "paint-only",
        );
        let built = |p: &Page| p.view.borrow().layout.is_some();
        assert!(built(&p));
        let (x, y) = center(&p, "a");
        p.pointer_moved(x, y);
        assert!(built(&p), "colors, shadows and transforms don't need a new layout");
        let a = p.doc.borrow().find(|n| n.attr("id") == Some("a")).unwrap();
        let xf = p.view.borrow_mut().layout().boxes.iter().find(|b| b.node == a).unwrap().transform.unwrap();
        assert!(xf.b != 0.0, "but the box picks up its new transform");
        p.layout(600.0, 300.0);
        let (x, y) = center(&p, "b");
        p.pointer_moved(x, y);
        assert!(!built(&p), "a height change does");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn cursor_follows_the_hovered_element() {
        use nile_style::Cursor;
        let (mut p, path) = page(
            r##"<body style="margin:0"><a id="l" href="#" style="display:block;height:20px">link</a>
               <input id="i"><div id="g" style="height:20px;cursor:grab"><span id="s">x</span></div><div id="d" style="height:20px">plain</div></body>"##,
            "cursor",
        );
        let at = |p: &mut Page, id: &str| {
            let (x, y) = center(p, id);
            p.pointer_moved(x, y);
            p.cursor()
        };
        assert_eq!(at(&mut p, "l"), Cursor::Pointer);
        assert_eq!(at(&mut p, "i"), Cursor::Text);
        assert_eq!(at(&mut p, "s"), Cursor::Grab, "inherited by children");
        assert_eq!(at(&mut p, "d"), Cursor::Auto);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn window_roles_and_script_calls_queue_window_commands() {
        let (mut p, path) = page(
            r#"<html window-frame="solid"><body style="margin:0">
                 <div id="bar" role="window-caption" style="height:30px"><span id="t">Title</span>
                   <button id="min" role="window-minimize">_</button><button id="max" role="window-maximize">o</button>
                   <button id="x" role="window-close">x</button><button id="plain">p</button></div>
                 <script>
                   $('#plain').addEventListener('click', () => { document.title = 'Renamed'; window.moveTo(10, 20); window.resizeTo(500, 400); window.close(); });
                 </script></body></html>"#,
            "window-roles",
        );
        let take = |p: &Page| std::mem::take(&mut p.view.borrow_mut().window_requests);
        click(&mut p, "min");
        click(&mut p, "max");
        click(&mut p, "x");
        assert_eq!(take(&p), [WindowCommand::Minimize, WindowCommand::ToggleMaximize, WindowCommand::Close]);
        // Pressing the caption starts a drag; a quick second press toggles maximize; buttons don't drag.
        let (x, y) = center(&p, "t");
        p.pointer_moved(x, y);
        assert!(p.caption_press(), "first press drags");
        assert!(!p.caption_press(), "second press maximizes instead");
        assert_eq!(take(&p), [WindowCommand::ToggleMaximize]);
        let (x, y) = center(&p, "plain");
        p.pointer_moved(x, y);
        assert!(!p.caption_press(), "a button in the caption keeps its click");
        click(&mut p, "plain");
        assert_eq!(
            take(&p),
            [WindowCommand::MoveTo(10.0, 20.0), WindowCommand::ResizeTo(500.0, 400.0), WindowCommand::Close]
        );
        let doc = p.doc.borrow();
        assert_eq!(window::current_title(&doc, &WindowConfig::from_document(&doc)), "Renamed");
        drop(doc);
        let _ = std::fs::remove_file(path);
    }

    /// How many pixels in a painted page are strongly `rgb`-ish red / blue.
    fn count_colored(p: &mut Page) -> (usize, usize) {
        let mut px = vec![0u8; 300 * 120 * 4];
        p.paint(&mut px, 300, 120, 1.0);
        let reds = px.chunks(4).filter(|c| c[2] > 180 && c[1] < 80 && c[0] < 80).count();
        let blues = px.chunks(4).filter(|c| c[0] > 180 && c[1] < 80 && c[2] < 80).count();
        (reds, blues)
    }

    #[test]
    fn text_color_changes_repaint_without_a_new_layout_but_inline_backgrounds_do_relayout() {
        let (mut p, path) = page(
            r#"<style>
                 #a { width: 280px; height: 30px; color: #00f; font-size: 22px } #a:hover { color: #f00 }
                 #s { background: #00f; color: #fff } #s:hover { background: #f00 }
               </style><body style="margin:0"><div id="a">WWWWWWWW</div><div><span id="s">inline span here</span></div></body>"#,
            "text-color",
        );
        let built = |p: &Page| p.view.borrow().layout.is_some();
        let (r0, b0) = count_colored(&mut p);
        assert!(r0 == 0 && b0 > 100, "blue text to begin with: {r0} {b0}");
        let (x, y) = center(&p, "a");
        p.pointer_moved(x, y);
        assert!(built(&p), "a text color change keeps the layout");
        let (r1, _) = count_colored(&mut p);
        assert!(r1 > 100, "and the text turns red: {r1}");
        // An inline box's background is baked into its text run, so that one lays out again.
        let (x, y) = center(&p, "s");
        p.pointer_moved(x, y);
        assert!(!built(&p));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn clicking_a_summary_toggles_its_details() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><details id="d"><summary id="s">More</summary><p id="body">Hidden</p></details>
               <script>$('#d').addEventListener('toggle', () => console.log('toggle:' + $('#d').hasAttribute('open')));</script></body>"#,
            "details",
        );
        let shown = |p: &mut Page| {
            let id = p.doc.borrow().find(|n| n.attr("id") == Some("body")).unwrap();
            p.view.borrow_mut().layout().node_rect(id).is_some()
        };
        assert!(!shown(&mut p), "closed details hide their content");
        click(&mut p, "s");
        assert!(shown(&mut p), "open shows it");
        click(&mut p, "s");
        assert!(!shown(&mut p));
        assert_eq!(log(&p), ["toggle:true", "toggle:false"]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn range_sliders_follow_the_pointer_and_keys_and_fire_events() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><input id="r" type="range" min="0" max="10" value="2" style="margin:0;width:116px">
               <script>for (const t of ['input','change']) $('#r').addEventListener(t, () => console.log(t + ':' + $('#r').value));</script></body>"#,
            "range",
        );
        let id = p.doc.borrow().find(|n| n.attr("id") == Some("r")).unwrap();
        let rect = p.view.borrow_mut().layout().node_rect(id).unwrap();
        let at = |f: f32| (rect.x + 8.0 + (rect.width - 16.0) * f, rect.y + rect.height / 2.0);
        // Press at 50%: the thumb jumps there.
        let (x, y) = at(0.5);
        p.pointer_moved(x, y);
        p.press();
        assert_eq!(p.range_value(id), 5.0);
        // Dragging follows (snapped to the step) and clamps at the ends.
        let (x, y) = at(0.87);
        p.pointer_moved(x, y);
        assert_eq!(p.range_value(id), 9.0);
        p.pointer_moved(rect.x + rect.width + 50.0, y);
        assert_eq!(p.range_value(id), 10.0);
        p.release();
        assert_eq!(log(&p), ["input:5", "input:9", "input:10", "change:10"]);
        // Keys on the focused slider.
        let none = ModifiersState::default();
        press_key(&mut p, "ArrowLeft", none);
        press_key(&mut p, "Home", none);
        assert_eq!(p.range_value(id), 0.0);
        assert_eq!(log(&p).last().map(String::as_str), Some("change:0"));
        let (fraction, _) = nile_style::control_fraction(&p.doc.borrow(), id);
        assert_eq!(fraction, Some(0.0));
        let _ = std::fs::remove_file(path);
    }

    const EDITOR: &str = r#"<body style="margin:0;font-family:sans-serif;font-size:16px"><div id="h" contenteditable="true" style="width:400px"><p id="p1" style="margin:0">Hello</p><p id="p2" style="margin:0">World</p></div></body>"#;

    fn js(p: &mut Page, code: &str) -> String {
        p.eval(code).0.unwrap_or_else(|e| format!("ERR {e}"))
    }

    fn html(p: &mut Page) -> String {
        js(p, "document.getElementById('h').innerHTML")
    }

    fn type_keys(p: &mut Page, keys: &str) {
        for c in keys.chars() {
            let k = c.to_string();
            let (proceed, _) = p.key("keydown", k.clone(), ModifiersState::default());
            if proceed {
                p.default_key(&k, ModifiersState::default());
            }
        }
    }

    /// Click at the end of paragraph `id`'s text.
    fn click_end_of(p: &mut Page, id: &str) {
        // (Detached nodes keep their ids, so look only at what is in the document.)
        let node = {
            let doc = p.doc.borrow();
            doc.descendants(Document::ROOT).into_iter().find(|n| doc.node(*n).attr("id") == Some(id)).unwrap()
        };
        let r = p.view.borrow_mut().layout().node_rect(node).unwrap();
        p.pointer_moved(r.x + r.width - 2.0, r.y + r.height / 2.0);
        p.press();
        p.release();
    }

    #[test]
    fn contenteditable_takes_typing_enter_and_backspace() {
        let (mut p, path) = page(EDITOR, "edit-typing");
        click_end_of(&mut p, "p1");
        assert!(p.focused_host().is_some(), "the host has focus");
        let sel = p.edit_selection().expect("a caret");
        assert_eq!(sel.0, sel.1);
        type_keys(&mut p, "!?");
        assert_eq!(html(&mut p), r#"<p id="p1" style="margin:0">Hello!?</p><p id="p2" style="margin:0">World</p>"#);
        press_key(&mut p, "Backspace", ModifiersState::default());
        assert_eq!(js(&mut p, "document.getElementById('p1').textContent"), "Hello!");
        press_key(&mut p, "Enter", ModifiersState::default());
        type_keys(&mut p, "ab");
        assert_eq!(js(&mut p, "document.getElementById('h').children.map(c => c.textContent).join('|')"), "Hello!|ab|World");
        // Backspace at the start of a paragraph joins it with the one before.
        press_key(&mut p, "ArrowLeft", ModifiersState::default());
        press_key(&mut p, "ArrowLeft", ModifiersState::default());
        press_key(&mut p, "Backspace", ModifiersState::default());
        assert_eq!(js(&mut p, "document.getElementById('h').children.map(c => c.textContent).join('|')"), "Hello!ab|World");
        let _ = std::fs::remove_file(path);
    }

    /// Select `from..to` (character offsets) of paragraph `id`'s first text node.
    fn select_in(p: &mut Page, id: &str, from: usize, to: usize) {
        let code = format!("var t = document.getElementById('{id}').firstChild; document.getElementById('h').focus(); getSelection().setBaseAndExtent(t, {from}, t, {to});");
        js(p, &code);
        // `focus()` is a script call; let the page notice.
        p.sync();
        js(p, &format!("var t = document.getElementById('{id}').firstChild; getSelection().setBaseAndExtent(t, {from}, t, {to});"));
    }

    fn inner(p: &mut Page, id: &str) -> String {
        js(p, &format!("document.getElementById('{id}').innerHTML"))
    }

    #[test]
    fn execcommand_formats_ranges_and_reports_state() {
        let (mut p, path) = page(EDITOR, "edit-format");
        select_in(&mut p, "p1", 1, 4);
        assert_eq!(js(&mut p, "document.execCommand('bold')"), "true");
        assert_eq!(inner(&mut p, "p1"), "H<b>ell</b>o");
        assert_eq!(js(&mut p, "document.queryCommandState('bold')"), "true");
        assert_eq!(js(&mut p, "document.queryCommandState('italic')"), "false");
        // Part of the bold run turns it off again: "e" and "o" stay as they are.
        js(&mut p, "var b = document.querySelector('b').firstChild; getSelection().setBaseAndExtent(b, 1, b, 2)");
        js(&mut p, "document.execCommand('bold')");
        assert_eq!(inner(&mut p, "p1"), "H<b>e</b>l<b>l</b>o");
        // Across paragraphs: both get italics; colours and sizes use spans.
        js(&mut p, "var a = document.getElementById('p1'), b = document.getElementById('p2'); getSelection().setBaseAndExtent(a.lastChild, 0, b.firstChild, 3)");
        js(&mut p, "document.execCommand('italic'); document.execCommand('foreColor', false, '#ff0000')");
        assert_eq!(inner(&mut p, "p1"), r##"H<b>e</b>l<b>l</b><i><span style="color: #ff0000">o</span></i>"##);
        assert_eq!(inner(&mut p, "p2"), r##"<i><span style="color: #ff0000">Wor</span></i>ld"##);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn editing_undo_lists_blocks_pending_format_and_selection_deletes() {
        let (mut p, path) = page(EDITOR, "edit-more");
        // Typing after Bold with a caret types bold text.
        click_end_of(&mut p, "p1");
        js(&mut p, "document.execCommand('bold')");
        type_keys(&mut p, "XY");
        assert_eq!(inner(&mut p, "p1"), "Hello<b>XY</b>");
        js(&mut p, "document.execCommand('bold')");
        type_keys(&mut p, "z");
        assert_eq!(inner(&mut p, "p1"), "Hello<b>XY</b>z");
        // Undo steps back through the edits, redo returns.
        assert_eq!(js(&mut p, "document.execCommand('undo')"), "true");
        assert_eq!(js(&mut p, "document.getElementById('p1').textContent"), "HelloXY");
        js(&mut p, "document.execCommand('undo'); document.execCommand('undo')");
        assert_eq!(inner(&mut p, "p1"), "Hello");
        js(&mut p, "document.execCommand('redo')");
        assert_eq!(js(&mut p, "document.getElementById('p1').textContent"), "HelloXY");
        js(&mut p, "document.execCommand('undo')");
        // Blocks: alignment, headings, lists.
        select_in(&mut p, "p2", 1, 3);
        js(&mut p, "document.execCommand('justifyCenter'); document.execCommand('formatBlock', false, 'h2')");
        assert_eq!(js(&mut p, "document.getElementById('h').lastChild.outerHTML"), r#"<h2 style="margin: 0; text-align: center">World</h2>"#);
        js(&mut p, "document.execCommand('insertUnorderedList')");
        assert_eq!(js(&mut p, "document.getElementById('h').lastChild.tagName"), "UL");
        assert_eq!(js(&mut p, "document.queryCommandState('insertUnorderedList')"), "true");
        js(&mut p, "document.execCommand('insertUnorderedList')");
        assert_eq!(js(&mut p, "document.getElementById('h').lastChild.tagName"), "P");
        // Deleting a selection across paragraphs joins them.
        js(&mut p, "var a = document.getElementById('p1').firstChild, b = document.getElementById('h').lastChild.firstChild; getSelection().setBaseAndExtent(a, 3, b, 2)");
        js(&mut p, "document.execCommand('delete')");
        assert_eq!(js(&mut p, "document.getElementById('h').children.map(c => c.textContent).join('|')"), "Helrld");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_word_example_formats_the_document_from_its_ribbon() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/word-ribbon.html");
        let mut p = Page::load(file, (1200.0, 800.0), 0.0);
        p.layout(1200.0, 800.0);
        let q = |p: &mut Page, code: &str| js(p, code);
        // Select "clear purpose" in the Overview paragraph and use the ribbon like a person would.
        q(&mut p, "var t = document.getElementById('sample').firstChild; document.getElementById('paper').focus(); var i = t.data.indexOf('clear purpose'); getSelection().setBaseAndExtent(t, i, t, i + 13)");
        p.sync();
        q(&mut p, "var t = document.getElementById('sample').firstChild; var i = t.data.indexOf('clear purpose'); getSelection().setBaseAndExtent(t, i, t, i + 13)");
        click(&mut p, "bold");
        click(&mut p, "italic");
        let sample = q(&mut p, "document.getElementById('sample').innerHTML");
        assert!(sample.contains("<i><b>clear purpose</b></i>") || sample.contains("<b><i>clear purpose</i></b>"), "{sample}");
        assert_eq!(q(&mut p, "document.getElementById('bold').classList.contains('selected')"), "true", "the Bold button shows its state");
        // Undo from the title bar, then redo, then change the paragraph alignment.
        let undo = p.doc.borrow().find(|n| n.attr("data-action") == Some("undo")).unwrap();
        let r = p.view.borrow_mut().layout().node_rect(undo).unwrap();
        for _ in 0..2 {
            p.pointer_moved(r.x + 3.0, r.y + 3.0);
            p.press();
            p.release();
        }
        assert!(!q(&mut p, "document.getElementById('sample').innerHTML").contains("<b>"), "two undos remove both commands");
        let align = p.doc.borrow().find(|n| n.attr("data-align") == Some("center")).unwrap();
        let r = p.view.borrow_mut().layout().node_rect(align).unwrap();
        p.pointer_moved(r.x + 3.0, r.y + 3.0);
        p.press();
        p.release();
        assert!(q(&mut p, "document.getElementById('sample').style.textAlign") == "center");
        assert!(q(&mut p, "document.getElementById('word-count').textContent").ends_with("words"));
    }

    #[test]
    fn editing_with_the_mouse_clipboard_arrows_and_lists() {
        let (mut p, path) = page(
            r#"<body style="margin:0;font-family:sans-serif;font-size:16px"><div id="h" contenteditable="true" style="width:400px"><p id="p1" style="margin:0">Hello</p><p id="p2" style="margin:0">World</p><ul id="u"><li id="l1">one</li></ul></div></body>"#,
            "edit-mouse",
        );
        let rect = |p: &mut Page, id: &str| {
            let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
            p.view.borrow_mut().layout().node_rect(n).unwrap()
        };
        // Drag from inside "Hello" to inside "World": the selection covers the line break.
        let _ = rect(&mut p, "p1");
        let caret_at = |p: &mut Page, id: &str, offset: usize| {
            let t = p.doc.borrow().find(|n| n.attr("id") == Some(id)).map(|n| p.doc.borrow().node(n).children[0]).unwrap();
            p.edit_caret_painted(DomPoint { node: t, offset }).unwrap()
        };
        let (a, b) = (caret_at(&mut p, "p1", 3), caret_at(&mut p, "p2", 3));
        p.pointer_moved(a.0 + 0.5, a.1 + a.2 / 2.0);
        p.press();
        p.pointer_moved(b.0 + 0.5, b.1 + b.2 / 2.0);
        p.release();
        assert_eq!(p.edit_selected_text(), "lo\nWor");
        let ctrl = ModifiersState::CONTROL;
        press_key(&mut p, "c", ctrl);
        assert_eq!(p.clipboard.text().as_deref(), Some("lo\nWor"));
        // Typing replaces the selection; paste puts the text (and the line break) back.
        type_keys(&mut p, "X");
        assert_eq!(js(&mut p, "document.getElementById('h').children.map(c => c.textContent).join('|')"), "HelXld|one");
        press_key(&mut p, "v", ctrl);
        assert_eq!(js(&mut p, "document.getElementById('h').children.map(c => c.textContent).join('|')"), "HelXlo|World|one");
        // Select all, then cut: nothing but an empty line remains.
        press_key(&mut p, "a", ctrl);
        press_key(&mut p, "x", ctrl);
        assert_eq!(p.clipboard.text().as_deref(), Some("HelXlo\nWorld\none"));
        assert_eq!(js(&mut p, "document.getElementById('h').textContent"), "");
        // Undo brings it all back.
        press_key(&mut p, "z", ctrl);
        assert_eq!(js(&mut p, "document.getElementById('h').children.length"), "3");
        // Lists: Enter makes a new item, Enter on the empty item leaves the list.
        click_end_of(&mut p, "l1");
        press_key(&mut p, "Enter", ModifiersState::default());
        type_keys(&mut p, "two");
        assert_eq!(js(&mut p, "document.getElementById('u').children.map(c => c.textContent).join('|')"), "one|two");
        press_key(&mut p, "Enter", ModifiersState::default());
        press_key(&mut p, "Enter", ModifiersState::default());
        assert_eq!(js(&mut p, "document.getElementById('h').lastChild.tagName"), "P");
        assert_eq!(js(&mut p, "document.getElementById('u').children.length"), "2");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn transitions_and_animations_fire_their_events() {
        let (mut p, path) = page(
            r#"<style>
                #t { width: 10px; transition: width 0.05s linear }
                #t.on { width: 50px }
                #a { animation: nudge 0.05s linear }
                @keyframes nudge { from { margin-left: 0 } to { margin-left: 5px } }
            </style>
            <div id=t></div><div id=a></div>
            <script>
              window.log = [];
              for (const id of ["t", "a"]) {
                const el = document.getElementById(id);
                for (const k of ["transitionend", "animationstart", "animationend"])
                  el.addEventListener(k, e => log.push(k + ":" + (e.propertyName || e.animationName) + ":" + e.target.id));
              }
              document.getElementById("t").classList.add("on");
            </script>"#,
            "anim_events",
        );
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            p.tick();
        }
        let log = js(&mut p, "log.join(',')");
        assert!(log.contains("animationstart:nudge:a"), "{log}");
        assert!(log.contains("animationend:nudge:a"), "{log}");
        assert!(log.contains("transitionend:width:t"), "{log}");
        assert_eq!(log.matches("end").count(), 2, "each fires once: {log}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn fixed_boxes_stay_put_and_stay_clickable_while_scrolled() {
        let (mut p, path) = page(
            r#"<style>
                body { margin: 0 }
                .row { height: 100px }
                #f { position: fixed; right: 10px; bottom: 10px; width: 40px; height: 30px }
                #s { position: sticky; top: 0; height: 20px }
            </style>
            <div class=row></div><div id=s></div>
            <div class=row></div><div class=row></div><div class=row></div><div class=row></div><div class=row></div>
            <div id=f onclick="window.hit = 1"></div>"#,
            "fixed_click",
        );
        let at = |p: &mut Page, id: &str| {
            let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
            let (sc, offs) = {
                let v = p.view.borrow();
                (v.scroll_y, v.scroll_offsets.clone())
            };
            let r = p.view.borrow_mut().layout().node_rect_scrolled(node, &offs).unwrap();
            (r.x + r.width / 2.0, r.y - sc + r.height / 2.0)
        };
        let before = at(&mut p, "f");
        p.wheel(0.0, -250.0);
        assert!(p.view.borrow().scroll_y > 0.0);
        // Same place on screen after scrolling; the sticky bar has caught up with the top.
        assert_eq!(at(&mut p, "f"), before);
        let (_, sticky_y) = at(&mut p, "s");
        assert!((sticky_y - 10.0).abs() < 1.0, "{sticky_y}");
        let (x, y) = before;
        p.pointer_moved(x, y);
        p.press();
        p.release();
        assert_eq!(js(&mut p, "String(window.hit)"), "1");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn number_inputs_step_clamp_filter_and_spin() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><input id="q" type="number" value="9" min="0" max="10" style="margin:0;width:100px">
               <input id="d" type="number" value="0.1" step="0.1" style="margin:0;width:100px"></body>"#,
            "number",
        );
        let none = ModifiersState::default();
        let val = |p: &mut Page, id: &str| js(p, &format!("document.getElementById('{id}').value"));
        click(&mut p, "q");
        press_key(&mut p, "ArrowUp", none);
        press_key(&mut p, "ArrowUp", none);
        assert_eq!(val(&mut p, "q"), "10"); // clamped to max
        press_key(&mut p, "ArrowDown", none);
        assert_eq!(val(&mut p, "q"), "9");
        // Letters are dropped, digits and signs kept.
        press_key(&mut p, "End", none);
        type_keys(&mut p, "x5");
        assert_eq!(val(&mut p, "q"), "95");
        click(&mut p, "d");
        press_key(&mut p, "ArrowUp", none);
        press_key(&mut p, "ArrowUp", none);
        assert_eq!(val(&mut p, "d"), "0.3"); // no 0.30000000000000004
        // The spin arrows at the right edge: upper half up, lower half down.
        let node = p.doc.borrow().find(|n| n.attr("id") == Some("d")).unwrap();
        let r = p.view.borrow_mut().layout().node_rect(node).unwrap();
        p.pointer_moved(r.x + r.width - 8.0, r.y + 3.0);
        p.press();
        p.release();
        assert_eq!(val(&mut p, "d"), "0.4");
        p.pointer_moved(r.x + r.width - 8.0, r.y + r.height - 3.0);
        p.press();
        p.release();
        assert_eq!(val(&mut p, "d"), "0.3");
        let _ = std::fs::remove_file(path);
    }

    #[cfg(feature = "net")]
    #[test]
    fn fetch_reaches_a_local_http_server() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                // Read the whole request (headers, then the two body bytes) before answering.
                let mut req = String::new();
                let mut buf = [0u8; 2048];
                while !req.contains("\r\n\r\nhi") {
                    match s.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => req.push_str(&String::from_utf8_lossy(&buf[..n])),
                    }
                }
                let body = format!("{{\"method\":\"{}\"}}", req.split(' ').next().unwrap_or(""));
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            }
        });
        let (mut p, path) = page(
            &format!("<p id=o></p><script>fetch('http://127.0.0.1:{port}/x', {{ method: 'POST', body: 'hi' }}).then(r => r.json()).then(j => {{ document.getElementById('o').textContent = j.method; }})</script>"),
            "fetch_http",
        );
        for _ in 0..1000 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            p.tick();
            if js(&mut p, "document.getElementById('o').textContent") == "POST" {
                break;
            }
        }
        assert_eq!(js(&mut p, "document.getElementById('o').textContent"), "POST");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn observers_location_and_history_follow_the_page() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="top" style="height:100px"></div><div style="height:600px"></div><div id="far" style="height:50px"></div>
               <script>
                 window.log = [];
                 new ResizeObserver(es => es.forEach(e => log.push('ro ' + e.target.id + ' ' + e.contentRect.height))).observe(document.getElementById('top'));
                 new IntersectionObserver(es => es.forEach(e => log.push('io ' + e.target.id + ' ' + e.isIntersecting))).observe(document.getElementById('far'));
                 window.addEventListener('hashchange', e => log.push('hash ' + location.hash));
               </script>"#,
            "observers",
        );
        let log = |p: &mut Page| js(p, "log.join(',')");
        p.sync();
        assert_eq!(log(&mut p), "ro top 100,io far false");
        // A size change reaches the resize observer; scrolling reveals the far box.
        js(&mut p, "document.getElementById('top').style.height = '120px'; 0");
        p.sync();
        p.wheel(0.0, -700.0);
        p.sync();
        assert_eq!(log(&mut p), "ro top 100,io far false,ro top 120,io far true");
        // location and history.
        assert!(js(&mut p, "location.href").starts_with("file://"));
        assert_eq!(js(&mut p, "location.pathname.endsWith('observers.html') + ',' + devicePixelRatio"), "true,1");
        js(&mut p, "location.hash = 'two'; 0");
        assert!(js(&mut p, "log.join(',')").ends_with("hash #two"));
        js(&mut p, "history.pushState({ a: 1 }, '', '?q=1#three'); 0");
        assert_eq!(js(&mut p, "location.search + location.hash + history.state.a + history.length"), "?q=1#three12");
        let _ = std::fs::remove_file(path);
    }
}
