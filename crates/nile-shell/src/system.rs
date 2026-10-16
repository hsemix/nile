//! The OS's colour theme for `nile_style::set_system_theme`: light or dark (from winit, which
//! follows the system setting) and the accent colour (macOS: `NSColor.controlAccentColor`; Windows:
//! the registry's `AccentColor`; Linux: GNOME's `accent-color` through `gsettings`; elsewhere none,
//! so the platform's stock accent is used).

use nile_style::{Color, SystemTheme};
use winit::window::{Theme, Window};

/// The theme right now, for `window`.
pub fn read(window: &Window) -> SystemTheme {
    let mut theme = SystemTheme { dark: window.theme() == Some(Theme::Dark), ..SystemTheme::default() };
    if let Some(accent) = accent() {
        theme.accent = accent;
    }
    theme
}

#[cfg(target_os = "macos")]
fn accent() -> Option<Color> {
    use objc2_app_kit::{NSColor, NSColorSpace};
    let color = NSColor::controlAccentColor();
    let srgb = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(Color::rgb(byte(srgb.redComponent()), byte(srgb.greenComponent()), byte(srgb.blueComponent())))
}

/// `HKCU\Software\Microsoft\Windows\DWM\AccentColor`, a DWORD in 0xAABBGGRR order.
#[cfg(windows)]
fn accent() -> Option<Color> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\DWM\0".encode_utf16().collect();
    let value: Vec<u16> = "AccentColor\0".encode_utf16().collect();
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: valid NUL-terminated names and a DWORD-sized output buffer.
    let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), (&mut data as *mut u32).cast(), &mut size) };
    (status == 0).then_some(Color::rgb(data as u8, (data >> 8) as u8, (data >> 16) as u8))
}

/// GNOME 47's accent names; other desktops have no such setting.
#[cfg(all(unix, not(target_os = "macos")))]
fn accent() -> Option<Color> {
    let out = std::process::Command::new("gsettings").args(["get", "org.gnome.desktop.interface", "accent-color"]).output().ok()?;
    let name = String::from_utf8_lossy(&out.stdout);
    let (r, g, b) = match name.trim().trim_matches('\'') {
        "blue" => (0x35, 0x84, 0xe4),
        "teal" => (0x21, 0x90, 0xa4),
        "green" => (0x3a, 0x94, 0x4a),
        "yellow" => (0xc8, 0x88, 0x00),
        "orange" => (0xed, 0x5b, 0x00),
        "red" => (0xe6, 0x2d, 0x42),
        "pink" => (0xd5, 0x61, 0x99),
        "purple" => (0x91, 0x41, 0xac),
        "slate" => (0x6f, 0x83, 0x96),
        _ => return None,
    };
    Some(Color::rgb(r, g, b))
}

#[cfg(not(any(unix, windows)))]
fn accent() -> Option<Color> {
    None
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn the_macos_accent_colour_is_readable() {
        assert!(super::accent().is_some());
    }
}
