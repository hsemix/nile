//! Signed content updates for packed apps.
//!
//! A packed app is the `nile` runtime plus an archive of the app's files. An update replaces the *archive*: the
//! runtime (and so the signed executable, the icon, the `.app` bundle) is never touched, which makes an update
//! a plain file download that works the same on every platform and while the program is running.
//!
//! Trust starts at the install. The app's own `nile.json` names the Ed25519 public key updates must be signed
//! with (`"update": {"publicKey": "<base64>", "url": "https://.../update.json"}`). A staged update is only
//! mounted at the next start when its SHA-256 and signature check out against that key *and* it is newer; the
//! update's own `nile.json` may name a new key, which then governs the updates after it.
//!
//! The manifest (`update.json`) is `{"version", "url", "sha256", "signature", "size", "notes", "minRuntime"}`;
//! the signature covers `nile-update-v1\n<version>\n<sha256>\n`. `nile --keygen` and `nile --release` make them.

use std::io::Read;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ring::{
    digest,
    rand::SystemRandom,
    signature::{self, Ed25519KeyPair, KeyPair, UnparsedPublicKey},
};

/// This runtime's version (what `minRuntime` is compared with).
pub const RUNTIME: &str = env!("CARGO_PKG_VERSION");

/// What an app's `nile.json` says about itself and its updates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    pub id: String,
    pub version: String,
    pub public_key: String,
    pub url: String,
}

impl Config {
    pub fn parse(nile_json: &str) -> Config {
        let v: serde_json::Value = serde_json::from_str(nile_json).unwrap_or_default();
        let text = |v: Option<&serde_json::Value>| v.and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        let id = match text(v.get("id")) {
            id if !id.is_empty() => id,
            _ => std::env::current_exe().ok().and_then(|e| e.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "app".into()),
        };
        Config { id, version: text(v.get("version")), public_key: text(v.pointer("/update/publicKey")), url: text(v.pointer("/update/url")) }
    }

    /// The running program's configuration, from the archive that is mounted.
    pub fn mounted() -> Config {
        Config::parse(&nile_dom::vfs::read_to_string(&nile_dom::vfs::mount_path().join("nile.json")).unwrap_or_default())
    }

    /// The configuration inside an archive that isn't mounted (yet).
    pub fn of_archive(archive: &nile_dom::vfs::Archive) -> Config {
        let mut buf = Vec::new();
        let _ = archive.copy_to("nile.json", &mut buf, 1 << 20);
        Config::parse(&String::from_utf8_lossy(&buf))
    }
}

fn numbers(version: &str) -> Vec<u64> {
    version.split(['-', '+']).next().unwrap_or("").split('.').map(|p| p.trim().chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect()
}

/// Whether version `a` is newer than `b` (numeric parts; a pre-release suffix is ignored).
pub fn is_newer(a: &str, b: &str) -> bool {
    let (mut x, mut y) = (numbers(a), numbers(b));
    let len = x.len().max(y.len());
    x.resize(len, 0);
    y.resize(len, 0);
    x > y
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of a file, streamed.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut ctx = digest::Context::new(&digest::SHA256);
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    Ok(hex(ctx.finish().as_ref()))
}

fn sha256(bytes: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, bytes).as_ref())
}

/// What is signed.
fn message(version: &str, sha256: &str) -> Vec<u8> {
    format!("nile-update-v1\n{version}\n{}\n", sha256.to_ascii_lowercase()).into_bytes()
}

pub fn verify(public_key_b64: &str, version: &str, sha256: &str, signature_b64: &str) -> bool {
    let (Ok(key), Ok(sig)) = (B64.decode(public_key_b64.trim()), B64.decode(signature_b64.trim())) else { return false };
    UnparsedPublicKey::new(&signature::ED25519, key).verify(&message(version, sha256), &sig).is_ok()
}

pub fn sign(key_pkcs8_b64: &str, version: &str, sha256: &str) -> Result<String, String> {
    let pkcs8 = B64.decode(key_pkcs8_b64.trim()).map_err(|_| "the key file is not base64")?;
    let pair = Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|_| "the key file is not an Ed25519 key")?;
    Ok(B64.encode(pair.sign(&message(version, sha256)).as_ref()))
}

/// A new signing key: (private key as base64 PKCS#8, public key as base64).
pub fn keygen() -> Result<(String, String), String> {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| "no randomness available")?;
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).map_err(|_| "could not read the new key")?;
    Ok((B64.encode(pkcs8.as_ref()), B64.encode(pair.public_key().as_ref())))
}

