use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
fn source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/neon-breaker")
}
fn copy_game(destination: &Path) {
    fs::create_dir_all(destination.join("sounds")).unwrap();
    for entry in fs::read_dir(source()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
        }
    }
    for entry in fs::read_dir(source().join("sounds")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(
            entry.path(),
            destination.join("sounds").join(entry.file_name()),
        )
        .unwrap();
    }
}
fn run(mut command: Command, root: &Path, label: &str) -> String {
    let log_path = root.join(format!("{label}.log"));
    let log = fs::File::create(&log_path).unwrap();
    let mut child = command
        .env("NILE_AUDIO", "off")
        .env("NILE_STORAGE_DIR", root.join("storage"))
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(35);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{}",
                fs::read_to_string(&log_path).unwrap()
            );
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "game check timed out: {}",
                fs::read_to_string(log_path).unwrap()
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
    let text = fs::read_to_string(log_path).unwrap();
    assert!(!text.contains("[js error]"), "{text}");
    text
}
fn report(root: &Path, file: &str) -> Value {
    let text = fs::read(root.join(file)).unwrap_or_else(|error| panic!("missing {file}: {error}"));
    let result: Value = serde_json::from_slice(&text).unwrap();
    assert_eq!(result["ok"], true, "{result:#}");
    result
}
#[test]
fn neon_breaker_physics_input_audio_and_saved_records() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let app = root.join("game");
    copy_game(&app);
    fs::OpenOptions::new()
        .append(true)
        .open(app.join("app.js"))
        .unwrap()
        .write_all(
            format!(
                "\n{}",
                fs::read_to_string(source().join("checks.js")).unwrap()
            )
            .as_bytes(),
        )
        .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_nile"));
    command
        .arg(app.join("index.html"))
        .arg("--png")
        .arg(root.join("game.png"))
        .args(["--width", "1180", "--height", "900", "--wait", "8000"])
        .env("NEON_TEST_REPORT", root.join("checks.json"));
    run(command, root, "checks");
    let checks = report(root, "checks.json");
    assert!(checks["results"].as_array().unwrap().len() >= 40);
    for result in checks["results"].as_array().unwrap() {
        println!("PASS: {}", result.as_str().unwrap());
    }
    fs::copy(source().join("app.js"), app.join("app.js")).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(app.join("app.js"))
        .unwrap()
        .write_all(
            br#"
import {fs, getenv} from '@sys';
setTimeout(()=>{
 const ok = best === 4321 && preferences.muted && preferences.volume === .3 && !preferences.music;
 fs.writeFileSync(getenv('NEON_TEST_REPORT'),JSON.stringify({ok,best,preferences}));
 stopped = true; cancelAnimationFrame(rafId); sound.stop();
},100);
"#,
        )
        .unwrap();
    let mut restart = Command::new(env!("CARGO_BIN_EXE_nile"));
    restart
        .arg(app.join("index.html"))
        .arg("--png")
        .arg(root.join("restart.png"))
        .args(["--wait", "500"])
        .env("NEON_TEST_REPORT", root.join("restart.json"));
    run(restart, root, "restart");
    report(root, "restart.json");
}
#[test]
fn neon_breaker_packed_assets_include_sounds_and_play_without_source() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let app = root.join("source");
    copy_game(&app);
    fs::OpenOptions::new().append(true).open(app.join("app.js")).unwrap().write_all(br#"
import {fs, getenv} from '@sys';
setTimeout(async()=>{
 try {
  const clip = new Audio('sounds/brick.wav'); await clip.play();
  if (!(clip.duration > .08) || clip.error) throw new Error('Packed sound failed');
  clip.pause(); primary();
  if (game.phase !== 'playing') throw new Error('Packed game failed to start');
  fs.writeFileSync(getenv('NEON_TEST_REPORT'),JSON.stringify({ok:true}));
 } catch(error) {fs.writeFileSync(getenv('NEON_TEST_REPORT'),JSON.stringify({ok:false,error:String(error)}));}
 stopped=true;cancelAnimationFrame(rafId);sound.stop();
},120);
"#).unwrap();
    let bundle = root.join(if cfg!(target_os = "macos") {
        "Neon.app"
    } else if cfg!(windows) {
        "Neon.exe"
    } else {
        "Neon"
    });
    let mut pack = Command::new(env!("CARGO_BIN_EXE_nile"));
    pack.arg("--pack").arg(&app).arg("-o").arg(&bundle);
    run(pack, root, "pack");
    fs::remove_dir_all(app).unwrap();
    let executable = if cfg!(target_os = "macos") {
        bundle.join("Contents/MacOS/Neon")
    } else {
        bundle
    };
    let mut play = Command::new(executable);
    play.arg("--png")
        .arg(root.join("packed.png"))
        .args(["--width", "1180", "--height", "900", "--wait", "1000"])
        .env("NEON_TEST_REPORT", root.join("packed.json"));
    run(play, root, "packed");
    report(root, "packed.json");
}
