//! Native dialogs without extra dependencies: the Win32 common dialogs on Windows (owned by the
//! active window, so its parent is blocked while one is open), AppleScript on macOS, `zenity` or
//! `kdialog` elsewhere. Each call blocks until the user answers. This is the shell's [`Dialogs`] for scripts and for `<input type=file>`.

use std::path::Path;
#[cfg(not(windows))]
use std::process::Command;

use nile_engine::dialogs::{Dialogs, FileFilter, MessageAnswer, MessageKind, MessageStyle};

pub struct NativeDialogs;

/// `accept="image/*,.png"` as a filter list (only the `.ext` entries are usable).
pub fn accept_filters(accept: &str) -> Vec<FileFilter> {
    let extensions: Vec<String> = accept
        .split(',')
        .filter_map(|t| t.trim().strip_prefix('.'))
        .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(str::to_string)
        .collect();
    if extensions.is_empty() {
        Vec::new()
    } else {
        vec![FileFilter { label: "Files".into(), extensions }]
    }
}

pub fn pick_files(multiple: bool, accept: &str) -> Vec<String> {
    NativeDialogs.open_files(multiple, &accept_filters(accept), "", "")
}

/// A folder to start in: `path` itself if it is one, else its parent.
fn start_dir(path: &str) -> Option<String> {
    let p = Path::new(path);
    if path.is_empty() {
        None
    } else if p.is_dir() {
        Some(path.to_string())
    } else {
        p.parent().filter(|d| d.is_dir()).map(|d| d.to_string_lossy().into_owned())
    }
}

#[cfg(not(windows))]
fn lines(out: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(out).lines().map(|l| l.trim_end_matches('\r').to_string()).filter(|l| !l.is_empty()).collect()
}

#[cfg(not(windows))]
/// Runs `command`; its output lines on success, nothing when cancelled. Real failures are reported.
fn run(mut command: Command) -> Vec<String> {
    match command.output() {
        Ok(o) if o.status.success() => lines(&o.stdout),
        Ok(o) => {
            // A cancelled chooser also fails (osascript says "User canceled").
            let err = String::from_utf8_lossy(&o.stderr);
            if !err.trim().is_empty() && !err.contains("User canceled") {
                eprintln!("[dialog] {}", err.trim());
            }
            Vec::new()
        }
        Err(e) => {
            eprintln!("[dialog] could not run {:?}: {e}", command.get_program());
            Vec::new()
        }
    }
}

#[cfg(not(windows))]
fn osa(script: &str) -> Vec<String> {
    let mut c = Command::new("osascript");
    c.args(["-e", script]);
    run(c)
}

#[cfg(not(windows))]
fn osa_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(not(windows))]
fn zenity_filters(c: &mut Command, filters: &[FileFilter]) {
    for f in filters {
        let pats = if f.extensions.is_empty() { "*".to_string() } else { f.extensions.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(" ") };
        c.arg(format!("--file-filter={} | {pats}", f.label));
    }
}

#[cfg(not(windows))]
impl Dialogs for NativeDialogs {
    fn open_files(&self, multiple: bool, filters: &[FileFilter], caption: &str, dir: &str) -> Vec<String> {
        let start = start_dir(dir);
        if cfg!(target_os = "macos") {
            let exts: Vec<String> = filters.iter().flat_map(|f| f.extensions.iter().map(|e| osa_str(e))).collect();
            let mut script = String::from("set fs to choose file");
            if !caption.is_empty() {
                script += &format!(" with prompt {}", osa_str(caption));
            }
            if !exts.is_empty() {
                script += &format!(" of type {{{}}}", exts.join(", "));
            }
            if let Some(d) = &start {
                script += &format!(" default location (POSIX file {})", osa_str(d));
            }
            if multiple {
                script += " with multiple selections allowed";
            }
            script += "\nif class of fs is not list then set fs to {fs}\nset out to \"\"\nrepeat with f in fs\nset out to out & POSIX path of f & linefeed\nend repeat\nout";
            osa(&script)
        } else {
            let mut zenity = Command::new("zenity");
            zenity.arg("--file-selection").arg("--separator=\n");
            if multiple {
                zenity.arg("--multiple");
            }
            if !caption.is_empty() {
                zenity.arg(format!("--title={caption}"));
            }
            if let Some(d) = &start {
                zenity.arg(format!("--filename={}/", d.trim_end_matches('/')));
            }
            zenity_filters(&mut zenity, filters);
            let found = zenity.output();
            match found {
                Ok(_) => run(zenity),
                Err(_) => {
                    let mut k = Command::new("kdialog");
                    k.arg("--getopenfilename").arg(start.unwrap_or_else(|| ".".into()));
                    if multiple {
                        k.args(["--multiple", "--separate-output"]);
                    }
                    run(k)
                }
            }
        }
    }

