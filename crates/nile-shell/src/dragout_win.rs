//! Windows: a system drag of files with `DoDragDrop`. The data object comes from the shell
//! (`SHCreateDataObject` over the files' item ID lists), so it carries every format Explorer
//! understands; only the drop source (the cursor feedback and the button check) is ours. Only
//! `windows-sys` and std are used so the file can be type-checked on its own. Written without a
//! Windows machine to hand: CI is where it gets compiled.

use std::ffi::c_void;

use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::{E_NOINTERFACE, E_POINTER, HWND, S_OK},
        System::{
            Ole::{DoDragDrop, DROPEFFECT_COPY},
            SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS},
        },
    },
};

// windows-sys 0.52 has none of these (the shell functions sit behind a feature it lacks), so they are
// declared by hand, as the dialogs do.
const DRAGDROP_S_DROP: i32 = 0x0004_0100;
const DRAGDROP_S_CANCEL: i32 = 0x0004_0101;
const DRAGDROP_S_USEDEFAULTCURSORS: i32 = 0x0004_0102;

type Pidl = *mut c_void;

#[link(name = "shell32")]
extern "system" {
    fn SHParseDisplayName(name: *const u16, bind_context: *mut c_void, pidl: *mut Pidl, attrs_in: u32, attrs_out: *mut u32) -> i32;
    fn SHCreateDataObject(parent: *const c_void, count: u32, items: *const Pidl, inner: *mut c_void, riid: *const GUID, out: *mut *mut c_void) -> i32;
    fn ILFree(pidl: Pidl);
}

const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_C000_000000000046);
const IID_IDROPSOURCE: GUID = GUID::from_u128(0x00000121_0000_0000_C000_000000000046);
const IID_IDATAOBJECT: GUID = GUID::from_u128(0x0000010E_0000_0000_C000_000000000046);

#[repr(C)]
struct DropSourceVtbl {
    query_interface: unsafe extern "system" fn(*mut DropSource, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut DropSource) -> u32,
    release: unsafe extern "system" fn(*mut DropSource) -> u32,
    query_continue_drag: unsafe extern "system" fn(*mut DropSource, i32, MODIFIERKEYS_FLAGS) -> i32,
    give_feedback: unsafe extern "system" fn(*mut DropSource, u32) -> i32,
}

#[repr(C)]
struct DropSource {
    vtbl: *const DropSourceVtbl,
}

unsafe extern "system" fn query_interface(this: *mut DropSource, iid: *const GUID, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    let wanted = &*iid;
    let same = |g: &GUID| g.data1 == wanted.data1 && g.data2 == wanted.data2 && g.data3 == wanted.data3 && g.data4 == wanted.data4;
    if same(&IID_IUNKNOWN) || same(&IID_IDROPSOURCE) {
        *out = this as *mut c_void;
        return S_OK;
    }
    *out = std::ptr::null_mut();
    E_NOINTERFACE
}

// The object is static and lives for the whole call: reference counting is a no-op.
unsafe extern "system" fn add_ref(_: *mut DropSource) -> u32 {
    1
}

unsafe extern "system" fn release(_: *mut DropSource) -> u32 {
    1
}

/// Keep going while the left button is down, drop when it is released, cancel on Escape.
unsafe extern "system" fn query_continue_drag(_: *mut DropSource, escape: i32, keys: MODIFIERKEYS_FLAGS) -> i32 {
    if escape != 0 {
        DRAGDROP_S_CANCEL
    } else if keys & MK_LBUTTON == 0 {
        DRAGDROP_S_DROP
    } else {
        S_OK
    }
}

unsafe extern "system" fn give_feedback(_: *mut DropSource, _effect: u32) -> i32 {
    DRAGDROP_S_USEDEFAULTCURSORS
}

static VTBL: DropSourceVtbl = DropSourceVtbl { query_interface, add_ref, release, query_continue_drag, give_feedback };

/// Drag `files` (absolute paths) out of the window `_hwnd`; returns once the drag ends. False when the
/// drag could not be set up.
pub fn drag_files(_hwnd: HWND, files: &[String]) -> bool {
    let mut pidls: Vec<Pidl> = Vec::new();
    for file in files {
        let wide: Vec<u16> = file.replace('/', "\\").encode_utf16().chain(std::iter::once(0)).collect();
        let mut pidl: Pidl = std::ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated and `pidl` receives a list the shell allocated (freed below).
        let hr = unsafe { SHParseDisplayName(wide.as_ptr(), std::ptr::null_mut(), &mut pidl, 0, std::ptr::null_mut()) };
        if hr >= 0 && !pidl.is_null() {
            pidls.push(pidl);
        }
    }
    if pidls.is_empty() {
        return false;
    }
    let mut data: *mut c_void = std::ptr::null_mut();
    // SAFETY: `pidls` holds absolute item ID lists; the call fills `data` with an IDataObject.
    let hr = unsafe { SHCreateDataObject(std::ptr::null(), pidls.len() as u32, pidls.as_ptr(), std::ptr::null_mut(), &IID_IDATAOBJECT, &mut data) };
    let mut started = false;
    if hr >= 0 && !data.is_null() {
        let mut source = DropSource { vtbl: &VTBL };
        let mut effect = 0u32;
        // SAFETY: both objects are valid COM objects for the duration of the (blocking) call.
        unsafe {
            let result = DoDragDrop(data, &mut source as *mut DropSource as *mut _, DROPEFFECT_COPY, &mut effect);
            started = result == DRAGDROP_S_DROP || result == DRAGDROP_S_CANCEL;
            // Release the data object (IUnknown::Release is the third slot of any COM vtable).
            let vtbl = *(data as *mut *const [usize; 3]);
            let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute((*vtbl)[2]);
            release(data);
        }
    }
    for pidl in pidls {
        // SAFETY: allocated by `SHParseDisplayName`.
        unsafe { ILFree(pidl) };
    }
    started
}
