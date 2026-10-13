//! `<frame>` and `<iframe>`: a sub-document inside a document (Sciter's `behavior: frame`). Each
//! frame owns a nested [`Page`] with its own DOM, styles, scripts and scrolling. The outer layout
//! only sizes the box (the UA sheet gives it a default size); afterwards the outer page paints the
//! nested page into that rectangle and routes input to it.

use std::{cell::RefCell, collections::BTreeMap, path::PathBuf, rc::Rc, time::Duration};

use nile_dom::NodeId;
use skia_safe::{images, AlphaType, ColorType, Data, ImageInfo, Rect};

use crate::{ModifiersState, Page};

/// One frame element's sub-document.
pub struct FrameSlot {
    pub page: Box<Page>,
    /// What the `src` attribute said when the page was loaded.
    pub src: String,
    /// The content box in the outer viewport's logical pixels.
    pub rect: (f32, f32, f32, f32),
    /// Messages the frame's scripts sent with `window.parent.postMessage`, not yet delivered.
    pub outbox: Rc<RefCell<Vec<String>>>,
    /// Reused pixel buffer for painting.
    buf: Vec<u8>,
}

#[derive(Default)]
pub struct Frames {
    pub slots: BTreeMap<NodeId, FrameSlot>,
    /// The frame keyboard input goes to (the last one pressed in).
    pub focus: Option<NodeId>,
    /// The frame a press began in; it gets the moves and the release.
    pub capture: Option<NodeId>,
    /// The frame under the pointer.
    pub hover: Option<NodeId>,
    /// The frame whose file chooser request is pending.
    pub file_frame: Option<NodeId>,
    /// Frame elements the document has, found again when `Document::version` changes.
    known: Vec<NodeId>,
    known_version: Option<u64>,
    /// Messages the outer page's scripts posted to frames (`frame.postMessage`), not yet delivered.
    posts: Vec<(NodeId, String)>,
}

fn is_frame(tag: Option<&str>) -> bool {
    matches!(tag, Some("frame" | "iframe"))
}

impl Page {
    /// Find frame elements, load or drop their documents and refresh their rectangles. Cheap when
    /// the page has no frames: the document is only scanned after it changed.
    pub fn update_frames(&mut self) {
        let version = self.doc.borrow().version;
        {
            let mut fr = self.frames.borrow_mut();
            if fr.known_version != Some(version) {
                let doc = self.doc.borrow();
                fr.known = doc.descendants(nile_dom::Document::ROOT).into_iter().filter(|&n| is_frame(doc.node(n).tag())).collect();
                fr.known_version = Some(version);
                let known = fr.known.clone();
                fr.slots.retain(|id, _| known.contains(id));
                fr.focus = fr.focus.filter(|id| known.contains(id));
            }
            if fr.known.is_empty() {
                return;
            }
        }
        let (scroll, offsets) = {
            let v = self.view.borrow();
            (v.scroll_y, v.scroll_offsets.clone())
        };
        let known = self.frames.borrow().known.clone();
        let mut created = Vec::new();
        for id in known {
            let (source, border, pad) = {
                let doc = self.doc.borrow();
                let v = self.view.borrow();
                let s = v.styles.get(id);
                let node = doc.node(id);
                let source = match node.attr("srcdoc") {
                    Some(html) => FrameSource::Html(html.to_string(), node.attr("nile-base").map(str::to_string)),
                    None => FrameSource::File(node.attr("src").unwrap_or("").to_string()),
                };
                (source, s.border_width, s.padding)
            };
            let rect = self.view.borrow_mut().layout().node_rect_scrolled(id, &offsets);
            let Some(r) = rect else {
                self.frames.borrow_mut().slots.remove(&id);
                continue;
            };
            let (x, y) = (r.x + border.left + pad.left, r.y - scroll + border.top + pad.top);
            let w = (r.width - border.left - border.right - pad.left - pad.right).max(0.0);
            let h = (r.height - border.top - border.bottom - pad.top - pad.bottom).max(0.0);
            let key = source.key();
            let existing = self.frames.borrow().slots.get(&id).map(|s| s.src.clone());
            if existing.as_deref() != Some(key.as_str()) {
                if existing.is_none() && matches!(&source, FrameSource::File(f) if f.is_empty()) {
                    continue;
                }
                let outbox = Rc::new(RefCell::new(Vec::new()));
                let mut page = self.load_frame_document(&source, (w, h), outbox.clone());
                page.clipboard = Box::new(crate::MemoryClipboard::default());
                self.frames.borrow_mut().slots.insert(id, FrameSlot { page: Box::new(page), src: key, rect: (x, y, w, h), outbox, buf: Vec::new() });
                created.push(id);
            } else if let Some(slot) = self.frames.borrow_mut().slots.get_mut(&id) {
                slot.rect = (x, y, w, h);
                if slot.page.view.borrow().viewport != (w, h) {
                    slot.page.layout(w, h);
                }
            }
        }
        // Sciter's frame events, once the document exists and is complete.
        for id in created {
            self.emit(id, nile_script::UiEvent::Plain("newdocument"));
            self.emit(id, nile_script::UiEvent::Plain("complete"));
        }
    }

