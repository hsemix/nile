//! `window-material`: the system's blur/Mica/acrylic behind a window whose page is transparent.
//! macOS puts an `NSVisualEffectView` under the page's view, Windows asks DWM for a backdrop through
//! winit, KDE's blur protocol is reached through `Window::set_blur`; other platforms ignore it.

use nile_engine::window::Material;
use winit::window::Window;

/// Applies `material` to the window, replacing the one it had; `Material::None` removes it. The
/// window is made transparent for the effect (a window created without one can't be solid again
/// on every platform, so it stays transparent: the page paints its own background).
pub fn apply(window: &Window, material: Material) {
    if material != Material::None {
        window.set_transparent(true);
    }
    #[cfg(target_os = "macos")]
    macos(window, material);
    #[cfg(windows)]
    windows(window, material);
    #[cfg(not(any(target_os = "macos", windows)))]
    window.set_blur(material != Material::None);
    #[cfg(any(target_os = "macos", windows))]
    let _ = window;
}

#[cfg(windows)]
fn windows(window: &Window, material: Material) {
    use winit::platform::windows::{BackdropType, WindowExtWindows};
    window.set_system_backdrop(match material {
        Material::None => BackdropType::None,
        Material::Mica => BackdropType::MainWindow,
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
