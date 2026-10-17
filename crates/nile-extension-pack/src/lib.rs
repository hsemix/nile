//! Packed package planning, bounded native materialization, and stable writable paths.
//! The SDK/extension host remain independent of archive and cache implementation.
use nile_dom::vfs;
use nile_extensions::{Manifest, PreparedPackage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

mod json;
#[cfg(windows)]
mod windows;

pub const INVENTORY: &str = "__nile_extensions.json";
const FILE_LIMIT: u64 = 256 * 1024 * 1024;
const PACKAGE_LIMIT: u64 = 512 * 1024 * 1024;
const META_LIMIT: u64 = 2 * 1024 * 1024;
const TARGETS: &[&str] = &[
    "macos-aarch64",
    "macos-x86_64",
    "windows-aarch64-msvc",
    "windows-x86_64-msvc",
    "linux-aarch64-gnu",
    "linux-x86_64-gnu",
    "linux-aarch64-musl",
    "linux-x86_64-musl",
];
#[derive(Debug, Serialize)]
pub struct StartupError {
    pub package: String,
    pub target: String,
    pub operation: String,
    pub message: String,
}
impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string(self).map_err(|_| std::fmt::Error)?
        )
    }
}
impl std::error::Error for StartupError {}
type Result<T> = std::result::Result<T, StartupError>;
fn failure(package: &str, target: &str, op: &str, error: impl ToString) -> StartupError {
    StartupError {
        package: package.into(),
        target: target.into(),
        operation: op.into(),
        message: error.to_string(),
    }
}
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Application {
    pub id: String,
    pub version: String,
}
#[derive(Deserialize)]
struct Config {
    #[serde(default)]
    extensions: BTreeMap<String, Location>,
    application: Option<Application>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Location {
    Embedded(String),
    External {
        package: String,
        native_root: ExternalRoot,
    },
}
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalRoot {
    pub base: String,
    pub path: String,
}
impl Location {
    fn package(&self) -> &str {
        match self {
            Self::Embedded(p) => p,
            Self::External { package, .. } => package,
        }
    }
    fn external(&self) -> Option<ExternalRoot> {
        match self {
            Self::Embedded(_) => None,
            Self::External { native_root, .. } => Some(native_root.clone()),
        }
    }
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NativeFile {
    pub path: String,
    pub len: u64,
    pub sha256: String,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NativePackage {
    pub import: String,
    pub id: String,
    pub version: String,
    pub root: String,
    pub entry: String,
    pub library: String,
    pub files: Vec<NativeFile>,
    pub hash: String,
    pub external: Option<ExternalRoot>,
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub format: u32,
    pub application: Application,
    pub target: String,
    pub packages: Vec<NativePackage>,
}
pub struct PackPlan {
    pub inventory: Option<Inventory>,
    native_paths: BTreeSet<String>,
    package_roots: Vec<String>,
    all_native_paths: BTreeSet<String>,
}
impl PackPlan {
    pub fn include(&self, path: &str) -> bool {
        if self.native_paths.contains(path) {
            return true;
        }
        if self.all_native_paths.contains(path) {
            return false;
        }
        !self
            .package_roots
            .iter()
            .any(|p| path.starts_with(&format!("{p}/native/")))
    }
    pub fn metadata(&self) -> std::result::Result<Option<Vec<u8>>, serde_json::Error> {
        self.inventory.as_ref().map(serde_json::to_vec).transpose()
    }
}
fn json<T: serde::de::DeserializeOwned>(path: &Path) -> std::result::Result<T, String> {
    json::decode(&vfs::read_limited(path, META_LIMIT).map_err(|e| e.to_string())?)
}
fn safe(path: &str) -> std::result::Result<(), String> {
    vfs::validate_path(path).map_err(|e| e.to_string())?;
    if path
        .split('/')
        .any(|p| p.starts_with('.') || p == "target" || p == "node_modules")
    {
        return Err(format!(
            "native package path excluded by pack policy: {path}"
        ));
    }
    Ok(())
}
fn safe_set<'a>(paths: impl IntoIterator<Item = &'a str>) -> std::result::Result<(), String> {
    let mut names = BTreeSet::new();
    for path in paths {
        safe(path)?;
        if !names.insert(path.to_lowercase()) {
            return Err(format!("duplicate native destination: {path}"));
        }
    }
    for path in &names {
        for (at, _) in path.match_indices('/') {
            if names.contains(&path[..at]) {
                return Err(format!("conflicting native destination: {path}"));
            }
        }
    }
    Ok(())
}
fn identity(app: &Application) -> std::result::Result<(), String> {
    if app.id.is_empty()
        || app.id.len() > 128
        || !app
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        || app.id.starts_with('.')
        || app.version.is_empty()
    {
        return Err(
            "packed native extensions require application.id (portable lowercase identifier) and version"
                .into(),
        );
    }
    safe(&app.id)
}
fn validate_manifest(import: &str, m: &Manifest, target: &str) -> std::result::Result<(), String> {
    if !import.starts_with('@')
        || import.contains('/')
        || matches!(
            import,
            "@nile" | "@sciter" | "@native" | "@sys" | "@env" | "@debug" | "@storage"
        )
    {
        return Err(format!("invalid/reserved import: {import}"));
    }
    if m.abi != 1
        || m.id.is_empty()
        || !m.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || m.version.is_empty()
    {
        return Err("invalid extension identity/version/ABI".into());
    }
    safe(&m.entry)?;
    let selected = m
        .native
        .get(target)
        .ok_or_else(|| format!("missing platform binary for {target}"))?;
    let files = selected.files();
    if files.len() > 128 {
        return Err("native inventory exceeds 128 files".into());
    }
    safe_set(files.iter().copied())?;
    if files
        .iter()
        .any(|p| *p == m.entry || *p == "extension.json")
    {
        return Err("native files conflict with wrapper/manifest".into());
    }
    for native in m.native.values() {
        safe_set(native.files())?;
    }
    Ok(())
}
fn regular(root: &Path, rel: &str) -> std::result::Result<PathBuf, String> {
    safe(rel)?;
    for ancestor in root.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|e| e.to_string())?;
        if is_link(&metadata) || !metadata.is_dir() {
            return Err(format!("unsafe native root: {}", ancestor.display()));
        }
    }
    let mut current = root.to_path_buf();
    for part in rel.split('/') {
        current.push(part);
        let meta =
            fs::symlink_metadata(&current).map_err(|e| format!("{}: {e}", current.display()))?;
        if is_link(&meta) {
            return Err(format!(
                "symlink/reparse point is not allowed: {}",
                current.display()
            ));
        }
    }
    if !current.is_file() {
        return Err(format!("not a regular file: {}", current.display()));
    }
    Ok(current)
}
fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}
struct HashWriter {
    hash: Sha256,
    len: u64,
}
impl Default for HashWriter {
    fn default() -> Self {
        Self {
            hash: Sha256::new(),
            len: 0,
        }
    }
}
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.len = self
            .len
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| std::io::Error::other("length overflow"))?;
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn hash_file(path: &Path) -> std::result::Result<(u64, String), String> {
    let mut digest = HashWriter::default();
    vfs::copy_to(path, &mut digest, FILE_LIMIT).map_err(|e| e.to_string())?;
    Ok((digest.len, format!("{:x}", digest.hash.finalize())))
}
fn package_hash(package: &NativePackage, target: &str, app: &Application) -> String {
    let mut copy = package.clone();
    copy.hash.clear();
    let bytes = serde_json::to_vec(&(app, target, copy)).expect("serializable native inventory");
    format!("{:x}", Sha256::digest(bytes))
}
/// Validate all declarations before the shell writes an executable. Native binaries
/// for other targets are omitted; JS/manifests/ordinary assets remain archived.
pub fn plan(folder: &Path, target: &str) -> Result<PackPlan> {
    let fail = |op: &str, e: String| failure("application", target, op, e);
    if !TARGETS.contains(&target) {
        return Err(fail("target", "unsupported target".into()));
    }
    let root = folder
        .canonicalize()
        .map_err(|e| fail("root", e.to_string()))?;
    let mut plan = PackPlan {
        inventory: None,
        native_paths: BTreeSet::new(),
        package_roots: Vec::new(),
        all_native_paths: BTreeSet::new(),
    };
    fn nested_configs(dir: &Path, root: &Path) -> std::result::Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
                return Err("symlink in packed source".into());
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if entry.path().is_dir() {
                nested_configs(&entry.path(), root)?;
            } else if dir != root && name == "nile.json" {
                let config: Config = json(&entry.path())?;
                if !config.extensions.is_empty() {
                    return Err("packed native declarations must use root nile.json; nested declarations are unsupported".into());
                }
            }
        }
        Ok(())
    }
    nested_configs(&root, &root).map_err(|e| fail("config-discovery", e))?;
    if !root.join("nile.json").exists() {
        return Ok(plan);
    }
    let config: Config = json(&root.join("nile.json")).map_err(|e| fail("config", e))?;
    if config.extensions.is_empty() {
        return Ok(plan);
    }
    if root.join(INVENTORY).exists() {
        return Err(fail(
            "inventory",
            "reserved native inventory already exists in source".into(),
        ));
    }
    let app = config
        .application
        .ok_or_else(|| fail("config", "missing application identity".into()))?;
    identity(&app).map_err(|e| fail("identity", e))?;
    let mut packages = Vec::new();
    let mut ids = BTreeSet::new();
    for (import, location) in config.extensions {
        let fail = |op: &str, e: String| failure(&import, target, op, e);
        // Permit ./ for existing application-relative configuration strings only.
        let rel = location
            .package()
            .strip_prefix("./")
            .unwrap_or(location.package());
        safe(rel).map_err(|e| fail("package-path", e))?;
        let package_root = root.join(rel);
        regular(&root, &format!("{rel}/extension.json")).map_err(|e| fail("manifest", e))?;
        let m: Manifest =
            json(&package_root.join("extension.json")).map_err(|e| fail("manifest", e))?;
        validate_manifest(&import, &m, target).map_err(|e| fail("manifest", e))?;
        if !ids.insert(m.id.clone()) {
            return Err(fail("manifest", "duplicate package identity".into()));
        }
        let entry = regular(&package_root, &m.entry).map_err(|e| fail("entry", e))?;
        if fs::metadata(entry)
            .map_err(|e| fail("entry", e.to_string()))?
            .len()
            > META_LIMIT
        {
            return Err(fail("entry", "wrapper entry exceeds 2 MiB".into()));
        }
        if let Some(external) = location.external() {
            validate_external(&external).map_err(|e| fail("external-mode", e))?;
        }
        let native = &m.native[target];
        let mut files = Vec::new();
        let mut total = 0u64;
        for path in native.files() {
            let real = regular(&package_root, path).map_err(|e| fail("native-file", e))?;
            let (len, sha256) = hash_file(&real).map_err(|e| fail("hash", e))?;
            total = total
                .checked_add(len)
                .filter(|n| *n <= PACKAGE_LIMIT)
                .ok_or_else(|| fail("size", "native package exceeds 512 MiB".into()))?;
            files.push(NativeFile {
                path: path.into(),
                len,
                sha256,
            });
            if location.external().is_none() {
                plan.native_paths.insert(format!("{rel}/{path}"));
            }
        }
        for n in m.native.values() {
            for path in n.files() {
                plan.all_native_paths.insert(format!("{rel}/{path}"));
            }
        }
        let library = native.library().to_string();
        let mut package = NativePackage {
            import,
            id: m.id,
            version: m.version,
            root: rel.into(),
            entry: m.entry,
            library,
            files,
            hash: String::new(),
            external: location.external(),
        };
        package.hash = package_hash(&package, target, &app);
        plan.package_roots.push(rel.into());
        packages.push(package);
    }
    safe_set(plan.package_roots.iter().map(String::as_str))
        .map_err(|e| fail("package-paths", e))?;
    plan.inventory = Some(Inventory {
        format: 1,
        application: app,
        target: target.into(),
        packages,
    });
    if serde_json::to_vec(plan.inventory.as_ref().unwrap())
        .map_err(|e| fail("inventory", e.to_string()))?
        .len() as u64
        > META_LIMIT
    {
        return Err(fail("inventory", "native inventory exceeds 2 MiB".into()));
    }
    Ok(plan)
}
fn validate_external(root: &ExternalRoot) -> std::result::Result<(), String> {
    if !matches!(root.base.as_str(), "executable" | "bundle-resources") {
        return Err("native_root.base must be executable or bundle-resources".into());
    }
    safe(&root.path)
}
fn external_path(root: &ExternalRoot, exe: &Path) -> std::result::Result<PathBuf, String> {
    validate_external(root)?;
    let mut base = exe.parent().ok_or("executable has no directory")?;
    if root.base == "bundle-resources" {
        if base.file_name().is_none_or(|n| n != "MacOS")
            || base
                .parent()
                .and_then(Path::file_name)
                .is_none_or(|n| n != "Contents")
        {
            return Err("bundle-resources requires Contents/MacOS executable layout".into());
        }
        base = base.parent().unwrap();
        return Ok(base.join("Resources").join(&root.path));
    }
    Ok(base.join(&root.path))
}
/// OS paths are explicit and absolute; overrides are useful to custom hosts/CI.
fn os_root(cache: bool) -> std::result::Result<PathBuf, String> {
    let override_name = if cache {
        "NILE_NATIVE_CACHE_HOME"
    } else {
        "NILE_APP_DATA_HOME"
    };
    let value = std::env::var_os(override_name)
        .map(PathBuf::from)
        .or_else(|| {
            if cfg!(windows) {
                std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
            } else if cfg!(target_os = "macos") {
                std::env::var_os("HOME").map(|h| {
                    PathBuf::from(h).join(if cache {
                        "Library/Caches"
                    } else {
                        "Library/Application Support"
                    })
                })
            } else {
                std::env::var_os(if cache {
                    "XDG_CACHE_HOME"
                } else {
                    "XDG_DATA_HOME"
                })
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|h| {
                        PathBuf::from(h).join(if cache { ".cache" } else { ".local/share" })
                    })
                })
            }
        })
        .ok_or("no user cache/data directory configured")?;
    if !value.is_absolute() {
        return Err("cache/data root must be absolute".into());
    }
    Ok(value)
}
/// Refuse link/reparse ancestors and create new directories privately. Existing
/// OS parents need not be private; Nile's own root and every child must be.
fn directory(path: &Path, private: bool) -> std::result::Result<(), String> {
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        if matches!(part, std::path::Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) if is_link(&meta) || !meta.is_dir() => {
                return Err(format!("unsafe directory: {}", current.display()))
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let builder = fs::DirBuilder::new();
                #[cfg(unix)]
                let builder = {
                    use std::os::unix::fs::DirBuilderExt;
                    let mut b = builder;
                    b.mode(0o700);
                    b
                };
                #[cfg(windows)]
                let created = if private && current == path {
                    windows::create_private(&current)
                } else {
                    builder.create(&current)
                };
                #[cfg(not(windows))]
                let created = builder.create(&current);
                match created {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                    Err(e) => return Err(e.to_string()),
                }
                if is_link(&fs::symlink_metadata(&current).map_err(|e| e.to_string())?) {
                    return Err("directory replaced by symlink".into());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    if private {
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
                return Err(format!(
                    "cache/data directory is not user-private: {}",
                    path.display()
                ));
            }
        }
        #[cfg(target_os = "macos")]
        verify_macos_acl(path)?;
        #[cfg(windows)]
        {
            let _ = meta;
            windows::verify(path)?;
        }
    }
    Ok(())
}
// macOS extended ACLs can grant access beyond the Unix mode bits. Refuse
// nonempty ACLs conservatively rather than silently accept inherited grants.
#[cfg(target_os = "macos")]
fn verify_macos_acl(path: &Path) -> std::result::Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn acl_get_file(path: *const libc::c_char, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_get_entry(
            acl: *mut libc::c_void,
            entry: libc::c_int,
            out: *mut *mut libc::c_void,
        ) -> libc::c_int;
        fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
    let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    unsafe {
        let acl = acl_get_file(cpath.as_ptr(), 0x100); // ACL_TYPE_EXTENDED (sys/acl.h)
        if acl.is_null() {
            let error = std::io::Error::last_os_error();
            // Darwin reports ENOENT for an existing file with no extended ACL.
            if error.raw_os_error() == Some(libc::ENOENT) && fs::symlink_metadata(path).is_ok() {
                return Ok(());
            }
            return Err(error.to_string());
        }
        let mut entry = std::ptr::null_mut();
        let status = acl_get_entry(acl, 0, &mut entry); // ACL_FIRST_ENTRY
        let error = std::io::Error::last_os_error();
        acl_free(acl);
        if status == -1 && error.raw_os_error() == Some(libc::EINVAL) {
            Ok(())
        } else {
            Err("cache/data directory has unsupported extended ACL entries".into())
        }
    }
}
pub fn data_path(app: &Application) -> std::result::Result<PathBuf, String> {
    identity(app)?;
    let root = os_root(false)?.join("nile");
    directory(&root, true)?;
    let apps = root.join("apps");
    directory(&apps, true)?;
    let path = apps.join(&app.id);
    directory(&path, true)?;
    Ok(path)
}
fn cache_path(app: &Application, hash: &str) -> std::result::Result<PathBuf, String> {
    let root = os_root(true)?.join("nile-native");
    directory(&root, true)?;
    let app_dir = root.join(&app.id);
    directory(&app_dir, true)?;
    let path = app_dir.join(hash);
    directory(&path, true)?;
    Ok(path)
}
fn verified(root: &Path, package: &NativePackage) -> std::result::Result<(), String> {
    directory(root, true)?;
    fn inventory_walk(
        root: &Path,
        dir: &Path,
        found: &mut BTreeSet<String>,
    ) -> std::result::Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let meta = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            if is_link(&meta) {
                return Err("link/reparse point in cached inventory".into());
            }
            if meta.is_dir() {
                inventory_walk(root, &entry.path(), found)?;
            } else if meta.is_file() {
                found.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .map_err(|e| e.to_string())?
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            } else {
                return Err("non-file in cached inventory".into());
            }
        }
        Ok(())
    }
    let mut found = BTreeSet::new();
    inventory_walk(root, root, &mut found)?;
    if found != package.files.iter().map(|f| f.path.clone()).collect() {
        return Err("cached file inventory mismatch".into());
    }
    for entry in &package.files {
        let path = regular(root, &entry.path)?;
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() != entry.len {
            return Err("cached length mismatch".into());
        }
        let (len, hash) = hash_file(&path)?;
        if len != entry.len || hash != entry.sha256 {
            return Err("cached SHA-256 mismatch".into());
        }
    }
    Ok(())
}
/// Materialize only the inventory. Each generation is immutable: corruption rebuilds
/// into a new generation, never overwrites/deletes potentially loaded code.
pub fn materialize(
    app: &Application,
    target: &str,
    package: &NativePackage,
    source: &Path,
) -> Result<PathBuf> {
    let fail = |op: &str, e: String| failure(&package.import, target, op, e);
    validate_inventory_package(app, target, package).map_err(|e| fail("inventory", e))?;
    let cache = cache_path(app, &package.hash).map_err(|e| fail("cache-directory", e))?;
    let lock_path = cache.join("lock");
    if fs::symlink_metadata(&lock_path).is_ok_and(|m| is_link(&m) || !m.is_file()) {
        return Err(fail("lock", "unsafe cache lock".into()));
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options
        .open(&lock_path)
        .map_err(|e| fail("lock", e.to_string()))?;
    lock.lock().map_err(|e| fail("lock", e.to_string()))?;
    for entry in fs::read_dir(&cache).map_err(|e| fail("cache-scan", e.to_string()))? {
        let entry = entry.map_err(|e| fail("cache-scan", e.to_string()))?;
        if entry.file_name().to_string_lossy().starts_with("object-")
            && verified(&entry.path(), package).is_ok()
        {
            return Ok(entry.path());
        }
    }
    let mut builder = tempfile::Builder::new();
    builder.prefix(".stage-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let staging = builder
        .tempdir_in(&cache)
        .map_err(|e| fail("staging", e.to_string()))?;
    #[cfg(windows)]
    windows::protect(staging.path()).map_err(|e| fail("staging-acl", e))?;
    for entry in &package.files {
        let dest = staging.path().join(&entry.path);
        directory(dest.parent().unwrap(), true).map_err(|e| fail("staging-directory", e))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o700).custom_flags(libc::O_NOFOLLOW);
        }
        let mut output = options
            .open(&dest)
            .map_err(|e| fail("extract-create", e.to_string()))?;
        vfs::copy_to(
            &source.join(&entry.path),
            &mut output,
            entry.len.min(FILE_LIMIT),
        )
        .map_err(|e| fail("extract-stream", e.to_string()))?;
        output
            .sync_all()
            .map_err(|e| fail("extract-sync", e.to_string()))?;
    }
    verified(staging.path(), package).map_err(|e| fail("verify", e))?;
    let name = staging
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .replace(".stage-", "object-");
    let published = cache.join(name);
    fs::rename(staging.path(), &published).map_err(|e| fail("publish", e.to_string()))?;
    Ok(published)
}
fn validate_inventory_package(
    app: &Application,
    target: &str,
    p: &NativePackage,
) -> std::result::Result<(), String> {
    identity(app)?;
    safe(&p.root)?;
    safe(&p.entry)?;
    safe(&p.library)?;
    safe_set(p.files.iter().map(|f| f.path.as_str()))?;
    if p.files.len() > 128 || !p.files.iter().any(|f| f.path == p.library) {
        return Err("invalid native inventory".into());
    }
    let mut total = 0u64;
    for f in &p.files {
        if f.len > FILE_LIMIT
            || f.sha256.len() != 64
            || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("invalid native file length/hash".into());
        }
        total = total
            .checked_add(f.len)
            .filter(|n| *n <= PACKAGE_LIMIT)
            .ok_or("native package exceeds limit")?;
    }
    if p.hash != package_hash(p, target, app) {
        return Err("native package hash mismatch".into());
    }
    Ok(())
}
pub struct PreparedApplication {
    pub data_path: PathBuf,
    pub packages: Vec<PreparedPackage>,
}
/// Packed JS and manifests remain virtual. Only verified native entry paths reach
/// the host loader; all packages finish materialization before any initialization.
pub fn prepare(config_path: &Path) -> Result<PreparedApplication> {
    let target =
        nile_extensions::platform().map_err(|e| failure("application", "unknown", "target", e))?;
    let fail = |op: &str, e: String| failure("application", &target, op, e);
    let config_path = if vfs::is_virtual(config_path) {
        config_path.to_path_buf()
    } else {
        config_path
            .canonicalize()
            .map_err(|e| fail("config", e.to_string()))?
    };
    let config: Config = json(&config_path).map_err(|e| fail("config", e))?;
    let base = config_path
        .parent()
        .ok_or_else(|| fail("config", "config has no parent".into()))?;
    let packed = vfs::is_virtual(&config_path);
    if config.extensions.is_empty() {
        return Ok(PreparedApplication {
            data_path: base.to_path_buf(),
            packages: Vec::new(),
        });
    }
    let inventory: Option<Inventory> = if packed {
        Some(json(&vfs::mount_path().join(INVENTORY)).map_err(|e| fail("inventory", e))?)
    } else {
        None
    };
    let app = config.application.as_ref();
    if packed
        && (inventory.as_ref().unwrap().format != 1
            || inventory.as_ref().unwrap().target != target
            || app != Some(&inventory.as_ref().unwrap().application))
    {
        return Err(fail(
            "inventory",
            "application/target inventory mismatch".into(),
        ));
    }
    let data = if packed {
        data_path(app.ok_or_else(|| fail("identity", "missing application identity".into()))?)
            .map_err(|e| fail("data-directory", e))?
    } else {
        base.canonicalize()
            .map_err(|e| fail("config", e.to_string()))?
    };
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| fail("executable", e.to_string()))?;
    let mut packages = Vec::new();
    let mut ids = BTreeSet::new();
    if let Some(i) = &inventory {
        if i.packages.len() != config.extensions.len() {
            return Err(fail(
                "inventory",
                "configured/native inventory count mismatch".into(),
            ));
        }
    }
    for (import, location) in config.extensions {
        let fail = |op: &str, e: String| failure(&import, &target, op, e);
        let rel = location
            .package()
            .strip_prefix("./")
            .unwrap_or(location.package());
        if packed {
            safe(rel).map_err(|e| fail("package-path", e))?;
        }
        let root = if packed {
            base.join(rel)
        } else {
            base.join(location.package())
                .canonicalize()
                .map_err(|e| fail("package-path", e.to_string()))?
        };
        let manifest: Manifest =
            json(&root.join("extension.json")).map_err(|e| fail("manifest", e))?;
        validate_manifest(&import, &manifest, &target).map_err(|e| fail("manifest", e))?;
        if !ids.insert(manifest.id.clone()) {
            return Err(fail("manifest", "duplicate package identity".into()));
        }
        let native = &manifest.native[&target];
        let entry = root.join(&manifest.entry);
        vfs::read_limited(&entry, META_LIMIT).map_err(|e| fail("entry", e.to_string()))?;
        let native_root = if let Some(inventory) = &inventory {
            let p = inventory
                .packages
                .iter()
                .find(|p| p.import == import)
                .ok_or_else(|| fail("inventory", "missing package".into()))?;
            validate_inventory_package(&inventory.application, &target, p)
                .map_err(|e| fail("inventory", e))?;
            if p.root != rel
                || p.id != manifest.id
                || p.version != manifest.version
                || p.entry != manifest.entry
                || p.library != native.library()
                || p.external != location.external()
                || p.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>() != native.files()
            {
                return Err(fail(
                    "inventory",
                    "manifest/native inventory mismatch".into(),
                ));
            }
            if let Some(external) = &p.external {
                let path = external_path(external, &exe).map_err(|e| fail("external-mode", e))?;
                // External distribution directories need not be writable/private caches.
                for f in &p.files {
                    let real = regular(&path, &f.path).map_err(|e| fail("external-file", e))?;
                    let (len, hash) = hash_file(&real).map_err(|e| fail("external-hash", e))?;
                    if len != f.len || hash != f.sha256 {
                        return Err(fail("external-hash", "native integrity mismatch".into()));
                    }
                }
                path
            } else {
                materialize(&inventory.application, &target, p, &root)?
            }
        } else if let Some(external) = location.external() {
            external_path(&external, &exe).map_err(|e| fail("external-mode", e))?
        } else {
            root.clone()
        };
        let library_path =
            regular(&native_root, native.library()).map_err(|e| fail("library", e))?;
        for path in native.files() {
            regular(&native_root, path).map_err(|e| fail("dependency", e))?;
        }
        packages.push(PreparedPackage {
            import,
            namespace: manifest.id,
            root,
            entry,
            library_path,
        });
    }
    Ok(PreparedApplication {
        data_path: data,
        packages,
    })
}