    fn load_frame_document(&self, source: &FrameSource, viewport: (f32, f32), outbox: Rc<RefCell<Vec<String>>>) -> Page {
        let dir = self.path.parent().map(PathBuf::from).unwrap_or_default();
        let resolve = |src: &str| {
            let rel = src.strip_prefix("file://").unwrap_or(src);
            if std::path::Path::new(rel).is_absolute() { PathBuf::from(rel) } else { dir.join(rel) }
        };
        match source {
            FrameSource::File(src) => Page::load_embedded(resolve(src), viewport, outbox),
            FrameSource::Html(html, base) => {
                let base = base.as_deref().map(resolve).unwrap_or_else(|| dir.clone());
                Page::html_embedded(html, base, viewport, outbox)
            }
        }
    }

    /// The frame under a viewport point (the last one in document order wins).
    pub fn frame_at(&mut self, x: f32, y: f32) -> Option<NodeId> {
        self.update_frames();
        if self.view.borrow().menu.is_some() {
            return None;
        }
        self.frames.borrow().slots.iter().rev().find(|(_, s)| {
            let (fx, fy, w, h) = s.rect;
            x >= fx && y >= fy && x < fx + w && y < fy + h
        }).map(|(&id, _)| id)
    }

    /// Run `f` on a frame's page with the outer clipboard and modifier keys lent to it.
    pub(crate) fn with_frame<R>(&mut self, id: NodeId, f: impl FnOnce(&mut Page) -> R) -> Option<R> {
        let mods = self.mods;
        let mut fr = self.frames.try_borrow_mut().ok()?;
        let slot = fr.slots.get_mut(&id)?;
        std::mem::swap(&mut self.clipboard, &mut slot.page.clipboard);
        slot.page.mods = mods;
        let out = f(&mut slot.page);
        std::mem::swap(&mut self.clipboard, &mut slot.page.clipboard);
        Some(out)
    }

    fn local(&self, id: NodeId, x: f32, y: f32) -> (f32, f32) {
        let (fx, fy, ..) = self.frames.borrow().slots[&id].rect;
        (x - fx, y - fy)
    }

    fn outer_dragging(&self) -> bool {
        self.bar_drag.is_some() || self.range_drag.is_some() || self.list_drag.is_some() || self.edit_drag.is_some() || self.page_drag || self.drag.is_some()
    }

