//! Making the running program a citizen of the desktop at run time: the default handler of a URL scheme or a
//! file type, and starting at login. Everything is per user (no administrator rights) and best effort: each
//! function says whether the system took it.
//!
//! macOS cannot do the first two from a running program: the scheme and the document types are declared in the
//! app bundle's `Info.plist` (`nile --pack ... --protocol myapp --file-type md`).

use std::path::PathBuf;

/// The MIME type of a common extension (the Linux desktop associates MIME types, not extensions).
pub fn mime_of(ext: &str) -> Option<&'static str> {
    Some(match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "txt" | "text" | "log" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "wav" => "audio/x-wav",
        "flac" => "audio/flac",
        "ogg" | "oga" => "audio/ogg",
        "m4a" | "aac" => "audio/mp4",
        "mp4" => "video/mp4",
        _ => return None,
    })
}

fn exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// The program's name: its file name without the extension (`My App`).
fn name() -> String {
    exe().and_then(|e| e.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "nile-app".into())
}

/// A name safe in file names and registry keys.
fn id() -> String {
    name().chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

fn valid_scheme(s: &str) -> bool {
    !s.is_empty() && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn valid_ext(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(not(target_os = "macos"))]
fn run(program: &str, args: &[&str]) -> bool {
    let mut command = std::process::Command::new(program);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command.output().is_ok_and(|o| o.status.success())
}

/// Make this program the handler of `scheme://` links.
pub fn register_protocol(scheme: &str) -> bool {
    let scheme = scheme.trim().to_ascii_lowercase();
    let (Some(exe), true) = (exe(), valid_scheme(&scheme)) else { return false };
    platform::register_protocol(&exe, &scheme)
}

/// Make this program a handler of files ending in `.ext`.
pub fn register_file_type(ext: &str) -> bool {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    let (Some(exe), true) = (exe(), valid_ext(&ext)) else { return false };
    platform::register_file_type(&exe, &ext)
}

/// Start (or stop starting) the program when the user logs in.
pub fn set_login_item(enabled: bool) -> bool {
    let Some(exe) = exe() else { return false };
    platform::set_login_item(&exe, enabled)
}

pub fn login_item() -> bool {
    platform::login_item()
}

#[cfg(windows)]
mod platform {
    use super::*;

    fn classes(sub: &str) -> String {
        format!(r"HKCU\Software\Classes\{sub}")
    }

    fn command_value(exe: &std::path::Path) -> String {
        format!("\"{}\" \"%1\"", exe.display())
    }

    pub fn register_protocol(exe: &std::path::Path, scheme: &str) -> bool {
        let key = classes(scheme);
        run("reg", &["add", &key, "/ve", "/d", &format!("URL:{}", name()), "/f"])
            && run("reg", &["add", &key, "/v", "URL Protocol", "/d", "", "/f"])
            && run("reg", &["add", &format!(r"{key}\shell\open\command"), "/ve", "/d", &command_value(exe), "/f"])
    }

    pub fn register_file_type(exe: &std::path::Path, ext: &str) -> bool {
        let prog = format!("{}.{ext}", id());
        run("reg", &["add", &classes(&format!(".{ext}")), "/ve", "/d", &prog, "/f"])
            && run("reg", &["add", &classes(&prog), "/ve", "/d", &format!("{} .{ext} file", name()), "/f"])
            && run("reg", &["add", &classes(&format!(r"{prog}\shell\open\command")), "/ve", "/d", &command_value(exe), "/f"])
    }

    const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

    pub fn set_login_item(exe: &std::path::Path, enabled: bool) -> bool {
        if enabled {
            run("reg", &["add", RUN, "/v", &id(), "/d", &format!("\"{}\"", exe.display()), "/f"])
        } else {
            !login_item() || run("reg", &["delete", RUN, "/v", &id(), "/f"])
        }
    }

    pub fn login_item() -> bool {
        run("reg", &["query", RUN, "/v", &id()])
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    fn agent() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/LaunchAgents").join(format!("app.nile.{}.plist", id())))
    }

    pub fn register_protocol(_exe: &std::path::Path, _scheme: &str) -> bool {
        false
    }

    pub fn register_file_type(_exe: &std::path::Path, _ext: &str) -> bool {
        false
    }

    pub fn set_login_item(exe: &std::path::Path, enabled: bool) -> bool {
        let Some(path) = agent() else { return false };
        if enabled {
            let esc = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            let plist = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n  <key>Label</key><string>app.nile.{}</string>\n  <key>ProgramArguments</key><array><string>{}</string></array>\n  <key>RunAtLoad</key><true/>\n</dict></plist>\n",
                esc(&id()),
                esc(&exe.to_string_lossy())
            );
            path.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(path, plist).is_ok()
        } else {
            !path.exists() || std::fs::remove_file(path).is_ok()
        }
    }

    pub fn login_item() -> bool {
        agent().is_some_and(|p| p.exists())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use super::*;

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    fn data_dir() -> Option<PathBuf> {
        Some(std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or(home()?.join(".local/share")))
    }

    fn config_dir() -> Option<PathBuf> {
        Some(std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or(home()?.join(".config")))
    }

    fn entry_text(exe: &std::path::Path, mimes: &[String]) -> String {
        let quoted = format!("\"{}\"", exe.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""));
        let mut out = format!("[Desktop Entry]\nType=Application\nName={}\nExec={quoted} %U\nTerminal=false\nCategories=Utility;\n", name());
        if !mimes.is_empty() {
            out += &format!("MimeType={};\n", mimes.join(";"));
        }
        out
    }

    /// The MIME types the entry already lists, plus `add`.
    fn with_mime(path: &std::path::Path, add: &str) -> Vec<String> {
        let mut mimes: Vec<String> = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| t.lines().find_map(|l| l.strip_prefix("MimeType=").map(str::to_string)))
            .map(|l| l.split(';').filter(|m| !m.is_empty()).map(str::to_string).collect())
            .unwrap_or_default();
        if !mimes.iter().any(|m| m == add) {
            mimes.push(add.to_string());
        }
        mimes
    }

    fn register(exe: &std::path::Path, mime: &str) -> bool {
        let Some(dir) = data_dir().map(|d| d.join("applications")) else { return false };
        let file = format!("{}.desktop", id());
        let path = dir.join(&file);
        let mimes = with_mime(&path, mime);
        std::fs::create_dir_all(&dir).is_ok()
            && std::fs::write(&path, entry_text(exe, &mimes)).is_ok()
            && {
                let _ = run("update-desktop-database", &[&dir.to_string_lossy()]);
                run("xdg-mime", &["default", &file, mime])
            }
    }

    pub fn register_protocol(exe: &std::path::Path, scheme: &str) -> bool {
        register(exe, &format!("x-scheme-handler/{scheme}"))
    }

    pub fn register_file_type(exe: &std::path::Path, ext: &str) -> bool {
        mime_of(ext).is_some_and(|mime| register(exe, mime))
    }

    fn autostart() -> Option<PathBuf> {
        Some(config_dir()?.join("autostart").join(format!("{}.desktop", id())))
    }

    pub fn set_login_item(exe: &std::path::Path, enabled: bool) -> bool {
        let Some(path) = autostart() else { return false };
        if enabled {
            let text = entry_text(exe, &[]).replace(" %U", "") + "X-GNOME-Autostart-enabled=true\n";
            path.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(path, text).is_ok()
        } else {
            !path.exists() || std::fs::remove_file(path).is_ok()
        }
    }

    pub fn login_item() -> bool {
        autostart().is_some_and(|p| p.exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes_extensions_and_mime_types_are_checked() {
        assert!(valid_scheme("my-app+x.1") && !valid_scheme("1app") && !valid_scheme("") && !valid_scheme("a b"));
        assert!(valid_ext("md") && !valid_ext("") && !valid_ext("a/b") && !valid_ext("a b"));
        assert_eq!(mime_of(".MD"), Some("text/markdown"));
        assert_eq!(mime_of("xyz"), None);
        assert!(!register_protocol("not a scheme!") && !register_file_type("a/b"));
    }
}
