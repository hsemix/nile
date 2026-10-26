//! The application menu bar as plain data. A page names a `<menu>` element with
//! `Window.this.menu = element`; the engine describes it (`Page::app_menu`) and the embedder turns
//! that into the platform's menu bar (the `nile` shell does for macOS and Windows), then reports a
//! chosen item back with [`Page::menu_click`]. The markup is Sciter's menu markup:
//!
//! ```html
//! <menu id="app">
//!   <li>File <menu>
//!     <li accesskey="CmdOrCtrl+O">Open…</li>
//!     <li accesskey="CmdOrCtrl+S" disabled>Save</li>
//!     <hr/>
//!     <li role="quit">Quit</li>
//!   </menu></li>
//! </menu>
//! ```

use nile_dom::{Document, NodeId, NodeKind};
use nile_script::UiEvent;
use nile_style::Display;

use crate::{ModifiersState, Page};

/// A keyboard shortcut. `primary` is Cmd on macOS and Ctrl elsewhere; `meta` the Windows or Cmd key
/// when named explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accel {
    /// One lower-case character, or a key name such as `f5`, `enter`, `delete`.
    pub key: String,
    pub primary: bool,
    pub meta: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Accel {
    /// `CmdOrCtrl+Shift+S`, `Ctrl+O`, `Alt+F4`, `^S` (Ctrl). Unknown text gives `None`.
    pub fn parse(text: &str) -> Option<Accel> {
        let mut a = Accel { key: String::new(), primary: false, meta: false, alt: false, shift: false };
        let text = text.trim();
        let (text, caret) = match text.strip_prefix('^') {
            Some(rest) => (rest, true),
            None => (text, false),
        };
        a.primary = caret;
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (key, mods) = parts.split_last()?;
        // A lone "+" key ends in an empty part.
        let key = if key.is_empty() && !mods.is_empty() { "+" } else { key };
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "cmdorctrl" | "commandorcontrol" | "ctrl" | "control" | "primary" => a.primary = true,
                "cmd" | "command" | "meta" | "super" | "win" => a.meta = true,
                "alt" | "option" | "opt" => a.alt = true,
                "shift" => a.shift = true,
                _ => return None,
            }
        }
        if key.is_empty() {
            return None;
        }
        a.key = key.to_ascii_lowercase();
        Some(a)
    }

    /// The shortcut as text (`CmdOrCtrl+Shift+S`): what `Accel::parse` reads back.
    pub fn to_text(&self) -> String {
        let mut parts = Vec::new();
        if self.primary {
            parts.push("CmdOrCtrl".to_string());
        }
        if self.meta {
            parts.push("Meta".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        parts.push(self.key.clone());
        parts.join("+")
    }

    /// Whether a DOM `key` with `mods` is this shortcut (on macOS `primary` is Cmd, elsewhere Ctrl).
    pub fn matches(&self, key: &str, mods: ModifiersState) -> bool {
        let want = self.key.as_str();
        let got = key.to_ascii_lowercase();
        let got = match got.as_str() {
            "arrowleft" => "left".to_string(),
            "arrowright" => "right".to_string(),
            "arrowup" => "up".to_string(),
            "arrowdown" => "down".to_string(),
            "escape" => "esc".to_string(),
            _ => got,
        };
        let mac = cfg!(target_os = "macos");
        let primary_down = if mac { mods.super_key() } else { mods.control_key() };
        let meta_down = if mac { mods.control_key() } else { mods.super_key() };
        got == want && primary_down == self.primary && mods.alt_key() == self.alt && mods.shift_key() == self.shift && (!self.meta || meta_down)
    }
}

/// One entry of a menu.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuItem {
    Separator,
    Item {
        /// The `<li>`'s node id, for `Page::menu_click`.
        id: u64,
        label: String,
        accel: Option<Accel>,
        enabled: bool,
        checked: bool,
        /// A standard behaviour (`copy`, `paste`, `quit`, ...) instead of a `click` event.
        role: Option<String>,
        /// Items of the submenu; empty for a plain item.
        items: Vec<MenuItem>,
    },
}

