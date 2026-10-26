#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
/// `path.canonicalize()` without the `\\?\` prefix Windows adds, which cl.exe cannot open source
/// files through.
pub fn canonical(path: &Path) -> PathBuf {
    let path = path.canonicalize().unwrap();
    match path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
        Some(plain) if !plain.starts_with("UNC\\") => PathBuf::from(plain),
        _ => path,
    }
}
pub fn root() -> PathBuf {
    canonical(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}
pub fn libraries() -> &'static PathBuf {
    static LIBS: OnceLock<PathBuf> = OnceLock::new();
    LIBS.get_or_init(|| {
        let root = root();
        // A separate directory avoids locking the parent's cargo artifact directory during tests.
        let dir = root.join("target/extension-tests");
        let status = std::process::Command::new(env!("CARGO"))
            .current_dir(&root)
            .args([
                "build",
                "--offline",
                "-p",
                "nile-ext-sqlite",
                "-p",
                "nile-ext-notifications",
                "-p",
                "nile-ext-media",
                "--target-dir",
            ])
            .arg(&dir)
            .status()
            .unwrap();
        assert!(status.success(), "build actual extension shared libraries");
        dir.join("debug")
    })
}
pub fn library(name: &str) -> PathBuf {
    libraries().join(format!(
        "{}nile_ext_{}{}",
        std::env::consts::DLL_PREFIX,
        name,
        std::env::consts::DLL_SUFFIX
    ))
}
pub fn fixture(dir: &Path, define: Option<&str>) -> PathBuf {
    fixture_source(dir, define, "fixture.c")
}
pub fn fixture_source(dir: &Path, define: Option<&str>, filename: &str) -> PathBuf {
    let out = dir.join(format!("fixture{}", std::env::consts::DLL_SUFFIX));
    let compiler = cc::Build::new()
        .target(&target())
        .host(&target())
        .opt_level(0)
        .get_compiler();
    let mut command = compiler.to_command();
    let header = root().join("crates/nile-extension-sdk/include");
    let source = root()
        .join("crates/nile-extensions/tests/support")
        .join(filename);
    if compiler.is_like_msvc() {
        command
            .arg("/LD")
            .arg("/W3")
            .arg(format!("/I{}", header.display()));
        if let Some(define) = define {
            command.arg(format!("/D{define}"));
        }
        command
            .arg(source)
            .arg("/link")
            .arg(format!("/OUT:{}", out.display()));
    } else {
        command
            .args([
                "-shared",
                "-fPIC",
                "-Wall",
                "-Werror",
                "-Wno-unused-function",
            ])
            .arg("-I")
            .arg(header);
        if let Some(define) = define {
            command.arg(format!("-D{define}"));
        }
        command.arg(source).arg("-o").arg(&out);
    }
    let output = command.current_dir(dir).output().unwrap();
    if !output.status.success() {
        // One `error:` line per compiler line, so CI's failure annotation shows the cause.
        for line in String::from_utf8_lossy(&output.stdout).lines().chain(String::from_utf8_lossy(&output.stderr).lines()) {
            eprintln!("error: C fixture compiler: {line}");
        }
        panic!("compile external C extension against generated header: {command:?}");
    }
    out
}
fn target() -> String {
    let output = std::process::Command::new("rustc")
        .arg("-vV")
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|s| s.strip_prefix("host: ").map(str::to_string))
        .unwrap()
}
pub fn package(base: &Path, name: &str, import: &str, library: &Path, entry: &str) -> PathBuf {
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    // Unique native path, still within the package. Copy rather than symlink for Windows.
    let native = format!("native{}", std::env::consts::DLL_SUFFIX);
    std::fs::copy(library, dir.join(&native)).unwrap();
    std::fs::write(dir.join("extension.json"), serde_json::json!({"id":name, "version":"0.1.0", "abi":1,"entry":"index.js","native":{nile_extensions::platform().unwrap():native}}).to_string()).unwrap();
    std::fs::write(dir.join("index.js"), entry).unwrap();
    let config = base.join("nile.json");
    std::fs::write(
        &config,
        serde_json::json!({"extensions":{import: format!("./{name}")}}).to_string(),
    )
    .unwrap();
    config
}
