//! The accessibility tree: what a screen reader or other assistive technology sees of the page.
//! The engine builds a plain description (`A11yTree`) from the DOM, its styles and layout;
//! an embedder turns it into its platform's API (the `nile` shell uses AccessKit) and sends
//! requests back through [`Page::a11y_action`].
//!
//! Roles and names follow the HTML and ARIA mappings browsers use: elements have implicit roles,
//! `role` overrides them, the accessible name comes from `aria-label`, `aria-labelledby`, `<label>`,
//! `alt`, `title` or the element's own text.

use super::*;

/// What an element is, for assistive technology.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A11yRole {
    Window,
    Document,
    GenericContainer,
    Paragraph,
    Heading,
    Link,
    Button,
    CheckBox,
    RadioButton,
    Switch,
    TextInput,
    MultilineInput,
    NumberInput,
    SearchInput,
    PasswordInput,
    DateInput,
    ColorWell,
    Slider,
    ComboBox,
    ListBox,
    ListBoxOption,
    List,
    ListItem,
    Image,
    Table,
    RowGroup,
    Row,
    Cell,
    ColumnHeader,
    RowHeader,
    ProgressBar,
    Meter,
    Group,
    Dialog,
    AlertDialog,
    Alert,
    Status,
    Menu,
    MenuBar,
    MenuItem,
    Tab,
    TabList,
    TabPanel,
    Toolbar,
    Tooltip,
    Navigation,
    Main,
    Banner,
    Footer,
    Complementary,
    Region,
    Article,
    Form,
    Search,
    Details,
    Disclosure,
    Separator,
    Blockquote,
    Code,
    Figure,
    FigureCaption,
    Label,
    /// A run of text.
    StaticText,
    Iframe,
    Splitter,
}

/// What assistive technology can ask an element to do.
#[derive(Debug, Clone, PartialEq)]
pub enum A11yAction {
    /// Click it (press a button, follow a link, toggle a box, choose an option).
    Click,
    Focus,
    /// Replace the text of an input.
    SetValue(String),
    Increment,
    Decrement,
    /// Scroll so it is visible.
    ScrollIntoView,
}

#[derive(Debug, Clone, PartialEq)]
pub struct A11yNode {
    pub id: u64,
    pub role: A11yRole,
    pub name: String,
    pub description: String,
    pub value: Option<String>,
    /// `(value, min, max, step)` of sliders, spin buttons, progress bars and meters.
    pub numeric: Option<(f64, f64, f64, f64)>,
    pub placeholder: String,
    /// Window coordinates in logical pixels `(x, y, width, height)`.
    pub bounds: (f32, f32, f32, f32),
    pub children: Vec<u64>,
    pub level: u8,
    pub focusable: bool,
    pub clickable: bool,
    pub disabled: bool,
    /// `Some(Some(on))` for checkable things, `Some(None)` for a mixed state.
    pub checked: Option<Option<bool>>,
    pub expanded: Option<bool>,
    pub selected: bool,
    pub required: bool,
    pub read_only: bool,
    pub link_target: String,
}

