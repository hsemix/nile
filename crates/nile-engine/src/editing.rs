//! Rich-text editing: `contenteditable` hosts. The DOM edits themselves (typing, Enter,
//! deleting, formatting commands, undo) are written in JavaScript (`edit.js` in nile-script)
//! on top of the selection this module keeps; the engine supplies what needs layout: placing
//! the caret from the pointer, moving it up and down and to line edges, drawing it, and the
//! clipboard keys.

use super::*;
use nile_layout::DomPoint;

/// The nearest ancestor-or-self with `contenteditable` that isn't `false`.
pub fn editing_host_of(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let Some(v) = doc.node(n).attr("contenteditable") {
            return (!v.trim().eq_ignore_ascii_case("false")).then_some(n);
        }
        cur = doc.node(n).parent;
    }
    None
}

/// A resize handle being dragged on the selected image.
#[derive(Debug, Clone, Copy)]
pub struct ImageDrag {
    pub node: NodeId,
    /// Which edges move: `(-1, -1)` the top left corner, `(1, 0)` the right edge, ...
    pub handle: (i8, i8),
    /// The image's box and the pointer (page coordinates) when the drag began.
    pub start: (f32, f32, f32, f32),
    pub from: (f32, f32),
}

impl Page {
    /// The image the editing selection covers exactly (clicked, or arrowed onto), if any.
    pub fn selected_image(&self) -> Option<NodeId> {
        let (a, f) = self.edit_selection()?;
        if a.node != f.node || f.offset != a.offset + 1 {
            return None;
        }
        let doc = self.doc.borrow();
        doc.node(a.node).tag()?;
        let kid = *doc.node(a.node).children.get(a.offset)?;
        (doc.node(kid).tag() == Some("img")).then_some(kid)
    }

    /// An element's box where it paints, in page coordinates.
    pub(crate) fn painted_rect_pub(&self, node: NodeId) -> Option<nile_layout::Rect> {
        self.painted_rect(node)
    }

    fn painted_rect(&self, node: NodeId) -> Option<nile_layout::Rect> {
        let mut v = self.view.borrow_mut();
        let offsets = v.scroll_offsets.clone();
        v.layout().node_rect_scrolled(node, &offsets)
    }

    fn pointer_in_page(&self) -> Option<(f32, f32)> {
        self.pointer.map(|(x, y)| (x, y + self.view.borrow().scroll_y))
    }

    /// The selected image and the resize handle under the pointer, if any.
    fn image_handle_at_pointer(&self) -> Option<(NodeId, (i8, i8), nile_layout::Rect)> {
        let img = self.selected_image()?;
        let (px, py) = self.pointer_in_page()?;
        let rect = self.painted_rect(img)?;
        let slack = 2.0;
        nile_layout::handles_of(rect)
            .into_iter()
            .find(|(_, h)| px >= h.x - slack && px <= h.x + h.width + slack && py >= h.y - slack && py <= h.y + h.height + slack)
            .map(|(dir, _)| (img, dir, rect))
    }

    /// The cursor a selected image's handles ask for under the pointer.
    pub fn image_handle_cursor(&self) -> Option<nile_style::Cursor> {
        use nile_style::Cursor;
        let dir = match self.image_drag {
            Some(d) => d.handle,
            None => self.image_handle_at_pointer()?.1,
        };
        Some(match dir {
            (0, _) => Cursor::ResizeV,
            (_, 0) => Cursor::ResizeH,
            (-1, -1) | (1, 1) => Cursor::ResizeNwse,
            _ => Cursor::ResizeNesw,
        })
    }

    /// The selected image's box in page coordinates, to draw its outline and handles.
    pub fn selected_image_rect(&self) -> Option<nile_layout::Rect> {
        let img = self.selected_image()?;
        self.painted_rect(img)
    }

    /// Drag a resize handle: corners keep the proportions (Shift frees them).
    pub fn image_drag_to(&mut self, x: f32, y: f32) -> bool {
        let Some(d) = self.image_drag else { return false };
        let y = y + self.view.borrow().scroll_y;
        let (_, _, w, h) = d.start;
        let (mx, my) = (x - d.from.0, y - d.from.1);
        let mut nw = if d.handle.0 != 0 { w + f32::from(d.handle.0) * mx } else { w };
        let mut nh = if d.handle.1 != 0 { h + f32::from(d.handle.1) * my } else { h };
        if d.handle.0 != 0 && d.handle.1 != 0 && !self.mods.shift_key() {
            let scale = ((nw / w) + (nh / h)) / 2.0;
            (nw, nh) = (w * scale, h * scale);
        }
        let (nw, nh) = (nw.max(8.0).round(), nh.max(8.0).round());
        let (_, changed) = self.call("__imageSet", &format!("[{},{nw},{nh}]", d.node.0));
        changed
    }