    fn save_file(&self, filters: &[FileFilter], extension: &str, caption: &str, path: &str) -> Option<String> {
        let start = start_dir(path);
        let name = if Path::new(path).is_dir() || path.is_empty() { String::new() } else { Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() };
        let with_ext = |p: String| {
            if extension.is_empty() || Path::new(&p).extension().is_some() {
                p
            } else {
                format!("{p}.{}", extension.trim_start_matches('.'))
            }
        };
        let answer = if cfg!(target_os = "macos") {
            let mut script = String::from("set f to choose file name");
            if !caption.is_empty() {
                script += &format!(" with prompt {}", osa_str(caption));
            }
            if !name.is_empty() {
                script += &format!(" default name {}", osa_str(&name));
            }
            if let Some(d) = &start {
                script += &format!(" default location (POSIX file {})", osa_str(d));
            }
            script += "\nPOSIX path of f";
            osa(&script)
        } else {
            let mut zenity = Command::new("zenity");
            zenity.args(["--file-selection", "--save", "--confirm-overwrite"]);
            if !caption.is_empty() {
                zenity.arg(format!("--title={caption}"));
            }
            let initial = match (&start, name.is_empty()) {
                (Some(d), false) => format!("{}/{name}", d.trim_end_matches('/')),
                (Some(d), true) => format!("{}/", d.trim_end_matches('/')),
                (None, false) => name.clone(),
                (None, true) => String::new(),
            };
            if !initial.is_empty() {
                zenity.arg(format!("--filename={initial}"));
            }
            zenity_filters(&mut zenity, filters);
            run(zenity)
        };
        answer.into_iter().next().map(with_ext)
    }

    fn pick_folder(&self, caption: &str, dir: &str) -> Option<String> {
        let start = start_dir(dir);
        let answer = if cfg!(target_os = "macos") {
            let mut script = String::from("set f to choose folder");
            if !caption.is_empty() {
                script += &format!(" with prompt {}", osa_str(caption));
            }
            if let Some(d) = &start {
                script += &format!(" default location (POSIX file {})", osa_str(d));
            }
            script += "\nPOSIX path of f";
            osa(&script)
        } else {
            let mut zenity = Command::new("zenity");
            zenity.args(["--file-selection", "--directory"]);
            if !caption.is_empty() {
                zenity.arg(format!("--title={caption}"));
            }
            if let Some(d) = &start {
                zenity.arg(format!("--filename={}/", d.trim_end_matches('/')));
            }
            run(zenity)
        };
        answer.into_iter().next()
    }

    fn message(&self, kind: MessageKind, caption: &str, text: &str) -> bool {
        let question = kind == MessageKind::Question;
        if cfg!(target_os = "macos") {
            let icon = match kind {
                MessageKind::Info | MessageKind::Question => "note",
                MessageKind::Alert => "caution",
                MessageKind::Error => "stop",
            };
            let buttons = if question { "{\"No\", \"Yes\"} default button \"Yes\"" } else { "{\"OK\"} default button \"OK\"" };
            let script = format!("display dialog {} with title {} buttons {buttons} with icon {icon}", osa_str(text), osa_str(caption));
            // Cancelling (Escape) is "no"; the button name comes back as `button returned:Yes`.
            let out = osa(&script).join(" ");
            !question || out.contains("button returned:Yes")
        } else {
            let flag = match kind {
                MessageKind::Info => "--info",
                MessageKind::Alert => "--warning",
                MessageKind::Error => "--error",
                MessageKind::Question => "--question",
            };
            let mut zenity = Command::new("zenity");
            zenity.args([flag, "--no-markup", &format!("--title={caption}"), &format!("--text={text}")]);
            match zenity.status() {
                Ok(s) => s.success(),
                Err(_) => {
                    let mut k = Command::new("kdialog");
                    k.arg(match kind {
                        MessageKind::Info => "--msgbox",
                        MessageKind::Alert => "--sorry",
                        MessageKind::Error => "--error",
                        MessageKind::Question => "--yesno",
                    })
                    .arg(text)
                    .arg("--title")
                    .arg(caption);
                    k.status().map(|s| s.success()).unwrap_or(!question)
                }
            }
        }
    }

