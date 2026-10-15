//! `libnile_app`: libnile (see `nile.h`) plus a standalone runner. Where `libnile` leaves the
//! window and the event loop to the host, these functions own them: [`nile_run_file`] opens a
//! window and returns when the last one closes, like the `nile` program does.
//!
//! Call them on the **main thread** (macOS and winit require it); they block until the program ends.

use std::{
    ffi::{c_char, c_void, CStr, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
};

// Every libnile function (`nile_view_*`, ...) is part of this library too.
pub use nile::*;

/// A function the program provides for scripts: it receives the call's arguments as a JSON array
/// and returns a JSON value as a NUL-terminated string (copied immediately), or NULL for
/// `undefined`. Scripts call it as `host.name(...)`.
pub type NileAppFn = Option<unsafe extern "C" fn(args_json: *const c_char, user: *mut c_void) -> *const c_char>;

fn text<'a>(p: *const c_char) -> Option<&'a str> {
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_str().ok()).flatten()
}

/// Panics must not cross into C: 101 is the exit code Rust itself uses for one.
fn guard(f: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(101)
}

/// Expose `callback` to the scripts of every window the program opens as `host.<name>(...)`, from
/// their first line on. Call it before [`nile_run`] and friends. Registering a name again replaces
/// it. Returns false for bad arguments.
///
/// # Safety
/// `name` must be a valid string; `callback` and `user` must stay valid until the program ends.
/// The callback runs on the main thread, inside the event loop.
#[no_mangle]
pub unsafe extern "C" fn nile_app_register_function(name: *const c_char, callback: NileAppFn, user: *mut c_void) -> bool {
    let (Some(name), Some(callback)) = (text(name), callback) else { return false };
    let user = user as usize;
    nile_script::add_default_host_fn(name, move |args| {
        let Ok(args) = CString::new(args) else { return String::new() };
        let p = unsafe { callback(args.as_ptr(), user as *mut c_void) };
        if p.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    });
    true
}

/// Run the `nile` program with command-line arguments: `argv[0]` is the program's name and is
/// skipped; the rest are the `nile` program's own (`page.html`, a folder, `--png out.png`,
/// `--pack`, ...). Opens windows and returns the exit code once the last one closes (or after a
/// headless render). With no file it opens the start file of the pack, the program's folder or the
/// working directory.
///
/// # Safety
/// `argv` must point to `argc` valid strings. Main thread only.
#[no_mangle]
pub unsafe extern "C" fn nile_run(argc: i32, argv: *const *const c_char) -> i32 {
    let args: Vec<String> = (1..argc.max(0) as usize)
        .filter_map(|i| (!argv.is_null()).then(|| unsafe { text(*argv.add(i)) }).flatten().map(str::to_string))
        .collect();
    guard(|| nile_shell::run(args))
}

/// Open the HTML file at `path` (or the start file of a folder) in a window and run until the last
/// window closes. Returns the exit code, which is `application.quit(code)`'s. Main thread only.
///
/// # Safety
/// `path` must be a valid string.
#[no_mangle]
pub unsafe extern "C" fn nile_run_file(path: *const c_char) -> i32 {
    let Some(path) = text(path) else { return 2 };
    guard(|| nile_shell::run(vec![path.to_string()]))
}

/// Show `html` in a window `width` x `height` (logical pixels; 0 means the default) and run until
/// the last window closes. `base_dir` (may be NULL) is where relative `src`/`href`/`url()` paths
/// and `new Window({url})` resolve. Returns the exit code. Main thread only.
///
/// # Safety
/// `html` must be a valid string and `base_dir` NULL or valid.
#[no_mangle]
pub unsafe extern "C" fn nile_run_html(html: *const c_char, base_dir: *const c_char, width: f32, height: f32) -> i32 {
    let Some(html) = text(html) else { return 2 };
    let dir = text(base_dir).map(PathBuf::from).unwrap_or_default();
    let size = (if width > 0.0 { width } else { 900.0 }, if height > 0.0 { height } else { 640.0 });
    guard(|| {
        nile_dom::vfs::mount_self();
        let info = nile_script::WindowInfo { id: nile_script::next_window_id(), ..Default::default() };
        nile_shell::run_page(nile_engine::Page::html_window(html, dir, size, info))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_run_registers_host_functions_and_renders_headless() {
        unsafe extern "C" fn add(args: *const c_char, _user: *mut c_void) -> *const c_char {
            let v: Vec<f64> = CStr::from_ptr(args).to_str().unwrap().trim_matches(|c| c == '[' || c == ']').split(',').filter_map(|n| n.trim().parse().ok()).collect();
            if v.iter().sum::<f64>() == 5.0 { c"5".as_ptr() } else { c"0".as_ptr() }
        }
        let dir = std::env::temp_dir().join(format!("nile-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let page = dir.join("p.html");
        // The script runs at load: it must already see host.add, and shows the answer as text.
        std::fs::write(&page, r#"<body style="margin:0"><div id="r" style="font:40px sans-serif"></div><script>document.getElementById("r").textContent = "sum " + host.add(2, 3);</script></body>"#).unwrap();
        let out = dir.join("out.png");
        unsafe {
            assert!(nile_app_register_function(c"add".as_ptr(), Some(add), std::ptr::null_mut()));
            let args: Vec<CString> = ["nile", page.to_str().unwrap(), "--png", out.to_str().unwrap(), "--width", "200", "--height", "60"].iter().map(|s| CString::new(*s).unwrap()).collect();
            let ptrs: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
            assert_eq!(nile_run(ptrs.len() as i32, ptrs.as_ptr()), 0);
        }
        assert!(out.exists() && std::fs::metadata(&out).unwrap().len() > 200);
        // Bad arguments fail softly.
        unsafe {
            assert_eq!(nile_run_file(std::ptr::null()), 2);
            assert!(!nile_app_register_function(std::ptr::null(), None, std::ptr::null_mut()));
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