impl A11yNode {
    fn new(id: u64, role: A11yRole) -> A11yNode {
        A11yNode {
            id,
            role,
            name: String::new(),
            description: String::new(),
            value: None,
            numeric: None,
            placeholder: String::new(),
            bounds: (0.0, 0.0, 0.0, 0.0),
            children: Vec::new(),
            level: 0,
            focusable: false,
            clickable: false,
            disabled: false,
            checked: None,
            expanded: None,
            selected: false,
            required: false,
            read_only: false,
            link_target: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct A11yTree {
    /// Parents come before their children; the first node is the root.
    pub nodes: Vec<A11yNode>,
    pub root: u64,
    /// The node with keyboard focus (the root when none has).
    pub focus: u64,
}

impl A11yTree {
    /// The tree as JSON: `{"root": id, "focus": id, "nodes": [{id, role, name, ...}]}`, for hosts that
    /// bridge to a platform accessibility API themselves (the C API returns this).
    pub fn to_json(&self) -> String {
        use crate::editing::json_string as q;
        let opt = |on: bool, key: &str| if on { format!(",\"{key}\":true") } else { String::new() };
        let mut out = format!("{{\"root\":{},\"focus\":{},\"nodes\":[", self.root, self.focus);
        for (i, n) in self.nodes.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out += &format!("{{\"id\":{},\"role\":{},\"name\":{},\"bounds\":[{},{},{},{}],\"children\":[{}]", n.id, q(&format!("{:?}", n.role)), q(&n.name), n.bounds.0, n.bounds.1, n.bounds.2, n.bounds.3, n.children.iter().map(u64::to_string).collect::<Vec<_>>().join(","));
            if !n.description.is_empty() {
                out += &format!(",\"description\":{}", q(&n.description));
            }
            if let Some(v) = &n.value {
                out += &format!(",\"value\":{}", q(v));
            }
            if let Some((v, min, max, step)) = n.numeric {
                out += &format!(",\"numeric\":[{v},{min},{max},{step}]");
            }
            if n.level > 0 {
                out += &format!(",\"level\":{}", n.level);
            }
            if let Some(c) = n.checked {
                out += &format!(",\"checked\":{}", match c { Some(true) => "true", Some(false) => "false", None => "\"mixed\"" });
            }
            if let Some(e) = n.expanded {
                out += &format!(",\"expanded\":{e}");
            }
            out += &opt(n.focusable, "focusable");
            out += &opt(n.clickable, "clickable");
            out += &opt(n.disabled, "disabled");
            out += &opt(n.selected, "selected");
            out += &opt(n.required, "required");
            out += &opt(n.read_only, "readOnly");
            out.push('}');
        }
        out + "]}"
    }
}

/// Ids: the root is 1, a document node is `NodeId + 2`, and a frame's nodes carry the frame
/// element's id in the high 32 bits.
const ROOT: u64 = 1;
const SHIFT: u32 = 32;

fn id_of(base: u64, node: NodeId) -> u64 {
    base | (node.0 as u64 + 2)
}

struct Ctx<'a> {
    doc: &'a Document,
    styles: &'a StyleMap,
    tree: &'a LayoutTree,
    offsets: ScrollOffsets,
    scroll: f32,
    base: u64,
    out: Vec<A11yNode>,
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Ctx<'_> {
    fn attr(&self, n: NodeId, name: &str) -> Option<&str> {
        self.doc.node(n).attr(name)
    }

    fn hidden(&self, n: NodeId) -> bool {
        let node = self.doc.node(n);
        match node.tag() {
            Some("script" | "style" | "head" | "meta" | "link" | "title" | "template" | "noscript" | "option" | "optgroup" | "col" | "colgroup") => true,
            _ => {
                node.attr("aria-hidden").is_some_and(|v| v.eq_ignore_ascii_case("true"))
                    || node.attr("hidden").is_some()
                    || (!self.styles.is_pseudo(n) && self.styles.get(n).display == nile_style::Display::None)
            }
        }
    }

    fn text(&self, n: NodeId) -> String {
        collapse(&self.doc.text_content(n))
    }

    /// The text a label element or id reference contributes.
    fn text_of_ids(&self, ids: &str) -> String {
        ids.split_whitespace()
            .filter_map(|id| self.doc.find(|n| n.attr("id") == Some(id)))
            .map(|n| self.text(n))
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `<label for=id>` or a wrapping `<label>`.
    fn label_for(&self, n: NodeId) -> String {
        let id = self.attr(n, "id");
        let mut parts = Vec::new();
        for l in self.doc.descendants(Document::ROOT) {
            if self.doc.node(l).tag() != Some("label") {
                continue;
            }
            let wraps = {
                let mut cur = self.doc.node(n).parent;
                let mut found = false;
                while let Some(p) = cur {
                    if p == l {
                        found = true;
                        break;
                    }
                    cur = self.doc.node(p).parent;
                }
                found
            };
            if wraps || id.is_some_and(|i| self.attr(l, "for") == Some(i)) {
                let t = self.text(l);
                if !t.is_empty() {
                    parts.push(t);
                }
            }
        }
        parts.join(" ")
    }

    fn role_of(&self, n: NodeId) -> Option<(A11yRole, u8)> {
        use A11yRole as R;
        let node = self.doc.node(n);
        let tag = node.tag()?;
        let level = |t: &str| t.as_bytes().get(1).map_or(2, |d| d - b'0');
        if let Some(role) = node.attr("role") {
            let first = role.split_whitespace().next().unwrap_or("");
            let mapped = match first {
                "button" | "window-close" | "window-minimize" | "window-maximize" => R::Button,
                "link" => R::Link,
                "checkbox" => R::CheckBox,
                "radio" => R::RadioButton,
                "switch" => R::Switch,
                "textbox" => R::TextInput,
                "searchbox" => R::SearchInput,
                "spinbutton" => R::NumberInput,
                "slider" => R::Slider,
                "combobox" => R::ComboBox,
                "listbox" => R::ListBox,
                "option" => R::ListBoxOption,
                "list" => R::List,
                "listitem" => R::ListItem,
                "menu" => R::Menu,
                "menubar" => R::MenuBar,
                "menuitem" | "menuitemcheckbox" | "menuitemradio" => R::MenuItem,
                "tab" => R::Tab,
                "tablist" => R::TabList,
                "tabpanel" => R::TabPanel,
                "toolbar" => R::Toolbar,
                "tooltip" => R::Tooltip,
                "dialog" => R::Dialog,
                "alertdialog" => R::AlertDialog,
                "alert" => R::Alert,
                "status" => R::Status,
                "navigation" => R::Navigation,
                "main" => R::Main,
                "banner" => R::Banner,
                "contentinfo" => R::Footer,
                "complementary" => R::Complementary,
                "region" => R::Region,
                "article" => R::Article,
                "form" => R::Form,
                "search" => R::Search,
                "img" | "image" => R::Image,
                "heading" => return Some((R::Heading, node.attr("aria-level").and_then(|l| l.parse().ok()).unwrap_or(2))),
                "table" | "grid" | "treegrid" => R::Table,
                "row" => R::Row,
                "rowgroup" => R::RowGroup,
                "cell" | "gridcell" => R::Cell,
                "columnheader" => R::ColumnHeader,
                "rowheader" => R::RowHeader,
                "progressbar" => R::ProgressBar,
                "meter" => R::Meter,
                "group" | "radiogroup" => R::Group,
                "separator" => R::Separator,
                "figure" => R::Figure,
                "paragraph" => R::Paragraph,
                // `presentation`/`none` and unknown roles keep the element's own meaning.
                _ => return self.implicit_role(n, tag, level),
            };
            return Some((mapped, 0));
        }
        self.implicit_role(n, tag, level)
    }

    fn implicit_role(&self, n: NodeId, tag: &str, level: impl Fn(&str) -> u8) -> Option<(A11yRole, u8)> {
        use A11yRole as R;
        let attr = |name: &str| self.attr(n, name);
        Some((
            match tag {
                "a" if attr("href").is_some() => R::Link,
                "button" => R::Button,
                "input" => match attr("type").map(str::to_ascii_lowercase).as_deref() {
                    Some("button" | "submit" | "reset" | "image" | "file") => R::Button,
                    Some("checkbox") => R::CheckBox,
                    Some("radio") => R::RadioButton,
                    Some("range") => R::Slider,
                    Some("number") => R::NumberInput,
                    Some("search") => R::SearchInput,
                    Some("password") => R::PasswordInput,
                    Some("color") => R::ColorWell,
                    Some("date" | "time" | "month" | "week" | "datetime-local") => R::DateInput,
                    Some("hidden") => return None,
                    _ => R::TextInput,
                },
                "textarea" => R::MultilineInput,
                "select" => {
                    if attr("multiple").is_some() || attr("size").and_then(|s| s.parse::<u32>().ok()).is_some_and(|s| s > 1) {
                        R::ListBox
                    } else {
                        R::ComboBox
                    }
                }
                "progress" => R::ProgressBar,
                "meter" => R::Meter,
                "img" => R::Image,
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => return Some((R::Heading, level(tag))),
                "p" => R::Paragraph,
                "ul" | "ol" | "menu" | "dl" => R::List,
                "li" | "dt" | "dd" => R::ListItem,
                "table" => R::Table,
                "thead" | "tbody" | "tfoot" => R::RowGroup,
                "tr" => R::Row,
                "td" => R::Cell,
                "th" => {
                    if attr("scope") == Some("row") {
                        R::RowHeader
                    } else {
                        R::ColumnHeader
                    }
                }
                "nav" => R::Navigation,
                "main" => R::Main,
                "header" => R::Banner,
                "footer" => R::Footer,
                "aside" => R::Complementary,
                "section" if attr("aria-label").is_some() || attr("aria-labelledby").is_some() => R::Region,
                "article" => R::Article,
                "form" => R::Form,
                "fieldset" => R::Group,
                "details" => R::Details,
                "summary" => R::Disclosure,
                "hr" => R::Separator,
                "blockquote" => R::Blockquote,
                "code" | "pre" | "kbd" | "samp" => R::Code,
                "dialog" => R::Dialog,
                "label" => R::Label,
                "figure" => R::Figure,
                "figcaption" => R::FigureCaption,
                "frame" | "iframe" => R::Iframe,
                "splitter" => R::Splitter,
                _ => R::GenericContainer,
            },
            0,
        ))
    }

    /// Roles whose name is their own text.
    fn names_from_content(role: A11yRole) -> bool {
        use A11yRole as R;
        matches!(
            role,
            R::Button | R::Link | R::Heading | R::Cell | R::ColumnHeader | R::RowHeader | R::ListBoxOption | R::MenuItem | R::Tab | R::Tooltip | R::Disclosure | R::Switch | R::CheckBox | R::RadioButton | R::Label | R::FigureCaption
        )
    }

    fn name_of(&self, n: NodeId, role: A11yRole) -> String {
        use A11yRole as R;
        if let Some(l) = self.attr(n, "aria-label").map(collapse).filter(|l| !l.is_empty()) {
            return l;
        }
        if let Some(ids) = self.attr(n, "aria-labelledby") {
            let t = self.text_of_ids(ids);
            if !t.is_empty() {
                return t;
            }
        }
        match role {
            R::Image => return collapse(self.attr(n, "alt").or(self.attr(n, "title")).unwrap_or("")),
            R::TextInput | R::MultilineInput | R::NumberInput | R::SearchInput | R::PasswordInput | R::DateInput | R::ColorWell | R::Slider | R::ComboBox | R::ListBox | R::ProgressBar | R::Meter => {
                let l = self.label_for(n);
                if !l.is_empty() {
                    return l;
                }
                return collapse(self.attr(n, "title").or(self.attr(n, "placeholder")).unwrap_or(""));
            }
            R::Group => {
                // A fieldset is named by its <legend>.
                if let Some(legend) = self.doc.node(n).children.iter().copied().find(|c| self.doc.node(*c).tag() == Some("legend")) {
                    return self.text(legend);
                }
            }
            _ => {}
        }
        if self.doc.node(n).tag() == Some("input") && matches!(role, R::Button) {
            let t = self.doc.node(n).attr("type").unwrap_or("");
            return collapse(self.attr(n, "value").unwrap_or(match t {
                "submit" => "Submit",
                "reset" => "Reset",
                "file" => "Choose File",
                _ => "",
            }));
        }
        if matches!(self.attr(n, "role"), Some("window-close")) {
            return "Close".into();
        }
        if matches!(self.attr(n, "role"), Some("window-minimize")) {
            return "Minimize".into();
        }
        if matches!(self.attr(n, "role"), Some("window-maximize")) {
            return "Maximize".into();
        }
        if matches!(role, R::CheckBox | R::RadioButton | R::Switch) {
            let l = self.label_for(n);
            if !l.is_empty() {
                return l;
            }
        }
        if Self::names_from_content(role) {
            let t = self.text(n);
            if !t.is_empty() {
                return t;
            }
            return collapse(self.attr(n, "title").unwrap_or(""));
        }
        if matches!(role, R::CheckBox | R::RadioButton) {
            let l = self.label_for(n);
            if !l.is_empty() {
                return l;
            }
        }
        collapse(self.attr(n, "title").unwrap_or(""))
    }

    fn bounds(&self, n: NodeId) -> (f32, f32, f32, f32) {
        match self.tree.node_rect_scrolled(n, &self.offsets) {
            Some(r) => (r.x, r.y - self.scroll, r.width, r.height),
            None => (0.0, 0.0, 0.0, 0.0),
        }
    }

    fn has_interesting_descendant(&self, n: NodeId) -> bool {
        self.doc.descendants(n).into_iter().any(|d| {
            let node = self.doc.node(d);
            node.tag().is_some()
                && !self.hidden(d)
                && matches!(node.tag(), Some("img" | "input" | "button" | "select" | "textarea" | "a" | "svg" | "canvas" | "progress" | "meter"))
        })
    }

    /// Build the node for `n` (and its subtree). Returns the ids this element contributes to its
    /// parent: itself, or its children when it is only a wrapper.
    fn visit(&mut self, n: NodeId) -> Vec<u64> {
        use A11yRole as R;
        if self.hidden(n) {
            return Vec::new();
        }
        let node = self.doc.node(n);
        if let nile_dom::NodeKind::Text(t) = &node.kind {
            let text = collapse(t);
            if text.is_empty() {
                return Vec::new();
            }
            let id = id_of(self.base, n);
            let mut s = A11yNode::new(id, R::StaticText);
            s.name = text;
            s.bounds = self.bounds(n);
            self.out.push(s);
            return vec![id];
        }
        let Some((role, level)) = self.role_of(n) else { return Vec::new() };
        let presentational = matches!(self.attr(n, "role"), Some("presentation" | "none"));
        let id = id_of(self.base, n);
        // Plain wrappers without anything to say are dropped; their content moves up.
        let wrapper = role == R::GenericContainer
            && self.attr(n, "aria-label").is_none()
            && self.attr(n, "aria-labelledby").is_none()
            && self.attr(n, "title").is_none()
            && self.attr(n, "tabindex").is_none()
            && !self.attr(n, "contenteditable").is_some_and(|v| !v.eq_ignore_ascii_case("false"))
            || presentational;
        if wrapper {
            let kids: Vec<NodeId> = node.children.clone();
            return kids.into_iter().flat_map(|c| self.visit(c)).collect();
        }
        let slot = self.out.len();
        let mut a = A11yNode::new(id, role);
        a.name = self.name_of(n, role);
        a.level = level;
        a.bounds = self.bounds(n);
        a.description = collapse(self.attr(n, "aria-description").or(self.attr(n, "aria-describedby").map(|_| "")).unwrap_or(""));
        if let Some(ids) = self.attr(n, "aria-describedby") {
            a.description = self.text_of_ids(ids);
        }
        a.disabled = self.doc.has_state(n, "disabled") || self.attr(n, "aria-disabled") == Some("true");
        a.required = self.attr(n, "required").is_some() || self.attr(n, "aria-required") == Some("true");
        a.read_only = self.attr(n, "readonly").is_some();
        a.placeholder = self.attr(n, "placeholder").unwrap_or("").to_string();
        let tag = node.tag().unwrap_or("");
        let editable = self.attr(n, "contenteditable").is_some_and(|v| !v.eq_ignore_ascii_case("false"));
        a.focusable = !a.disabled
            && (matches!(tag, "input" | "button" | "select" | "textarea") || (tag == "a" && self.attr(n, "href").is_some()) || self.attr(n, "tabindex").is_some() || editable || role == R::Disclosure);
        a.clickable = !a.disabled
            && matches!(role, R::Button | R::Link | R::CheckBox | R::RadioButton | R::Switch | R::ListBoxOption | R::MenuItem | R::Tab | R::Disclosure | R::ColorWell);
        if a.clickable && role == R::Link {
            a.link_target = self.attr(n, "href").unwrap_or("").to_string();
        }
        // States.
        match role {
            R::CheckBox | R::RadioButton | R::Switch | R::MenuItem => {
                let aria = self.attr(n, "aria-checked");
                a.checked = match aria {
                    Some("mixed") => Some(None),
                    Some(v) => Some(Some(v == "true")),
                    None if matches!(role, R::MenuItem) => None,
                    None => Some(Some(self.doc.has_state(n, "checked"))),
                };
            }
            _ => {}
        }
        a.expanded = match (role, tag) {
            (_, "details") => Some(self.attr(n, "open").is_some()),
            (R::Disclosure, _) => node.parent.map(|p| self.attr(p, "open").is_some()),
            _ => self.attr(n, "aria-expanded").map(|v| v == "true"),
        };
        a.selected = self.doc.has_state(n, "selected") || self.attr(n, "aria-selected") == Some("true");
        // Values.
        match (role, tag) {
            (R::TextInput | R::SearchInput | R::NumberInput | R::DateInput | R::ColorWell, _) => {
                a.value = Some(self.attr(n, "value").unwrap_or("").to_string());
            }
            (R::PasswordInput, _) => a.value = Some("•".repeat(self.attr(n, "value").unwrap_or("").chars().count())),
            (R::MultilineInput, _) => a.value = Some(self.doc.text_content(n)),
            (R::ComboBox, _) => a.value = self.selected_option_text(n),
            _ => {}
        }
        if matches!(role, R::Slider | R::NumberInput | R::ProgressBar | R::Meter) {
            let num = |name: &str| self.attr(n, name).and_then(|v| v.trim().parse::<f64>().ok());
            let (min, max) = (num("min").unwrap_or(0.0), num("max").unwrap_or(if matches!(role, R::ProgressBar | R::Meter | R::Slider) && role != R::NumberInput { if role == R::Slider { 100.0 } else { 1.0 } } else { f64::MAX }));
            let value = num("value").or(num("aria-valuenow")).unwrap_or(min);
            a.numeric = Some((value, min, if max == f64::MAX { value.max(min) } else { max }, num("step").unwrap_or(1.0)));
        }
        // Children: text-only controls fold their text into the name.
        let collapse_children = Self::names_from_content(role) && !self.has_interesting_descendant(n);
        let mut kids = Vec::new();
        if matches!(tag, "select" | "textarea" | "input") {
            // Their content is part of the control.
        } else if tag == "frame" || tag == "iframe" {
            // Filled in by the caller with the frame's own tree.
        } else if !collapse_children {
            let children: Vec<NodeId> = node.children.clone();
            for c in children {
                kids.extend(self.visit(c));
            }
        }
        if role == R::ListBox || role == R::ComboBox {
            // Options are real nodes for list boxes (they are what the user picks).
            if role == R::ListBox {
                let opts: Vec<NodeId> = self.doc.descendants(n).into_iter().filter(|d| self.doc.node(*d).tag() == Some("option")).collect();
                for o in opts {
                    let oid = id_of(self.base, o);
                    let mut oa = A11yNode::new(oid, R::ListBoxOption);
                    oa.name = self.text(o);
                    oa.selected = self.doc.has_state(o, "selected");
                    oa.disabled = self.doc.has_state(o, "disabled");
                    oa.clickable = !oa.disabled;
                    oa.bounds = a.bounds;
                    self.out.push(oa);
                    kids.push(oid);
                }
            }
        }
        a.children = kids;
        self.out.insert(slot.min(self.out.len()), a);
        // `insert` put the parent before the children it just pushed.
        vec![id]
    }

    fn selected_option_text(&self, select: NodeId) -> Option<String> {
        let options: Vec<NodeId> = self.doc.descendants(select).into_iter().filter(|d| self.doc.node(*d).tag() == Some("option")).collect();
        let chosen = options.iter().find(|o| self.doc.has_state(**o, "selected")).or(options.first())?;
        Some(self.text(*chosen))
    }
}

impl Page {
    /// The page's accessibility tree, with window coordinates in logical pixels.
    pub fn a11y_tree(&mut self) -> A11yTree {
        self.update_frames();
        let focus_node = self.interaction().focus;
        let mut tree = self.a11y_tree_of(0, focus_node);
        // Frames' documents hang below their elements.
        let frame_ids: Vec<NodeId> = self.frames.borrow().slots.keys().copied().collect();
        for fid in frame_ids {
            let (rect, mut sub) = {
                let mut fr = self.frames.borrow_mut();
                let Some(slot) = fr.slots.get_mut(&fid) else { continue };
                let base = (u64::from(fid.0 as u32) + 1) << SHIFT;
                let mut sub = slot.page.a11y_tree_with_base(base);
                if let Some(r) = sub.nodes.first_mut() {
                    r.role = A11yRole::Document;
                }
                (slot.rect, sub)
            };
            let host = id_of(0, fid);
            for n in &mut sub.nodes {
                n.bounds.0 += rect.0;
                n.bounds.1 += rect.1;
            }
            if let Some(h) = tree.nodes.iter_mut().find(|n| n.id == host) {
                h.children.push(sub.root);
            }
            if sub.focus != sub.root && self.frames.borrow().focus == Some(fid) {
                tree.focus = sub.focus;
            }
            tree.nodes.extend(sub.nodes);
        }
        tree
    }

    fn a11y_tree_with_base(&mut self, base: u64) -> A11yTree {
        let focus_node = self.interaction().focus;
        self.a11y_tree_of(base, focus_node)
    }

    fn a11y_tree_of(&mut self, base: u64, focus: Option<NodeId>) -> A11yTree {
        let title = self.doc.borrow().find(|n| n.tag() == Some("title")).map(|t| collapse(&self.doc.borrow().text_content(t))).unwrap_or_default();
        let viewport = self.view.borrow().viewport;
        let mut v = self.view.borrow_mut();
        v.layout();
        let (offsets, scroll) = (v.scroll_offsets.clone(), v.scroll_y);
        let v = &*v;
        let doc = self.doc.borrow();
        let tree = &v.layout.as_ref().expect("laid out above").2;
        let mut cx = Ctx { doc: &doc, styles: &v.styles, tree, offsets, scroll, base, out: Vec::new() };
        let root_id = base | ROOT;
        let body = doc.body().unwrap_or(Document::ROOT);
        let top = cx.visit_body(body);
        let mut root = A11yNode::new(root_id, A11yRole::Window);
        root.name = title;
        root.bounds = (0.0, 0.0, viewport.0, viewport.1);
        root.children = top;
        let mut nodes = vec![root];
        nodes.append(&mut cx.out);
        let focus_id = focus.map(|f| id_of(base, f)).filter(|id| nodes.iter().any(|n| n.id == *id)).unwrap_or(root_id);
        A11yTree { nodes, root: root_id, focus: focus_id }
    }

    /// Carry out a request from assistive technology on the node `id`. True if it did something.
    pub fn a11y_action(&mut self, id: u64, action: A11yAction) -> bool {
        // A frame's nodes carry the frame element in their high bits.
        let high = id >> SHIFT;
        if high != 0 {
            let fid = NodeId((high - 1) as usize);
            let inner = id & ((1 << SHIFT) - 1);
            self.frames.borrow_mut().focus = Some(fid);
            return self.with_frame(fid, |p| p.a11y_action(inner, action)).unwrap_or(false);
        }
        if id < 2 {
            return false;
        }
        let node = NodeId((id - 2) as usize);
        if node.0 >= self.doc.borrow().node_count() {
            return false;
        }
        match action {
            A11yAction::Focus => {
                let target = self.focusable(node).unwrap_or(node);
                let mut next = self.interaction();
                next.focus = Some(target);
                self.set_interaction(next);
                true
            }
            A11yAction::Click => {
                let target = self.focusable(node).filter(|f| self.is_text_input(*f));
                if let Some(t) = target {
                    let mut next = self.interaction();
                    next.focus = Some(t);
                    return self.set_interaction(next);
                }
                let (proceed, changed) = self.emit(node, self.mouse_event("click"));
                changed | (proceed && self.click_default(node))
            }
            A11yAction::SetValue(text) => {
                if !self.is_text_input(node) {
                    return false;
                }
                let mut next = self.interaction();
                next.focus = Some(node);
                self.set_interaction(next);
                self.set_sel(0, self.value_of(node).chars().count());
                self.insert_text(&text)
            }
            A11yAction::Increment | A11yAction::Decrement => {
                let key = if action == A11yAction::Increment { "ArrowUp" } else { "ArrowDown" };
                let target = self.focusable(node).unwrap_or(node);
                let mut next = self.interaction();
                next.focus = Some(target);
                self.set_interaction(next);
                if self.is_range(target) {
                    self.range_key(target, if action == A11yAction::Increment { "ArrowRight" } else { "ArrowLeft" })
                } else {
                    self.default_key(key, ModifiersState::default())
                }
            }
            A11yAction::ScrollIntoView => {
                let Some(r) = self.painted_rect_pub(node) else { return false };
                let mut v = self.view.borrow_mut();
                let (top, h) = (v.scroll_y, v.viewport.1);
                if r.y < top {
                    v.scroll_y = r.y.max(0.0);
                } else if r.y + r.height > top + h {
                    v.scroll_y = (r.y + r.height - h).max(0.0);
                } else {
                    return false;
                }
                v.scroll_dirty = true;
                true
            }
        }
    }
}

impl Ctx<'_> {
    /// The document's content: a `<body>` is a wrapper, anything else is visited normally.
    fn visit_body(&mut self, body: NodeId) -> Vec<u64> {
        let kids: Vec<NodeId> = self.doc.node(body).children.clone();
        kids.into_iter().flat_map(|c| self.visit(c)).collect()
    }
}
