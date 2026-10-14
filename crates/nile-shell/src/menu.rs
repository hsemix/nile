//! The application menu bar. macOS: an `NSMenu` built from the page's `<menu>` (see
//! `nile_engine::menus`) with the standard App, Edit and Window menus around it. Windows: a Win32
//! menu on the window. Elsewhere: nothing (the page draws its own).

#[cfg(target_os = "macos")]
pub use mac::NativeMenu;

/// The standard action behind a negative menu tag (the Edit menu's items and Quit).
pub fn role_of_tag(tag: isize) -> Option<&'static str> {
    Some(match tag {
        -1 => "undo",
        -2 => "redo",
        -3 => "cut",
        -4 => "copy",
        -5 => "paste",
        -6 => "selectall",
        -7 => "quit",
        _ => return None,
    })
}

#[cfg(target_os = "windows")]
pub use win::{install_hook, NativeMenu};

/// No native menu bar on this platform: the page draws its own.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct NativeMenu;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl NativeMenu {
    pub fn new(_proxy: winit::event_loop::EventLoopProxy<crate::app::UserEvent>) -> Option<NativeMenu> {
        None
    }
    pub fn set(&mut self, _window: &winit::window::Window, _app_name: &str, _items: Option<Vec<nile_engine::menus::MenuItem>>) {}
}

/// Win32 menu bars: one `HMENU` per window, commands arriving as `WM_COMMAND` through winit's
/// message hook. Written without a Windows machine to hand: CI is where it gets compiled.
#[cfg(target_os = "windows")]
mod win {
    use std::{collections::HashMap, sync::{Mutex, OnceLock}};

    use nile_engine::menus::{Accel, MenuItem};
    use winit::{
        event_loop::{EventLoopBuilder, EventLoopProxy},
        platform::windows::EventLoopBuilderExtWindows,
        raw_window_handle::{HasWindowHandle, RawWindowHandle},
        window::Window,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateMenu, CreatePopupMenu, DestroyMenu, DrawMenuBar, GetMenu, SetMenu, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, WM_COMMAND,
    };

    use crate::app::UserEvent;

    static PROXY: OnceLock<EventLoopProxy<UserEvent>> = OnceLock::new();
    /// Per window: the page node behind each command id (id - 1 indexes it).
    static TABLES: Mutex<Option<HashMap<isize, Vec<u64>>>> = Mutex::new(None);

    /// Route menu commands to the event loop. Call before the loop is built.
    pub fn install_hook(builder: &mut EventLoopBuilder<UserEvent>) {
        builder.with_msg_hook(|msg| {
            // SAFETY: winit passes a pointer to the `MSG` it is dispatching.
            let msg = unsafe { &*(msg as *const MSG) };
            if msg.message != WM_COMMAND || (msg.wParam >> 16) != 0 || msg.lParam != 0 {
                return false;
            }
            let cmd = msg.wParam & 0xFFFF;
            let node = TABLES.lock().ok().and_then(|t| t.as_ref()?.get(&(msg.hwnd as isize))?.get(cmd.wrapping_sub(1)).copied());
            match (node, PROXY.get()) {
                (Some(node), Some(proxy)) => proxy.send_event(UserEvent::MenuIn(msg.hwnd as isize, node)).is_ok(),
                _ => false,
            }
        });
    }

    pub struct NativeMenu {
        last: HashMap<isize, Option<Vec<MenuItem>>>,
    }

    impl NativeMenu {
        pub fn new(proxy: EventLoopProxy<UserEvent>) -> Option<NativeMenu> {
            let _ = PROXY.set(proxy);
            Some(NativeMenu { last: HashMap::new() })
        }

