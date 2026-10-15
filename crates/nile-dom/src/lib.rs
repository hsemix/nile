//! Lightweight arena DOM.
//!
//! Nodes live in a flat `Vec` and refer to each other by [`NodeId`]. This keeps
//! the tree cheap to walk from layout/paint and easy to expose to script later.
//!
//! Parsing currently goes through `markup5ever_rcdom` and is converted into the
//! arena. TODO: implement html5ever's `TreeSink` directly to skip the copy.

use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub usize);

#[derive(Debug, Clone)]
pub enum NodeKind {
    Document,
    Element { tag: String, attrs: Vec<(String, String)> },
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub kind: NodeKind,
}

impl Node {
    pub fn tag(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Element { tag, .. } => Some(tag),
            _ => None,
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        match &self.kind {
            NodeKind::Element { attrs, .. } => {
                attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
            }
            _ => None,
        }
    }
}

/// Flags scripts can set on elements, matched by the same-named pseudo-classes.
pub const STATE_NAMES: &[&str] =
    &["checked", "disabled", "readonly", "expanded", "collapsed", "current", "selected", "invalid", "busy", "valid"];

#[derive(Debug, Default)]
pub struct Document {
    pub nodes: Vec<Node>,
    /// Bumped by every mutation, so callers can tell when to restyle.
    // TODO: detached nodes stay in `nodes` forever; compact or free-list them
    // if scripts that create lots of short-lived nodes become common.
    pub version: u64,
    /// Runtime state flags (Sciter's `Element.state`), as bits of `STATE_NAMES`.
    states: std::collections::HashMap<NodeId, u32>,
}

mod include;
mod stylelink;
pub mod vfs;
pub use include::expand as expand_includes;
pub use stylelink::{inline_imports, inline_links};
use include::rewrite_frames;

impl Document {
    pub const ROOT: NodeId = NodeId(0);

    pub fn parse(html: &str) -> Document {
        let html = rewrite_frames(html);
        let dom = html5ever::parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .expect("reading from a byte slice cannot fail");

        let mut doc = Document::default();
        doc.convert(&dom.document, None);
        doc
    }

    fn convert(&mut self, handle: &Handle, parent: Option<NodeId>) {
        let kind = match &handle.data {
            NodeData::Document => NodeKind::Document,
            NodeData::Element { name, attrs, .. } => NodeKind::Element {
                // `rewrite_frames` hid `<frame>` from the HTML parser behind this name.
                tag: match &*name.local {
                    "nile-frame" => "frame".to_string(),
                    "nile-frameset" => "frameset".to_string(),
                    other => other.to_string(),
                },
                attrs: attrs
                    .borrow()
                    .iter()
                    .map(|a| (a.name.local.to_string(), a.value.to_string()))
                    .collect(),
            },
            NodeData::Text { contents } => NodeKind::Text(contents.borrow().to_string()),
            // Comments, doctypes and processing instructions are dropped.
            _ => return,
        };

        let id = NodeId(self.nodes.len());
        self.nodes.push(Node { parent, children: Vec::new(), kind });
        if let Some(p) = parent {
            self.nodes[p.0].children.push(id);
        }
        for child in handle.children.borrow().iter() {
            self.convert(child, Some(id));
        }
    }

    /// How many nodes the arena holds (detached ones included).
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    /// The `<body>` element, if present.
    pub fn body(&self) -> Option<NodeId> {
        self.find(|n| n.tag() == Some("body"))
    }

    pub fn find(&self, pred: impl Fn(&Node) -> bool) -> Option<NodeId> {
        self.nodes.iter().position(pred).map(NodeId)
    }

    /// `<script>` elements in document order: classic (`Inline`/`External`) and
    /// `type="module"`. Other types (JSON, templates) are skipped.
    pub fn script_sources(&self) -> Vec<ScriptSource> {
        self.descendants(Document::ROOT)
            .into_iter()
            .filter(|id| self.node(*id).tag() == Some("script"))
            .filter_map(|id| {
                let ty = self.node(id).attr("type").unwrap_or("").trim().to_ascii_lowercase();
                let module = ty == "module";
                if !(module || ty.is_empty() || ty.contains("javascript") || ty.contains("ecmascript")) {
                    return None;
                }
                Some(match (self.node(id).attr("src"), module) {
                    (Some(src), false) => ScriptSource::External(src.to_string()),
                    (Some(src), true) => ScriptSource::ModuleExternal(src.to_string()),
                    (None, false) => ScriptSource::Inline(self.text_content(id)),
                    (None, true) => ScriptSource::ModuleInline(self.text_content(id)),
                })
            })
            .collect()
    }