    /// Pointer routing for [`Page::pointer_moved`]. True if a frame needs repainting.
    pub(crate) fn frames_pointer_moved(&mut self, x: f32, y: f32) -> bool {
        let captured = self.frames.borrow().capture.filter(|c| self.frames.borrow().slots.contains_key(c));
        let over = match captured {
            Some(c) => Some(c),
            None if self.outer_dragging() => None,
            None => self.frame_at(x, y),
        };
        let mut changed = false;
        let old = self.frames.borrow().hover;
        if old != over {
            if let Some(old) = old {
                changed |= self.with_frame(old, |p| p.pointer_left()).unwrap_or(false);
            }
            self.frames.borrow_mut().hover = over;
        }
        if let Some(id) = over {
            let (lx, ly) = self.local(id, x, y);
            changed |= self.with_frame(id, |p| p.pointer_moved(lx, ly)).unwrap_or(false);
        }
        changed
    }

    pub(crate) fn frames_pointer_left(&mut self) -> bool {
        let (old, captured) = {
            let mut fr = self.frames.borrow_mut();
            (fr.hover.take(), fr.capture.is_some())
        };
        match old {
            Some(old) if !captured => self.with_frame(old, |p| p.pointer_left()).unwrap_or(false),
            _ => false,
        }
    }

    /// A press: the frame under the pointer takes keyboard focus and the rest of the gesture.
    pub(crate) fn frames_press(&mut self) -> bool {
        let over = self.pointer.and_then(|(x, y)| self.frame_at(x, y));
        {
            let mut fr = self.frames.borrow_mut();
            fr.focus = over;
            fr.capture = over;
        }
        match over {
            Some(id) => self.with_frame(id, |p| p.press()).unwrap_or(false),
            None => false,
        }
    }

    pub(crate) fn frames_release(&mut self) -> bool {
        let captured = self.frames.borrow_mut().capture.take();
        match captured {
            Some(id) => self.with_frame(id, |p| p.release()).unwrap_or(false),
            None => false,
        }
    }

    /// A wheel over a frame scrolls it; true if it moved, otherwise the outer page takes the wheel.
    pub(crate) fn frames_wheel(&mut self, dx: f32, dy: f32) -> bool {
        let Some((x, y)) = self.pointer else { return false };
        let Some(id) = self.frame_at(x, y) else { return false };
        self.with_frame(id, |p| {
            let before = (p.view.borrow().scroll_y, p.view.borrow().scroll_offsets.clone());
            p.wheel(dx, dy);
            let v = p.view.borrow();
            before.0 != v.scroll_y || before.1 != v.scroll_offsets
        })
        .unwrap_or(false)
    }

    /// The frame keyboard input goes to, if one has focus.
    pub(crate) fn focused_frame(&self) -> Option<NodeId> {
        let fr = self.frames.borrow();
        fr.focus.filter(|id| fr.slots.contains_key(id))
    }

    pub(crate) fn frames_key(&mut self, kind: &'static str, key: &str, mods: ModifiersState) -> Option<(bool, bool)> {
        let id = self.focused_frame()?;
        self.with_frame(id, |p| p.key(kind, key.to_string(), mods))
    }

    /// Tick every frame's page and deliver the messages between pages; wake again when the soonest wants to.
    pub(crate) fn frames_tick(&mut self) -> (bool, Option<Duration>) {
        self.update_frames();
        let mut changed = false;
        let mut next: Option<Duration> = None;
        let ids: Vec<NodeId> = self.frames.borrow().slots.keys().copied().collect();
        for id in ids {
            if let Some((c, n)) = self.with_frame(id, |p| p.tick()) {
                changed |= c;
                next = [next, n].into_iter().flatten().min();
            }
        }
        changed |= self.deliver_frame_messages();
        (changed, next)
    }

