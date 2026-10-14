//! What the `@env` module can ask of the operating system (`envCall` in the prelude).

use std::path::PathBuf;
use std::process::Command;

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// A well-known folder: `home`, `applications`, `root`, `desktop`, `appdata`, `downloads`,
/// `documents`, `music`, `videos`, `pictures`.
fn folder(name: &str) -> Option<PathBuf> {
    let h = home();
    let sub = |s: &str| h.clone().map(|h| h.join(s));
    match name {
        "home" => h,
        "desktop" => sub("Desktop"),
        "downloads" => sub("Downloads"),
        "documents" => sub("Documents"),
        "music" => sub("Music"),
        "videos" => sub(if cfg!(target_os = "macos") { "Movies" } else { "Videos" }),
        "pictures" => sub("Pictures"),
        "root" => Some(if cfg!(windows) { PathBuf::from("C:\\") } else { PathBuf::from("/") }),
        "applications" => Some(PathBuf::from(if cfg!(target_os = "macos") {
            "/Applications"
        } else if cfg!(windows) {
            "C:\\Program Files"
        } else {
            "/usr/share/applications"
        })),
        "appdata" => {
            if cfg!(target_os = "macos") {
                sub("Library/Application Support")
            } else if cfg!(windows) {
                std::env::var_os("APPDATA").map(PathBuf::from)
            } else {
                std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| sub(".local/share"))
            }
        }
        _ => None,
    }
}

fn locale() -> (String, String) {
    let raw = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty() && v != "C" && v != "POSIX")).unwrap_or_default();
    let base = raw.split(['.', '@']).next().unwrap_or("");
    let mut parts = base.split(['_', '-']);
    let lang = parts.next().filter(|l| l.len() == 2).unwrap_or("en").to_lowercase();
    let country = parts.next().filter(|c| c.len() == 2).unwrap_or("US").to_uppercase();
    (lang, country)
}

fn command_output(program: &str) -> String {
    Command::new(program).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// Run one `@env` operation. Lists come back newline-separated; unknown names are empty.
pub fn call(op: &str, args: &[String]) -> String {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match op {
        "os" => format!("{}-{}", platform(), std::env::consts::ARCH),
        "platform" => platform().to_string(),
        "language" => locale().0,
        "country" => locale().1,
        "userName" => std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default(),
        "machineName" => std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).ok().filter(|h| !h.is_empty()).unwrap_or_else(|| command_output("hostname")),
        "domainName" => std::env::var("USERDOMAIN").unwrap_or_default(),
        "arguments" => std::env::args().skip(1).collect::<Vec<_>>().join("\n"),
        "home" => {
            let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_default();
            exe.join(arg(0)).to_string_lossy().into_owned()
        }
        "path" => folder(arg(0)).map(|p| p.join(arg(1)).to_string_lossy().trim_end_matches(['/', '\\']).to_string()).unwrap_or_default(),
        "drives" => {
            if cfg!(windows) {
                ('A'..='Z').map(|c| format!("{c}:")).filter(|d| std::path::Path::new(&format!("{d}\\")).exists()).collect::<Vec<_>>().join("\n")
            } else {
                "/".to_string()
            }
        }
        "variable" => match args.get(1).map(String::as_str) {
            // Reading: `variable(name)`. Setting or removing: a second argument, `\u{0}` meaning remove.
            None => std::env::var(arg(0)).unwrap_or_default(),
            Some("\u{0}") => {
                std::env::remove_var(arg(0));
                String::new()
            }
            Some(v) => {
                std::env::set_var(arg(0), v);
                v.to_string()
            }
        },
        "launch" => {
            let target = arg(0);
            let result = if cfg!(target_os = "macos") {
                Command::new("open").arg(target).spawn()
            } else if cfg!(windows) {
                Command::new("cmd").args(["/C", "start", "", target]).spawn()
            } else {
                Command::new("xdg-open").arg(target).spawn()
            };
            result.is_ok().to_string()
        }
        "exec" => Command::new(arg(0)).args(&args[1.min(args.len())..]).spawn().is_ok().to_string(),
        _ => String::new(),
    }
}

fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "OSX",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

/// A path or URL (`this://app/...`, `file://...`) as a path.
pub(crate) fn path_of(p: &str) -> PathBuf {
    nile_dom::vfs::url_to_path(p).unwrap_or_else(|| PathBuf::from(crate::net::path_of_url(p)))
}

fn secs(t: std::io::Result<std::time::SystemTime>) -> f64 {
    t.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_secs_f64())
}