        /// Give `window` the page's menu as its menu bar (none removes it).
        pub fn set(&mut self, window: &Window, _app_name: &str, items: Option<Vec<MenuItem>>) {
            let Ok(RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) else { return };
            let hwnd = h.hwnd.get();
            if self.last.get(&hwnd) == Some(&items) || (items.is_none() && !self.last.contains_key(&hwnd)) {
                return;
            }
            let mut table = Vec::new();
            // SAFETY: plain Win32 calls on the window's own handle, from the thread that owns it.
            unsafe {
                let old = GetMenu(hwnd);
                match &items {
                    Some(items) => {
                        let bar = CreateMenu();
                        for item in items {
                            append(bar, item, &mut table);
                        }
                        SetMenu(hwnd, bar);
                    }
                    None => {
                        SetMenu(hwnd, std::ptr::null_mut());
                    }
                }
                if !old.is_null() {
                    DestroyMenu(old);
                }
                DrawMenuBar(hwnd);
            }
            if let Ok(mut t) = TABLES.lock() {
                t.get_or_insert_with(HashMap::new).insert(hwnd, table);
            }
            self.last.insert(hwnd, items);
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.replace('&', "&&").encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn shortcut_text(a: &Accel) -> String {
        let mut parts = Vec::new();
        if a.primary {
            parts.push("Ctrl".to_string());
        }
        if a.meta {
            parts.push("Win".to_string());
        }
        if a.alt {
            parts.push("Alt".to_string());
        }
        if a.shift {
            parts.push("Shift".to_string());
        }
        let mut key = a.key.clone();
        if let Some(c) = key.get(..1) {
            key = c.to_uppercase() + &key[1..];
        }
        parts.push(key);
        parts.join("+")
    }

    /// Add `item` to `menu`, recording commands in `table`.
    ///
    /// SAFETY: `menu` must be a live menu handle.
    unsafe fn append(menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU, item: &MenuItem, table: &mut Vec<u64>) {
        match item {
            MenuItem::Separator => {
                AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            }
            MenuItem::Item { id, label, accel, enabled, checked, items, .. } => {
                let mut flags = MF_STRING;
                if !enabled {
                    flags |= MF_GRAYED;
                }
                if *checked {
                    flags |= MF_CHECKED;
                }
                let text = match accel {
                    Some(a) if items.is_empty() => format!("{label}\t{}", shortcut_text(a)),
                    _ => label.clone(),
                };
                if items.is_empty() {
                    table.push(*id);
                    AppendMenuW(menu, flags, table.len(), wide(&text).as_ptr());
                } else {
                    let sub = CreatePopupMenu();
                    for child in items {
                        append(sub, child, table);
                    }
                    AppendMenuW(menu, flags | MF_POPUP, sub as usize, wide(&text).as_ptr());
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::OnceLock;

    use nile_engine::menus::{Accel, MenuItem};
    use objc2::{define_class, msg_send, rc::Retained, sel, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSControlStateValueOn, NSEventModifierFlags, NSMenu, NSMenuItem};
    use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString};
    use winit::event_loop::EventLoopProxy;

    use crate::app::UserEvent;

    /// Where `fire:` sends the chosen item (the target is called from AppKit, with no `App` at hand).
    static PROXY: OnceLock<EventLoopProxy<UserEvent>> = OnceLock::new();

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and the class has no ivars or Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "NileMenuTarget"]
        struct MenuTarget;

        unsafe impl NSObjectProtocol for MenuTarget {}

        impl MenuTarget {
            #[unsafe(method(fire:))]
            fn fire(&self, sender: &NSMenuItem) {
                if let Some(proxy) = PROXY.get() {
                    let _ = proxy.send_event(UserEvent::Menu(sender.tag()));
                }
            }
        }
    );

    pub struct NativeMenu {
        target: Retained<MenuTarget>,
        /// What the bar was last built from (the app name and the page's menu), so it is only rebuilt
        /// when that changes.
        last: Option<(String, Option<Vec<MenuItem>>)>,
    }

    impl NativeMenu {
        pub fn new(proxy: EventLoopProxy<UserEvent>) -> Option<NativeMenu> {
            let mtm = MainThreadMarker::new()?;
            let _ = PROXY.set(proxy);
            let target: Retained<MenuTarget> = unsafe { msg_send![mtm.alloc::<MenuTarget>(), init] };
            Some(NativeMenu { target, last: None })
        }

        /// Show the page's menu as the application's menu bar (or, with `None` after one was shown,
        /// the standard menus alone).
        pub fn set(&mut self, _window: &winit::window::Window, app_name: &str, items: Option<Vec<MenuItem>>) {
            let key = (app_name.to_string(), items);
            if self.last.as_ref() == Some(&key) {
                return;
            }
            // Until a page has asked for a menu, winit's own default menu stays.
            if key.1.is_none() && self.last.is_none() {
                self.last = Some(key);
                return;
            }
            let Some(mtm) = MainThreadMarker::new() else { return };
            let bar = NSMenu::new(mtm);
            bar.setAutoenablesItems(false);
            let author = key.1.as_deref().unwrap_or(&[]);
            bar.addItem(&self.app_menu(mtm, app_name));
            for top in author {
                if let MenuItem::Item { label, items, .. } = top {
                    bar.addItem(&self.submenu_item(mtm, label, items));
                }
            }
            let has = |name: &str| author.iter().any(|i| matches!(i, MenuItem::Item { label, .. } if label.eq_ignore_ascii_case(name)));
            if !has("Edit") {
                bar.addItem(&self.edit_menu(mtm));
            }
            if !has("Window") {
                let (item, menu) = self.window_menu(mtm);
                bar.addItem(&item);
                NSApplication::sharedApplication(mtm).setWindowsMenu(Some(&menu));
            }
            NSApplication::sharedApplication(mtm).setMainMenu(Some(&bar));
            self.last = Some(key);
        }

        fn item(&self, mtm: MainThreadMarker, title: &str, action: Option<objc2::runtime::Sel>, key: &str, mask: NSEventModifierFlags) -> Retained<NSMenuItem> {
            let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &NSString::from_str(title), action, &NSString::from_str(key)) };
            item.setKeyEquivalentModifierMask(mask);
            item
        }

        /// An item that tells the shell (tag = a page item's node id, or a negative standard action).
        fn fires(&self, mtm: MainThreadMarker, title: &str, tag: isize, key: &str, mask: NSEventModifierFlags) -> Retained<NSMenuItem> {
            let item = self.item(mtm, title, Some(sel!(fire:)), key, mask);
            unsafe { item.setTarget(Some(&self.target)) };
            item.setTag(tag);
            item
        }

        fn submenu_item(&self, mtm: MainThreadMarker, title: &str, items: &[MenuItem]) -> Retained<NSMenuItem> {
            let top = self.item(mtm, title, None, "", NSEventModifierFlags::empty());
            let menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str(title));
            menu.setAutoenablesItems(false);
            for it in items {
                menu.addItem(&self.entry(mtm, it));
            }
            top.setSubmenu(Some(&menu));
            top
        }

