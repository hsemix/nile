#![cfg(target_os = "macos")]

use std::process::{Command, Output};

fn success(output: Output) -> Output {
    assert!(output.status.success(), "stdout={} stderr={}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    output
}

#[test]
fn native_pack_creates_and_replaces_a_signed_runnable_bundle() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let source = root.join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("index.html"), "<html><body>Packaged app<script>console.log('bundle-smoke-ready')</script></body></html>").unwrap();
    let app = root.join("Smoke.app");
    let pack = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nile"));
        command.arg("--pack").arg(&source).arg("-o").arg(&app);
        command
    };
    success(pack().output().unwrap());
    let icon = app.join("Contents/Resources/AppIcon.icns");
    assert_eq!(&std::fs::read(&icon).unwrap()[..4], b"icns");
    let decoded = success(Command::new("sips").arg("-g").arg("pixelWidth").arg(&icon).output().unwrap());
    assert!(String::from_utf8_lossy(&decoded.stdout).contains("1024"), "macOS must decode the generated icon");
    let custom = root.join("custom.icns");
    std::fs::copy(&icon, &custom).unwrap();
    // An existing .app directory must be replaceable. Already encoded ICNS
    // icons must retain their original bytes instead of being re-encoded.
    success(pack().arg("--icon").arg(&custom).output().unwrap());
    assert_eq!(std::fs::read(&icon).unwrap(), std::fs::read(&custom).unwrap());
    success(Command::new("codesign").args(["--verify", "--deep", "--strict"]).arg(&app).output().unwrap());
    std::fs::remove_dir_all(&source).unwrap();
    let image = root.join("smoke.png");
    let rendered = success(Command::new(app.join("Contents/MacOS/Smoke"))
        .arg("--png").arg(&image).env("NILE_STORAGE_DIR", root.join("storage")).output().unwrap());
    assert!(String::from_utf8_lossy(&rendered.stdout).contains("bundle-smoke-ready"));
    assert!(image.is_file());
}