    fn message_with(&self, kind: MessageKind, caption: &str, text: &str, style: MessageStyle) -> MessageAnswer {
        if cfg!(target_os = "macos") && style.cancel && kind == MessageKind::Question {
            // Cancel (or Escape) makes `display dialog` fail, so only a named Yes/No button counts.
            let script = format!(
                "display dialog {} with title {} buttons {{\"Cancel\", \"No\", \"Yes\"}} default button \"Yes\" cancel button \"Cancel\" with icon note",
                osa_str(text),
                osa_str(caption)
            );
            let out = osa(&script).join(" ");
            return if out.contains("button returned:Yes") {
                MessageAnswer::Yes
            } else if out.contains("button returned:No") {
                MessageAnswer::No
            } else {
                MessageAnswer::Cancel
            };
        }
        // TODO: a Cancel button with zenity/kdialog (`--extra-button`, `--yesnocancel`); until then it is a plain Yes/No box.
        if self.message(kind, caption, text) { MessageAnswer::Yes } else { MessageAnswer::No }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_extensions_from_accept() {
        let f = accept_filters(".png, .JPG,image/*,.,.a-b");
        assert_eq!(f[0].extensions, vec!["png", "JPG"]);
        assert!(accept_filters("image/*").is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn quoting_survives_awkward_text() {
        assert_eq!(osa_str("say \"hi\" \\ now"), "\"say \\\"hi\\\" \\\\ now\"");
    }
}

#[cfg(windows)]
impl Dialogs for NativeDialogs {
    fn open_files(&self, multiple: bool, filters: &[FileFilter], caption: &str, dir: &str) -> Vec<String> {
        win::open_files(multiple, filters, caption, start_dir(dir).as_deref())
    }

    fn save_file(&self, filters: &[FileFilter], extension: &str, caption: &str, path: &str) -> Option<String> {
        let name = if Path::new(path).is_dir() || path.is_empty() { String::new() } else { Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() };
        win::save_file(filters, extension.trim_start_matches('.'), caption, start_dir(path).as_deref(), &name)
    }

    fn pick_folder(&self, caption: &str, dir: &str) -> Option<String> {
        win::pick_folder(caption, start_dir(dir).as_deref())
    }

    fn message(&self, kind: MessageKind, caption: &str, text: &str) -> bool {
        win::message(kind, caption, text, MessageStyle::default()) == MessageAnswer::Yes
    }

    fn message_with(&self, kind: MessageKind, caption: &str, text: &str, style: MessageStyle) -> MessageAnswer {
        win::message(kind, caption, text, style)
    }
}

/// Tell the dialogs which window they belong to (Windows; elsewhere the helper programs place themselves).
#[cfg(windows)]
pub use win::set_owner;

/// The Win32 common dialogs. They are owned by the window that is active on this thread, which
/// disables it (its parent can't be used or moved around) until the dialog closes, and they open at
/// once: no helper process is started.
#[cfg(windows)]
mod win {
    use std::{
        ffi::c_void,
        ptr::{null, null_mut},
    };

    use nile_engine::dialogs::{FileFilter, MessageAnswer, MessageKind, MessageStyle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::{
            ApplicationInstallationAndServicing::{ActivateActCtx, CreateActCtxW, DeactivateActCtx, ACTCTXW},
            Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED},
            LibraryLoader::{GetProcAddress, LoadLibraryW},
        },
        UI::{
            Controls::{
                Dialogs::{GetOpenFileNameW, GetSaveFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW},
                TASKDIALOGCONFIG,
            },
            Input::KeyboardAndMouse::GetActiveWindow,
            WindowsAndMessaging::{EnumWindows, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowThreadProcessId, ShowWindow, GW_OWNER, SW_SHOWNORMAL, IsIconic, IsWindow, IsWindowVisible, MessageBoxW, SendMessageW, SetForegroundWindow, MB_SETFOREGROUND, MB_TOPMOST, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_YESNO, MB_YESNOCANCEL, IDCANCEL},
        },
    };

    /// The window the shell is handling events for (set before page code runs): dialogs belong to it.
    static OWNER: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

    pub fn set_owner(hwnd: isize) {
        OWNER.store(hwnd, std::sync::atomic::Ordering::Relaxed);
    }

    /// Is `hwnd` one of our windows that is shown and not minimized (a dialog owned by anything else
    /// never shows up)?
    fn usable(hwnd: isize) -> bool {
        // SAFETY: plain queries.
        unsafe {
            if hwnd == 0 || IsWindow(hwnd) == 0 {
                return false;
            }
            let mut process = 0u32;
            GetWindowThreadProcessId(hwnd, &mut process);
            process == std::process::id() && IsWindowVisible(hwnd) != 0 && IsIconic(hwnd) == 0
        }
    }

    /// The window a dialog belongs to: the one the shell is serving (`set_owner`), else this thread's active
    /// or the foreground window, but only if it is one of ours that is shown. With no usable owner (`None`) the
    /// box has none and is made topmost instead, so it can't open behind the application.
    fn owner() -> Option<isize> {
        let set = OWNER.load(std::sync::atomic::Ordering::Relaxed);
        if usable(set) {
            return Some(set);
        }
        // SAFETY: plain queries.
        let guess = unsafe {
            match GetActiveWindow() {
                0 => GetForegroundWindow(),
                active => active,
            }
        };
        usable(guess).then_some(guess)
    }

    /// What `keep_visible` looks for: dialogs of this process owned by `owner` that are not shown.
    struct Hidden {
        owner: isize,
        found: Vec<isize>,
    }

    unsafe extern "system" fn collect_hidden(hwnd: isize, lparam: isize) -> i32 {
        // SAFETY: `lparam` is the `Hidden` that `keep_visible` keeps alive for the whole enumeration.
        let hidden = &mut *(lparam as *mut Hidden);
        let mut process = 0u32;
        GetWindowThreadProcessId(hwnd, &mut process);
        if process == std::process::id() && IsWindowVisible(hwnd) == 0 && GetWindow(hwnd, GW_OWNER) == hidden.owner {
            let mut class = [0u16; 16];
            let n = GetClassNameW(hwnd, class.as_mut_ptr(), 16).max(0) as usize;
            if String::from_utf16_lossy(&class[..n]) == "#32770" {
                hidden.found.push(hwnd);
            }
        }
        1
    }

    /// While a message box is open (`open`), show it if Windows created it but never made it visible. On
    /// Windows 11 (seen in a VM) a `MessageBoxW` could stay hidden with an empty title: its thread was
    /// answering messages and the owner was disabled (it beeped when clicked), so there was a box nobody could
    /// see or answer. Showing the dialog by hand brought it up. Cheap: three looks in the first 1.5 s.
    fn keep_visible(owner: Option<isize>, open: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        use std::sync::atomic::Ordering::Relaxed;
        std::thread::spawn(move || {
            for wait in [250u64, 500, 750] {
                std::thread::sleep(std::time::Duration::from_millis(wait));
                if !open.load(Relaxed) {
                    return;
                }
                let mut hidden = Hidden { owner: owner.unwrap_or(0), found: Vec::new() };
                // SAFETY: plain window calls; `hidden` outlives the enumeration, and a handle that has gone away just fails.
                unsafe {
                    EnumWindows(Some(collect_hidden), &mut hidden as *mut Hidden as isize);
                    for dialog in hidden.found {
                        if open.load(Relaxed) {
                            if std::env::var_os("NILE_TRACE").is_some() {
                                eprintln!("[dialog] the box {dialog} was not shown; showing it");
                            }
                            ShowWindow(dialog, SW_SHOWNORMAL);
                            SetForegroundWindow(dialog);
                        }
                    }
                }
            }
        });
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn from_wide(buf: &[u16]) -> String {
        String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(buf.len())])
    }

    /// `label\0*.a;*.b\0label\0*.*\0\0`, the list the common dialogs want.
    fn filter_list(filters: &[FileFilter]) -> Vec<u16> {
        let mut out = Vec::new();
        let mut add = |text: &str| {
            out.extend(text.encode_utf16());
            out.push(0);
        };
        if filters.is_empty() {
            add("All files");
            add("*.*");
        }
        for f in filters {
            add(&f.label);
            add(&if f.extensions.is_empty() { "*.*".to_string() } else { f.extensions.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";") });
        }
        out.push(0);
        out
    }

