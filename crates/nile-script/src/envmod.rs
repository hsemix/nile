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
