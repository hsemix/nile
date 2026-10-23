//! Windows 11 Snap Layouts for frameless windows. The flyout appears when the pointer rests on a
//! maximize button that the window reports as one: the window answers `WM_NCHITTEST` with
//! `HTMAXBUTTON` over the page's `role="window-maximize"` element. Windows then owns the pointer
//! there, so hover is fed back to the page (`NOTIFY`) and the click is carried out here as a system
//! maximize or restore. Written without a Windows compiler to hand: CI compiles it.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use windows_sys::Win32::{
    Foundation::POINT,
    Graphics::{
        Dwm::{DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_NCRENDERING_POLICY},
        Gdi::ScreenToClient,
    },
    UI::{
        Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT},
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::{
            GetWindowLongW, IsZoomed, PostMessageW, SetWindowLongW, SetWindowPos, GWL_STYLE, HTMAXBUTTON, SC_MAXIMIZE, SC_RESTORE, STYLESTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE, SWP_NOZORDER, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_STYLECHANGING, WM_SYSCOMMAND, WS_CAPTION,
        },
    },
};

type Rect4 = (i32, i32, i32, i32);

/// 0: the pointer left the button, 1: it is over it.
type Notify = Box<dyn Fn(isize, u8) + Send + Sync>;

static NOTIFY: OnceLock<Notify> = OnceLock::new();
/// Per window: the maximize button's rectangle (left, top, right, bottom) in physical client pixels.
static RECTS: Mutex<Option<HashMap<isize, Rect4>>> = Mutex::new(None);

const SUBCLASS_ID: usize = 0x4e49_4c45; // "NILE"

/// Where hover changes go (the event loop's proxy, from the shell).
pub fn set_notify(f: impl Fn(isize, u8) + Send + Sync + 'static) {
    let _ = NOTIFY.set(Box::new(f));
}

/// Start answering hit tests for `hwnd`. Call once, after the window exists.
///
/// Winit makes a frameless window by keeping `WS_CAPTION` and emptying the non-client area in
/// `WM_NCCALCSIZE`, so the system still thinks it has a title bar and draws one whenever it renders the
/// frame itself (the taskbar's hover preview, the maximize/restore transition). The style is dropped
/// here, and again each time winit rewrites it (`WM_STYLECHANGING`); resizing and maximizing only need
/// the sizing border, which stays.
pub fn install(hwnd: isize, shadow: bool) {
    // SAFETY: the window belongs to this thread and outlives the subclass (it is removed with it).
    unsafe {
        SetWindowSubclass(hwnd, Some(proc), SUBCLASS_ID, 0);
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        if style & WS_CAPTION != 0 {
            SetWindowLongW(hwnd, GWL_STYLE, (style & !WS_CAPTION) as i32);
            SetWindowPos(hwnd, 0, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        // The window has no frame of its own to render: this stops the system drawing one while it shows,
        // hides and previews the window (that also takes its drop shadow, which `solid-with-shadow` keeps).
        if !shadow {
            let policy = DWMNCRP_DISABLED;
            DwmSetWindowAttribute(hwnd, DWMWA_NCRENDERING_POLICY as u32, std::ptr::from_ref(&policy).cast(), std::mem::size_of_val(&policy) as u32);
        }
    }
}

/// The maximize button's rectangle in physical client pixels, or `None` when the window has none
/// (or can't be maximized).
pub fn set_rect(hwnd: isize, rect: Option<Rect4>) {
    if let Ok(mut all) = RECTS.lock() {
        let map = all.get_or_insert_with(HashMap::new);
        match rect {
            Some(r) => {
                map.insert(hwnd, r);
            }
            None => {
                map.remove(&hwnd);
            }
        }
    }
}

fn rect_of(hwnd: isize) -> Option<Rect4> {
    RECTS.lock().ok()?.as_ref()?.get(&hwnd).copied()
}

fn notify(hwnd: isize, state: u8) {
    if let Some(f) = NOTIFY.get() {
        f(hwnd, state);
    }
}

unsafe extern "system" fn proc(hwnd: isize, msg: u32, wparam: usize, lparam: isize, _id: usize, _data: usize) -> isize {
    match msg {
        WM_STYLECHANGING if wparam as i32 == GWL_STYLE => {
            // SAFETY: for this message `lparam` points at the STYLESTRUCT the system is about to apply.
            unsafe { (*(lparam as *mut STYLESTRUCT)).styleNew &= !WS_CAPTION };
        }
        WM_NCHITTEST => {
            if let Some((l, t, r, b)) = rect_of(hwnd) {
                // The point comes in screen coordinates, packed as two signed 16-bit halves.
                let mut p = POINT { x: i32::from(lparam as i16), y: i32::from((lparam >> 16) as i16) };
                // SAFETY: `hwnd` is the window this subclass is installed on.
                unsafe { ScreenToClient(hwnd, &mut p) };
                if p.x >= l && p.x < r && p.y >= t && p.y < b {
                    return HTMAXBUTTON as isize;
                }
            }
        }
        WM_NCMOUSEMOVE if wparam == HTMAXBUTTON as usize => {
            // Ask for the leave message, which non-client areas don't send unless told to.
            let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE | TME_NONCLIENT, hwndTrack: hwnd, dwHoverTime: 0 };
            // SAFETY: `track` is a valid, initialised TRACKMOUSEEVENT.
            unsafe { TrackMouseEvent(&mut track) };
            notify(hwnd, 1);
        }
        WM_NCMOUSELEAVE => notify(hwnd, 0),
        // The button belongs to the page, so the click is ours: swallow the press, and on release do
        // what a caption button does.
        WM_NCLBUTTONDOWN if wparam == HTMAXBUTTON as usize => return 0,
        WM_NCLBUTTONUP if wparam == HTMAXBUTTON as usize => {
            // SAFETY: plain window queries and a posted system command on this window.
            unsafe {
                let command = if IsZoomed(hwnd) != 0 { SC_RESTORE } else { SC_MAXIMIZE };
                PostMessageW(hwnd, WM_SYSCOMMAND, command as usize, 0);
            }
            return 0;
        }
        _ => {}
    }
    // SAFETY: forwards the message unchanged to the next handler in the chain.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
