//! Windows: the notification-area icon (`Shell_NotifyIconW`) and the taskbar overlay badge
//! (`ITaskbarList3::SetOverlayIcon`). Only `windows-sys` and std are used, so the file can be
//! type-checked on its own. Written without a Windows machine to hand: CI is where it gets compiled.

use std::ffi::c_void;

use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::{HWND, POINT},
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::{
            Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW},
            WindowsAndMessaging::{
                CreateIcon, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, PostMessageW, SetForegroundWindow, TrackPopupMenu, HICON, HMENU, MSG, TPM_RETURNCMD, TPM_RIGHTBUTTON,
                WM_APP, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
            },
        },
    },
};

/// The message Windows posts to the window when the tray icon is used.
pub const TRAY_MESSAGE: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;

/// What a tray message means: `trayclick`, `traydoubleclick` or `traymenu` (secondary button).
pub fn tray_event(msg: &MSG) -> Option<&'static str> {
    if msg.message != TRAY_MESSAGE || msg.wParam != TRAY_ID as usize {
        return None;
    }
    match (msg.lParam as u32) & 0xFFFF {
        WM_LBUTTONUP => Some("trayclick"),
        WM_LBUTTONDBLCLK => Some("traydoubleclick"),
        WM_RBUTTONUP => Some("traymenu"),
        _ => None,
    }
}

/// A window icon from RGBA pixels (as `winit` builds one).
fn icon_from(rgba: &[u8], width: u32, height: u32) -> Option<HICON> {
    if rgba.len() != width as usize * height as usize * 4 || width == 0 || height == 0 {
        return None;
    }
    let mut bgra = Vec::with_capacity(rgba.len());
    let mut mask = Vec::with_capacity(rgba.len() / 4);
    for px in rgba.as_chunks::<4>().0 {
        bgra.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        mask.push(if px[3] == 0 { 0xFF } else { 0 });
    }
    // SAFETY: both buffers outlive the call and have the sizes `CreateIcon` reads for a 32-bit icon.
    let icon = unsafe { CreateIcon(0, width as i32, height as i32, 1, 32, mask.as_ptr(), bgra.as_ptr()) };
    (icon != 0).then_some(icon)
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    // SAFETY: an all-zero NOTIFYICONDATAW is a valid starting point (null handles, empty strings).
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid
}

