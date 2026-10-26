//! Global hotkeys (`application.registerHotkey`): the system-wide shortcuts a page asked for, registered through the
//! `global-hotkey` crate. One set per program, owned by the window that registered them; `set` replaces the registration
//! whenever the page's list changes and reports which accelerators the system accepted.

use std::str::FromStr;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

#[derive(Default)]
pub struct Hotkeys {
    manager: Option<GlobalHotKeyManager>,
    /// What is registered: the page's id, the accelerator text, the system's id for it.
    active: Vec<(u32, String, u32, HotKey)>,
    /// The window that owns them.
    pub owner: Option<u32>,
    wired: bool,
    asked: Vec<(u32, String)>,
}

/// `Ctrl+Shift+K` and friends in the crate's spelling: `Cmd`/`Command`/`Win`/`Meta` are the super key, `CmdOrCtrl` is
/// the platform's usual one, a single letter or digit names its key.
fn translate(accel: &str) -> String {
    let parts: Vec<&str> = accel.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    let n = parts.len();
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let low = p.to_ascii_lowercase();
            if i + 1 < n {
                match low.as_str() {
                    "cmd" | "command" | "win" | "meta" | "super" => "super".to_string(),
                    "cmdorctrl" | "commandorcontrol" => if cfg!(target_os = "macos") { "super" } else { "ctrl" }.to_string(),
                    "option" => "alt".to_string(),
                    _ => low,
                }
            } else if p.chars().count() == 1 && p.chars().all(|c| c.is_ascii_alphabetic()) {
                format!("Key{}", p.to_ascii_uppercase())
            } else if p.chars().count() == 1 && p.chars().all(|c| c.is_ascii_digit()) {
                format!("Digit{p}")
            } else {
                p.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

impl Hotkeys {
    /// Make the registration match `wanted` (`(id, accelerator)`); returns `(id, accepted)` for the ones newly tried, or
    /// `None` when nothing changed.
    pub fn set(&mut self, owner: u32, wanted: &[(u32, String)], proxy: EventLoopProxy<UserEvent>) -> Option<Vec<(u32, bool)>> {
        // Compare with what was asked for last time (a refused key isn't in `active`).
        if self.owner == Some(owner) && self.asked == wanted {
            return None;
        }
        self.asked = wanted.to_vec();
        if self.manager.is_none() {
            self.manager = GlobalHotKeyManager::new().ok();
        }
        if !self.wired {
            self.wired = true;
            GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
                if e.state == HotKeyState::Pressed {
                    let _ = proxy.send_event(UserEvent::Hotkey(e.id));
                }
            }));
        }
        let mut results = Vec::new();
        let manager = self.manager.as_ref();
        // Unregister what is gone; keep what is still wanted.
        self.active.retain(|(id, acc, _, key)| {
            let keep = wanted.iter().any(|(i, a)| i == id && a == acc);
            if !keep {
                if let Some(m) = manager {
                    let _ = m.unregister(*key);
                }
            }
            keep
        });
        for (id, accel) in wanted {
            if self.active.iter().any(|(i, ..)| i == id) {
                continue;
            }
            let ok = match (manager, HotKey::from_str(&translate(accel))) {
                (Some(m), Ok(key)) => match m.register(key) {
                    Ok(()) => {
                        self.active.push((*id, accel.clone(), key.id(), key));
                        true
                    }
                    Err(e) => {
                        eprintln!("hotkey {accel}: {e}");
                        false
                    }
                },
                (_, Err(e)) => {
                    eprintln!("hotkey {accel}: {e}");
                    false
                }
                (None, _) => false,
            };
            results.push((*id, ok));
        }
        self.owner = (!wanted.is_empty()).then_some(owner);
        Some(results)
    }

    /// The (window, page id) of a pressed system hotkey.
    pub fn lookup(&self, system_id: u32) -> Option<(u32, u32)> {
        let owner = self.owner?;
        self.active.iter().find(|(_, _, sys, _)| *sys == system_id).map(|(id, ..)| (owner, *id))
    }
}

#[cfg(test)]
mod tests {
    use super::translate;

    #[test]
    fn accelerators_are_spelled_for_the_crate() {
        assert_eq!(translate("Ctrl+Shift+K"), "ctrl+shift+KeyK");
        assert_eq!(translate("Cmd+1"), "super+Digit1");
        assert_eq!(translate("Alt+F9"), "alt+F9");
        assert_eq!(translate("Option+Space"), "alt+Space");
    }
}