    /// `id`'s descendants in document order (not including `id`).
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.node(id).children.iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.node(n).children.iter().rev());
        }
        out
    }

    /// True if `id` is reachable from the document root.
    pub fn is_attached(&self, mut id: NodeId) -> bool {
        loop {
            if id == Document::ROOT {
                return true;
            }
            match self.node(id).parent {
                Some(p) => id = p,
                None => return false,
            }
        }
    }

    /// DOM `textContent`: all descendant text, or the node's own text.
    pub fn text_content(&self, id: NodeId) -> String {
        match &self.node(id).kind {
            NodeKind::Text(t) => t.clone(),
            _ => self
                .descendants(id)
                .into_iter()
                .filter_map(|d| match &self.node(d).kind {
                    NodeKind::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect(),
        }
    }

    /// Whether a runtime state flag is on. `checked`, `disabled`, `readonly` and
    /// `selected` are also on when the HTML attribute is present, as in browsers.
    pub fn has_state(&self, id: NodeId, name: &str) -> bool {
        let Some(bit) = STATE_NAMES.iter().position(|n| *n == name) else { return false };
        self.states.get(&id).is_some_and(|f| f & (1 << bit) != 0)
            || (matches!(name, "checked" | "disabled" | "readonly" | "selected") && self.node(id).attr(name).is_some())
    }

    pub fn set_state(&mut self, id: NodeId, name: &str, on: bool) {
        let Some(bit) = STATE_NAMES.iter().position(|n| *n == name) else { return };
        let flags = self.states.entry(id).or_default();
        let before = *flags;
        if on {
            *flags |= 1 << bit;
        } else {
            *flags &= !(1 << bit);
        }
        if *flags != before {
            self.version += 1;
        }
    }

    // ---- mutation -------------------------------------------------------

    fn push_node(&mut self, kind: NodeKind) -> NodeId {
        self.version += 1;
        self.nodes.push(Node { parent: None, children: Vec::new(), kind });
        NodeId(self.nodes.len() - 1)
    }

    /// A detached element. Tag names are lowercased, as for HTML documents.
    pub fn create_element(&mut self, tag: &str) -> NodeId {
        self.push_node(NodeKind::Element { tag: tag.to_ascii_lowercase(), attrs: Vec::new() })
    }

    pub fn create_text(&mut self, text: &str) -> NodeId {
        self.push_node(NodeKind::Text(text.to_string()))
    }

    /// Remove `id` from its parent (it stays in the arena, detached).
    pub fn detach(&mut self, id: NodeId) {
        if let Some(p) = self.nodes[id.0].parent.take() {
            self.nodes[p.0].children.retain(|c| *c != id);
            self.version += 1;
        }
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        self.insert_before(parent, child, None)
    }

    /// Insert `child` under `parent` before `before` (or at the end). Moves the
    /// child if it already has a parent.
    pub fn insert_before(
        &mut self,
        parent: NodeId,
        child: NodeId,
        before: Option<NodeId>,
    ) -> Result<(), DomError> {
        if matches!(self.node(parent).kind, NodeKind::Text(_)) {
            return Err(DomError::HierarchyRequest);
        }
        // A node can't become its own ancestor.
        let mut cur = Some(parent);
        while let Some(n) = cur {
            if n == child {
                return Err(DomError::HierarchyRequest);
            }
            cur = self.node(n).parent;
        }
        if let Some(b) = before {
            if self.node(b).parent != Some(parent) {
                return Err(DomError::NotFound);
            }
        }
        self.detach(child);
        let at = before
            .and_then(|b| self.node(parent).children.iter().position(|c| *c == b))
            .unwrap_or(self.node(parent).children.len());
        self.nodes[parent.0].children.insert(at, child);
        self.nodes[child.0].parent = Some(parent);
        self.version += 1;
        Ok(())
    }

    pub fn set_attr(&mut self, id: NodeId, name: &str, value: &str) {
        let name = name.to_ascii_lowercase();
        if let NodeKind::Element { attrs, .. } = &mut self.nodes[id.0].kind {
            match attrs.iter_mut().find(|(k, _)| *k == name) {
                Some((_, v)) => *v = value.to_string(),
                None => attrs.push((name, value.to_string())),
            }
            self.version += 1;
        }
    }

    pub fn remove_attr(&mut self, id: NodeId, name: &str) {
        let name = name.to_ascii_lowercase();
        if let NodeKind::Element { attrs, .. } = &mut self.nodes[id.0].kind {
            let before = attrs.len();
            attrs.retain(|(k, _)| *k != name);
            if attrs.len() != before {
                self.version += 1;
            }
        }
    }

    /// DOM `textContent = ...`: replace children with one text node (or set a
    /// text node's data).
    pub fn set_text_content(&mut self, id: NodeId, text: &str) {
        if let NodeKind::Text(t) = &mut self.nodes[id.0].kind {
            *t = text.to_string();
            self.version += 1;
            return;
        }
        for c in std::mem::take(&mut self.nodes[id.0].children) {
            self.nodes[c.0].parent = None;
        }
        if !text.is_empty() {
            let t = self.create_text(text);
            let _ = self.append_child(id, t);
        }
        self.version += 1;
    }

    /// DOM `innerHTML = ...`.
    pub fn set_inner_html(&mut self, id: NodeId, html: &str) {
        self.set_text_content(id, "");
        // Parse in a body context. TODO: use html5ever's fragment parser so
        // table rows and friends keep their context.
        let parsed = Document::parse(&format!("<!doctype html><body>{html}"));
        if let Some(body) = parsed.body() {
            for &c in &parsed.node(body).children {
                let copy = self.import(&parsed, c);
                let _ = self.append_child(id, copy);
            }
        }
    }

    /// Deep-copy `src`'s node `id` into this document, detached.
    fn import(&mut self, src: &Document, id: NodeId) -> NodeId {
        let n = self.push_node(src.node(id).kind.clone());
        for &c in &src.node(id).children {
            let copy = self.import(src, c);
            self.nodes[copy.0].parent = Some(n);
            self.nodes[n.0].children.push(copy);
        }
        n
    }

    /// DOM `innerHTML` (serialized children).
    pub fn inner_html(&self, id: NodeId) -> String {
        let mut out = String::new();
        for &c in &self.node(id).children {
            self.serialize(c, &mut out);
        }
        out
    }

    fn serialize(&self, id: NodeId, out: &mut String) {
        const VOID: &[&str] =
            &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];
        match &self.node(id).kind {
            NodeKind::Text(t) => {
                let raw = self.node(id).parent.is_some_and(|p| matches!(self.node(p).tag(), Some("script" | "style")));
                if raw {
                    out.push_str(t);
                } else {
                    escape(t, false, out);
                }
            }
            NodeKind::Element { tag, attrs } => {
                out.push('<');
                out.push_str(tag);
                for (k, v) in attrs {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    escape(v, true, out);
                    out.push('"');
                }
                out.push('>');
                if !VOID.contains(&tag.as_str()) {
                    for &c in &self.node(id).children {
                        self.serialize(c, out);
                    }
                    out.push_str("</");
                    out.push_str(tag);
                    out.push('>');
                }
            }
            NodeKind::Document => {}
        }
    }
}