        fn entry(&self, mtm: MainThreadMarker, it: &MenuItem) -> Retained<NSMenuItem> {
            match it {
                MenuItem::Separator => NSMenuItem::separatorItem(mtm),
                MenuItem::Item { id, label, accel, enabled, checked, items, .. } => {
                    let (key, mask) = accel.as_ref().map(equivalent).unwrap_or((String::new(), NSEventModifierFlags::empty()));
                    let entry = if items.is_empty() {
                        self.fires(mtm, label, *id as isize, &key, mask)
                    } else {
                        self.submenu_item(mtm, label, items)
                    };
                    entry.setEnabled(*enabled);
                    if *checked {
                        entry.setState(NSControlStateValueOn);
                    }
                    entry
                }
            }
        }

        fn app_menu(&self, mtm: MainThreadMarker, name: &str) -> Retained<NSMenuItem> {
            let cmd = NSEventModifierFlags::Command;
            let items = [
                self.item(mtm, &format!("About {name}"), Some(sel!(orderFrontStandardAboutPanel:)), "", NSEventModifierFlags::empty()),
                NSMenuItem::separatorItem(mtm),
                self.item(mtm, &format!("Hide {name}"), Some(sel!(hide:)), "h", cmd),
                self.item(mtm, "Hide Others", Some(sel!(hideOtherApplications:)), "h", cmd | NSEventModifierFlags::Option),
                self.item(mtm, "Show All", Some(sel!(unhideAllApplications:)), "", NSEventModifierFlags::empty()),
                NSMenuItem::separatorItem(mtm),
                self.fires(mtm, &format!("Quit {name}"), -7, "q", cmd),
            ];
            let menu = NSMenu::new(mtm);
            for i in &items {
                menu.addItem(i);
            }
            let top = self.item(mtm, name, None, "", NSEventModifierFlags::empty());
            top.setSubmenu(Some(&menu));
            top
        }

