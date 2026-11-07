//! `Event.keyState(name)`: whether Caps Lock, Num Lock or Scroll Lock is on, or a modifier key is
//! held, read from the system when it can say (macOS, Windows). Elsewhere (Linux: no portable
//! way across X11 and Wayland) the answer is `None` and the script side falls back to what it saw.

/// A key by its `KeyboardEvent.key` name (`CapsLock`, `NumLock`, `ScrollLock`, `Shift`, `Control`,
/// `Alt`, `Meta`; any case). Lock keys report whether the lock is on, the others whether they are down.
pub fn key_state(name: &str) -> Option<bool> {
    let name = name.trim().to_ascii_lowercase();
    platform(&name)
}

#[cfg(target_os = "macos")]
fn platform(name: &str) -> Option<bool> {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }
    // kCGEventSourceStateCombinedSessionState: what the user sees, keyboards and synthetic events together.
    // SAFETY: a plain query with no pointers.
    let flags = unsafe { CGEventSourceFlagsState(0) };
    let mask = match name {
        "capslock" => 0x1_0000,
        "shift" => 0x2_0000,
        "control" | "ctrl" => 0x4_0000,
        "alt" | "option" => 0x8_0000,
        "meta" | "command" | "cmd" => 0x10_0000,
        _ => return None, // macOS keyboards have no Num Lock or Scroll Lock
    };
    Some(flags & mask != 0)
}

#[cfg(target_os = "windows")]
fn platform(name: &str) -> Option<bool> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CAPITAL, VK_CONTROL, VK_LWIN, VK_MENU, VK_NUMLOCK, VK_RWIN, VK_SCROLL, VK_SHIFT};
    // Toggle keys: the low bit is "on". Others: the high bit (the sign) is "down".
    let (vk, toggle) = match name {
        "capslock" => (VK_CAPITAL, true),
        "numlock" => (VK_NUMLOCK, true),
        "scrolllock" => (VK_SCROLL, true),
        "shift" => (VK_SHIFT, false),
        "control" | "ctrl" => (VK_CONTROL, false),
        "alt" => (VK_MENU, false),
        "meta" | "win" => {
            // SAFETY: plain queries.
            let down = unsafe { GetKeyState(i32::from(VK_LWIN)) < 0 || GetKeyState(i32::from(VK_RWIN)) < 0 };
            return Some(down);
        }
        _ => return None,
    };
    // SAFETY: a plain query with no pointers.
    let state = unsafe { GetKeyState(i32::from(vk)) };
    Some(if toggle { state & 1 != 0 } else { state < 0 })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform(_name: &str) -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::key_state;

    #[test]
    fn unknown_keys_have_no_answer() {
        assert_eq!(key_state("NoSuchKey"), None);
    }
}
