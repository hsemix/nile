//! Native window settings, read from the page's root `<html>` element the way Sciter does
//! (`window-frame`, `window-width`, `window-resizable`, ...; docs.sciter.com/docs/HTML/html-window),
//! and the `role="window-*"` attributes that turn page elements into window chrome.

use nile_dom::{Document, NodeId};

/// A length from a window attribute: CSS px, or a percentage of the desktop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    Px(f32),
    Percent(f32),
}

impl Size {
    /// Resolve against the desktop's extent in the same axis.
    pub fn resolve(self, desktop: f32) -> f32 {
        match self {
            Size::Px(p) => p,
            Size::Percent(p) => desktop * p / 100.0,
        }
    }
}

/// `window-material`: what the system draws behind the page's transparent areas. Nile's own
/// attribute (Sciter has none); needs a page whose background is transparent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Material {
    #[default]
    None,
    /// A frosted blur of what is behind the window (any platform that can).
    Blur,
    /// Windows 10/11 acrylic; elsewhere like `blur`.
    Acrylic,
    /// Windows 11 Mica; macOS window background.
    Mica,
    /// Windows 11 tabbed Mica; macOS titlebar material.
    Tabbed,
    /// macOS sidebar material; elsewhere like `blur`.
    Sidebar,
    /// macOS HUD (dark, translucent) material; elsewhere like `blur`.
    Hud,
}

impl Material {
    fn parse(s: &str) -> Material {
        match s.trim().to_ascii_lowercase().as_str() {
            "blur" | "frosted" => Material::Blur,
            "acrylic" => Material::Acrylic,
            "mica" => Material::Mica,
            "tabbed" | "mica-alt" => Material::Tabbed,
            "sidebar" => Material::Sidebar,
            "hud" => Material::Hud,
            _ => Material::None,
        }
    }
}

/// `window-frame`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Frame {
    /// `default`, `standard`: the system title bar and border.
    #[default]
    Native,
    /// `extended`: the page draws into the title bar (system buttons stay on macOS).
    Extended,
    /// `solid`: no system frame; the page supplies the chrome.
    Solid,
    /// `solid-with-shadow`: like `solid`, with the system's drop shadow around the window.
    SolidShadow,
    /// `transparent`: like `solid`, and the page's transparent areas show the desktop.
    Transparent,
}

/// `window-resizable`: `true`, `false`, or a grab margin measured inward from the edge.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Resizable {
    No,
    #[default]
    Yes,
    Margin(f32),
}

/// `window-state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InitialState {
    #[default]
    Shown,
    Minimized,
    Maximized,
    Fullscreen,
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowConfig {
    pub title: String,
    pub width: Option<Size>,
    pub height: Option<Size>,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    pub resizable: Resizable,
    pub minimizable: bool,
    pub maximizable: bool,
    pub state: InitialState,
    /// `window-alignment` 1..=9 on the numeric keypad's layout (5 centres, 7 is top-left, ...).
    pub alignment: Option<u8>,
    pub frame: Frame,
    /// `window-material`.
    pub material: Material,
    pub icon: Option<String>,
}

fn size(v: &str) -> Option<Size> {
    let v = v.trim();
    match v.strip_suffix('%') {
        Some(p) => p.trim().parse().ok().map(Size::Percent),
        None => v.strip_suffix("px").unwrap_or(v).trim().parse().ok().map(Size::Px),
    }
}

fn px(v: &str) -> Option<f32> {
    match size(v)? {
        Size::Px(p) => Some(p),
        Size::Percent(_) => None,
    }
}

fn boolean(v: Option<&str>, default: bool) -> bool {
    match v.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("false" | "0" | "no") => false,
        Some("true" | "1" | "yes" | "") => true,
        _ => default,
    }
}

impl WindowConfig {
    pub fn from_document(doc: &Document) -> WindowConfig {
        let html = doc.find(|n| n.tag() == Some("html")).unwrap_or(Document::ROOT);
        let attr = |name: &str| doc.node(html).attr(name);
        let title = attr("window-title")
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .or_else(|| document_title(doc))
            .unwrap_or_else(|| "nile".into());
        let resizable = match attr("window-resizable").map(str::trim) {
            None => Resizable::Yes,
            Some(v) if v.eq_ignore_ascii_case("false") => Resizable::No,
            Some(v) if v.eq_ignore_ascii_case("true") || v.is_empty() => Resizable::Yes,
            Some(v) => px(v).map_or(Resizable::Yes, Resizable::Margin),
        };
        WindowConfig {
            title,
            width: attr("window-width").and_then(size),
            height: attr("window-height").and_then(size),
            min_width: attr("window-min-width").and_then(px),
            min_height: attr("window-min-height").and_then(px),
            max_width: attr("window-max-width").and_then(px),
            max_height: attr("window-max-height").and_then(px),
            resizable,
            minimizable: boolean(attr("window-minimizable"), true),
            maximizable: boolean(attr("window-maximizable"), true),
            state: match attr("window-state").map(|s| s.trim().to_ascii_lowercase()).as_deref() {
                Some("minimized") => InitialState::Minimized,
                Some("maximized") => InitialState::Maximized,
                Some("full-screen" | "fullscreen") => InitialState::Fullscreen,
                Some("hidden") => InitialState::Hidden,
                _ => InitialState::Shown,
            },
            alignment: attr("window-alignment").and_then(|a| a.trim().parse::<i32>().ok()).filter(|a| (1..=9).contains(a)).map(|a| a as u8),
            frame: match attr("window-frame").map(|s| s.trim().to_ascii_lowercase()).as_deref() {
                Some("extended") => Frame::Extended,
                Some("solid") => Frame::Solid,
                Some("solid-with-shadow") => Frame::SolidShadow,
                Some("transparent") => Frame::Transparent,
                _ => Frame::Native,
            },
            material: attr("window-material").map_or(Material::None, Material::parse),
            icon: attr("window-icon").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        }
    }

