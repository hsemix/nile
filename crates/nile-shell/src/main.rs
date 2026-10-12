//! `nile` — opens an HTML file in a native window, or renders it to PNG.
//!
//!   nile page.html                      open a window (F5 / Ctrl-R reloads, wheel scrolls)
//!   nile page.html --png out.png        render headless
//!        [--width 900] [--height 640] [--scale 2]
//!        [--hover X,Y] [--press X,Y] [--click X,Y]   simulate the pointer (logical px)
//!        [--type TEXT]                 type into the focused input ({Backspace}, {ArrowLeft}, ... for keys)
//!        [--wait MS]                   let timers run this long before the snapshot

use std::{cell::RefCell, num::NonZeroU32, path::PathBuf, rc::Rc, time::{Duration, Instant}};

use nile_dom::{Document, NodeId, ScriptSource};
use nile_layout::LayoutTree;
use nile_render::Renderer;
use nile_script::{Environment, ScriptHost, UiEvent};
use nile_style::{compute_styles_with, InteractionState, StyleMap};
use nile_text::TextEngine;
use skia_safe::{surfaces, AlphaType, Color, ColorType, EncodedImageFormat, ImageInfo};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{Window, WindowId},
};

/// Style, layout and scroll state. Shared (`Rc<RefCell<..>>`) between the page
/// and the script host, so scripts can read layout results. Never hold a borrow
/// across a call into scripts.
struct View {
    doc: Rc<RefCell<Document>>,
    /// `doc.version` that `styles` were computed for.
    seen_version: u64,
    styles: StyleMap,
    text: TextEngine,
    layout: Option<(f32, f32, LayoutTree)>,
    /// Logical viewport size, so hit testing and scripts can lay out on demand.
    viewport: (f32, f32),
    scroll_y: f32,
    /// A script moved the scroll position; the window needs repainting.
    scroll_dirty: bool,
    interaction: InteractionState,
    /// Focus moves made by scripts, so the page can fire `blur`/`focus` for them.
    focus_changes: Vec<(Option<NodeId>, Option<NodeId>)>,
}

impl View {
    /// Recompute styles (after an interaction change or a DOM mutation).
    // TODO: restyle only the affected elements and relayout only if a
    // layout-affecting property changed.
    fn restyle(&mut self) {
        let doc = self.doc.borrow();
        self.styles = compute_styles_with(&doc, &self.interaction);
        self.seen_version = doc.version;
        self.layout = None;
    }

    /// Restyle if scripts changed the document. True if they did.
    fn sync(&mut self) -> bool {
        let changed = self.doc.borrow().version != self.seen_version;
        if changed {
            self.restyle();
        }
        changed
    }

    fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
    }

    /// Layout at the current viewport, reusing the last result if unchanged.
    fn layout(&mut self) -> &LayoutTree {
        let (width, height) = self.viewport;
        let stale = !matches!(&self.layout, Some((w, h, _)) if *w == width && *h == height);
        if stale {
            let tree = nile_layout::layout(&self.doc.borrow(), &self.styles, &mut self.text, width, height);
            self.layout = Some((width, height, tree));
        }
        &self.layout.as_ref().unwrap().2
    }
}

/// What scripts see of the view. Each query first brings styles and layout up
/// to date, so a script reads what it just wrote.
struct Env(Rc<RefCell<View>>);