    /// Runs a get-open/save-file dialog with `flags`; the buffer's contents when the user accepted.
    fn file_dialog(save: bool, flags: u32, filters: &[FileFilter], caption: &str, dir: Option<&str>, name: &str, ext: &str) -> Option<Vec<u16>> {
        let list = filter_list(filters);
        let (title, initial, default_ext) = (wide(caption), dir.map(wide), wide(ext));
        let mut file = vec![0u16; 64 * 1024];
        for (slot, c) in file.iter_mut().zip(name.encode_utf16()) {
            *slot = c;
        }
        // SAFETY: an all-zero OPENFILENAMEW is a valid empty one; the buffers outlive the call.
        let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
        ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = owner().unwrap_or(0);
        ofn.lpstrFilter = list.as_ptr();
        ofn.nFilterIndex = 1;
        ofn.lpstrFile = file.as_mut_ptr();
        ofn.nMaxFile = file.len() as u32;
        ofn.lpstrInitialDir = initial.as_ref().map_or(null(), |d| d.as_ptr());
        ofn.lpstrTitle = if caption.is_empty() { null() } else { title.as_ptr() };
        ofn.lpstrDefExt = if ext.is_empty() { null() } else { default_ext.as_ptr() };
        ofn.Flags = flags | OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST;
        let ok = unsafe { if save { GetSaveFileNameW(&mut ofn) } else { GetOpenFileNameW(&mut ofn) } };
        (ok != 0).then_some(file)
    }