    /// Whether the system draws a drop shadow around a frameless window.
    pub fn shadow(&self) -> bool {
        self.frame == Frame::SolidShadow
    }

    /// Whether the window's pixels carry alpha (a transparent frame or a material behind the page).
    pub fn translucent(&self) -> bool {
        self.frame == Frame::Transparent || self.material != Material::None
    }

    /// Whether the system draws no frame, so the page has to provide drag and resize handling.
    pub fn frameless(&self) -> bool {
        matches!(self.frame, Frame::Solid | Frame::SolidShadow | Frame::Transparent)
    }

    /// Inner size in logical px for a desktop of the given size, defaulting to 900x640 and
    /// kept within the min/max limits.
    pub fn inner_size(&self, desktop: (f32, f32)) -> (f32, f32) {
        let w = self.width.map_or(900.0, |s| s.resolve(desktop.0));
        let h = self.height.map_or(640.0, |s| s.resolve(desktop.1));
        (
            w.clamp(self.min_width.unwrap_or(0.0), self.max_width.unwrap_or(f32::MAX)),
            h.clamp(self.min_height.unwrap_or(0.0), self.max_height.unwrap_or(f32::MAX)),
        )
    }

    /// Top-left of a window of `size` aligned within `desktop` per `window-alignment`.
    pub fn position(&self, size: (f32, f32), desktop: (f32, f32)) -> Option<(f32, f32)> {
        let a = self.alignment?;
        // Keypad layout: 7 8 9 / 4 5 6 / 1 2 3.
        let col = (a - 1) % 3;
        let row = 2 - (a - 1) / 3;
        let at = |i: u8, win: f32, area: f32| match i {
            0 => 0.0,
            1 => ((area - win) / 2.0).max(0.0),
            _ => (area - win).max(0.0),
        };
        Some((at(col, size.0, desktop.0), at(row, size.1, desktop.1)))
    }
}

fn document_title(doc: &Document) -> Option<String> {
    doc.find(|n| n.tag() == Some("title"))
        .map(|t| doc.text_content(t).split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|t| !t.is_empty())
}

/// The title the page currently asks for (`<title>`, changed by `document.title`).
pub fn current_title(doc: &Document, config: &WindowConfig) -> String {
    let html = doc.find(|n| n.tag() == Some("html")).unwrap_or(Document::ROOT);
    // `window-title` only wins while the page has no <title> of its own.
    match document_title(doc) {
        Some(t) => t,
        None => doc.node(html).attr("window-title").map(|t| t.trim().to_string()).unwrap_or_else(|| config.title.clone()),
    }
}

/// What an element with `role="window-*"` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Caption,
    Minimize,
    Maximize,
    Close,
    Icon,
}

pub fn role_of(doc: &Document, node: NodeId) -> Option<Role> {
    match doc.node(node).attr("role")?.trim() {
        "window-caption" => Some(Role::Caption),
        "window-minimize" => Some(Role::Minimize),
        "window-maximize" => Some(Role::Maximize),
        "window-close" => Some(Role::Close),
        "window-icon" => Some(Role::Icon),
        _ => None,
    }
}

/// The nearest window role at or above `node`, with the element that has it. A control
/// inside a caption (a button, an input, a link) keeps its own behaviour.
pub fn role_at(doc: &Document, node: NodeId) -> Option<(NodeId, Role)> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let Some(r) = role_of(doc, n) {
            return Some((n, r));
        }
        if matches!(doc.node(n).tag(), Some("input" | "button" | "select" | "textarea" | "a")) && r_is_interactive(doc, n) {
            return None;
        }
        cur = doc.node(n).parent;
    }
    None
}

fn r_is_interactive(doc: &Document, n: NodeId) -> bool {
    doc.node(n).tag() != Some("a") || doc.node(n).attr("href").is_some()
}