impl Environment for Env {
    fn bounding_rect(&self, node: NodeId) -> Option<[f32; 4]> {
        let mut v = self.0.borrow_mut();
        v.sync();
        let scroll = v.scroll_y;
        let r = v.layout().node_rect(node)?;
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

    fn set_focus(&self, node: Option<NodeId>) {
        let mut v = self.0.borrow_mut();
        if v.interaction.focus != node {
            let old = v.interaction.focus;
            v.interaction.focus = node;
            v.focus_changes.push((old, node));
            v.restyle();
        }
    }
}

/// One loaded document plus everything needed to paint and interact with it.
struct Page {
    path: PathBuf,
    /// Shared with the script host, which mutates it.
    doc: Rc<RefCell<Document>>,
    view: Rc<RefCell<View>>,
    renderer: Renderer,
    /// Last pointer position in logical viewport pixels.
    pointer: Option<(f32, f32)>,
    /// Element the primary button went down on, for deriving `click`.
    pressed: Option<NodeId>,
    scripts: Option<ScriptHost>,
    /// Caret position (in characters of `value`) in the focused text input.
    caret: usize,
    /// Value when the focused input got focus, to decide whether `change` fires.
    value_at_focus: Option<String>,
}

impl Page {
    fn load(path: PathBuf, viewport: (f32, f32), scroll_y: f32) -> Page {
        let html = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            format!("<body><h1>Could not open file</h1><p>{}: {e}</p></body>", path.display())
        });
        let doc = Rc::new(RefCell::new(Document::parse(&html)));
        let interaction = InteractionState::default();
        let styles = compute_styles_with(&doc.borrow(), &interaction);
        let view = Rc::new(RefCell::new(View {
            doc: doc.clone(),
            seen_version: doc.borrow().version,
            styles,
            text: TextEngine::new(),
            layout: None,
            viewport,
            scroll_y,
            scroll_dirty: false,
            interaction,
            focus_changes: Vec::new(),
        }));

        let scripts = match ScriptHost::new(doc.clone(), Rc::new(Env(view.clone()))) {
            Ok(host) => {
                let sources = doc.borrow().script_sources();
                let dir = path.parent().unwrap_or(std::path::Path::new("."));
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
            doc,
            view,
            renderer: Renderer::new(),
            pointer: None,
            pressed: None,
            scripts,
            caret: 0,
            value_at_focus: None,
        }
    }

    fn reload(&mut self) {
        let (viewport, scroll) = {
            let v = self.view.borrow();
            (v.viewport, v.scroll_y)
        };
        *self = Page::load(self.path.clone(), viewport, scroll);
    }

    /// Lay out at the given logical viewport size (a no-op if unchanged).
    fn layout(&mut self, width: f32, height: f32) {
        let mut v = self.view.borrow_mut();
        v.set_viewport(width, height);
        v.layout();
    }

    /// Restyle after an interaction state change.
    fn restyle(&mut self) {
        self.view.borrow_mut().restyle();
    }

    /// Pick up DOM changes and script-driven scrolling. True if the page
    /// needs repainting.
    fn sync(&mut self) -> bool {
        let (changed, moves) = {
            let mut v = self.view.borrow_mut();
            let changed = v.sync();
            (std::mem::take(&mut v.scroll_dirty) | changed, std::mem::take(&mut v.focus_changes))
        };
        let moved = !moves.is_empty();
        for (old, new) in moves {
            self.focus_moved(old, new);
        }
        changed | moved
    }

    /// Focus moved from `old` to `new`: end the old input's edit (firing
    /// `change` if its value changed), start the new one, and fire `blur`/`focus`.
    fn focus_moved(&mut self, old: Option<NodeId>, new: Option<NodeId>) {
        if let Some(o) = old {
            if self.value_at_focus.take().is_some_and(|v| v != self.value_of(o)) {
                self.emit(o, UiEvent::Plain("change"));
            }
            self.emit(o, UiEvent::Plain("blur"));
        }
        if let Some(n) = new {
            self.caret = self.value_of(n).chars().count();
            self.value_at_focus = self.is_text_input(n).then(|| self.value_of(n));
            self.emit(n, UiEvent::Plain("focus"));
        }
    }

    fn interaction(&self) -> InteractionState {
        self.view.borrow().interaction
    }

    /// Dispatch an event to scripts, then pick up any changes. Returns
    /// (not default-prevented, page changed).
    fn emit(&mut self, target: NodeId, event: UiEvent) -> (bool, bool) {
        let Some(host) = &self.scripts else { return (true, false) };
        let proceed = host.dispatch(target, &event);
        (proceed, self.sync())
    }

    /// Run due timers. Returns whether the page changed and when to wake next.
    fn run_timers(&mut self) -> (bool, Option<Duration>) {
        let Some(host) = &self.scripts else { return (false, None) };
        let next = host.tick();
        (self.sync(), next)
    }

    /// The element under a viewport point, if the page has been laid out.
    fn hit_test(&mut self, x: f32, y: f32) -> Option<NodeId> {
        let mut v = self.view.borrow_mut();
        if v.viewport.0 <= 0.0 {
            return None;
        }
        let scroll = v.scroll_y;
        v.layout().hit_test(x, y + scroll)
    }

