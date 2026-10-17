//! Verify an SDK consumer builds without engine/QuickJS dependencies.
mod support;
use std::process::Command;
use support::{output, Result};
fn run() -> Result<()> {
    let root = support::root();
    let temporary = tempfile::tempdir()?;
    let base = temporary.path();
    support::copy_tree(
        &root.join("crates/nile-extension-sdk"),
        &base.join("crates/nile-extension-sdk"),
    )?;
    support::copy_tree(
        &root.join("extensions/sqlite"),
        &base.join("extensions/sqlite"),
    )?;
    let manifest = base.join("extensions/sqlite/Cargo.toml");
    let metadata = output(
        Command::new(env!("CARGO"))
            .env("CARGO_NET_OFFLINE", "true")
            .args([
                "metadata",
                "--offline",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(&manifest),
    )?;
    let names = support::package_names(&metadata.stdout)?;
    for forbidden in [
        "nile-script",
        "nile-engine",
        "nile-dom",
        "nile-style",
        "nile-layout",
        "nile-render",
        "nile-shell",
        "rquickjs",
    ] {
        if names.contains(forbidden) {
            return Err(format!("SDK consumer acquired engine dependency: {forbidden}").into());
        }
    }
    let status = Command::new(env!("CARGO"))
        .env("CARGO_NET_OFFLINE", "true")
        .args(["build", "--offline", "--release", "--manifest-path"])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(root.join("target/standalone-extensions"))
        .status()?;
    if !status.success() {
        return Err("standalone extension build failed".into());
    }
    println!("Standalone SQLite build passed; no engine or QuickJS dependencies.");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