    pub fn open_files(multiple: bool, filters: &[FileFilter], caption: &str, dir: Option<&str>) -> Vec<String> {
        let flags = OFN_FILEMUSTEXIST | if multiple { OFN_ALLOWMULTISELECT } else { 0 };
        let Some(buf) = file_dialog(false, flags, filters, caption, dir, "", "") else { return Vec::new() };
        // One path, or `folder\0name\0name\0\0` when several were chosen.
        let mut parts = buf.split(|c| *c == 0).take_while(|p| !p.is_empty()).map(String::from_utf16_lossy);
        let first = parts.next().unwrap_or_default();
        let rest: Vec<String> = parts.collect();
        if rest.is_empty() {
            vec![first]
        } else {
            rest.into_iter().map(|n| format!("{}\\{n}", first.trim_end_matches('\\'))).collect()
        }
    }

    pub fn save_file(filters: &[FileFilter], extension: &str, caption: &str, dir: Option<&str>, name: &str) -> Option<String> {
        file_dialog(true, OFN_OVERWRITEPROMPT, filters, caption, dir, name, extension).map(|b| from_wide(&b)).filter(|p| !p.is_empty())
    }

    /// `BROWSEINFOW` and the two shell functions, declared here because windows-sys 0.52 gates them
    /// behind a feature it doesn't define.
    #[repr(C)]
    struct BrowseInfo {
        owner: isize,
        root: *mut c_void,
        display_name: *mut u16,
        title: *const u16,
        flags: u32,
        callback: Option<unsafe extern "system" fn(isize, u32, isize, isize) -> i32>,
        lparam: isize,
        image: i32,
    }

    #[link(name = "shell32")]
    extern "system" {
        fn SHBrowseForFolderW(info: *const BrowseInfo) -> *mut c_void;
        fn SHGetPathFromIDListW(item: *const c_void, path: *mut u16) -> i32;
    }

    const BIF_RETURNONLYFSDIRS: u32 = 0x1;
    const BIF_NEWDIALOGSTYLE: u32 = 0x40;
    const BFFM_INITIALIZED: u32 = 1;
    const BFFM_SETSELECTIONW: u32 = 0x400 + 103;