/// The public key of a private key file's contents.
pub fn public_key_of(key_pkcs8_b64: &str) -> Option<String> {
    let pair = Ed25519KeyPair::from_pkcs8(&B64.decode(key_pkcs8_b64.trim()).ok()?).ok()?;
    Some(B64.encode(pair.public_key().as_ref()))
}

/// Where this app's staged update lives: `<user data>/nile/updates/<id>/`.
fn store(id: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support");
    #[cfg(windows)]
    let base = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or(PathBuf::from(std::env::var_os("HOME")?).join(".local/share"));
    let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    Some(base.join("nile/updates").join(safe))
}

#[cfg(test)]
thread_local! {
    /// Tests keep staged updates out of the user's folders.
    static TEST_STORE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn store_dir(id: &str) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(dir) = TEST_STORE.with(|s| s.borrow().clone()) {
        return Some(dir.join(id));
    }
    store(id)
}

/// A downloaded update, checked against the app's key and staged for the next start.
pub fn stage(config: &Config, manifest_json: &str, bytes: &[u8]) -> Result<String, String> {
    let m: serde_json::Value = serde_json::from_str(manifest_json).map_err(|_| "the update manifest is not JSON")?;
    let field = |name: &str| m.get(name).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let (version, want_hash, signature) = (field("version"), field("sha256").to_ascii_lowercase(), field("signature"));
    if config.public_key.is_empty() {
        return Err("this app has no update key (nile.json: update.publicKey)".into());
    }
    if version.is_empty() || want_hash.is_empty() || signature.is_empty() {
        return Err("the update manifest lacks a version, hash or signature".into());
    }
    if !is_newer(&version, &config.version) {
        return Err(format!("version {version} is not newer than {}", config.version));
    }
    let min = field("minRuntime");
    if !min.is_empty() && is_newer(&min, RUNTIME) {
        return Err(format!("the update needs Nile {min} or newer (this is {RUNTIME})"));
    }
    if sha256(bytes) != want_hash {
        return Err("the download does not match its hash".into());
    }
    if !verify(&config.public_key, &version, &want_hash, &signature) {
        return Err("the update is not signed with this app's key".into());
    }
    let dir = store_dir(&config.id).ok_or("no folder for updates")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let pack = dir.join("current.nilepack");
    let temp = dir.join("current.nilepack.tmp");
    std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    // It must be a readable archive before it can replace anything.
    if nile_dom::vfs::Archive::open(&temp).is_err() {
        let _ = std::fs::remove_file(&temp);
        return Err("the update is not a Nile archive".into());
    }
    let meta = serde_json::json!({ "version": version, "sha256": want_hash, "signature": signature }).to_string();
    // The meta file goes last: until it names the new pack the old one is what start-up finds.
    std::fs::rename(&temp, &pack).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("current.json"), meta).map_err(|e| e.to_string())?;
    Ok(version)
}

/// The staged update's file and version, if it is intact, correctly signed and newer than `config`'s version.
pub fn staged(config: &Config) -> Option<(PathBuf, String)> {
    if config.public_key.is_empty() {
        return None;
    }
    let dir = store_dir(&config.id)?;
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("current.json")).ok()?).ok()?;
    let field = |name: &str| meta.get(name).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let (version, hash, sig) = (field("version"), field("sha256"), field("signature"));
    let pack = dir.join("current.nilepack");
    let ok = is_newer(&version, &config.version) && sha256_file(&pack).ok()? == hash && verify(&config.public_key, &version, &hash, &sig);
    ok.then_some((pack, version))
}

/// Start-up: the staged update to mount in place of the program's own archive.
pub fn replacement(own: &nile_dom::vfs::Archive) -> Option<PathBuf> {
    staged(&Config::of_archive(own)).map(|(pack, _)| pack)
}

