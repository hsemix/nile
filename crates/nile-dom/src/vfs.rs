//! A read-only file system for packaged apps. `nile --pack folder -o app` appends the folder's files
//! (deflated) to a copy of the `nile` binary; at startup the binary finds them behind itself and
//! mounts them at a path no real file system has ([`MOUNT`]). Everything that reads a page's files
//! (the page, `<include>`, images, scripts and modules, `fetch`, frames) goes through [`read`] and
//! friends, which serve that path from the archive and everything else from disk. Scripts see the
//! mount as `this://app/`.
//!
//! Layout of a packed binary: `runtime | file data | index | index_len u64 | data_start u64 | "NILEPAK1"`.
//! The index is text, one `path<TAB>deflated(0|1)<TAB>offset<TAB>stored_len<TAB>len` line per file.

use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const MAGIC: &[u8; 8] = b"NILEPAK1";

/// Where the archive is mounted (no real file system has this folder).
pub const MOUNT: &str = if cfg!(windows) { r"C:\__nile_app__" } else { "/__nile_app__" };

struct Entry {
    deflated: bool,
    offset: u64,
    stored: u64,
    len: u64,
}

struct Pack {
    file: Mutex<File>,
    base: u64,
    entries: HashMap<String, Entry>,
}

static PACK: OnceLock<Pack> = OnceLock::new();

/// The mount point as a path.
pub fn mount_path() -> PathBuf {
    PathBuf::from(MOUNT)
}

/// Whether an archive is mounted.
pub fn mounted() -> bool {
    PACK.get().is_some()
}

/// Whether `path` is inside the mount.
pub fn is_virtual(path: &Path) -> bool {
    path.starts_with(MOUNT)
}

/// Mount the archive appended to the running executable, if there is one. Returns whether there is.
pub fn mount_self() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    // Behind the executable, or (a macOS `.app` bundle, whose signed executable can't be appended to)
    // as `Contents/Resources/app.nilepack`.
    let beside = exe.parent().and_then(Path::parent).map(|contents| contents.join("Resources/app.nilepack"));
    for file in [Some(exe.clone()), beside].into_iter().flatten() {
        if let Ok(Some(pack)) = open(&file) {
            return PACK.set(pack).is_ok();
        }
    }
    mounted()
}

fn open(exe: &Path) -> io::Result<Option<Pack>> {
    let mut file = File::open(exe)?;
    let size = file.metadata()?.len();
    if size < 24 {
        return Ok(None);
    }
    let mut tail = [0u8; 24];
    file.seek(SeekFrom::Start(size - 24))?;
    file.read_exact(&mut tail)?;
    if &tail[16..] != MAGIC {
        return Ok(None);
    }
    let index_len = u64::from_le_bytes(tail[..8].try_into().unwrap_or_default());
    let base = u64::from_le_bytes(tail[8..16].try_into().unwrap_or_default());
    if index_len > size || base > size {
        return Ok(None);
    }
    let mut index = vec![0u8; index_len as usize];
    file.seek(SeekFrom::Start(size - 24 - index_len))?;
    file.read_exact(&mut index)?;
    let mut entries = HashMap::new();
    for line in String::from_utf8_lossy(&index).lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if let [path, deflated, offset, stored, len] = f[..] {
            if let (Ok(offset), Ok(stored), Ok(len)) = (offset.parse(), stored.parse(), len.parse()) {
                entries.insert(path.to_string(), Entry { deflated: deflated == "1", offset, stored, len });
            }
        }
    }
    Ok(Some(Pack { file: Mutex::new(file), base, entries }))
}

/// The archive key of a path inside the mount: `a/b.html`, with `.` and `..` resolved.
fn key_of(path: &Path) -> Option<String> {
    let rest = path.strip_prefix(MOUNT).ok()?;
    let mut parts: Vec<String> = Vec::new();
    for c in rest.components() {
        match c {
            Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            Component::ParentDir => {
                parts.pop();
            }
            _ => {}
        }
    }
    Some(parts.join("/"))
}

/// A file's bytes: from the archive for paths in the mount, else from disk.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    if !is_virtual(path) {
        return std::fs::read(path);
    }
    let not_found = || io::Error::new(io::ErrorKind::NotFound, format!("{} is not in the packaged app", path.display()));
    let pack = PACK.get().ok_or_else(not_found)?;
    let entry = pack.entries.get(&key_of(path).ok_or_else(not_found)?).ok_or_else(not_found)?;
    let mut stored = vec![0u8; entry.stored as usize];
    {
        let mut file = pack.file.lock().map_err(|_| io::Error::other("archive lock"))?;
        file.seek(SeekFrom::Start(pack.base + entry.offset))?;
        file.read_exact(&mut stored)?;
    }
    if !entry.deflated {
        return Ok(stored);
    }
    let bytes = miniz_oxide::inflate::decompress_to_vec(&stored).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}")))?;
    if bytes.len() as u64 != entry.len {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "packaged file is damaged"));
    }
    Ok(bytes)
}