    /// Opens the folder chooser on the folder `data` (a NUL-terminated wide path) points to.
    unsafe extern "system" fn start_at(hwnd: isize, msg: u32, _l: isize, data: isize) -> i32 {
        if msg == BFFM_INITIALIZED {
            unsafe {
                // Come to the front even when the chooser was opened by a timer or a background script.
                SetForegroundWindow(hwnd);
                if data != 0 {
                    SendMessageW(hwnd, BFFM_SETSELECTIONW, 1, data);
                }
            }
        }
        0
    }

    pub fn pick_folder(caption: &str, dir: Option<&str>) -> Option<String> {
        let title = wide(caption);
        let start = dir.map(wide);
        let mut path = [0u16; 1024];
        // SAFETY: the structure is filled in before the call and the buffers outlive it; the returned
        // item list is freed after use.
        unsafe {
            // The new-style dialog needs COM; it is fine if the thread already has it.
            CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
            let info = BrowseInfo {
                owner: owner().unwrap_or(0),
                root: null_mut(),
                display_name: null_mut(),
                title: if caption.is_empty() { null() } else { title.as_ptr() },
                flags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
                callback: Some(start_at),
                lparam: start.as_ref().map_or(0, |s| s.as_ptr() as isize),
                image: 0,
            };
            let item = SHBrowseForFolderW(&info);
            if item.is_null() {
                return None;
            }
            let found = SHGetPathFromIDListW(item, path.as_mut_ptr()) != 0;
            CoTaskMemFree(item);
            found.then(|| from_wide(&path)).filter(|p| !p.is_empty())
        }
    }

    /// The task dialog function (comctl32 version 6) and the activation context it must run in. Linking against
    /// `TaskDialogIndirect` would stop the program from starting where version 5 is loaded, and the exe carries
    /// no manifest asking for version 6, so a small manifest is activated around loading and calling it.
    type TaskDialogIndirectFn = unsafe extern "system" fn(*const TASKDIALOGCONFIG, *mut i32, *mut i32, *mut i32) -> i32;

    fn task_dialog_fn() -> Option<(isize, TaskDialogIndirectFn)> {
        static FN: std::sync::OnceLock<Option<(isize, TaskDialogIndirectFn)>> = std::sync::OnceLock::new();
        *FN.get_or_init(|| {
            const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"><dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency></assembly>"#;
            let path = std::env::temp_dir().join(format!("nile-comctl6-{}.manifest", std::process::id()));
            std::fs::write(&path, MANIFEST).ok()?;
            let source = wide(&path.to_string_lossy());
            // SAFETY: plain calls; the structure and the strings outlive them.
            let found = unsafe {
                let mut config: ACTCTXW = std::mem::zeroed();
                config.cbSize = std::mem::size_of::<ACTCTXW>() as u32;
                config.lpSource = source.as_ptr();
                let context = CreateActCtxW(&config);
                let _ = std::fs::remove_file(&path);
                if context == INVALID_HANDLE_VALUE {
                    return None;
                }
                let mut cookie = 0usize;
                if ActivateActCtx(context, &mut cookie) == 0 {
                    return None;
                }
                let library = LoadLibraryW(wide("comctl32.dll").as_ptr());
                let function = if library == 0 { None } else { GetProcAddress(library, b"TaskDialogIndirect\0".as_ptr()) };
                DeactivateActCtx(0, cookie);
                function.map(|f| (context, std::mem::transmute::<unsafe extern "system" fn() -> isize, TaskDialogIndirectFn>(f)))
            };
            if found.is_none() {
                eprintln!("[dialog] TaskDialogIndirect is not available; using a plain message box");
            }
            found
        })
    }