/// A menu as JSON, for C hosts: `[{"id", "label", "accel"?, "enabled", "checked", "role"?, "items"?},
/// {"separator": true}, ...]` where `items` holds a submenu.
pub fn menu_json(items: &[MenuItem]) -> String {
    use crate::editing::json_string as q;
    let body: Vec<String> = items
        .iter()
        .map(|i| match i {
            MenuItem::Separator => "{\"separator\":true}".to_string(),
            MenuItem::Item { id, label, accel, enabled, checked, role, items } => {
                let mut o = format!("{{\"id\":{id},\"label\":{},\"enabled\":{enabled},\"checked\":{checked}", q(label));
                if let Some(a) = accel {
                    o += &format!(",\"accel\":{}", q(&a.to_text()));
                }
                if let Some(r) = role {
                    o += &format!(",\"role\":{}", q(r));
                }
                if !items.is_empty() {
                    o += &format!(",\"items\":{}", menu_json(items));
                }
                o + "}"
            }
        })
        .collect();
    format!("[{}]", body.join(","))
}

/// The tray icon as the shell needs it: the icon file's bytes, tooltip and menu.
#[derive(Debug, Clone, PartialEq)]
pub struct TrayInfo {
    pub icon: Vec<u8>,
    pub tooltip: String,
    pub items: Vec<MenuItem>,
    pub template: bool,
}

impl Page {
    /// The tray icon the page asked for (`Window.this.tray`), with its menu read from the DOM now.
    pub fn tray(&mut self) -> Option<TrayInfo> {
        let spec = self.view.borrow().tray.clone()?;
        self.view.borrow_mut().sync();
        let v = self.view.borrow();
        let icon = nile_layout::ImageLoader::bytes(&v.images, &spec.icon).unwrap_or_default();
        let doc = self.doc.borrow();
        let items = match spec.menu {
            Some(m) if doc.node(m).tag().is_some() => items_of(&doc, &v.styles, m),
            _ => Vec::new(),
        };
        Some(TrayInfo { icon, tooltip: spec.tooltip, items, template: spec.template })
    }

    /// The dock / taskbar badge text, if the page set one.
    pub fn badge(&self) -> Option<String> {
        self.view.borrow().badge.clone()
    }

    /// The dock / taskbar progress, if the page set one.
    pub fn progress(&self) -> nile_script::Progress {
        self.view.borrow().progress
    }

    /// The tray icon was clicked (`trayclick`) or double-clicked (`traydoubleclick`).
    pub fn tray_event(&mut self, kind: &str) -> bool {
        self.call("__trayEvent", &format!("[\"{kind}\"]")).1
    }

    /// The global hotkeys the page registered (`application.registerHotkey`), as `(id, accelerator)`.
    pub fn hotkeys(&self) -> Vec<(u32, String)> {
        self.view.borrow().hotkeys.clone()
    }

    /// A global hotkey was pressed (`ok` = false: the system refused the registration).
    pub fn hotkey_event(&mut self, id: u32, state: &str) -> bool {
        self.call("__hotkeyEvent", &format!("[{id},\"{state}\"]")).1
    }

    /// The application menu the page chose (`Window.this.menu`), if any.
    pub fn app_menu(&mut self) -> Option<Vec<MenuItem>> {
        let root = self.view.borrow().app_menu?;
        self.view.borrow_mut().sync();
        let v = self.view.borrow();
        let doc = self.doc.borrow();
        doc.node(root).tag()?;
        Some(items_of(&doc, &v.styles, root))
    }

    /// A menu item was chosen. Standard roles act on the page (clipboard and editing roles send the
    /// matching key); the window roles (`quit`, `close`, `minimize`, `zoom`, `about`, ...) are
    /// returned for the embedder; anything else becomes a `click` event on the `<li>`.
    pub fn menu_click(&mut self, id: u64) -> Option<String> {
        let node = NodeId(id as usize);
        {
            let doc = self.doc.borrow();
            if id as usize >= doc.node_count() || doc.node(node).tag() != Some("li") || doc.has_state(node, "disabled") {
                return None;
            }
        }
        let role = self.doc.borrow().node(node).attr("role").map(str::to_ascii_lowercase);
        if let Some(role) = role {
            return self.menu_role(&role);
        }
        // Toggle `checked` on items that have it, the way a checkable menu item flips itself.
        let _ = self.emit(node, UiEvent::Plain("click"));
        None
    }