        fn edit_menu(&self, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
            let cmd = NSEventModifierFlags::Command;
            let items = [
                self.fires(mtm, "Undo", -1, "z", cmd),
                self.fires(mtm, "Redo", -2, "z", cmd | NSEventModifierFlags::Shift),
                NSMenuItem::separatorItem(mtm),
                self.fires(mtm, "Cut", -3, "x", cmd),
                self.fires(mtm, "Copy", -4, "c", cmd),
                self.fires(mtm, "Paste", -5, "v", cmd),
                self.fires(mtm, "Select All", -6, "a", cmd),
            ];
            let menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str("Edit"));
            menu.setAutoenablesItems(false);
            for i in &items {
                menu.addItem(i);
            }
            let top = self.item(mtm, "Edit", None, "", NSEventModifierFlags::empty());
            top.setSubmenu(Some(&menu));
            top
        }

        fn window_menu(&self, mtm: MainThreadMarker) -> (Retained<NSMenuItem>, Retained<NSMenu>) {
            let cmd = NSEventModifierFlags::Command;
            let items = [
                self.item(mtm, "Minimize", Some(sel!(performMiniaturize:)), "m", cmd),
                self.item(mtm, "Zoom", Some(sel!(performZoom:)), "", NSEventModifierFlags::empty()),
                NSMenuItem::separatorItem(mtm),
                self.item(mtm, "Bring All to Front", Some(sel!(arrangeInFront:)), "", NSEventModifierFlags::empty()),
            ];
            let menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str("Window"));
            for i in &items {
                menu.addItem(i);
            }
            let top = self.item(mtm, "Window", None, "", NSEventModifierFlags::empty());
            top.setSubmenu(Some(&menu));
            (top, menu)
        }
    }

    /// The key equivalent and modifier mask of a shortcut.
    fn equivalent(a: &Accel) -> (String, NSEventModifierFlags) {
        let mut mask = NSEventModifierFlags::empty();
        if a.primary {
            mask |= NSEventModifierFlags::Command;
        }
        if a.meta {
            mask |= NSEventModifierFlags::Control;
        }
        if a.alt {
            mask |= NSEventModifierFlags::Option;
        }
        if a.shift {
            mask |= NSEventModifierFlags::Shift;
        }
        let named = |code: u32| char::from_u32(code).map(String::from).unwrap_or_default();
        let key = match a.key.as_str() {
            "enter" | "return" => "\r".to_string(),
            "esc" | "escape" => "\u{1b}".to_string(),
            "tab" => "\t".to_string(),
            "space" => " ".to_string(),
            "backspace" => "\u{8}".to_string(),
            "delete" => named(0xF728),
            "up" => named(0xF700),
            "down" => named(0xF701),
            "left" => named(0xF702),
            "right" => named(0xF703),
            "home" => named(0xF729),
            "end" => named(0xF72B),
            "pageup" => named(0xF72C),
            "pagedown" => named(0xF72D),
            k if k.len() > 1 && k.starts_with('f') && k[1..].parse::<u32>().is_ok_and(|n| (1..=12).contains(&n)) => named(0xF703 + k[1..].parse::<u32>().unwrap_or(1)),
            k => k.to_string(),
        };
        (key, mask)
    }
}
