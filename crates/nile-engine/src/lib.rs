//! The Nile engine without a window: parse a page, run its scripts, feed it input, and paint it
//! into a pixel buffer. The `nile` shell wraps it in a winit window and `nile-capi` exposes it as a
//! C library. Everything here is single-threaded.
//!
//! `Page` is the entry point. Its fields and methods are public for those two embedders; most of
//! them are internal plumbing, so prefer the C API (`nile-capi`) for anything else.

pub mod a11y;
pub mod dialogs;

type Defaults = (Option<Rc<dyn dialogs::Dialogs>>, Option<fn() -> Box<dyn Clipboard>>);
thread_local! {
    static DEFAULTS: std::cell::RefCell<Defaults> = const { std::cell::RefCell::new((None, None)) };
}
pub mod editing;
pub mod frames;
pub mod menus;
pub mod paged;
mod inputs;
mod net;
pub mod window;

use std::{cell::RefCell, path::PathBuf, rc::Rc, time::{Duration, Instant}};

use nile_dom::{Document, NodeId, ScriptSource};
use nile_layout::{DomPoint, TextPoint, FileImages, LayoutOptions, LayoutTree, MenuRequest, ScrollOffsets};
use nile_render::{Overlay, Renderer};
use nile_script::{Environment, Progress, ScriptHost, TraySpec, UiEvent, WindowCommand, WindowInfo};
use nile_style::{compute_styles_in, Animator, InteractionState, StyleMap};
use nile_text::TextEngine;
use skia_safe::{surfaces, AlphaType, Color, ColorType, ImageInfo};
use window::{Role, WindowConfig};

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

pub trait Clipboard {
    fn text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: &str);
    /// The clipboard's HTML flavour, if it has one (rich paste into editors).
    fn html(&mut self) -> Option<String> {
        None
    }
    /// Put `html` on the clipboard with `text` as its plain-text form. Clipboards without an
    /// HTML flavour keep just the text.
    fn set_html(&mut self, _html: &str, text: &str) {
        self.set_text(text);
    }
    /// A picture on the clipboard.
    fn image(&mut self) -> Option<ClipImage> {
        None
    }
    fn set_image(&mut self, _image: &ClipImage) {}
    /// Files copied in the system's file manager.
    fn files(&mut self) -> Vec<String> {
        Vec::new()
    }
    fn set_files(&mut self, _files: &[String]) {}
}

/// A clipboard picture: premultiplied-agnostic RGBA, row by row.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Default)]
pub struct MemoryClipboard {
    text: Option<String>,
    html: Option<String>,
    image: Option<ClipImage>,
    files: Vec<String>,
}

impl Clipboard for MemoryClipboard {
    fn text(&mut self) -> Option<String> {
        self.text.clone()
    }
    fn set_text(&mut self, text: &str) {
        *self = MemoryClipboard { text: Some(text.to_string()), ..Default::default() };
    }
    fn html(&mut self) -> Option<String> {
        self.html.clone()
    }
    fn set_html(&mut self, html: &str, text: &str) {
        *self = MemoryClipboard { text: Some(text.to_string()), html: Some(html.to_string()), ..Default::default() };
    }
    fn image(&mut self) -> Option<ClipImage> {
        self.image.clone()
    }
    fn set_image(&mut self, image: &ClipImage) {
        *self = MemoryClipboard { image: Some(image.clone()), ..Default::default() };
    }
    fn files(&mut self) -> Vec<String> {
        self.files.clone()
    }
    fn set_files(&mut self, files: &[String]) {
        *self = MemoryClipboard { files: files.to_vec(), ..Default::default() };
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
    fn html(&mut self) -> Option<String> {
        self.get()?.get().html().ok()
    }
    fn set_html(&mut self, html: &str, text: &str) {
        if let Some(c) = self.get() {
            let _ = c.set_html(html, Some(text));
        }
    }
    fn image(&mut self) -> Option<ClipImage> {
        let img = self.get()?.get_image().ok()?;
        Some(ClipImage { width: img.width as u32, height: img.height as u32, rgba: img.bytes.into_owned() })
    }
    fn set_image(&mut self, image: &ClipImage) {
        if let Some(c) = self.get() {
            let _ = c.set_image(arboard::ImageData { width: image.width as usize, height: image.height as usize, bytes: image.rgba.as_slice().into() });
        }
    }
    fn files(&mut self) -> Vec<String> {
        let Some(c) = self.get() else { return Vec::new() };
        c.get().file_list().map(|l| l.iter().map(|p| p.to_string_lossy().into_owned()).collect()).unwrap_or_default()
    }
    fn set_files(&mut self, files: &[String]) {
        if let Some(c) = self.get() {
            let _ = c.set().file_list(files);
        }
    }
}

/// The clipboard a page, its frames and its scripts all share. `json` is the one flavour the
/// system clipboards don't carry: it is kept in the process and counts only while the text that
/// was put with it is still the clipboard's text.
#[derive(Clone)]
pub struct SharedClipboard(Rc<RefCell<SharedInner>>);

struct SharedInner {
    system: Box<dyn Clipboard>,
    json: Option<(String, Option<String>)>,
}

impl Default for SharedClipboard {
    fn default() -> Self {
        SharedClipboard::new(Box::new(MemoryClipboard::default()))
    }
}

impl SharedClipboard {
    pub fn new(system: Box<dyn Clipboard>) -> Self {
        SharedClipboard(Rc::new(RefCell::new(SharedInner { system, json: None })))
    }
    /// Swap the clipboard everyone shares for another (the embedder's own, or the OS one).
    pub fn replace(&self, system: Box<dyn Clipboard>) {
        let mut inner = self.0.borrow_mut();
        inner.system = system;
        inner.json = None;
    }
    pub fn json(&self) -> Option<String> {
        let mut inner = self.0.borrow_mut();
        let (json, text) = inner.json.clone()?;
        (inner.system.text() == text).then_some(json)
    }
    pub fn set_json(&self, json: &str, text: Option<&str>) {
        let mut inner = self.0.borrow_mut();
        match text {
            Some(t) => inner.system.set_text(t),
            None => inner.system.set_text(""),
        }
        inner.json = Some((json.to_string(), text.or(Some("")).map(str::to_string)));
    }
}

impl Clipboard for SharedClipboard {
    fn text(&mut self) -> Option<String> {
        self.0.borrow_mut().system.text()
    }
    fn set_text(&mut self, text: &str) {
        self.0.borrow_mut().system.set_text(text)
    }
    fn html(&mut self) -> Option<String> {
        self.0.borrow_mut().system.html()
    }
    fn set_html(&mut self, html: &str, text: &str) {
        self.0.borrow_mut().system.set_html(html, text)
    }
    fn image(&mut self) -> Option<ClipImage> {
        self.0.borrow_mut().system.image()
    }
    fn set_image(&mut self, image: &ClipImage) {
        self.0.borrow_mut().system.set_image(image)
    }
    fn files(&mut self) -> Vec<String> {
        self.0.borrow_mut().system.files()
    }
    fn set_files(&mut self, files: &[String]) {
        self.0.borrow_mut().system.set_files(files)
    }
}

/// How long the pointer rests on an element before its tooltip shows.
const TOOLTIP_DELAY: Duration = Duration::from_millis(500);
/// Half the caret's blink period.
pub const CARET_BLINK: Duration = Duration::from_millis(530);

/// An HTML5 drag in progress: the dragged element, what is under the pointer, whether the last `dragover` accepted a
/// drop there, and where in the element the pointer took hold (for the ghost).
#[derive(Debug, Clone)]
pub struct Dnd {
    pub source: NodeId,
    pub over: Option<NodeId>,
    pub accept: bool,
    pub grab: (f32, f32),
}

/// A scroll-snap container that was just scrolled (see `Page::snap_due`).
#[derive(Debug, Clone, Copy)]
pub struct SnapPending {
    pub node: NodeId,
    pub from: (f32, f32),
    pub due: Instant,
}

/// A popup element that the shell shows in an OS window of its own. `rect` is its border box in
/// this window's viewport coordinates (it may lie outside the viewport).
#[derive(Debug, Clone, PartialEq)]
pub struct Portal {
    pub node: NodeId,
    pub rect: (f32, f32, f32, f32),
}

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
    /// Text an input method is composing in a text input (a contenteditable host keeps its
    /// composition in the page instead).
    pub preedit: Option<nile_layout::Preedit>,
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
    /// The clipboard scripts read and write (`Clipboard.read()`...); the page's own handle is the same one.
    pub clipboard: SharedClipboard,
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
    /// Value when the focused input got focus (or last had a script assign it), to decide whether `change` fires.
    pub value_at_focus: Option<String>,
    /// Focus moves made by scripts, so the page can fire `blur`/`focus` for them.
    pub focus_changes: Vec<(Option<NodeId>, Option<NodeId>)>,
    /// Window actions scripts and `role="window-*"` elements asked for; the window loop applies them.
    pub window_requests: Vec<WindowCommand>,
    /// `Window.this.print()` / `exportPdf()` jobs waiting for the page to write its PDF: (id, path, title).
    pub print_requests: Vec<(u32, String, String)>,
    /// How this window was opened (its id, opener, `parameters`).
    pub window_info: WindowInfo,
    /// The `<menu>` element the page chose as the application menu bar (`Window.this.menu`).
    pub app_menu: Option<NodeId>,
    /// A system menu bar shows `app_menu` (true on macOS and Windows, where the shell and C hosts can build
    /// one). False makes scripts draw an in-window bar: the default elsewhere, `NILE_MENU=inline` forces it
    /// (screenshots), `Page::set_native_menu` lets an embedder choose.
    pub native_menu: bool,
    /// Items chosen in the in-window bar, waiting for the page to run them.
    pub menu_invocations: Vec<u64>,
    /// The tray icon and dock/taskbar badge the page asked for (the shell shows them).
    pub tray: Option<TraySpec>,
    /// The global hotkeys the page registered, `(id, accelerator)`; the shell owns the registration.
    pub hotkeys: Vec<(u32, String)>,
    pub badge: Option<String>,
    pub progress: Progress,
    /// Popups that are shown in windows of their own (see `Portal`).
    pub portals: Vec<Portal>,
    /// A file chooser a click on `<input type=file>` asked for; the embedder answers with
    /// `Page::choose_files`.
    pub file_request: Option<FileRequest>,
    /// The embedder's native dialogs, for `Window.this.selectFile` and friends.
    pub dialogs: Option<Rc<dyn dialogs::Dialogs>>,
    /// The viewport changed since `resize` last fired.
    pub resized: bool,
    /// Where the window is: `[screenX, screenY, outerWidth, outerHeight, fullscreen]`, kept
    /// current by the window loop. `None` in headless runs.
    pub window_rect: Option<[f32; 5]>,
    /// The monitors and the index of the window's, kept current by the window loop (empty in headless runs).
    pub screens: (Vec<nile_script::ScreenInfo>, usize),
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

    /// Restyle after the interaction state went from `old` to `self.interaction`: only the subtrees
    /// whose elements changed state are cascaded again (a hover shouldn't re-cascade the whole page).
    /// Falls back to [`restyle`](Self::restyle) when that isn't possible (DOM changes pending,
    /// animations running, generated content).
    pub fn restyle_after_state(&mut self, old: InteractionState) {
        if !self.try_partial_restyle(old) {
            self.restyle();
        }
    }

