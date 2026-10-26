//! Small desktop helpers behind `@env`: show a file in the system file manager, move it to the trash, open it with a
//! chosen program. Each returns `Ok(())` or a short message.

use std::path::{Path, PathBuf};
use std::process::Command;

fn run(mut cmd: Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{}: {e}", cmd.get_program().to_string_lossy()))?;
    if out.status.success() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr }).trim().to_string();
        Err(if msg.is_empty() { format!("{} failed", cmd.get_program().to_string_lossy()) } else { msg })
    }
}

/// Show `path` selected in Finder / Explorer / the file manager (Linux opens the containing folder).
pub fn reveal(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("ENOENT: no such file or directory: {}", path.display()));
    }
    if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg("-R").arg(path);
        run(c)
    } else if cfg!(windows) {
        // `explorer` returns 1 even when it works, so only a failure to start counts.
        Command::new("explorer").arg(format!("/select,{}", path.display())).spawn().map(|_| ()).map_err(|e| format!("explorer: {e}"))
    } else {
        let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
        Command::new("xdg-open").arg(dir).spawn().map(|_| ()).map_err(|e| format!("xdg-open: {e}"))
    }
}

/// Open `path` with `app` (a program name or path; on macOS an application name or `.app` path).
pub fn open_with(path: &Path, app: &str) -> Result<(), String> {
    if app.is_empty() {
        return Err("no program given".into());
    }
    if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg("-a").arg(app).arg(path);
        run(c)
    } else {
        Command::new(app).arg(path).spawn().map(|_| ()).map_err(|e| format!("{app}: {e}"))
    }
}

/// A name in `dir` that doesn't exist yet: `name`, then `name 2`, `name 3`...
pub(crate) fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() && first.symlink_metadata().is_err() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} {n}{ext}")))
        .find(|p| !p.exists() && p.symlink_metadata().is_err())
        .expect("an unused name")
}

/// Move `path` into a trash folder laid out as the freedesktop spec says (`files/` + `info/<name>.trashinfo`).
pub(crate) fn trash_into_xdg(path: &Path, trash: &Path, when: &str) -> Result<PathBuf, String> {
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&info).map_err(|e| e.to_string())?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or("invalid file name")?;
    let target = free_name(&files, name);
    let stored = target.file_name().and_then(|n| n.to_str()).unwrap_or(name).to_string();
    // The original place, percent-encoded, so the file manager can put it back.
    let encoded: String = path.to_string_lossy().bytes().map(|b| if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    std::fs::write(info.join(format!("{stored}.trashinfo")), format!("[Trash Info]\nPath={encoded}\nDeletionDate={when}\n")).map_err(|e| e.to_string())?;
    std::fs::rename(path, &target).map_err(|e| {
        let _ = std::fs::remove_file(info.join(format!("{stored}.trashinfo")));
        format!("{}: {e}", path.display())
    })?;
    Ok(target)
}

/// Move a file or folder to the trash / recycle bin.
pub fn trash(path: &Path) -> Result<(), String> {
    if path.symlink_metadata().is_err() {
        return Err(format!("ENOENT: no such file or directory: {}", path.display()));
    }
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if cfg!(target_os = "macos") {
        // The user's Trash folder (what Finder shows). It doesn't offer "Put Back", but needs no permission prompt.
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("no home folder")?;
        let dir = home.join(".Trash");
        let name = path.file_name().and_then(|n| n.to_str()).ok_or("invalid file name")?;
        std::fs::rename(&path, free_name(&dir, name)).map_err(|e| format!("{}: {e}", path.display()))
    } else if cfg!(windows) {
        let dir = path.is_dir();
        let escaped = path.to_string_lossy().replace('\'', "''");
        let call = if dir { "DeleteDirectory" } else { "DeleteFile" };
        let script = format!("Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::{call}('{escaped}', 'OnlyErrorDialogs', 'SendToRecycleBin')");
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-Command", &script]);
        run(c)
    } else {
        for tool in [["gio", "trash"], ["trash-put", ""]] {
            let mut c = Command::new(tool[0]);
            if !tool[1].is_empty() {
                c.arg(tool[1]);
            }
            c.arg(&path);
            if c.output().map(|o| o.status.success()).unwrap_or(false) {
                return Ok(());
            }
        }
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))).ok_or("no home folder")?;
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        trash_into_xdg(&path, &data.join("Trash"), &format!("@{secs}")).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_unique_and_the_xdg_trash_keeps_the_origin() {
        let root = std::env::temp_dir().join(format!("nile-desktop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bin = root.join("Trash");
        for n in 0..2 {
            let f = root.join("my report.txt");
            std::fs::write(&f, format!("v{n}")).unwrap();
            trash_into_xdg(&f, &bin, "2026-10-25T10:00:00").unwrap();
            assert!(!f.exists(), "moved out of its place");
        }
        let mut names: Vec<_> = std::fs::read_dir(bin.join("files")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["my report 2.txt", "my report.txt"]);
        let info = std::fs::read_to_string(bin.join("info/my report 2.txt.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath=") && info.contains("my%20report.txt") && info.contains("DeletionDate=2026-10-25T10:00:00"), "{info}");
        // folders move whole
        let d = root.join("folder");
        std::fs::create_dir_all(d.join("inner")).unwrap();
        trash_into_xdg(&d, &bin, "x").unwrap();
        assert!(bin.join("files/folder/inner").is_dir());
        assert!(trash(Path::new("/definitely/not/here")).unwrap_err().starts_with("ENOENT"));
        assert!(reveal(Path::new("/definitely/not/here")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