/// Which edge or corner of a frameless window a point is over, as `(horizontal, vertical)`
/// with -1 = left/top, 1 = right/bottom, 0 = neither.
pub fn edge_at(x: f32, y: f32, size: (f32, f32), margin: f32) -> Option<(i8, i8)> {
    let h = if x < margin { -1 } else if x >= size.0 - margin { 1 } else { 0 };
    let v = if y < margin { -1 } else if y >= size.1 - margin { 1 } else { 0 };
    (h != 0 || v != 0).then_some((h, v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(html: &str) -> WindowConfig {
        WindowConfig::from_document(&Document::parse(html))
    }

    #[test]
    fn material_and_transparent_frames_make_a_translucent_window() {
        assert!(!config("<html><body></body></html>").translucent());
        let c = config(r#"<html window-material="Mica"><body></body></html>"#);
        assert_eq!((c.material, c.frame, c.translucent()), (Material::Mica, Frame::Native, true));
        assert!(config(r#"<html window-frame="transparent"></html>"#).translucent());
        assert_eq!(config(r#"<html window-material="nonsense"></html>"#).material, Material::None);
    }

    #[test]
    fn reads_the_root_elements_window_attributes() {
        let c = config(
            r#"<html window-frame="solid-with-shadow" window-width="50%" window-height="480" window-min-width="320px"
                     window-resizable="8px" window-maximizable="false" window-state="maximized" window-alignment="5"
                     window-title="Hi"><head><title>Ignored</title></head><body></body></html>"#,
        );
        assert_eq!(c.title, "Hi");
        assert_eq!((c.width, c.height), (Some(Size::Percent(50.0)), Some(Size::Px(480.0))));
        assert_eq!(c.inner_size((1000.0, 800.0)), (500.0, 480.0));
        assert_eq!(c.min_width, Some(320.0));
        assert_eq!(c.resizable, Resizable::Margin(8.0));
        assert!(c.minimizable && !c.maximizable);
        assert_eq!((c.state, c.frame, c.alignment), (InitialState::Maximized, Frame::SolidShadow, Some(5)));
        assert!(c.shadow() && !config(r#"<html window-frame="solid"></html>"#).shadow());
        assert!(c.frameless());
    }

    #[test]
    fn defaults_and_title_fallbacks() {
        let c = config("<title> My  page </title><p>x</p>");
        assert_eq!(c.title, "My page");
        assert_eq!(c.inner_size((1920.0, 1080.0)), (900.0, 640.0));
        assert_eq!((c.resizable, c.frame, c.state), (Resizable::Yes, Frame::Native, InitialState::Shown));
        assert!(c.minimizable && c.maximizable && !c.frameless());
        assert_eq!(config("<p>x</p>").title, "nile");
        assert_eq!(config(r#"<html window-resizable="false"></html>"#).resizable, Resizable::No);
        // Limits win over the requested size.
        let c = config(r#"<html window-width="2000" window-max-width="1200"></html>"#);
        assert_eq!(c.inner_size((1920.0, 1080.0)).0, 1200.0);
    }

    #[test]
    fn alignment_uses_the_keypad_layout() {
        let at = |a: u8| WindowConfig { alignment: Some(a), ..Default::default() }.position((100.0, 50.0), (500.0, 300.0)).unwrap();
        assert_eq!(at(5), (200.0, 125.0));
        assert_eq!(at(7), (0.0, 0.0));
        assert_eq!(at(9), (400.0, 0.0));
        assert_eq!(at(1), (0.0, 250.0));
        assert_eq!(at(3), (400.0, 250.0));
        assert_eq!(at(2), (200.0, 250.0));
        assert_eq!(at(4), (0.0, 125.0));
    }

    #[test]
    fn roles_skip_controls_inside_a_caption() {
        let doc = Document::parse(
            r#"<div id="bar" role="window-caption"><span id="t">Title</span><button id="b">x</button><a id="a" href="/x">l</a><a id="n">plain</a>
               <button id="c" role="window-close">c</button></div>"#,
        );
        let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
        assert_eq!(role_at(&doc, id("t")).map(|r| r.1), Some(Role::Caption));
        assert_eq!(role_at(&doc, id("b")), None, "buttons inside the caption stay buttons");
        assert_eq!(role_at(&doc, id("a")), None);
        assert_eq!(role_at(&doc, id("n")).map(|r| r.1), Some(Role::Caption), "an anchor without href isn't a control");
        assert_eq!(role_at(&doc, id("c")).map(|r| r.1), Some(Role::Close));
    }

    #[test]
    fn edges_of_a_frameless_window() {
        assert_eq!(edge_at(2.0, 2.0, (100.0, 80.0), 6.0), Some((-1, -1)));
        assert_eq!(edge_at(99.0, 40.0, (100.0, 80.0), 6.0), Some((1, 0)));
        assert_eq!(edge_at(50.0, 79.0, (100.0, 80.0), 6.0), Some((0, 1)));
        assert_eq!(edge_at(50.0, 40.0, (100.0, 80.0), 6.0), None);
    }
}