/// One `@sys.fs` operation. `Ok` carries the result as text (JSON, or base64 for file contents),
/// `Err` an error message beginning with its code (`ENOENT: ...`).
pub fn fs(op: &str, args: &[String]) -> Result<String, String> {
    use base64::Engine as _;
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let path = path_of(arg(0));
    let failed = |e: std::io::Error| {
        let code = match e.kind() {
            std::io::ErrorKind::NotFound => "ENOENT",
            std::io::ErrorKind::PermissionDenied => "EACCES",
            std::io::ErrorKind::AlreadyExists => "EEXIST",
            _ => "EIO",
        };
        format!("{code}: {e}: {}", path.display())
    };
    let b64 = base64::engine::general_purpose::STANDARD;
    match op {
        "stat" => {
            if nile_dom::vfs::is_virtual(&path) {
                return match nile_dom::vfs::read(&path) {
                    Ok(bytes) => Ok(format!("{{\"isFile\":true,\"isDirectory\":false,\"isSymbolicLink\":false,\"st_size\":{},\"st_mode\":33060}}", bytes.len())),
                    Err(_) if nile_dom::vfs::exists(&path) => Ok("{\"isFile\":false,\"isDirectory\":true,\"isSymbolicLink\":false,\"st_size\":0,\"st_mode\":16749}".into()),
                    Err(e) => Err(failed(e)),
                };
            }
            let meta = if arg(1) == "link" { std::fs::symlink_metadata(&path) } else { std::fs::metadata(&path) }.map_err(failed)?;
            #[cfg(unix)]
            let mode = std::os::unix::fs::PermissionsExt::mode(&meta.permissions());
            #[cfg(not(unix))]
            let mode = if meta.is_dir() { 0o40755 } else { 0o100644 };
            Ok(format!(
                "{{\"isFile\":{},\"isDirectory\":{},\"isSymbolicLink\":{},\"st_size\":{},\"st_mode\":{mode},\"st_mtime\":{},\"st_atime\":{},\"st_birthtime\":{}}}",
                meta.is_file(),
                meta.is_dir(),
                meta.file_type().is_symlink(),
                meta.len(),
                secs(meta.modified()),
                secs(meta.accessed()),
                secs(meta.created())
            ))
        }
        "readdir" => {
            let entries: Vec<(String, bool)> = if nile_dom::vfs::is_virtual(&path) {
                nile_dom::vfs::list_dir(&path).ok_or_else(|| format!("ENOTDIR: not a folder: {}", path.display()))?
            } else {
                let mut v: Vec<(String, bool)> = std::fs::read_dir(&path).map_err(failed)?.filter_map(Result::ok).map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir())).collect();
                v.sort();
                v
            };
            // `fs.UV_DIRENT_FILE` is 1 and `UV_DIRENT_DIR` 2.
            Ok(format!("[{}]", entries.iter().map(|(n, d)| format!("{{\"name\":{},\"type\":{}}}", json_str(n), if *d { 2 } else { 1 })).collect::<Vec<_>>().join(",")))
        }
        "read" => nile_dom::vfs::read(&path).map(|b| b64.encode(b)).map_err(failed),
        "write" => std::fs::write(&path, b64.decode(arg(1)).map_err(|e| e.to_string())?).map(|_| String::new()).map_err(failed),
        "unlink" => std::fs::remove_file(&path).map(|_| String::new()).map_err(failed),
        "mkdir" => std::fs::create_dir_all(&path).map(|_| String::new()).map_err(failed),
        "rmdir" => std::fs::remove_dir(&path).map(|_| String::new()).map_err(failed),
        "rename" => std::fs::rename(&path, path_of(arg(1))).map(|_| String::new()).map_err(failed),
        "copyfile" => std::fs::copy(&path, path_of(arg(1))).map(|_| String::new()).map_err(failed),
        "realpath" => nile_dom::vfs::canonicalize(&path).map(|p| p.to_string_lossy().into_owned()).map_err(failed),
        _ => Err(format!("ENOSYS: unknown operation {op}")),
    }
}

pub(crate) fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_basic_questions() {
        assert!(["OSX", "Windows", "Linux"].contains(&call("platform", &[]).as_str()) || cfg!(not(any(target_os = "macos", windows, target_os = "linux"))));
        assert_eq!(call("language", &[]).len(), 2);
        assert!(call("path", &["home".into(), "x".into()]).ends_with('x'));
        assert_eq!(call("path", &["nonsense".into()]), "");
        call("variable", &["NILE_ENVMOD_TEST".into(), "1".into()]);
        assert_eq!(call("variable", &["NILE_ENVMOD_TEST".into()]), "1");
        call("variable", &["NILE_ENVMOD_TEST".into(), "\u{0}".into()]);
        assert_eq!(call("variable", &["NILE_ENVMOD_TEST".into()]), "");
    }
}