    /// The press began on a handle of the selected image, or on an image: select it.
    fn begin_image_interaction(&mut self, host: NodeId) -> bool {
        if let Some((img, handle, rect)) = self.image_handle_at_pointer() {
            let from = self.pointer_in_page().unwrap_or_default();
            self.image_drag = Some(ImageDrag { node: img, handle, start: (rect.x, rect.y, rect.width, rect.height), from });
            let _ = self.call("__imageStart", &format!("[{}]", img.0));
            return true;
        }
        let hit = self.interaction().hover.filter(|n| {
            let doc = self.doc.borrow();
            doc.node(*n).tag() == Some("img") && editing_host_of(&doc, *n) == Some(host)
        });
        let Some(img) = hit else { return false };
        let (parent, index) = {
            let doc = self.doc.borrow();
            let parent = doc.node(img).parent.expect("an image in a host has a parent");
            (parent, doc.node(parent).children.iter().position(|c| *c == img).unwrap_or(0))
        };
        self.edit_drag = None;
        self.set_edit_selection(DomPoint { node: parent, offset: index }, DomPoint { node: parent, offset: index + 1 });
        true
    }

    /// The focused element, if it is an editing host.
    pub fn focused_host(&self) -> Option<NodeId> {
        let f = self.interaction().focus?;
        let doc = self.doc.borrow();
        (editing_host_of(&doc, f) == Some(f) && doc.node(f).attr("contenteditable").is_some()).then_some(f)
    }

    /// The editing selection `(anchor, focus)`.
    pub fn edit_selection(&self) -> Option<(DomPoint, DomPoint)> {
        self.view.borrow().edit_sel
    }

    /// Set the editing selection and tell the page (`selectionchange`) if it moved.
    pub fn set_edit_selection(&mut self, anchor: DomPoint, focus: DomPoint) -> bool {
        let new = Some((anchor, focus));
        if self.view.borrow().edit_sel == new {
            return false;
        }
        self.view.borrow_mut().edit_sel = new;
        self.view.borrow_mut().page_sel = None;
        self.emit(Document::ROOT, UiEvent::Plain("selectionchange"));
        true
    }

    /// The DOM position under the pointer, or the host's start when the page has no text there.
    fn pointer_dom_point(&mut self, host: NodeId) -> Option<DomPoint> {
        let (x, y) = self.pointer?;
        let mut v = self.view.borrow_mut();
        let (scroll, offsets, doc) = (v.scroll_y, v.scroll_offsets.clone(), v.doc.clone());
        let doc = doc.borrow();
        let p = v.layout().dom_point_at(&doc, x, y + scroll, &offsets);
        // Only positions inside the host count.
        Some(p.filter(|p| editing_host_of(&doc, p.node) == Some(host)).unwrap_or(DomPoint { node: host, offset: 0 }))
    }

    /// A press inside an editing host: place the caret (Shift extends, double-click picks a
    /// word, triple-click a paragraph) and start a drag selection.
    pub fn begin_edit_selection(&mut self, host: NodeId) -> bool {
        let clicks = self.register_click();
        self.edit_x = None;
        if self.begin_image_interaction(host) {
            return true;
        }
        let Some(p) = self.pointer_dom_point(host) else { return false };
        let shift = self.mods.shift_key();
        let anchor = self.edit_selection().map(|s| s.0).filter(|_| shift);
        let (a, f) = if clicks >= 2 && !shift {
            let (from, to) = self.word_or_run_around(p, clicks >= 3).unwrap_or((p, p));
            (from, to)
        } else {
            (anchor.unwrap_or(p), p)
        };
        self.edit_drag = Some((a, clicks >= 2));
        self.set_edit_selection(a, f);
        true
    }

    /// The word (or the whole run, for `whole`) around a DOM position, as a pair of positions.
    fn word_or_run_around(&mut self, p: DomPoint, whole: bool) -> Option<(DomPoint, DomPoint)> {
        let mut v = self.view.borrow_mut();
        let doc = v.doc.clone();
        let doc = doc.borrow();
        let tree = v.layout();
        let t = tree.text_point_of(&doc, p)?;
        let (from, to) = tree.word_at(t, whole)?;
        Some((tree.dom_point_of(&doc, from)?, tree.dom_point_of(&doc, to)?))
    }