    fn try_partial_restyle(&mut self, old: InteractionState) -> bool {
        let doc = self.doc.clone();
        let doc = doc.borrow();
        if doc.version != self.seen_version {
            return false;
        }
        let new = self.interaction;
        let deps = self.styles.state_deps;
        // Elements whose state differs: hover and active reach the target and its ancestors.
        let chain = |n: Option<NodeId>| {
            let mut v = Vec::new();
            let mut cur = n;
            while let Some(id) = cur {
                v.push(id);
                cur = doc.node(id).parent;
            }
            v
        };
        let mut changed: Vec<NodeId> = Vec::new();
        let mut diff = |a: Option<NodeId>, b: Option<NodeId>, on: bool, whole_chain: bool| {
            if !on || a == b {
                return;
            }
            if whole_chain {
                let (ca, cb) = (chain(a), chain(b));
                changed.extend(ca.iter().filter(|n| !cb.contains(n)).chain(cb.iter().filter(|n| !ca.contains(n))));
            } else {
                changed.extend(a.into_iter().chain(b));
            }
        };
        diff(old.hover, new.hover, deps.hover, true);
        diff(old.active, new.active, deps.active, true);
        diff(old.focus, new.focus, deps.focus, false);
        changed.sort_by_key(|n| n.0);
        changed.dedup();
        // Each changed element's own subtree, unless a sibling combinator can reach further: its parent's.
        let mut roots: Vec<NodeId> = changed
            .iter()
            .filter(|n| deps.sibling || !doc.node(**n).parent.is_some_and(|p| changed.contains(&p)))
            .map(|n| if deps.sibling { doc.node(*n).parent.unwrap_or(*n) } else { *n })
            .collect();
        roots.sort_by_key(|n| n.0);
        roots.dedup();
        // A root inside another root is covered by it.
        let covered = |r: NodeId| {
            let mut cur = doc.node(r).parent;
            while let Some(p) = cur {
                if roots.contains(&p) {
                    return true;
                }
                cur = doc.node(p).parent;
            }
            false
        };
        let roots: Vec<NodeId> = roots.iter().copied().filter(|r| !covered(*r)).collect();
        // While transitions or animations are set up, `base` has the targets and `styles` what shows now.
        let animating = self.base.is_some();
        let result = traced("style (partial)", || match &mut self.base {
            Some(base) => base.recascade(&doc, &new, self.viewport, &roots),
            None => self.styles.recascade(&doc, &new, self.viewport, &roots),
        });
        let Some(before) = result else { return false };
        if animating {
            // The targets are recomputed; the displayed copies follow them, then transitions start for these.
            let ids: Vec<NodeId> = before.iter().map(|(id, _)| *id).collect();
            let shown: Vec<nile_style::ComputedStyle> = ids.iter().map(|id| self.styles.get(*id).clone()).collect();
            let now = self.clock.elapsed().as_secs_f64();
            let base = self.base.as_ref().expect("animating");
            for id in &ids {
                self.styles.styles[id.0] = base.styles[id.0].clone();
            }
            self.animator.update_nodes(base, &mut self.styles, now, &ids);
            let keep = self.layout.is_some() && ids.iter().zip(&shown).all(|(id, o)| self.styles.get(*id).same_layout_with(o, true));
            match (&mut self.layout, keep) {
                (Some((_, _, tree)), true) => tree.refresh_paint(&self.styles),
                _ => self.layout = None,
            }
            return true;
        }
        // Nothing animated before; if the new rules start something, undo and let the full path set it up.
        if before.iter().any(|(id, _)| {
            let s = self.styles.get(*id);
            !s.transitions.is_empty() || !s.animations.is_empty()
        }) {
            for (id, old) in before {
                self.styles.styles[id.0] = old;
            }
            return false;
        }
        let keep = self.layout.is_some() && before.iter().all(|(id, o)| self.styles.get(*id).same_layout_with(o, true));
        match (&mut self.layout, keep) {
            (Some((_, _, tree)), true) => tree.refresh_paint(&self.styles),
            _ => self.layout = None,
        }
        true
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
            // Removing the focused element leaves nothing focused (keys go to the document again), as in a browser.
            let lost = self.interaction.focus.filter(|f| !self.doc.borrow().is_attached(*f));
            if let Some(old) = lost {
                self.interaction.focus = None;
                self.focus_changes.push((Some(old), None));
                self.restyle();
            }
        }
        changed
    }

    /// Where a scroll container with `scroll-snap-type` can come to rest on each axis (offsets).
    pub fn snap_points(&mut self, node: NodeId) -> (Vec<f32>, Vec<f32>) {
        use nile_style::SnapAlign;
        let style = self.styles.get(node).clone();
        let Some(sb) = self.layout().scroll_box(node).copied() else { return (Vec::new(), Vec::new()) };
        let (mx, my) = sb.max_offset();
        let pad = style.scroll_padding;
        let port = sb.clip;
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        let doc_rc = self.doc.clone();
        let doc = doc_rc.borrow();
        let inside = |mut n: NodeId| {
            while let Some(parent) = doc.node(n).parent {
                if parent == node {
                    return true;
                }
                n = parent;
            }
            false
        };
        let tree = self.layout().boxes.iter().filter(|b| matches!(b.content, nile_layout::BoxContent::Element)).map(|b| (b.node, b.rect)).collect::<Vec<_>>();
        for (n, r) in tree {
            let a = self.styles.get(n).scroll_snap_align;
            if (a.0 == SnapAlign::None && a.1 == SnapAlign::None) || n == node || !inside(n) {
                continue;
            }
            let at = |align: SnapAlign, start: f32, size: f32, port_start: f32, port_size: f32, pad_start: f32, pad_end: f32| match align {
                SnapAlign::None => None,
                SnapAlign::Start => Some(start - port_start - pad_start),
                SnapAlign::Center => Some(start + size / 2.0 - (port_start + (pad_start + port_size - pad_end) / 2.0)),
                SnapAlign::End => Some(start + size - (port_start + port_size) + pad_end),
            };
            if style.scroll_snap_type.x {
                if let Some(v) = at(a.0, r.x, r.width, port.x, port.width, pad.left, pad.right) {
                    xs.push(v.clamp(0.0, mx));
                }
            }
            if style.scroll_snap_type.y {
                if let Some(v) = at(a.1, r.y, r.height, port.y, port.height, pad.top, pad.bottom) {
                    ys.push(v.clamp(0.0, my));
                }
            }
        }
        for list in [&mut xs, &mut ys] {
            list.sort_by(f32::total_cmp);
            list.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        }
        (xs, ys)
    }

    /// Settle a snap container: on each snapping axis, the nearest snap point beyond where the
    /// gesture began in the direction it went (or the nearest one, if it came back).
    pub fn snap_settle(&mut self, pending: SnapPending) -> bool {
        let style = self.styles.get(pending.node).clone();
        let (xs, ys) = self.snap_points(pending.node);
        let (ox, oy) = self.scroll_offsets.get(&pending.node).copied().unwrap_or_default();
        let pick = |points: &[f32], origin: f32, now: f32, extent: f32| -> f32 {
            if points.is_empty() {
                return now;
            }
            let dir = (now - origin).signum();
            let nearest = |it: &mut dyn Iterator<Item = f32>| it.min_by(|a, b| (a - now).abs().total_cmp(&(b - now).abs()));
            let beyond = if (now - origin).abs() < 0.5 { None } else { nearest(&mut points.iter().copied().filter(|p| dir * (p - origin) > 0.5)) };
            let target = beyond.or_else(|| nearest(&mut points.iter().copied())).unwrap_or(now);
            // `proximity` only pulls when a point is near; `mandatory` always does.
            if style.scroll_snap_type.mandatory || (target - now).abs() <= extent * 0.3 {
                target
            } else {
                now
            }
        };
        let extent = self.layout().scroll_box(pending.node).map_or((300.0, 300.0), |sb| (sb.clip.width, sb.clip.height));
        let (nx, ny) = (pick(&xs, pending.from.0, ox, extent.0), pick(&ys, pending.from.1, oy, extent.1));
        self.set_element_scroll(pending.node, nx, ny)
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
        // Ordinary overflow scrolling only changes paint/hit-test offsets, not geometry.
        // Native list boxes bake their scroll into ListBox; a drop-down also needs clearing.
        let rebuild = self.menu.is_some() || self.layout.as_ref().is_some_and(|(_, _, tree)| tree.list_box(node).is_some());
        self.menu = None;
        if rebuild {
            self.layout = None;
        }
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
    pub fn set_preedit(&mut self, preedit: Option<nile_layout::Preedit>) {
        if self.preedit != preedit {
            self.preedit = preedit;
            self.layout = None;
        }
    }

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
            let options = LayoutOptions { preedit: self.preedit.clone(), menu: self.menu, scroll_y: self.scroll_y, offsets: Some(&self.scroll_offsets), images: Some(&self.images) };
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

/// A click on a file input: which one, and what it accepts.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRequest {
    pub node: NodeId,
    pub multiple: bool,
    /// The `accept` attribute (extensions like `.png` are honoured, MIME types are not).
    pub accept: String,
}

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

    fn system_dark(&self) -> bool {
        nile_style::system_theme().dark
    }

    fn image_bytes(&self, src: &str) -> Option<Vec<u8>> {
        nile_layout::ImageLoader::bytes(&self.0.borrow().images, src)
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

    fn page_info(&self, node: NodeId) -> Option<[usize; 2]> {
        let mut v = self.0.borrow_mut();
        v.sync();
        let pb = v.layout().pages.iter().find(|pb| pb.node == node)?.clone();
        let (doc, styles) = (v.doc.clone(), v.styles.clone());
        let caret = v.edit_sel.and_then(|(_, f)| v.layout().dom_caret_rect(&doc.borrow(), &styles, f)).filter(|(_, run)| *run > pb.index && *run < pb.end);
        let current = match caret {
            Some((r, _)) => pb.page_of_flow(r.y + 1.0),
            None => {
                let offsets = v.scroll_offsets.clone();
                let painted = v.layout().node_rect_scrolled(node, &offsets)?;
                let middle = v.scroll_y + v.viewport.1 / 2.0;
                (((middle - painted.y) / (pb.page_h + nile_layout::SHEET_GAP)).floor().max(0.0) as usize).min(pb.count() - 1)
            }
        };
        Some([pb.count(), current + 1])
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

    fn clipboard_get(&self, kind: &str) -> Option<String> {
        let mut c = self.0.borrow().clipboard.clone();
        match kind {
            "text" => c.text(),
            "html" => c.html().filter(|h| !h.trim().is_empty()),
            "json" => c.json(),
            "files" => Some(c.files().join("\n")).filter(|f| !f.is_empty()),
            _ => None,
        }
    }

    fn clipboard_get_image(&self) -> Option<(u32, u32, Vec<u8>)> {
        let mut c = self.0.borrow().clipboard.clone();
        c.image().map(|i| (i.width, i.height, i.rgba))
    }

    fn clipboard_put(&self, kind: &str, a: &str, b: Option<&str>) -> bool {
        let mut c = self.0.borrow().clipboard.clone();
        match kind {
            "text" => c.set_text(a),
            "html" => c.set_html(a, b.unwrap_or_default()),
            "json" => c.set_json(a, b),
            "files" => c.set_files(&a.split('\n').filter(|f| !f.is_empty()).map(str::to_string).collect::<Vec<_>>()),
            _ => return false,
        }
        true
    }

    fn clipboard_put_image(&self, width: u32, height: u32, rgba: &[u8]) -> bool {
        let mut c = self.0.borrow().clipboard.clone();
        c.set_image(&ClipImage { width, height, rgba: rgba.to_vec() });
        true
    }

    fn window_command(&self, cmd: WindowCommand) {
        self.0.borrow_mut().window_requests.push(cmd);
    }

    fn window_info(&self) -> WindowInfo {
        self.0.borrow().window_info.clone()
    }

    fn set_hotkeys(&self, keys: Vec<(u32, String)>) {
        self.0.borrow_mut().hotkeys = keys;
    }

    fn set_tray(&self, spec: Option<TraySpec>) {
        self.0.borrow_mut().tray = spec;
    }

    fn set_badge(&self, text: Option<String>) {
        self.0.borrow_mut().badge = text;
    }

    fn set_progress(&self, progress: Progress) {
        self.0.borrow_mut().progress = progress;
    }

    fn native_menu(&self) -> bool {
        self.0.borrow().native_menu
    }

    fn menu_invoke(&self, id: u64) {
        self.0.borrow_mut().menu_invocations.push(id);
    }

    fn set_app_menu(&self, node: Option<NodeId>) {
        let mut v = self.0.borrow_mut();
        v.app_menu = node;
        v.scroll_dirty = true;
    }

    fn print_request(&self, id: u32, path: &str, title: &str) {
        self.0.borrow_mut().print_requests.push((id, path.to_string(), title.to_string()));
    }

    fn portal_set(&self, node: NodeId, rect: [f32; 4]) {
        let mut v = self.0.borrow_mut();
        let rect = (rect[0], rect[1], rect[2], rect[3]);
        match v.portals.iter_mut().find(|p| p.node == node) {
            Some(p) => p.rect = rect,
            None => v.portals.push(Portal { node, rect }),
        }
        // The popup is no longer painted here: the cached layout is still right, the paint must run.
        v.scroll_dirty = true;
    }

    fn portal_clear(&self, node: NodeId) {
        let mut v = self.0.borrow_mut();
        v.portals.retain(|p| p.node != node);
        v.scroll_dirty = true;
    }

    fn dialog(&self, kind: &str, args: [&str; 4]) -> Option<String> {
        use dialogs::{parse_filter, MessageKind};
        // The borrow must not be held while the dialog blocks: a click handler may be mid-flight.
        let dialogs = self.0.borrow().dialogs.clone()?;
        let [a, b, c, d] = args;
        Some(match kind {
            "open" => dialogs.open_files(a == "open-multiple", &parse_filter(b), c, d).join("\n"),
            "save" => dialogs.save_file(&parse_filter(a), b, c, d).unwrap_or_default(),
            "folder" => dialogs.pick_folder(c, d).unwrap_or_default(),
            "message" => {
                let kind = match a {
                    "info" => MessageKind::Info,
                    "error" => MessageKind::Error,
                    "question" => MessageKind::Question,
                    _ => MessageKind::Alert,
                };
                if dialogs.message(kind, c, b) { "yes" } else { "no" }.to_string()
            }
            _ => return None,
        })
    }

    fn screens(&self) -> (Vec<nile_script::ScreenInfo>, usize) {
        let v = self.0.borrow();
        if v.screens.0.is_empty() {
            // Headless: one screen the size of the window.
            let (w, h) = v.viewport;
            let one = nile_script::ScreenInfo { name: "Screen".into(), x: 0, y: 0, width: w as u32, height: h as u32, scale: v.scale.max(1.0), primary: true };
            return (vec![one], 0);
        }
        v.screens.clone()
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

    fn value_synced(&self, node: NodeId) {
        let mut v = self.0.borrow_mut();
        if v.interaction.focus == Some(node) && v.value_at_focus.is_some() {
            let now = nile_layout::control_value(&v.doc.borrow(), node);
            v.value_at_focus = Some(now);
        }
    }

    fn set_focus(&self, node: Option<NodeId>) {
        let mut v = self.0.borrow_mut();
        if v.interaction.focus != node {
            let before = v.interaction;
            let old = before.focus;
            v.interaction.focus = node;
            if let Some(n) = node {
                v.select_for_focus(n);
            }
            v.focus_changes.push((old, node));
            let after = v.interaction;
            if v.styles.state_deps.affected_by(&before, &after) {
                v.restyle_after_state(before);
            }
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
    /// A resize handle of the selected image being dragged.
    pub image_drag: Option<editing::ImageDrag>,
    /// The x the caret keeps while moving up and down.
    pub edit_x: Option<f32>,
    /// The file input a chooser was last taken for.
    pub file_target: Option<NodeId>,
    /// The composition's own caret, as a byte range of the composed text.
    pub ime_cursor: Option<(usize, usize)>,
    /// A composition is showing as a span inside a contenteditable host.
    pub ime_host: bool,
    /// When the hover target is next recomputed after scrolling.
    pub hover_due: Option<Instant>,
    /// A scroll container with `scroll-snap-type` that was scrolled: when it settles, the offset it
    /// started the gesture at, and when to snap.
    pub snap_due: Option<SnapPending>,
    /// A press on a `draggable` element: the element and where the pointer went down. It becomes a drag after a few pixels.
    pub dnd_candidate: Option<(NodeId, f32, f32)>,
    /// The HTML5 drag in progress (see `dnd_start`).
    pub dnd: Option<Dnd>,
    /// Files the page's drag carries out of the window, for the shell to start a system drag with.
    drag_out: Option<Vec<String>>,
    /// List box row the shift-range started from, and the row keys moved to last.
    pub list_anchor: Option<(NodeId, usize)>,
    /// A list box being dragged over (the button went down on a row).
    pub list_drag: Option<NodeId>,
    /// The list box whose press already selected a row, so its click doesn't do it twice.
    pub list_press: Option<NodeId>,
    pub list_cursor: Option<(NodeId, usize)>,
    /// Time, position and count of the last primary click, for double/triple-click.
    pub last_click: Option<(Instant, (f32, f32), u32)>,
    pub clipboard: SharedClipboard,
    /// A mouse drag extending the page selection.
    pub page_drag: bool,
    /// A scrollbar thumb being dragged.
    pub bar_drag: Option<BarDrag>,
    /// The document version `prototype`s were last handed to scripts for.
    proto_version: Option<u64>,
    /// The element Tab is about to focus, so `focus_moved` can mark it `:tab-focus`.
    tab_focus_next: Option<NodeId>,
    /// The scrollbar under the pointer: its scroller (`None`: the page) and whether it is the vertical one.
    pub bar_hover: Option<(Option<NodeId>, bool)>,
    /// Undo/redo stacks of the text inputs edited so far.
    pub history: std::collections::HashMap<NodeId, History>,
    /// Blink phase of the caret and when it flips next.
    pub caret_visible: bool,
    pub blink_at: Instant,
    /// Sub-documents of `<frame>`/`<iframe>` elements.
    pub frames: Rc<RefCell<frames::Frames>>,
    /// When the tooltip for the hovered element shows, and whether one is showing.
    pub tooltip_due: Option<Instant>,
    pub tooltip_shown: bool,
    /// A frameset splitter being dragged.
    pub splitter_drag: Option<frames::SplitDrag>,
    /// Where each page of paged output starts (see `paginate`); empty until then.
    pub page_starts: Vec<f32>,
    /// The table header each page repeats at its top, if any.
    pub page_repeats: Vec<Option<nile_layout::Repeat>>,
    /// The table footer each page repeats at its bottom, if any.
    pub page_feet: Vec<Option<nile_layout::Repeat>>,
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
    /// Keep the page's `localStorage` in `file`. Pages loaded from a file already use one in the
    /// user's data directory (`NILE_STORAGE_DIR` moves it, `off` disables it) and their first
    /// scripts have run by the time this returns, so values read at startup come from there;
    /// what is in `file` is merged in over them.
    pub fn set_storage_file(&mut self, file: &std::path::Path) {
        if let Some(host) = &self.scripts {
            host.set_storage_file(file);
        }
    }

    /// The user asked to close the window (`reason`: 0 the title bar's button, 1 a script): fires
    /// `closerequest` at the window's listeners. True unless one of them called `preventDefault()`.
    pub fn close_request(&mut self, reason: u32) -> bool {
        match self.call("__closeRequest", &format!("[{reason}]")).0 {
            Ok(v) => v != "false",
            Err(_) => true,
        }
    }

    /// The window gained (`true`) or lost focus: fires `activate`.
    pub fn window_activated(&mut self, active: bool) -> bool {
        self.call("__windowActivate", &format!("[{}]", u32::from(active))).1
    }

    /// A `message` from another window (`from` is its id, `json` the data).
    pub fn window_message(&mut self, from: u32, json: &str) -> bool {
        self.call("__windowMessage", &format!("[{from},{json}]")).1
    }

    /// An application-level event for scripts: `second-instance` (`arguments`, `cwd`), `open-url` (`url`)
    /// or `open-file` (`path`), fired on the document and the window. `data_json` is a JSON object.
    pub fn application_event(&mut self, kind: &str, data_json: &str) -> bool {
        self.call("__applicationEvent", &format!("[\"{kind}\",{data_json}]")).1
    }

    /// A window this one opened has closed, with the value it passed to `close()` (JSON).
    pub fn window_closed(&mut self, id: u32, value_json: &str) -> bool {
        self.call("__windowClosed", &format!("[{id},{value_json}]")).1
    }

    /// Dialogs and a clipboard every page created on this thread starts with, so they exist before
    /// the page's own scripts run (`DOMContentLoaded` handlers can show a message box).
    pub fn set_defaults(dialogs: Option<Rc<dyn dialogs::Dialogs>>, clipboard: Option<fn() -> Box<dyn Clipboard>>) {
        DEFAULTS.with(|d| *d.borrow_mut() = (dialogs, clipboard));
    }

    /// Let scripts open native file choosers and message boxes.
    pub fn set_dialogs(&mut self, dialogs: Rc<dyn dialogs::Dialogs>) {
        self.view.borrow_mut().dialogs = Some(dialogs);
    }

    pub fn load(path: PathBuf, viewport: (f32, f32), scroll_y: f32) -> Page {
        Page::load_in(path, viewport, scroll_y, None, WindowInfo::default())
    }

    /// A page for a window a script opened: `info` says which window it is and who opened it.
    pub fn load_window(path: PathBuf, viewport: (f32, f32), info: WindowInfo) -> Page {
        Page::load_in(path, viewport, 0.0, None, info)
    }

    /// Like [`Page::load_window`] for markup given as text (`new Window({ html })`).
    pub fn html_window(html: &str, base_dir: PathBuf, viewport: (f32, f32), info: WindowInfo) -> Page {
        Page::from_source_in(html.to_string(), base_dir.join("index.html"), Some(html.to_string()), viewport, 0.0, None, info)
    }

    /// A page shown inside a `<frame>`: `outbox` collects what its scripts post to `window.parent`.
    pub(crate) fn load_embedded(path: PathBuf, viewport: (f32, f32), embed: frames::Embed) -> Page {
        Page::load_in(path, viewport, 0.0, Some(embed), WindowInfo::default())
    }

    pub(crate) fn html_embedded(html: &str, base_dir: PathBuf, viewport: (f32, f32), embed: frames::Embed) -> Page {
        Page::from_source_in(html.to_string(), base_dir.join("index.html"), Some(html.to_string()), viewport, 0.0, Some(embed), WindowInfo::default())
    }

    fn load_in(path: PathBuf, viewport: (f32, f32), scroll_y: f32, embed: Option<frames::Embed>, info: WindowInfo) -> Page {
        let html = nile_dom::vfs::read_to_string(&path).unwrap_or_else(|e| {
            format!("<body><h1>Could not open file</h1><p>{}: {e}</p></body>", path.display())
        });
        Page::from_source_in(html, path, None, viewport, scroll_y, embed, info)
    }

    /// A page from HTML text. Relative `src`, `href` and `url()` paths resolve against `base_dir`.
    pub fn load_html(html: &str, base_dir: PathBuf, viewport: (f32, f32)) -> Page {
        Page::from_source(html.to_string(), base_dir.join("index.html"), Some(html.to_string()), viewport, 0.0)
    }

    /// `memory`: the HTML to use again on reload, for pages that didn't come from a file.
    fn from_source(html: String, path: PathBuf, memory: Option<String>, viewport: (f32, f32), scroll_y: f32) -> Page {
        Page::from_source_in(html, path, memory, viewport, scroll_y, None, WindowInfo::default())
    }

    fn from_source_in(html: String, path: PathBuf, memory: Option<String>, viewport: (f32, f32), scroll_y: f32, embed: Option<frames::Embed>, info: WindowInfo) -> Page {
        // A frame's `content-style` goes on top of the document's own styles.
        let html = match embed.as_ref().and_then(|e| e.css.as_deref()) {
            Some(css) => format!("{html}<style>{css}</style>"),
            None => html,
        };
        let outbox = embed.map(|e| e.outbox);
        // `<include>` fragments are spliced in as text before parsing; paths are the page's folder's.
        let folder = path.parent().map(PathBuf::from).unwrap_or_default();
        let html = nile_dom::expand_includes(&html, &mut |src| {
            let file = folder.join(src);
            nile_dom::vfs::read_to_string(&file).map_err(|e| eprintln!("[include] could not load {}: {e}", file.display())).ok()
        });
        // Stylesheet links and `@import`s the same way (paths are the page's folder's).
        let html = nile_dom::inline_links(&html, &mut |href| {
            let mapped = nile_dom::vfs::url_to_path(href).map(|p| p.to_string_lossy().into_owned());
            let rel = mapped.as_deref().unwrap_or_else(|| nile_script::path_of_url(href));
            let file = if std::path::Path::new(rel).is_absolute() { PathBuf::from(rel) } else { folder.join(rel) };
            nile_dom::vfs::read_to_string(&file).map_err(|e| eprintln!("[css] could not load {}: {e}", file.display())).ok()
        });
        let frames: Rc<RefCell<frames::Frames>> = Rc::default();
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
            preedit: None,
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
            value_at_focus: None,
            interaction,
            page_sel: None,
            edit_sel: None,
            clipboard_requests: Vec::new(),
            clipboard: DEFAULTS.with(|d| d.borrow().1.map(|make| SharedClipboard::new(make()))).unwrap_or_default(),
            images: FileImages::new(path.parent().map(PathBuf::from).unwrap_or_default()),
            focus_changes: Vec::new(),
            window_requests: Vec::new(),
            print_requests: Vec::new(),
            file_request: None,
            dialogs: DEFAULTS.with(|d| d.borrow().0.clone()),
            resized: false,
            window_rect: None,
            screens: (Vec::new(), 0),
            window_info: info,
            app_menu: None,
            native_menu: cfg!(any(target_os = "macos", target_os = "windows")) && std::env::var("NILE_MENU").map_or(true, |v| v != "inline"),
            menu_invocations: Vec::new(),
            tray: None,
            hotkeys: Vec::new(),
            badge: None,
            progress: Progress::None,
            portals: Vec::new(),
        }));
        view.borrow_mut().adopt(styles);

        let scripts_result = ScriptHost::new(doc.clone(), Rc::new(Env(view.clone())))
            .map_err(|e| e.to_string()).and_then(|host| {
                host.discover_extensions(&path)?;
                Ok(host)
            });
        let scripts = match scripts_result {
            Ok(host) => {
                let sources = doc.borrow().script_sources();
                let dir = path.parent().unwrap_or(std::path::Path::new("."));
                host.set_fetcher(std::sync::Arc::new(net::PageFetcher::new(dir)));
                if let Some(outbox) = outbox.clone() {
                    host.register_host_fn("__parentPost", move |args| {
                        if let Some(json) = args.trim().strip_prefix('[').and_then(|a| a.strip_suffix(']')).and_then(editing::unjson_string) {
                            outbox.borrow_mut().push(json);
                        }
                        String::new()
                    });
                }
                frames::register_frame_functions(&host, &frames);
                let url = nile_dom::vfs::canonicalize(&path).ok().map(|p| nile_dom::vfs::app_url(&p).unwrap_or_else(|| nile_script::file_url(&p)));
                host.set_location(url.as_deref().unwrap_or("about:blank"));
                if memory.is_none() {
                    if let Some(file) = net::default_storage_file(&path) {
                        host.set_storage_file(file);
                    }
                }
                let read = |rel: &str| -> Option<(String, String)> {
                    let file = dir.join(rel);
                    match nile_dom::vfs::read_to_string(&file) {
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

        let clipboard = view.borrow().clipboard.clone();
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
            image_drag: None,
            edit_x: None,
            file_target: None,
            ime_cursor: None,
            ime_host: false,
            hover_due: None,
            snap_due: None,
            dnd_candidate: None,
            dnd: None,
            drag_out: None,
            list_anchor: None,
            list_drag: None,
            list_press: None,
            list_cursor: None,
            last_click: None,
            clipboard,
            history: std::collections::HashMap::new(),
            bar_drag: None,
            bar_hover: None,
            tab_focus_next: None,
            proto_version: None,
            page_drag: false,
            caret_visible: true,
            blink_at: Instant::now() + CARET_BLINK,
            frames,
            splitter_drag: None,
            page_starts: Vec::new(),
            page_repeats: Vec::new(),
            page_feet: Vec::new(),
            tooltip_due: None,
            tooltip_shown: false,
        }
    }

    /// Use `clipboard` for this page and its scripts (frames share their outer page's).
    pub fn share_clipboard(&mut self, clipboard: SharedClipboard) {
        self.view.borrow_mut().clipboard = clipboard.clone();
        self.clipboard = clipboard;
    }

    /// Tell scripts whether a system menu bar shows `Window.this.menu` (false: they draw one in the window).
    pub fn set_native_menu(&mut self, native: bool) {
        self.view.borrow_mut().native_menu = native;
    }

    /// Replace the system clipboard behind the one this page shares.
    pub fn set_clipboard(&mut self, system: Box<dyn Clipboard>) {
        self.clipboard.replace(system);
    }

    pub fn reload(&mut self) {
        let (viewport, scroll) = {
            let v = self.view.borrow();
            (v.viewport, v.scroll_y)
        };
        let clipboard = self.clipboard.clone();
        let dialogs = self.view.borrow().dialogs.clone();
        let info = self.view.borrow().window_info.clone();
        *self = match self.memory.take() {
            Some(html) => Page::from_source_in(html.clone(), self.path.clone(), Some(html), viewport, scroll, None, info),
            None => Page::load_in(self.path.clone(), viewport, scroll, None, info),
        };
        self.share_clipboard(clipboard);
        self.view.borrow_mut().dialogs = dialogs;
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

    /// The system theme (dark/light, accent colour; see `nile_style::set_system_theme`) changed:
    /// restyle this page and its frames so `prefers-color-scheme`, system colours and the accent apply.
    pub fn system_theme_changed(&mut self) {
        self.view.borrow_mut().restyle();
        // `matchMedia('(prefers-color-scheme: dark)')` lists fire `change` when their answer flipped.
        if let Some(host) = &self.scripts {
            let _ = host.call("__mediaCheck", "[]");
        }
        let frames = self.frames.clone();
        for slot in frames.borrow_mut().slots.values_mut() {
            slot.page.system_theme_changed();
        }
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
        self.apply_prototypes();
        let moved = !moves.is_empty();
        for (old, new) in moves {
            self.focus_moved(old, new);
        }
        let jobs = std::mem::take(&mut self.view.borrow_mut().print_requests);
        for (id, path, title) in jobs {
            self.run_print_job(id, &path, &title);
        }
        let requests = std::mem::take(&mut self.view.borrow_mut().clipboard_requests);
        for kind in requests {
            self.clipboard_command(&kind);
        }
        let chosen = std::mem::take(&mut self.view.borrow_mut().menu_invocations);
        for id in chosen {
            if let Some(role) = self.menu_click(id) {
                let command = match role.as_str() {
                    "quit" => Some(WindowCommand::Quit(0)),
                    "close" => Some(WindowCommand::Close),
                    "minimize" => Some(WindowCommand::Minimize),
                    "zoom" | "maximize" => Some(WindowCommand::ToggleMaximize),
                    _ => None,
                };
                if let Some(c) = command {
                    self.view.borrow_mut().window_requests.push(c);
                }
            }
        }
        for node in scrolled {
            self.emit(node, UiEvent::Plain("scroll"));
        }
        let delivered = self.deliver_frame_messages();
        // A canvas drawn on needs a repaint, not a new layout.
        changed | moved | delivered | nile_canvas::take_dirty()
    }

    /// Focus moved from `old` to `new`: end the old input's edit (firing
    /// `change` if its value changed), start the new one, and fire `blur`/`focus`.
    pub fn focus_moved(&mut self, old: Option<NodeId>, new: Option<NodeId>) {
        if old.is_some() {
            self.ime_cancel();
        }
        // `:tab-focus`: the element got the focus from the Tab key.
        let by_tab = self.tab_focus_next.take();
        {
            let mut doc = self.doc.borrow_mut();
            if let Some(o) = old {
                doc.set_state(o, "tabfocus", false);
            }
            if let Some(n) = new {
                doc.set_state(n, "tabfocus", by_tab == Some(n));
            }
        }
        if let Some(o) = old {
            let before = self.view.borrow_mut().value_at_focus.take();
            self.commit_value(o, before.clone());
            if before.is_some_and(|v| v != self.value_of(o)) {
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
            let at_focus = self.is_text_input(n).then(|| self.value_of(n));
            self.view.borrow_mut().value_at_focus = at_focus;
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
        self.tooltip_hide();
        if self.frames_wheel(dx, dy) {
            return;
        }
        if let Some((x, y)) = self.pointer {
            if self.menu_row_at(x, y).is_some() {
                self.scroll_menu(-dy);
                return;
            }
        }
        self.close_menu();
        let target = self.pointer.and_then(|(x, y)| self.hit_test(x, y));
        // A focused number input under the pointer takes the wheel as steps (up = increase).
        if dy != 0.0 {
            if let (Some(focus), Some(t)) = (self.interaction().focus.filter(|f| self.is_number(*f)), target) {
                let over = {
                    let doc = self.doc.borrow();
                    let mut cur = Some(t);
                    while cur.is_some_and(|c| c != focus) {
                        cur = doc.node(cur.unwrap()).parent;
                    }
                    cur.is_some()
                };
                if over {
                    self.number_step(focus, if dy > 0.0 { 1.0 } else { -1.0 });
                    return;
                }
            }
        }
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
                self.hover_after_scroll();
                let snaps = self.view.borrow().styles.get(n).scroll_snap_type;
                if snaps.x || snaps.y {
                    // Keep where the gesture began; snap once the wheel has been quiet for a moment.
                    let from = self.snap_due.filter(|p| p.node == n).map_or_else(
                        || {
                            let (ox, oy) = self.view.borrow().scroll_offsets.get(&n).copied().unwrap_or_default();
                            (ox + dx, oy + dy)
                        },
                        |p| p.from,
                    );
                    self.snap_due = Some(SnapPending { node: n, from, due: Instant::now() + Duration::from_millis(140) });
                }
                return;
            }
            cur = self.doc.borrow().node(n).parent;
        }
        {
            let mut v = self.view.borrow_mut();
            let max = (v.layout().content_height - v.viewport.1).max(0.0);
            v.scroll_y = (v.scroll_y - dy).clamp(0.0, max);
        }
        self.hover_after_scroll();
    }

    /// Content moved under a resting pointer. Re-cascading for `:hover` on every wheel tick is
    /// wasted work while it flies past, so (like a browser) the hover waits for the scrolling
    /// to pause; a click or a pointer move settles it at once.
    fn hover_after_scroll(&mut self) {
        self.hover_due = Some(Instant::now() + Duration::from_millis(90));
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
        let thick = v.styles.page_scrollbar().thickness;
        if let Some(t) = nile_layout::page_thumb(viewport, content, scroll, thick) {
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
                        || doc.has_state(id, "focusable")
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
        let bytes = nile_dom::vfs::read(&self.path.parent()?.join(src)).ok()?;
        Page::icon_from_bytes(&bytes)
    }

    /// An encoded picture (PNG, ...) as unpremultiplied RGBA pixels with its width and height.
    pub fn icon_from_bytes(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
        let img = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(bytes))?;
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

    /// The lowercase `type` of an `<input>` ("" for other elements or when unset).
    pub fn input_type(&self, id: NodeId) -> String {
        let doc = self.doc.borrow();
        let n = doc.node(id);
        if n.tag() == Some("input") { n.attr("type").map(|t| t.trim().to_ascii_lowercase()).unwrap_or_default() } else { String::new() }
    }

    /// Step the date/time field under the caret; fires `input` and `change`.
    fn temporal_step(&mut self, id: NodeId, dir: i32) -> bool {
        let kind = self.input_type(id);
        let value = self.value_of(id);
        let Some(next) = inputs::step(&kind, &value, self.caret(), dir) else { return false };
        let within = {
            let doc = self.doc.borrow();
            let n = doc.node(id);
            n.attr("min").is_none_or(|m| next.as_str() >= m) && n.attr("max").is_none_or(|m| next.as_str() <= m)
        };
        if !within || next == value {
            return false;
        }
        self.doc.borrow_mut().set_attr(id, "value", &next);
        self.view.borrow_mut().value_at_focus = Some(next);
        self.sync();
        self.emit(id, UiEvent::Plain("input"));
        self.emit(id, UiEvent::Plain("change"));
        true
    }

    /// A finished edit of a colour input: `#rgb` becomes `#rrggbb`, anything invalid goes back
    /// to what it was when editing started.
    fn commit_value(&mut self, id: NodeId, before: Option<String>) {
        if self.input_type(id) != "color" {
            return;
        }
        let now = self.value_of(id);
        let fixed = inputs::normalize_color(&now).or_else(|| before.as_deref().and_then(inputs::normalize_color)).unwrap_or_else(|| "#000000".into());
        if fixed != now {
            self.doc.borrow_mut().set_attr(id, "value", &fixed);
            self.sync();
        }
    }

    /// The file chooser a click asked for, if any (see `View::file_request`).
    pub fn take_file_request(&mut self) -> Option<FileRequest> {
        let request = self.view.borrow_mut().file_request.take();
        if let Some(r) = &request {
            self.file_target = Some(r.node);
            self.frames.borrow_mut().file_frame = None;
        }
        if request.is_none() {
            let ids: Vec<NodeId> = self.frames.borrow().slots.keys().copied().collect();
            for id in ids {
                if let Some(r) = self.with_frame(id, |p| p.take_file_request()).flatten() {
                    self.frames.borrow_mut().file_frame = Some(id);
                    return Some(r);
                }
            }
        }
        request
    }

    /// Answer the request last returned by `take_file_request`.
    pub fn choose_files_for_request(&mut self, paths: &[String]) -> bool {
        match self.file_target.take() {
            Some(node) => {
                self.choose_files(node, paths);
                true
            }
            None => false,
        }
    }

    /// Files dropped on the window from the desktop: a file input under the pointer (or the one
    /// a label under it points at) takes them, as if chosen in its dialog (only the first unless
    /// it is `multiple`); anywhere else the target gets a `drop` event whose `dataTransfer.files`
    /// holds them. Returns whether the page changed.
    pub fn drop_files(&mut self, paths: &[String]) -> bool {
        if paths.is_empty() {
            return false;
        }
        let Some((x, y)) = self.pointer else { return false };
        let Some(target) = self.hit_test(x, y) else { return false };
        let input = {
            let doc = self.doc.borrow();
            let is_file = |n: NodeId| doc.node(n).tag() == Some("input") && doc.node(n).attr("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("file"));
            let mut found = None;
            let mut cur = Some(target);
            while let Some(n) = cur {
                if is_file(n) {
                    found = Some(n);
                    break;
                }
                if doc.node(n).tag() == Some("label") {
                    found = match doc.node(n).attr("for") {
                        Some(id) => doc.find(|c| c.attr("id") == Some(id)),
                        None => doc.descendants(n).into_iter().find(|d| is_file(*d)),
                    }
                    .filter(|c| is_file(*c));
                    break;
                }
                cur = doc.node(n).parent;
            }
            found
        };
        if let Some(input) = input.filter(|i| !self.is_disabled(*i)) {
            let multiple = self.doc.borrow().node(input).attr("multiple").is_some();
            self.choose_files(input, if multiple { paths } else { &paths[..1] });
            return true;
        }
        let list: Vec<String> = paths.iter().map(|p| editing::json_string(p)).collect();
        let (_, changed) = self.call("__fileDrop", &format!("[{},[{}]]", target.0, list.join(",")));
        changed
    }

    /// Give a file input the files the user picked (an empty list clears it) and fire
    /// `input` and `change`.
    pub fn choose_files(&mut self, node: NodeId, paths: &[String]) {
        let file_frame = self.frames.borrow_mut().file_frame.take();
        if let Some(id) = file_frame {
            self.with_frame(id, |p| p.choose_files(node, paths));
            return;
        }
        let joined = paths.join("\n");
        {
            let mut doc = self.doc.borrow_mut();
            if paths.is_empty() {
                doc.remove_attr(node, "value");
                doc.remove_attr(node, "nile-files");
            } else {
                doc.set_attr(node, "value", &paths[0]);
                doc.set_attr(node, "nile-files", &joined);
            }
        }
        self.sync();
        self.emit(node, UiEvent::Plain("input"));
        self.emit(node, UiEvent::Plain("change"));
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
        self.view.borrow_mut().value_at_focus = Some(text);
        self.sync();
        self.emit(id, UiEvent::Plain("input"));
        self.emit(id, UiEvent::Plain("change"));
        true
    }

    /// Whether `id` is a date/time/colour input that isn't disabled or read-only.
    fn has_picker(&self, id: NodeId) -> bool {
        !self.is_disabled(id)
            && self.doc.borrow().node(id).attr("readonly").is_none()
            && self.doc.borrow().node(id).tag() == Some("input")
            && matches!(self.input_type(id).as_str(), "date" | "time" | "month" | "week" | "datetime-local" | "color")
    }

    /// Whether the pointer is over the part of a picker input that opens it: the glyph at the right
    /// edge (the swatch at the left for colours).
    fn picker_opener_at_pointer(&self, id: NodeId) -> bool {
        if !self.has_picker(id) {
            return false;
        }
        let Some((x, y)) = self.pointer else { return false };
        let rect = {
            let mut v = self.view.borrow_mut();
            let offsets = v.scroll_offsets.clone();
            v.layout().node_rect_scrolled(id, &offsets)
        };
        let Some(rect) = rect else { return false };
        let _ = y;
        if self.input_type(id) == "color" {
            x <= rect.x + 30.0
        } else {
            x >= rect.x + rect.width - 28.0
        }
    }

    /// Hand the elements CSS gave a `prototype: Class url(file.js)` to the script side, which loads the
    /// class and makes each element an instance of it (`__applyPrototypes` in web.js; already bound ones
    /// are skipped).
    fn apply_prototypes(&mut self) {
        let Some(host) = &self.scripts else { return };
        let version = self.doc.borrow().version;
        if self.proto_version == Some(version) {
            return;
        }
        let list = {
            let v = self.view.borrow();
            if !v.styles.has_prototypes {
                return;
            }
            v.styles.prototypes(&self.doc.borrow())
        };
        self.proto_version = Some(version);
        if list.is_empty() {
            return;
        }
        let items: Vec<String> = list.iter().map(|(n, class, url)| format!("[{},{},{}]", n.0, editing::json_string(class), editing::json_string(url))).collect();
        if let Err(e) = host.call("__applyPrototypes", &format!("[[{}]]", items.join(","))) {
            eprintln!("[prototype] {e}");
        }
    }

    /// Ask the page's script (`__pickerOpen` in web.js) to show the picker for an input.
    fn open_picker(&mut self, id: NodeId) {
        self.sync();
        let (result, _) = self.call("__pickerOpen", &format!("[{}]", id.0));
        if let Err(e) = result {
            eprintln!("[picker] {e}");
        }
    }

    /// Which spin arrow is under the pointer: `Some(1.0)` up, `Some(-1.0)` down.
    fn number_arrow_at_pointer(&self, id: NodeId) -> Option<f64> {
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
        let hovered = self.frames.borrow().hover;
        if let Some(cursor) = hovered.and_then(|id| self.frames.borrow().slots.get(&id).map(|s| s.page.cursor())) {
            return cursor;
        }
        use nile_style::Cursor;
        if let Some(d) = &self.dnd {
            return if d.accept { Cursor::Move } else { Cursor::NotAllowed };
        }
        if self.bar_drag.is_some() || self.view.borrow().menu.is_some() {
            return Cursor::Default;
        }
        if let Some(c) = self.image_handle_cursor() {
            return c;
        }
        match self.interaction().hover {
            // The spin arrows and the picker glyph are buttons, not text.
            Some(n) if self.is_number(n) && self.number_arrow_at_pointer(n).is_some() => Cursor::Default,
            Some(n) if self.picker_opener_at_pointer(n) => Cursor::Default,
            Some(n) => self.view.borrow().styles.get(n).cursor,
            None => Cursor::Auto,
        }
    }

    /// Re-run hit testing for the current pointer and scroll, firing
    /// `mouseout`/`mouseover` when the hovered element changes. True if the
    /// page needs repainting.
    pub fn refresh_hover(&mut self) -> bool {
        self.hover_due = None;
        let hover = self.pointer.and_then(|(x, y)| self.hit_test(x, y));
        let old = self.interaction().hover;
        if hover == old {
            return false;
        }
        let before = self.interaction();
        self.view.borrow_mut().interaction.hover = hover;
        self.tooltip_hover_changed(hover.is_some());
        {
            let mut v = self.view.borrow_mut();
            let after = v.interaction;
            if v.styles.state_deps.affected_by(&before, &after) {
                v.restyle_after_state(before);
            }
        }
        let (x, y) = self.pointer.unwrap_or_default();
        let mods = self.mod_bits();
        let mouse = |kind| UiEvent::Mouse { kind, x, y, button: 0, mods };
        if let Some(o) = old {
            self.emit(o, mouse("mouseout"));
        }
        if let Some(n) = hover {
            self.emit(n, mouse("mouseover"));
        }
        true
    }

    /// The nearest `draggable="true"` element at or above `node`.
    fn draggable_at_or_above(&self, node: NodeId) -> Option<NodeId> {
        let doc = self.doc.borrow();
        let mut cur = Some(node);
        while let Some(n) = cur {
            match doc.node(n).attr("draggable").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
                Some("true") => return Some(n),
                Some("false") => return None,
                _ => {}
            }
            cur = doc.node(n).parent;
        }
        None
    }

    /// The pointer moved far enough from a press on a draggable element: fire `dragstart`, and if scripts allow it
    /// the element is being dragged.
    fn dnd_start(&mut self, source: NodeId, px: f32, py: f32) -> bool {
        self.dnd_candidate = None;
        let (x, y) = self.pointer.unwrap_or((px, py));
        let started = self.call("__dndStart", &format!("[{},{x},{y}]", source.0)).0.is_ok_and(|r| r.trim() == "true");
        self.pressed = None;
        let next = InteractionState { active: None, ..self.interaction() };
        self.set_interaction(next);
        if !started {
            return true;
        }
        let (scroll, rect) = {
            let mut v = self.view.borrow_mut();
            let offsets = v.scroll_offsets.clone();
            (v.scroll_y, v.layout().node_rect_scrolled(source, &offsets))
        };
        let grab = rect.map_or((0.0, 0.0), |r| (px - r.x, py - (r.y - scroll)));
        self.dnd = Some(Dnd { source, over: None, accept: false, grab });
        self.dnd_move(x, y);
        true
    }

    fn dnd_move(&mut self, x: f32, y: f32) -> bool {
        let over = self.hit_test(x, y);
        let verdict = self.call("__dndMove", &format!("[{},{x},{y}]", over.map_or(0, |n| n.0))).0.unwrap_or_default();
        let accept = verdict.contains("\"accept\":true");
        if let Some(d) = &mut self.dnd {
            d.over = over;
            d.accept = accept;
        }
        true
    }

    /// The drag left the window: if it carries files, end it here and queue them for the system.
    fn dnd_out(&mut self, x: f32, y: f32) -> bool {
        let files = self.call("__dndOut", &format!("[{x},{y}]")).0.ok().and_then(|r| crate::editing::unjson_string(&r)).unwrap_or_default();
        if files.is_empty() {
            return false;
        }
        let dir = self.path.parent().map(PathBuf::from).unwrap_or_default();
        let paths: Vec<String> = files
            .lines()
            .map(|f| {
                let p = std::path::Path::new(f);
                if p.is_absolute() { f.to_string() } else { dir.join(p).to_string_lossy().into_owned() }
            })
            .collect();
        self.dnd = None;
        self.drag_out = Some(paths);
        self.refresh_hover();
        true
    }

    /// Files a drag took out of the window (`dataTransfer.setFiles`), once: the embedder starts the
    /// system's own drag session with them.
    pub fn take_drag_out(&mut self) -> Option<Vec<String>> {
        self.drag_out.take()
    }

    /// The button was released (`drop`) or Escape pressed: end the drag.
    fn dnd_end(&mut self, drop: bool) -> bool {
        let (x, y) = self.pointer.unwrap_or((0.0, 0.0));
        let accepted = self.dnd.as_ref().is_some_and(|d| d.accept);
        let _ = self.call("__dndEnd", &format!("[{},{x},{y}]", drop && accepted));
        self.dnd = None;
        self.refresh_hover();
        true
    }

    pub fn pointer_moved(&mut self, x: f32, y: f32) -> bool {
        self.pointer = Some((x, y));
        if self.splitter_drag.is_some() {
            return self.splitter_drag_to(x, y);
        }
        let in_frames = self.frames_pointer_moved(x, y);
        let moved = in_frames | self.pointer_moved_here(x, y);
        moved | self.update_bar_hover()
    }

    /// Track which scrollbar the pointer is over, for its hover look. Only looks when the pointer is
    /// in the page's bar strip or over a scroll container, so ordinary pointer moves stay cheap.
    fn update_bar_hover(&mut self) -> bool {
        let now = match self.pointer {
            Some((x, y)) if self.bar_drag.is_none() && self.near_a_bar(x) => self.scrollbar_at(x, y).map(|(n, t, _)| (n, t.vertical)),
            Some(_) if self.bar_drag.is_some() => self.bar_hover,
            _ => None,
        };
        let changed = self.bar_hover != now;
        self.bar_hover = now;
        changed
    }

    fn near_a_bar(&self, x: f32) -> bool {
        if self.bar_hover.is_some() {
            return true;
        }
        let v = self.view.borrow();
        if x >= v.viewport.0 - v.styles.page_scrollbar().thickness - 12.0 {
            return true;
        }
        // Without a laid-out page (it is rebuilt lazily) there is no bar to be over.
        let Some((_, _, tree)) = v.layout.as_ref() else { return false };
        let doc = v.doc.borrow();
        let mut cur = v.interaction.hover;
        while let Some(n) = cur {
            if tree.scroll_box(n).is_some() {
                return true;
            }
            cur = doc.node(n).parent;
        }
        false
    }

    fn pointer_moved_here(&mut self, x: f32, y: f32) -> bool {
        if self.dnd.is_some() {
            let (vw, vh) = self.view.borrow().viewport;
            if (x < 0.0 || y < 0.0 || x >= vw || y >= vh) && self.dnd_out(x, y) {
                return true;
            }
            return self.dnd_move(x, y);
        }
        if let Some((node, sx, sy)) = self.dnd_candidate {
            if self.pressed.is_some() && (x - sx).hypot(y - sy) >= 4.0 {
                return self.dnd_start(node, sx, sy);
            }
        }
        if self.image_drag.is_some() {
            return self.image_drag_to(x, y);
        }
        if self.bar_drag.is_some() {
            return self.bar_drag_to(x, y);
        }
        if let Some((node, _)) = self.range_drag.clone() {
            return self.range_from_pointer(node);
        }
        if let Some(select) = self.list_drag {
            return self.list_drag_to_pointer(select);
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
            changed |= self.emit(t, UiEvent::Mouse { kind: "mousemove", x, y, button: 0, mods: self.mod_bits() }).1;
        }
        changed
    }

    pub fn pointer_left(&mut self) -> bool {
        self.pointer = None;
        let in_frames = self.frames_pointer_left();
        let left_bar = self.bar_hover.take().is_some();
        in_frames | self.refresh_hover() | left_bar
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
        UiEvent::Mouse { kind, x, y, button: 0, mods: self.mod_bits() }
    }

    /// The held modifier keys as the bits `UiEvent::Mouse` carries (1 shift, 2 control, 4 alt, 8 meta).
    fn mod_bits(&self) -> u8 {
        u8::from(self.mods.shift_key()) | u8::from(self.mods.control_key()) << 1 | u8::from(self.mods.alt_key()) << 2 | u8::from(self.mods.super_key()) << 3
    }

    /// Primary button down: the hovered element becomes active and focus moves
    /// to its nearest focusable ancestor (or is cleared).
    /// The hovered element changed: drop the tooltip and wait for the pointer to rest on the new one.
    fn tooltip_hover_changed(&mut self, over_something: bool) {
        self.tooltip_hide();
        self.tooltip_due = over_something.then(|| Instant::now() + TOOLTIP_DELAY);
    }

    /// Hide the tooltip and stop waiting to show one. True if one was showing.
    fn tooltip_hide(&mut self) -> bool {
        self.tooltip_due = None;
        if std::mem::take(&mut self.tooltip_shown) {
            let _ = self.call("__tooltipHide", "[]");
            return true;
        }
        false
    }

    /// Show the tooltip once the pointer has rested long enough.
    fn tooltip_tick(&mut self) -> (bool, Option<Duration>) {
        let Some(due) = self.tooltip_due else { return (false, None) };
        let now = Instant::now();
        if now < due {
            return (false, Some(due - now));
        }
        self.tooltip_due = None;
        let (Some(target), Some((x, y))) = (self.interaction().hover, self.pointer) else { return (false, None) };
        if self.view.borrow().menu.is_some() || self.pressed.is_some() {
            return (false, None);
        }
        let (shown, changed) = self.call("__tooltipShow", &format!("[{},{x},{y}]", target.0));
        self.tooltip_shown = shown.is_ok_and(|v| v == "true");
        (changed || self.tooltip_shown, None)
    }

    /// A right click at the pointer: fires `contextmenu`, and shows the menu a script hands back.
    pub fn context_menu(&mut self) -> bool {
        if self.hover_due.is_some() {
            self.refresh_hover();
        }
        if let Some((x, y)) = self.pointer {
            if let Some(id) = self.frame_at(x, y) {
                let (lx, ly) = self.local(id, x, y);
                self.frames.borrow_mut().focus = Some(id);
                return self.with_frame(id, |p| {
                    p.pointer_moved(lx, ly);
                    p.context_menu()
                })
                .unwrap_or(false);
            }
        }
        let (Some(target), Some((x, y))) = (self.interaction().hover, self.pointer) else { return false };
        let _ = self.call("__contextMenu", &format!("[{},{x},{y}]", target.0));
        true
    }

    pub fn press(&mut self) -> bool {
        if self.hover_due.is_some() {
            self.refresh_hover();
        }
        if self.splitter_press() {
            return true;
        }
        let here = self.press_here();
        self.frames_press() | here
    }

    fn press_here(&mut self) -> bool {
        if self.hover_due.is_some() {
            self.refresh_hover(); // a click lands on what is under the pointer now
        }
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
        let mut changed = self.tooltip_hide();
        // A script that cancels `mousedown` keeps the focus and selection where they are (how
        // toolbar buttons avoid taking them from an editor).
        let mut keep_focus = false;
        // A press on empty canvas still reaches the document (popups close on it).
        let event_target = target.or_else(|| self.doc.borrow().body());
        self.dnd_candidate = None;
        if let Some(t) = event_target {
            let (proceed, c) = self.emit(t, self.mouse_event("mousedown"));
            changed |= c;
            keep_focus = !proceed;
            if proceed {
                if let (Some(source), Some((x, y))) = (self.draggable_at_or_above(t), self.pointer) {
                    self.dnd_candidate = Some((source, x, y));
                }
            }
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
        // A press on a list box row selects it now, so dragging can extend the choice.
        self.list_press = None;
        let list = self
            .interaction()
            .hover
            .and_then(|h| self.select_at_or_above(h))
            .filter(|s| nile_layout::list_rows(&self.doc.borrow(), *s).is_some() && !self.is_disabled(*s));
        if let Some(select) = list {
            changed |= self.list_click(select);
            self.list_drag = Some(select);
            self.list_press = Some(select);
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
        if self.splitter_drag.take().is_some() {
            return true;
        }
        let there = self.frames_release();
        self.release_here() | there
    }

    fn release_here(&mut self) -> bool {
        self.dnd_candidate = None;
        if self.dnd.is_some() {
            return self.dnd_end(true);
        }
        if self.bar_drag.take().is_some() {
            return true;
        }
        if let Some(d) = self.image_drag.take() {
            let _ = self.call("__imageEnd", &format!("[{}]", d.node.0));
            return true;
        }
        self.drag = None;
        self.list_drag = None;
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
        let (mut c0, mut n0) = (false, None);
        if let Some(due) = self.hover_due {
            let now = Instant::now();
            if now >= due {
                self.hover_due = None;
                c0 = self.refresh_hover();
            } else {
                n0 = Some(due - now);
            }
        }
        if let Some(p) = self.snap_due {
            let now = Instant::now();
            if now >= p.due {
                self.snap_due = None;
                c0 |= self.view.borrow_mut().snap_settle(p);
            } else {
                n0 = Some(n0.map_or(p.due - now, |d| d.min(p.due - now)));
            }
        }
        let (c1, n1) = self.run_timers();
        let (c2, n2) = self.blink();
        let (c3, n3) = self.animate();
        let (c4, n4) = self.frames_tick();
        let (c5, n5) = self.tooltip_tick();
        (c0 | c1 | c2 | c3 | c4 | c5, [n0, n1, n2, n3, n4, n5].into_iter().flatten().min())
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
                _ => doc.has_state(id, "focusable"),
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
        self.tab_focus_next = Some(order[next]);
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
        if let Some(id) = self.focused_frame() {
            return self.with_frame(id, |p| p.default_key(key, mods)).unwrap_or(false);
        }
        if self.view.borrow().app_menu.is_some() && self.menu_accelerator(key, mods) {
            return true;
        }
        if self.view.borrow().menu.is_none() && self.page_key(key, mods) {
            return true;
        }
        let changed = if self.view.borrow().menu.is_some() {
            self.menu_key(key)
        } else if key == "Tab" && self.focused_host().is_some_and(|host| self.edit_dom_key(host, key, mods)) {
            // Tab inside a table cell moved between cells.
            true
        } else if key == "Tab" {
            self.focus_next(!mods.shift_key())
        } else if let Some(f) = self.interaction().focus.filter(|f| matches!(key, "Enter" | " ") && self.is_button(*f)) {
            // Space or Enter presses a focused button like a click would.
            let (proceed, c) = self.emit(f, self.mouse_event("click"));
            c | (proceed && self.click_default(f))
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

    /// `<button>` and the `button`/`submit`/`reset` inputs.
    pub fn is_button(&self, id: NodeId) -> bool {
        let doc = self.doc.borrow();
        let n = doc.node(id);
        n.tag() == Some("button") || (n.tag() == Some("input") && matches!(n.attr("type"), Some("button" | "submit" | "reset")))
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
        nile_layout::select_options(&doc, m.select).into_iter().map(|o| (o, nile_layout::option_disabled(&doc, o))).collect()
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
        nile_layout::select_options(&doc, select).into_iter().map(|o| (o, nile_layout::option_disabled(&doc, o))).collect()
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

    /// The pointer moved with the button down on a list box: select from the pressed row to the
    /// one under the pointer (a single-choice list just follows it), scrolling when it leaves
    /// the box at the top or bottom.
    fn list_drag_to_pointer(&mut self, select: NodeId) -> bool {
        let Some((_, y)) = self.pointer else { return false };
        let Some((anchor_select, anchor)) = self.list_anchor else { return false };
        if anchor_select != select {
            return false;
        }
        let row = {
            let mut v = self.view.borrow_mut();
            let offsets = v.scroll_offsets.clone();
            let top = v.scroll_y;
            let Some(rect) = v.layout().node_rect_scrolled(select, &offsets) else { return false };
            let Some((rh, inset, view_h, scroll, max, count)) =
                v.layout().list_box(select).map(|l| (l.row_height, l.inset_top, l.view_height, l.scroll, l.max_scroll, l.items.len()))
            else {
                return false;
            };
            let rel = y + top - rect.y;
            // Past an edge: scroll a little each move.
            let mut scroll = scroll;
            if rel < inset {
                scroll = (scroll - rh / 2.0).max(0.0);
            } else if rel > inset + view_h {
                scroll = (scroll + rh / 2.0).min(max);
            }
            if scroll != v.layout().list_box(select).map_or(0.0, |l| l.scroll) {
                v.set_element_scroll(select, 0.0, scroll);
            }
            (((rel.clamp(inset, inset + view_h - 1.0) - inset + scroll) / rh).floor().max(0.0) as usize).min(count.saturating_sub(1))
        };
        let multiple = self.doc.borrow().node(select).attr("multiple").is_some();
        let disabled: Vec<bool> = self.list_options(select).iter().map(|o| o.1).collect();
        if disabled.get(row).copied().unwrap_or(true) {
            return false;
        }
        self.list_cursor = Some((select, row));
        if multiple {
            let (lo, hi) = (anchor.min(row), anchor.max(row));
            self.set_list_selection(select, |i| i >= lo && i <= hi && !disabled[i])
        } else {
            self.set_list_selection(select, |i| i == row)
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
            !nile_layout::option_disabled(&doc, options[i])
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
            nile_layout::select_options(&doc, select).into_iter().map(|o| (o, nile_layout::option_disabled(&doc, o))).collect()
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
        // Following a link marks it visited (`:visited`; there is no history, so only for this page's life).
        let link = {
            let doc = self.doc.borrow();
            let mut cur = Some(target);
            let mut found = None;
            while let Some(n) = cur {
                if doc.node(n).tag() == Some("a") && doc.node(n).attr("href").is_some() {
                    found = Some(n);
                    break;
                }
                cur = doc.node(n).parent;
            }
            found
        };
        if let Some(a) = link {
            self.doc.borrow_mut().set_state(a, "visited", true);
            self.sync();
        }
        // The glyph of a date, time or colour input opens its picker.
        if self.picker_opener_at_pointer(target) {
            self.open_picker(target);
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
                    Some("input") if matches!(ty.as_deref(), Some("checkbox" | "radio" | "file")) => {
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
                if self.list_press.take() == Some(select) {
                    return false; // the press already chose the row
                }
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
        if self.input_type(n) == "file" {
            let doc = self.doc.borrow();
            let node = doc.node(n);
            let request = FileRequest { node: n, multiple: node.attr("multiple").is_some(), accept: node.attr("accept").unwrap_or("").to_string() };
            drop(doc);
            self.view.borrow_mut().file_request = Some(request);
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
                    let kind = self.input_type(focus);
                    let text = if kind.is_empty() { text } else { text.chars().filter(|c| inputs::accepts(&kind, *c)).collect() };
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
            "ArrowDown" if mods.alt_key() && self.has_picker(focus) => {
                self.open_picker(focus);
                return true;
            }
            "ArrowUp" | "ArrowDown" if !multiline && self.is_number(focus) => {
                return self.number_step(focus, if key == "ArrowUp" { 1.0 } else { -1.0 });
            }
            "ArrowUp" | "ArrowDown" if !multiline && !readonly && inputs::is_temporal(&self.input_type(focus)) => {
                return self.temporal_step(focus, if key == "ArrowUp" { 1 } else { -1 });
            }
            "Enter" if multiline && !readonly => edit = Some((lo, hi, "\n".into())),
            "Enter" => {
                let before = self.view.borrow().value_at_focus.clone();
                self.commit_value(focus, before);
                // Commit: fire `change` if the value moved since focus.
                let moved = self.view.borrow().value_at_focus.as_ref().is_some_and(|v| *v != self.value_of(focus));
                if moved {
                    let now = self.value_of(focus);
                    self.view.borrow_mut().value_at_focus = Some(now);
                    return self.emit(focus, UiEvent::Plain("change")).1 | true;
                }
                return false;
            }
            k if !readonly && !command && k.chars().count() == 1 => {
                let c = k.chars().next().unwrap();
                if !c.is_control() && inputs::accepts(&self.input_type(focus), c) {
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

    /// Caret to draw: the focused text input and the caret's byte offset in its displayed text
    /// (inside the composition while an input method is composing).
    pub fn caret_for_paint(&self) -> Option<(NodeId, usize)> {
        let focus = self.interaction().focus.filter(|f| self.is_text_input(*f))?;
        if !self.caret_visible {
            return None;
        }
        let doc = self.doc.borrow();
        let mut byte = nile_layout::caret_byte(&doc, focus, self.caret());
        if let Some(p) = self.view.borrow().preedit.as_ref().filter(|p| p.node == focus) {
            byte = nile_layout::caret_byte(&doc, focus, p.at) + self.ime_cursor.map_or(p.text.len(), |c| c.1.min(p.text.len()));
        }
        Some((focus, byte))
    }

    /// The composition's byte range in its input's displayed text, for underlining.
    pub fn ime_underline(&self) -> Option<(NodeId, usize, usize)> {
        let v = self.view.borrow();
        let p = v.preedit.as_ref()?;
        let from = nile_layout::caret_byte(&self.doc.borrow(), p.node, p.at);
        Some((p.node, from, from + p.text.len()))
    }

    /// Whether the focused element takes text, so the window should enable its input method.
    pub fn ime_wanted(&self) -> bool {
        if let Some(id) = self.focused_frame() {
            return self.frames.borrow().slots[&id].page.ime_wanted();
        }
        if self.focused_host().is_some() {
            return true;
        }
        self.interaction().focus.is_some_and(|f| {
            self.is_text_input(f) && !self.is_disabled(f) && self.doc.borrow().node(f).attr("readonly").is_none()
        })
    }

    /// Where the input method should put its candidate window: the caret, in window
    /// coordinates `(x, y, width, height)`.
    pub fn ime_cursor_rect(&mut self) -> Option<(f32, f32, f32, f32)> {
        if let Some(id) = self.focused_frame() {
            let (fx, fy, ..) = self.frames.borrow().slots[&id].rect;
            return self.with_frame(id, |p| p.ime_cursor_rect()).flatten().map(|(x, y, w, h)| (x + fx, y + fy, w, h));
        }
        if let Some(_host) = self.focused_host() {
            let (_, f) = self.edit_selection()?;
            let (x, y, h) = self.edit_caret_painted(f)?;
            let scroll = self.view.borrow().scroll_y;
            return Some((x, y - scroll, 1.0, h));
        }
        let focus = self.interaction().focus.filter(|f| self.is_text_input(*f))?;
        let byte = {
            let doc = self.doc.borrow();
            let v = self.view.borrow();
            match v.preedit.as_ref().filter(|p| p.node == focus) {
                Some(p) => nile_layout::caret_byte(&doc, focus, p.at) + self.ime_cursor.map_or(p.text.len(), |c| c.1.min(p.text.len())),
                None => nile_layout::caret_byte(&doc, focus, self.caret()),
            }
        };
        let mut v = self.view.borrow_mut();
        let (offsets, scroll) = (v.scroll_offsets.clone(), v.scroll_y);
        let r = v.layout().input_caret_rect(focus, byte, &offsets)?;
        Some((r.x, r.y - scroll, r.width.max(1.0), r.height))
    }

    /// The input method is composing `text` (`cursor` is its caret as a byte range of `text`).
    /// An empty `text` ends the composition without inserting anything.
    pub fn ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) -> bool {
        if let Some(id) = self.focused_frame() {
            return self.with_frame(id, |p| p.ime_preedit(text, cursor)).unwrap_or(false);
        }
        if self.focused_host().is_some() {
            let ok = self.call("__imePreedit", &format!("[{}]", editing::json_string(text))).0.is_ok();
            self.ime_host = !text.is_empty();
            self.sync();
            return ok;
        }
        let Some(focus) = self.interaction().focus.filter(|f| self.is_text_input(*f) && !self.is_disabled(*f)) else { return false };
        if text.is_empty() {
            self.ime_cursor = None;
            self.view.borrow_mut().set_preedit(None);
            return true;
        }
        // A selection is what the composition will replace: it stays until commit.
        let len = self.value_of(focus).chars().count();
        let (a, c) = (self.anchor().min(len), self.caret().min(len));
        self.ime_cursor = cursor;
        self.view.borrow_mut().set_preedit(Some(nile_layout::Preedit { node: focus, at: a.min(c), end: a.max(c), text: text.to_string() }));
        true
    }

    /// The input method finished: `text` goes in where the composition was.
    pub fn ime_commit(&mut self, text: &str) -> bool {
        if let Some(id) = self.focused_frame() {
            return self.with_frame(id, |p| p.ime_commit(text)).unwrap_or(false);
        }
        self.ime_cursor = None;
        self.view.borrow_mut().set_preedit(None);
        if self.focused_host().is_some() {
            self.ime_host = false;
            let ok = self.call("__imeCommit", &format!("[{}]", editing::json_string(text))).0.is_ok();
            self.sync();
            self.reveal_edit_caret();
            return ok;
        }
        self.insert_text(text)
    }

    /// The composition was abandoned (focus moved, or the input method turned off).
    pub fn ime_cancel(&mut self) {
        if let Some(id) = self.focused_frame() {
            self.with_frame(id, |p| p.ime_cancel());
            return;
        }
        if std::mem::take(&mut self.ime_host) {
            // The composition lives in the page; the span goes wherever focus is now.
            let _ = self.call("__imePreedit", "[\"\"]");
            self.sync();
        }
        self.ime_cursor = None;
        self.view.borrow_mut().set_preedit(None);
    }

    /// Replace the selection of the focused text input with `text` (filtered to what the input
    /// takes) as one edit; an empty `text` just deletes the selection.
    pub fn insert_text(&mut self, text: &str) -> bool {
        if let Some(id) = self.focused_frame() {
            return self.with_frame(id, |p| p.insert_text(text)).unwrap_or(false);
        }
        let Some(focus) = self.interaction().focus.filter(|f| self.is_text_input(*f)) else { return false };
        if self.is_disabled(focus) || self.doc.borrow().node(focus).attr("readonly").is_some() {
            return false;
        }
        let kind = self.input_type(focus);
        let multiline = self.is_textarea(focus);
        let text: String = text
            .chars()
            .map(|c| if c == '\n' && !multiline { ' ' } else { c })
            .filter(|c| (!c.is_control() || (*c == '\n' && multiline)) && inputs::accepts(&kind, *c))
            .collect();
        let mut chars: Vec<char> = self.value_of(focus).chars().collect();
        let len = chars.len();
        let (anchor, caret) = (self.anchor().min(len), self.caret().min(len));
        let (lo, hi) = (anchor.min(caret), anchor.max(caret));
        if text.is_empty() && lo == hi {
            return false;
        }
        self.record_edit(focus, EditKind::Other, EditState { value: chars.iter().collect(), anchor, caret });
        chars.splice(lo..hi, text.chars());
        let value: String = chars.iter().collect();
        self.doc.borrow_mut().set_attr(focus, "value", &value);
        let end = lo + text.chars().count();
        self.set_sel(end, end);
        if let Some(h) = self.history.get_mut(&focus) {
            h.last = None;
        }
        self.sync();
        self.emit(focus, UiEvent::Plain("input"));
        true
    }

    /// The focused input's selected range as byte offsets in its displayed text.
    pub fn selection_for_paint(&self) -> Option<(NodeId, usize, usize)> {
        let focus = self.interaction().focus.filter(|f| self.is_text_input(*f))?;
        if self.view.borrow().preedit.is_some() {
            return None; // the composition stands in for it
        }
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
        if self.dnd.is_some() {
            // Escape cancels a drag; other keys wait for it to end.
            let cancel = kind == "keydown" && key == "Escape";
            return (false, cancel && self.dnd_end(false));
        }
        self.tooltip_hide();
        if let Some(r) = self.frames_key(kind, &key, mods) {
            return r;
        }
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
            self.view.borrow_mut().restyle_after_state(current);
        }
        if old_focus != new_focus {
            self.focus_moved(old_focus, new_focus);
        }
        true
    }

    /// The border box `[x, y, width, height]` (viewport px) of the page's maximize button, the first
    /// element with `role="window-maximize"` that has a box, for the embedder's hit testing (Windows
    /// Snap Layouts).
    pub fn maximize_button_rect(&self) -> Option<[f32; 4]> {
        let node = self.doc.borrow().find(|n| n.attr("role").map(str::trim) == Some("window-maximize"))?;
        let mut v = self.view.borrow_mut();
        v.sync();
        let offsets = v.scroll_offsets.clone();
        let r = v.layout().node_rect_scrolled(node, &offsets)?;
        (r.width > 0.0 && r.height > 0.0).then_some([r.x, r.y, r.width, r.height])
    }

    /// The corner radius (logical px) of the root `<html>` element: a frameless or translucent window
    /// is clipped to it, so the system's material and the page's own pixels share one rounded shape.
    pub fn window_corner_radius(&self) -> f32 {
        let Some(root) = self.doc.borrow().find(|n| n.tag() == Some("html")) else { return 0.0 };
        self.view.borrow().styles.try_get(root).map_or(0.0, |s| s.border_radius.tl.max(0.0))
    }

    /// CSS canvas background: the root element's background, or the body's if
    /// the root has none, fills the whole viewport. Defaults to white.
    pub fn canvas_background(&self) -> Color {
        ["html", "body"]
            .iter()
            .filter_map(|tag| self.doc.borrow().find(|n| n.tag() == Some(tag)))
            .find_map(|id| self.view.borrow().styles.get(id).background)
            .map(|c| {
                // A translucent window keeps the colour's alpha; an ordinary one shows it over white.
                if WindowConfig::from_document(&self.doc.borrow()).translucent() {
                    Color::from_argb(c.a, c.r, c.g, c.b)
                } else {
                    let a = c.a as u32;
                    let over = |v: u8| ((v as u32 * a + 255 * (255 - a)) / 255) as u8;
                    Color::from_argb(255, over(c.r), over(c.g), over(c.b))
                }
            })
            .unwrap_or_else(|| {
                // A transparent or material window shows what is behind it where the page has no background.
                if WindowConfig::from_document(&self.doc.borrow()).translucent() { Color::TRANSPARENT } else { Color::WHITE }
            })
    }

    /// Paint into a BGRA pixel buffer of `width`×`height` physical pixels.
    pub fn paint(&mut self, pixels: &mut [u8], width: u32, height: u32, scale: f32) {
        self.paint_inner(pixels, width, height, scale, None);
    }

    /// Paint one popup that has a window of its own: `node`'s subtree, with the page point `origin`
    /// at the buffer's top-left. The page keeps its own viewport size.
    pub fn paint_portal(&mut self, pixels: &mut [u8], width: u32, height: u32, scale: f32, node: NodeId, origin: (f32, f32)) {
        self.paint_inner(pixels, width, height, scale, Some((node, origin)));
    }

    fn paint_inner(&mut self, pixels: &mut [u8], width: u32, height: u32, scale: f32, portal: Option<(NodeId, (f32, f32))>) {
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
        if portal.is_none() {
            self.view.borrow_mut().scale = scale;
            self.layout(lw, lh);
            self.update_frames();
        }
        let bg = if portal.is_some() { Color::WHITE } else { self.canvas_background() };
        let caret = self.caret_for_paint();
        let selection = self.selection_for_paint();
        let dom_caret = self.dom_caret_for_paint();
        let ime_underline = self.ime_underline();
        let bar = match (&self.bar_drag, self.bar_hover) {
            (Some(d), _) => Some((d.node, d.thumb.vertical, true)),
            (None, Some((n, v))) => Some((n, v, false)),
            _ => None,
        };
        let selected_image = if portal.is_none() { self.selected_image_rect().map(|r| nile_layout::Rect { x: r.x, y: r.y, width: r.width, height: r.height }) } else { None };
        {
        let mut view = self.view.borrow_mut();
        view.layout();
        let page_selection = view.page_ranges();
        if portal.is_none() {
            let max_scroll = (view.layout().content_height - lh).max(0.0);
            view.scroll_y = view.scroll_y.clamp(0.0, max_scroll);
        }
        let scroll = view.scroll_y;
        let view = &*view;

        let canvas = surface.canvas();
        canvas.clear(bg);
        canvas.save();
        canvas.scale((scale, scale));
        if let Some((_, (ox, oy))) = portal {
            canvas.translate((-ox, -oy));
        }
        let tree = &view.layout.as_ref().expect("laid out above").2;
        let only = portal.and_then(|(node, _)| tree.subtree_range(node));
        // A popup shown in its own window is not also painted in this one.
        let skip: Vec<(usize, usize)> = if portal.is_none() { view.portals.iter().filter_map(|p| tree.subtree_range(p.node)).collect() } else { Vec::new() };
        if portal.is_some() && only.is_none() {
            canvas.restore();
            return;
        }
        traced("paint", || {
            self.renderer.paint(
                canvas,
                tree,
                &view.styles,
                &Overlay { scroll_y: scroll, offsets: &view.scroll_offsets, caret, selection, viewport: view.viewport, page_selection: &page_selection, dom_caret, preedit: ime_underline, only, skip: &skip, selected_image, bar },
            )
        });
        // A drag in progress: the dragged element, translucent, following the pointer.
        if let (Some(d), Some((px, py)), None) = (&self.dnd, self.pointer, portal) {
            if let (Some(range), Some(rect)) = (tree.subtree_range(d.source), tree.node_rect_scrolled(d.source, &view.scroll_offsets)) {
                let (dx, dy) = (px - d.grab.0 - rect.x, py - d.grab.1 - (rect.y - scroll));
                canvas.save();
                canvas.translate((dx, dy));
                canvas.save_layer_alpha_f(None, 0.65);
                self.renderer.paint(
                    canvas,
                    tree,
                    &view.styles,
                    &Overlay { scroll_y: scroll, offsets: &view.scroll_offsets, caret: None, selection: None, viewport: view.viewport, page_selection: &[], dom_caret: None, preedit: None, only: Some(range), skip: &[], selected_image: None, bar: None },
                );
                canvas.restore();
                canvas.restore();
            }
        }
        canvas.restore();
        }
        if portal.is_none() {
            self.paint_frames(surface.canvas(), scale);
        }
    }
}

#[cfg(test)]
mod tests;