    /// A task dialog: like the message box, but the title bar's close button and Escape work (they answer
    /// `IDCANCEL`). `None` when it can't be used, so the caller falls back to `MessageBoxW`.
    fn task_dialog(owner: Option<isize>, kind: MessageKind, caption: &str, text: &str, cancel: bool) -> Option<i32> {
        const TDF_ALLOW_DIALOG_CANCELLATION: i32 = 0x0008;
        const TDF_POSITION_RELATIVE_TO_WINDOW: i32 = 0x1000;
        const TDCBF_OK: i32 = 1;
        const TDCBF_YES: i32 = 2;
        const TDCBF_NO: i32 = 4;
        const TDCBF_CANCEL: i32 = 8;
        let (context, function) = task_dialog_fn()?;
        let (caption, text) = (wide(caption), wide(text));
        // MAKEINTRESOURCE(-1 / -2 / -3): TD_WARNING_ICON, TD_ERROR_ICON, TD_INFORMATION_ICON. A question has no icon.
        let icon: usize = match kind {
            MessageKind::Alert => 0xFFFF,
            MessageKind::Error => 0xFFFE,
            MessageKind::Info => 0xFFFD,
            MessageKind::Question => 0,
        };
        let buttons = if kind == MessageKind::Question { TDCBF_YES | TDCBF_NO | if cancel { TDCBF_CANCEL } else { 0 } } else { TDCBF_OK };
        let mut pressed = 0i32;
        // SAFETY: the configuration and its strings live across the call; the activation context is active for it.
        let result = unsafe {
            let mut config: TASKDIALOGCONFIG = std::mem::zeroed();
            config.cbSize = std::mem::size_of::<TASKDIALOGCONFIG>() as u32;
            config.hwndParent = owner.unwrap_or(0);
            config.dwFlags = TDF_ALLOW_DIALOG_CANCELLATION | if owner.is_some() { TDF_POSITION_RELATIVE_TO_WINDOW } else { 0 };
            config.dwCommonButtons = buttons;
            config.pszWindowTitle = caption.as_ptr();
            config.Anonymous1.pszMainIcon = icon as *const u16;
            config.pszContent = text.as_ptr();
            let mut cookie = 0usize;
            if ActivateActCtx(context, &mut cookie) == 0 {
                return None;
            }
            let result = function(&config, &mut pressed, std::ptr::null_mut(), std::ptr::null_mut());
            DeactivateActCtx(0, cookie);
            result
        };
        (result >= 0).then_some(pressed)
    }

    pub fn message(kind: MessageKind, caption: &str, text: &str, style: MessageStyle) -> MessageAnswer {
        let question = kind == MessageKind::Question;
        let icon = match kind {
            MessageKind::Info => MB_ICONINFORMATION,
            MessageKind::Alert => MB_ICONWARNING,
            MessageKind::Error => MB_ICONERROR,
            MessageKind::Question => MB_ICONQUESTION,
        };
        let owner = owner();
        if std::env::var_os("NILE_TRACE").is_some() {
            // Before the call: a box that opens out of sight blocks here, so the line after it never prints.
            // SAFETY: a plain query.
            let foreground = unsafe { GetForegroundWindow() };
            eprintln!("[dialog] opening {kind:?} {style:?} owner {owner:?} usable {} foreground {foreground}", owner.is_some_and(usable));
        }
        let open = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        keep_visible(owner, open.clone());
        let task = if style.task { task_dialog(owner, kind, caption, text, style.cancel) } else { None };
        let answer = task.unwrap_or_else(|| {
            let (text, caption) = (wide(text), wide(caption));
            // Without a usable owner the box is topmost so it can't open behind the application.
            let buttons = if question && style.cancel { MB_YESNOCANCEL } else if question { MB_YESNO } else { MB_OK };
            let styles = icon | MB_SETFOREGROUND | buttons;
            let mut answer = unsafe { MessageBoxW(owner.unwrap_or(0), text.as_ptr(), caption.as_ptr(), if owner.is_some() { styles } else { styles | MB_TOPMOST }) };
            if answer == 0 {
                // 0 means the box could not be shown at all: say why, and try once more with no owner.
                eprintln!("[dialog] MessageBoxW({kind:?}, owner {owner:?}) failed: {}", std::io::Error::last_os_error());
                answer = unsafe { MessageBoxW(0, text.as_ptr(), caption.as_ptr(), styles | MB_TOPMOST) };
            }
            answer
        });
        open.store(false, std::sync::atomic::Ordering::Relaxed);
        if std::env::var_os("NILE_TRACE").is_some() {
            eprintln!("[dialog] {kind:?} owner {owner:?} task {} -> {answer}", task.is_some());
        }
        match answer {
            _ if !question => MessageAnswer::Yes,
            IDYES => MessageAnswer::Yes,
            // The close button and Escape of a task dialog (and Cancel itself) answer IDCANCEL.
            IDCANCEL if style.cancel => MessageAnswer::Cancel,
            _ => MessageAnswer::No,
        }
    }
}
