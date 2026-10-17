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
    io::{self, Read, Seek, SeekFrom, Write},
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

pub struct Archive {
    file: Mutex<File>,
    base: u64,
    entries: HashMap<String, Entry>,
}

static PACK: OnceLock<Archive> = OnceLock::new();

/// The program's window icon inside a packed archive (`nile --pack ... --icon x.png` adds it).
pub const ICON_FILE: &str = "__nile_icon.png";

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

fn open(exe: &Path) -> io::Result<Option<Archive>> {
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
    let index_start = size.checked_sub(24).and_then(|n| n.checked_sub(index_len))
        .filter(|&n| n >= base && index_len <= 16 * 1024 * 1024)
        .ok_or_else(|| io::Error::other("invalid archive index bounds"))?;
    let mut index = vec![0u8; index_len as usize];
    file.seek(SeekFrom::Start(index_start))?;
    file.read_exact(&mut index)?;
    let index = std::str::from_utf8(&index).map_err(io::Error::other)?;
    let mut entries = HashMap::new();
    let mut names = std::collections::BTreeSet::new();
    for line in index.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let [path, deflated, offset, stored, len] = f.as_slice() else { return Err(io::Error::other("invalid archive entry")); };
        validate_path(path)?;
        let folded = path.to_lowercase();
        if !names.insert(folded) { return Err(io::Error::other("duplicate archive path")); }
        let parse = |s: &str| s.parse::<u64>().map_err(io::Error::other);
        let (offset, stored, len) = (parse(offset)?, parse(stored)?, parse(len)?);
        if !matches!(*deflated, "0" | "1") || base.checked_add(offset).and_then(|n| n.checked_add(stored)).is_none_or(|end| end > index_start)
            || (*deflated == "0" && stored != len) { return Err(io::Error::other("invalid archive file bounds")); }
        entries.insert(path.to_string(), Entry { deflated: *deflated == "1", offset, stored, len });
    }
    for name in &names {
        for (at, _) in name.match_indices('/') {
            if names.contains(&name[..at]) { return Err(io::Error::other("conflicting archive paths")); }
        }
    }
    Ok(Some(Archive { file: Mutex::new(file), base, entries }))
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
    read_limited(path, 512 * 1024 * 1024)
}

/// Strict portable archive destinations: no traversal, Windows prefixes or aliases.
pub fn validate_path(path: &str) -> io::Result<()> {
    if path.is_empty() || path.len() > 1024 || path.contains(['\\', ':', '\0', '\t', '\r', '\n', '<', '>', '"', '|', '?', '*'])
        || path.split('/').any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with(['.', ' '])
            || matches!(p.split('.').next().unwrap_or("").to_ascii_uppercase().as_str(), "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9")) {
        return Err(io::Error::other(format!("invalid archive path: {path:?}")));
    }
    Ok(())
}
impl Archive {
    pub fn open(path: &Path) -> io::Result<Self> { open(path)?.ok_or_else(|| io::Error::other("no Nile archive")) }
    /// Stream with an explicit decoded-size bound; no native file is inflated into a Vec.
    pub fn copy_to(&self, key: &str, output: &mut dyn Write, max: u64) -> io::Result<u64> {
        validate_path(key)?;
        let entry = self.entries.get(key).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, key))?;
        if entry.len > max { return Err(io::Error::other("archive extraction limit exceeded")); }
        let mut file = self.file.lock().map_err(|_| io::Error::other("archive lock"))?;
        file.seek(SeekFrom::Start(self.base + entry.offset))?;
        let stored = (&mut *file).take(entry.stored);
        let mut decoded: Box<dyn Read + '_> = if entry.deflated { Box::new(flate2::read::DeflateDecoder::new(stored)) } else { Box::new(stored) };
        let copied = io::copy(&mut (&mut decoded).take(max.saturating_add(1)), output)?;
        if copied != entry.len || copied > max { return Err(io::Error::other("archive length mismatch/extraction limit")); }
        Ok(copied)
    }
}
pub fn copy_to(path: &Path, output: &mut dyn Write, max: u64) -> io::Result<u64> {
    if is_virtual(path) {
        let pack = PACK.get().ok_or_else(|| io::Error::other("archive not mounted"))?;
        pack.copy_to(&key_of(path).ok_or_else(|| io::Error::other("invalid virtual path"))?, output, max)
    } else {
        let file = File::open(path)?;
        if file.metadata()?.len() > max { return Err(io::Error::other("file size limit exceeded")); }
        let copied = io::copy(&mut file.take(max.saturating_add(1)), output)?;
        if copied > max { return Err(io::Error::other("file size limit exceeded")); }
        Ok(copied)
    }
}
pub fn read_limited(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    copy_to(path, &mut bytes, max)?;
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
    pack_with(folder, &[], runtime, out)
}