pub fn host_target() -> std::result::Result<String, String> {
    nile_extensions::platform()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    static ENV: Mutex<()> = Mutex::new(());
    fn example(root: &Path) {
        fs::create_dir_all(root.join("extensions/demo/native")).unwrap();
        fs::write(root.join("nile.json"), r#"{"application":{"id":"org.nile.test","version":"1"},"extensions":{"@demo":"./extensions/demo"}}"#).unwrap();
        fs::write(root.join("extensions/demo/index.js"), "export default 42").unwrap();
        fs::write(
            root.join("extensions/demo/native/main.bin"),
            b"native-entry",
        )
        .unwrap();
        fs::write(
            root.join("extensions/demo/native/companion.bin"),
            b"companion",
        )
        .unwrap();
        fs::write(root.join("extensions/demo/extension.json"), serde_json::json!({"id":"demo","version":"1","abi":1,"entry":"index.js","native":{host_target().unwrap():{"library":"native/main.bin","files":["native/companion.bin"]}}}).to_string()).unwrap();
    }
    #[test]
    fn planning_rejects_traversal_duplicates_conflicts_and_missing_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        example(&root);
        assert!(identity(&Application {
            id: "Org.Nile.Test".into(),
            version: "1".into()
        })
        .is_err());
        assert!(identity(&Application {
            id: "org.nile.test".into(),
            version: "1".into()
        })
        .is_ok());
        let target = host_target().unwrap();
        let manifest = root.join("extensions/demo/extension.json");
        let original = fs::read_to_string(&manifest).unwrap();
        let plan_ok = plan(&root, &target).unwrap();
        assert_eq!(plan_ok.inventory.unwrap().packages[0].files.len(), 2);
        for paths in [
            vec!["../escape"],
            vec!["/absolute"],
            vec!["C:/absolute"],
            vec!["native/a", "native/a/b"],
            vec!["native/main.bin"],
            vec!["native/A", "native/a"],
            vec!["missing.bin"],
        ] {
            let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
            value["native"][&target]["files"] = serde_json::json!(paths);
            fs::write(&manifest, value.to_string()).unwrap();
            assert!(plan(&root, &target).is_err());
        }
        fs::write(&manifest, &original).unwrap();
        assert!(plan(&root, "unsupported-target").is_err());
        let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
        value["native"] = serde_json::json!({});
        fs::write(&manifest, value.to_string()).unwrap();
        assert!(plan(&root, &target)
            .err()
            .unwrap()
            .to_string()
            .contains("missing platform"));
        fs::write(&manifest, &original).unwrap();
        fs::write(
            root.join("nile.json"),
            r#"{"extensions":{"@demo":"a","@demo":"b"}}"#,
        )
        .unwrap();
        assert!(plan(&root, &target).is_err());
    }
    #[test]
    fn immutable_cache_concurrent_reuse_corruption_versions_and_interrupted_stages() {
        let _guard = ENV.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        example(&root);
        let old = std::env::var_os("NILE_NATIVE_CACHE_HOME");
        std::env::set_var("NILE_NATIVE_CACHE_HOME", root.join("cache"));
        let inventory = plan(&root, &host_target().unwrap())
            .unwrap()
            .inventory
            .unwrap();
        let app = inventory.application.clone();
        let package = inventory.packages[0].clone();
        let target = inventory.target.clone();
        let source = root.join(&package.root);
        let first = materialize(&app, &target, &package, &source).unwrap();
        assert!(!first.join("index.js").exists());
        assert!(!first.join("extension.json").exists());
        let mut threads = Vec::new();
        let barrier = Arc::new(std::sync::Barrier::new(6));
        for _ in 0..6 {
            let (app, target, package, source, barrier) = (
                app.clone(),
                target.clone(),
                package.clone(),
                source.clone(),
                barrier.clone(),
            );
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                materialize(&app, &target, &package, &source).unwrap()
            }));
        }
        for worker in threads {
            assert_eq!(worker.join().unwrap(), first);
        }
        fs::write(first.join(&package.library), b"tampered-lib").unwrap();
        fs::create_dir(first.parent().unwrap().join(".stage-interrupted")).unwrap();
        let second = materialize(&app, &target, &package, &source).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            fs::read(first.join(&package.library)).unwrap(),
            b"tampered-lib"
        );
        fs::write(second.join("undeclared.dll"), b"extra").unwrap();
        let third = materialize(&app, &target, &package, &source).unwrap();
        assert_ne!(third, second);
        let mut changed = package.clone();
        changed.version = "2".into();
        changed.hash = package_hash(&changed, &target, &app);
        let version = materialize(&app, &target, &changed, &source).unwrap();
        assert_ne!(version.parent(), third.parent());
        assert!(third.exists());
        let mut huge = package.clone();
        huge.files[0].len = FILE_LIMIT + 1;
        huge.hash = package_hash(&huge, &target, &app);
        assert!(materialize(&app, &target, &huge, &source).is_err());
        let mut bad = package.clone();
        bad.files[0].sha256 = "0".repeat(64);
        bad.hash = package_hash(&bad, &target, &app);
        assert!(materialize(&app, &target, &bad, &source).is_err());
        let bad_cache = cache_path(&app, &bad.hash).unwrap();
        assert!(!fs::read_dir(bad_cache).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".stage-")));
        if let Some(old) = old {
            std::env::set_var("NILE_NATIVE_CACHE_HOME", old);
        } else {
            std::env::remove_var("NILE_NATIVE_CACHE_HOME");
        }
    }
    #[cfg(unix)]
    #[test]
    fn rejects_source_and_cache_symlinks() {
        use std::os::unix::fs::symlink;
        let _guard = ENV.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        example(&root);
        let entry = root.join("extensions/demo/native/main.bin");
        fs::remove_file(&entry).unwrap();
        symlink("/etc/passwd", &entry).unwrap();
        assert!(plan(&root, &host_target().unwrap()).is_err());
        fs::remove_file(&entry).unwrap();
        fs::write(&entry, b"native-entry").unwrap();
        let inventory = plan(&root, &host_target().unwrap())
            .unwrap()
            .inventory
            .unwrap();
        let old = std::env::var_os("NILE_NATIVE_CACHE_HOME");
        fs::create_dir(root.join("real-cache")).unwrap();
        symlink(root.join("real-cache"), root.join("cache")).unwrap();
        std::env::set_var("NILE_NATIVE_CACHE_HOME", root.join("cache"));
        assert!(materialize(
            &inventory.application,
            &inventory.target,
            &inventory.packages[0],
            &root.join("extensions/demo")
        )
        .is_err());
        if let Some(old) = old {
            std::env::set_var("NILE_NATIVE_CACHE_HOME", old);
        } else {
            std::env::remove_var("NILE_NATIVE_CACHE_HOME");
        }
    }
}
