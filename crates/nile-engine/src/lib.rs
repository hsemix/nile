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
mod tests {
    use super::*;
    use crate::menus::{Accel, MenuItem};

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
    fn bootstrap_scripts_get_application_and_their_own_folder() {
        let dir = std::env::temp_dir().join(format!("nile-boot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("run.js"), "window.seen = [__DIR__.endsWith('/'), __FILE__.endsWith('/run.js'), application.run()].join(); application.start('gpu'); application.quit(3);").unwrap();
        let mut p = Page::html_window(r#"<html window-state="hidden"><script type="module" src="run.js"></script></html>"#, dir.clone(), (400.0, 300.0), nile_script::WindowInfo::default());
        assert_eq!(js(&mut p, "window.seen"), "true,true,0");
        assert_eq!(p.view.borrow_mut().window_requests.last(), Some(&nile_script::WindowCommand::Quit(3)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_ui_barcode_module_draws_codes_that_match_reference_encoders() {
        // Expected bars/rows come from python-barcode and qrcode (the encoders were also checked against
        // them over thousands of values: all of QR versions 1-40 at every level, EAN-13/8, UPC-A, Code 39,
        // and Code 128 decoded back to its text).
        let (mut p, path) = page(
            r#"<body><nile-barcode id="a" type="ean13" value="590123412345" height="40"></nile-barcode>
               <nile-barcode id="b" type="code39" value="@@"></nile-barcode>
            <script type="module">
              import { encode, svg, types } from "@nile/ui";
              const qr = encode("qr", "Nile", { ecc: "H", mask: 3 });
              window.out = [
                encode("ean13", "5901234123457").bars,
                encode("ean13", "590123412345").text,
                encode("code39", "nile").bars,
                qr.version + ":" + qr.rows.join(","),
                svg("code128", "ORDER-1042").startsWith("<svg"),
                types.join(" "),
              ].join(";");
              try { encode("ean13", "5901234123458"); } catch (e) { window.err = e.message; }
            </script></body>"#,
            "ui-barcode",
        );
        let want = [
            "10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101",
            "5901234123457",
            "10001011101110101010111010001110101110100011101010111010100011101110101110001010100010111011101",
            "1:111111100000001111111,100000100110001000001,101110100010101011101,101110100011101011101,101110101010101011101,100000100111101000001,111111101010101111111,000000001010100000000,001100111111011010000,100010001100000100001,110111111110100110011,010001000000101001011,111011110011000010011,000000001010100100101,111111101001001010000,100000100101110001100,101110100000010001111,101110101100011011010,101110101010100011100,100000100100100011001,111111100110000100000",
            "true",
            "code128 code39 ean13 ean8 upca qr",
        ];
        assert_eq!(js(&mut p, "window.out"), want.join(";"));
        assert!(js(&mut p, "window.err").contains("check digit is wrong"));
        // The element draws an <svg> inside itself, and flags a value it can't encode.
        assert_eq!(js(&mut p, "document.getElementById('a').innerHTML.startsWith('<svg')"), "true");
        assert_eq!(js(&mut p, "document.getElementById('b').hasAttribute('invalid')"), "true");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_drawer_opens_closes_and_keeps_focus() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><button id="t" data-drawer-toggle="#d">menu</button>
               <button id="o" data-drawer-open="#d" style="position:absolute;left:100px;top:0">open</button>
               <nile-drawer id="d" size="200"><button id="in">a</button><button id="x" data-drawer-close>close</button></nile-drawer>
            <script type="module">
              import "@nile/ui";
              window.ev = [];
              const d = window.d = document.getElementById("d");
              d.addEventListener("open", () => ev.push("open"));
              d.addEventListener("close", () => ev.push("close"));
              d.addEventListener("beforeclose", (e) => { if (window.hold) e.preventDefault(); });
            </script></body>"##,
            "ui-drawer",
        );
        let settle = |p: &mut Page, ms: u64| {
            std::thread::sleep(Duration::from_millis(ms));
            p.run_timers();
            p.run_timers();
        };
        let state = |p: &mut Page| js(p, "[d.opened, d.classList.contains('nile-shown'), d.classList.contains('nile-open'), document.activeElement && document.activeElement.id].join()");
        p.run_timers();
        assert_eq!(state(&mut p), "false,false,false,t".replace("t", "") + "", "closed at first");
        click(&mut p, "t");
        settle(&mut p, 40);
        assert_eq!(state(&mut p), "true,true,true,in", "the toggle button opens it and focus moves inside");
        // Escape closes it, and focus goes back to where it was.
        p.key("keydown", "Escape".into(), ModifiersState::default());
        settle(&mut p, 260);
        assert_eq!(state(&mut p), "false,false,false,t");
        // A listener can veto the close.
        js(&mut p, "window.hold = true; d.show()");
        settle(&mut p, 40);
        p.key("keydown", "Escape".into(), ModifiersState::default());
        settle(&mut p, 260);
        assert_eq!(js(&mut p, "d.opened"), "true", "beforeclose preventDefault keeps it open");
        js(&mut p, "window.hold = false");
        // The close button inside closes it; a bubbling `open`/`close` pair was fired each time.
        click(&mut p, "x");
        settle(&mut p, 260);
        assert_eq!(js(&mut p, "d.opened"), "false");
        assert_eq!(js(&mut p, "ev.join()"), "open,close,open,close");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_dialog_toast_and_shortcuts_work_together() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><input id="field"><button id="b" data-shortcut="F8" onclick="window.pressed = (window.pressed || 0) + 1">New</button>
            <script type="module">
              import { confirm, prompt, alert, dialog, toast, shortcuts } from "@nile/ui";
              window.ui = { confirm, prompt, alert, dialog, toast, shortcuts };
              window.hits = [];
              shortcuts.on("F3", () => hits.push("F3"));
              shortcuts.on("Mod+S, Alt+S", () => hits.push("save"), { label: "Save" });
              shortcuts.on("a", () => hits.push("a"));
              shortcuts.on("Escape", () => hits.push("esc-always"), { always: true });
            </script></body>"##,
            "ui-dialog",
        );
        let settle = |p: &mut Page, ms: u64| {
            std::thread::sleep(Duration::from_millis(ms));
            p.run_timers();
            p.run_timers();
        };
        p.run_timers();
        let key = |p: &mut Page, k: &str, m: ModifiersState| p.key("keydown", k.into(), m);
        let ctrl = if cfg!(target_os = "macos") { ModifiersState::SUPER } else { ModifiersState::CONTROL };

        // Shortcuts: a function key, a modified key, an alias list, a data-shortcut button.
        key(&mut p, "F3", ModifiersState::default());
        key(&mut p, "s", ctrl);
        key(&mut p, "s", ModifiersState::ALT);
        assert_eq!(js(&mut p, "hits.join()"), "F3,save,save");
        key(&mut p, "F8", ModifiersState::default());
        assert_eq!(js(&mut p, "window.pressed"), "1", "data-shortcut clicks the button");
        assert_eq!(js(&mut p, "document.getElementById('b').getAttribute('data-hint')"), "F8");
        // A plain letter fires from the page, not while typing in a field.
        key(&mut p, "a", ModifiersState::default());
        js(&mut p, "document.getElementById('field').focus()");
        key(&mut p, "a", ModifiersState::default());
        assert_eq!(js(&mut p, "hits.slice(3).join()"), "a");
        assert_eq!(js(&mut p, "ui.shortcuts.format('Alt+S')"), if cfg!(target_os = "macos") { "\u{2325}S" } else { "Alt+S" }.to_string());
        assert_eq!(js(&mut p, "ui.shortcuts.list().length"), "4");

        // confirm(): resolves true on OK, false on Escape; Escape reaches an `always` shortcut only when it isn't dialog-handled.
        js(&mut p, "window.r = []; ui.confirm('Void this order?', { title: 'Void', ok: 'Void', danger: true }).then((v) => r.push(v));");
        settle(&mut p, 40);
        assert_eq!(js(&mut p, "document.querySelectorAll('nile-dialog.nile-open').length"), "1");
        assert_eq!(js(&mut p, "document.querySelector('.nile-dialog-head').textContent"), "Void");
        assert_eq!(js(&mut p, "document.querySelector('.nile-dialog-text').textContent"), "Void this order?");
        assert_eq!(js(&mut p, "document.activeElement.className"), "nile-primary nile-danger", "the primary button has focus");
        // Shortcuts are quiet while the dialog is open.
        key(&mut p, "F3", ModifiersState::default());
        assert_eq!(js(&mut p, "hits.filter((h) => h === 'F3').length"), "1");
        key(&mut p, "Enter", ModifiersState::default());
        settle(&mut p, 220);
        assert_eq!(js(&mut p, "r.join()"), "true", "Enter presses the primary button");
        assert_eq!(js(&mut p, "document.querySelectorAll('nile-dialog').length"), "0", "the temporary dialog removes itself");
        assert_eq!(js(&mut p, "document.activeElement.id"), "field", "focus goes back");
        js(&mut p, "ui.confirm('again?').then((v) => r.push(v));");
        settle(&mut p, 40);
        key(&mut p, "Escape", ModifiersState::default());
        settle(&mut p, 220);
        assert_eq!(js(&mut p, "r.join()"), "true,false");

        // prompt(): validation keeps it open with the message, a good value resolves, cancel gives null.
        js(&mut p, "window.q = []; ui.prompt('Name?', { value: 'x', validate: (t) => t.length < 3 ? 'Too short' : '' }).then((v) => q.push(v));");
        settle(&mut p, 40);
        assert_eq!(js(&mut p, "document.activeElement.tagName + ':' + document.activeElement.value"), "INPUT:x");
        js(&mut p, "document.querySelector('nile-dialog button.nile-primary').click()");
        settle(&mut p, 30);
        assert_eq!(js(&mut p, "document.querySelector('.nile-dialog-error').textContent"), "Too short");
        assert_eq!(js(&mut p, "q.length"), "0");
        js(&mut p, "document.querySelector('nile-dialog input').value = 'Ada Lovelace'; document.querySelector('nile-dialog button.nile-primary').click()");
        settle(&mut p, 220);
        assert_eq!(js(&mut p, "q.join()"), "Ada Lovelace");
        js(&mut p, "ui.prompt('Name?').then((v) => q.push(v === null ? 'null' : v));");
        settle(&mut p, 40);
        js(&mut p, "document.querySelector('nile-dialog button:not(.nile-primary)').click()");
        settle(&mut p, 220);
        assert_eq!(js(&mut p, "q.join()"), "Ada Lovelace,null");

        // dialog(): custom buttons, a function value, tab stays inside.
        js(&mut p, "window.d = []; ui.dialog({ title: 'Print', message: 'Print now?', buttons: [{ label: 'No', value: 'no', cancel: true }, { label: 'Yes', value: 'yes', primary: true }] }).then((v) => d.push(v));");
        settle(&mut p, 40);
        key(&mut p, "Tab", ModifiersState::default());
        assert_eq!(js(&mut p, "document.activeElement.textContent"), "No", "Tab wraps around inside the dialog");
        js(&mut p, "document.querySelector('nile-dialog button').click()");
        settle(&mut p, 220);
        assert_eq!(js(&mut p, "d.join()"), "no");

        // toast(): shows, picks its type and role, closes by itself, and is capped.
        js(&mut p, "window.t = ui.toast.error('Printer offline', { duration: 600 })");
        settle(&mut p, 30);
        assert_eq!(js(&mut p, "[document.querySelector('.nile-toast').getAttribute('data-type'), document.querySelector('.nile-toast').getAttribute('role'), document.querySelector('.nile-toast-text').textContent].join()"), "error,alert,Printer offline");
        assert_eq!(js(&mut p, "document.querySelector('.nile-toast').classList.contains('nile-in')"), "true");
        settle(&mut p, 700);
        settle(&mut p, 250);
        assert_eq!(js(&mut p, "document.querySelectorAll('.nile-toast').length"), "0", "it goes away on its own");
        js(&mut p, "for (let i = 0; i < 8; i++) ui.toast('m' + i, { duration: 0 });");
        settle(&mut p, 250);
        settle(&mut p, 250);
        assert_eq!(js(&mut p, "document.querySelectorAll('.nile-toast').length"), "5", "at most five at a time");
        js(&mut p, "window.acted = 0; ui.toast.clear(); ui.toast('Deleted', { duration: 0, action: { label: 'Undo', onClick: () => acted++ } })");
        settle(&mut p, 250);
        js(&mut p, "document.querySelector('.nile-toast-action').click()");
        settle(&mut p, 250);
        assert_eq!(js(&mut p, "acted + ':' + document.querySelectorAll('.nile-toast').length"), "1:0");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_tabs_accordion_and_tiles_behave() {
        let (mut p, path) = page(
            r##"<body style="margin:0">
              <nile-tabs id="t" selected="b">
                <nile-tab label="One" name="a"><p id="pa">A</p></nile-tab>
                <nile-tab label="Two" name="b" badge="3"><p id="pb">B</p></nile-tab>
                <nile-tab label="Three" name="c" disabled><p>C</p></nile-tab>
                <nile-tab label="Four" name="d"><p>D</p></nile-tab>
              </nile-tabs>
              <nile-accordion id="acc" exclusive>
                <nile-section heading="S1" open><i id="s1">x</i></nile-section>
                <nile-section heading="S2"><i id="s2">y</i></nile-section>
              </nile-accordion>
              <nile-tiles id="tiles" columns="2" gap="4" style="width:300px">
                <nile-tile id="k1" label="Search" shortcut="F3" command="search"></nile-tile>
                <nile-tile id="k2" label="Pay" shortcut="F10" variant="success" badge="2"></nile-tile>
                <nile-tile id="k3" label="Wide" span="2" disabled></nile-tile>
              </nile-tiles>
            <script type="module">
              import "@nile/ui";
              for (const id of ["t", "acc", "tiles", "pa", "pb", "s1", "s2", "k1", "k2", "k3"]) window[id] = document.getElementById(id);
              window.log = [];
              t.addEventListener("tabchange", (e) => log.push("tab:" + e.detail.name));
              acc.addEventListener("sectionchange", (e) => log.push("sec:" + e.detail.index + (e.detail.open ? "+" : "-")));
              tiles.addEventListener("tile", (e) => log.push("tile:" + e.detail.command));
              window.vis = (el) => { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
            </script></body>"##,
            "ui-tabs",
        );
        p.run_timers();
        p.run_timers();
        let key = |p: &mut Page, k: &str| p.key("keydown", k.into(), ModifiersState::default());

        // Tabs: `selected` takes a name; the strip has a button per tab with a badge; only one panel is shown.
        assert_eq!(js(&mut p, "t.selectedIndex"), "1");
        assert_eq!(js(&mut p, "document.querySelectorAll('.nile-tab-button').length"), "4");
        assert_eq!(js(&mut p, "document.querySelector('.nile-tab-badge').textContent"), "3");
        assert_eq!(js(&mut p, "[vis(pa), vis(pb)].join()"), "false,true");
        js(&mut p, "document.querySelectorAll('.nile-tab-button')[0].click()");
        assert_eq!(js(&mut p, "[t.selectedIndex, vis(pa), vis(pb)].join()"), "0,true,false");
        // Arrow keys skip the disabled tab; Home/End jump; a disabled tab can't be selected.
        js(&mut p, "document.querySelectorAll('.nile-tab-button')[0].focus()");
        key(&mut p, "ArrowRight");
        key(&mut p, "ArrowRight");
        assert_eq!(js(&mut p, "t.selectedIndex"), "3", "Right from Two goes to Four, skipping Three");
        key(&mut p, "Home");
        assert_eq!(js(&mut p, "t.selectedIndex"), "0");
        js(&mut p, "t.select('c')");
        assert_eq!(js(&mut p, "t.selectedIndex"), "0");
        // beforetabchange can veto; select(name) works.
        js(&mut p, "window.hold = true; t.addEventListener('beforetabchange', (e) => { if (hold) e.preventDefault(); }); t.select('d')");
        assert_eq!(js(&mut p, "t.selectedIndex"), "0");
        js(&mut p, "hold = false; t.select('d')");
        assert_eq!(js(&mut p, "t.selectedTab.getAttribute('name')"), "d");
        assert_eq!(js(&mut p, "log.join()"), "tab:a,tab:b,tab:d,tab:a,tab:d");

        // Accordion: exclusive keeps one section open; the event says which.
        assert_eq!(js(&mut p, "[acc.sections[0].opened, acc.sections[1].opened, vis(s1), vis(s2)].join()"), "true,false,true,false");
        js(&mut p, "log.length = 0; document.querySelectorAll('.nile-section-head')[1].click()");
        assert_eq!(js(&mut p, "[acc.sections[0].opened, acc.sections[1].opened, vis(s1), vis(s2)].join()"), "false,true,false,true");
        assert_eq!(js(&mut p, "log.join()"), "sec:0-,sec:1+");
        js(&mut p, "document.querySelectorAll('.nile-section-head')[1].click()");
        assert_eq!(js(&mut p, "acc.sections[1].opened"), "false", "a click on an open heading closes it");
        assert_eq!(js(&mut p, "document.querySelectorAll('.nile-section-head')[0].getAttribute('aria-expanded')"), "false");

        // Tiles: a shortcut presses the tile; a click does too; the corner key text and span show; disabled does nothing.
        js(&mut p, "log.length = 0");
        key(&mut p, "F3");
        click(&mut p, "k2");
        click(&mut p, "k3");
        assert_eq!(js(&mut p, "log.join()"), "tile:search,tile:Pay");
        assert_eq!(js(&mut p, "k1.querySelector('.nile-tile-key').textContent"), "F3");
        assert_eq!(js(&mut p, "k2.querySelector('.nile-tile-badge').textContent"), "2");
        assert_eq!(js(&mut p, "k3.style.gridColumn"), "span 2");
        let w = |p: &mut Page, id: &str| js(p, &format!("Math.round(document.getElementById('{id}').getBoundingClientRect().width)"));
        assert_eq!(w(&mut p, "k1"), "148", "two columns of (300 - 4) / 2");
        assert_eq!(w(&mut p, "k3"), "300", "span 2 fills the row");
        js(&mut p, "k1.focus()");
        key(&mut p, "ArrowRight");
        assert_eq!(js(&mut p, "document.activeElement.id"), "k2");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_combobox_filters_picks_and_loads_from_a_source() {
        let (mut p, path) = page(
            r##"<body style="margin:0;height:300px"><input id="before"><nile-combobox id="c" placeholder="Product" style="width:240px">
                <option value="1" data-hint="2.50">Whole milk</option><option value="2">Sourdough loaf</option><option value="3" disabled>Sold out</option><option value="4">Soda water</option>
              </nile-combobox>
            <script type="module">
              import "@nile/ui";
              window.c = document.getElementById("c");
              window.log = [];
              c.addEventListener("change", (e) => log.push("change:" + e.detail.value));
              window.rows = () => Array.from(document.querySelectorAll('.nile-combo-option')).map((r) => r.textContent.replace(/\d\.\d\d$/, '')).join('|');
              window.listOpen = () => document.querySelector('.nile-combo-list').classList.contains('nile-open');
            </script></body>"##,
            "ui-combo",
        );
        p.run_timers();
        p.run_timers();
        let settle = |p: &mut Page, ms: u64| {
            std::thread::sleep(Duration::from_millis(ms));
            p.run_timers();
            p.run_timers();
        };
        let key = |p: &mut Page, k: &str| p.key("keydown", k.into(), ModifiersState::default());
        assert_eq!(js(&mut p, "document.querySelector('.nile-combo-input').getAttribute('placeholder')"), "Product");
        // Made through innerHTML, with its value attribute: the choice survives the late upgrade.
        js(&mut p, "const h = document.createElement('div'); h.id = 'made'; document.body.appendChild(h); h.innerHTML = '<nile-combobox id=\\'m\\' value=\\'2\\' placeholder=\\'Late\\'><option value=\\'1\\'>One</option><option value=\\'2\\'>Two</option></nile-combobox>'");
        settle(&mut p, 30);
        assert_eq!(js(&mut p, "const m = document.getElementById('m'); [m.value, m.text, m.selectedItem && m.selectedItem.label].join()"), "2,Two,Two");
        assert_eq!(js(&mut p, "listOpen()"), "false");
        // Focus opens it with everything listed (the hint shows beside the label).
        js(&mut p, "c.focus()");
        assert_eq!(js(&mut p, "listOpen()"), "true");
        assert_eq!(js(&mut p, "rows()"), "Whole milk|Sourdough loaf|Sold out|Soda water");
        // Typing filters: prefix matches first, the matching text is marked.
        p.insert_text("so");
        assert_eq!(js(&mut p, "rows()"), "Sourdough loaf|Sold out|Soda water");
        assert_eq!(js(&mut p, "document.querySelector('.nile-combo-match').textContent"), "So");
        // The first usable row is highlighted; Down skips the disabled one; Enter picks.
        assert_eq!(js(&mut p, "document.querySelector('.nile-combo-option.nile-active').textContent"), "Sourdough loaf");
        key(&mut p, "ArrowDown");
        assert_eq!(js(&mut p, "document.querySelector('.nile-combo-option.nile-active').textContent"), "Soda water");
        key(&mut p, "Enter");
        assert_eq!(js(&mut p, "[c.value, c.text, listOpen(), c.selectedItem && c.selectedItem.label, log.join()].join()"), "4,Soda water,false,Soda water,change:4");
        // Escape reverts typed text to the chosen item.
        p.insert_text("x");
        assert_eq!(js(&mut p, "rows()"), "", "nothing matches \"Soda waterx\" (the list shows the empty row)");
        key(&mut p, "Escape");
        assert_eq!(js(&mut p, "[c.text, listOpen()].join()"), "Soda water,false");
        // Blur with text that isn't an item reverts it; with an exact (case-insensitive) label it picks it.
        js(&mut p, "c.focus(); c.text = 'whole MILK'; document.getElementById('before').focus()");
        settle(&mut p, 20);
        assert_eq!(js(&mut p, "[c.value, c.text].join()"), "1,Whole milk");
        js(&mut p, "c.focus(); c.text = 'zzz'; document.getElementById('before').focus()");
        settle(&mut p, 20);
        assert_eq!(js(&mut p, "c.text"), "Whole milk");
        // value setter, allow-custom.
        js(&mut p, "c.value = '2'");
        assert_eq!(js(&mut p, "c.text"), "Sourdough loaf");
        js(&mut p, "c.setAttribute('allow-custom', ''); c.focus(); c.text = 'Gift card'; document.getElementById('before').focus()");
        settle(&mut p, 20);
        assert_eq!(js(&mut p, "[c.value, c.selectedItem, log[log.length - 1]].join()"), "Gift card,,change:Gift card");

        // source: loaded as the user types, only the newest answer is used.
        js(&mut p, "c.clear(); window.asked = []; c.source = (q) => { asked.push(q); return new Promise((ok) => setTimeout(() => ok(q === 'a' ? [{ value: 9, label: 'Apple' }] : [{ value: 7, label: 'Apricot', hint: 'kg' }, 'Avocado']), q === 'a' ? 120 : 10)); }; c.focus();");
        settle(&mut p, 200);
        p.insert_text("a");
        p.insert_text("p");
        settle(&mut p, 200);
        settle(&mut p, 40);
        assert_eq!(js(&mut p, "asked.join()"), ",ap", "opening asks for the initial list, then typing is debounced: only the settled text is asked for");
        assert_eq!(js(&mut p, "rows()"), "Apricotkg|Avocado");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_ribbon_builds_tabs_groups_menus_and_state() {
        let (mut p, path) = page(
            r##"<body style="margin:0;width:900px"><div id="host"></div>
            <script type="module">
              import { ribbon } from "@nile/ui";
              window.log = [];
              const cfg = {
                file: { label: "File", onClick: () => log.push("file") },
                quick: [{ label: "Save", command: "save", shortcut: "Mod+S", iconOnly: true, icon: '<svg viewBox="0 0 16 16"><rect width="16" height="16"/></svg>' }],
                tabs: [
                  { label: "Home", groups: [
                    { label: "Clipboard", items: [
                      { label: "Paste", size: "large", command: "paste", menu: [{ label: "Paste special", command: "pasteSpecial" }, { separator: true }, { label: "Plain", command: "plain", checked: true }] },
                      { type: "stack", items: [{ label: "Cut", command: "cut" }, { label: "Copy", command: "copy", disabled: true }] },
                    ] },
                    { label: "Font", items: [
                      { type: "stack", items: [{ type: "select", command: "font", options: ["Segoe UI", { value: "a", label: "Arial" }], value: "Segoe UI" },
                        { type: "row", items: [{ type: "toggle", label: "B", command: "bold" }] }] },
                    ] },
                  ] },
                  { label: "Insert", groups: [{ label: "Tables", items: [{ label: "Table", size: "large", command: "table", split: true, menu: [{ label: "Quick table", command: "quick" }] }] }] },
                  { label: "Table tools", contextual: true, accent: "#7e3fb4", hidden: true, groups: [{ label: "Rows", items: [{ label: "Add row", command: "addRow" }] }] },
                ],
              };
              window.r = ribbon(cfg);
              document.getElementById("host").appendChild(r);
              r.addEventListener("command", (e) => log.push(e.detail.command + (e.detail.checked !== undefined ? ":" + e.detail.checked : "") + (e.detail.value !== undefined ? "=" + e.detail.value : "")));
              r.addEventListener("tabchange", (e) => log.push("tab:" + e.detail.label));
              window.q = (s) => document.querySelector(s);
              window.qa = (s) => document.querySelectorAll(s).length;
              window.vis = (el) => { const b = el.getBoundingClientRect(); return b.width > 0 && b.height > 0; };
            </script></body>"##,
            "ui-ribbon",
        );
        p.run_timers();
        p.run_timers();
        let key = |p: &mut Page, k: &str, m: ModifiersState| p.key("keydown", k.into(), m);
        // Built: a file tab, three tab buttons (the contextual one hidden), one panel with its groups and labels.
        assert_eq!(js(&mut p, "qa('.nile-ribbon-tab')"), "4");
        assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-ribbon-tab')).filter(vis).map((t) => t.textContent).join()"), "File,Home,Insert");
        assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-ribbon-group-label')).map((t) => t.textContent).join()"), "Clipboard,Font");
        assert_eq!(js(&mut p, "q('.nile-ribbon-button.nile-large').textContent"), "Paste");
        assert_eq!(js(&mut p, "q('[data-command=copy]').disabled"), "true");
        assert_eq!(js(&mut p, "q('[data-command=save]').getAttribute('title')"), if cfg!(target_os = "macos") { "Save (\u{2318}S)" } else { "Save (Ctrl+S)" }.to_string());
        // Buttons raise `command`; toggles flip; a select reports its value; the file tab runs its handler.
        js(&mut p, "q('[data-command=cut]').click(); q('[data-command=bold]').click(); q('[data-command=bold]').click()");
        let sel = js(&mut p, "const s = q('.nile-ribbon-select'); s.value = 'a'; s.dispatchEvent(new Event('change', { bubbles: true })); log.join()");
        assert_eq!(sel, "cut,bold:true,bold:false,font=a");
        assert_eq!(js(&mut p, "q('[data-command=bold]').getAttribute('aria-pressed')"), "false");
        js(&mut p, "log.length = 0; q('.nile-ribbon-tab.nile-file').click()");
        assert_eq!(js(&mut p, "log.join()"), "file,file", "the file tab runs its onClick and raises a `command` event");
        // The Mod+S shortcut presses the quick-access button.
        js(&mut p, "log.length = 0");
        key(&mut p, "s", if cfg!(target_os = "macos") { ModifiersState::SUPER } else { ModifiersState::CONTROL });
        assert_eq!(js(&mut p, "log.join()"), "save");
        // A drop-down opens a menu; a row runs its command and closes it; the checkmark shows.
        js(&mut p, "log.length = 0; q('.nile-ribbon-button.nile-large').click()");
        assert_eq!(js(&mut p, "qa('.nile-ribbon-menu')"), "1");
        assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-ribbon-menu-item')).map((i) => i.textContent).join('|')"), "Paste special|\u{2713}Plain");
        key(&mut p, "ArrowDown", ModifiersState::default());
        key(&mut p, "Enter", ModifiersState::default());
        assert_eq!(js(&mut p, "log.join() + ':' + qa('.nile-ribbon-menu')"), "pasteSpecial:0");
        js(&mut p, "q('.nile-ribbon-button.nile-large').click()");
        key(&mut p, "Escape", ModifiersState::default());
        assert_eq!(js(&mut p, "qa('.nile-ribbon-menu')"), "0", "Escape closes the menu");
        // Tabs: selecting swaps the panel; a split button's two halves act separately; the contextual tab shows on demand.
        js(&mut p, "log.length = 0; r.select('Insert')");
        assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-ribbon-group-label')).map((t) => t.textContent).join() + '|' + r.selectedIndex + '|' + log.join()"), "Tables|1|tab:Insert");
        js(&mut p, "log.length = 0; q('[data-command=table]').click()");
        assert_eq!(js(&mut p, "log.join() + ':' + qa('.nile-ribbon-menu')"), "table:0", "the main half just acts");
        js(&mut p, "q('.nile-ribbon-drop').click()");
        assert_eq!(js(&mut p, "qa('.nile-ribbon-menu')"), "1", "the arrow half opens the menu");
        js(&mut p, "document.dispatchEvent(new Event('keydown'))");
        key(&mut p, "Escape", ModifiersState::default());
        js(&mut p, "r.showTab('Table tools', true)");
        assert_eq!(js(&mut p, "r.selectedIndex + ',' + q('.nile-ribbon-group-label').textContent + ',' + vis(document.querySelectorAll('.nile-ribbon-tab')[3])"), "2,Rows,true");
        js(&mut p, "r.hideTab('Table tools')");
        assert_eq!(js(&mut p, "r.selectedIndex"), "0", "hiding the active tab goes back to the first");
        // setState reaches items by command; collapsing hides the panel and a tab click pops it open.
        js(&mut p, "r.setState('copy', { disabled: false }); r.setState('bold', { checked: true })");
        assert_eq!(js(&mut p, "q('[data-command=copy]').disabled + ',' + q('[data-command=bold]').getAttribute('aria-pressed')"), "false,true");
        js(&mut p, "r.collapse()");
        assert_eq!(js(&mut p, "vis(q('.nile-ribbon-panel'))"), "false");
        js(&mut p, "document.querySelectorAll('.nile-ribbon-tab')[2].click()");
        assert_eq!(js(&mut p, "vis(q('.nile-ribbon-panel')) + ',' + q('.nile-ribbon-group-label').textContent"), "true,Tables");
        js(&mut p, "q('[data-command=table]').click()");
        assert_eq!(js(&mut p, "vis(q('.nile-ribbon-panel'))"), "false", "a command closes the pop-up panel");
        js(&mut p, "r.expand()");
        assert_eq!(js(&mut p, "vis(q('.nile-ribbon-panel'))"), "true");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_chart_draws_series_hovers_and_toggles_from_the_legend() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><nile-chart id="ch" type="bar" height="200" style="width:400px"></nile-chart>
            <script type="module">
              import { niceScale } from "@nile/ui";
              window.niceScale = niceScale;
              window.ch = document.getElementById("ch");
              window.hits = [];
              ch.addEventListener("chartclick", (e) => hits.push(e.detail.label + ":" + e.detail.series.map((s) => s.name + "=" + s.value).join("+")));
              ch.data = { labels: ["A", "B", "C"], series: [{ name: "One", values: [10, 40, 25] }, { name: "Two", values: [5, 20, 35] }] };
              window.count = (needle) => (ch.innerHTML.match(new RegExp(needle, "g")) || []).length;
            </script></body>"##,
            "ui-chart",
        );
        p.run_timers();
        p.run_timers();
        assert_eq!(js(&mut p, "JSON.stringify(niceScale(0, 18000, 5).ticks)"), "[0,5000,10000,15000,20000]");
        assert_eq!(js(&mut p, "niceScale(0, 40, 5).max"), "40");
        // Two series x three categories: six bars; the legend names both series.
        assert_eq!(js(&mut p, "count('<rect')"), "6");
        assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-chart-key')).map((k) => k.textContent).join()"), "One,Two");
        // Clicking a legend entry hides that series (and the value axis rescales).
        js(&mut p, "document.querySelectorAll('.nile-chart-key')[1].click()");
        assert_eq!(js(&mut p, "count('<rect')"), "3");
        assert_eq!(js(&mut p, "document.querySelectorAll('.nile-chart-key')[1].classList.contains('nile-off')"), "true");
        js(&mut p, "document.querySelectorAll('.nile-chart-key')[1].click()");
        assert_eq!(js(&mut p, "count('<rect')"), "6");
        // Hover over category B: tooltip with both values; click raises chartclick.
        let at = js(&mut p, "const g = ch._geo; (g.left + g.w * 1.5 / g.n) + ',' + (g.top + g.h / 2)");
        let mut it = at.split(',').map(|v| v.parse::<f32>().unwrap());
        let (x, y) = (it.next().unwrap(), it.next().unwrap());
        p.pointer_moved(x, y);
        assert_eq!(js(&mut p, "const t = document.querySelector('.nile-chart-tooltip'); t.style.display + '|' + t.textContent"), "block|BOne: 40Two: 20");
        p.press();
        p.release();
        assert_eq!(js(&mut p, "hits.join()"), "B:One=40+Two=20");
        p.pointer_moved(x, 150.0 + 40.0);
        // Pie: slices from the first series, legend by label, a slice's tooltip shows its share.
        js(&mut p, "ch.setAttribute('type', 'pie'); ch.data = [{ label: 'Cash', value: 75 }, { label: 'Card', value: 25 }];");
        assert_eq!(js(&mut p, "count('<path')"), "2");
        assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-chart-key')).map((k) => k.textContent).join()"), "Cash,Card");
        let g = js(&mut p, "ch._geo.cx + ',' + ch._geo.cy + ',' + ch._geo.r");
        let v: Vec<f32> = g.split(',').map(|n| n.parse().unwrap()).collect();
        p.pointer_moved(v[0] - v[2] * 0.35, v[1] - v[2] * 0.35);
        assert_eq!(js(&mut p, "document.querySelector('.nile-chart-tooltip').textContent"), "Card25%: 25", "10:30 is inside the last quarter");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_grid_edits_cells_and_resizes_columns_from_the_keyboard() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><nile-grid id="g" key="id" style="width:560px;height:220px"></nile-grid>
            <script type="module">
              import "@nile/ui";
              window.g = document.getElementById("g");
              g.columns = [
                { key: "name", title: "Item", width: 200, editable: true },
                { key: "qty", title: "Qty", width: 80, type: "number", editable: true, validate: (v) => (v === null || v >= 0 ? true : "Not negative") },
                { key: "price", title: "Price", width: 80, type: "number" },
                { key: "amount", title: "Amount", width: 100, type: "number", value: (r) => r.qty * r.price },
                { key: "paid", title: "Paid", width: 60, type: "boolean", editable: true, format: (v) => (v ? "yes" : "no") },
                { key: "size", title: "Size", editable: true, editor: "select", options: ["S", "M", { value: "L", label: "Large" }] },
              ];
              g.rows = [{ id: 1, name: "Milk", qty: 2, price: 2.5, paid: false, size: "S" }, { id: 2, name: "Bread", qty: 1, price: 3, paid: true, size: "M" }, { id: 3, name: "Eggs", qty: 6, price: 0.5, paid: false, size: "L" }];
              window.log = [];
              g.addEventListener("cellchange", (e) => log.push(e.detail.key + ":" + e.detail.oldValue + ">" + e.detail.value));
              g.addEventListener("celleditcancel", () => log.push("cancel"));
              g.addEventListener("columnresize", (e) => log.push("resize:" + e.detail.key + "=" + e.detail.width));
              window.shown = () => Array.from(document.querySelectorAll(".nile-grid-row")).filter((x) => x.style.display !== "none").sort((a, b) => Number(a.getAttribute("aria-rowindex")) - Number(b.getAttribute("aria-rowindex")));
              window.cell = (r, c) => shown()[r].children[c];
              window.rowOf = (id) => g.rows.find((x) => x.id === id);
            </script></body>"##,
            "ui-grid-edit",
        );
        p.run_timers();
        p.run_timers();
        let settle = |p: &mut Page| {
            std::thread::sleep(Duration::from_millis(30));
            p.run_timers();
            p.run_timers();
        };
        let key = |p: &mut Page, k: &str, m: ModifiersState| p.key("keydown", k.into(), m);
        let none = ModifiersState::default();
        let click_cell = |p: &mut Page, r: usize, c: usize| {
            let v = js(p, &format!("(() => {{ const b = cell({r}, {c}).getBoundingClientRect(); return (b.x + 6) + ',' + (b.y + b.height / 2); }})()"));
            let mut it = v.split(',').map(|n| n.parse::<f32>().unwrap_or_else(|_| panic!("click_cell({r},{c}): {v}")));
            let (x, y) = (it.next().unwrap(), it.next().unwrap());
            p.pointer_moved(x, y);
            p.press();
            p.release();
        };
        // Column widths come through two custom properties on the grid, not per-row styles.
        assert!(js(&mut p, "g.style.getPropertyValue('--nile-grid-cols')").starts_with("200px 80px 80px 100px 60px"));

        // A click sets the active cell; F2 opens the editor with the value; Enter commits and moves down.
        click_cell(&mut p, 0, 0);
        key(&mut p, "F2", none);
        assert_eq!(js(&mut p, "g.editing && g.editing.key + g.editing.index"), "name0");
        assert_eq!(js(&mut p, "document.activeElement.className + ':' + document.activeElement.value"), "nile-grid-editor:Milk");
        js(&mut p, "document.activeElement.value = 'Oat milk'");
        key(&mut p, "Enter", none);
        settle(&mut p);
        assert_eq!(js(&mut p, "[rowOf(1).name, cell(0, 0).textContent, g.editing, g.selectedIndex].join()"), "Oat milk,Oat milk,,1");
        assert_eq!(js(&mut p, "log.join()"), "name:Milk>Oat milk");

        // Escape cancels and keeps the old value.
        click_cell(&mut p, 1, 0);
        key(&mut p, "F2", none);
        js(&mut p, "document.activeElement.value = 'zzz'");
        key(&mut p, "Escape", none);
        assert_eq!(js(&mut p, "[rowOf(2).name, cell(1, 0).textContent, g.editing, log[log.length - 1]].join()"), "Bread,Bread,,cancel");

        // Typing starts an edit with that character; numbers parse; computed cells refresh; Tab goes to the next editable cell.
        click_cell(&mut p, 0, 1);
        key(&mut p, "5", none);
        assert_eq!(js(&mut p, "g.editing.key + ':' + document.activeElement.value"), "qty:5");
        key(&mut p, "Tab", none);
        settle(&mut p);
        assert_eq!(js(&mut p, "[rowOf(1).qty, cell(0, 3).textContent, g.editing && g.editing.key].join()"), "5,12.5,paid".replace("paid", "size"), "Tab skips the check box and the read-only cells");
        key(&mut p, "Escape", none);

        // Invalid input keeps the editor open and marked; fixing it commits.
        click_cell(&mut p, 1, 1);
        key(&mut p, "F2", none);
        js(&mut p, "document.activeElement.value = '-3'");
        key(&mut p, "Enter", none);
        assert_eq!(js(&mut p, "g.editing.key + ',' + document.activeElement.classList.contains('nile-invalid') + ',' + document.activeElement.getAttribute('title')"), "qty,true,Not negative");
        js(&mut p, "document.activeElement.value = 'abc'");
        key(&mut p, "Enter", none);
        assert_eq!(js(&mut p, "document.activeElement.getAttribute('title')"), "Enter a number");
        js(&mut p, "document.activeElement.value = '4'");
        key(&mut p, "Enter", none);
        settle(&mut p);
        assert_eq!(js(&mut p, "[rowOf(2).qty, cell(1, 3).textContent, g.editing].join()"), "4,12,");

        // A boolean cell toggles with Space or a click; a select editor writes the option's own value.
        click_cell(&mut p, 2, 4);
        assert_eq!(js(&mut p, "[rowOf(3).paid, cell(2, 4).textContent].join()"), "true,yes", "a click toggles it");
        key(&mut p, " ", none);
        assert_eq!(js(&mut p, "rowOf(3).paid"), "false", "Space toggles it too");
        click_cell(&mut p, 0, 5);
        key(&mut p, "Enter", none);
        assert_eq!(js(&mut p, "document.activeElement.tagName"), "SELECT");
        js(&mut p, "document.activeElement.value = 'L'");
        js(&mut p, "g.commitEdit()");
        assert_eq!(js(&mut p, "rowOf(1).size + ',' + cell(0, 5).textContent"), "L,L");

        // Editing that changes the sorted column moves the row but the active row follows it.
        js(&mut p, "g.sortBy('name', 'asc'); log.length = 0");
        settle(&mut p);
        assert_eq!(js(&mut p, "g.rows.length + ',' + cell(0, 0).textContent"), "3,Bread");
        js(&mut p, "g.edit(0, 'name'); document.activeElement.value = 'Zucchini'");
        key(&mut p, "Enter", none);
        settle(&mut p);
        assert_eq!(js(&mut p, "shown().map((r) => r.children[0].textContent).join()"), "Eggs,Oat milk,Zucchini");

        // Column headers: Shift+Right widens the focused column by 10px (Ctrl adds more); Left/Right move focus; Enter sorts.
        js(&mut p, "log.length = 0; document.querySelectorAll('.nile-grid-hcell')[1].focus()");
        key(&mut p, "ArrowRight", ModifiersState::SHIFT);
        key(&mut p, "ArrowRight", ModifiersState::SHIFT);
        key(&mut p, "ArrowLeft", ModifiersState::SHIFT);
        assert_eq!(js(&mut p, "log.join()"), "resize:qty=90,resize:qty=100,resize:qty=90");
        assert!(js(&mut p, "g.style.getPropertyValue('--nile-grid-cols')").starts_with("200px 90px"));
        key(&mut p, "ArrowLeft", ModifiersState::SHIFT);
        for _ in 0..6 {
            key(&mut p, "ArrowLeft", ModifiersState::SHIFT);
        }
        assert_eq!(js(&mut p, "g.columns[1]._w"), "60", "never narrower than the column's minimum");
        key(&mut p, "ArrowRight", none);
        assert_eq!(js(&mut p, "document.activeElement.textContent.indexOf('Price') === 0"), "true");
        key(&mut p, "Enter", none);
        assert_eq!(js(&mut p, "g.sortKey + ',' + g.sortDirection"), "price,asc");
        assert_eq!(js(&mut p, "document.activeElement.textContent.indexOf('Price') === 0"), "true", "focus stays on the header after it is redrawn");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_tree_expands_selects_filters_and_loads_lazily() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><nile-tree id="tr" style="width:240px;height:200px"></nile-tree>
            <script type="module">
              import "@nile/ui";
              window.tr = document.getElementById("tr");
              tr.items = [{ id: 1, label: "Drinks", children: [{ id: 2, label: "Coffee" }, { id: 3, label: "Tea", children: [{ id: 6, label: "Green" }] }] }, { id: 4, label: "Food", hasChildren: true }, { id: 5, label: "Misc" }];
              tr.load = (item) => new Promise((r) => setTimeout(() => r([{ id: 7, label: "Bread" }]), 0));
              window.log = [];
              tr.addEventListener("selectionchange", (e) => log.push("sel:" + e.detail.id));
              tr.addEventListener("expand", (e) => log.push("exp:" + e.detail.id));
              tr.addEventListener("collapse", (e) => log.push("col:" + e.detail.id));
              tr.addEventListener("activate", (e) => log.push("act:" + e.detail.id));
              window.rows = () => Array.from(tr.querySelectorAll(".nile-tree-row")).map((r) => r.getAttribute("data-id")).join();
            </script></body>"##,
            "ui-tree",
        );
        p.run_timers();
        p.run_timers();
        let none = ModifiersState::default();
        let key = |p: &mut Page, k: &str| p.key("keydown", k.into(), none);
        let settle = |p: &mut Page| {
            std::thread::sleep(Duration::from_millis(30));
            p.run_timers();
            p.run_timers();
        };
        assert_eq!(js(&mut p, "rows()"), "1,4,5");
        assert_eq!(js(&mut p, "tr.getAttribute('role') + ',' + document.querySelector('.nile-tree-row').getAttribute('aria-expanded')"), "tree,false");

        // A click selects; Right expands, then goes to the first child; Down walks; Left goes back to the parent.
        js(&mut p, "document.querySelector('.nile-tree-row').click()");
        js(&mut p, "tr.focus()");
        key(&mut p, "ArrowRight");
        assert_eq!(js(&mut p, "rows()"), "1,2,3,4,5");
        key(&mut p, "ArrowRight");
        key(&mut p, "ArrowDown");
        assert_eq!(js(&mut p, "tr.selected"), "3");
        key(&mut p, "ArrowRight");
        assert_eq!(js(&mut p, "rows()"), "1,2,3,6,4,5");
        key(&mut p, "ArrowRight");
        assert_eq!(js(&mut p, "tr.selected"), "6", "Right on an open branch enters it");
        key(&mut p, "ArrowLeft");
        assert_eq!(js(&mut p, "tr.selected"), "3");
        assert_eq!(js(&mut p, "document.querySelector('[data-id=\"6\"]').getAttribute('aria-level')"), "3");
        // Collapsing an ancestor of the selection moves the selection onto it.
        key(&mut p, "ArrowRight");
        key(&mut p, "ArrowRight");
        js(&mut p, "tr.collapse(1)");
        assert_eq!(js(&mut p, "[rows(), tr.selected].join('|')"), "1,4,5|1");
        key(&mut p, "End");
        assert_eq!(js(&mut p, "tr.selected"), "5");
        key(&mut p, "Enter");
        assert!(js(&mut p, "log.join()").ends_with("act:5"), "{}", js(&mut p, "log.join()"));

        // Lazy children load once, when the branch is first opened.
        js(&mut p, "tr.select(4)");
        key(&mut p, "ArrowRight");
        settle(&mut p);
        assert_eq!(js(&mut p, "rows()"), "1,4,7,5");
        // Type-ahead jumps to a label.
        js(&mut p, "tr.select(1)");
        key(&mut p, "m");
        assert_eq!(js(&mut p, "tr.selected"), "5");

        // A filter keeps matches and their ancestors, opened.
        js(&mut p, "tr.filter = 'gre'");
        assert_eq!(js(&mut p, "rows()"), "1,3,6");
        js(&mut p, "tr.filter = ''");
        assert_eq!(js(&mut p, "rows()"), "1,4,7,5");
        // Data changes: add, update, remove, reveal.
        js(&mut p, "tr.add({ id: 8, label: 'Juice' }, 1)");
        js(&mut p, "tr.reveal(6)");
        assert_eq!(js(&mut p, "rows()"), "1,2,3,6,8,4,7,5");
        js(&mut p, "tr.remove(1)");
        assert_eq!(js(&mut p, "rows()"), "4,7,5");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_keypad_types_into_a_field_and_limits_what_it_takes() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><input id="f"><nile-keypad id="k" target="#f" decimal sign double-zero></nile-keypad>
            <nile-keypad id="n" max-length="3" display enter-label="Go"></nile-keypad>
            <nile-keypad id="pin" pin display max-length="4"></nile-keypad>
            <script type="module">
              import "@nile/ui";
              window.log = [];
              for (const id of ["f", "k", "n", "pin"]) window[id] = document.getElementById(id);
              k.addEventListener("enter", (e) => log.push("enter:" + e.detail.number));
              f.addEventListener("input", () => log.push("in:" + f.value));
              window.press = (pad, ...keys) => keys.forEach((x) => pad.querySelector('[data-key="' + x + '"]').click());
            </script></body>"##,
            "ui-keypad",
        );
        p.run_timers();
        p.run_timers();
        assert_eq!(js(&mut p, "k.querySelectorAll('.nile-keypad-key').length"), "16", "10 digits, point, C, sign, 00, backspace and OK");
        js(&mut p, "press(k, '7', '5')");
        assert_eq!(js(&mut p, "f.value"), "75");
        js(&mut p, "press(k, '.', '5', '5', '5')");
        assert_eq!(js(&mut p, "f.value"), "75.55", "two decimals at most");
        js(&mut p, "press(k, '.')");
        assert_eq!(js(&mut p, "f.value"), "75.55", "one point only");
        js(&mut p, "press(k, 'Backspace', '-')");
        assert_eq!(js(&mut p, "[f.value, k.number].join()"), "-75.5,-75.5");
        js(&mut p, "press(k, 'Clear')");
        assert_eq!(js(&mut p, "f.value"), "");
        js(&mut p, "press(k, '0', '0', '0', '4', '2')");
        assert_eq!(js(&mut p, "f.value"), "42", "no leading zeros");
        js(&mut p, "press(k, 'Enter')");
        assert!(js(&mut p, "log.join()").ends_with("enter:42"), "{}", js(&mut p, "log.join()"));
        // The first digit after Enter starts again; the text the field already held is replaced too.
        js(&mut p, "press(k, '9')");
        assert_eq!(js(&mut p, "f.value"), "9");
        js(&mut p, "f.value = '120'; k.value = ''; press(k, '3')");
        assert_eq!(js(&mut p, "f.value"), "3");

        // Length limit, display, custom Enter label.
        js(&mut p, "press(n, '1', '2', '3', '4')");
        assert_eq!(js(&mut p, "[n.value, n.querySelector('.nile-keypad-display').textContent, n.querySelector('.nile-enter').textContent].join()"), "123,123,Go");
        js(&mut p, "press(pin, '1', '2', '3', '4', '5')");
        assert_eq!(js(&mut p, "[pin.value, pin.querySelector('.nile-keypad-display').textContent].join()"), "1234,●●●●");
        // Quick amounts add up.
        js(&mut p, "const q = document.createElement('nile-keypad'); q.setAttribute('keys', '5,10'); q.setAttribute('display', ''); document.body.appendChild(q); window.q = q");
        p.run_timers();
        js(&mut p, "press(q, '+5', '+10')");
        assert_eq!(js(&mut p, "q.value"), "15");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_date_widgets_pick_parse_and_apply_presets() {
        let (mut p, path) = page(
            r##"<body style="margin:0">
              <nile-calendar id="c" value="2026-10-24" min="2026-10-05" max="2026-10-28"></nile-calendar>
              <nile-calendar id="r" range month="2026-10"></nile-calendar>
              <nile-date id="d" value="2026-10-24" format="dd/MM/yyyy"></nile-date>
              <nile-date-range id="dr" start="2026-10-01" end="2026-10-31"></nile-date-range>
            <script type="module">
              import { formatDate, parseDate } from "@nile/ui";
              window.formatDate = formatDate; window.parseDate = parseDate;
              for (const id of ["c", "r", "d", "dr"]) window[id] = document.getElementById(id);
              window.log = [];
              c.addEventListener("change", (e) => log.push("c:" + e.detail.value));
              c.addEventListener("monthchange", (e) => log.push("m:" + e.detail.month));
              r.addEventListener("change", (e) => log.push("r:" + e.detail.start + ">" + e.detail.end));
              d.addEventListener("change", (e) => log.push("d:" + e.detail.value));
              dr.addEventListener("change", (e) => log.push("dr:" + e.detail.start + ">" + e.detail.end));
              window.day = (cal, iso) => cal.querySelector('.nile-cal-day[data-date="' + iso + '"]');
            </script></body>"##,
            "ui-date",
        );
        p.run_timers();
        p.run_timers();
        let none = ModifiersState::default();
        // Text formats both ways, and rubbish is refused.
        assert_eq!(js(&mut p, "formatDate('2026-10-24', 'dd/MM/yyyy') + '|' + formatDate('2026-03-04', 'MMM d, yyyy') + '|' + formatDate('2026-03-04', 'MMMM yy')"), "24/10/2026|Mar 4, 2026|March 26");
        assert_eq!(js(&mut p, "[parseDate('24/10/2026', 'dd/MM/yyyy'), parseDate('Oct 4, 2026', 'MMM d, yyyy'), parseDate('31/02/2026', 'dd/MM/yyyy'), parseDate('x', 'dd/MM/yyyy')].join()"), "2026-10-24,2026-10-04,,");

        // A month of days: title, 42 cells, Monday first, today's marks, min/max greyed out, the value selected.
        assert_eq!(js(&mut p, "c.querySelector('.nile-cal-title').textContent + '|' + c.querySelectorAll('.nile-cal-day').length + '|' + c.querySelector('.nile-cal-dow').textContent"), "October 2026|42|Mo");
        assert_eq!(js(&mut p, "[day(c, '2026-10-24').getAttribute('aria-selected'), day(c, '2026-10-03').disabled, day(c, '2026-10-29').disabled, day(c, '2026-10-10').disabled].join()"), "true,true,true,false");
        js(&mut p, "day(c, '2026-10-20').click()");
        assert_eq!(js(&mut p, "[c.value, log.join()].join('|')"), "2026-10-20|c:2026-10-20");
        js(&mut p, "day(c, '2026-10-03').click()");
        assert_eq!(js(&mut p, "c.value"), "2026-10-20", "a disabled day can't be picked");
        js(&mut p, "c.querySelector('.nile-cal-nav[data-dir=\"1\"]').click()");
        assert_eq!(js(&mut p, "c.querySelector('.nile-cal-title').textContent + '|' + log[log.length - 1]"), "November 2026|m:2026-11");
        // Keyboard: arrows move the focused day, Enter picks, PageUp goes back a month.
        js(&mut p, "c.value = '2026-10-20'");
        js(&mut p, "c.focus()");
        p.key("keydown", "ArrowRight".into(), none);
        p.key("keydown", "ArrowDown".into(), none);
        p.key("keydown", "Enter".into(), none);
        assert_eq!(js(&mut p, "c.value"), "2026-10-28");

        // A span: two clicks, the in-between days highlighted, the later click before the first swaps them.
        js(&mut p, "day(r, '2026-10-12').click(); day(r, '2026-10-05').click()");
        assert_eq!(js(&mut p, "[r.start, r.end, r.querySelectorAll('.nile-in-range').length, log[log.length - 1]].join()"), "2026-10-05,2026-10-12,6,r:2026-10-05>2026-10-12");

        // The date field: typed text in its format commits on Enter; a bad one is refused and flagged.
        assert_eq!(js(&mut p, "d.querySelector('input').value"), "24/10/2026");
        js(&mut p, "d.querySelector('input').focus(); d.querySelector('input').value = '25/12/2026'");
        p.key("keydown", "Enter".into(), none);
        assert_eq!(js(&mut p, "[d.value, d.querySelector('input').value, log[log.length - 1]].join()"), "2026-12-25,25/12/2026,d:2026-12-25");
        js(&mut p, "d.querySelector('input').value = 'soon'");
        p.key("keydown", "Enter".into(), none);
        assert_eq!(js(&mut p, "[d.value, d.querySelector('input').value, d.hasAttribute('invalid')].join()"), "2026-12-25,25/12/2026,true");
        js(&mut p, "d.querySelector('.nile-date-btn').click()");
        assert_eq!(js(&mut p, "d.querySelector('.nile-date-pop').classList.contains('nile-open') + ',' + d.querySelector('.nile-cal-title').textContent"), "true,December 2026");
        js(&mut p, "day(d, '2026-12-09').click()");
        assert_eq!(js(&mut p, "[d.value, d.querySelector('input').value, d.querySelector('.nile-date-pop').classList.contains('nile-open')].join()"), "2026-12-09,09/12/2026,false");

        // The range field: shows both ends, a preset fills it.
        assert_eq!(js(&mut p, "dr.querySelector('.nile-date-text').textContent"), "2026-10-01 – 2026-10-31");
        js(&mut p, "dr.querySelector('.nile-date-box').click()");
        assert_eq!(js(&mut p, "dr.querySelectorAll('.nile-date-preset').length > 4"), "true");
        js(&mut p, "dr.querySelectorAll('.nile-date-preset')[2].click()");
        assert_eq!(js(&mut p, "Math.round((new Date(dr.end) - new Date(dr.start)) / 864e5)"), "6", "Last 7 days");
        assert!(js(&mut p, "log[log.length - 1]").starts_with("dr:"));
        js(&mut p, "dr.setRange('2026-11-10', '2026-11-02')");
        assert_eq!(js(&mut p, "dr.start + '>' + dr.end"), "2026-11-02>2026-11-10");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_menu_and_palette_choose_by_keyboard_and_pointer() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><div id="zone" style="width:200px;height:100px"></div>
            <script type="module">
              import { menu, contextMenu, palette, shortcuts, fuzzy } from "@nile/ui";
              window.menu = menu; window.palette = palette; window.fuzzy = fuzzy; window.shortcuts = shortcuts;
              window.log = [];
              window.items = () => [{ label: "Open", id: "open", shortcut: "Mod+O", run: () => log.push("run-open") }, { label: "Recent", id: "recent", items: [{ label: "a.txt", id: "a" }, { label: "b.txt", id: "b" }] }, { separator: true }, { label: "Wrap", id: "wrap", checked: true }, { label: "Delete", id: "del", disabled: true }];
              contextMenu(document.getElementById("zone"), () => items());
              palette.add({ id: "new-sale", label: "New sale", shortcut: "F8", run: () => log.push("sale") });
              palette.add({ id: "stock", label: "Open stock register", hint: "Stock", run: () => log.push("stock") });
              shortcuts.on("Mod+S", () => log.push("save"), { label: "Save document" });
              palette.bind("Mod+K");
              window.rows = () => Array.from(document.querySelectorAll(".nile-menu-row")).map((r) => r.querySelector(".nile-menu-label").textContent).join();
            </script></body>"##,
            "ui-menu",
        );
        p.run_timers();
        p.run_timers();
        let none = ModifiersState::default();
        let ctrl = if cfg!(target_os = "macos") { ModifiersState::SUPER } else { ModifiersState::CONTROL };
        let key = |p: &mut Page, k: &str, m: ModifiersState| p.key("keydown", k.into(), m);

        // A menu from data: rows, a check mark, a disabled row, a submenu; Down/Down/Right/Enter chooses a.txt.
        js(&mut p, "menu(items(), { x: 20, y: 20 }).then((c) => log.push('chose:' + (c && c.id)))");
        assert_eq!(js(&mut p, "rows()"), "Open,Recent,Wrap,Delete");
        assert_eq!(js(&mut p, "[document.querySelector('[aria-checked]').getAttribute('aria-checked'), document.querySelector('[aria-disabled]').textContent.indexOf('Delete') >= 0].join()"), "true,true");
        key(&mut p, "ArrowDown", none);
        key(&mut p, "ArrowDown", none);
        key(&mut p, "ArrowRight", none);
        assert_eq!(js(&mut p, "document.querySelectorAll('.nile-menu-popup').length"), "2");
        key(&mut p, "ArrowDown", none);
        key(&mut p, "Enter", none);
        p.run_timers();
        assert_eq!(js(&mut p, "[log.join(), document.querySelectorAll('.nile-menu-popup').length].join('|')"), "chose:b|0");
        // Disabled rows don't choose; a click runs the item's own handler; Escape dismisses with null.
        js(&mut p, "log.length = 0; menu(items(), { x: 20, y: 20 }).then((c) => log.push('chose:' + (c && c.id)))");
        js(&mut p, "Array.from(document.querySelectorAll('.nile-menu-row')).find((r) => r.textContent.indexOf('Delete') >= 0).click()");
        assert_eq!(js(&mut p, "document.querySelectorAll('.nile-menu-popup').length"), "1");
        js(&mut p, "document.querySelector('.nile-menu-row').click()");
        p.run_timers();
        assert_eq!(js(&mut p, "log.join()"), "run-open,chose:open");
        js(&mut p, "log.length = 0; menu(items(), { x: 5, y: 5 }).then((c) => log.push('chose:' + (c && c.id)))");
        key(&mut p, "Escape", none);
        p.run_timers();
        assert_eq!(js(&mut p, "log.join()"), "chose:null");
        // The right button over an element opens it.
        js(&mut p, "document.getElementById('zone').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 30, clientY: 30 }))");
        assert_eq!(js(&mut p, "rows()"), "Open,Recent,Wrap,Delete");
        key(&mut p, "Escape", none);

        // The palette: Mod+K opens it with commands and labelled shortcuts; fuzzy typing filters; Enter runs.
        key(&mut p, "k", ctrl);
        assert_eq!(js(&mut p, "palette.isOpen + ',' + document.querySelectorAll('.nile-palette-row').length"), "true,3");
        assert_eq!(js(&mut p, "document.querySelector('.nile-palette-key') ? 'keys' : 'none'"), "keys");
        js(&mut p, "const i = document.querySelector('.nile-palette-input'); i.value = 'osr'; i.dispatchEvent(new Event('input', { bubbles: true }))");
        assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-palette-row')).map((r) => r.querySelector('.nile-palette-label').textContent).join()"), "Open stock register");
        assert_eq!(js(&mut p, "document.querySelectorAll('.nile-palette-match').length"), "3");
        key(&mut p, "Enter", none);
        assert_eq!(js(&mut p, "[log.join(), palette.isOpen].join('|')"), "chose:null,stock|false");
        // Shortcuts stay quiet while it is open, and Mod+K closes it again.
        key(&mut p, "k", ctrl);
        key(&mut p, "s", ctrl);
        assert!(!js(&mut p, "log.join()").contains("save"), "{}", js(&mut p, "log.join()"));
        key(&mut p, "k", ctrl);
        assert_eq!(js(&mut p, "palette.isOpen"), "false");
        // A picker over your own items resolves with the chosen one.
        js(&mut p, "palette.open({ items: [{ label: 'Ada Lovelace', data: 1 }, { label: 'Alan Turing', data: 2 }], placeholder: 'Find' }).then((c) => log.push('picked:' + (c && c.data)))");
        js(&mut p, "const j = document.querySelector('.nile-palette-input'); j.value = 'tur'; j.dispatchEvent(new Event('input', { bubbles: true }))");
        key(&mut p, "Enter", none);
        p.run_timers();
        assert!(js(&mut p, "log.join()").ends_with("picked:2"), "{}", js(&mut p, "log.join()"));
        // The matcher itself: a run beats scattered letters; non-matches are null.
        assert_eq!(js(&mut p, "[fuzzy('abc', 'xabcx').score > fuzzy('abc', 'a-b-c').score, fuzzy('zz', 'abc') === null].join()"), "true,true");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_wizard_validates_steps_and_finishes() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><nile-wizard id="w" linear cancel-label="Cancel" style="width:500px;height:300px">
              <nile-step label="Company" description="Name"><input id="name"></nile-step>
              <nile-step label="Taxes" optional><p>Taxes</p></nile-step>
              <nile-step label="Hidden" disabled><p>nope</p></nile-step>
              <nile-step label="Done"><p>All set</p></nile-step>
            </nile-wizard>
            <script type="module">
              import "@nile/ui";
              window.w = document.getElementById("w");
              window.log = [];
              w.addEventListener("stepchange", (e) => log.push(e.detail.from + ">" + e.detail.to));
              w.addEventListener("finish", () => log.push("finish"));
              w.addEventListener("cancel", () => log.push("cancel"));
              window.ready = false;
              w.steps[0].validate = () => (document.getElementById("name").value ? true : "Enter a company name");
              w.steps[1].validate = () => new Promise((r) => setTimeout(() => r(true), 0));
              window.btn = (cls) => w.querySelector(".nile-wizard-" + cls);
            </script></body>"##,
            "ui-wizard",
        );
        p.run_timers();
        p.run_timers();
        let settle = |p: &mut Page| {
            for _ in 0..3 {
                std::thread::sleep(Duration::from_millis(20));
                p.run_timers();
            }
        };
        assert_eq!(js(&mut p, "[w.selectedIndex, w.querySelectorAll('.nile-wizard-dot').length, btn('back').disabled, btn('next').textContent].join()"), "0,4,true,Next");
        assert_eq!(js(&mut p, "w.querySelectorAll('.nile-wizard-dot')[1].querySelector('.nile-wizard-sub').textContent"), "Optional");
        // Validation keeps you on the step and shows its text; a linear header won't jump ahead.
        js(&mut p, "btn('next').click()");
        settle(&mut p);
        assert_eq!(js(&mut p, "[w.selectedIndex, w.querySelector('.nile-wizard-error').textContent].join()"), "0,Enter a company name");
        js(&mut p, "w.querySelectorAll('.nile-wizard-dot')[3].click()");
        settle(&mut p);
        assert_eq!(js(&mut p, "w.selectedIndex"), "0");
        js(&mut p, "document.getElementById('name').value = 'SoftSale'; btn('next').click()");
        settle(&mut p);
        assert_eq!(js(&mut p, "[w.selectedIndex, w.querySelector('.nile-wizard-error').textContent, log.join()].join('|')"), "1||0>1");
        // Disabled steps are skipped, in both directions; the last step offers Finish.
        js(&mut p, "btn('next').click()");
        settle(&mut p);
        assert_eq!(js(&mut p, "[w.selectedIndex, btn('next').textContent, w.querySelectorAll('.nile-wizard-dot.nile-done').length].join()"), "3,Finish,2");
        js(&mut p, "btn('back').click()");
        settle(&mut p);
        assert_eq!(js(&mut p, "w.selectedIndex"), "1");
        // Visited steps can be clicked in the header; Finish and Cancel raise their events.
        js(&mut p, "w.querySelectorAll('.nile-wizard-dot')[3].click()");
        settle(&mut p);
        js(&mut p, "btn('next').click()");
        settle(&mut p);
        js(&mut p, "btn('cancel').click()");
        assert_eq!(js(&mut p, "log.slice(-2).join()"), "finish,cancel");
        js(&mut p, "w.reset()");
        assert_eq!(js(&mut p, "[w.selectedIndex, btn('back').disabled].join()"), "0,true");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn removing_the_focused_element_sends_keys_to_the_document_again() {
        let (mut p, path) = page(
            r##"<body><div id="host"><button id="b">x</button></div>
            <script>window.keys = []; document.addEventListener("keydown", (e) => keys.push(e.key));</script></body>"##,
            "focus-removed",
        );
        js(&mut p, "document.getElementById('b').focus()");
        assert_eq!(js(&mut p, "document.activeElement.id"), "b");
        js(&mut p, "document.getElementById('host').innerHTML = ''");
        p.sync();
        assert_eq!(js(&mut p, "document.activeElement === document.body || document.activeElement === null"), "true");
        p.key("keydown", "q".into(), ModifiersState::default());
        assert_eq!(js(&mut p, "keys.join()"), "q", "the key reaches the document");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_escpos_receipt_builds_the_right_bytes() {
        let (mut p, path) = page(
            r##"<body><script type="module">
              import { Receipt, encodeText, printReceipt } from "@nile/escpos";
              window.Receipt = Receipt; window.encodeText = encodeText; window.printReceipt = printReceipt;
              window.hex = (r) => r.hex();
            </script></body>"##,
            "escpos",
        );
        p.run_timers();
        p.run_timers();
        // Setup: initialise + code page; text and styles.
        assert_eq!(js(&mut p, "new Receipt().hex()"), "1b 40 1b 74 00");
        assert_eq!(js(&mut p, "new Receipt({ init: false }).align('center').bold().line('Hi').hex()"), "1b 61 01 1b 45 01 48 69 0a");
        assert_eq!(js(&mut p, "new Receipt({ init: false }).size(2, 2).size(1, 1).underline().invert().font('B').hex()"), "1d 21 11 1d 21 00 1b 2d 01 1d 42 01 1b 4d 01");
        // Layout helpers pad to the width; double-width halves it.
        assert_eq!(js(&mut p, "new Receipt({ init: false, width: 10 }).row('A', '1.00').preview()"), "A     1.00");
        assert_eq!(js(&mut p, "new Receipt({ init: false, width: 10 }).size(2, 1).row('A', '1').preview()"), "A   1");
        assert_eq!(js(&mut p, "new Receipt({ init: false, width: 12 }).columns([{ text: 'Milk', width: 6 }, { text: 'x2', width: 2, align: 'right' }, { text: '5.00', align: 'right' }]).preview()"), "Milk  x25.00");
        assert_eq!(js(&mut p, "new Receipt({ init: false, width: 4 }).rule('=').preview()"), "====");
        assert_eq!(js(&mut p, "new Receipt({ init: false, width: 12 }).align('center').line('ABCD').preview()"), "    ABCD");
        // Code pages: é, ñ, the euro sign (only in 858/1252), and "?" for what a page lacks.
        assert_eq!(js(&mut p, "[encodeText('é', 'cp437'), encodeText('ñ', 'cp437'), encodeText('€', 'cp858'), encodeText('€', 'cp437'), encodeText('é€', 'cp1252'), encodeText('日', 'cp437'), encodeText('Ab', 'ascii')].map((a) => a.join('.')).join('|')"), "130|164|213|63|233.128|63|65.98");
        assert_eq!(js(&mut p, "new Receipt({ codepage: 'cp858' }).hex()"), "1b 40 1b 74 13");
        // Barcode, QR, image, cut, drawer, beep.
        assert_eq!(js(&mut p, "new Receipt({ init: false }).barcode('code128', '123', { height: 60, width: 2 }).hex()"), "1d 68 3c 1d 77 02 1d 48 02 1d 6b 49 05 7b 42 31 32 33");
        assert_eq!(js(&mut p, "new Receipt({ init: false }).barcode('ean13', '5901234123457', { text: 'none' }).hex()"), "1d 68 50 1d 77 03 1d 48 00 1d 6b 43 0d 35 39 30 31 32 33 34 31 32 33 34 35 37");
        assert_eq!(js(&mut p, "new Receipt({ init: false }).qr('AB', { size: 5 }).hex()"), "1d 28 6b 04 00 31 41 32 00 1d 28 6b 03 00 31 43 05 1d 28 6b 03 00 31 45 31 1d 28 6b 05 00 31 50 30 41 42 1d 28 6b 03 00 31 51 30");
        assert_eq!(js(&mut p, "new Receipt({ init: false }).image({ width: 8, height: 2, data: new Uint8ClampedArray(8 * 2 * 4).fill(0).map((v, i) => (i % 4 === 3 ? 255 : 0)) }).hex()"), "1d 76 30 00 01 00 02 00 ff ff");
        assert_eq!(js(&mut p, "new Receipt({ init: false }).image({ width: 3, height: 1, data: Uint8ClampedArray.from([0,0,0,255, 255,255,255,255, 0,0,0,255]) }).hex()"), "1d 76 30 00 01 00 01 00 a0");
        assert_eq!(js(&mut p, "new Receipt({ init: false }).feed(2).cut().cut({ partial: true }).drawer().drawer(1).beep(2).hex()"), "1b 64 02 1d 56 41 03 1d 56 42 03 1b 70 00 19 fa 1b 70 01 19 fa 1b 42 02 02");
        assert_eq!(js(&mut p, "(() => { try { new Receipt().barcode('nope', '1'); return 'no error'; } catch (e) { return 'threw'; } })()"), "threw");
        assert_eq!(js(&mut p, "new Receipt({ init: false }).text('a\\nb').preview()"), "a\nb");
        // A file target writes the bytes.
        let out = std::env::temp_dir().join(format!("nile-escpos-{}.bin", std::process::id()));
        js(&mut p, &format!("window.done = null; printReceipt(new Receipt().line('OK').cut(), {{ file: {:?} }}).then(() => (done = 'ok'), (e) => (done = 'err ' + e.message))", out.to_string_lossy()));
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(20));
            p.run_timers();
        }
        assert_eq!(js(&mut p, "done"), "ok");
        assert_eq!(std::fs::read(&out).unwrap(), [0x1b, 0x40, 0x1b, 0x74, 0, b'O', b'K', 0x0a, 0x1d, 0x56, 0x41, 3]);
        let _ = std::fs::remove_file(out);
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(unix)]
    #[test]
    fn the_print_module_lists_printers_and_sends_jobs_through_cups_commands() {
        use std::os::unix::fs::PermissionsExt;
        // These tests change the process's PATH: one at a time.
        let _path = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("nile-print-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = |name: &str, body: &str| {
            let f = dir.join(name);
            std::fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        script(
            "lpstat",
            "echo 'printer Office is idle.  enabled since Thu 01 Jan 2026'\necho 'printer Kitchen disabled since Thu 01 Jan 2026 -'\necho '\treason unknown'\necho 'printer TM-T20 now printing TM-T20-5.  enabled since Thu 01 Jan 2026'\necho 'system default destination: Office'",
        );
        let log = dir.join("calls.txt");
        script("lp", &format!("echo \"$@\" >> '{}'\ncat > '{}.stdin' 2>/dev/null\necho 'request id is X-1 (1 file(s))'", log.display(), log.display()));
        script("lpfail", "echo boom >&2\nexit 1");
        let (mut p, path) = page(
            &format!(
                r##"<body><script type="module">
              import {{ printers, printFile, printRaw, sendNetwork, parseLpstat }} from "@nile/print";
              import {{ setenv, getenv }} from "@sys";
              window.parseLpstat = parseLpstat;
              setenv("PATH", {dir:?} + ":" + getenv("PATH"));
              window.out = [];
              window.step = async () => {{
                out.push(JSON.stringify(await printers()));
                await printFile("/tmp/invoice.pdf", {{ printer: "Office", copies: 2, title: "Invoice 7" }});
                await printFile("/tmp/other.pdf");
                await printRaw("TM-T20", new Uint8Array([27, 64, 72, 105, 10]));
                try {{ await printRaw("", [1]); }} catch (e) {{ out.push("empty: " + e.message); }}
                out.push("done");
              }};
              step().catch((e) => out.push("ERR " + e.message));
            </script></body>"##,
                dir = dir.to_string_lossy()
            ),
            "print",
        );
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(30));
            p.run_timers();
            if js(&mut p, "out.includes('done')") == "true" {
                break;
            }
        }
        let out = js(&mut p, "out.join('\\n')");
        assert!(out.contains("done"), "{out}");
        assert!(out.contains(r#"[{"name":"Office","default":true,"status":"idle"},{"name":"Kitchen","default":false,"status":"offline"},{"name":"TM-T20","default":false,"status":"printing"}]"#), "{out}");
        assert!(out.contains("empty: printRaw needs a printer name"), "{out}");
        let calls = std::fs::read_to_string(&log).unwrap();
        assert_eq!(calls.lines().collect::<Vec<_>>(), ["-d Office -n 2 -t Invoice 7 /tmp/invoice.pdf", "/tmp/other.pdf", "-d TM-T20 -o raw -t Nile receipt"]);
        assert_eq!(std::fs::read(format!("{}.stdin", log.display())).unwrap(), [27, 64, 72, 105, 10], "the raw bytes reach lp's standard input");

        // The parser alone, and a failing command surfaces its message.
        assert_eq!(js(&mut p, "JSON.stringify(parseLpstat('system default destination: B\\nprinter A is idle.\\nprinter B is idle.'))"), r#"[{"name":"A","default":false,"status":"idle"},{"name":"B","default":true,"status":"idle"}]"#);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_print_module_sends_raw_bytes_to_a_network_printer() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::Read;
            let (mut s, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let _ = s.read_to_end(&mut got);
            let _ = tx.send(got);
        });
        let (mut p, path) = page(
            &format!(
                r##"<body><script type="module">
              import {{ Receipt, printReceipt }} from "@nile/escpos";
              window.done = null;
              printReceipt(new Receipt().line("Net").cut(), {{ host: "127.0.0.1", port: {port} }}).then(() => (done = "sent"), (e) => (done = "err " + e.message));
            </script></body>"##
            ),
            "print-net",
        );
        for _ in 0..80 {
            std::thread::sleep(Duration::from_millis(25));
            p.run_timers();
            if js(&mut p, "done") != "null" {
                break;
            }
        }
        assert_eq!(js(&mut p, "done"), "sent");
        let got = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(got, [0x1b, 0x40, 0x1b, 0x74, 0, b'N', b'e', b't', 0x0a, 0x1d, 0x56, 0x41, 3]);
        // Nothing listening: the promise rejects instead of hanging.
        let (mut p2, path2) = page(
            r##"<body><script type="module">
              import { sendNetwork } from "@nile/print";
              window.done = null;
              sendNetwork("127.0.0.1", 1, [1], { timeout: 1500 }).then(() => (done = "sent"), (e) => (done = "err"));
            </script></body>"##,
            "print-net-down",
        );
        for _ in 0..120 {
            std::thread::sleep(Duration::from_millis(25));
            p2.run_timers();
            if js(&mut p2, "done") != "null" {
                break;
            }
        }
        assert_eq!(js(&mut p2, "done"), "err");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path2);
    }

    #[cfg(unix)]
    #[test]
    fn window_print_writes_the_page_as_a_pdf_and_hands_it_to_the_printer() {
        use std::os::unix::fs::PermissionsExt;
        let _path = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("nile-winprint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("calls.txt");
        let lp = dir.join("lp");
        std::fs::write(&lp, format!("#!/bin/sh\necho \"$@\" >> '{}'\ncp \"$(eval echo \\${{$#}})\" '{}.pdf'\n", log.display(), log.display())).unwrap();
        std::fs::set_permissions(&lp, std::fs::Permissions::from_mode(0o755)).unwrap();
        let saved = dir.join("saved.pdf");
        let (mut p, path) = page(
            &format!(
                r##"<html><head><title>Invoice 7</title><style>@page {{ size: 200px 300px; margin: 10px }}</style></head><body><h1>Invoice</h1><p>Total 8.80</p>
            <script type="module">
              import {{ setenv, getenv }} from "@sys";
              setenv("PATH", {dir:?} + ":" + getenv("PATH"));
              window.out = [];
              window.w0 = window.innerWidth;
              (async () => {{
                out.push("export:" + (await Window.this.exportPdf({saved:?})));
                out.push("print:" + (await Window.this.print({{ printer: "Office", copies: 2 }})));
                try {{ await Window.this.exportPdf("/nonexistent-dir/x.pdf"); }} catch (e) {{ out.push("failed"); }}
              }})().catch((e) => out.push("ERR " + e.message));
            </script></body></html>"##,
                dir = dir.to_string_lossy(),
                saved = saved.to_string_lossy()
            ),
            "window-print",
        );
        for _ in 0..80 {
            std::thread::sleep(Duration::from_millis(25));
            p.run_timers();
            if js(&mut p, "out.includes('failed')") == "true" {
                break;
            }
        }
        let out = js(&mut p, "out.join('\\n')");
        assert!(out.contains("print:true") && out.contains("failed"), "{out}");
        let head = std::fs::read(&saved).unwrap();
        assert!(head.starts_with(b"%PDF"), "a PDF was written");
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(calls.starts_with("-d Office -n 2 "), "{calls}");
        assert!(std::fs::read(format!("{}.pdf", log.display())).unwrap().starts_with(b"%PDF"), "lp was given the PDF");
        assert_eq!(js(&mut p, "window.innerWidth === w0"), "true", "the window's own size is untouched");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn web_crypto_hashes_signs_derives_and_encrypts_with_the_published_vectors() {
        let (mut p, path) = page(
            r##"<body><script type="module">
              import { hashPassword, verifyPassword, needsRehash, sha256, hmac, encryptText, decryptText } from "@nile/crypto";
              const enc = (t) => new TextEncoder().encode(t);
              const hx = (b) => Array.from(new Uint8Array(b), (x) => (x < 16 ? "0" : "") + x.toString(16)).join("");
              const s = crypto.subtle;
              window.out = [];
              const note = (k, v) => out.push(k + "=" + v);
              (async () => {
                note("sha256", hx(await s.digest("SHA-256", enc("abc"))));
                note("sha1", hx(await s.digest({ name: "SHA-1" }, enc("abc"))));
                note("helper", await sha256("abc"));
                const hk = await s.importKey("raw", enc("key"), { name: "HMAC", hash: "SHA-256" }, false, ["sign", "verify"]);
                const tag = await s.sign("HMAC", hk, enc("The quick brown fox jumps over the lazy dog"));
                note("hmac", hx(tag));
                note("verify", [await s.verify("HMAC", hk, tag, enc("The quick brown fox jumps over the lazy dog")), await s.verify("HMAC", hk, tag, enc("other"))].join());
                note("hmac2", await hmac("key", "The quick brown fox jumps over the lazy dog"));
                const pk = await s.importKey("raw", enc("password"), "PBKDF2", false, ["deriveBits", "deriveKey"]);
                note("pbkdf2", hx(await s.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt: enc("salt"), iterations: 4096 }, pk, 256)));
                const zero = await s.importKey("raw", new Uint8Array(16), "AES-GCM", true, ["encrypt", "decrypt"]);
                note("gcm", hx(await s.encrypt({ name: "AES-GCM", iv: new Uint8Array(12) }, zero, new Uint8Array(16))));
                const key = await s.generateKey({ name: "AES-GCM", length: 256 }, true, ["encrypt", "decrypt"]);
                const iv = crypto.getRandomValues(new Uint8Array(12));
                const sealed = await s.encrypt({ name: "AES-GCM", iv, additionalData: enc("ctx") }, key, enc("hello"));
                note("roundtrip", new TextDecoder().decode(await s.decrypt({ name: "AES-GCM", iv, additionalData: enc("ctx") }, key, sealed)));
                try { await s.decrypt({ name: "AES-GCM", iv, additionalData: enc("other") }, key, sealed); note("tamper", "accepted"); } catch (e) { note("tamper", e.name); }
                try { await s.digest("MD5", enc("x")); } catch (e) { note("md5", e.name); }
                note("exported", (await s.exportKey("raw", key)).byteLength);
                const derived = await s.deriveKey({ name: "PBKDF2", hash: "SHA-256", salt: enc("s"), iterations: 10 }, pk, { name: "AES-GCM", length: 128 }, false, ["encrypt"]);
                note("derived", derived.algorithm.name + derived.algorithm.length + derived.extractable);
                // helpers
                const stored = await hashPassword("correct horse", { iterations: 2000 });
                note("stored", stored.split("$").length + ":" + stored.split("$").slice(0, 2).join("$"));
                note("verify-pw", [await verifyPassword("correct horse", stored), await verifyPassword("wrong", stored), await verifyPassword("x", "garbage")].join());
                note("rehash", [needsRehash(stored), needsRehash(stored, { iterations: 1000 })].join());
                const box = await encryptText("pw", "secret note", { iterations: 1000 });
                note("text", await decryptText("pw", box));
                try { await decryptText("nope", box); } catch (e) { note("badpw", e.name); }
                out.push("done");
              })().catch((e) => out.push("ERR " + e.name + " " + e.message));
              // sync parts
              const a = crypto.getRandomValues(new Uint8Array(32)), b = crypto.getRandomValues(new Uint8Array(32));
              window.sync = [a.some((x) => x !== 0), a.join() !== b.join(), /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(crypto.randomUUID()), crypto.randomUUID() !== crypto.randomUUID()].join();
              try { crypto.getRandomValues(new Float32Array(2)); window.sync += ",float-ok"; } catch (e) { window.sync += "," + e.name; }
              try { crypto.getRandomValues(new Uint8Array(70000)); } catch (e) { window.sync += "," + e.name; }
            </script></body>"##,
            "web-crypto",
        );
        for _ in 0..200 {
            std::thread::sleep(Duration::from_millis(30));
            p.run_timers();
            if js(&mut p, "out.includes('done') || out.some((l) => l.startsWith('ERR'))") == "true" {
                break;
            }
        }
        let out = js(&mut p, "out.join('\\n')");
        for line in [
            "sha256=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "sha1=a9993e364706816aba3e25717850c26c9cd0d89d",
            "helper=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "hmac=f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8",
            "verify=true,false",
            "hmac2=f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8",
            "pbkdf2=c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a",
            "gcm=0388dace60b6a392f328c2b971b2fe78ab6e47d42cec13bdf53a67b21257bddf",
            "roundtrip=hello",
            "tamper=OperationError",
            "md5=NotSupportedError",
            "exported=32",
            "derived=AES-GCM128false",
            "stored=4:pbkdf2-sha256$2000",
            "verify-pw=true,false,false",
            "rehash=true,false",
            "text=secret note",
            "badpw=OperationError",
        ] {
            assert!(out.lines().any(|l| l == line), "missing {line:?} in:\n{out}");
        }
        assert_eq!(js(&mut p, "sync"), "true,true,true,true,TypeMismatchError,QuotaExceededError");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn intl_formats_numbers_currencies_dates_and_collates_like_icu() {
        let (mut p, path) = page(r##"<body><script>window.d = new Date(2026, 9, 24, 15, 5, 9);</script></body>"##, "intl");
        let check = |p: &mut Page, code: &str, want: &str| assert_eq!(js(p, code), want, "{code}");
        let nb = "\u{a0}";
        let nn = "\u{202f}";
        // numbers
        check(&mut p, "(1234567.891).toLocaleString('en-US')", "1,234,567.891");
        check(&mut p, "(1234567.891).toLocaleString('de-DE')", "1.234.567,891");
        check(&mut p, "(1234567.891).toLocaleString('fr-FR')", &format!("1{nn}234{nn}567,891"));
        check(&mut p, "(1234567.891).toLocaleString('en-IN')", "12,34,567.891");
        check(&mut p, "[(1234.5).toLocaleString('es-ES'), (12345.5).toLocaleString('es-ES')].join('|')", "1234,5|12.345,5");
        check(&mut p, "(1.005).toLocaleString('en-US', { maximumFractionDigits: 2 })", "1.01");
        check(&mut p, "(2.5).toLocaleString('en-US', { maximumFractionDigits: 0 })", "3");
        check(&mut p, "(0.1 + 0.2).toLocaleString('en-US', { maximumFractionDigits: 2 })", "0.3");
        check(&mut p, "(0.256).toLocaleString('en-US', { style: 'percent' })", "26%");
        check(&mut p, "(5).toLocaleString('en-US', { minimumIntegerDigits: 3 })", "005");
        check(&mut p, "(0).toLocaleString('en-US', { minimumFractionDigits: 2 })", "0.00");
        check(&mut p, "(123456).toLocaleString('en-US', { maximumSignificantDigits: 3 })", "123,000");
        check(&mut p, "(1234).toLocaleString('en-US', { notation: 'compact' })", "1.2K");
        check(&mut p, "(1234567).toLocaleString('en-US', { notation: 'compact', compactDisplay: 'long' })", "1.2 million");
        check(&mut p, "(1234).toLocaleString('en-US', { useGrouping: false })", "1234");
        check(&mut p, "[(5).toLocaleString('en-US', { signDisplay: 'always' }), (-0.5).toLocaleString('en-US'), NaN.toLocaleString('en-US')].join('|')", "+5|-0.5|NaN");
        // currencies
        check(&mut p, "(1234.5).toLocaleString('en-US', { style: 'currency', currency: 'USD' })", "$1,234.50");
        check(&mut p, "(-1234.5).toLocaleString('en-US', { style: 'currency', currency: 'USD' })", "-$1,234.50");
        check(&mut p, "(1234.5).toLocaleString('de-DE', { style: 'currency', currency: 'EUR' })", &format!("1.234,50{nb}€"));
        check(&mut p, "(1234.5).toLocaleString('en-UG', { style: 'currency', currency: 'UGX' })", &format!("USh{nb}1,235"));
        check(&mut p, "(1234.5).toLocaleString('en-US', { style: 'currency', currency: 'JPY' })", "¥1,235");
        check(&mut p, "(1234.5).toLocaleString('en-US', { style: 'currency', currency: 'CHF' })", &format!("CHF{nb}1,234.50"));
        check(&mut p, "(1234.5).toLocaleString('en-US', { style: 'currency', currency: 'EUR', currencyDisplay: 'code' })", &format!("EUR{nb}1,234.50"));
        check(&mut p, "new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' }).formatToParts(1.5).map((x) => x.type).join()", "currency,integer,decimal,fraction");
        check(&mut p, "(() => { try { (1).toLocaleString('en-US', { style: 'currency' }); } catch (e) { return e.name; } })()", "TypeError");
        // dates
        check(&mut p, "[d.toLocaleDateString('en-US'), d.toLocaleDateString('en-GB'), d.toLocaleDateString('de-DE'), d.toLocaleDateString('fr-FR')].join('|')", "10/24/2026|24/10/2026|24.10.2026|24/10/2026");
        check(&mut p, "d.toLocaleTimeString('en-US')", &format!("3:05:09{nn}PM"));
        check(&mut p, "d.toLocaleTimeString('de-DE')", "15:05:09");
        check(&mut p, "d.toLocaleString('en-US')", &format!("10/24/2026, 3:05:09{nn}PM"));
        check(&mut p, "d.toLocaleDateString('en-US', { dateStyle: 'full' })", "Saturday, October 24, 2026");
        check(&mut p, "d.toLocaleDateString('de-DE', { dateStyle: 'long' })", "24. Oktober 2026");
        check(&mut p, "d.toLocaleDateString('fr-FR', { dateStyle: 'full' })", "samedi 24 octobre 2026");
        check(&mut p, "d.toLocaleDateString('es-ES', { dateStyle: 'long' })", "24 de octubre de 2026");
        check(&mut p, "d.toLocaleDateString('en-US', { year: 'numeric', month: 'long', day: 'numeric' })", "October 24, 2026");
        check(&mut p, "d.toLocaleDateString('en-GB', { year: 'numeric', month: 'long', day: 'numeric' })", "24 October 2026");
        check(&mut p, "d.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' })", "Sat, Oct 24");
        check(&mut p, "d.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' })", &format!("03:05{nn}PM"));
        check(&mut p, "d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })", "15:05");
        check(&mut p, "d.toLocaleTimeString('en-US', { hour12: false })", "15:05:09");
        check(&mut p, "new Intl.DateTimeFormat('en-US', { timeZone: 'UTC', dateStyle: 'short' }).format(new Date(Date.UTC(2026, 0, 5, 23, 30)))", "1/5/26");
        check(&mut p, "(() => { try { d.toLocaleDateString('xx-invalid-locale-!'); } catch (e) { return e.name; } })()", "RangeError");
        // plural, list, relative, collation
        check(&mut p, "[new Intl.PluralRules('en').select(1), new Intl.PluralRules('en').select(2), new Intl.PluralRules('en', { type: 'ordinal' }).select(22), new Intl.PluralRules('ru').select(3), new Intl.PluralRules('fr').select(0)].join()", "one,other,two,few,one");
        check(&mut p, "[new Intl.ListFormat('en').format(['a', 'b', 'c']), new Intl.ListFormat('en', { type: 'disjunction' }).format(['a', 'b']), new Intl.ListFormat('fr').format(['a', 'b', 'c'])].join('|')", "a, b, and c|a or b|a, b et c");
        check(&mut p, "[new Intl.RelativeTimeFormat('en', { numeric: 'auto' }).format(-1, 'day'), new Intl.RelativeTimeFormat('en').format(3, 'hours'), new Intl.RelativeTimeFormat('en').format(-2, 'day')].join('|')", "yesterday|in 3 hours|2 days ago");
        check(&mut p, "['item 10', 'item 2', 'Item 1', 'élan', 'eclair', 'zebra', 'Zebra'].sort((a, b) => a.localeCompare(b, undefined, { numeric: true })).join()", "eclair,élan,Item 1,item 2,item 10,zebra,Zebra");
        check(&mut p, "['a'.localeCompare('B'), 'a'.localeCompare('A'), 'é'.localeCompare('e', undefined, { sensitivity: 'base' }), 'é'.localeCompare('e', undefined, { sensitivity: 'accent' })].join()", "-1,-1,0,1");
        check(&mut p, "new Intl.NumberFormat('de-DE').resolvedOptions().locale + '|' + Intl.getCanonicalLocales('en_us')", "de-DE|en-US");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_keychain_module_reaches_the_native_store_and_rejects_bad_names() {
        let (mut p, path) = page(
            r##"<body><script type="module">
              import { keychain } from "@nile/keychain";
              window.out = [];
              (async () => {
                try { await keychain.get(""); out.push("accepted"); } catch (e) { out.push("rejected:" + e.message); }
                const ok = await keychain.available();
                out.push("available:" + typeof ok);
                out.push("reason:" + typeof keychain.reason());
                keychain.service = "NileTest";
                out.push("service:" + keychain.service);
                out.push("done");
              })().catch((e) => out.push("ERR " + e.message));
            </script></body>"##,
            "keychain",
        );
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(30));
            p.run_timers();
            if js(&mut p, "out.includes('done') || out.some((l) => l.startsWith('ERR'))") == "true" {
                break;
            }
        }
        let out = js(&mut p, "out.join('\\n')");
        assert!(out.contains("rejected:a service and an account name are required"), "{out}");
        assert!(out.contains("available:boolean") && out.contains("reason:string") && out.contains("service:NileTest") && out.contains("done"), "{out}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn mail_builds_mime_and_talks_smtp_with_a_fake_server() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel::<(Vec<String>, String)>();
        std::thread::spawn(move || {
            // one session per connection: the first is accepted mail, the second rejects the recipient
            for session in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut out = stream.try_clone().unwrap();
                let mut input = BufReader::new(stream);
                let mut commands = Vec::new();
                let mut message = String::new();
                out.write_all(b"220 fake ESMTP ready\r\n").unwrap();
                loop {
                    let mut line = String::new();
                    if input.read_line(&mut line).unwrap() == 0 {
                        break;
                    }
                    let line = line.trim_end().to_string();
                    commands.push(line.clone());
                    let upper = line.to_uppercase();
                    if upper.starts_with("EHLO") {
                        out.write_all(b"250-fake greets you\r\n250-SIZE 1000000\r\n250 AUTH PLAIN LOGIN\r\n").unwrap();
                    } else if upper.starts_with("AUTH PLAIN") {
                        out.write_all(b"235 2.7.0 accepted\r\n").unwrap();
                    } else if upper.starts_with("RCPT") && session == 1 {
                        out.write_all(b"550 5.1.1 no such user\r\n").unwrap();
                    } else if upper.starts_with("MAIL") || upper.starts_with("RCPT") {
                        out.write_all(b"250 ok\r\n").unwrap();
                    } else if upper == "DATA" {
                        out.write_all(b"354 go ahead\r\n").unwrap();
                        loop {
                            let mut l = String::new();
                            input.read_line(&mut l).unwrap();
                            if l == ".\r\n" {
                                break;
                            }
                            message.push_str(&l);
                        }
                        out.write_all(b"250 2.0.0 queued as 42\r\n").unwrap();
                    } else if upper == "QUIT" {
                        out.write_all(b"221 bye\r\n").unwrap();
                        break;
                    } else {
                        out.write_all(b"502 unknown\r\n").unwrap();
                    }
                }
                tx.send((commands, message)).unwrap();
            }
        });
        let (mut p, path) = page(
            &format!(
                r##"<body><script type="module">
              import {{ sendMail, buildMessage, parseAddress }} from "@nile/mail";
              window.out = [];
              window.parseAddress = parseAddress;
              window.buildMessage = buildMessage;
              (async () => {{
                const r = await sendMail({{
                  host: "127.0.0.1", port: {port}, secure: "none", allowInsecureAuth: true, user: "shop@example.com", password: "p@ss",
                  from: {{ name: "Soft Sale", address: "shop@example.com" }}, to: ["ada@example.com", "Alan T <alan@example.com>"], bcc: "audit@example.com",
                  subject: "Reçu n° 7", text: "Hello é\r\n.dotted line\r\nbye", html: "<p>Hello</p>",
                  attachments: [{{ filename: "r.pdf", content: new Uint8Array([37, 80, 68, 70, 0, 255]) }}],
                }});
                out.push("sent:" + r.accepted.join(",") + "|" + r.response);
                try {{
                  await sendMail({{ host: "127.0.0.1", port: {port}, secure: "none", from: "a@b.co", to: "nobody@example.com", subject: "x", text: "y" }});
                  out.push("second: accepted");
                }} catch (e) {{ out.push("second:" + e.responseCode + ":" + e.message); }}
                out.push("done");
              }})().catch((e) => out.push("ERR " + e.message));
            </script></body>"##
            ),
            "mail",
        );
        for _ in 0..200 {
            std::thread::sleep(Duration::from_millis(30));
            p.run_timers();
            if js(&mut p, "out.includes('done') || out.some((l) => l.startsWith('ERR'))") == "true" {
                break;
            }
        }
        let out = js(&mut p, "out.join('\\n')");
        assert!(out.contains("sent:ada@example.com,alan@example.com,audit@example.com|2.0.0 queued as 42"), "{out}");
        assert!(out.contains("second:550:550 5.1.1 no such user"), "{out}");
        let (commands, message) = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(commands[0], "EHLO localhost");
        // AUTH PLAIN "\0shop@example.com\0p@ss"
        assert_eq!(commands[1], "AUTH PLAIN AHNob3BAZXhhbXBsZS5jb20AcEBzcw==");
        assert_eq!(&commands[2..6], ["MAIL FROM:<shop@example.com>", "RCPT TO:<ada@example.com>", "RCPT TO:<alan@example.com>", "RCPT TO:<audit@example.com>"]);
        assert_eq!(commands[6], "DATA");
        assert_eq!(commands[7], "QUIT");
        // headers: encoded subject, to without the bcc, a mixed message holding an alternative
        assert!(message.contains("From: Soft Sale <shop@example.com>\r\n"), "{message}");
        assert!(message.contains("To: ada@example.com, Alan T <alan@example.com>\r\n") && !message.contains("audit@example.com"), "{message}");
        assert!(message.contains("Subject: =?UTF-8?B?UmXDp3UgbsKwIDc=?=\r\n"), "{message}");
        assert!(message.contains("Content-Type: multipart/mixed; boundary=") && message.contains("Content-Type: multipart/alternative; boundary="), "{message}");
        assert!(message.contains("Content-Type: text/plain; charset=utf-8") && message.contains("Content-Type: text/html; charset=utf-8"), "{message}");
        assert!(message.contains("Content-Type: application/pdf; name=\"r.pdf\"") && message.contains("Content-Disposition: attachment; filename=\"r.pdf\""), "{message}");
        assert!(message.contains("JVBERgD/"), "the attachment is base64 (%PDF + 00 ff): {message}");
        // the dot-stuffed line inside the text part survives base64 untouched, and no raw line starts with a lone dot
        assert!(!message.lines().any(|l| l == "."), "{message}");
        let (second, _) = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(second.iter().any(|c| c.starts_with("RCPT TO:<nobody@example.com>")) && !second.contains(&"DATA".to_string()), "{second:?}");
        // the pure helpers: addresses, a refusal to send a password in the clear, header injection, no recipients
        assert_eq!(js(&mut p, "JSON.stringify([parseAddress('Ada <a@b.co>'), parseAddress('x@y.zz')])"), r#"[{"name":"Ada","address":"a@b.co"},{"name":"","address":"x@y.zz"}]"#);
        assert_eq!(js(&mut p, "(() => { try { buildMessage({ from: 'a@b.co', to: 'c@d.ee', subject: 'x\\r\\nBcc: evil@e.vil', text: 'y' }); return 'built'; } catch (e) { return e.message; } })()"), "line breaks are not allowed in a header");
        assert_eq!(js(&mut p, "(() => { try { buildMessage({ from: 'a@b.co', subject: 'x' }); return 'built'; } catch (e) { return e.message; } })()"), "no recipients");
        assert_eq!(js(&mut p, "(() => { try { buildMessage({ from: 'not-an-address', to: 'c@d.ee' }); return 'built'; } catch (e) { return e.message; } })()"), "not an email address: not-an-address");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_grid_keeps_an_edit_with_its_record_and_the_drag_clean() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><div id="host"></div>
            <script type="module">
              import "@nile/ui";
              const host = document.getElementById("host");
              // Properties set right after innerHTML, before the element is upgraded, must reach the setters.
              host.innerHTML = '<nile-grid id="g" key="id" style="width:400px;height:220px"></nile-grid><nile-combobox id="cb"></nile-combobox>';
              window.g = document.getElementById("g");
              g.columns = [{ key: "name", title: "Name", width: 200, editable: true, validate: (v) => (v ? true : "Required") }, { key: "n", title: "N", width: 80, type: "number" }];
              g.rows = Array.from({ length: 100 }, (_, i) => ({ id: i, name: "Row " + i, n: i }));
              window.cb = document.getElementById("cb");
              cb.items = ["Red", "Green"];
              window.log = [];
              g.addEventListener("cellchange", (e) => log.push(e.detail.row.id + ":" + e.detail.value));
              g.addEventListener("columnresize", (e) => log.push("resize:" + e.detail.width));
              window.shown = () => Array.from(document.querySelectorAll(".nile-grid-row")).filter((x) => x.style.display !== "none").sort((a, b) => Number(a.getAttribute("aria-rowindex")) - Number(b.getAttribute("aria-rowindex")));
            </script></body>"##,
            "ui-grid-record",
        );
        p.run_timers();
        p.run_timers();
        let settle = |p: &mut Page| {
            std::thread::sleep(Duration::from_millis(30));
            p.run_timers();
            p.run_timers();
        };
        let none = ModifiersState::default();
        assert_eq!(js(&mut p, "[g.rows.length, shown().length > 0, cb.querySelectorAll('.nile-combo-list [role=option]').length >= 0].join()"), "100,true,true", "pre-upgrade properties arrive");

        // A sort while an edit is open: the value still lands in the record being edited.
        js(&mut p, "g.edit(0, 'name')");
        js(&mut p, "document.activeElement.value = 'Edited'");
        js(&mut p, "g.sortBy('name', 'desc')");
        settle(&mut p);
        assert_eq!(js(&mut p, "g.editing ? g.editing.key + ':' + (g.editing.index > 0) + ':' + shown().some((r) => r.querySelector('.nile-grid-editor')) : 'none'"), "name:true:true", "the editor follows its record");
        p.key("keydown", "Enter".into(), none);
        settle(&mut p);
        assert_eq!(js(&mut p, "[g.rows[0].name, g.rows[99].name, log.join()].join()"), "Edited,Row 99,0:Edited");

        // A refused value survives a refresh: the editor stays in its row with focus.
        js(&mut p, "g.sortBy(null)");
        js(&mut p, "g.edit(3, 'name')");
        js(&mut p, "document.activeElement.value = ''");
        assert_eq!(js(&mut p, "g.commitEdit()"), "false");
        js(&mut p, "g.refresh(); g.add({ id: 500, name: 'new', n: 0 }, 0)");
        settle(&mut p);
        assert_eq!(js(&mut p, "[g.editing && g.editing.index, !!document.querySelector('.nile-grid-editor.nile-invalid'), document.activeElement.className].join()"), "4,true,nile-grid-editor nile-invalid");
        js(&mut p, "g.cancelEdit()");
        assert_eq!(js(&mut p, "g.editing"), "null");
        // Scrolling the invalid cell out of the window keeps it too.
        js(&mut p, "g.edit(3, 'name')");
        js(&mut p, "document.activeElement.value = ''");
        js(&mut p, "g.querySelector('.nile-grid-body').scrollTop = 2000");
        settle(&mut p);
        assert_eq!(js(&mut p, "[!!g.editing, !!document.querySelector('.nile-grid-editor')].join()"), "true,true");
        js(&mut p, "g.cancelEdit()");

        // A drag ends once when the window loses the pointer; later moves change nothing.
        js(&mut p, "(() => { const h = g.querySelector('.nile-grid-resizer'); h.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, clientX: 200 })); document.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 250 })); })()");
        js(&mut p, "window.dispatchEvent(new Event('blur'))");
        settle(&mut p);
        js(&mut p, "document.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 300 })); document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))");
        settle(&mut p);
        assert_eq!(js(&mut p, "log.filter((x) => x.startsWith('resize')).length"), "1");
        // Removing the grid mid-drag ends it as well.
        js(&mut p, "(() => { const h = g.querySelector('.nile-grid-resizer'); h.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, clientX: 200 })); document.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 230 })); })()");
        // (`g.remove()` is the grid's row API: detach the element itself.)
        js(&mut p, "g.parentNode.removeChild(g)");
        settle(&mut p);
        assert_eq!(js(&mut p, "g.isConnected"), "false");
        // The drag ended on disconnect, before any mouseup: reported once, later moves and releases do nothing.
        assert_eq!(js(&mut p, "log.filter((x) => x.startsWith('resize')).length"), "2", "reported on disconnect");
        js(&mut p, "document.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 400 })); document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))");
        settle(&mut p);
        assert_eq!(js(&mut p, "log.filter((x) => x.startsWith('resize')).length"), "2", "reported once per drag");
        let _ = std::fs::remove_file(path);
    }


    #[test]
    fn the_ui_splitter_drags_collapses_and_remembers() {
        let key = format!("splitter-test-{}", std::process::id());
        let (mut p, path) = page(
            &r#"<body style="margin:0"><nile-splitter id="s" persist="PERSIST_KEY" style="width:600px;height:100px">
                 <div id="a" size="200" min="100" max="400" collapsible>a</div><div id="b" min="50">b</div>
               </nile-splitter>
            <script type="module">
              import "@nile/ui";
              window.s = document.getElementById("s");
              window.changes = 0;
              s.addEventListener("splitterchange", () => changes++);
              window.grip = () => { const r = document.querySelector(".nile-splitter-grip").getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; };
            </script></body>"#
                .replace("PERSIST_KEY", &key),
            "ui-splitter",
        );
        p.run_timers();
        let sizes = |p: &mut Page| js(p, "s.sizes.join()");
        let grip = |p: &mut Page| -> (f32, f32) {
            let v = js(p, "grip().join()");
            let mut it = v.split(',').map(|n| n.parse::<f32>().unwrap());
            (it.next().unwrap(), it.next().unwrap())
        };
        assert_eq!(sizes(&mut p), "200,390", "the size attribute sets the first pane, the other takes the rest");
        // Drag the grip 100px right.
        let (x, y) = grip(&mut p);
        p.pointer_moved(x, y);
        p.press();
        p.pointer_moved(x + 100.0, y);
        p.release();
        assert_eq!(sizes(&mut p), "300,290");
        // max is 400: dragging further stops there.
        let (x, y) = grip(&mut p);
        p.pointer_moved(x, y);
        p.press();
        p.pointer_moved(x + 290.0, y);
        p.release();
        assert_eq!(sizes(&mut p), "400,190");
        // A click without a drag collapses the collapsible pane, and another brings it back.
        let (x, y) = grip(&mut p);
        p.pointer_moved(x, y);
        p.press();
        p.release();
        assert_eq!(js(&mut p, "[s.sizes.join(), s.isCollapsed(0)].join(';')"), "0,590;true");
        let (x, y) = grip(&mut p);
        p.pointer_moved(x, y);
        p.press();
        p.release();
        assert_eq!(sizes(&mut p), "400,190", "expanding restores the size it had");
        // Arrow keys on the focused grip move it by 10px.
        js(&mut p, "document.querySelector('.nile-splitter-grip').focus()");
        p.key("keydown", "ArrowLeft".into(), ModifiersState::default());
        assert_eq!(sizes(&mut p), "390,200");
        assert!(js(&mut p, "changes").parse::<u32>().unwrap() >= 5);
        // The layout is saved for next time.
        std::thread::sleep(Duration::from_millis(150));
        p.run_timers();
        assert_eq!(js(&mut p, &format!("localStorage.getItem('nile-splitter:{key}')")), r#"{"sizes":[390,null],"collapsed":[false,false]}"#);
        js(&mut p, &format!("localStorage.removeItem('nile-splitter:{key}')"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_grid_draws_only_visible_rows_and_sorts_selects_and_resizes() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><nile-grid id="g" key="id" select="multiple" row-height="30" empty="Nothing here" style="width:600px;height:200px">
                 <nile-column key="id" title="#" width="80" type="number"></nile-column>
                 <nile-column key="name" title="Name"></nile-column>
               </nile-grid>
            <script type="module">
              import "@nile/ui";
              const g = window.g = document.getElementById("g");
              window.events = [];
              g.addEventListener("selectionchange", (e) => events.push("sel:" + e.detail.indexes.join(".")));
              g.addEventListener("sort", (e) => events.push("sort:" + e.detail.key + ":" + e.detail.direction));
              g.addEventListener("columnresize", (e) => events.push("resize:" + e.detail.key + ":" + e.detail.width));
              g.addEventListener("rowactivate", (e) => events.push("go:" + e.detail.index));
              window.empty = () => getComputedStyle(g.querySelector(".nile-grid-empty")).display;
              window.mounted = () => g.mountedCount;
              // Rows are recycled, so the document order isn't the grid order: go by each row's position.
              window.shown = () => Array.from(g.querySelectorAll(".nile-grid-row")).filter((r) => r.style.display !== "none").sort((a, b) => a._vi - b._vi);
              window.firstCells = () => shown().slice(0, 3).map((r) => r.querySelector(".nile-grid-cell:nth-child(2)").textContent).join();
              window.at = (sel, i) => { const el = sel === ".nile-grid-row" ? shown()[i] : (i === undefined ? g.querySelector(sel) : g.querySelectorAll(sel)[i]); const r = el.getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2].join(); };
              g.rows = Array.from({ length: 10000 }, (_, i) => ({ id: i + 1, name: "Item " + (10000 - i) }));
            </script></body>"##,
            "ui-grid",
        );
        p.run_timers();
        let at = |p: &mut Page, sel: &str, i: Option<usize>| -> (f32, f32) {
            let v = match i {
                Some(i) => js(p, &format!("at('{sel}', {i})")),
                None => js(p, &format!("at('{sel}')")),
            };
            let mut it = v.split(',').map(|n| n.parse::<f32>().unwrap());
            (it.next().unwrap(), it.next().unwrap())
        };
        let click_at = |p: &mut Page, (x, y): (f32, f32)| {
            p.pointer_moved(x, y);
            p.press();
            p.release();
        };
        // 10,000 rows, but only the ones in view (and a little beyond) are in the document.
        let mounted: usize = js(&mut p, "mounted()").parse().unwrap();
        assert!((5..=20).contains(&mounted), "mounted {mounted}");
        assert_eq!(js(&mut p, "g.rowCount"), "10000");
        assert_eq!(js(&mut p, "empty()"), "none");
        // Scrolling mounts new rows and drops the old ones.
        js(&mut p, "g.querySelector('.nile-grid-body').scrollTop = 30 * 5000");
        p.run_timers();
        std::thread::sleep(Duration::from_millis(40));
        p.run_timers();
        assert_eq!(js(&mut p, "g.querySelector('.nile-grid-row').textContent.length > 0 && mounted() <= 20"), "true");
        assert_eq!(js(&mut p, "shown().some((r) => r.firstChild.textContent === '5001')"), "true");
        js(&mut p, "g.querySelector('.nile-grid-body').scrollTop = 0");
        p.run_timers();
        std::thread::sleep(Duration::from_millis(40));
        p.run_timers();
        // Click the Name header: natural-order ascending ("Item 1", "Item 2", "Item 10"...), then descending.
        let header = at(&mut p, ".nile-grid-hcell", Some(1));
        click_at(&mut p, header);
        assert_eq!(js(&mut p, "firstCells()"), "Item 1,Item 2,Item 3");
        click_at(&mut p, header);
        assert_eq!(js(&mut p, "firstCells()"), "Item 10000,Item 9999,Item 9998");
        click_at(&mut p, header);
        assert_eq!(js(&mut p, "firstCells()"), "Item 10000,Item 9999,Item 9998", "a third click clears the sort, back to the data order");
        click_at(&mut p, header);
        click_at(&mut p, header);
        // Click selects, Shift-click extends, Ctrl-click toggles.
        let row = |p: &mut Page, i: usize| at(p, ".nile-grid-row", Some(i));
        let r1 = row(&mut p, 1);
        click_at(&mut p, r1);
        assert_eq!(js(&mut p, "g.selectedIndex"), "1");
        p.mods = ModifiersState::SHIFT;
        let r3 = row(&mut p, 3);
        click_at(&mut p, r3);
        assert_eq!(js(&mut p, "g.selected.map((r) => r.id).join()"), "2,3,4", "shift-click selects the range");
        p.mods = ModifiersState::CONTROL;
        let r2 = row(&mut p, 2);
        click_at(&mut p, r2);
        assert_eq!(js(&mut p, "g.selected.map((r) => r.id).join()"), "2,4", "ctrl-click removed the middle one");
        p.mods = ModifiersState::default();
        // Keys: arrows move the selection, Enter activates.
        js(&mut p, "g.querySelector('.nile-grid-body').focus()");
        p.key("keydown", "ArrowDown".into(), ModifiersState::default());
        assert_eq!(js(&mut p, "g.selectedIndex"), "3");
        p.key("keydown", "Enter".into(), ModifiersState::default());
        assert!(js(&mut p, "events.join()").contains("go:3"));
        p.key("keydown", "End".into(), ModifiersState::default());
        assert_eq!(js(&mut p, "g.selectedIndex"), "9999");
        // Dragging the edge of a column header resizes it.
        let edge = at(&mut p, ".nile-grid-resizer", Some(0));
        p.pointer_moved(edge.0, edge.1);
        p.press();
        p.pointer_moved(edge.0 + 40.0, edge.1);
        p.release();
        assert_eq!(js(&mut p, "g.columns[0]._w"), "120");
        assert!(js(&mut p, "events.join()").contains("resize:id:120"));
        // Changing the data keeps the selection by key; an empty grid shows its message.
        js(&mut p, "g.rows = [{ id: 7, name: 'x' }]");
        assert_eq!(js(&mut p, "g.selected.length"), "0");
        js(&mut p, "g.rows = []");
        assert_eq!(js(&mut p, "empty()"), "flex");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_ui_splitter_reads_frameset_style_sizes() {
        let (mut p, path) = page(
            r#"<body style="margin:0">
               <nile-splitter id="a" cols="100px 1* 2*" style="width:600px;height:50px"><div>a</div><div>b</div><div>c</div></nile-splitter>
               <nile-splitter id="b" style="width:600px;height:50px"><div size="25%">a</div><div size="3*">b</div><div size="1*">c</div></nile-splitter>
               <nile-splitter id="c" rows="40 *" style="width:300px;height:200px"><div>a</div><div>b</div></nile-splitter>
            <script type="module">
              import "@nile/ui";
              window.sizes = (id) => document.getElementById(id).sizes.map(Math.round).join();
            </script></body>"#,
            "ui-splitter-units",
        );
        p.run_timers();
        // 600px minus two 10px grips: 100px fixed, the other 480 shared 1:2.
        assert_eq!(js(&mut p, "sizes('a')"), "100,160,320");
        // 25% of 600 = 150; the other 430 (after the grips) is shared 3:1, rounded to whole pixels.
        assert_eq!(js(&mut p, "sizes('b')"), "150,323,107");
        // `rows` stacks the panes: 40px, then the rest (200 - 10 grip).
        assert_eq!(js(&mut p, "sizes('c')"), "40,150");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_sys_module_reads_and_writes_files() {
        let dir = std::env::temp_dir().join(format!("nile-sys-{}", std::process::id())).to_string_lossy().replace('\\', "/");
        let (mut p, path) = page(
            &format!(r#"<body><script type="module">
              import * as sys from "@sys";
              const dir = "{dir}";
              const fs = sys.fs;
              fs.mkdirSync(dir);
              fs.writeFileSync(dir + "/a.txt", "héllo");
              fs.writeFileSync(dir + "/b.bin", new Uint8Array([1, 2, 3]).buffer);
              const text = new TextDecoder().decode(fs.readFileSync(dir + "/a.txt"));
              const st = fs.statSync(dir + "/a.txt");
              const names = fs.readdirSync(dir).map((e) => e.name + ":" + e.type).join(",");
              const missing = [fs.statSync(dir + "/nope"), fs.readdirSync(dir + "/nope")];
              fs.renameSync(dir + "/b.bin", dir + "/c.bin");
              fs.unlinkSync(dir + "/a.txt");
              window.out = [text, st.isFile, st.st_size, names, JSON.stringify(missing), fs.readdirSync(dir).map((e) => e.name).join(","), fs.splitpath("/x/y/z.txt").join("|")].join(";");
              fs.readFile(dir + "/nope").catch((e) => {{ window.err = e.code; }});
            </script></body>"#),
            "sys",
        );
        p.run_timers();
        assert_eq!(js(&mut p, "window.out"), "héllo;true;6;a.txt:1,b.bin:1;[null,null];c.bin;/x/y|z.txt");
        assert_eq!(js(&mut p, "window.err"), "ENOENT");
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_sys_module_runs_processes_talks_over_sockets_and_opens_files() {
        let dir = std::env::temp_dir().join(format!("nile-sys2-{}", std::process::id())).to_string_lossy().replace('\\', "/");
        let command = if cfg!(windows) { r#"["cmd", "/c", "echo hi"]"# } else { r#"["echo", "hi"]"# };
        let (mut p, path) = page(
            &format!(r#"<body><script type="module">
              import * as sys from "@sys";
              const log = []; window.log = log; window.done = 0;
              const dec = new TextDecoder();
              const dir = "{dir}";
              sys.fs.mkdirSync(dir);
              // A file handle: write, then read back at a position.
              const f = sys.fs.openSync(dir + "/f.txt", "w+");
              f.writeSync("hello world");
              log.push("file:" + dec.decode(f.readSync(5, 6)) + ":" + f.statSync().st_size);
              f.closeSync();
              // A process and its stdout.
              const proc = sys.spawn({command});
              (async () => {{
                const out = await proc.stdout.read();
                const status = await proc.wait();
                log.push("proc:" + dec.decode(out).trim() + ":" + status.exit_status);
                // A TCP echo over loopback.
                const server = new sys.TCP();
                server.bind({{ ip: "127.0.0.1", port: 0 }});
                server.listen();
                const port = server.getsockname().port;
                const accepting = server.accept().then(async (conn) => {{ const data = await conn.read(); await conn.write(data); conn.close(); }});
                const client = new sys.TCP();
                await client.connect({{ ip: "127.0.0.1", port }});
                await client.write("ping");
                log.push("tcp:" + dec.decode(await client.read()));
                await accepting;
                client.close(); server.close();
                // UDP.
                const a = new sys.UDP(); a.bind({{ ip: "127.0.0.1", port: 0 }});
                const b = new sys.UDP();
                await b.send("dgram", {{ ip: "127.0.0.1", port: a.getsockname().port }});
                log.push("udp:" + dec.decode((await a.recv()).data));
                log.push("os:" + (typeof sys.uname().sysname) + ":" + (sys.getenv("NILE_SURELY_UNSET") === undefined) + ":" + sys.random(8).length + ":" + (typeof sys.hrtime()) + ":" + (sys.cwd().length > 0));
                sys.setenv("NILE_SYS_TEST", "1"); log.push("env:" + sys.getenv("NILE_SYS_TEST"));
                window.done = 1;
              }})().catch((e) => {{ log.push("error:" + e.message); window.done = 1; }});
            </script></body>"#),
            "sys2",
        );
        for _ in 0..500 {
            p.tick();
            if js(&mut p, "window.done") == "1" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(js(&mut p, "window.log.join('|')"), "file:world:11|proc:hi:0|tcp:ping|udp:dgram|os:string:true:8:bigint:true|env:1");
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_sys_module_has_named_pipes_ttys_and_fs_watch() {
        let dir = std::env::temp_dir().join(format!("nile-sys3-{}", std::process::id())).to_string_lossy().replace('\\', "/");
        let (mut p, path) = page(
            &format!(r#"<body><script type="module">
              import * as sys from "@sys";
              const log = []; window.log = log; window.done = 0;
              const dec = new TextDecoder();
              const dir = "{dir}";
              sys.fs.mkdirSync(dir);
              (async () => {{
                // A named pipe: the server echoes in upper case.
                const server = new sys.Pipe();
                server.bind(dir + "/echo.sock");
                server.listen();
                const serving = server.accept().then(async (conn) => {{ const d = await conn.read(); await conn.write(dec.decode(d).toUpperCase()); conn.close(); }});
                const client = new sys.Pipe();
                await client.connect(dir + "/echo.sock");
                await client.write("ping");
                log.push("pipe:" + dec.decode(await client.read()) + ":" + client.getpeername().endsWith("echo.sock"));
                await serving;
                client.close(); server.close();
                // The terminal calls answer even without a terminal; only 0, 1 and 2 are terminals.
                const size = new sys.TTY(1).getWinSize();
                let bad = "none";
                try {{ new sys.TTY(7); }} catch (e) {{ bad = e.code; }}
                log.push("tty:" + (size.width > 0 && size.height > 0) + ":" + bad);
                // fs.watch sees a new file and then a change to it.
                const seen = [];
                const watch = sys.fs.watch(dir, (path, events) => seen.push([path.split("/").pop(), events]));
                let missing = "none";
                try {{ sys.fs.watch(dir + "/nope", () => {{}}); }} catch (e) {{ missing = e.code; }}
                await new Promise((r) => setTimeout(r, 400));
                sys.fs.writeFileSync(dir + "/new.txt", "one");
                await new Promise((r) => setTimeout(r, 700));
                sys.fs.writeFileSync(dir + "/new.txt", "one two three");
                await new Promise((r) => setTimeout(r, 700));
                watch.close();
                // The system may merge flags (a create arrives with the write that followed it); the second write has the change bit.
                const mine = seen.filter((s) => s[0] === "new.txt").map((s) => s[1]);
                log.push("watch:" + (mine.length >= 2 && (mine[0] & 1) === 1 && (mine[mine.length - 1] & 2) === 2 ? "ok" : JSON.stringify(mine)) + ":" + missing + ":" + (watch.path === dir));
                window.done = 1;
              }})().catch((e) => {{ log.push("error:" + e.message); window.done = 1; }});
            </script></body>"#),
            "sys3",
        );
        for _ in 0..800 {
            p.tick();
            if js(&mut p, "window.done") == "1" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let log = js(&mut p, "window.log.join('|')");
        assert!(log.starts_with("pipe:PING:true|tty:true:EBADF|watch:ok:ENOENT"), "{log}");
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn linked_style_sheets_and_imports_apply() {
        let dir = std::env::temp_dir().join(format!("nile-css-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("main.css"), "@import \"sub/part.css\"; #a { color: rgb(255, 0, 0) }").unwrap();
        std::fs::write(dir.join("sub/part.css"), "#b { color: rgb(0, 0, 255) }").unwrap();
        let main = dir.join("index.html");
        std::fs::write(&main, r#"<head><link rel="stylesheet" href="main.css"></head><body><p id="a">a</p><p id="b">b</p></body>"#).unwrap();
        let mut p = Page::load(main, (400.0, 200.0), 0.0);
        p.layout(400.0, 200.0);
        assert_eq!(js(&mut p, "getComputedStyle(document.getElementById('a')).color"), "rgb(255, 0, 0)");
        assert_eq!(js(&mut p, "getComputedStyle(document.getElementById('b')).color"), "rgb(0, 0, 255)");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn restyling_after_a_state_change_matches_a_full_cascade() {
        let (mut p, path) = page(
            r#"<head><style>
                 .row:hover { background: #def; color: red }
                 .row:hover .cell { font-weight: bold }
                 .a:hover + .b { color: green }
                 .a:hover ~ .c { color: blue }
                 input:focus { border-color: orange }
                 .row:active { margin-left: 3px }
               </style></head><body style="margin:0">
               <div id="list"><div class="row" id="r1"><span class="cell" id="c1">one</span></div><div class="row" id="r2"><span class="cell">two</span></div></div>
               <p class="a" id="a">a</p><p class="b" id="b">b</p><p class="c" id="c">c</p><p class="d">d</p><input id="i"></body>"#,
            "partial",
        );
        p.layout(400.0, 300.0);
        let id = |p: &Page, s: &str| p.doc.borrow().find(|n| n.attr("id") == Some(s)).unwrap();
        let (r1, r2, c1, a, i) = (id(&p, "r1"), id(&p, "r2"), id(&p, "c1"), id(&p, "a"), id(&p, "i"));
        let check = |p: &Page, what: &str| {
            let v = p.view.borrow();
            let doc = p.doc.borrow();
            let full = nile_style::compute_styles_in(&doc, &v.interaction, v.viewport);
            for n in 0..doc.node_count() {
                assert!(v.styles.styles[n] == full.styles[n], "{what}: node {n} differs after a partial restyle");
            }
        };
        for (what, next) in [
            ("hover a row", InteractionState { hover: Some(c1), ..Default::default() }),
            ("move to the other row", InteractionState { hover: Some(r2), ..Default::default() }),
            ("hover the sibling-combinator source", InteractionState { hover: Some(a), ..Default::default() }),
            ("press", InteractionState { hover: Some(r1), active: Some(r1), ..Default::default() }),
            ("focus", InteractionState { hover: None, active: None, focus: Some(i) }),
            ("leave", InteractionState::default()),
        ] {
            p.set_interaction(next);
            check(&p, what);
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn partial_restyles_keep_transitions_working() {
        let (mut p, path) = page(
            r#"<head><style>
                 .row { height: 30px; background: #fff; transition: background 0.1s }
                 .row:hover { background: #def }
               </style></head><body style="margin:0"><div class="row" id="r1">one</div><div class="row" id="r2">two</div></body>"#,
            "partial-anim",
        );
        p.layout(400.0, 300.0);
        let row = |p: &Page, s: &str| p.doc.borrow().find(|n| n.attr("id") == Some(s)).unwrap();
        let (r1, r2) = (row(&p, "r1"), row(&p, "r2"));
        let bg = |p: &Page, n: NodeId| p.view.borrow().styles.get(n).background;
        // The first change after load seeds the animator (there is nothing to transition from yet).
        p.pointer_moved(20.0, 10.0);
        std::thread::sleep(std::time::Duration::from_millis(150));
        p.tick();
        let (hovered, plain) = (bg(&p, r1), bg(&p, r2));
        assert_ne!(hovered, plain, "the first row shows the hover colour");
        // Moving to the other row restyles only those rows and starts transitions on both.
        p.pointer_moved(20.0, 40.0);
        assert!(p.view.borrow().animator.is_active(), "a transition is running");
        assert_eq!(bg(&p, r2), plain, "it starts from the old colour");
        std::thread::sleep(std::time::Duration::from_millis(150));
        p.tick();
        assert_eq!((bg(&p, r1), bg(&p, r2)), (plain, hovered), "the transitions ended on the new colours");
        let (v, doc) = (p.view.borrow(), p.doc.borrow());
        let full = nile_style::compute_styles_in(&doc, &v.interaction, v.viewport);
        for n in 0..doc.node_count() {
            assert!(v.styles.styles[n] == full.styles[n], "node {n} differs from a full cascade after the transitions ended");
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn blobs_form_data_and_streams_work() {
        let (mut p, path) = page(
            r#"<body><form id="f"><input name="a" value="1"><input name="b" type="checkbox" checked><input name="c" type="checkbox"><select name="s"><option value="x">X</option><option value="y" selected>Y</option></select><textarea name="t">hi</textarea><button name="go">Go</button></form><script>
              window.log = []; window.done = 0;
              (async () => {
                const log = window.log;
                const blob = new Blob(["héllo ", new Uint8Array([119, 111, 114, 108, 100])], { type: "Text/Plain" });
                log.push(blob.size + ":" + blob.type + ":" + await blob.text() + ":" + await blob.slice(7, 12).text());
                const file = new File(["data"], "a.txt", { type: "text/plain" });
                log.push(file.name + ":" + file.size + ":" + (file instanceof Blob) + ":" + await file.text());
                const fd = new FormData(document.getElementById("f"));
                log.push([...fd].map(([k, v]) => k + "=" + v).join("&"));
                fd.append("file", file); fd.set("a", "2"); fd.append("a", "3");
                log.push(fd.getAll("a").join(",") + ":" + fd.has("file") + ":" + fd.get("file").name);
                // The multipart body round-trips through a Response.
                const req = new Request("http://x/", { method: "POST", body: fd });
                const ct = req.headers.get("content-type");
                const back = await new Response(await req.arrayBuffer(), { headers: { "content-type": ct } }).formData();
                log.push(ct.split(";")[0] + ":" + back.getAll("a").join(",") + ":" + back.get("s") + ":" + back.get("file").name + ":" + await back.get("file").text());
                // Streams: a source, a transform and a reader.
                const src = new ReadableStream({ start(c) { c.enqueue("ab"); c.enqueue("cd"); c.close(); } });
                const up = src.pipeThrough(new TransformStream({ transform(ch, c) { c.enqueue(ch.toUpperCase()); } }));
                let out = ""; for await (const ch of up) out += ch;
                const r = new Response("one two").body.getReader();
                const first = await r.read();
                log.push(out + ":" + first.done + ":" + new TextDecoder().decode(first.value) + ":" + (await r.read()).done);
                const [t1, t2] = new Blob(["xyz"]).stream().tee();
                const read = async (st) => { let n = 0; for await (const c of st) n += c.length; return n; };
                log.push((await read(t1)) + "," + (await read(t2)));
                const sink = []; const w = new WritableStream({ write(c) { sink.push(c); } }).getWriter();
                await w.write(1); await w.write(2); await w.close();
                log.push(sink.join("+"));
                window.done = 1;
              })().catch((e) => { window.log.push("error:" + e.message); window.done = 1; });
            </script></body>"#,
            "blobs",
        );
        for _ in 0..200 {
            p.tick();
            if js(&mut p, "window.done") == "1" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            js(&mut p, "window.log.join('|')"),
            "12:text/plain:héllo world:world|a.txt:4:true:data|a=1&b=on&s=y&t=hi|2,3:true:a.txt|multipart/form-data:2,3:y:a.txt:data|AB CD:false:one two:true".replace("AB CD", "ABCD") + "|3,3|1+2"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn dropped_files_go_to_a_file_input_or_a_drop_event() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><input id="f" type="file" style="margin:0;width:200px;height:30px"><input id="m" type="file" multiple style="margin:0;width:200px;height:30px"><div id="z" style="height:60px"></div>
               <script>window.log = [];
                 for (const id of ["f", "m"]) document.getElementById(id).addEventListener("change", () => log.push(id + ":" + [...document.getElementById(id).files].map((f) => f.name).join("+")));
                 document.getElementById("z").addEventListener("drop", (e) => log.push("drop:" + e.dataTransfer.files.length + ":" + e.dataTransfer.files[0].name));</script></body>"#,
            "drop",
        );
        let files = vec!["/tmp/a.txt".to_string(), "/tmp/b.txt".to_string()];
        let over = |p: &mut Page, id: &str| {
            let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
            let r = p.view.borrow_mut().layout().node_rect(node).unwrap();
            p.pointer_moved(r.x + 5.0, r.y + 5.0);
        };
        over(&mut p, "f");
        assert!(p.drop_files(&files));
        over(&mut p, "m");
        p.drop_files(&files);
        over(&mut p, "z");
        p.drop_files(&files);
        assert_eq!(js(&mut p, "log.join('|')"), "f:a.txt|m:a.txt+b.txt|drop:2:a.txt");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn date_time_and_colour_inputs_open_pickers_that_set_the_value() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><input id="d" type="date" value="2026-10-15" style="margin:0;width:200px"><br>
               <input id="t" type="time" value="09:30" style="margin:0;width:200px"><br>
               <input id="c" type="color" value="#102030" style="margin:0"><br>
               <input id="w" type="week" value="2026-W42" style="margin:0;width:200px"><br>
               <input id="m" type="month" value="2026-10" style="margin:0;width:200px">
               <script>window.log = [];
                 for (const id of ["d", "t", "c", "w", "m"]) document.getElementById(id).addEventListener("change", (e) => log.push(id + "=" + e.target.value));
               </script></body>"##,
            "pickers",
        );
        let at = |p: &mut Page, id: &str| {
            let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
            p.view.borrow_mut().layout().node_rect(node).unwrap()
        };
        let open = |p: &mut Page, id: &str, left: bool| {
            let r = at(p, id);
            p.pointer_moved(if left { r.x + 10.0 } else { r.x + r.width - 10.0 }, r.y + r.height / 2.0);
            p.press();
            p.release();
        };
        let count = |p: &mut Page| js(p, "document.querySelectorAll('.nile-picker').length");
        // A click in the middle of a date field edits it; the glyph opens the calendar.
        let r = at(&mut p, "d");
        p.pointer_moved(r.x + 20.0, r.y + 8.0);
        p.press();
        p.release();
        assert_eq!(count(&mut p), "0");
        assert_eq!(p.cursor(), nile_style::Cursor::Text, "the text of the field");
        p.pointer_moved(r.x + r.width - 10.0, r.y + 8.0);
        assert_eq!(p.cursor(), nile_style::Cursor::Default, "the glyph is a button");
        open(&mut p, "d", false);
        assert_eq!(count(&mut p), "1");
        js(&mut p, "[...document.querySelectorAll('.np-day:not(.np-out)')].find((b) => b.textContent === '20').click()");
        assert_eq!(js(&mut p, "document.getElementById('d').value"), "2026-10-20");
        assert_eq!(count(&mut p), "0", "choosing a day closes the picker");
        open(&mut p, "t", false);
        js(&mut p, "document.querySelectorAll('.np-col')[0].children[14].click()");
        js(&mut p, "document.querySelectorAll('.np-col')[1].children[5].click()");
        assert_eq!(js(&mut p, "document.getElementById('t').value"), "14:05");
        open(&mut p, "c", true);
        assert_eq!(count(&mut p), "1");
        js(&mut p, "document.querySelector('.np-swatch').click()");
        assert_eq!(js(&mut p, "document.getElementById('c').value"), "#000000");
        js(&mut p, "const f = document.querySelector('.np-hex'); f.value = '#336699'; f.dispatchEvent(new Event('change', { bubbles: true }))");
        assert_eq!(js(&mut p, "document.getElementById('c').value"), "#336699");
        // Escape closes a picker (this one covers the fields below it); then the week and month pickers.
        press_key(&mut p, "Escape", ModifiersState::default());
        assert_eq!(count(&mut p), "0");
        open(&mut p, "w", false);
        assert_eq!(js(&mut p, "[...document.querySelectorAll('.nile-picker')].map((x) => x.getAttribute('aria-label')).join()"), "Choose week");
        js(&mut p, "[...document.querySelectorAll('.np-day:not(.np-out)')].find((b) => b.textContent === '1').click()");
        assert_eq!(js(&mut p, "document.getElementById('w').value"), "2026-W40");
        open(&mut p, "m", false);
        js(&mut p, "[...document.querySelectorAll('.np-cell')].find((b) => b.textContent === 'Mar').click()");
        assert_eq!(js(&mut p, "document.getElementById('m').value"), "2026-03");
        assert!(js(&mut p, "log.join(',')").starts_with("d=2026-10-20,t=14:05"), "{}", js(&mut p, "log.join(',')"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn followed_links_match_visited_and_stop_matching_link() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><a id="l" href="#x" style="display:block;height:20px">link</a><a id="n">plain</a></body>"##,
            "visited",
        );
        let q = |p: &mut Page, sel: &str| js(p, &format!("document.getElementById('l').matches('{sel}')"));
        assert_eq!((q(&mut p, ":link"), q(&mut p, ":any-link"), q(&mut p, ":visited")), ("true".into(), "true".into(), "false".into()));
        assert_eq!(js(&mut p, "document.getElementById('n').matches(':any-link')"), "false");
        click(&mut p, "l");
        assert_eq!((q(&mut p, ":link"), q(&mut p, ":any-link"), q(&mut p, ":visited")), ("false".into(), "true".into(), "true".into()));
        // Scripts can set the flag too, as in Sciter.
        js(&mut p, "document.getElementById('l').state.visited = false");
        assert_eq!(q(&mut p, ":link"), "true");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn css_prototype_makes_elements_instances_of_a_script_class() {
        let module = std::env::temp_dir().join(format!("nile-proto-{}.js", std::process::id()));
        std::fs::write(
            &module,
            r#"export class Tabs extends Element {
                 clicks = 0;
                 componentDidMount() { window.log.push("mount:" + this.localName + ":" + this.clicks); this.state.focusable = true; }
                 componentWillUnmount() { window.log.push("unmount"); }
                 get current() { const l = this.$(">strip>label:current"); return l ? l.attributes["for"] : "none"; }
                 ["on click at >strip>label"](event, label) { this.clicks++; this.$$(">strip>label").forEach((l) => l.state.current = l === label); window.log.push("click:" + label.textContent + ":" + this.current); return true; }
                 ["on ^keydown"](event) { window.log.push("key:" + event.key); }
               }"#,
        )
        .unwrap();
        let file = module.file_name().unwrap().to_string_lossy().into_owned();
        let (mut p, path) = page(
            &format!(
                r#"<head><style>tabs {{ prototype: Tabs url({file}) }} label:current {{ color: red }}</style></head>
                   <body><script>window.log = [];</script>
                   <tabs id="t"><strip><label for="a">A</label><label for="b">B</label></strip><panels><div id="p"><label>inner</label></div></panels></tabs></body>"#
            ),
            "proto",
        );
        for _ in 0..50 {
            p.tick();
            if js(&mut p, "window.log.length") != "0" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(js(&mut p, "window.log.join('|')"), "mount:tabs:0");
        assert_eq!(js(&mut p, "document.getElementById('t') instanceof Element && typeof document.getElementById('t').current"), "string");
        // The method-name handler: relative selector, `this` is the element, `true` stops the event.
        js(&mut p, "document.querySelectorAll('#t > strip > label')[1].click()");
        js(&mut p, "document.querySelector('#p label').click()"); // not a child of the strip: no handler
        assert_eq!(js(&mut p, "window.log.slice(1).join('|')"), "click:B:b");
        assert_eq!(js(&mut p, "document.getElementById('t').clicks"), "1");
        assert_eq!(js(&mut p, "document.getElementById('t').matches(':focusable')"), "true");
        // `:root` in a query started from an element is that element.
        assert_eq!(js(&mut p, "document.getElementById('t').$(':root > strip') !== null"), "true");
        js(&mut p, "document.getElementById('t').remove()");
        assert_eq!(js(&mut p, "window.log[window.log.length - 1]"), "unmount");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(module);
    }

    #[test]
    fn custom_elements_upgrade_parsed_created_and_new_elements() {
        let (mut p, path) = page(
            r##"<body><x-a id="p" k="1"></x-a><div id="host"></div><script>
              window.log = [];
              class XA extends HTMLElement {
                static observedAttributes = ["k"];
                n = 5;
                connectedCallback() { log.push("conn:" + this.id + ":" + this.n); }
                disconnectedCallback() { log.push("disc:" + this.id); }
                attributeChangedCallback(a, o, v) { log.push("attr:" + a + ":" + o + ">" + v); }
                hello() { return "hi " + this.localName; }
              }
              customElements.define("x-a", XA);
              const c = document.createElement("x-a"); c.id = "c"; c.setAttribute("k", "2");
              document.getElementById("host").append(c);
              const m = new XA(); m.id = "m";
              window.made = m;
              try { customElements.define("bad", class extends HTMLElement {}); } catch (e) { log.push("bad:" + e.name); }
            </script></body>"##,
            "custom-elements",
        );
        p.tick();
        let _ = js(&mut p, "0"); // let the mutation observer run
        p.tick();
        assert_eq!(js(&mut p, "document.getElementById('p') instanceof customElements.get('x-a')"), "true");
        assert_eq!(js(&mut p, "document.getElementById('p').hello()"), "hi x-a");
        assert_eq!(js(&mut p, "made.localName + ':' + (made instanceof XA)"), "x-a:true");
        assert_eq!(js(&mut p, "document.getElementById('c') instanceof XA"), "true");
        let log = js(&mut p, "window.log.join('|')");
        for want in ["conn:p:5", "attr:k:null>1", "conn:c:5", "attr:k:null>2", "bad:SyntaxError"] {
            assert!(log.contains(want), "{want} missing from {log}");
        }
        js(&mut p, "document.getElementById('c').remove()");
        p.tick();
        js(&mut p, "document.getElementById('p').setAttribute('k', '9')");
        let log = js(&mut p, "window.log.join('|')");
        assert!(log.contains("disc:c") && log.contains("attr:k:1>9"), "{log}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn translucent_windows_keep_the_canvas_alpha() {
        let canvas = |html: &str| {
            let (p, path) = page(html, "canvas-alpha");
            let c = p.canvas_background();
            let _ = std::fs::remove_file(path);
            (c.a(), c.r())
        };
        // Transparent frame: no background -> clear; a translucent one keeps its alpha.
        assert_eq!(canvas(r#"<html window-frame="transparent"><body></body></html>"#).0, 0);
        assert_eq!(canvas(r#"<html window-material="blur" style="background: rgba(255, 0, 0, 0.5)"><body></body></html>"#), (127, 255));
        // An ordinary window shows the same colour over white.
        assert_eq!(canvas(r#"<html style="background: rgba(0, 0, 0, 0.5)"><body></body></html>"#), (255, 128));
    }

    #[test]
    fn window_material_is_a_script_property_of_the_root_attribute() {
        let (mut p, path) = page(r#"<html window-material="hud"><body></body></html>"#, "material");
        assert_eq!(js(&mut p, "Window.this.material"), "hud");
        js(&mut p, "Window.this.material = 'mica'");
        assert_eq!(window::WindowConfig::from_document(&p.doc.borrow()).material, window::Material::Mica);
        js(&mut p, "Window.this.material = ''");
        assert!(!window::WindowConfig::from_document(&p.doc.borrow()).translucent());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_window_corner_radius_is_the_roots_border_radius() {
        let (p, path) = page(r#"<html style="border-radius: 18px"><body></body></html>"#, "corner");
        assert_eq!(p.window_corner_radius(), 18.0);
        let _ = std::fs::remove_file(path);
        let (p, path) = page("<html><body></body></html>", "corner0");
        assert_eq!(p.window_corner_radius(), 0.0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_maximize_button_has_a_rect_for_hit_testing() {
        let (p, path) = page(
            r#"<body style="margin:0"><div style="position:absolute;left:100px;top:4px;width:46px;height:30px" role="window-maximize"></div><div style="position:absolute;left:10px;top:4px;width:30px;height:30px" role="window-close"></div></body>"#,
            "maxbtn",
        );
        assert_eq!(p.maximize_button_rect(), Some([100.0, 4.0, 46.0, 30.0]));
        let _ = std::fs::remove_file(path);
        let (p, path) = page("<body></body>", "maxbtn0");
        assert_eq!(p.maximize_button_rect(), None);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn form_controls_validate_and_match_valid_and_invalid() {
        let (mut p, path) = page(
            r##"<body><form id="f"><input id="a" required><input id="n" type="number" min="1" max="5" step="2" value="4"><input id="e" type="email" value="x@y.z"><input id="t" pattern="[a-z]+" value="abc" maxlength="5"><input id="c" type="checkbox" required><input id="d" disabled required></form><script>
              window.log = [];
              const $ = (s) => document.querySelector(s);
              const v = (id) => { const x = $(id).validity; return Object.keys(x).filter((k) => x[k] === true && k !== "valid").join("+") || "ok"; };
              document.addEventListener("DOMContentLoaded", () => {
                log.push(v("#a"), v("#n"), v("#e"), v("#t"), v("#c"), v("#d"), $("#d").willValidate);
              });
              window.later = () => {
                $("#a").value = "x"; $("#n").value = "3"; $("#e").value = "nope"; $("#t").value = "ABCDEFG"; $("#c").checked = true;
                return new Promise((r) => setTimeout(r, 30)).then(() => {
                  log.push(v("#a"), v("#n"), v("#e"), v("#t"), v("#c"), $("#f").checkValidity(), $("#e").validationMessage !== "");
                  $("#e").setCustomValidity("custom"); log.push($("#e").validationMessage);
                  window.done = 1;
                });
              };
            </script></body>"##,
            "validity",
        );
        let doc_invalid = |p: &mut Page, id: &str, flag: &str| js(p, &format!("document.querySelector('#{id}').matches(':{flag}')"));
        assert_eq!(js(&mut p, "log.join('|')"), "valueMissing|stepMismatch|ok|ok|valueMissing|ok|false");
        assert_eq!(doc_invalid(&mut p, "a", "invalid"), "true");
        assert_eq!(doc_invalid(&mut p, "e", "valid"), "true");
        assert_eq!(doc_invalid(&mut p, "d", "invalid"), "false");
        js(&mut p, "later()");
        for _ in 0..100 {
            p.tick();
            if js(&mut p, "window.done") == "1" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(js(&mut p, "log.slice(7).join('|')"), "ok|ok|typeMismatch|patternMismatch+tooLong|ok|false|true|custom");
        assert_eq!(doc_invalid(&mut p, "e", "invalid"), "true");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn class_components_forms_storage_and_debug_work() {
        let db = std::env::temp_dir().join(format!("nile-store-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let src = r#"<body><form id="f"><input name="a" value="1"><input name="n" type="number" value="5"><input name="c" type="checkbox"><input name="r" type="radio" value="x"><input name="r" type="radio" value="y" checked></form><div id="host"></div><script type="module">
              import * as debug from "@debug";
              import * as DB from "@storage";
              window.log = [];
              const log = window.log;
              class Clock extends Element {
                time = 1;
                this(props, kids) { this.props = props; this.kids = kids; log.push("this:" + props.label); }
                render() { return <span class="c">{this.props.label}:{this.time}:{this.kids.length}</span>; }
                componentDidMount() { log.push("mount:" + this.isConnected); }
                componentDidUpdate() { log.push("update"); }
                componentWillUnmount() { log.push("unmount"); }
              }
              const host = document.getElementById("host");
              host.content(<div><Clock label="A"><i/></Clock></div>);
              const clock = host.querySelector("div").firstElementChild;
              log.push(clock instanceof Clock, clock.className, clock.textContent);
              host.patch(<div><Clock label="B"><i/></Clock></div>, true);
              log.push(host.querySelector("div").firstElementChild === clock ? "reused" : "new", clock.textContent);
              clock.componentUpdate({ time: 2 });
              clock.componentUpdate({ time: 3 });
              await 0;
              log.push(clock.textContent);
              clock.componentUpdate({ time: 3 });
              await 0;
              host.content(<p/>);
              log.push(JSON.stringify(document.getElementById("f").value));
              document.getElementById("f").value = { a: "z", c: true, r: "x" };
              log.push(JSON.stringify(document.getElementById("f").value));
              const e = Element.create(<b>hi</b>);
              log.push(e.localName);
              const path = "DBPATH";
              class Note { hello() { return "hi " + this.t; } }
              const store = DB.open(path);
              store.registerClass(Note);
              store.root.n = Object.assign(new Note(), { t: "x" });
              store.root.when = new Date(86400000);
              store.close();
              const again = DB.open(path);
              again.registerClass(Note);
              log.push(again.root.n.hello() + ":" + (again.root.when instanceof Date) + ":" + again.root.when.getTime());
              let caught = "";
              debug.setConsoleOutputHandler((sys, sev, msg) => { caught = sev + msg; return true; });
              console.warn("careful");
              log.push(caught);
              const frame = debug.callStackAt(0);
              log.push(typeof frame.lineNo);
              window.done = 1;
            </script></body>"#.replace("DBPATH", &db.to_string_lossy().replace('\\', "/"));
        let (mut p, path) = page(&src, "components");
        for _ in 0..200 {
            p.tick();
            if js(&mut p, "window.done") == "1" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            js(&mut p, "window.log.join('|')"),
            "this:A|mount:true|true|c|A:1:1|this:B|reused|B:1:1|update|B:3:1|unmount|{\"a\":\"1\",\"n\":5,\"c\":false,\"r\":\"y\"}|{\"a\":\"z\",\"n\":5,\"c\":true,\"r\":\"x\"}|b|hi x:true:86400000|1careful|number"
        );
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(db);
    }

    /// A WebSocket server for one connection: accepts, echoes data frames and answers a close.
    fn echo_websocket_server() -> u16 {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((mut s, _)) = listener.accept() else { return };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                if s.read(&mut byte).unwrap_or(0) == 0 {
                    return;
                }
                head.push(byte[0]);
            }
            let text = String::from_utf8_lossy(&head).to_string();
            assert!(text.starts_with("GET /chat?x=1 HTTP/1.1") && text.contains("Upgrade: websocket") && text.contains("Sec-WebSocket-Key: "), "{text}");
            s.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: x\r\n\r\n").unwrap();
            loop {
                let mut h = [0u8; 2];
                if s.read_exact(&mut h).is_err() {
                    return;
                }
                let (op, masked) = (h[0] & 15, h[1] & 0x80 != 0);
                let mut len = (h[1] & 127) as usize;
                if len == 126 {
                    let mut l = [0u8; 2];
                    s.read_exact(&mut l).unwrap();
                    len = u16::from_be_bytes(l) as usize;
                }
                let mut mask = [0u8; 4];
                assert!(masked, "client frames are masked");
                s.read_exact(&mut mask).unwrap();
                let mut data = vec![0u8; len];
                s.read_exact(&mut data).unwrap();
                for (i, b) in data.iter_mut().enumerate() {
                    *b ^= mask[i % 4];
                }
                // Echo (text and binary) or answer the close; a ping comes first, to be answered by the client.
                let mut out = vec![0x80 | op, data.len() as u8];
                if data.len() >= 126 {
                    out = vec![0x80 | op, 126, (data.len() >> 8) as u8, data.len() as u8];
                }
                out.extend(&data);
                if op == 1 && data == b"hello" {
                    s.write_all(&[0x89, 2, b'o', b'k']).unwrap();
                }
                s.write_all(&out).unwrap();
                if op == 8 {
                    return;
                }
            }
        });
        port
    }

    #[test]
    fn websockets_talk_to_a_server() {
        let port = echo_websocket_server();
        let (mut p, path) = page(
            &format!(r#"<body><script>
              window.log = []; window.done = 0;
              const ws = new WebSocket("ws://127.0.0.1:{port}/chat?x=1");
              ws.binaryType = "arraybuffer";
              ws.onopen = () => {{ log.push("open:" + ws.readyState); ws.send("hello"); ws.send(new Uint8Array([1, 2, 3])); ws.send("x".repeat(200)); }};
              ws.onmessage = (e) => {{
                log.push(typeof e.data === "string" ? e.data.slice(0, 8) + e.data.length : "bin:" + new Uint8Array(e.data).join(""));
                if (log.length === 4) ws.close(1000, "bye");
              }};
              ws.onerror = (e) => log.push("error");
              ws.onclose = (e) => {{ log.push("close:" + e.code + ":" + e.reason + ":" + ws.readyState); window.done = 1; }};
            </script></body>"#),
            "ws",
        );
        for _ in 0..400 {
            p.tick();
            if js(&mut p, "window.done") == "1" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(js(&mut p, "window.log.join('|')"), "open:1|hello5|bin:123|xxxxxxxx200|close:1000:bye:3");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn live_regions_are_marked_in_the_accessibility_tree() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="poll" aria-live="polite">Saved</div><div role="alert">Oops</div><div role="status" aria-atomic="false">Ready</div>
               <div role="log">line</div><div aria-live="off">quiet</div><div aria-live="assertive" aria-atomic="true"></div></body>"#,
            "live",
        );
        let tree = p.a11y_tree();
        let live = |name: &str| tree.nodes.iter().find(|n| n.role != a11y::A11yRole::StaticText && tree.nodes.iter().any(|c| n.children.contains(&c.id) && c.name == name)).map(|n| (n.live, n.live_atomic));
        use a11y::A11yLive::*;
        assert_eq!(live("Saved"), Some((Polite, false)));
        assert_eq!(live("Oops"), Some((Assertive, true)));
        assert_eq!(live("Ready"), Some((Polite, false)));
        assert_eq!(live("line"), Some((Polite, false)));
        // `aria-live="off"` is a plain container; an empty live region stays in the tree so a change can be announced.
        assert_eq!(live("quiet").map(|l| l.0), Some(Off));
        assert!(tree.nodes.iter().any(|n| n.live == Assertive && n.children.is_empty() && n.live_atomic));
        assert!(tree.to_json().contains("\"live\":\"assertive\""));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn frames_follow_scrolling_clipping_and_transforms_of_their_containers() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="box" style="width:200px;height:100px;overflow:auto"><div style="height:30px"></div>
               <iframe id="f" srcdoc="<p>hi</p>" style="display:block;width:150px;height:150px;border:0"></iframe></div>
               <iframe id="r" srcdoc="<p>tilt</p>" style="position:absolute;left:300px;top:20px;width:100px;height:40px;border:0;transform:rotate(90deg)"></iframe></body>"#,
            "frames-clip",
        );
        p.layout(600.0, 300.0);
        p.update_frames();
        let f = p.doc.borrow().find(|n| n.attr("id") == Some("f")).unwrap();
        let r = p.doc.borrow().find(|n| n.attr("id") == Some("r")).unwrap();
        assert_eq!(p.frame_at(10.0, 40.0), Some(f));
        // The container clips it: the part below the box's 100px is not the frame's.
        assert_eq!(p.frame_at(10.0, 120.0), None);
        // Scrolled by 20, the frame moves up with the content.
        let _ = p.eval("document.$('#box').scrollTop = 20");
        p.sync();
        p.update_frames();
        assert_eq!(p.frames.borrow().slots[&f].rect.1, 10.0);
        assert_eq!(p.frame_at(10.0, 5.0), None, "above the box's top edge is clipped");
        assert_eq!(p.frame_at(10.0, 15.0), Some(f));
        // Rotated a quarter turn about its centre (350, 40): the 100x40 frame now covers 330..370 x -10..90.
        assert_eq!(p.frame_at(350.0, 80.0), Some(r));
        assert_eq!(p.frame_at(310.0, 40.0), None, "the unrotated rectangle's ends are empty now");
        let (lx, ly) = p.frames.borrow().slots[&r].to_local(350.0, 80.0);
        assert!((lx - 90.0).abs() < 0.5 && (ly - 20.0).abs() < 0.5, "{lx},{ly}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn selections_in_editable_text_reach_the_accessibility_tree() {
        let (mut p, path) = page(r#"<body style="margin:0"><div id="ed" contenteditable="true"><p>Hello brave world</p></div></body>"#, "a11ysel");
        let id = p.doc.borrow().find(|n| n.tag() == Some("p")).unwrap();
        let text = p.doc.borrow().node(id).children[0];
        let host = p.doc.borrow().find(|n| n.tag() == Some("div")).unwrap();
        let mut next = p.interaction();
        next.focus = Some(host);
        p.set_interaction(next);
        p.view.borrow_mut().edit_sel = Some((nile_layout::DomPoint { node: text, offset: 6 }, nile_layout::DomPoint { node: text, offset: 11 }));
        let tree = p.a11y_tree();
        let h = tree.nodes.iter().find(|n| n.selection.is_some()).expect("the host carries the selection");
        let s = h.selection.unwrap();
        assert_eq!((s.anchor.1, s.focus.1), (6, 11));
        assert_eq!(s.anchor.0, s.focus.0);
        assert!(tree.nodes.iter().any(|n| n.id == s.anchor.0 && n.name == "Hello brave world"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_menu_element_becomes_the_application_menu() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><menu id="app"><li>File <menu>
                 <li id="open" accesskey="CmdOrCtrl+O">Open…</li><li id="save" accesskey="Shift+CmdOrCtrl+S" disabled>Save</li><hr/><hr/>
                 <li id="auto" checked>Auto <b>save</b></li><li id="gone" hidden>Hidden</li><li id="quit" role="quit">Quit</li></menu></li>
               <li>Edit <menu><li id="copy" role="copy">Copy</li></menu></li></menu>
               <input id="t" value="abc">
               <script>window.log = []; document.getElementById('open').addEventListener('click', () => log.push('open'));
                       document.getElementById('save').addEventListener('click', () => log.push('save'));</script></body>"#,
            "appmenu",
        );
        assert!(p.app_menu().is_none());
        js(&mut p, "Window.this.menu = document.getElementById('app'); 0");
        let menu = p.app_menu().expect("a menu");
        let MenuItem::Item { label, items, .. } = &menu[0] else { panic!() };
        assert_eq!(label, "File");
        let names: Vec<String> = items.iter().map(|i| match i { MenuItem::Item { label, enabled, checked, .. } => format!("{label}{}{}", if *enabled { "" } else { " (off)" }, if *checked { " (on)" } else { "" }), MenuItem::Separator => "-".into() }).collect();
        assert_eq!(names, ["Open…", "Save (off)", "-", "Auto save (on)", "Quit"]);
        let MenuItem::Item { id: open, accel, .. } = &items[0] else { panic!() };
        assert_eq!(accel, &Accel::parse("Ctrl+o"));
        let MenuItem::Item { id: save, .. } = &items[1] else { panic!() };
        let MenuItem::Item { id: quit, .. } = &items[4] else { panic!() };
        // Choosing an item fires `click`; a disabled one does nothing; window roles go to the embedder.
        assert_eq!(p.menu_click(*open), None);
        assert_eq!(p.menu_click(*save), None);
        assert_eq!(p.menu_click(*quit).as_deref(), Some("quit"));
        assert_eq!(js(&mut p, "log.join(',')"), "open");
        // The shortcut runs it where the platform's menu doesn't.
        let mut mods = ModifiersState::default();
        mods |= if cfg!(target_os = "macos") { ModifiersState::SUPER } else { ModifiersState::CONTROL };
        assert!(p.menu_accelerator("o", mods));
        assert!(!p.menu_accelerator("o", ModifiersState::default()));
        assert_eq!(js(&mut p, "log.join(',')"), "open,open");
        // toggleAttribute flips `checked`, which the menu follows.
        js(&mut p, "document.getElementById('auto').toggleAttribute('checked'); 0");
        let MenuItem::Item { items, .. } = &p.app_menu().unwrap()[0] else { panic!() };
        assert!(matches!(&items[3], MenuItem::Item { checked: false, .. }));
        assert_eq!(js(&mut p, "const a = document.getElementById('auto'); String(a.toggleAttribute('checked', true)) + a.hasAttribute('checked') + a.toggleAttribute('checked', false) + a.hasAttribute('checked')"), "truetruefalsefalse");
        // Hosts that build their own menus get it as JSON.
        let json = crate::menus::menu_json(&p.app_menu().unwrap());
        assert!(json.starts_with("[{\"id\":") && json.contains("\"label\":\"File\"") && json.contains("\"accel\":\"CmdOrCtrl+o\"") && json.contains("\"separator\":true") && json.contains("\"role\":\"quit\""), "{json}");
        js(&mut p, "Window.this.menu = null; 0");
        assert!(p.app_menu().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn paged_elements_edit_across_sheets() {
        let paras: String = (0..12).map(|i| format!("<p id=p{i} style=\"margin:0;font-size:16px;line-height:20px\">line {i}</p>")).collect();
        let (mut p, path) = page(
            &format!("<style>@page {{ size: 300px 140px; margin: 20px }} body{{margin:0}} #d{{page-view:paged;outline:none}}</style><div id=d contenteditable=true>{paras}</div>"),
            "paged-edit",
        );
        // 100px of content per sheet: five lines each, so three sheets.
        let painted = |p: &Page, id: &str| {
            let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
            let mut v = p.view.borrow_mut();
            let offsets = v.scroll_offsets.clone();
            v.layout().node_rect_scrolled(node, &offsets).unwrap()
        };
        assert_eq!(painted(&p, "p4").y, 20.0 + 4.0 * 20.0);
        let second = painted(&p, "p5");
        assert_eq!(second.y, 140.0 + 20.0 + 20.0, "the sixth line is the first on sheet two (140px sheet, 20px gap)");
        // Click in the middle of line 5 (viewport 300px: scroll so sheet two is on screen), then type.
        p.layout(600.0, 300.0);
        p.pointer_moved(second.x + 10.0, second.y + 10.0);
        p.press();
        p.release();
        type_keys(&mut p, "X");
        let doc = p.doc.clone();
        let text = move |id: &str| {
            let node = doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
            doc.borrow().text_content(node)
        };
        assert!(text("p5").contains('X'), "typed on line 5: {:?} / {:?}", text("p5"), text("p4"));
        assert!(!text("p4").contains('X'));
        // ArrowUp from the first line of sheet two goes to the last line of sheet one.
        press_key(&mut p, "ArrowUp", ModifiersState::default());
        type_keys(&mut p, "Y");
        assert!(text("p4").contains('Y'), "up crossed the sheet gap: {:?} / {:?}", text("p4"), text("p5"));
        // And ArrowDown comes back to the first line of sheet two.
        press_key(&mut p, "ArrowDown", ModifiersState::default());
        type_keys(&mut p, "Z");
        assert!(text("p5").contains('Z'), "down crossed the sheet gap: {:?}", text("p5"));
        // Scripts read the sheet count and the sheet with the caret; a page break adds one.
        assert_eq!(js(&mut p, "const d = document.getElementById('d'); d.pageCount + ',' + d.pageNumber"), "3,2");
        assert_eq!(js(&mut p, "document.execCommand('insertPageBreak'); document.getElementById('d').pageCount"), "4");
        assert_eq!(js(&mut p, "document.getElementById('d').pageNumber"), "3", "the caret starts the new sheet");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn documents_paginate_and_export_to_pdf() {
        let rows: String = (0..30).map(|i| format!("<p class=r>Row {i}</p>")).collect();
        let (mut p, path) = page(
            &format!("<style>@page {{ size: 400px 300px; margin: 20px; @bottom-center {{ content: \"{{n}}\" counter(page) \"/\" counter(pages) }} }} body{{margin:0}} .r{{margin:0;height:30px;break-inside:avoid}} .new{{break-before:page}}</style>{rows}<p class=new>Last</p>"),
            "paged",
        );
        // 260px of content per page: eight 30px rows each; 30 rows -> 4 pages, then a forced one.
        assert_eq!(p.paginate(), 5);
        assert_eq!(p.page_starts[1], 240.0);
        let (pixels, w, h) = p.render_page(0, 1.0).unwrap();
        assert_eq!((w, h), (400, 300));
        assert!(pixels.chunks(4).any(|px| px[..3] != [255, 255, 255]), "something was drawn");
        let mut pdf = Vec::new();
        assert_eq!(p.write_pdf(&mut pdf, "t"), 5);
        assert!(pdf.starts_with(b"%PDF-"));
        let _ = std::fs::remove_file(path);
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
    fn scroll_snap_settles_on_the_next_snap_point_after_the_wheel_pauses() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="box" style="width:200px; height:60px; overflow-x:auto; overflow-y:hidden; display:flex; scroll-snap-type:x mandatory">
                 <div style="flex:none; width:200px; scroll-snap-align:start">a</div><div style="flex:none; width:200px; scroll-snap-align:start">b</div>
                 <div style="flex:none; width:200px; scroll-snap-align:start">c</div></div></body>"#,
            "snap",
        );
        let (x, y) = center(&p, "box");
        p.pointer_moved(x, y);
        p.wheel(-30.0, 0.0);
        assert_eq!(offset(&p, "box"), (30.0, 0.0), "the wheel moves freely while it is running");
        std::thread::sleep(Duration::from_millis(200));
        p.tick();
        assert_eq!(offset(&p, "box"), (200.0, 0.0), "then it settles on the next item");
        // A small nudge back goes back to the item it came from.
        p.wheel(10.0, 0.0);
        std::thread::sleep(Duration::from_millis(200));
        p.tick();
        assert_eq!(offset(&p, "box"), (0.0, 0.0));
        let _ = std::fs::remove_file(path);
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
        assert!(p.view.borrow().layout.is_some(), "overflow scrolling must preserve cached geometry");
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

    #[test]
    fn scrollbars_follow_scrollbar_width_colors_and_the_pointer() {
        let (mut p, path) = page(
            r##"<head><style>
                 #thin { scrollbar-width: thin }
                 #none { scrollbar-width: none }
                 #wk::-webkit-scrollbar { width: 14px }
                 #wk::-webkit-scrollbar-thumb { background: #ff0000; border-radius: 2px }
                 #col { scrollbar-color: #00ff00 #0000ff }
               </style></head>
               <body style="margin:0"><div id="a" style="width:100px;height:80px;overflow:auto"><div style="height:400px">in</div></div>
               <div id="thin" style="width:100px;height:80px;overflow:auto"><div style="height:400px">in</div></div>
               <div id="none" style="width:100px;height:80px;overflow:auto"><div style="height:400px">in</div></div>
               <div id="wk" style="width:100px;height:80px;overflow:auto"><div style="height:400px">in</div></div>
               <div id="col" style="width:100px;height:80px;overflow:auto"><div style="height:400px">in</div></div></body>"##,
            "barlook",
        );
        let thick = |p: &mut Page, id: &str| {
            let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
            let v = p.view.borrow_mut();
            let look = v.styles.scrollbar_look(node);
            (look.thickness, look.thumb.map(|c| (c.r, c.g, c.b)), look.track.map(|c| (c.r, c.g, c.b)), look.thumb_radius)
        };
        assert_eq!(thick(&mut p, "a"), (nile_style::scrollbar_auto(), None, None, None));
        assert_eq!(thick(&mut p, "thin").0, nile_style::scrollbar_thin());
        assert_eq!(thick(&mut p, "none").0, 0.0);
        assert_eq!(thick(&mut p, "wk"), (14.0, Some((255, 0, 0)), None, Some(2.0)));
        assert_eq!(thick(&mut p, "col"), (nile_style::scrollbar_auto(), Some((0, 255, 0)), Some((0, 0, 255)), None));
        // A `none` bar has nothing to grab, but the wheel still scrolls.
        let node = p.doc.borrow().find(|n| n.attr("id") == Some("none")).unwrap();
        let sb = p.view.borrow_mut().layout().scroll_box(node).copied().unwrap();
        assert!(sb.thumbs(0.0, 0.0).is_empty() && sb.wheel_y);
        // The pointer over a bar marks it hovered; leaving clears it.
        p.pointer_moved(95.0, 20.0);
        assert!(p.bar_hover.is_some_and(|(n, v)| n.is_some() && v));
        p.pointer_moved(30.0, 20.0);
        assert_eq!(p.bar_hover, None);
        p.pointer_moved(95.0, 20.0);
        assert!(p.pointer_left() && p.bar_hover.is_none());
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
    fn range_progress_meter_and_file_inputs_show_the_arrow() {
        use nile_style::Cursor;
        let (mut p, path) = page(
            r##"<body style="margin:0"><input id="r" type="range"><br><progress id="p" value="3" max="10"></progress><br><meter id="m" value="0.4"></meter><br><input id="f" type="file"><br><input id="t" type="text"></body>"##,
            "cursor-controls",
        );
        let mut at = |id: &str| {
            let (x, y) = center(&p, id);
            p.pointer_moved(x, y);
            p.cursor()
        };
        assert_eq!((at("r"), at("p"), at("m"), at("f")), (Cursor::Default, Cursor::Default, Cursor::Default, Cursor::Default));
        assert_eq!(at("t"), Cursor::Text);
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
        // The ribbon tabs switch panels, and Insert > Table puts a table in the document.
        let click_node = |p: &mut Page, n: NodeId| {
            let r = p.view.borrow_mut().layout().node_rect(n).unwrap();
            p.pointer_moved(r.x + r.width / 2.0, r.y + r.height / 2.0);
            p.press();
            p.release();
        };
        let tab = p.doc.borrow().find(|n| n.attr("data-tab") == Some("Insert")).unwrap();
        click_node(&mut p, tab);
        assert_eq!(q(&mut p, "document.getElementById('home-ribbon').classList.contains('hidden')"), "true");
        assert_eq!(q(&mut p, "document.querySelectorAll('#other-ribbon [data-action]').length > 0"), "true");
        q(&mut p, "var t = document.getElementById('sample').firstChild; document.getElementById('paper').focus(); getSelection().setBaseAndExtent(t, 3, t, 3)");
        let table = p.doc.borrow().find(|n| n.attr("data-action") == Some("table")).unwrap();
        click_node(&mut p, table);
        assert_eq!(q(&mut p, "document.querySelectorAll('#paper td').length"), "9");
        let home = p.doc.borrow().find(|n| n.attr("data-tab") == Some("Home")).unwrap();
        click_node(&mut p, home);
        assert_eq!(q(&mut p, "document.getElementById('home-ribbon').classList.contains('hidden')"), "false");
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
        // Over the spin arrows the pointer is an arrow, over the text an I-beam.
        p.pointer_moved(r.x + r.width - 8.0, r.y + 3.0);
        assert_eq!(p.cursor(), nile_style::Cursor::Default);
        p.pointer_moved(r.x + 10.0, r.y + 5.0);
        assert_eq!(p.cursor(), nile_style::Cursor::Text);
        // The wheel over the focused input steps it (up increases) instead of scrolling the page.
        p.pointer_moved(r.x + 10.0, r.y + 5.0);
        p.wheel(0.0, 10.0);
        assert_eq!(val(&mut p, "d"), "0.4");
        p.wheel(0.0, -10.0);
        p.wheel(0.0, -10.0);
        assert_eq!(val(&mut p, "d"), "0.2");
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
    fn scripts_read_and_write_the_clipboard() {
        let (mut p, path) = page(
            r#"<body><script>
                 window.out = [];
                 out.push(Clipboard.has("text"), Clipboard.readText() === undefined);
                 Clipboard.writeText("plain");
                 out.push(Clipboard.readText(), Clipboard.has("html"));
                 Clipboard.write({ html: "<b>x</b>", text: "x" });
                 const r = Clipboard.read();
                 out.push(r.html, r.text);
                 const px = new ImageData(new Uint8ClampedArray([255, 0, 0, 255, 0, 255, 0, 255]), 2, 1);
                 out.push(Clipboard.write({ image: px }), Clipboard.has("image"), Clipboard.has("text"));
                 const im = Clipboard.read().image;
                 out.push(im.width, im.height, Array.from(im.data).join(" "));
                 Clipboard.write({ json: { a: [1, 2] }, text: "tag" });
                 out.push(JSON.stringify(Clipboard.read().json), Clipboard.readText());
                 Clipboard.writeText("other");
                 out.push(Clipboard.has("json"));
                 Clipboard.write({ file: ["/a/b.txt", "/c.png"] });
                 out.push(Clipboard.read().file.join("|"));
               </script></body>"#,
            "clipboardjs",
        );
        p.sync();
        assert_eq!(
            js(&mut p, "out.join(';')"),
            "false;true;plain;false;<b>x</b>;x;true;true;false;2;1;255 0 0 255 0 255 0 255;{\"a\":[1,2]};tag;false;/a/b.txt|/c.png"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tray_and_badge_requests_reach_the_shell() {
        let (mut p, path) = page(
            r#"<body><menu id="t"><li id="a">One</li><li id="b" disabled>Two</li></menu><script>
                 Window.this.tray = { icon: "none.png", tooltip: "Tip", menu: document.getElementById("t"), template: true };
                 application.badge = 4;
               </script></body>"#,
            "tray",
        );
        p.sync();
        let info = p.tray().expect("tray");
        assert_eq!((info.tooltip.as_str(), info.template, info.items.len()), ("Tip", true, 2));
        assert_eq!(p.badge().as_deref(), Some("4"));
        assert_eq!(p.progress(), nile_script::Progress::None);
        js(&mut p, "application.progress = 0.25; 0");
        assert_eq!(p.progress(), nile_script::Progress::Value(0.25));
        js(&mut p, "application.progress = 7; 0");
        assert_eq!(p.progress(), nile_script::Progress::Value(1.0));
        js(&mut p, "application.progress = 'indeterminate'; 0");
        assert_eq!(p.progress(), nile_script::Progress::Indeterminate);
        js(&mut p, "application.progress = null; 0");
        assert_eq!(p.progress(), nile_script::Progress::None);
        js(&mut p, "application.badge = null; Window.this.tray = null; 0");
        assert!(p.tray().is_none() && p.badge().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn window_screens_describe_the_monitors() {
        let (mut p, path) = page("<body></body>", "screens");
        p.view.borrow_mut().screens = (
            vec![
                nile_script::ScreenInfo { name: "A".into(), x: 0, y: 0, width: 2880, height: 1800, scale: 2.0, primary: true },
                nile_script::ScreenInfo { name: "B".into(), x: 2880, y: 0, width: 1920, height: 1080, scale: 1.0, primary: false },
            ],
            1,
        );
        let out = js(
            &mut p,
            "[Window.screens, Window.this.screen, Window.screenBox(0, 'frame').join(), Window.screenBox(0, 'frame', 'dimension', false).join(), Window.screenBox(1, 'frame', 'right'), Window.this.screenBox('device'), Window.screenBox(0, 'isPrimary'), Window.screenBox(0, 'devicePixelRatio')].join('|')",
        );
        assert_eq!(out, "2|1|0,0,2880,1800|1440,900|4800|B|true|2");
        js(&mut p, "window.seen = 0; addEventListener('resolutionchange', () => seen++); 0");
        p.screen_changed();
        assert_eq!(js(&mut p, "window.seen"), "1");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_machine_state_queries_answer_off_the_window_thread() {
        let (mut p, path) = page(
            r#"<body><script type="module">
              import * as env from "@env";
              window.out = "";
              window.seen = [];
              addEventListener("online", () => seen.push("online"));
              Promise.all([env.online(), env.power(), env.idleSeconds().catch((e) => "none")]).then(([o, pw, idle]) => {
                window.out = typeof o + ":" + typeof pw.battery + ":" + typeof pw.charging + ":" + (idle === "none" || typeof idle === "number");
              });
            </script></body>"#,
            "machine-state",
        );
        for _ in 0..600 {
            p.tick();
            if !js(&mut p, "window.out").is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(js(&mut p, "window.out"), "boolean:boolean:boolean:true");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn global_hotkeys_are_registered_by_the_page_and_fire_its_handler() {
        let (mut p, path) = page(
            r#"<body><script>
                 window.log = [];
                 window.k1 = application.registerHotkey("Ctrl+Shift+K", () => log.push("k1"));
                 window.k2 = application.registerHotkey("Alt+F9", () => log.push("k2"));
                 k1.ready.then((ok) => log.push("ready:" + ok));
                 k2.ready.then((ok) => log.push("ready2:" + ok));
               </script></body>"#,
            "hotkeys",
        );
        p.sync();
        assert_eq!(p.hotkeys(), vec![(1, "Ctrl+Shift+K".to_string()), (2, "Alt+F9".to_string())]);
        p.hotkey_event(1, "ok");
        p.hotkey_event(2, "refused");
        p.hotkey_event(1, "pressed");
        p.hotkey_event(2, "pressed");
        js(&mut p, "k2.close(); 0");
        assert_eq!(p.hotkeys(), vec![(1, "Ctrl+Shift+K".to_string())]);
        p.hotkey_event(2, "pressed");
        p.run_timers();
        assert_eq!(js(&mut p, "log.join(',')"), "ready:true,ready2:false,k1,k2");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_in_window_menu_bar_runs_items_like_a_native_one() {
        let (mut p, path) = page(
            r#"<body><menu id="m" style="display:none"><li>File <menu><li id="a">Alpha</li><li id="b" disabled>Beta</li><hr><li id="q" role="quit">Quit</li></menu></li><li>Help</li></menu>
               <script>window.log = []; document.getElementById("a").addEventListener("click", () => log.push("a")); document.getElementById("b").addEventListener("click", () => log.push("b"));</script></body>"#,
            "menubar",
        );
        p.set_native_menu(false);
        js(&mut p, "Window.this.menu = document.getElementById('m'); 0");
        p.sync();
        assert_eq!(js(&mut p, "document.querySelectorAll('.nile-menubar-item').length"), "2");
        js(&mut p, "document.querySelector('.nile-menubar-item').click(); 0");
        assert_eq!(js(&mut p, "[...document.querySelectorAll('.nile-menu-row')].map(r => r.textContent).join('|')"), "Alpha|Beta|Quit");
        // A disabled row does nothing; an enabled one closes the menu and fires `click` on its <li>.
        js(&mut p, "document.querySelectorAll('.nile-menu-row')[1].click(); 0");
        p.sync();
        assert_eq!(js(&mut p, "log.join()"), "");
        js(&mut p, "document.querySelectorAll('.nile-menu-row')[0].click(); 0");
        p.sync();
        assert_eq!(js(&mut p, "log.join() + document.querySelectorAll('.nile-menu').length"), "a0");
        // A window role comes back as a window command.
        js(&mut p, "document.querySelector('.nile-menubar-item').click(); document.querySelectorAll('.nile-menu-row')[2].click(); 0");
        p.sync();
        assert!(p.view.borrow().window_requests.iter().any(|c| matches!(c, WindowCommand::Quit(0))));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn observers_and_location_follow_the_page() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="top" style="height:100px"></div><div style="height:600px"></div><div id="far" style="height:50px"></div>
               <script>
                 window.log = [];
                 new ResizeObserver(es => es.forEach(e => log.push('ro ' + e.target.id + ' ' + e.contentRect.height))).observe(document.getElementById('top'));
                 new IntersectionObserver(es => es.forEach(e => log.push('io ' + e.target.id + ' ' + e.isIntersecting))).observe(document.getElementById('far'));
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
        // location.
        assert!(js(&mut p, "location.href").starts_with("file://"));
        assert_eq!(js(&mut p, "location.pathname.endsWith('observers.html') + ',' + devicePixelRatio"), "true,1");
        // Nothing navigates: `location` is read-only and there is no `history`.
        js(&mut p, "location.hash = 'two'; location.href = 'x.html'; 0");
        assert_eq!(js(&mut p, "location.hash + typeof history"), "undefined");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn color_date_and_time_inputs_filter_step_and_normalize() {
        let (mut p, path) = page(
            r##"<body style="margin:0"><input id="c" type="color" style="margin:0"><input id="d" type="date" value="2024-01-31" style="margin:0">
               <input id="t" type="time" value="23:59" style="margin:0"></body>"##,
            "temporal",
        );
        let none = ModifiersState::default();
        let val = |p: &mut Page, id: &str| js(p, &format!("document.getElementById('{id}').value"));
        assert_eq!(val(&mut p, "c"), "#000000");
        click(&mut p, "c");
        press_key(&mut p, "a", ModifiersState::CONTROL);
        type_keys(&mut p, "#AbCz");
        press_key(&mut p, "Enter", none);
        assert_eq!(val(&mut p, "c"), "#aabbcc");
        press_key(&mut p, "a", ModifiersState::CONTROL);
        type_keys(&mut p, "#12");
        press_key(&mut p, "Tab", none); // invalid: back to the old colour
        assert_eq!(val(&mut p, "c"), "#aabbcc");
        // Dates step the field under the caret.
        click(&mut p, "d");
        press_key(&mut p, "Home", none);
        for _ in 0..5 {
            press_key(&mut p, "ArrowRight", none);
        }
        press_key(&mut p, "ArrowUp", none);
        assert_eq!(val(&mut p, "d"), "2024-02-29"); // month 1 -> 2, the day clamps
        type_keys(&mut p, "x");
        assert_eq!(val(&mut p, "d"), "2024-02-29");
        click(&mut p, "t");
        press_key(&mut p, "Home", none);
        press_key(&mut p, "ArrowUp", none);
        assert_eq!(val(&mut p, "t"), "00:59"); // hours wrap
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn file_inputs_ask_for_files_and_expose_them() {
        let dir = std::env::temp_dir().join(format!("nile-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.txt");
        std::fs::write(&file, "hello").unwrap();
        let (mut p, path) = page(r#"<body style="margin:0"><input id="f" type="file" accept=".txt" multiple>"#, "file_input");
        click(&mut p, "f");
        let request = p.take_file_request().expect("a click asks for a chooser");
        assert!(request.multiple && request.accept == ".txt");
        assert!(p.take_file_request().is_none());
        let named = file.to_string_lossy().to_string();
        assert!(p.choose_files_for_request(std::slice::from_ref(&named)));
        assert_eq!(js(&mut p, "document.getElementById('f').files.length + ' ' + document.getElementById('f').files[0].name + ' ' + document.getElementById('f').files[0].size"), "1 note.txt 5");
        assert_eq!(js(&mut p, "document.getElementById('f').value"), named);
        js(&mut p, "window.t = ''; document.getElementById('f').files[0].text().then(s => { window.t = s; }); 0");
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            p.tick();
            if js(&mut p, "window.t") == "hello" {
                break;
            }
        }
        assert_eq!(js(&mut p, "window.t"), "hello");
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn optgroups_are_headings_and_list_boxes_select_by_dragging() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><select id="m" multiple size="7" style="margin:0;width:160px">
                 <optgroup label="G1"><option>a</option><option>b</option><option>c</option></optgroup>
                 <optgroup label="G2"><option>d</option><option>e</option></optgroup>
               </select><select id="s" style="margin:0"><optgroup label="G"><option>x</option><option>y</option></optgroup></select></body>"#,
            "optgroup",
        );
        let picked = |p: &mut Page| js(p, "Array.from(document.getElementById('m').querySelectorAll('option')).filter(o => o.selected).map(o => o.text).join('')");
        // Rows: G1 a b c G2 d e. Press on "a" (row 1), drag down through G2's heading to "d" (row 5).
        let node = p.doc.borrow().find(|n| n.attr("id") == Some("m")).unwrap();
        let r = p.view.borrow_mut().layout().node_rect(node).unwrap();
        let (rh, inset) = {
            let mut v = p.view.borrow_mut();
            let l = v.layout().list_box(node).unwrap();
            (l.row_height, l.inset_top)
        };
        let row_y = |i: usize| r.y + inset + rh * (i as f32 + 0.5);
        p.pointer_moved(r.x + 20.0, row_y(0)); // the heading: not a choice
        p.press();
        p.release();
        assert_eq!(picked(&mut p), "");
        p.pointer_moved(r.x + 20.0, row_y(1));
        p.press();
        p.pointer_moved(r.x + 20.0, row_y(3));
        assert_eq!(picked(&mut p), "abc");
        p.pointer_moved(r.x + 20.0, row_y(5));
        assert_eq!(picked(&mut p), "abcd"); // G2's heading is skipped
        p.release();
        assert_eq!(picked(&mut p), "abcd");
        // The drop-down: the heading takes a row but can't be chosen, and arrows skip it.
        let sel = p.doc.borrow().find(|n| n.attr("id") == Some("s")).unwrap();
        assert_eq!(nile_layout::select_options(&p.doc.borrow(), sel).len(), 3);
        assert_eq!(nile_layout::selected_index(&p.doc.borrow(), sel), Some(1));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn input_methods_compose_in_inputs_and_editors() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><input id="i" value="ab" style="margin:0;width:200px">
               <div id="h" contenteditable style="margin:0;width:200px"><p id="para">xy</p></div></body>"#,
            "ime",
        );
        let none = ModifiersState::default();
        assert!(!p.ime_wanted());
        click(&mut p, "i");
        assert!(p.ime_wanted());
        press_key(&mut p, "Home", none);
        press_key(&mut p, "ArrowRight", none);
        // Composing shows in the field but isn't the value yet.
        assert!(p.ime_preedit("ni", Some((0, 2))));
        assert_eq!(js(&mut p, "document.getElementById('i').value"), "ab");
        let shown = p.view.borrow_mut().layout().boxes.iter().find_map(|b| match &b.content {
            nile_layout::BoxContent::Text(c) if c.control => Some(c.text.clone()),
            _ => None,
        });
        assert_eq!(shown.as_deref(), Some("anib"));
        let (x0, ..) = p.ime_cursor_rect().expect("a caret to put the candidate window at");
        assert!(p.ime_underline().is_some());
        assert!(p.ime_preedit("nin", Some((0, 3))));
        assert!(p.ime_cursor_rect().unwrap().0 > x0, "the caret follows the composition");
        // Commit replaces the composition with the final text, and undo takes it back as one edit.
        assert!(p.ime_commit("你好"));
        assert_eq!(js(&mut p, "document.getElementById('i').value"), "a你好b");
        assert!(p.ime_underline().is_none());
        press_key(&mut p, "z", ModifiersState::CONTROL);
        assert_eq!(js(&mut p, "document.getElementById('i').value"), "ab");
        // Losing focus abandons a composition.
        click(&mut p, "i");
        p.ime_preedit("x", None);
        click(&mut p, "h");
        assert!(p.ime_underline().is_none());
        assert_eq!(js(&mut p, "document.getElementById('i').value"), "ab");

        // In an editing host the composition is an underlined span that commit replaces.
        click(&mut p, "para");
        press_key(&mut p, "End", none);
        assert!(p.ime_preedit("ka", None));
        assert_eq!(js(&mut p, "document.getElementById('para').innerHTML"), r#"xy<span data-nile-ime="" style="text-decoration: underline">ka</span>"#);
        assert!(p.ime_commit("か"));
        assert_eq!(js(&mut p, "document.getElementById('para').textContent"), "xyか");
        assert_eq!(js(&mut p, "document.getElementById('para').querySelectorAll('span').length"), "0");
        p.ime_preedit("zz", None);
        p.ime_cancel();
        assert_eq!(js(&mut p, "document.getElementById('para').textContent"), "xyか");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rich_copy_paste_and_range_extras() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="h" contenteditable><p id="a">one <b>two</b> three</p><p id="b">tail</p></div></body>"#,
            "rich_paste",
        );
        let html = |p: &mut Page| js(p, "document.getElementById('h').innerHTML");
        let ctrl = ModifiersState::CONTROL;
        // Range extras on the unedited document.
        assert_eq!(
            js(&mut p, "(() => { const t = document.getElementById('a'); const r = document.createRange(); r.setStart(t.firstChild, 2); r.setEnd(t.querySelector('b').firstChild, 2); return r.cloneContents().innerHTML + '|' + t.innerHTML; })()"),
            "e <b>tw</b>|one <b>two</b> three"
        );
        assert_eq!(
            js(&mut p, "(() => { const t = document.getElementById('a'); const r = document.createRange(), s = document.createRange(); r.setStart(t.firstChild, 1); r.setEnd(t.firstChild, 3); s.setStart(t.firstChild, 2); s.setEnd(t.firstChild, 4); return [r.compareBoundaryPoints(Range.START_TO_START, s), r.compareBoundaryPoints(Range.END_TO_END, s), r.comparePoint(t.firstChild, 2), r.comparePoint(t.firstChild, 9), r.isPointInRange(t.firstChild, 0)].join(); })()"),
            "-1,-1,0,1,false"
        );
        // Copy "two three" (spanning the bold run) and paste it at the start of the second paragraph.
        click_end_of(&mut p, "a");
        for _ in 0..9 {
            press_key(&mut p, "ArrowLeft", ModifiersState::SHIFT);
        }
        press_key(&mut p, "c", ctrl);
        let copied = p.clipboard.html().unwrap_or_default();
        assert!(copied.contains("<b>two</b>"), "{copied}");
        assert_eq!(p.clipboard.text().as_deref(), Some("two three"));
        click_end_of(&mut p, "b");
        press_key(&mut p, "v", ctrl);
        assert_eq!(js(&mut p, "document.getElementById('b').innerHTML"), "tail<b>two</b> three");
        // HTML from elsewhere: blocks stay blocks, scripts and unknown wrappers go, styles are filtered.
        p.clipboard.set_html(
            r#"<meta charset="utf-8"><div style="color: red; position: fixed"><strong>Hello</strong></div><script>evil()</script><div><section>World <em>wide</em> <a href="javascript:x()">link</a></section></div>"#,
            "Hello World wide link",
        );
        click_end_of(&mut p, "a");
        press_key(&mut p, "v", ctrl);
        let out = html(&mut p);
        assert!(out.contains(r#"<p style="color: red"><b>Hello</b></p>"#) || out.contains("<b>Hello</b>"), "{out}");
        assert!(!out.contains("script") && !out.contains("evil") && !out.contains("javascript") && !out.contains("fixed"), "{out}");
        assert!(out.contains("World <i>wide</i>"), "{out}");
        // Undo takes the whole paste back.
        press_key(&mut p, "z", ctrl);
        assert_eq!(js(&mut p, "document.getElementById('a').textContent"), "one two three");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tables_in_the_editor_insert_navigate_and_keep_their_shape() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="h" contenteditable><p id="a">before</p></div><input id="after"></body>"#,
            "editor_table",
        );
        let none = ModifiersState::default();
        click_end_of(&mut p, "a");
        js(&mut p, "document.execCommand('insertTable', false, '2x3'); 0");
        assert_eq!(js(&mut p, "document.querySelectorAll('#h table tr').length + 'x' + document.querySelectorAll('#h td').length"), "2x6");
        assert_eq!(js(&mut p, "document.getElementById('h').lastElementChild.tagName"), "P", "a paragraph follows the table");
        type_keys(&mut p, "A");
        press_key(&mut p, "Tab", none);
        type_keys(&mut p, "B");
        press_key(&mut p, "Tab", none);
        press_key(&mut p, "Tab", none);
        type_keys(&mut p, "D");
        assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('#h td')).map(c => c.textContent).join('|')"), "A|B||D||");
        // Enter stays inside the cell; Backspace at a cell's start doesn't eat the previous cell.
        press_key(&mut p, "Enter", none);
        assert_eq!(js(&mut p, "document.querySelectorAll('#h td').length"), "6");
        press_key(&mut p, "Backspace", none);
        press_key(&mut p, "Backspace", none);
        press_key(&mut p, "Backspace", none);
        assert_eq!(js(&mut p, "document.querySelectorAll('#h td').length + ':' + document.querySelectorAll('#h td')[3].textContent"), "6:");
        // Shift+Tab goes back; Tab from the last cell adds a row.
        press_key(&mut p, "Tab", ModifiersState::SHIFT);
        type_keys(&mut p, "C");
        assert_eq!(js(&mut p, "document.querySelectorAll('#h td')[2].textContent"), "C");
        for _ in 0..4 {
            press_key(&mut p, "Tab", none);
        }
        assert_eq!(js(&mut p, "document.querySelectorAll('#h tr').length"), "3");
        // Select across cells and delete: contents go, cells stay.
        js(&mut p, "const tds = document.querySelectorAll('#h td'); const r = document.createRange(); r.setStart(tds[0], 0); r.setEnd(tds[4], 0); const s = getSelection(); s.removeAllRanges(); s.addRange(r); 0");
        press_key(&mut p, "Backspace", none);
        assert_eq!(js(&mut p, "document.querySelectorAll('#h td').length + ':' + Array.from(document.querySelectorAll('#h td')).map(c => c.textContent).join('')"), "9:");
        // Selecting everything and deleting takes the whole table with it.
        press_key(&mut p, "a", ModifiersState::CONTROL);
        press_key(&mut p, "Backspace", none);
        assert_eq!(js(&mut p, "document.querySelectorAll('#h table').length"), "0");
        let _ = std::fs::remove_file(path);
    }

    /// `cargo test --release -p nile-engine bench_hover -- --ignored --nocapture`: pointer moves over the 1,500-row page.
    #[test]
    #[ignore]
    fn bench_hover() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/stress.html");
        let mut p = Page::load(file, (900.0, 640.0), 0.0);
        p.layout(900.0, 640.0);
        p.pointer_moved(100.0, 100.0);
        let t = Instant::now();
        let n = 100;
        for i in 0..n {
            p.pointer_moved(100.0, 60.0 + (i % 20) as f32 * 14.0);
            p.sync();
            p.view.borrow_mut().layout();
        }
        eprintln!("hover move: {:.3} ms each", t.elapsed().as_secs_f64() * 1e3 / n as f64);
    }

    #[test]
    #[ignore]
    fn bench_splitter_drag() {
        let file = std::env::var("NILE_BENCH_FILE").map(PathBuf::from).unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/splitter.html"));
        let (w, h) = (1000u32, 760u32);
        let mut p = Page::load(file, (w as f32, h as f32), 0.0);
        p.layout(w as f32, h as f32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for _ in 0..3 {
            p.run_timers();
            p.paint(&mut px, w, h, 1.0);
        }
        let v = p.eval("(() => { const r = document.querySelector('.nile-splitter-grip').getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2].join(); })()").0.unwrap();
        let mut it = v.split(',').map(|n| n.parse::<f32>().unwrap());
        let (x, y) = (it.next().unwrap(), it.next().unwrap());
        p.pointer_moved(x, y);
        p.press();
        let n = 80;
        let (mut total, mut worst) = (0.0f64, 0.0f64);
        let mut frames = Vec::new();
        for i in 1..=n {
            let t = Instant::now();
            p.pointer_moved(x + (i as f32) * 3.0, y);
            p.run_timers();
            p.sync();
            p.paint(&mut px, w, h, 1.0);
            let ms = t.elapsed().as_secs_f64() * 1e3;
            total += ms;
            worst = worst.max(ms);
            frames.push(ms);
        }
        p.release();
        eprintln!("splitter drag (3px per move): {:.2} ms/frame avg, {:.1} ms worst", total / n as f64, worst);
        eprintln!("  frames: {}", frames.iter().map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" "));
    }

    #[test]
    #[ignore]
    fn bench_grid_resize() {
        let file = std::env::var("NILE_BENCH_FILE").map(PathBuf::from).unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data-grid.html"));
        let (w, h) = (1000u32, 720u32);
        let mut p = Page::load(file, (w as f32, h as f32), 0.0);
        p.layout(w as f32, h as f32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for _ in 0..3 {
            p.run_timers();
            p.paint(&mut px, w, h, 1.0);
        }
        // The last grid on the page (the big catalogue): its second column's drag handle.
        let v = p.eval("(() => { const g = Array.from(document.querySelectorAll('nile-grid')).pop(); const r = g.querySelectorAll('.nile-grid-resizer')[1].getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2].join(); })()").0.unwrap();
        let mut it = v.split(',').map(|n| n.parse::<f32>().unwrap());
        let (x, y) = (it.next().unwrap(), it.next().unwrap());
        p.pointer_moved(x, y);
        p.press();
        let n = 80;
        let (mut total, mut worst) = (0.0f64, 0.0f64);
        let mut frames = Vec::new();
        for i in 1..=n {
            let t = Instant::now();
            p.pointer_moved(x + (i as f32) * 2.0, y);
            std::thread::sleep(Duration::from_millis(2));
            p.run_timers();
            p.sync();
            p.paint(&mut px, w, h, 1.0);
            let ms = t.elapsed().as_secs_f64() * 1e3 - 2.0;
            total += ms;
            worst = worst.max(ms);
            frames.push(ms);
        }
        p.release();
        eprintln!("grid column resize (2px per move): {:.2} ms/frame avg, {:.1} ms worst", total / n as f64, worst);
        eprintln!("  frames: {}", frames.iter().map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" "));
    }

    #[test]
    #[ignore]
    fn bench_grid_scroll() {
        let file = std::env::var("NILE_BENCH_FILE").map(PathBuf::from).unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data-grid.html"));
        let (w, h) = (1000u32, 720u32);
        let mut p = Page::load(file, (w as f32, h as f32), 0.0);
        p.layout(w as f32, h as f32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for _ in 0..3 {
            p.run_timers();
            p.paint(&mut px, w, h, 1.0);
        }
        p.pointer_moved(700.0, 300.0);
        for step in [18.0f32, 60.0, 120.0] {
            let mut rows = Vec::new();
            let (mut js_ms, mut sync_ms, mut paint_ms, mut worst) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            let n = 40;
            for _ in 0..n {
                let t = Instant::now();
                p.wheel(0.0, -step);
                let t_js = Instant::now();
                std::thread::sleep(Duration::from_millis(2));
                p.run_timers(); // the grid's requestAnimationFrame work
                let a = t_js.elapsed().as_secs_f64() * 1e3 - 2.0;
                let t_sync = Instant::now();
                p.sync();
                let _ = p.animate();
                let b = t_sync.elapsed().as_secs_f64() * 1e3;
                let t_paint = Instant::now();
                p.paint(&mut px, w, h, 1.0);
                let c = t_paint.elapsed().as_secs_f64() * 1e3;
                let total = t.elapsed().as_secs_f64() * 1e3 - 2.0;
                js_ms += a;
                sync_ms += b;
                paint_ms += c;
                worst = worst.max(total);
                rows.push(total);
            }
            eprintln!(
                "wheel {step:>5}px: scripts {:.2} ms, relayout {:.2} ms, paint {:.2} ms (avg), worst frame {:.1} ms",
                js_ms / n as f64,
                sync_ms / n as f64,
                paint_ms / n as f64,
                worst
            );
            eprintln!("  frames: {}", rows.iter().map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" "));
        }
    }

    #[test]
    #[ignore]
    fn bench_scroll_paint() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/word-ribbon.html");
        let (w, h) = (1100u32, 760u32);
        let mut p = Page::load(file, (w as f32, h as f32), 0.0);
        p.layout(w as f32, h as f32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for _ in 0..3 {
            p.paint(&mut px, w, h, 1.0);
        }
        let n = 60;
        let t = Instant::now();
        for i in 0..n {
            p.view.borrow_mut().scroll_y = (i * 7) as f32;
            p.paint(&mut px, w, h, 1.0);
        }
        eprintln!("paint at 1x: {:.2} ms/frame", t.elapsed().as_secs_f64() * 1e3 / n as f64);
        let (w2, h2) = (w * 2, h * 2);
        let mut px2 = vec![0u8; (w2 * h2 * 4) as usize];
        p.paint(&mut px2, w2, h2, 2.0);
        let t = Instant::now();
        for i in 0..n {
            p.view.borrow_mut().scroll_y = (i * 7) as f32;
            p.paint(&mut px2, w2, h2, 2.0);
        }
        eprintln!("paint at 2x: {:.2} ms/frame", t.elapsed().as_secs_f64() * 1e3 / n as f64);
        // The real sequence: wheel events with the pointer resting on the page, then the frame.
        p.view.borrow_mut().scroll_y = 0.0;
        p.pointer_moved(500.0, 400.0);
        let mut worst = 0.0f64;
        let mut total = 0.0f64;
        let mut all = Vec::new();
        for _ in 0..40 {
            let t = Instant::now();
            p.wheel(0.0, -18.0);
            p.sync();
            let _ = p.animate();
            p.paint(&mut px, w, h, 1.0);
            let ms = t.elapsed().as_secs_f64() * 1e3;
            worst = worst.max(ms);
            total += ms;
            all.push(ms);
        }
        eprintln!("wheel+sync+paint: {:.2} ms avg, {:.2} ms worst", total / 40.0, worst);
        eprintln!("frames: {}", all.iter().map(|m| format!("{m:.1}")).collect::<Vec<_>>().join(" "));
    }

    #[test]
    fn frames_host_documents_route_input_and_talk_to_scripts() {
        let dir = std::env::temp_dir().join(format!("nile-frames-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("child.html"),
            r#"<body style="margin:0"><input id="t"><button id="b" style="display:block">Go</button><p id="o"></p>
               <div style="height:600px"></div>
               <script>
                 let n = 0;
                 document.$('#b').on('click', () => { n++; document.$('#o').text = n; });
                 window.addEventListener('message', (e) => window.parent.postMessage({ echo: e.data.say, same: window.parent !== window }));
               </script></body>"#,
        )
        .unwrap();
        let main = dir.join("main.html");
        std::fs::write(
            &main,
            r#"<body style="margin:0"><p style="margin:0;height:40px">top</p><frame id="f" src="child.html" style="height:200px"></frame>
               <script>
                 globalThis.seen = [];
                 const f = document.$('#f');
                 f.on('newdocument', () => seen.push('new'));
                 f.on('complete', () => seen.push('complete'));
                 window.addEventListener('message', (e) => seen.push('msg:' + e.data.echo + ':' + e.data.same + ':' + (e.source === f)));
               </script></body>"#,
        )
        .unwrap();
        let mut p = Page::load(main, (600.0, 300.0), 0.0);
        p.layout(600.0, 300.0);
        p.update_frames();
        let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
        assert_eq!(ev(&mut p, "seen.join()"), "new,complete");
        assert_eq!(ev(&mut p, "f.document.$('#b').text"), "Go");

        // A click inside the frame reaches the frame's own script, and the frame takes keyboard focus.
        let id = p.doc.borrow().find(|n| n.attr("id") == Some("f")).unwrap();
        let (fx, fy, ..) = p.frames.borrow().slots[&id].rect;
        assert_eq!((fx, fy), (0.0, 40.0));
        let button = {
            let fr = p.frames.borrow();
            let child = &fr.slots[&id].page;
            let node = child.doc.borrow().find(|n| n.attr("id") == Some("b")).unwrap();
            let r = child.view.borrow_mut().layout().node_rect(node).unwrap();
            (fx + r.x + r.width / 2.0, fy + r.y + r.height / 2.0)
        };
        p.pointer_moved(button.0, button.1);
        p.press();
        p.release();
        assert_eq!(ev(&mut p, "f.document.$('#o').text"), "1");
        assert_eq!(p.focused_frame(), Some(id));

        // Typing goes to the focused frame's input.
        let input = {
            let fr = p.frames.borrow();
            let child = &fr.slots[&id].page;
            let node = child.doc.borrow().find(|n| n.attr("id") == Some("t")).unwrap();
            let r = child.view.borrow_mut().layout().node_rect(node).unwrap();
            (fx + r.x + 4.0, fy + r.y + 4.0)
        };
        p.pointer_moved(input.0, input.1);
        p.press();
        p.release();
        for k in ["h", "i"] {
            let (proceed, _) = p.key("keydown", k.to_string(), ModifiersState::default());
            if proceed {
                p.default_key(k, ModifiersState::default());
            }
        }
        assert_eq!(ev(&mut p, "f.document.$('#t').value"), "hi");

        // The wheel scrolls the frame, not the outer page.
        p.wheel(0.0, -50.0);
        assert_eq!(p.view.borrow().scroll_y, 0.0);
        assert!(p.frames.borrow().slots[&id].page.view.borrow().scroll_y > 0.0);

        // Messages go both ways.
        ev(&mut p, "f.postMessage({ say: 'yo' })");
        p.sync();
        assert_eq!(ev(&mut p, "seen.slice(2).join()"), "msg:yo:true:true");

        // loadHtml and loadFile replace the document and fire the events again.
        ev(&mut p, "f.loadHtml('<p id=z>hello</p>')");
        p.update_frames();
        assert_eq!(ev(&mut p, "f.document.$('#z').text"), "hello");
        assert_eq!(ev(&mut p, "seen.slice(3).join()"), "new,complete");
        ev(&mut p, "f.loadFile('child.html')");
        p.update_frames();
        assert_eq!(ev(&mut p, "f.document.$('#b').text"), "Go");
        assert_eq!(ev(&mut p, "f.document.$('#nope') === null"), "true");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn framesets_split_panes_and_splitters_resize_them() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><frameset id="fs" cols="100px,*,50px" style="height:200px">
                 <div id="a">a</div><splitter id="s1"/><div id="b">b</div><splitter id="s2"></splitter><div id="c">c</div>
               </frameset></body>"#,
            "frameset",
        );
        let rect = |p: &mut Page, id: &str| {
            let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
            let r = p.view.borrow_mut().layout().node_rect(n).unwrap();
            (r.x, r.width)
        };
        assert_eq!(rect(&mut p, "a"), (0.0, 100.0));
        assert_eq!(rect(&mut p, "s1"), (100.0, 6.0));
        assert_eq!(rect(&mut p, "b").0, 106.0);
        assert_eq!(rect(&mut p, "c"), (550.0, 50.0));
        // Dragging the first splitter 40px right widens the first pane and leaves the flexible one to absorb it.
        p.pointer_moved(103.0, 50.0);
        p.press();
        p.pointer_moved(143.0, 50.0);
        p.release();
        assert_eq!(rect(&mut p, "a"), (0.0, 140.0));
        assert_eq!(p.eval("document.$('#fs').getAttribute('cols')").0.unwrap(), "140px,*,50px");
        assert_eq!(p.eval("document.$('#fs').frameset.state.map(Math.round).join()").0.unwrap(), "140,398,50");
        p.eval("document.$('#fs').frameset.state = [60, 300, 80]").0.unwrap();
        assert_eq!(rect(&mut p, "a"), (0.0, 60.0));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn scripts_open_native_dialogs() {
        use dialogs::{Dialogs, FileFilter, MessageKind};
        #[derive(Default)]
        struct Fake(RefCell<Vec<String>>);
        impl Dialogs for Fake {
            fn open_files(&self, multiple: bool, filters: &[FileFilter], caption: &str, dir: &str) -> Vec<String> {
                self.0.borrow_mut().push(format!("open {multiple} {:?} {caption} {dir}", filters.iter().map(|f| f.extensions.clone()).collect::<Vec<_>>()));
                vec!["/tmp/a b.txt".into(), "/tmp/c.txt".into()]
            }
            fn save_file(&self, _: &[FileFilter], extension: &str, caption: &str, path: &str) -> Option<String> {
                self.0.borrow_mut().push(format!("save {extension} {caption} {path}"));
                None
            }
            fn pick_folder(&self, caption: &str, _: &str) -> Option<String> {
                self.0.borrow_mut().push(format!("folder {caption}"));
                Some("/tmp/dir".into())
            }
            fn message(&self, kind: MessageKind, caption: &str, text: &str) -> bool {
                self.0.borrow_mut().push(format!("message {kind:?} {caption} {text}"));
                kind != MessageKind::Question
            }
        }
        let (mut p, path) = page(r#"<body><script>
              globalThis.filter = 'Text (*.txt)|*.txt;*.text|All (*.*)|*.*';
            </script></body>"#, "dialogs");
        // Without an embedder dialog host everything is cancelled.
        assert_eq!(p.eval("Window.this.selectFile({}) === null && Window.this.selectFolder() === null").0.unwrap(), "true");
        let fake = Rc::new(Fake::default());
        p.set_dialogs(fake.clone());
        assert_eq!(p.eval("JSON.stringify(Window.this.selectFile({ mode: 'open-multiple', filter, caption: 'Pick', path: '/tmp' }))").0.unwrap(), r#"["file:///tmp/a%20b.txt","file:///tmp/c.txt"]"#);
        assert_eq!(p.eval("Window.this.selectFile({ mode: 'save', extension: 'txt', caption: 'Save' })").0.unwrap(), "null");
        assert_eq!(p.eval("Window.this.selectFolder({ caption: 'Dir' })").0.unwrap(), "file:///tmp/dir");
        assert_eq!(p.eval("Window.this.modal(JSX('question', null, ['Sure?']))").0.unwrap(), "no");
        assert_eq!(p.eval("Window.this.modal(JSX('alert', { caption: 'Hey' }, ['Careful ', 'now']))").0.unwrap(), "yes");
        assert_eq!(
            *fake.0.borrow(),
            [
                "open true [[\"txt\", \"text\"], []] Pick /tmp",
                "save txt Save ",
                "folder Dir",
                "message Question  Sure?",
                "message Alert Hey Careful now",
            ]
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn default_dialogs_are_there_for_the_first_scripts() {
        use dialogs::{Dialogs, FileFilter, MessageKind};
        struct Count(Rc<std::cell::Cell<u32>>);
        impl Dialogs for Count {
            fn open_files(&self, _: bool, _: &[FileFilter], _: &str, _: &str) -> Vec<String> {
                Vec::new()
            }
            fn save_file(&self, _: &[FileFilter], _: &str, _: &str, _: &str) -> Option<String> {
                None
            }
            fn pick_folder(&self, _: &str, _: &str) -> Option<String> {
                None
            }
            fn message(&self, _: MessageKind, _: &str, _: &str) -> bool {
                self.0.set(self.0.get() + 1);
                true
            }
        }
        let shown = Rc::new(std::cell::Cell::new(0));
        Page::set_defaults(Some(Rc::new(Count(shown.clone()))), None);
        let (_p, path) = page(
            r#"<body><script>
              document.addEventListener('DOMContentLoaded', () => { Window.this.modal(JSX('alert', null, ['Hello'])); });
            </script></body>"#,
            "default-dialogs",
        );
        Page::set_defaults(None, None);
        assert_eq!(shown.get(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn popups_open_beside_their_anchor_and_close_on_outside_clicks() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><button id="b" style="position:absolute;left:100px;top:50px;width:80px;height:20px">go</button>
               <div id="elsewhere" style="position:absolute;left:400px;top:200px;width:50px;height:50px"></div>
               <menu.context id="m"><li id="one">One</li><li id="two">Two</li></menu>
               <script>
                 globalThis.seen = [];
                 const b = document.$('#b');
                 b.on('popup-request', (e) => seen.push('request:' + e.source.id));
                 b.on('popup-dismissed', () => seen.push('dismissed'));
                 document.on('popup-ready', (e) => seen.push('ready'));
                 document.on('popup-dismissing', (e) => seen.push('dismissing:' + e.target.id));
                 document.on('click', 'li', (e, li) => { seen.push('picked:' + li.id); });
                 globalThis.win = null;
               </script></body>"#,
            "popups",
        );
        let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
        // The menu is hidden until it is popped up (menu.context is the class shorthand).
        assert_eq!(ev(&mut p, "document.$('#m').getBoundingClientRect().height"), "0");
        ev(&mut p, "win = b.popup(document.$('#m'))");
        // Below the anchor's bottom-left corner.
        assert_eq!(ev(&mut p, "const ar = b.getBoundingClientRect(); JSON.stringify(['left','top'].map(k => Math.round(document.$('#m').getBoundingClientRect()[k]) - Math.round(k === 'top' ? ar.bottom : ar.left)))"), "[0,0]");
        assert_eq!(ev(&mut p, "win.isValid"), "true");
        // Clicking an item runs its handler and closes the menu.
        click(&mut p, "two");
        p.tick();
        assert_eq!(ev(&mut p, "seen.slice(0, 3).join()"), "request:m,ready,picked:two");
        assert_eq!(ev(&mut p, "win.isValid"), "false");
        assert_eq!(ev(&mut p, "seen.slice(3).join()"), "dismissing:m,dismissed");
        // Pressing outside closes it too.
        ev(&mut p, "win = b.popup(document.$('#m'), { anchorAt: 3, popupAt: 9 })");
        assert_eq!(ev(&mut p, "Math.round(document.$('#m').getBoundingClientRect().right - ar.right) + ',' + Math.round(document.$('#m').getBoundingClientRect().top - ar.bottom)"), "0,0");
        click(&mut p, "elsewhere");
        assert_eq!(ev(&mut p, "win.isValid"), "false");
        // So does a press on bare canvas, where no element is under the pointer.
        ev(&mut p, "win = b.popup(document.$('#m'))");
        p.pointer_moved(590.0, 290.0);
        p.press();
        p.release();
        assert_eq!(ev(&mut p, "win.isValid"), "false");
        // Airborne elements sit at the given window position until taken back.
        ev(&mut p, "const o = document.$('#elsewhere').takeOff({ x: 10, y: 20, width: 33, relativeTo: 'window' })");
        assert_eq!(ev(&mut p, "JSON.stringify(['left','top','width'].map(k => Math.round(document.$('#elsewhere').getBoundingClientRect()[k])))"), "[10,20,33]");
        ev(&mut p, "document.$('#elsewhere').takeOff()");
        assert_eq!(ev(&mut p, "Math.round(document.$('#elsewhere').getBoundingClientRect().left)"), "400");
        // A right click hands scripts `contextmenu`; `evt.source` is shown as a menu at the pointer.
        ev(&mut p, "document.$('#elsewhere').on('contextmenu', (e) => { e.source = document.$('#m'); })");
        let (x, y) = center(&p, "elsewhere");
        p.pointer_moved(x, y);
        p.context_menu();
        assert_eq!(ev(&mut p, "JSON.stringify(['left','top'].map(k => Math.round(document.$('#m').getBoundingClientRect()[k])))"), format!("[{},{}]", x as i32, y as i32));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn example_popup_closes_on_outside_clicks() {
        let mut p = Page::load(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/popups.html")), (900.0, 640.0), 0.0);
        p.layout(900.0, 640.0);
        let open = |p: &mut Page| p.eval("document.$$('menu.popup, popup').length").0.unwrap();
        assert_eq!(open(&mut p), "0");
        p.pointer_moved(40.0, 88.0);
        p.press();
        assert!(p.release(), "opening a popup asks for a repaint");
        assert_eq!(open(&mut p), "1", "the File menu opened");
        for (x, y) in [(500.0, 400.0), (300.0, 170.0), (850.0, 20.0)] {
            p.pointer_moved(x, y);
            p.press();
            p.release();
            assert_eq!(open(&mut p), "0", "closed by a press at {x},{y}");
            p.pointer_moved(40.0, 88.0);
            p.press();
            p.release();
            assert_eq!(open(&mut p), "1");
        }
        // The pixels agree: the menu is painted while open and gone after the press.
        let pixel = |p: &mut Page| {
            let mut buf = vec![0u8; 900 * 640 * 4];
            p.paint(&mut buf, 900, 640, 1.0);
            buf
        };
        p.pointer_moved(500.0, 400.0);
        p.press();
        p.release();
        let closed = pixel(&mut p);
        p.pointer_moved(40.0, 88.0);
        p.press();
        p.release();
        let opened = pixel(&mut p);
        assert_ne!(closed, opened, "the menu is painted");
        p.pointer_moved(500.0, 400.0);
        p.press();
        p.release();
        assert_eq!(pixel(&mut p), closed, "and gone again");
        // Clicking the button that opened a menu closes it (and doesn't open it again).
        p.pointer_moved(40.0, 88.0);
        p.press();
        p.release();
        assert_eq!(open(&mut p), "1");
        p.press();
        p.release();
        assert_eq!(open(&mut p), "0", "a second click on the button closes the menu");
        p.press();
        p.release();
        assert_eq!(open(&mut p), "1");
        p.pointer_moved(500.0, 400.0);
        p.press();
        p.release();
        // The right-click menu closes the same way.
        p.pointer_moved(500.0, 400.0);
        p.press();
        p.release();
        assert_eq!(open(&mut p), "0");
        p.pointer_moved(300.0, 170.0);
        p.context_menu();
        let shown = || p.eval("Math.round(document.$('#ctx').getBoundingClientRect().height) > 0").0.unwrap();
        let _ = shown;
        assert_eq!(p.eval("Math.round(document.$('#ctx').getBoundingClientRect().height) > 0").0.unwrap(), "true");
        p.pointer_moved(500.0, 400.0);
        let redraw = p.press() | p.release();
        assert!(redraw, "closing a popup asks for a repaint");
        assert_eq!(p.eval("Math.round(document.$('#ctx').getBoundingClientRect().height) > 0").0.unwrap(), "false");
    }

    #[test]
    fn inline_message_boxes_are_html_and_resolve_a_promise() {
        let (mut p, path) = page(r#"<body style="margin:0"><script>globalThis.answers = [];</script></body>"#, "inlinebox");
        let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
        // The default stays the system's box (here no host, so the fallback answers).
        assert_eq!(ev(&mut p, "Window.this.messageBoxes"), "native");
        assert_eq!(ev(&mut p, "Window.this.modal(JSX('question', null, ['Sure?']))"), "no");
        // An `inline` attribute opts one box in; buttons and rich content come from the markup.
        ev(&mut p, "Window.this.modal(JSX('question', { inline: true, caption: 'Quit' }, [JSX('b', null, ['Save']), ' first?', JSX('button', { value: 'save', default: true }, ['Save']), JSX('button', { id: 'discard' }, [\"Don't save\"]), JSX('button', { value: 'cancel', cancel: true }, ['Cancel'])])).then((v) => answers.push(v))");
        assert_eq!(ev(&mut p, "document.$$('.nile-messagebox button').map((b) => b.textContent).join('|')"), "Save|Don't save|Cancel");
        assert_eq!(ev(&mut p, "document.$('.nile-messagebox-title').textContent + ':' + document.$('.nile-messagebox-text b').textContent"), "Quit:Save");
        assert_eq!(ev(&mut p, "document.$('.nile-messagebox.question') !== null && document.activeElement.textContent"), "Save");
        // The box covers the window and is gone once an answer is given.
        assert_eq!(ev(&mut p, "JSON.stringify(document.$('.nile-modal').getBoundingClientRect().width)"), "600");
        let (x, y) = {
            let n = p.eval("document.$$('.nile-messagebox button')[1]._id").0.unwrap().parse::<usize>().unwrap();
            let r = p.view.borrow_mut().layout().node_rect(NodeId(n)).unwrap();
            (r.x + r.width / 2.0, r.y + r.height / 2.0)
        };
        p.pointer_moved(x, y);
        p.press();
        p.release();
        assert_eq!(ev(&mut p, "answers.join()"), "discard");
        assert_eq!(ev(&mut p, "document.$('.nile-modal')"), "null");
        // The global setting, the default buttons and Escape.
        ev(&mut p, "Window.this.messageBoxes = 'inline'; Window.this.modal(JSX('question', null, ['Delete?'])).then((v) => answers.push(v))");
        assert_eq!(ev(&mut p, "document.$$('.nile-messagebox button').map((b) => b.textContent).join('|')"), "No|Yes");
        p.key("keydown", "Escape".to_string(), ModifiersState::default());
        assert_eq!(ev(&mut p, "answers.slice(1).join()"), "no");
        ev(&mut p, "Window.this.modal(JSX('info', null, ['Done'])).then((v) => answers.push(v))");
        // Enter on the focused default button clicks it, as for any button.
        let (proceed, _) = p.key("keydown", "Enter".to_string(), ModifiersState::default());
        if proceed {
            p.default_key("Enter", ModifiersState::default());
        }
        assert_eq!(ev(&mut p, "answers.slice(2).join()"), "yes");
        // `native` wins over the global setting.
        assert_eq!(ev(&mut p, "Window.this.modal(JSX('question', { native: true }, ['x']))"), "no");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tooltips_follow_title_and_tooltip_attributes() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="a" title="Plain tip" style="width:100px;height:40px"></div>
               <div id="b" tooltip="<b>Rich</b> tip" style="width:100px;height:40px"></div>
               <div id="c" style="width:100px;height:40px"><span id="d" title="Inherited from a child"></span>x</div>
               <div id="e" style="width:100px;height:40px"></div>
               <div id="f" title="Never shown" style="width:100px;height:40px"></div>
               <div id="custom" style="width:100px;height:40px"></div>
               <script>
                 document.$('#custom').on('tooltiprequest', (e) => { const t = document.createElement('div'); t.id = 'mine'; t.style.display = 'none'; t.textContent = 'dynamic'; document.body.appendChild(t); e.source = t; });
               </script></body>"#,
            "tooltips",
        );
        let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
        let tip = |p: &mut Page| ev(p, "document.$('popup[role=tooltip]') ? document.$('popup[role=tooltip]').innerHTML : ''");
        let rest = |p: &mut Page, id: &str| {
            let (x, y) = center(p, id);
            p.pointer_moved(x, y);
            assert_eq!(tip(p), "", "nothing shows before the pointer has rested");
            std::thread::sleep(TOOLTIP_DELAY + Duration::from_millis(40));
            p.tick();
        };
        rest(&mut p, "a");
        assert_eq!(tip(&mut p), "Plain tip");
        // Near the pointer, not covering it.
        assert_eq!(ev(&mut p, "document.$('popup[role=tooltip]').getBoundingClientRect().top > 20"), "true");
        // Moving to another element replaces it; `tooltip` is HTML.
        rest(&mut p, "b");
        assert_eq!(tip(&mut p), "<b>Rich</b> tip");
        // A press or a key hides it.
        p.press();
        p.release();
        assert_eq!(tip(&mut p), "");
        // `title` on the pointer's own element or an ancestor; none: nothing.
        rest(&mut p, "e");
        assert_eq!(tip(&mut p), "");
        // A script answers `tooltiprequest` with its own element.
        rest(&mut p, "custom");
        assert_eq!(ev(&mut p, "document.$('#mine').getBoundingClientRect().width > 0"), "true");
        p.pointer_moved(500.0, 290.0);
        assert_eq!(ev(&mut p, "document.$('#mine').getBoundingClientRect().width > 0"), "false", "restored to hidden");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn popup_position_and_animation_css() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><button id="b" style="position:absolute;left:100px;top:50px;width:80px;height:20px">go</button>
               <popup id="pp" style="popup-position: top-right bottom-right; popup-animation: popup-animation(type:slide, axis:vertical, duration:100ms)">menu text</popup>
               <popup id="q" style="width:60px">other</popup>
               <style>popup#q { popup-reference-point: top-left; popup-anchor-reference-point: bottom-center }</style></body>"#,
            "popupcss",
        );
        let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
        assert_eq!(ev(&mut p, "getComputedStyle(document.$('#pp')).getPropertyValue('popup-position')"), "top-right bottom-right");
        assert!(ev(&mut p, "getComputedStyle(document.$('#pp')).getPropertyValue('popup-animation')").contains("slide"));
        ev(&mut p, "globalThis.w = document.$('#b').popup(document.$('#pp'), { animationDuration: 0 })");
        // The popup's right edge meets the anchor's right edge, its top the anchor's bottom.
        assert_eq!(ev(&mut p, "(() => { const r = document.$('#pp').getBoundingClientRect(), a = document.$('#b').getBoundingClientRect(); return Math.round(r.right - a.right) + ',' + Math.round(r.top - a.bottom); })()"), "0,0");
        ev(&mut p, "w.close()");
        ev(&mut p, "document.$('#b').popup(document.$('#q'))");
        assert_eq!(ev(&mut p, "(() => { const r = document.$('#q').getBoundingClientRect(), a = document.$('#b').getBoundingClientRect(); return Math.round(r.left - (a.left + a.width / 2)) + ',' + Math.round(r.top - a.bottom); })()"), "0,0");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn scripts_open_windows_and_talk_to_them() {
        let (mut p, path) = page(r#"<body><script>globalThis.log = [];
              window.addEventListener('message', (e) => log.push('msg:' + e.data.hi + ':' + e.source._id));
              Window.this.on('closerequest', (e) => { log.push('closerequest:' + e.reason); if (globalThis.keep) e.preventDefault(); });</script></body>"#, "windows");
        let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
        let take = |p: &Page| std::mem::take(&mut p.view.borrow_mut().window_requests);
        // `new Window` asks the shell for a window and gets its id at once.
        let id: u32 = ev(&mut p, "globalThis.w = new Window({ url: 'child.html', width: 300, height: 200, caption: 'Hi there', type: Window.TOOL_WINDOW, alignment: -5, parameters: { a: [1, 2] } }); w._id").parse().unwrap();
        let Some(WindowCommand::Open(spec)) = take(&p).into_iter().next() else { panic!("an Open request") };
        assert_eq!((spec.id, spec.modal), (id, false));
        assert_eq!((spec.get("url"), spec.get("caption"), spec.get("type"), spec.get("alignment")), (Some("child.html"), Some("Hi there"), Some("tool"), Some("-5")));
        assert_eq!((spec.get("width"), spec.get("parameters")), (Some("300"), Some(r#"{"a":[1,2]}"#)));
        // What the opened window sends back, and its close, reach the opener.
        p.window_message(id, r#"{"hi":"there"}"#);
        assert_eq!(ev(&mut p, "log.join()"), format!("msg:there:{id}"));
        ev(&mut p, "w.closed.then((v) => log.push('closed:' + JSON.stringify(v))); w.on('close', (e) => log.push('close:' + e.value))");
        assert_eq!(ev(&mut p, "w.isValid"), "true");
        p.window_closed(id, r#"{"ok":true}"#);
        assert_eq!(ev(&mut p, "log.slice(1).join()"), r#"close:[object Object],closed:{"ok":true}"#);
        assert_eq!(ev(&mut p, "w.isValid"), "false");
        // A dialog is flagged modal and resolves a Promise.
        ev(&mut p, "Window.this.modal({ url: 'dlg.html' }).then((v) => log.push('dialog:' + v))");
        let Some(WindowCommand::Open(dlg)) = take(&p).into_iter().next() else { panic!("an Open request") };
        assert!(dlg.modal);
        assert_eq!(dlg.get("type"), Some("dialog"));
        p.window_closed(dlg.id, "\"saved\"");
        assert_eq!(ev(&mut p, "log.slice(3).join()"), "dialog:saved");
        // Messages out, closing, and `closerequest`.
        ev(&mut p, "globalThis.w2 = new Window({ html: '<p>x</p>' }); w2.postMessage({ n: 1 }); w2.close('bye'); Window.this.postMessage(5); Window.this.close(7)");
        let cmds = take(&p);
        assert!(matches!(&cmds[1], WindowCommand::Post { json, .. } if json == r#"{"n":1}"#));
        assert!(matches!(&cmds[2], WindowCommand::CloseId(_)));
        assert!(matches!(&cmds[3], WindowCommand::Post { to: 0, json } if json == "5"));
        assert_eq!(cmds[4], WindowCommand::CloseWith("7".into()));
        assert!(p.close_request(0));
        ev(&mut p, "globalThis.keep = true");
        assert!(!p.close_request(1), "a listener can keep the window open");
        assert_eq!(ev(&mut p, "log.slice(4).join()"), "closerequest:0,closerequest:1");
        // Window.share crosses windows as JSON.
        ev(&mut p, "Window.share.theme = { dark: true }; Window.share.n = 3");
        assert_eq!(ev(&mut p, "JSON.stringify([Window.share.theme, Window.share.n, 'n' in Window.share, Object.keys(Window.share).sort(), Window.share.nothing])"), r#"[{"dark":true},3,true,["n","theme"],null]"#);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_opened_window_knows_its_opener_and_parameters() {
        let path = std::env::temp_dir().join(format!("nile-opened-{}.html", std::process::id()));
        std::fs::write(&path, "<body><script>globalThis.seen = [Window.this.parent && Window.this.parent._id, Window.this.parameters && Window.this.parameters.x];</script></body>").unwrap();
        let info = WindowInfo { id: 12, opener: Some(7), modal: true, parameters: r#"{"x":42}"#.into() };
        let mut p = Page::load_window(path.clone(), (400.0, 300.0), info.clone());
        assert_eq!(p.eval("JSON.stringify(seen)").0.unwrap(), "[7,42]");
        p.reload();
        assert_eq!(p.eval("JSON.stringify(seen)").0.unwrap(), "[7,42]", "reloading keeps who opened it");
        assert_eq!(p.view.borrow().window_info, info);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn popups_can_live_in_windows_of_their_own() {
        let (mut p, path) = page(
            r#"<body style="margin:0;background:#ffffff"><button id="b" style="position:absolute;left:10px;top:10px">go</button>
               <popup id="m" style="width:100px;height:40px;background:#ff0000;border:0;padding:0;box-shadow:none"></popup></body>"#,
            "portal",
        );
        let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
        let pixel = |buf: &[u8], w: u32, x: u32, y: u32| {
            let at = ((y * w + x) * 4) as usize;
            [buf[at + 2], buf[at + 1], buf[at]] // r, g, b
        };
        let paint = |p: &mut Page| {
            let mut buf = vec![0u8; 600 * 300 * 4];
            p.paint(&mut buf, 600, 300, 1.0);
            buf
        };
        // In the window the popup is painted by the page.
        ev(&mut p, "globalThis.w = document.$('#b').popup(document.$('#m'), { x: 200, y: 100 })");
        assert_eq!(pixel(&paint(&mut p), 600, 220, 110), [255, 0, 0]);
        ev(&mut p, "w.close()");
        // With a window of its own the page leaves it out and reports where it is...
        ev(&mut p, "globalThis.w = document.$('#b').popup(document.$('#m'), { x: 200, y: 100, window: 'popup' })");
        let portals = p.view.borrow().portals.clone();
        assert_eq!(portals.len(), 1);
        assert_eq!(portals[0].rect, (200.0, 100.0, 100.0, 40.0));
        assert_ne!(pixel(&paint(&mut p), 600, 220, 110), [255, 0, 0], "not drawn twice");
        // ...and paints it for that window, with the page point `origin` at the corner.
        let mut buf = vec![0u8; 100 * 40 * 4];
        p.paint_portal(&mut buf, 100, 40, 1.0, portals[0].node, (200.0, 100.0));
        assert_eq!(pixel(&buf, 100, 5, 5), [255, 0, 0]);
        assert_eq!(pixel(&buf, 100, 95, 35), [255, 0, 0]);
        // A popup may lie outside the window altogether.
        ev(&mut p, "w.close()");
        assert!(p.view.borrow().portals.is_empty());
        ev(&mut p, "globalThis.w = document.$('#b').popup(document.$('#m'), { x: 900, y: -20, window: 'popup' })");
        assert_eq!(p.view.borrow().portals[0].rect.0, 900.0);
        let node = p.view.borrow().portals[0].node;
        let mut buf = vec![0u8; 100 * 40 * 4];
        p.paint_portal(&mut buf, 100, 40, 1.0, node, (900.0, -20.0));
        assert_eq!(pixel(&buf, 100, 50, 20), [255, 0, 0]);
        // The pointer works in window coordinates: hovering the popup at its screen spot hits it.
        p.pointer_moved(950.0, 0.0);
        assert_eq!(p.interaction().hover, Some(node));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn table_rows_columns_and_merged_cells_are_editable() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="h" contenteditable><table id="t"><tbody>
                 <tr><td>a1</td><td>b1</td><td>c1</td></tr>
                 <tr><td>a2</td><td>b2</td><td>c2</td></tr>
                 <tr><td>a3</td><td>b3</td><td>c3</td></tr></tbody></table><p>after</p></div></body>"#,
            "table_commands",
        );
        // The table as text: rows by `/`, cells by `|`, spans as `[colsxrows]`.
        let show = |p: &mut Page| js(p, "Array.from(document.querySelectorAll('#t tr')).map(tr => Array.from(tr.children).map(c => c.textContent.trim() + (c.colSpan > 1 || c.rowSpan > 1 ? '[' + c.colSpan + 'x' + c.rowSpan + ']' : '')).join('|')).join('/')");
        let caret = |p: &mut Page, cell: &str| js(p, &format!("(() => {{ const c = Array.from(document.querySelectorAll('#t td')).find(x => x.textContent.trim().startsWith('{cell}')); const r = document.createRange(); r.setStart(c.firstChild, 1); r.collapse(true); const s = getSelection(); s.removeAllRanges(); s.addRange(r); return 0; }})()"));
        let cmd = |p: &mut Page, name: &str| js(p, &format!("document.execCommand('{name}'); 0"));
        caret(&mut p, "b2");
        cmd(&mut p, "tableInsertRowBelow");
        assert_eq!(show(&mut p), "a1|b1|c1/a2|b2|c2/||/a3|b3|c3");
        cmd(&mut p, "tableInsertRowAbove");
        assert_eq!(js(&mut p, "document.querySelectorAll('#t tr').length"), "5");
        caret(&mut p, "b3");
        cmd(&mut p, "tableDeleteRow");
        cmd(&mut p, "tableDeleteRow");
        assert_eq!(show(&mut p), "a1|b1|c1/a2|b2|c2/||");
        cmd(&mut p, "tableDeleteRow");
        caret(&mut p, "b1");
        cmd(&mut p, "tableInsertColumnRight");
        assert_eq!(show(&mut p), "a1|b1||c1/a2|b2||c2");
        cmd(&mut p, "tableInsertColumnLeft");
        assert_eq!(show(&mut p), "a1||b1||c1/a2||b2||c2");
        caret(&mut p, "c1");
        cmd(&mut p, "tableDeleteColumn");
        assert_eq!(show(&mut p), "a1||b1|/a2||b2|");
        // Merge the block from a1 to b2 (three columns, two rows) into one cell, then split it again.
        js(&mut p, "(() => { const tds = Array.from(document.querySelectorAll('#t td')); const a = tds.find(x => x.textContent === 'a1'), b = tds.find(x => x.textContent === 'b2'); const r = document.createRange(); r.setStart(a.firstChild, 0); r.setEnd(b.firstChild, 1); const s = getSelection(); s.removeAllRanges(); s.addRange(r); return 0; })()");
        cmd(&mut p, "tableMergeCells");
        assert_eq!(js(&mut p, "document.querySelectorAll('#t td').length"), "3");
        let merged = show(&mut p);
        assert!(merged.starts_with("a1b1a2b2[3x2]") || (merged.contains("a1") && merged.contains("a2") && merged.contains("b2") && merged.contains("[3x2]")), "{merged}");
        // A row inserted below the merged cell grows its span only when the line crosses it.
        caret(&mut p, "a1");
        cmd(&mut p, "tableInsertRowBelow");
        assert_eq!(js(&mut p, "document.querySelectorAll('#t tr').length + ':' + Array.from(document.querySelectorAll('#t td')).filter(c => c.rowSpan > 1).length"), "3:1");
        caret(&mut p, "a1");
        cmd(&mut p, "tableSplitCell");
        assert_eq!(js(&mut p, "document.querySelectorAll('#t tr').length + ':' + Array.from(document.querySelectorAll('#t td')).every(c => c.rowSpan === 1 && c.colSpan === 1)"), "3:true");
        // The last row or column takes the table with it; undo brings it back.
        cmd(&mut p, "tableDeleteTable");
        assert_eq!(js(&mut p, "document.querySelectorAll('#t').length"), "0");
        js(&mut p, "document.execCommand('undo'); 0");
        assert_eq!(js(&mut p, "document.querySelectorAll('#t').length"), "1");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn images_in_the_editor_select_resize_and_delete() {
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";
        let (mut p, path) = page(
            &format!(r#"<body style="margin:0"><div id="h" contenteditable style="padding:10px"><p id="a">before <img id="i" src="data:image/png;base64,{png}" style="width:100px;height:60px"> after</p></div></body>"#),
            "editor_image",
        );
        let none = ModifiersState::default();
        // A click on the image selects it: outline and eight handles.
        let (x, y) = center(&p, "i");
        p.pointer_moved(x, y);
        p.press();
        p.release();
        let img = p.doc.borrow().find(|n| n.attr("id") == Some("i")).unwrap();
        assert_eq!(p.selected_image(), Some(img));
        let r = p.selected_image_rect().unwrap();
        assert_eq!((r.width, r.height), (100.0, 60.0));
        // Over a corner handle the cursor says so; dragging it resizes in proportion.
        p.pointer_moved(r.x + r.width, r.y + r.height);
        assert_eq!(p.cursor(), nile_style::Cursor::ResizeNwse);
        p.press();
        p.pointer_moved(r.x + r.width + 50.0, r.y + r.height + 5.0);
        p.release();
        let size = |p: &mut Page| js(p, "document.getElementById('i').getAttribute('width') + 'x' + document.getElementById('i').getAttribute('height')");
        assert_eq!(size(&mut p), "129x78", "the corner keeps the proportions");
        assert_eq!(p.selected_image(), Some(img), "still selected");
        // An edge handle moves one side only.
        let r = p.selected_image_rect().unwrap();
        p.pointer_moved(r.x + r.width / 2.0, r.y + r.height);
        assert_eq!(p.cursor(), nile_style::Cursor::ResizeV);
        p.press();
        p.pointer_moved(r.x + r.width / 2.0, r.y + r.height - 20.0);
        p.release();
        assert_eq!(size(&mut p), "129x58");
        // Undo puts the old sizes back, step by step.
        js(&mut p, "document.execCommand('undo'); 0");
        assert_eq!(size(&mut p), "129x78");
        js(&mut p, "document.execCommand('undo'); 0");
        assert_eq!(js(&mut p, "document.getElementById('i').getAttribute('width')"), "null");
        // Delete removes the image. (Undo rebuilt the document, so the image is a new node.)
        let (x, y) = {
            let c = js(&mut p, "JSON.stringify((() => { const r = document.getElementById('i').getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })())");
            let v: Vec<f32> = c.trim_matches(|ch| ch == '[' || ch == ']').split(',').map(|n| n.parse().unwrap()).collect();
            (v[0], v[1])
        };
        p.pointer_moved(x, y);
        p.press();
        p.release();
        assert!(p.selected_image().is_some());
        press_key(&mut p, "Backspace", none);
        assert_eq!(js(&mut p, "document.querySelectorAll('#h img').length + ':' + document.getElementById('a').textContent"), "0:before  after");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn media_query_lists_fire_change_when_the_system_theme_flips() {
        use nile_style::{set_system_theme, Color, SystemTheme};
        set_system_theme(SystemTheme { dark: false, accent: Color::rgb(10, 20, 30) });
        let (mut p, path) = page(
            r#"<body><script>
                 window.log = [];
                 const m = matchMedia('(prefers-color-scheme: dark)');
                 m.addEventListener('change', (e) => log.push('listener:' + e.matches + ':' + e.media + ':' + (e instanceof MediaQueryListEvent)));
                 m.onchange = (e) => log.push('onchange:' + e.matches);
                 window.unwatched = matchMedia('(prefers-color-scheme: dark)');
                 window.old = matchMedia('(prefers-color-scheme: dark)');
                 old.addListener((e) => log.push('old:' + e.matches));
               </script></body>"#,
            "mql-change",
        );
        assert_eq!(js(&mut p, "log.length"), "0");
        p.system_theme_changed();
        assert_eq!(js(&mut p, "log.length"), "0", "nothing flipped, nothing fires");
        set_system_theme(SystemTheme { dark: true, accent: Color::rgb(10, 20, 30) });
        p.system_theme_changed();
        assert_eq!(
            js(&mut p, "log.slice().sort().join('|')"),
            "listener:true:(prefers-color-scheme: dark):true|old:true|onchange:true"
        );
        assert_eq!(js(&mut p, "unwatched.matches"), "true");
        p.system_theme_changed();
        assert_eq!(js(&mut p, "log.length"), "3", "an unchanged answer doesn't fire again");
        set_system_theme(SystemTheme { dark: false, accent: Color::rgb(10, 20, 30) });
        p.system_theme_changed();
        assert_eq!(js(&mut p, "log.filter((l) => l.includes(':false')).length"), "3");
        set_system_theme(SystemTheme::default());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_canvas_paints_what_scripts_draw_and_repaints_without_a_new_layout() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><canvas id="c" width="40" height="20"></canvas><script>
                 window.ctx = document.getElementById('c').getContext('2d');
                 ctx.fillStyle = '#ff0000'; ctx.fillRect(0, 0, 40, 20);
               </script></body>"#,
            "canvas",
        );
        assert_eq!(js(&mut p, "document.getElementById('c').width + 'x' + document.getElementById('c').height"), "40x20");
        let rect = p.view.borrow_mut().layout().node_rect(p.doc.borrow().find(|n| n.attr("id") == Some("c")).unwrap()).unwrap();
        assert_eq!((rect.width, rect.height), (40.0, 20.0));
        let (reds, blues) = count_colored(&mut p);
        assert_eq!((reds, blues), (40 * 20, 0), "the canvas shows its red fill");
        // Drawing again changes the picture and asks for a repaint, but not a new document version.
        p.sync();
        let version = p.doc.borrow().version;
        let (_, repaint) = p.eval("ctx.fillStyle = '#0000ff'; ctx.fillRect(0, 0, 20, 20);");
        assert_eq!(p.doc.borrow().version, version, "drawing doesn't touch the document");
        assert!(repaint, "a canvas drawn on makes the page repaint");
        assert!(!p.sync(), "and only once");
        let (reds, blues) = count_colored(&mut p);
        assert_eq!((reds, blues), (20 * 20, 20 * 20));
        // Resizing clears the canvas and re-lays-out the box.
        js(&mut p, "document.getElementById('c').width = 10;");
        assert_eq!(count_colored(&mut p), (0, 0));
        let rect = p.view.borrow_mut().layout().node_rect(p.doc.borrow().find(|n| n.attr("id") == Some("c")).unwrap()).unwrap();
        assert_eq!(rect.width, 10.0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn inline_svg_paints_follows_color_and_changes_with_scripts() {
        let (mut p, path) = page(
            r##"<style>.t { fill: #ff0000 } #cur { color: #0000ff }</style><body style="margin:0">
                 <svg id="a" width="20" height="10" viewBox="0 0 20 10"><rect class="t" width="20" height="10" fill="#00ff00"/></svg>
                 <svg id="cur" width="10" height="10"><rect id="c" width="10" height="10" fill="currentColor"/></svg>
               </body>"##,
            "svg-inline",
        );
        let rect = p.view.borrow_mut().layout().node_rect(p.doc.borrow().find(|n| n.attr("id") == Some("a")).unwrap()).unwrap();
        assert_eq!((rect.width, rect.height), (20.0, 10.0), "sized by its width and height");
        let (reds, blues) = count_colored(&mut p);
        // The page rule beats the fill attribute (20x10 red) and currentColor takes the CSS colour (10x10 blue).
        assert_eq!((reds, blues), (200, 100));
        // A script changes the SVG: it repaints with the new picture.
        js(&mut p, "document.getElementById('c').setAttribute('fill', '#ff0000');");
        let (reds, blues) = count_colored(&mut p);
        assert_eq!((reds, blues), (300, 0));
        // An SVG made by script (createElementNS keeps the camel-case names the format needs).
        js(
            &mut p,
            r##"const ns = 'http://www.w3.org/2000/svg';
               const s = document.createElementNS(ns, 'svg'); s.setAttribute('width', 30); s.setAttribute('height', 10); s.setAttribute('viewBox', '0 0 3 1');
               const r = document.createElementNS(ns, 'rect'); r.setAttribute('width', 3); r.setAttribute('height', 1); r.setAttribute('fill', '#0000ff');
               s.appendChild(r); document.body.appendChild(s);"##,
        );
        let (_, blues) = count_colored(&mut p);
        // 30x10 blue (the viewBox is scaled up), give or take the antialiased edges of a fractional position.
        assert!((280..=300).contains(&blues), "the new svg is painted blue: {blues}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn img_and_backgrounds_take_svg_files() {
        let svg = "data:image/svg+xml;utf8,<svg%20xmlns=%22http://www.w3.org/2000/svg%22%20width=%2210%22%20height=%2210%22><rect%20width=%2210%22%20height=%2210%22%20fill=%22%23ff0000%22/></svg>";
        let (mut p, path) = page(
            &format!(r#"<body style="margin:0"><img id="i" src="{svg}"><div style="width:10px;height:10px;background:url({svg})"></div></body>"#),
            "svg-img",
        );
        let rect = p.view.borrow_mut().layout().node_rect(p.doc.borrow().find(|n| n.attr("id") == Some("i")).unwrap()).unwrap();
        assert_eq!((rect.width, rect.height), (10.0, 10.0), "an svg image has its own size");
        assert_eq!(count_colored(&mut p).0, 200, "both the <img> and the background show it");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn filters_clip_paths_masks_and_blend_modes_change_the_pixels() {
        let (mut p, path) = page(
            r#"<body style="margin:0;background:#ffffff">
               <div style="position:absolute;left:0;top:0;width:40px;height:40px;background:#ff0000;filter:grayscale(1)"></div>
               <div style="position:absolute;left:50px;top:0;width:40px;height:40px;background:#ff0000;clip-path:circle(50%)"></div>
               <div style="position:absolute;left:100px;top:0;width:40px;height:40px;background:#000000;mask-image:linear-gradient(90deg,#000,transparent)"></div>
               <div style="position:absolute;left:150px;top:0;width:40px;height:40px;background:#ffff00"></div>
               <div style="position:absolute;left:150px;top:0;width:40px;height:40px;background:#00ffff;mix-blend-mode:multiply"></div>
               <div style="position:absolute;left:200px;top:0;width:40px;height:40px;background:#ff0000;visibility:hidden"></div>
               </body>"#,
            "effects",
        );
        let mut buf = vec![0u8; 300 * 60 * 4];
        p.paint(&mut buf, 300, 60, 1.0);
        let px = |x: u32, y: u32| {
            let at = ((y * 300 + x) * 4) as usize;
            [buf[at + 2], buf[at + 1], buf[at]]
        };
        let gray = px(20, 20);
        assert!(gray[0].abs_diff(gray[1]) < 3 && gray[1].abs_diff(gray[2]) < 3 && gray[0] > 30 && gray[0] < 120, "grayscale: {gray:?}");
        assert_eq!(px(70, 20), [255, 0, 0], "inside the circle");
        assert_eq!(px(51, 1), [255, 255, 255], "the circle's corner is clipped");
        let (left, right) = (px(101, 20), px(138, 20));
        assert!(left[0] < 40 && right[0] > 215, "the mask fades the black box out to the right: {left:?} {right:?}");
        assert_eq!(px(170, 20), [0, 255, 0], "yellow multiplied by cyan is green");
        assert_eq!(px(220, 20), [255, 255, 255], "visibility: hidden draws nothing");
        let _ = std::fs::remove_file(path);
    }

    /// A mono 8 kHz, 16-bit WAV of `ms` milliseconds of silence.
    fn silent_wav(ms: u32) -> Vec<u8> {
        let data = 8 * ms * 2;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&[1, 0, 1, 0]);
        v.extend_from_slice(&8000u32.to_le_bytes());
        v.extend_from_slice(&16000u32.to_le_bytes());
        v.extend_from_slice(&[2, 0, 16, 0]);
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data.to_le_bytes());
        v.resize(v.len() + data as usize, 0);
        v
    }

    #[test]
    fn a_drag_with_files_that_leaves_the_window_is_handed_to_the_system() {
        // Absolute paths look different per platform: build them from the temp folder.
        let abs = std::env::temp_dir().join("nile drag b.png").to_string_lossy().replace('\\', "/");
        let url = if abs.starts_with('/') { format!("file://{}", abs.replace(' ', "%20")) } else { format!("file:///{}", abs.replace(' ', "%20")) };
        let (mut p, path) = page(
            &format!(
                r#"<body style="margin:0"><div id="src" draggable="true" style="width:60px;height:40px">file</div><div id="txt" draggable="true" style="position:absolute;left:100px;top:0;width:60px;height:40px">text</div>
               <script>
                 window.log = [];
                 const src = document.getElementById('src');
                 src.addEventListener('dragstart', (e) => {{ e.dataTransfer.setFiles(['out/report.pdf', '{abs}']); }});
                 src.addEventListener('dragend', (e) => log.push('end:' + e.dataTransfer.dropEffect));
                 const txt = document.getElementById('txt');
                 txt.addEventListener('dragstart', (e) => {{ e.dataTransfer.setData('text/uri-list', '{url}'); }});
               </script></body>"#
            ),
            "dragout",
        );
        let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
        let (sx, sy) = center(&p, "src");
        p.pointer_moved(sx, sy);
        p.press();
        p.pointer_moved(sx + 20.0, sy);
        assert!(p.dnd.is_some() && p.take_drag_out().is_none(), "still inside: an in-page drag");
        p.pointer_moved(-12.0, sy);
        assert!(p.dnd.is_none(), "the in-page drag ended");
        let files = p.take_drag_out().expect("handed out");
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("out/report.pdf") && std::path::Path::new(&files[0]).is_absolute(), "{}", files[0]);
        assert_eq!(files[1], abs);
        assert_eq!(ev(&mut p, "log.join()"), "end:copy");
        p.release();
        // A uri-list of file: URLs counts too; a drag without files just stays in the page.
        let (tx, ty) = center(&p, "txt");
        p.pointer_moved(tx, ty);
        p.press();
        p.pointer_moved(tx + 20.0, ty);
        p.pointer_moved(-5.0, ty);
        assert_eq!(p.take_drag_out(), Some(vec![abs.clone()]));
        p.release();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn html5_drag_and_drop_fires_the_events_and_carries_data() {
        let (mut p, path) = page(
            r#"<body style="margin:0"><div id="src" draggable="true" style="width:60px;height:40px;background:#c33">drag me</div>
               <div id="ok" style="position:absolute;left:200px;top:0;width:80px;height:80px;background:#3a3"></div>
               <div id="no" style="position:absolute;left:300px;top:0;width:80px;height:80px;background:#999"></div>
               <div id="plain" style="width:60px;height:20px">not draggable</div>
               <script>
                 window.log = [];
                 const note = (e) => log.push(e.type + ':' + e.target.id);
                 const src = document.getElementById('src'), ok = document.getElementById('ok'), no = document.getElementById('no');
                 src.addEventListener('dragstart', (e) => { note(e); e.dataTransfer.setData('text/plain', 'payload'); e.dataTransfer.effectAllowed = 'copy'; });
                 src.addEventListener('dragend', (e) => { note(e); log.push('effect=' + e.dataTransfer.dropEffect); });
                 for (const t of ['dragenter', 'dragleave']) { ok.addEventListener(t, note); no.addEventListener(t, note); }
                 ok.addEventListener('dragover', (e) => { e.preventDefault(); });
                 ok.addEventListener('drop', (e) => { note(e); log.push('got=' + e.dataTransfer.getData('text/plain') + ',' + e.dataTransfer.types.join()); });
                 no.addEventListener('dragover', () => {});
                 no.addEventListener('drop', note);
               </script></body>"#,
            "dnd",
        );
        let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
        let (sx, sy) = center(&p, "src");
        // Over a plain element nothing starts, however far the pointer goes.
        let (px, py) = center(&p, "plain");
        p.pointer_moved(px, py);
        p.press();
        p.pointer_moved(px + 50.0, py + 50.0);
        p.release();
        assert_eq!(ev(&mut p, "log.join()"), "");
        // Accepted drop.
        p.pointer_moved(sx, sy);
        p.press();
        p.pointer_moved(sx + 2.0, sy);
        assert!(p.dnd.is_none(), "a tiny move is still a click");
        p.pointer_moved(sx + 20.0, sy);
        assert!(p.dnd.is_some());
        let (ox, oy) = center(&p, "ok");
        p.pointer_moved(ox, oy);
        assert_eq!(p.cursor(), nile_style::Cursor::Move);
        p.release();
        let log = ev(&mut p, "log.join()");
        assert!(log.starts_with("dragstart:src,dragenter:ok"), "{log}");
        assert!(log.contains("drop:ok") && log.contains("got=payload,text/plain"), "{log}");
        assert!(log.ends_with("dragend:src,effect=copy"), "{log}");
        assert!(p.dnd.is_none());
        // A zone that doesn't accept: no drop, and the drag ends with no effect.
        ev(&mut p, "log.length = 0");
        p.pointer_moved(sx, sy);
        p.press();
        p.pointer_moved(sx + 20.0, sy);
        let (nx, ny) = center(&p, "no");
        p.pointer_moved(nx, ny);
        assert_eq!(p.cursor(), nile_style::Cursor::NotAllowed);
        p.release();
        let log = ev(&mut p, "log.join()");
        assert!(!log.contains("drop:no") && log.contains("dragleave:no") && log.ends_with("effect=none"), "{log}");
        // Escape cancels.
        ev(&mut p, "log.length = 0");
        p.pointer_moved(sx, sy);
        p.press();
        p.pointer_moved(sx + 20.0, sy);
        p.pointer_moved(ox, oy);
        p.key("keydown", "Escape".to_string(), ModifiersState::default());
        p.release();
        let log = ev(&mut p, "log.join()");
        assert!(!log.contains("drop:ok") && log.ends_with("effect=none"), "{log}");
        let _ = std::fs::remove_file(path);
    }

    #[cfg(feature = "audio")]
    #[test]
    fn audio_elements_and_the_sciter_audio_class_play_and_end() {
        std::env::set_var("NILE_AUDIO", "off");
        let dir = std::env::temp_dir().join(format!("nile-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("beep.wav"), silent_wav(250)).unwrap();
        let path = dir.join("index.html");
        std::fs::write(
            &path,
            r#"<body><audio id="a" src="beep.wav"></audio><script>
                 window.log = [];
                 const a = document.getElementById('a');
                 for (const t of ['loadedmetadata', 'play', 'pause', 'ended']) a.addEventListener(t, () => log.push(t));
                 window.dur = a.duration;
                 a.volume = 0.5;
                 a.play().then(() => log.push('started'));
                 Audio.load(__DIR__ + '/beep.wav').then(async (s) => { log.push('loaded'); await s.play(); log.push('sciter-done'); });
                 window.missing = 'pending';
                 Audio.load('nope.wav').catch((e) => { window.missing = 'rejected'; });
               </script></body>"#,
        )
        .unwrap();
        let mut p = Page::load(path, (600.0, 300.0), 0.0);
        p.layout(600.0, 300.0);
        assert!((js(&mut p, "dur").parse::<f64>().unwrap() - 0.25).abs() < 0.01, "duration from the file");
        let until = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < until && !js(&mut p, "log.join()").contains("sciter-done") {
            std::thread::sleep(Duration::from_millis(30));
            p.tick();
        }
        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(10));
            p.tick();
        }
        let log = js(&mut p, "log.join()");
        // One `error:` line, so CI's failure annotation shows what happened.
        eprintln!("error: audio test log: {log}");
        for expect in ["loadedmetadata", "play", "started", "ended", "loaded", "sciter-done"] {
            assert!(log.contains(expect), "{expect} in {log}");
        }
        let ended_at = log.find("ended").unwrap();
        assert!(log.find("sciter-done").unwrap() > log.find("loaded").unwrap(), "{log}");
        assert!(ended_at > log.find("play").unwrap(), "{log}");
        assert_eq!(js(&mut p, "a.ended + ',' + a.paused + ',' + a.volume"), "true,true,0.5");
        assert_eq!(js(&mut p, "missing"), "rejected");
        assert_eq!(js(&mut p, "(new Audio(__DIR__ + '/beep.wav')) instanceof Audio && !(a instanceof HTMLElement === false)"), "true");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn canvas_draws_an_img_by_its_src() {
        // A 1x1 red PNG as a data URL.
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
        let (mut p, path) = page(
            &format!(
                r#"<body style="margin:0"><img id="i" src="data:image/png;base64,{png}" style="display:none"><canvas id="c" width="10" height="10"></canvas><script>
                     const c = document.getElementById('c').getContext('2d');
                     c.drawImage(document.getElementById('i'), 0, 0, 10, 10);
                     window.info = document.getElementById('i').naturalWidth + 'x' + document.getElementById('i').naturalHeight;
                   </script></body>"#
            ),
            "canvas-img",
        );
        assert_eq!(js(&mut p, "info"), "1x1");
        let (reds, _) = count_colored(&mut p);
        assert!(reds >= 90, "the image fills the canvas: {reds}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn media_query_lists_fire_change_when_the_window_resizes() {
        let (mut p, path) = page(
            r#"<body><script>
                 window.log = [];
                 matchMedia('(min-width: 700px)').addEventListener('change', (e) => log.push(e.matches));
               </script></body>"#,
            "mql-resize",
        );
        p.layout(800.0, 300.0);
        p.run_timers();
        p.layout(500.0, 300.0);
        p.run_timers();
        assert_eq!(js(&mut p, "log.join(',')"), "true,false");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_accessibility_tree_names_roles_and_states() {
        use a11y::{A11yAction, A11yRole as R};
        let (mut p, path) = page(
            r#"<html><head><title>Sign in</title></head><body style="margin:0">
                 <h2>Welcome back</h2>
                 <label for="u">User name</label><input id="u" value="ada" required>
                 <input id="pw" type="password" value="secret" aria-label="Password">
                 <label><input id="rem" type="checkbox" checked> Remember me</label>
                 <button id="go">Sign <b>in</b></button> <a id="lnk" href="/help">Help</a>
                 <img alt="Company logo" src="x.png"><div style="display:none">hidden text</div><p aria-hidden="true">also hidden</p>
                 <div role="alert">Careful</div>
                 <select id="s" aria-label="Colour"><option>Red</option><option selected>Green</option></select>
                 <input id="r" type="range" min="0" max="10" value="4" aria-label="Volume">
                 <table><tr><th>Name</th><td>Ada</td></tr></table>
                 <button id="off" disabled>Nope</button>
                 <details open><summary>More</summary>content</details>
               </body></html>"#,
            "a11y",
        );
        let tree = p.a11y_tree();
        let find = |name: &str, role: R| tree.nodes.iter().find(|n| n.role == role && n.name == name).unwrap_or_else(|| panic!("{role:?} {name:?} in {:#?}", tree.nodes.iter().map(|n| (n.role, &n.name)).collect::<Vec<_>>()));
        assert_eq!(tree.nodes[0].role, R::Window);
        assert_eq!(tree.nodes[0].name, "Sign in");
        assert_eq!(find("Welcome back", R::Heading).level, 2);
        let user = find("User name", R::TextInput);
        assert_eq!((user.value.as_deref(), user.required, user.focusable), (Some("ada"), true, true));
        assert_eq!(find("Password", R::PasswordInput).value.as_deref(), Some("••••••"));
        assert_eq!(find("Remember me", R::CheckBox).checked, Some(Some(true)));
        let button = find("Sign in", R::Button);
        assert!(button.clickable && button.children.is_empty(), "a text-only button is named by its text");
        assert_eq!(find("Help", R::Link).link_target, "/help");
        find("Company logo", R::Image);
        find("Careful", R::StaticText);
        assert!(tree.nodes.iter().all(|n| !n.name.contains("hidden")), "hidden content is left out");
        assert_eq!(find("Colour", R::ComboBox).value.as_deref(), Some("Green"));
        assert_eq!(find("Volume", R::Slider).numeric, Some((4.0, 0.0, 10.0, 1.0)));
        find("Name", R::ColumnHeader);
        assert!(find("Nope", R::Button).disabled);
        let details = tree.nodes.iter().find(|n| n.role == R::Details).unwrap();
        assert_eq!(details.expanded, Some(true));
        // Every child is listed, bounds sit inside the window, and focus defaults to the root.
        for n in &tree.nodes {
            for c in &n.children {
                assert!(tree.nodes.iter().any(|x| x.id == *c), "child {c} of {:?} exists", n.role);
            }
        }
        assert_eq!(tree.focus, tree.root);
        let json = tree.to_json();
        assert!(json.starts_with(r#"{"root":1,"focus":1,"nodes":[{"id":1,"role":"Window","name":"Sign in""#) && json.contains(r#""role":"CheckBox""#) && json.contains(r#""checked":true"#), "{json}");
        // Requests from assistive technology: focus, type, click.
        let id = user.id;
        assert!(p.a11y_action(id, A11yAction::Focus));
        assert_eq!(p.a11y_tree().focus, id);
        assert!(p.a11y_action(id, A11yAction::SetValue("grace".into())));
        assert_eq!(js(&mut p, "document.getElementById('u').value"), "grace");
        let rem = find("Remember me", R::CheckBox).id;
        assert!(p.a11y_action(rem, A11yAction::Click));
        assert_eq!(p.a11y_tree().nodes.iter().find(|n| n.id == rem).unwrap().checked, Some(Some(false)));
        let vol = find("Volume", R::Slider).id;
        p.a11y_action(vol, A11yAction::Increment);
        assert_eq!(js(&mut p, "document.getElementById('r').value"), "5");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn hover_waits_for_scrolling_to_pause() {
        let (mut p, path) = page(
            r#"<style>.r { height: 40px } .r:hover { background: red }</style><body style="margin:0"><div class=r id=a></div><div class=r id=b></div><div class=r id=c></div>
               <div style="height:900px"></div></body>"#,
            "hover_scroll",
        );
        p.pointer_moved(20.0, 20.0);
        let hovered = |p: &Page| p.interaction().hover.map(|n| p.doc.borrow().node(n).attr("id").map(str::to_string));
        assert_eq!(hovered(&p), Some(Some("a".into())));
        p.wheel(0.0, -45.0); // row b is now under the pointer
        assert_eq!(hovered(&p), Some(Some("a".into())), "unchanged while scrolling");
        assert!(p.tick().1.is_some(), "a wake-up is scheduled");
        std::thread::sleep(Duration::from_millis(110));
        p.tick();
        assert_eq!(hovered(&p), Some(Some("b".into())), "settled once scrolling paused");
        // A press settles it at once.
        p.wheel(0.0, -40.0);
        assert_eq!(hovered(&p), Some(Some("b".into())));
        p.press();
        assert_eq!(hovered(&p), Some(Some("c".into())));
        p.release();
        let _ = std::fs::remove_file(path);
    }
}