/// Add the icon, or update it when it is already there (`shown`). The shell copies the picture, so
/// the handle is freed right away.
pub fn set_tray(hwnd: HWND, shown: &mut bool, icon: Option<(&[u8], u32, u32)>, tooltip: &str) -> Result<(), String> {
    let hicon = icon.and_then(|(rgba, w, h)| icon_from(rgba, w, h));
    let mut nid = data(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_TIP | if hicon.is_some() { NIF_ICON } else { 0 };
    nid.uCallbackMessage = TRAY_MESSAGE;
    nid.hIcon = hicon.unwrap_or(0);
    for (slot, unit) in nid.szTip.iter_mut().take(127).zip(tooltip.encode_utf16()) {
        *slot = unit;
    }
    // SAFETY: `nid` is fully initialised and outlives the call.
    let added = unsafe {
        let ok = Shell_NotifyIconW(if *shown { NIM_MODIFY } else { NIM_ADD }, &nid);
        if let Some(h) = hicon {
            DestroyIcon(h);
        }
        ok
    };
    if added == 0 {
        return Err(format!("Shell_NotifyIconW failed (last error {})", std::io::Error::last_os_error()));
    }
    *shown = true;
    match (icon.is_some(), hicon.is_some()) {
        (false, _) => Err("the icon file could not be read (shown without a picture)".into()),
        (true, false) => Err("the icon picture could not be turned into a Windows icon (shown without a picture)".into()),
        _ => Ok(()),
    }
}

pub fn remove_tray(hwnd: HWND, shown: &mut bool) {
    if std::mem::take(shown) {
        let nid = data(hwnd);
        // SAFETY: deleting the icon this process added.
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

/// Show a popup menu at the pointer for the tray icon and return the command the user chose (the
/// 1-based index `fill` gave it), or 0. `fill` adds the items to the menu it is handed.
pub fn tray_menu(hwnd: HWND, fill: impl FnOnce(HMENU)) -> usize {
    // SAFETY: plain Win32 calls on this thread's own window; the menu is destroyed before returning.
    unsafe {
        let menu = CreatePopupMenu();
        if menu == 0 {
            return 0;
        }
        fill(menu);
        let mut at = POINT { x: 0, y: 0 };
        GetCursorPos(&mut at);
        // Without this the menu does not close when the user clicks elsewhere.
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        cmd.max(0) as usize
    }
}

const CLSID_TASKBAR_LIST: GUID = GUID::from_u128(0x56FDF344_FD6D_11D0_958A_006097C9A090);
const IID_ITASKBAR_LIST3: GUID = GUID::from_u128(0xEA1AFB91_9E28_4B86_90E9_9E9F8A5EEFAF);

type Hr = i32;

/// `ITaskbarList3`'s vtable up to `SetOverlayIcon` (IUnknown, ITaskbarList, ITaskbarList2, ITaskbarList3).
#[repr(C)]
struct TaskbarVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    hr_init: unsafe extern "system" fn(*mut c_void) -> Hr,
    add_tab: usize,
    delete_tab: usize,
    activate_tab: usize,
    set_active_alt: usize,
    mark_fullscreen_window: usize,
    set_progress_value: unsafe extern "system" fn(*mut c_void, HWND, u64, u64) -> Hr,
    set_progress_state: unsafe extern "system" fn(*mut c_void, HWND, i32) -> Hr,
    register_tab: usize,
    unregister_tab: usize,
    set_tab_order: usize,
    set_tab_active: usize,
    thumb_bar_add_buttons: usize,
    thumb_bar_update_buttons: usize,
    thumb_bar_set_image_list: usize,
    set_overlay_icon: unsafe extern "system" fn(*mut c_void, HWND, HICON, *const u16) -> Hr,
}

#[repr(C)]
struct TaskbarObject {
    vtbl: *const TaskbarVtbl,
}

/// The taskbar button's overlay: a small badge picture, or none. The list object is created on
/// each call (cheap, and nothing has to be kept alive or released at exit).
pub fn set_overlay(hwnd: HWND, icon: Option<(&[u8], u32, u32)>, description: &str) -> bool {
    let hicon = icon.and_then(|(rgba, w, h)| icon_from(rgba, w, h));
    let text: Vec<u16> = description.encode_utf16().chain(std::iter::once(0)).collect();
    let mut ok = false;
    // SAFETY: the COM object comes from `CoCreateInstance` and is released before returning; the
    // vtable layout above is ITaskbarList3's.
    unsafe {
        let mut raw: *mut c_void = std::ptr::null_mut();
        let hr = CoCreateInstance(&CLSID_TASKBAR_LIST, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_ITASKBAR_LIST3, &mut raw);
        if hr >= 0 && !raw.is_null() {
            let object = raw as *mut TaskbarObject;
            let vtbl = &*(*object).vtbl;
            if (vtbl.hr_init)(raw) >= 0 {
                ok = (vtbl.set_overlay_icon)(raw, hwnd, hicon.unwrap_or(0), text.as_ptr()) >= 0;
            }
            (vtbl.release)(raw);
        }
        // The taskbar keeps its own copy of the icon.
        if let Some(h) = hicon {
            DestroyIcon(h);
        }
    }
    ok
}

/// The taskbar button's progress bar: `None` clears it, `Some(None)` is the marquee, `Some(Some(f))` a
/// fraction from 0 to 1.
pub fn set_progress(hwnd: HWND, progress: Option<Option<f32>>) -> bool {
    const NOPROGRESS: i32 = 0;
    const INDETERMINATE: i32 = 1;
    const NORMAL: i32 = 2;
    let mut ok = false;
    // SAFETY: as in `set_overlay`.
    unsafe {
        let mut raw: *mut c_void = std::ptr::null_mut();
        let hr = CoCreateInstance(&CLSID_TASKBAR_LIST, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_ITASKBAR_LIST3, &mut raw);
        if hr >= 0 && !raw.is_null() {
            let object = raw as *mut TaskbarObject;
            let vtbl = &*(*object).vtbl;
            if (vtbl.hr_init)(raw) >= 0 {
                ok = match progress {
                    None => (vtbl.set_progress_state)(raw, hwnd, NOPROGRESS) >= 0,
                    Some(None) => (vtbl.set_progress_state)(raw, hwnd, INDETERMINATE) >= 0,
                    Some(Some(f)) => {
                        (vtbl.set_progress_state)(raw, hwnd, NORMAL) >= 0 && (vtbl.set_progress_value)(raw, hwnd, (f.clamp(0.0, 1.0) * 1000.0) as u64, 1000) >= 0
                    }
                };
            }
            (vtbl.release)(raw);
        }
    }
    ok
}