    /// The pointer moved with the button down inside an editing host: extend the selection.
    pub fn edit_drag_to_pointer(&mut self) -> bool {
        let Some((anchor, _)) = self.edit_drag else { return false };
        let Some(host) = self.focused_host() else { return false };
        let Some(p) = self.pointer_dom_point(host) else { return false };
        self.set_edit_selection(anchor, p);
        true
    }

    /// Caret position `(x, y, height)` in painted coordinates.
    pub fn edit_caret_painted(&mut self, p: DomPoint) -> Option<(f32, f32, f32)> {
        let mut v = self.view.borrow_mut();
        let (offsets, doc) = (v.scroll_offsets.clone(), v.doc.clone());
        let doc = doc.borrow();
        let styles = v.styles.clone();
        v.layout().caret_painted(&doc, &styles, p, &offsets)
    }

    /// Move the focus end of the selection to the painted point `(x, y)`, extending with Shift.
    fn edit_move_to(&mut self, host: NodeId, x: f32, y: f32, extend: bool) -> bool {
        let Some(sel) = self.edit_selection() else { return false };
        let p = {
            let mut v = self.view.borrow_mut();
            let (offsets, doc) = (v.scroll_offsets.clone(), v.doc.clone());
            let doc = doc.borrow();
            v.layout().dom_point_at(&doc, x, y, &offsets).filter(|p| editing_host_of(&doc, p.node) == Some(host))
        };
        let Some(p) = p else { return false };
        self.set_edit_selection(if extend { sel.0 } else { p }, p)
    }

    /// Keys that need layout (up, down, line start and end), clipboard shortcuts, and
    /// everything else, which scripts handle. True if the page changed.
    pub fn edit_dom_key(&mut self, host: NodeId, key: &str, mods: ModifiersState) -> bool {
        let command = mods.control_key() || mods.super_key();
        let shift = mods.shift_key();
        if self.edit_selection().is_none() {
            // First key without a click: start at the beginning of the host.
            let start = DomPoint { node: host, offset: 0 };
            self.set_edit_selection(start, start);
        }
        let Some(sel) = self.edit_selection() else { return false };
        // Command+Left/Right on a Mac: line start and end.
        let (key, command) = match (key, mods.super_key()) {
            ("ArrowLeft", true) => ("Home", false),
            ("ArrowRight", true) => ("End", false),
            _ => (key, command),
        };
        let moved = match key {
            "ArrowUp" | "ArrowDown" if !command => {
                let Some((x, y, h)) = self.edit_caret_painted(sel.1) else { return false };
                let hint = *self.edit_x.get_or_insert(x);
                let ty = if key == "ArrowDown" { y + h * 1.5 } else { y - h * 0.5 };
                self.edit_move_to(host, hint + 0.5, ty, shift);
                true
            }
            "Home" | "End" if !command => {
                let Some((_, y, h)) = self.edit_caret_painted(sel.1) else { return false };
                self.edit_x = None;
                self.edit_move_to(host, if key == "Home" { -1e5 } else { 1e5 }, y + h / 2.0, shift);
                true
            }
            "c" | "C" | "x" | "X" if command => {
                self.copy_selection(key.eq_ignore_ascii_case("x"));
                true
            }
            "v" | "V" if command => {
                self.paste_clipboard();
                true
            }
            _ => false,
        };
        if moved {
            self.reveal_edit_caret();
            return true;
        }
        self.edit_x = None;
        let arg = format!("[{},{},{},{},{}]", json_string(key), shift, mods.control_key(), mods.alt_key(), mods.super_key());
        let (result, _) = self.call("__editKey", &arg);
        let handled = matches!(result.as_deref(), Ok("true"));
        if handled {
            self.reveal_edit_caret();
        }
        handled
    }

    /// `execCommand("copy" | "cut" | "paste")` on the editing selection.
    pub fn clipboard_command(&mut self, kind: &str) {
        match kind {
            "copy" | "cut" => self.copy_selection(kind == "cut"),
            "paste" => self.paste_clipboard(),
            _ => {}
        }
    }