/// [`pack`] with extra files that aren't in the folder: `(name in the archive, bytes)`.
pub fn pack_with(folder: &Path, extras: &[(&str, Vec<u8>)], runtime: &[u8], out: &Path) -> io::Result<(usize, u64)> {
    pack_selected(folder, extras, runtime, out, |_| true)
}

/// Pack a validated selection of files, keeping the public ordinary pack API unchanged.
pub fn pack_selected(folder: &Path, extras: &[(&str, Vec<u8>)], runtime: &[u8], out: &Path, include: impl Fn(&str) -> bool) -> io::Result<(usize, u64)> {
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
            if e.file_type()?.is_symlink() { return Err(io::Error::other(format!("symlink in packed source: {}", path.display()))); }
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
    let mut entries: Vec<(String, Vec<u8>)> = Vec::with_capacity(files.len() + extras.len());
    for (rel, path) in &files {
        if !include(rel) { continue; }
        entries.push((rel.clone(), std::fs::read(path)?));
    }
    entries.extend(extras.iter().map(|(n, b)| (n.to_string(), b.clone())));
    let mut names = std::collections::BTreeSet::new();
    for (rel, _) in &entries {
        validate_path(rel)?;
        if !names.insert(rel.to_lowercase()) { return Err(io::Error::other("duplicate packed destination")); }
    }
    for name in &names {
        for (at, _) in name.match_indices('/') { if names.contains(&name[..at]) { return Err(io::Error::other("conflicting packed destinations")); } }
    }
    for (rel, bytes) in entries.iter().map(|(r, b)| (r, b)) {
        let packed = miniz_oxide::deflate::compress_to_vec(bytes, 9);
        let (stored, deflated) = if packed.len() < bytes.len() { (&packed, true) } else { (bytes, false) };
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
    Ok((entries.len(), (data.len() + index.len()) as u64))
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

    fn test_archive(path: &Path, data: &[u8], index: &str) {
        let mut bytes = b"RUNTIME".to_vec(); bytes.extend_from_slice(data);
        bytes.extend_from_slice(index.as_bytes()); bytes.extend_from_slice(&(index.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&7u64.to_le_bytes()); bytes.extend_from_slice(MAGIC);
        std::fs::write(path, bytes).unwrap();
    }
    #[test]
    fn archive_rejects_traversal_aliases_conflicts_and_invalid_bounds() {
        let dir = std::env::temp_dir().join(format!("nile-vfs-invalid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap(); let file = dir.join("invalid.pack");
        for index in [
            "../escape\t0\t0\t1\t1\n", "/absolute\t0\t0\t1\t1\n", "C:/absolute\t0\t0\t1\t1\n",
            "native/A.dll\t0\t0\t1\t1\nnative/a.dll\t0\t0\t1\t1\n",
            "native/a\t0\t0\t1\t1\nnative/a/file\t0\t0\t1\t1\n",
            "native/a\t0\t18446744073709551615\t1\t1\n", "native/a\t0\t0\t1\t2\n",
        ] {
            test_archive(&file, b"x", index); assert!(Archive::open(&file).is_err(), "accepted {index:?}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn archive_stream_limits_hold_for_incorrect_decoded_length() {
        let dir = std::env::temp_dir().join(format!("nile-vfs-stream-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap(); let file = dir.join("stream.pack");
        let data = miniz_oxide::deflate::compress_to_vec(&vec![42; 10000], 9);
        test_archive(&file, &data, &format!("native/a\t1\t0\t{}\t10000\n", data.len()));
        let archive = Archive::open(&file).unwrap(); let mut output = Vec::new();
        assert!(archive.copy_to("native/a", &mut output, 1000).is_err()); assert!(output.is_empty());
        test_archive(&file, &data, &format!("native/a\t1\t0\t{}\t100\n", data.len()));
        let archive = Archive::open(&file).unwrap();
        assert!(archive.copy_to("native/a", &mut output, 1000).is_err()); assert!(output.len() <= 1001);
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