    /// Hand `postMessage` data across: frames' `window.parent.postMessage` to this page's `message`
    /// listeners, and `frame.postMessage` to the frame's own. True if anything was delivered.
    pub(crate) fn deliver_frame_messages(&mut self) -> bool {
        let Ok(mut fr) = self.frames.try_borrow_mut() else { return false };
        let up: Vec<(NodeId, String)> = fr.slots.iter().flat_map(|(&id, s)| s.outbox.borrow_mut().drain(..).map(move |m| (id, m)).collect::<Vec<_>>()).collect();
        let down = std::mem::take(&mut fr.posts);
        drop(fr);
        let delivered = !up.is_empty() || !down.is_empty();
        for (id, json) in down {
            let _ = self.with_frame(id, |p| p.call("__frameMessage", &format!("[{json}]")));
        }
        for (id, json) in up {
            let _ = self.call("__frameMessage", &format!("[{},{json}]", id.0));
        }
        delivered
    }

    /// Paint every frame's page over its rectangle (after the outer page's own painting).
    pub(crate) fn paint_frames(&mut self, canvas: &skia_safe::Canvas, scale: f32) {
        for slot in self.frames.borrow_mut().slots.values_mut() {
            let (x, y, w, h) = slot.rect;
            let (pw, ph) = ((w * scale).round() as u32, (h * scale).round() as u32);
            if pw == 0 || ph == 0 {
                continue;
            }
            slot.buf.resize(pw as usize * ph as usize * 4, 0);
            slot.page.paint(&mut slot.buf, pw, ph, scale);
            let info = ImageInfo::new((pw as i32, ph as i32), ColorType::BGRA8888, AlphaType::Premul, None);
            if let Some(image) = images::raster_from_data(&info, Data::new_copy(&slot.buf), pw as usize * 4) {
                canvas.save();
                canvas.reset_matrix();
                let (px, py) = (x * scale, y * scale);
                canvas.clip_rect(Rect::from_xywh(px, py, pw as f32, ph as f32), None, false);
                canvas.draw_image(image, (px, py), None);
                canvas.restore();
            }
        }
    }
}

/// `[12,"text"]`: a node id and a string, as `host.name(id, text)` sends them.
fn id_and_text(args: &str) -> Option<(usize, String)> {
    let inner = args.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (id, text) = inner.split_once(',')?;
    Some((id.trim().parse().ok()?, crate::editing::unjson_string(text)?))
}

/// Where a frame's document comes from.
enum FrameSource {
    File(String),
    /// `srcdoc` (what `loadHtml` sets) and the folder its relative paths resolve against.
    Html(String, Option<String>),
}

impl FrameSource {
    /// Changes when the frame has to load again.
    fn key(&self) -> String {
        match self {
            FrameSource::File(s) => format!("src:{s}"),
            FrameSource::Html(h, b) => format!("html:{}:{h}", b.as_deref().unwrap_or("")),
        }
    }
}

/// The script functions that reach frames: evaluating code in one and `frame.postMessage`.
pub(crate) fn register_frame_functions(host: &nile_script::ScriptHost, frames: &Rc<RefCell<Frames>>) {
    let eval_frames = frames.clone();
    host.register_host_fn("__frameEval", move |args| {
        let Some((id, code)) = id_and_text(args) else { return r#"{"ok":false,"error":"bad arguments"}"#.into() };
        let Ok(mut fr) = eval_frames.try_borrow_mut() else { return r#"{"ok":false,"error":"frame is busy"}"#.into() };
        match fr.slots.get_mut(&NodeId(id)) {
            None => r#"{"ok":false,"error":"the frame has no document"}"#.into(),
            Some(slot) => match slot.page.eval(&code).0 {
                Ok(v) => format!(r#"{{"ok":true,"v":{}}}"#, crate::editing::json_string(&v)),
                Err(e) => format!(r#"{{"ok":false,"error":{}}}"#, crate::editing::json_string(&e)),
            },
        }
    });
    let post_frames = frames.clone();
    host.register_host_fn("__framePost", move |args| {
        if let (Some((id, json)), Ok(mut fr)) = (id_and_text(args), post_frames.try_borrow_mut()) {
            fr.posts.push((NodeId(id), json));
        }
        String::new()
    });
}