    /// Copy the editing selection as text and HTML; `cut` deletes it afterwards.
    fn copy_selection(&mut self, cut: bool) {
        let text = self.edit_selected_text();
        if text.is_empty() {
            return;
        }
        let html = self.call("__selectionHtml", "[]").0.ok().and_then(|j| unjson_string(&j)).filter(|h| !h.is_empty());
        match html {
            Some(h) => self.clipboard.set_html(&h, &text),
            None => self.clipboard.set_text(&text),
        }
        if cut {
            let _ = self.call("document.execCommand", r#"["delete"]"#);
        }
    }

    /// Paste the clipboard into the editor: its HTML (cleaned to what the editor knows) when it
    /// has some, else its text.
    fn paste_clipboard(&mut self) {
        let text = self.clipboard.text().unwrap_or_default();
        if let Some(html) = self.clipboard.html().filter(|h| !h.trim().is_empty()) {
            let (r, _) = self.call("__editPasteHtml", &format!("[{},{}]", json_string(&html), json_string(&text)));
            if matches!(r.as_deref(), Ok("true")) {
                return;
            }
        }
        if !text.is_empty() {
            let _ = self.call("__editPaste", &format!("[{}]", json_string(&text)));
        }
    }

    /// The selected text of the editing selection.
    pub fn edit_selected_text(&mut self) -> String {
        let Some((a, f)) = self.edit_selection() else { return String::new() };
        let mut v = self.view.borrow_mut();
        let doc = v.doc.clone();
        let doc = doc.borrow();
        let tree = v.layout();
        let (Some(a), Some(f)) = (tree.text_point_of(&doc, a), tree.text_point_of(&doc, f)) else { return String::new() };
        let ranges = tree.selection_ranges(a, f);
        tree.selection_text(&ranges)
    }

    /// What to draw for the caret of the focused editing host: a run and byte, or an empty
    /// element's content box.
    pub fn dom_caret_for_paint(&mut self) -> Option<nile_render::DomCaret> {
        if !self.caret_visible {
            return None;
        }
        let host = self.focused_host()?;
        let (a, f) = self.edit_selection()?;
        if a != f || editing_host_of(&self.doc.borrow(), f.node) != Some(host) {
            return None;
        }
        let mut v = self.view.borrow_mut();
        let doc = v.doc.clone();
        let doc = doc.borrow();
        let styles = v.styles.clone();
        let tree = v.layout();
        if let Some(t) = tree.text_point_of(&doc, f) {
            return Some(nile_render::DomCaret::Run { run: t.run, byte: t.byte });
        }
        let (rect, i) = tree.dom_caret_rect(&doc, &styles, f)?;
        Some(nile_render::DomCaret::Empty { run: i, rect: nile_layout::Rect { x: rect.x, y: rect.y, width: rect.width, height: rect.height } })
    }

    /// Scroll the nearest scrolling ancestor (or the page) so the caret shows.
    pub fn reveal_edit_caret(&mut self) {
        let Some((_, f)) = self.edit_selection() else { return };
        let Some(host) = self.focused_host() else { return };
        let mut v = self.view.borrow_mut();
        v.sync();
        let (doc, styles) = (v.doc.clone(), v.styles.clone());
        let doc = doc.borrow();
        let Some((c, _)) = v.layout().dom_caret_rect(&doc, &styles, f) else { return };
        // The nearest ancestor that scrolls vertically.
        let mut cur = Some(host);
        while let Some(n) = cur {
            if let Some(sb) = v.layout().scroll_box(n).copied() {
                if sb.wheel_y {
                    let oy = v.scroll_offsets.get(&n).map_or(0.0, |o| o.1);
                    let ox = v.scroll_offsets.get(&n).map_or(0.0, |o| o.0);
                    let (top, bottom) = (c.y - sb.clip.y, c.y + c.height - sb.clip.y);
                    let ny = if top < oy { top.max(0.0) } else if bottom > oy + sb.clip.height { bottom - sb.clip.height } else { oy };
                    if ny != oy {
                        v.set_element_scroll(n, ox, ny);
                    }
                    return;
                }
            }
            cur = doc.node(n).parent;
        }
        // No scroller: the page itself.
        let h = v.viewport.1;
        let y = v.scroll_y;
        if c.y < y {
            v.scroll_y = c.y.max(0.0);
        } else if c.y + c.height > y + h {
            v.scroll_y = c.y + c.height - h;
        }
    }
}

/// The string a JSON string literal holds (`"a\\nb"` -> `a<newline>b`).
pub fn unjson_string(json: &str) -> Option<String> {
    let inner = json.trim().strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    let mut pending_high: Option<u32> = None;
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let e = chars.next()?;
        let ch = match e {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'u' => {
                let hex: String = chars.by_ref().take(4).collect();
                let u = u32::from_str_radix(&hex, 16).ok()?;
                if (0xd800..0xdc00).contains(&u) {
                    pending_high = Some(u);
                    continue;
                }
                match pending_high.take() {
                    Some(h) if (0xdc00..0xe000).contains(&u) => char::from_u32(0x10000 + ((h - 0xd800) << 10) + (u - 0xdc00))?,
                    _ => char::from_u32(u).unwrap_or('\u{fffd}'),
                }
            }
            other => other,
        };
        out.push(ch);
    }
    Some(out)
}

/// A string as a JSON literal.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