pub fn read_to_string(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Whether a file (or, in the archive, a folder) exists.
pub fn exists(path: &Path) -> bool {
    if !is_virtual(path) {
        return path.exists();
    }
    let (Some(pack), Some(key)) = (PACK.get(), key_of(path)) else { return false };
    key.is_empty() || pack.entries.contains_key(&key) || pack.entries.keys().any(|k| k.starts_with(&format!("{key}/")))
}

/// The entries of a folder inside the mount: `(name, is_dir)`, sorted. `None` if it isn't one.
pub fn list_dir(path: &Path) -> Option<Vec<(String, bool)>> {
    let (pack, key) = (PACK.get()?, key_of(path)?);
    let prefix = if key.is_empty() { String::new() } else { format!("{key}/") };
    let mut out: Vec<(String, bool)> = Vec::new();
    for k in pack.entries.keys().filter(|k| k.starts_with(&prefix)) {
        let rest = &k[prefix.len()..];
        let (name, is_dir) = match rest.split_once('/') {
            Some((dir, _)) => (dir.to_string(), true),
            None => (rest.to_string(), false),
        };
        if !out.iter().any(|(n, _)| *n == name) {
            out.push((name, is_dir));
        }
    }
    if out.is_empty() && !key.is_empty() {
        return None;
    }
    out.sort();
    Some(out)
}

/// A path as one stable name: the real canonical path, or for the mount the path itself.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    if is_virtual(path) {
        return key_of(path).map(|k| mount_path().join(k)).ok_or_else(|| io::Error::other("bad path"));
    }
    std::fs::canonicalize(path)
}

/// The path a `this://app/...` URL names.
pub fn url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("this://app")?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest).trim_start_matches('/');
    Some(mount_path().join(rest))
}

/// The `this://app/...` URL of a path inside the mount.
pub fn app_url(path: &Path) -> Option<String> {
    key_of(path).map(|k| format!("this://app/{k}"))
}

/// Which file a packaged or unpacked app starts with, looking in `dir`: Sciter's order.
pub fn start_file(dir: &Path) -> Option<PathBuf> {
    ["run.js", "scapp.htm", "scapp.html", "main.htm", "main.html", "index.htm", "index.html"].iter().map(|n| dir.join(n)).find(|p| exists(p))
}

/// Append the files of `folder` to `runtime` (the bytes of a `nile` binary without an archive) and
/// write the result to `out`. Returns the number of files and the stored size of the archive.
pub fn pack(folder: &Path, runtime: &[u8], out: &Path) -> io::Result<(usize, u64)> {
    fn walk(dir: &Path, root: &Path, files: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            // Skip hidden files, VCS folders and build output.
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, files)?;
            } else if let Ok(rel) = path.strip_prefix(root) {
                files.push((rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"), path));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(folder, folder, &mut files)?;
    let mut data: Vec<u8> = Vec::new();
    let mut index = String::new();
    for (rel, path) in &files {
        let bytes = std::fs::read(path)?;
        let packed = miniz_oxide::deflate::compress_to_vec(&bytes, 9);
        let (stored, deflated) = if packed.len() < bytes.len() { (&packed, true) } else { (&bytes, false) };
        index.push_str(&format!("{rel}\t{}\t{}\t{}\t{}\n", u8::from(deflated), data.len(), stored.len(), bytes.len()));
        data.extend_from_slice(stored);
    }
    let mut blob = Vec::with_capacity(runtime.len() + data.len() + index.len() + 24);
    blob.extend_from_slice(runtime);
    blob.extend_from_slice(&data);
    blob.extend_from_slice(index.as_bytes());
    blob.extend_from_slice(&(index.len() as u64).to_le_bytes());
    blob.extend_from_slice(&(runtime.len() as u64).to_le_bytes());
    blob.extend_from_slice(MAGIC);
    std::fs::write(out, &blob)?;
    Ok((files.len(), (data.len() + index.len()) as u64))
}

/// The running executable's bytes without any archive behind it (what `pack` appends to).
pub fn runtime_bytes() -> io::Result<Vec<u8>> {
    let exe = std::env::current_exe()?;
    let mut bytes = std::fs::read(&exe)?;
    // Only an archive appended to this very file is behind it (a bundle's lives in its own file).
    if let Some(pack) = PACK.get().filter(|p| p.base as usize <= bytes.len() && bytes.ends_with(MAGIC)) {
        bytes.truncate(pack.base as usize);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_and_reads_back_with_virtual_paths() {
        let dir = std::env::temp_dir().join(format!("nile-vfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("img")).unwrap();
        std::fs::write(dir.join("index.html"), "<p>hello hello hello hello hello hello</p>".repeat(20)).unwrap();
        std::fs::write(dir.join("img/a.bin"), [1u8, 2, 3]).unwrap();
        let out = dir.join("app.bin");
        let (count, _) = pack(&dir, b"RUNTIME", &out).unwrap();
        assert_eq!(count, 2);
        let pack = open(&out).unwrap().expect("an archive");
        assert_eq!(pack.entries.len(), 2);
        assert!(pack.entries["index.html"].deflated && !pack.entries["img/a.bin"].deflated);
        assert!(std::fs::read(&out).unwrap().starts_with(b"RUNTIME"));
        assert_eq!(key_of(&mount_path().join("img/../index.html")).as_deref(), Some("index.html"));
        assert_eq!(url_to_path("this://app/img/a.bin?x=1"), Some(mount_path().join("img/a.bin")));
        assert_eq!(app_url(&mount_path().join("img/a.bin")).as_deref(), Some("this://app/img/a.bin"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
