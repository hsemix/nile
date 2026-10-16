//! `window-material`: the system's blur/Mica/acrylic behind a window whose page is transparent.
//! macOS puts an `NSVisualEffectView` under the page's view, Windows asks DWM for a backdrop through
//! winit, KDE's blur protocol is reached through `Window::set_blur`; other platforms ignore it.

use nile_engine::window::{Frame, Material, WindowConfig};
use winit::window::Window;

/// The material to show for a window with this `config`: what the page asked for, or for an unset
/// one the platform's default. On Windows that is Mica in the title bar of a window with a system
/// frame, which is how File Explorer and Sciter windows look; nothing elsewhere.
pub fn effective(config: &WindowConfig) -> Material {
    match config.material {
        Material::Auto if cfg!(windows) && matches!(config.frame, Frame::Native | Frame::Extended) => Material::Mica,
        Material::Auto => Material::None,
        m => m,
    }
}

/// Applies `material` (from [`effective`]) to the window, replacing the one it had; `Material::None`
/// removes it. With `translucent` (the page asked for a material or a transparent frame) the window
/// is made transparent for the effect, which stays on afterwards: the page paints its own background.
pub fn apply(window: &Window, material: Material, translucent: bool, radius: f32) {
    if translucent {
        window.set_transparent(true);
    }
    #[cfg(target_os = "macos")]
    {
        macos(window, material);
        set_corner(window, radius);
    }
    #[cfg(windows)]
    windows(window, material);
    #[cfg(not(any(target_os = "macos", windows)))]
    window.set_blur(material != Material::None);
    #[cfg(any(target_os = "macos", windows))]
    let _ = window;
    let _ = radius;
}

/// Clips a frameless window to rounded corners of `radius` logical px (0 = square), so a material
/// drawn by the system takes the shape of the page's own `border-radius`. Windows: a window region
/// (its edge isn't anti-aliased, and the system shadow is lost); macOS: the layers' corner radius.
/// Nothing elsewhere.
pub fn set_corner(window: &Window, radius: f32) {
    #[cfg(target_os = "macos")]
    macos_corner(window, radius);
    #[cfg(windows)]
    windows_corner(window, (radius * window.scale_factor() as f32).round() as i32);
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (window, radius);
}

#[cfg(windows)]
fn windows_corner(window: &Window, radius: i32) {
    use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(RawWindowHandle::Win32(handle)) = window.window_handle().map(|h| h.as_raw()) else { return };
    let size = window.outer_size();
    // SAFETY: the window belongs to this thread; SetWindowRgn owns the region it is given.
    unsafe {
        if radius <= 0 {
            SetWindowRgn(handle.hwnd.get(), 0, 1);
        } else {
            let region = CreateRoundRectRgn(0, 0, size.width as i32 + 1, size.height as i32 + 1, radius * 2, radius * 2);
            SetWindowRgn(handle.hwnd.get(), region, 1);
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_corner(window: &Window, radius: f32) {
    use objc2::{msg_send, runtime::AnyObject, ClassType};
    use objc2_app_kit::{NSVisualEffectView, NSView};
    use objc2_foundation::NSObjectProtocol;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(RawWindowHandle::AppKit(handle)) = window.window_handle().map(|h| h.as_raw()) else { return };
    // SAFETY: winit's AppKit handle is the window's live content view.
    let content: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let round = |view: &NSView| {
        view.setWantsLayer(true);
        // SAFETY: a layer-backed view's `layer` is a live CALayer; these are its documented setters.
        unsafe {
            let layer: *mut AnyObject = msg_send![view, layer];
            if let Some(layer) = layer.as_ref() {
                let _: () = msg_send![layer, setCornerRadius: f64::from(radius)];
                let _: () = msg_send![layer, setMasksToBounds: objc2::runtime::Bool::new(radius > 0.0)];
            }
        }
    };
    round(content);
    // SAFETY: the content view is attached to a window, so its superview exists and is live.
    if let Some(host) = unsafe { content.superview() } {
        for view in host.subviews().iter() {
            if view.isKindOfClass(NSVisualEffectView::class()) {
                round(&view);
            }
        }
        // SAFETY: plain window messages on the main thread.
        if let Some(win) = content.window() {
            win.invalidateShadow();
        }
    }
}

#[cfg(windows)]
fn windows(window: &Window, material: Material) {
    use winit::platform::windows::{BackdropType, WindowExtWindows};
    window.set_system_backdrop(match material {
        Material::None | Material::Auto => BackdropType::None,
        Material::Mica | Material::Sidebar => BackdropType::MainWindow, // the closest to macOS's sidebar
        Material::Tabbed => BackdropType::TabbedWindow,
        _ => BackdropType::TransientWindow, // acrylic
    });
}

#[cfg(target_os = "macos")]
fn macos(window: &Window, material: Material) {
    use objc2::{ClassType, MainThreadMarker};
    use objc2_foundation::NSObjectProtocol;
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSView,
        NSWindowOrderingMode,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Ok(RawWindowHandle::AppKit(handle)) = window.window_handle().map(|h| h.as_raw()) else { return };
    // SAFETY: winit's AppKit handle is the window's live content view.
    let content: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    // The effect view goes *beside* the content view, below it: the page's pixels are the content
    // view's layer, which always paints beneath its own subviews.
    // SAFETY: the content view is attached to a window, so its superview exists and is live.
    let Some(host) = (unsafe { content.superview() }) else { return };
    // Take away the effect view an earlier material put there.
    for old in host.subviews().iter() {
        if old.isKindOfClass(NSVisualEffectView::class()) {
            old.removeFromSuperview();
        }
    }
    if material == Material::None {
        return;
    }
    let effect = NSVisualEffectView::new(mtm);
    effect.setMaterial(match material {
        Material::Mica => NSVisualEffectMaterial::WindowBackground,
        Material::Tabbed => NSVisualEffectMaterial::Titlebar,
        Material::Sidebar => NSVisualEffectMaterial::Sidebar,
        Material::Hud => NSVisualEffectMaterial::HUDWindow,
        Material::Acrylic => NSVisualEffectMaterial::Popover,
        _ => NSVisualEffectMaterial::UnderWindowBackground,
    });
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::Active);
    effect.setFrame(content.frame());
    effect.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    host.addSubview_positioned_relativeTo(&effect, NSWindowOrderingMode::Below, Some(content));
}