    /// A standard menu action: the clipboard and editing roles (`undo`, `redo`, `cut`, `copy`, `paste`,
    /// `selectall`) send the matching key to the page; any other role (`quit`, `close`, `minimize`,
    /// `zoom`, `about`, ...) is handed back for the embedder to carry out.
    pub fn menu_role(&mut self, role: &str) -> Option<String> {
        let chord: Option<(&str, bool)> = match role {
            "undo" => Some(("z", false)),
            "redo" => Some(("z", true)),
            "cut" => Some(("x", false)),
            "copy" => Some(("c", false)),
            "paste" => Some(("v", false)),
            "selectall" => Some(("a", false)),
            _ => None,
        };
        let Some((key, shift)) = chord else { return Some(role.to_string()) };
        let mut mods = ModifiersState::default();
        mods |= if cfg!(target_os = "macos") { ModifiersState::SUPER } else { ModifiersState::CONTROL };
        if shift {
            mods |= ModifiersState::SHIFT;
        }
        let (proceed, _) = self.key("keydown", key.to_string(), mods);
        if proceed {
            self.default_key(key, mods);
        }
        self.key("keyup", key.to_string(), mods);
        None
    }

    /// Run a menu item whose shortcut is `key` + `mods`, for platforms where the menu bar doesn't
    /// take shortcuts itself. True if one ran.
    pub fn menu_accelerator(&mut self, key: &str, mods: ModifiersState) -> bool {
        let Some(items) = self.app_menu() else { return false };
        fn find(items: &[MenuItem], key: &str, mods: ModifiersState) -> Option<u64> {
            items.iter().find_map(|i| match i {
                MenuItem::Item { id, accel, enabled: true, items, .. } => {
                    if accel.as_ref().is_some_and(|a| a.matches(key, mods)) && items.is_empty() { Some(*id) } else { find(items, key, mods) }
                }
                _ => None,
            })
        }
        match find(&items, key, mods) {
            Some(id) => {
                self.menu_click(id);
                true
            }
            None => false,
        }
    }
}

fn items_of(doc: &Document, styles: &nile_style::StyleMap, menu: NodeId) -> Vec<MenuItem> {
    let mut out = Vec::new();
    for &kid in &doc.node(menu).children {
        let node = doc.node(kid);
        match node.tag() {
            Some("hr") => {
                if !matches!(out.last(), Some(MenuItem::Separator) | None) {
                    out.push(MenuItem::Separator);
                }
            }
            Some("li") => {
                if node.attr("hidden").is_some() || styles.get(kid).display == Display::None {
                    continue;
                }
                let submenu = node.children.iter().copied().find(|c| doc.node(*c).tag() == Some("menu"));
                let label = match node.attr("label") {
                    Some(l) => l.trim().to_string(),
                    None => label_of(doc, kid),
                };
                out.push(MenuItem::Item {
                    id: kid.0 as u64,
                    label,
                    accel: node.attr("accesskey").and_then(Accel::parse),
                    enabled: !doc.has_state(kid, "disabled"),
                    checked: doc.has_state(kid, "checked"),
                    role: node.attr("role").map(str::to_ascii_lowercase),
                    items: submenu.map(|m| items_of(doc, styles, m)).unwrap_or_default(),
                });
            }
            _ => {}
        }
    }
    while matches!(out.last(), Some(MenuItem::Separator)) {
        out.pop();
    }
    out
}

/// An item's text without its submenu, whitespace collapsed.
fn label_of(doc: &Document, li: NodeId) -> String {
    fn walk(doc: &Document, n: NodeId, out: &mut String) {
        for &k in &doc.node(n).children {
            match &doc.node(k).kind {
                NodeKind::Text(t) => {
                    out.push_str(t);
                    out.push(' ');
                }
                NodeKind::Element { .. } if doc.node(k).tag() != Some("menu") => walk(doc, k, out),
                _ => {}
            }
        }
    }
    let mut s = String::new();
    walk(doc, li, &mut s);
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
