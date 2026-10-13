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

impl Page {
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
                let text = self.edit_selected_text();
                if !text.is_empty() {
                    self.clipboard.set_text(&text);
                    if key.eq_ignore_ascii_case("x") {
                        let _ = self.call("document.execCommand", r#"["delete"]"#);
                    }
                }
                true
            }
            "v" | "V" if command => {
                if let Some(t) = self.clipboard.text() {
                    let arg = format!("[{}]", json_string(&t));
                    let _ = self.call("__editPaste", &arg);
                }
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
            "copy" | "cut" => {
                let text = self.edit_selected_text();
                if !text.is_empty() {
                    self.clipboard.set_text(&text);
                    if kind == "cut" {
                        let _ = self.call("document.execCommand", r#"["delete"]"#);
                    }
                }
            }
            "paste" => {
                if let Some(t) = self.clipboard.text() {
                    let _ = self.call("__editPaste", &format!("[{}]", json_string(&t)));
                }
            }
            _ => {}
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