/// The host functions scripts call for `application.checkForUpdates()` and friends.
pub fn register_host_functions() {
    nile_script::add_default_host_fn("nileUpdateConfig", |_| {
        let c = Config::mounted();
        let ready = staged(&c).map(|(_, v)| v);
        serde_json::json!({ "id": c.id, "version": c.version, "url": c.url, "signed": !c.public_key.is_empty(), "ready": ready, "runtime": RUNTIME }).to_string()
    });
    nile_script::add_default_host_fn("nileUpdateNewer", |args| {
        let a: Vec<String> = serde_json::from_str(args).unwrap_or_default();
        serde_json::json!(a.len() == 2 && is_newer(&a[0], &a[1])).to_string()
    });
    nile_script::add_default_host_fn("nileUpdateStage", |args| {
        let a: Vec<String> = serde_json::from_str(args).unwrap_or_default();
        let result = match (a.first(), a.get(1).map(|b| B64.decode(b))) {
            (Some(manifest), Some(Ok(bytes))) => stage(&Config::mounted(), manifest, &bytes),
            _ => Err("expected the manifest and the downloaded bytes".to_string()),
        };
        match result {
            Ok(version) => serde_json::json!({ "ok": true, "version": version }),
            Err(error) => serde_json::json!({ "error": error }),
        }
        .to_string()
    });
    nile_script::add_default_host_fn("nileUpdateRestart", |_| {
        let Ok(exe) = std::env::current_exe() else { return "false".into() };
        // The new process waits for this one to let go of the single-instance endpoint (`NILE_RELAUNCH`).
        let started = std::process::Command::new(exe)
            .args(std::env::args().skip(1))
            .env("NILE_RELAUNCH", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok();
        started.to_string()
    });
}

/// `nile --keygen <name>`: writes `<name>.key` (private, base64) and prints the public key for `nile.json`.
pub fn keygen_command(args: &[String]) -> ! {
    let Some(name) = args.first() else {
        eprintln!("usage: nile --keygen <name>   (writes <name>.key; keep it secret)");
        std::process::exit(2);
    };
    let path = PathBuf::from(format!("{name}.key"));
    if path.exists() {
        eprintln!("{} already exists", path.display());
        std::process::exit(1);
    }
    match keygen() {
        Ok((private, public)) => {
            if let Err(e) = std::fs::write(&path, private) {
                eprintln!("could not write {}: {e}", path.display());
                std::process::exit(1);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            println!("wrote {} (keep it secret)\npublic key for nile.json (\"update\": {{\"publicKey\": ...}}):\n{public}", path.display());
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

/// `nile --release <folder> --key <name>.key [--version V] [--url URL] [--notes TEXT] [--min-runtime V] [-o dir]`:
/// the app's archive as `<name>-<version>.nilepack` and its signed `update.json`.
pub fn release_command(args: &[String]) -> ! {
    let (mut folder, mut key, mut version, mut url, mut notes, mut min_runtime, mut out) = (None, None, None, None, None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--key" => key = it.next().map(PathBuf::from),
            "--version" => version = it.next().cloned(),
            "--url" => url = it.next().cloned(),
            "--notes" => notes = it.next().cloned(),
            "--min-runtime" => min_runtime = it.next().cloned(),
            "-o" | "--out" => out = it.next().map(PathBuf::from),
            _ => folder = Some(PathBuf::from(a)),
        }
    }
    let fail = |message: String| -> ! {
        eprintln!("{message}");
        std::process::exit(1);
    };
    let (Some(folder), Some(key)) = (folder.filter(|f| f.is_dir()), key) else {
        eprintln!("usage: nile --release <folder> --key <name>.key [--version V] [--url URL] [--notes TEXT] [--min-runtime V] [-o dir]");
        std::process::exit(2);
    };
    let config = Config::parse(&std::fs::read_to_string(folder.join("nile.json")).unwrap_or_default());
    let version = version.unwrap_or_else(|| config.version.clone());
    if version.is_empty() {
        fail("no version: give --version or set \"version\" in nile.json".into());
    }
    let key_text = std::fs::read_to_string(&key).unwrap_or_else(|e| fail(format!("could not read {}: {e}", key.display())));
    let public = public_key_of(&key_text).unwrap_or_else(|| fail("the key file is not an Ed25519 key".into()));
    if config.public_key != public {
        eprintln!("warning: nile.json's update.publicKey is not this key's public key ({public}); installed copies would refuse the update");
    }
    let out = out.unwrap_or_else(|| PathBuf::from("dist"));
    if let Err(e) = std::fs::create_dir_all(&out) {
        fail(format!("could not create {}: {e}", out.display()));
    }
    let target = nile_extension_pack::host_target().unwrap_or_default();
    let plan = nile_extension_pack::plan(&folder, &target).unwrap_or_else(|e| fail(format!("could not plan native extensions: {e}")));
    let mut embedded: Vec<(&str, Vec<u8>)> = Vec::new();
    match plan.metadata() {
        Ok(Some(bytes)) => embedded.push((nile_extension_pack::INVENTORY, bytes)),
        Ok(None) => (),
        Err(e) => fail(format!("could not serialize the extension inventory: {e}")),
    }
    let name = std::fs::canonicalize(&folder).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "app".into());
    let pack_name = format!("{name}-{version}.nilepack");
    let pack_path = out.join(&pack_name);
    if let Err(e) = nile_dom::vfs::pack_selected(&folder, &embedded, &[], &pack_path, |p| plan.include(p)) {
        fail(format!("could not pack {}: {e}", folder.display()));
    }
    let hash = sha256_file(&pack_path).unwrap_or_else(|e| fail(e.to_string()));
    let signature = sign(&key_text, &version, &hash).unwrap_or_else(|e| fail(e));
    let size = std::fs::metadata(&pack_path).map(|m| m.len()).unwrap_or(0);
    let mut manifest = serde_json::json!({ "version": version, "url": url.unwrap_or(pack_name), "sha256": hash, "signature": signature, "size": size });
    if let Some(n) = notes {
        manifest["notes"] = n.into();
    }
    if let Some(m) = min_runtime {
        manifest["minRuntime"] = m.into();
    }
    let manifest_path = out.join("update.json");
    if let Err(e) = std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap_or_default()) {
        fail(format!("could not write {}: {e}", manifest_path.display()));
    }
    println!("wrote {} ({} KB) and {}", pack_path.display(), size / 1024, manifest_path.display());
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive_bytes() -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        std::fs::create_dir(&app).unwrap();
        std::fs::write(app.join("index.html"), "<p>hi</p>").unwrap();
        std::fs::write(app.join("nile.json"), r#"{"version":"2.0.0"}"#).unwrap();
        let out = dir.path().join("a.nilepack");
        nile_dom::vfs::pack_selected(&app, &[], &[], &out, |_| true).unwrap();
        std::fs::read(out).unwrap()
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("1.10.0", "1.9.9") && is_newer("2", "1.99") && is_newer("1.0.1", "1.0"));
        assert!(!is_newer("1.0", "1.0.0") && !is_newer("1.2.3-beta", "1.2.3") && !is_newer("0.9", "1.0"));
    }

    #[test]
    fn signatures_check_the_version_and_hash() {
        let (private, public) = keygen().unwrap();
        let sig = sign(&private, "1.2.0", "ABCD").unwrap();
        assert!(verify(&public, "1.2.0", "abcd", &sig), "the hash is compared case-insensitively");
        assert!(!verify(&public, "1.2.1", "abcd", &sig) && !verify(&public, "1.2.0", "abce", &sig));
        let (_, other) = keygen().unwrap();
        assert!(!verify(&other, "1.2.0", "abcd", &sig) && !verify("not base64!", "1.2.0", "abcd", &sig));
        assert_eq!(public_key_of(&private).unwrap(), public);
    }

    #[test]
    fn an_update_is_staged_only_when_hash_signature_and_version_are_right() {
        let store = tempfile::tempdir().unwrap();
        TEST_STORE.with(|s| *s.borrow_mut() = Some(store.path().to_path_buf()));
        let (private, public) = keygen().unwrap();
        let config = Config { id: "demo".into(), version: "1.0.0".into(), public_key: public, url: String::new() };
        let bytes = archive_bytes();
        let hash = sha256(&bytes);
        let manifest = |version: &str, hash: &str, key: &str| serde_json::json!({ "version": version, "sha256": hash, "signature": sign(key, version, hash).unwrap() }).to_string();

        assert!(staged(&config).is_none());
        assert!(stage(&config, &manifest("1.0.0", &hash, &private), &bytes).unwrap_err().contains("not newer"));
        assert!(stage(&config, &manifest("1.1.0", "00", &private), &bytes).unwrap_err().contains("hash"));
        let (stranger, _) = keygen().unwrap();
        assert!(stage(&config, &manifest("1.1.0", &hash, &stranger), &bytes).unwrap_err().contains("signed"));
        let mut tampered = bytes.clone();
        tampered.push(0);
        assert!(stage(&config, &manifest("1.1.0", &hash, &private), &tampered).is_err());
        let mut needs_newer = serde_json::from_str::<serde_json::Value>(&manifest("1.1.0", &hash, &private)).unwrap();
        needs_newer["minRuntime"] = "99.0".into();
        assert!(stage(&config, &needs_newer.to_string(), &bytes).unwrap_err().contains("newer"));
        assert!(staged(&config).is_none(), "nothing was staged by the refusals");

        assert_eq!(stage(&config, &manifest("1.1.0", &hash, &private), &bytes).unwrap(), "1.1.0");
        let (pack, version) = staged(&config).unwrap();
        assert_eq!(version, "1.1.0");
        assert!(nile_dom::vfs::Archive::open(&pack).is_ok());
        // Once the app itself is at that version (or newer) the staged copy is stale.
        assert!(staged(&Config { version: "1.1.0".into(), ..config.clone() }).is_none());
        // A file changed behind our back is refused at start-up.
        std::fs::write(&pack, b"corrupt").unwrap();
        assert!(staged(&config).is_none());
        // And no key means no updates at all.
        assert!(staged(&Config { public_key: String::new(), ..config }).is_none());
        TEST_STORE.with(|s| *s.borrow_mut() = None);
    }
}
