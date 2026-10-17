//! Assemble a native package without a Python build dependency.
mod support;
use std::{fs, path::PathBuf, process::Command};
use support::{output, Result};
fn run() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let name = arguments.next().ok_or(
        "usage: build-extension <sqlite|notifications> --dest PATH [--release] [--target TRIPLE]",
    )?;
    if !["sqlite", "notifications"].contains(&name.as_str()) {
        return Err("unknown extension".into());
    }
    let (mut destination, mut target, mut release) = (None, None, false);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--dest" => {
                destination = Some(PathBuf::from(
                    arguments.next().ok_or("--dest needs a path")?,
                ))
            }
            "--target" => target = Some(arguments.next().ok_or("--target needs a triple")?),
            "--release" => release = true,
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    let destination = destination.ok_or("--dest is required")?;
    let triple = match &target {
        Some(target) => target.clone(),
        None => support::host(output(Command::new("rustc").arg("-vV"))?.stdout)?,
    };
    let (platform, prefix, suffix) = support::platform(&triple)?;
    let root = support::root();
    let mut build = Command::new(env!("CARGO"));
    build
        .current_dir(&root)
        .args(["build", "-p", &format!("nile-ext-{name}"), "--target-dir"])
        .arg(root.join("target"));
    if let Some(target) = &target {
        build.args(["--target", target]);
    }
    if release {
        build.args(["--profile", "extension"]);
    }
    if !build.status()?.success() {
        return Err("extension build failed".into());
    }
    let mut binary_dir = root.join("target");
    if let Some(target) = target {
        binary_dir.push(target);
    }
    binary_dir.push(if release { "extension" } else { "debug" });
    let binary_name = format!("{prefix}nile_ext_{name}{suffix}");
    let native = format!("native/{platform}/{binary_name}");
    fs::create_dir_all(destination.join("native").join(platform))?;
    fs::copy(binary_dir.join(binary_name), destination.join(&native))?;
    fs::copy(
        root.join(format!("extensions/{name}/package/index.js")),
        destination.join("index.js"),
    )?;
    let manifest_path = destination.join("extension.json");
    let mut manifest = if manifest_path.exists() {
        serde_json::from_slice(&fs::read(&manifest_path)?)?
    } else {
        serde_json::json!({"id":name,"version":"0.1.0","abi":1,"entry":"index.js","native":{}})
    };
    if !manifest["native"].is_object() {
        return Err("manifest native field must be an object".into());
    }
    manifest["native"][platform] = serde_json::Value::String(native);
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;
    println!("Assembled {} ({platform})", destination.display());
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