fn escape(s: &str, attr: bool, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' if !attr => out.push_str("&lt;"),
            '>' if !attr => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptSource {
    Inline(String),
    External(String),
    ModuleInline(String),
    ModuleExternal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomError {
    /// Inserting here would break the tree (cycle, or children under text).
    HierarchyRequest,
    /// The reference node isn't a child of the parent.
    NotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_and_serialization() {
        let mut doc = Document::parse(r#"<div id="a">hi</div>"#);
        let v0 = doc.version;
        let div = doc.find(|n| n.tag() == Some("div")).unwrap();
        let span = doc.create_element("SPAN");
        doc.set_attr(span, "class", "x y");
        doc.set_text_content(span, "a < b");
        doc.append_child(div, span).unwrap();
        assert!(doc.version > v0);
        assert_eq!(doc.inner_html(div), r#"hi<span class="x y">a &lt; b</span>"#);
        assert_eq!(doc.text_content(div), "hia < b");
        // Cycles are rejected; moving detaches from the old parent.
        assert_eq!(doc.append_child(span, div), Err(DomError::HierarchyRequest));
        doc.append_child(Document::ROOT, span).unwrap();
        assert_eq!(doc.inner_html(div), "hi");
        doc.set_inner_html(div, "<b>x</b><br>y");
        assert_eq!(doc.inner_html(div), "<b>x</b><br>y");
        let loose = doc.create_element("p");
        assert!(!doc.is_attached(loose));
    }

    #[test]
    fn finds_scripts_and_skips_other_types() {
        let doc = Document::parse(
            r#"<script>a()</script><script src="x.js"></script><script type="module">m()</script>
               <script type="module" src="m.js"></script>
               <script type="text/javascript">b()</script><script type="application/json">{}</script>"#,
        );
        assert_eq!(
            doc.script_sources(),
            vec![
                ScriptSource::Inline("a()".into()),
                ScriptSource::External("x.js".into()),
                ScriptSource::ModuleInline("m()".into()),
                ScriptSource::ModuleExternal("m.js".into()),
                ScriptSource::Inline("b()".into())
            ]
        );
    }

    #[test]
    fn state_flags() {
        let mut doc = Document::parse(r#"<input id="a" disabled><input id="b">"#);
        let a = doc.find(|n| n.attr("id") == Some("a")).unwrap();
        let b = doc.find(|n| n.attr("id") == Some("b")).unwrap();
        assert!(doc.has_state(a, "disabled") && !doc.has_state(b, "disabled"));
        let v = doc.version;
        doc.set_state(b, "checked", true);
        assert!(doc.has_state(b, "checked") && doc.version > v);
        doc.set_state(b, "checked", false);
        assert!(!doc.has_state(b, "checked") && !doc.has_state(b, "nonsense"));
    }

    #[test]
    fn parses_body_and_attributes() {
        let doc = Document::parse(r#"<div id="a" style="color: red">hi</div>"#);
        let div = doc.find(|n| n.tag() == Some("div")).unwrap();
        assert_eq!(doc.node(div).attr("id"), Some("a"));
        assert!(doc.body().is_some());
    }
}
