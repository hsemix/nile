//! Native dialogs without extra dependencies: the Win32 common dialogs on Windows (owned by the
//! active window, so its parent is blocked while one is open), AppleScript on macOS, `zenity` or
//! `kdialog` elsewhere. Each call blocks until the user answers. This is the shell's [`Dialogs`] for scripts and for `<input type=file>`.

use std::path::Path;
#[cfg(not(windows))]
use std::process::Command;

use nile_engine::dialogs::{Dialogs, FileFilter, MessageKind};

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
        win::message(kind, caption, text)
    }
}

/// The Win32 common dialogs. They are owned by the window that is active on this thread, which
/// disables it (its parent can't be used or moved around) until the dialog closes, and they open at
/// once: no helper process is started.
#[cfg(windows)]
mod win {
    use std::{
        ffi::c_void,
        ptr::{null, null_mut},
    };

    use nile_engine::dialogs::{FileFilter, MessageKind};
    use windows_sys::Win32::{
        System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED},
        UI::{
            Controls::Dialogs::{GetOpenFileNameW, GetSaveFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW},
            Input::KeyboardAndMouse::GetActiveWindow,
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible, MessageBoxW, SendMessageW, SetForegroundWindow, MB_SETFOREGROUND, MB_TOPMOST, IDOK, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_YESNO},
        },
    };

    /// The window a dialog belongs to: this thread's active window, else the foreground one, but only a window of
    /// ours that is shown and not minimized. A dialog owned by a hidden or minimized window (a bootstrap script's
    /// host window, a minimized app) or by another program's window never shows up, so those get no owner at all
    /// (`None`) and the box is made topmost instead.
    fn owner() -> Option<isize> {
        // SAFETY: plain queries.
        unsafe {
            let candidate = match GetActiveWindow() {
                0 => GetForegroundWindow(),
                active => active,
            };
            let mut process = 0u32;
            GetWindowThreadProcessId(candidate, &mut process);
            let ours = candidate != 0 && process == std::process::id();
            (ours && IsWindowVisible(candidate) != 0 && IsIconic(candidate) == 0).then_some(candidate)
        }
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

    pub fn message(kind: MessageKind, caption: &str, text: &str) -> bool {
        let question = kind == MessageKind::Question;
        let icon = match kind {
            MessageKind::Info => MB_ICONINFORMATION,
            MessageKind::Alert => MB_ICONWARNING,
            MessageKind::Error => MB_ICONERROR,
            MessageKind::Question => MB_ICONQUESTION,
        };
        let (text, caption) = (wide(text), wide(caption));
        let owner = owner();
        // Without a usable owner the box is topmost so it can't open behind the application.
        let styles = icon | MB_SETFOREGROUND | if question { MB_YESNO } else { MB_OK };
        let mut answer = unsafe { MessageBoxW(owner.unwrap_or(0), text.as_ptr(), caption.as_ptr(), if owner.is_some() { styles } else { styles | MB_TOPMOST }) };
        if answer == 0 {
            // 0 means the box could not be shown at all: say why, and try once more with no owner.
            eprintln!("[dialog] MessageBoxW({kind:?}, owner {owner:?}) failed: {}", std::io::Error::last_os_error());
            answer = unsafe { MessageBoxW(0, text.as_ptr(), caption.as_ptr(), styles | MB_TOPMOST) };
        }
        if std::env::var_os("NILE_TRACE").is_some() {
            eprintln!("[dialog] {kind:?} owner {owner:?} -> {answer}");
        }
        answer == IDYES || (!question && answer == IDOK)
    }
}