    /// Nearest ancestor-or-self that can take focus (links, form controls, `tabindex`).
    fn focusable(&self, mut id: NodeId) -> Option<NodeId> {
        loop {
            let doc = self.doc.borrow();
            let n = doc.node(id);
            let focusable = match n.tag() {
                Some("a") => n.attr("href").is_some(),
                Some("input" | "button" | "select" | "textarea") => true,
                _ => n.attr("tabindex").is_some(),
            };
            if focusable {
                return Some(id);
            }
            id = n.parent?;
        }
    }

    /// Re-run hit testing for the current pointer and scroll, firing
    /// `mouseout`/`mouseover` when the hovered element changes. True if the
    /// page needs repainting.
    fn refresh_hover(&mut self) -> bool {
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

    fn pointer_moved(&mut self, x: f32, y: f32) -> bool {
        self.pointer = Some((x, y));
        let mut changed = self.refresh_hover();
        if let Some(t) = self.interaction().hover {
            changed |= self.emit(t, UiEvent::Mouse { kind: "mousemove", x, y, button: 0 }).1;
        }
        changed
    }

    fn pointer_left(&mut self) -> bool {
        self.pointer = None;
        self.refresh_hover()
    }

    fn mouse_event(&self, kind: &'static str) -> UiEvent {
        let (x, y) = self.pointer.unwrap_or_default();
        UiEvent::Mouse { kind, x, y, button: 0 }
    }

    /// Primary button down: the hovered element becomes active and focus moves
    /// to its nearest focusable ancestor (or is cleared).
    fn press(&mut self) -> bool {
        let target = self.interaction().hover;
        self.pressed = target;
        let mut changed = false;
        if let Some(t) = target {
            changed |= self.emit(t, self.mouse_event("mousedown")).1;
        }
        let next = InteractionState {
            active: target,
            focus: target.and_then(|n| self.focusable(n)),
            ..self.interaction()
        };
        changed |= self.set_interaction(next);
        // Clicking inside a text input puts the caret there.
        if let (Some(f), Some((x, y))) = (self.interaction().focus.filter(|f| self.is_text_input(*f)), self.pointer) {
            let at = {
                let mut v = self.view.borrow_mut();
                let scroll = v.scroll_y;
                v.layout().text_index_at(f, x, y + scroll)
            };
            if let Some(at) = at {
                let caret = nile_layout::caret_char(&self.doc.borrow(), f, at);
                changed |= caret != self.caret;
                self.caret = caret;
            }
        }
        changed
    }

    /// Primary button up: `mouseup`, then `click` on the common ancestor of the
    /// pressed and released elements.
    fn release(&mut self) -> bool {
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

    fn value_of(&self, id: NodeId) -> String {
        self.doc.borrow().node(id).attr("value").unwrap_or("").to_string()
    }

    fn is_text_input(&self, id: NodeId) -> bool {
        let doc = self.doc.borrow();
        let n = doc.node(id);
        n.tag() == Some("input")
            && !matches!(
                n.attr("type").map(str::to_ascii_lowercase).as_deref(),
                Some("checkbox" | "radio" | "hidden" | "file" | "image" | "button" | "submit" | "reset")
            )
    }

    fn is_disabled(&self, id: NodeId) -> bool {
        self.doc.borrow().has_state(id, "disabled")
    }

    /// Default action of a click on `target`: toggle a checkbox, select a
    /// radio, or forward a `<label>` click to its control. True if the page changed.
    fn click_default(&mut self, target: NodeId) -> bool {
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

    /// Edit the focused text input for a key press. True if the value or
    /// caret changed (so the page needs repainting).
    fn edit_key(&mut self, key: &str, mods: ModifiersState) -> bool {
        let Some(focus) = self.interaction().focus.filter(|f| self.is_text_input(*f)) else { return false };
        if self.is_disabled(focus) || self.doc.borrow().node(focus).attr("readonly").is_some() {
            return false;
        }
        let mut chars: Vec<char> = self.value_of(focus).chars().collect();
        let caret = self.caret.min(chars.len());
        let mut new_caret = caret;
        let mut edited = false;
        match key {
            "Backspace" if caret > 0 => {
                chars.remove(caret - 1);
                new_caret = caret - 1;
                edited = true;
            }
            "Delete" if caret < chars.len() => {
                chars.remove(caret);
                edited = true;
            }
            "ArrowLeft" => new_caret = caret.saturating_sub(1),
            "ArrowRight" => new_caret = (caret + 1).min(chars.len()),
            "Home" => new_caret = 0,
            "End" => new_caret = chars.len(),
            "Enter" => {
                // Commit: fire `change` if the value moved since focus.
                if self.value_at_focus.as_ref().is_some_and(|v| *v != self.value_of(focus)) {
                    self.value_at_focus = Some(self.value_of(focus));
                    return self.emit(focus, UiEvent::Plain("change")).1 | true;
                }
                return false;
            }
            k if k.chars().count() == 1 && !mods.control_key() && !mods.super_key() => {
                let c = k.chars().next().unwrap();
                if !c.is_control() {
                    chars.insert(caret, c);
                    new_caret = caret + 1;
                    edited = true;
                }
            }
            _ => return false,
        }
        if edited {
            let value: String = chars.iter().collect();
            self.doc.borrow_mut().set_attr(focus, "value", &value);
            self.caret = new_caret;
            self.sync();
            self.emit(focus, UiEvent::Plain("input"));
            true
        } else {
            let moved = new_caret != self.caret;
            self.caret = new_caret;
            moved
        }
    }

    /// Caret to draw: the focused text input and the caret's byte offset in its displayed text.
    fn caret_for_paint(&self) -> Option<(NodeId, usize)> {
        let focus = self.interaction().focus.filter(|f| self.is_text_input(*f))?;
        let doc = self.doc.borrow();
        Some((focus, nile_layout::caret_byte(&doc, focus, self.caret)))
    }

    fn common_ancestor(&self, a: NodeId, b: NodeId) -> Option<NodeId> {
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
    fn key(&mut self, kind: &'static str, key: String, mods: ModifiersState) -> (bool, bool) {
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

    fn set_interaction(&mut self, next: InteractionState) -> bool {
        let current = self.interaction();
        if next == current {
            return false;
        }
        let (old_focus, new_focus) = (current.focus, next.focus);
        self.view.borrow_mut().interaction = next;
        self.restyle();
        if old_focus != new_focus {
            self.focus_moved(old_focus, new_focus);
        }
        true
    }

    /// CSS canvas background: the root element's background, or the body's if
    /// the root has none, fills the whole viewport. Defaults to white.
    fn canvas_background(&self) -> Color {
        ["html", "body"]
            .iter()
            .filter_map(|tag| self.doc.borrow().find(|n| n.tag() == Some(tag)))
            .find_map(|id| self.view.borrow().styles.get(id).background)
            .map(|c| Color::from_argb(255, c.r, c.g, c.b))
            .unwrap_or(Color::WHITE)
    }

    /// Paint into a BGRA pixel buffer of `width`×`height` physical pixels.
    fn paint(&mut self, pixels: &mut [u8], width: u32, height: u32, scale: f32) {
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
        let mut view = self.view.borrow_mut();
        let max_scroll = (view.layout().content_height - lh).max(0.0);
        view.scroll_y = view.scroll_y.clamp(0.0, max_scroll);
        let scroll = view.scroll_y;
        let view = &*view;

        let canvas = surface.canvas();
        canvas.clear(bg);
        canvas.save();
        canvas.scale((scale, scale));
        let tree = &view.layout.as_ref().expect("laid out above").2;
        self.renderer.paint(canvas, tree, &view.styles, scroll, caret);
        canvas.restore();
    }
}

/// Pointer simulation for headless renders: hover (and optionally press) at a logical point.
struct Pointer {
    x: f32,
    y: f32,
    press: bool,
    /// Release again after pressing (a full click).
    release: bool,
}

/// Options for a headless render.
struct Snapshot {
    out: String,
    width: u32,
    height: u32,
    scale: f32,
    pointer: Option<Pointer>,
    typed: Option<String>,
    wait_ms: u64,
}

fn render_png(page: &mut Page, snap: Snapshot) {
    let Snapshot { out, width, height, scale, pointer, typed, wait_ms } = snap;
    let out = out.as_str();
    // Let timers run (animations, setTimeout) before the snapshot.
    let deadline = Instant::now() + Duration::from_millis(wait_ms);
    while Instant::now() < deadline {
        let (_, next) = page.run_timers();
        let Some(next) = next else { break };
        std::thread::sleep(next.min(deadline.saturating_duration_since(Instant::now())));
    }
    page.run_timers();
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
            let key = if key == "Space" { " ".to_string() } else { key };
            let (proceed, _) = page.key("keydown", key.clone(), ModifiersState::default());
            if proceed {
                page.edit_key(&key, ModifiersState::default());
            }
            page.key("keyup", key, ModifiersState::default());
        }
    }
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
        let title = {
            let doc = self.page.doc.borrow();
            doc.find(|n| n.tag() == Some("title"))
                .map(|t| doc.text_content(t).trim().to_string())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "nile".into())
        };

        let attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(900.0, 640.0));
        let window = Rc::new(event_loop.create_window(attrs).expect("create window"));
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        self.surface = Some(surface);
        self.window = Some(window);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let (changed, next) = self.page.run_timers();
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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => window.request_redraw(),
            WindowEvent::CursorMoved { position, .. } => {
                let scale = window.scale_factor();
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
                let changed = match state {
                    ElementState::Pressed => self.page.press(),
                    ElementState::Released => self.page.release(),
                };
                if changed {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 40.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / window.scale_factor() as f32,
                };
                self.page.view.borrow_mut().scroll_y -= dy;
                // Content moved under a stationary pointer. Clamping happens in
                // paint, so hover may lag one frame at the scroll limits.
                self.page.refresh_hover();
                window.request_redraw();
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                let Some(key) = dom_key(&event.logical_key) else { return };
                let pressed = event.state == ElementState::Pressed;
                let (proceed, changed) = self.page.key(if pressed { "keydown" } else { "keyup" }, key.clone(), self.mods);
                // F5 or Ctrl/Cmd+R reloads, unless the page handled the key.
                let reload = key == "F5"
                    || (key.eq_ignore_ascii_case("r") && (self.mods.control_key() || self.mods.super_key()));
                let edited = pressed && proceed && !reload && self.page.edit_key(&key, self.mods);
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
    let mut typed = None;
    let (mut width, mut height, mut scale) = (900u32, 640u32, 1.0f32);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--png" => png = args.next(),
            "--width" => width = args.next().and_then(|v| v.parse().ok()).unwrap_or(width),
            "--height" => height = args.next().and_then(|v| v.parse().ok()).unwrap_or(height),
            "--hover" | "--press" | "--click" => {
                let xy = args.next().and_then(|v| {
                    let (x, y) = v.split_once(',')?;
                    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
                });
                if let Some((x, y)) = xy {
                    pointer = Some(Pointer { x, y, press: arg != "--hover", release: arg == "--click" });
                }
            }
            "--type" => typed = args.next(),
            "--wait" => wait_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--scale" => scale = args.next().and_then(|v| v.parse().ok()).unwrap_or(scale),
            _ => file = Some(PathBuf::from(arg)),
        }
    }
    let Some(file) = file else {
        eprintln!("usage: nile <file.html> [--png out.png] [--width N] [--height N] [--scale N] [--hover X,Y] [--press X,Y] [--click X,Y] [--type TEXT] [--wait MS]");
        std::process::exit(2);
    };

    let mut page = Page::load(file, (width as f32, height as f32), 0.0);
    if let Some(out) = png {
        render_png(&mut page, Snapshot { out, width, height, scale, pointer, typed, wait_ms });
        return;
    }

    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App { page, window: None, surface: None, mods: ModifiersState::default() };
    event_loop.run_app(&mut app).expect("run app");
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
                p.edit_key(k, ModifiersState::default());
            }
        };
        let (x, y) = center(&p, "t");
        p.pointer_moved(x + 60.0, y); // inside the input, right of the text: caret goes to the end
        p.press();
        p.release();
        assert_eq!(p.caret, 2);
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
        assert_eq!(p.caret, 2);
        assert!(log(&p).contains(&"a".to_string()), "{:?}", log(&p));
        let _ = std::fs::remove_file(path);
    }
}
