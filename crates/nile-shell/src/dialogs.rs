//! Native dialogs through whatever the system ships (no extra dependencies): AppleScript on macOS,
//! PowerShell and Windows Forms on Windows, `zenity` or `kdialog` elsewhere. Each call blocks
//! until the user answers. This is the shell's [`Dialogs`] for scripts and for `<input type=file>`.

use std::{path::Path, process::Command};

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

fn lines(out: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(out).lines().map(|l| l.trim_end_matches('\r').to_string()).filter(|l| !l.is_empty()).collect()
}

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

fn osa(script: &str) -> Vec<String> {
    let mut c = Command::new("osascript");
    c.args(["-e", script]);
    run(c)
}

fn osa_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn ps(script: &str) -> Vec<String> {
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-Command", &format!("Add-Type -AssemblyName System.Windows.Forms; {script}")]);
    run(c)
}

fn ps_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn ps_filter(filters: &[FileFilter]) -> String {
    if filters.is_empty() {
        return ps_str("All files|*.*");
    }
    let parts: Vec<String> = filters
        .iter()
        .map(|f| {
            let pats = if f.extensions.is_empty() { "*.*".to_string() } else { f.extensions.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";") };
            format!("{}|{pats}", f.label)
        })
        .collect();
    ps_str(&parts.join("|"))
}

fn zenity_filters(c: &mut Command, filters: &[FileFilter]) {
    for f in filters {
        let pats = if f.extensions.is_empty() { "*".to_string() } else { f.extensions.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(" ") };
        c.arg(format!("--file-filter={} | {pats}", f.label));
    }
}

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
        } else if cfg!(windows) {
            let mut script = format!("$d = New-Object System.Windows.Forms.OpenFileDialog; $d.Multiselect = ${multiple}; $d.Filter = {};", ps_filter(filters));
            if !caption.is_empty() {
                script += &format!(" $d.Title = {};", ps_str(caption));
            }
            if let Some(d) = &start {
                script += &format!(" $d.InitialDirectory = {};", ps_str(d));
            }
            script += " if ($d.ShowDialog() -eq 'OK') { $d.FileNames -join \"`n\" }";
            ps(&script)
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
        } else if cfg!(windows) {
            let mut script = format!("$d = New-Object System.Windows.Forms.SaveFileDialog; $d.Filter = {}; $d.DefaultExt = {}; $d.FileName = {};", ps_filter(filters), ps_str(extension.trim_start_matches('.')), ps_str(&name));
            if !caption.is_empty() {
                script += &format!(" $d.Title = {};", ps_str(caption));
            }
            if let Some(d) = &start {
                script += &format!(" $d.InitialDirectory = {};", ps_str(d));
            }
            script += " if ($d.ShowDialog() -eq 'OK') { $d.FileName }";
            ps(&script)
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
        } else if cfg!(windows) {
            let mut script = String::from("$d = New-Object System.Windows.Forms.FolderBrowserDialog;");
            if !caption.is_empty() {
                script += &format!(" $d.Description = {};", ps_str(caption));
            }
            if let Some(d) = &start {
                script += &format!(" $d.SelectedPath = {};", ps_str(d));
            }
            script += " if ($d.ShowDialog() -eq 'OK') { $d.SelectedPath }";
            ps(&script)
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
        } else if cfg!(windows) {
            let (buttons, icon) = (if question { "YesNo" } else { "OK" }, match kind {
                MessageKind::Info => "Information",
                MessageKind::Alert => "Warning",
                MessageKind::Error => "Error",
                MessageKind::Question => "Question",
            });
            let out = ps(&format!("[System.Windows.Forms.MessageBox]::Show({}, {}, '{buttons}', '{icon}')", ps_str(text), ps_str(caption)));
            out.iter().any(|l| l == "Yes" || l == "OK")
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

    #[test]
    fn quoting_survives_awkward_text() {
        assert_eq!(osa_str("say \"hi\" \\ now"), "\"say \\\"hi\\\" \\\\ now\"");
        assert_eq!(ps_str("it's"), "'it''s'");
    }
}
