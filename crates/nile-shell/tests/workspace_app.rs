#[path = "../../nile-extensions/tests/support/mod.rs"]
mod support;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn launch(app: &Path, root: &Path, label: &str) {
    let log_path = root.join(format!("{label}.log"));
    let log = fs::File::create(&log_path).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_nile"))
        .arg(app.join("index.html"))
        .arg("--png")
        .arg(root.join(format!("{label}.png")))
        .args(["--width", "1280", "--height", "840", "--wait", "60000"])
        .env("NORTHSTAR_DATA_DIR", root.join("data"))
        .env("NILE_STORAGE_DIR", root.join("localstorage"))
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
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
                "workspace check timed out: {}",
                fs::read_to_string(log_path).unwrap()
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
}
fn report(root: &Path, name: &str) -> Value {
    let bytes = fs::read(root.join("data").join(name)).unwrap_or_else(|error| {
        panic!(
            "missing {name}: {error}\n{}\n{}",
            fs::read_to_string(root.join("checks.log")).unwrap_or_default(),
            fs::read_to_string(root.join("restart.log")).unwrap_or_default()
        )
    });
    let report: Value = serde_json::from_slice(&bytes).unwrap();
    if report["ok"] != true {
        // `error:` lines reach CI's failure annotations; the panic message does not.
        let text = format!(
            "{name}: {report:#}\n{}",
            fs::read_to_string(root.join("checks.log")).unwrap_or_default()
        );
        for line in text.lines() {
            eprintln!("error: {line}");
        }
    }
    assert_eq!(report["ok"], true, "{report:#}");
    report
}
#[test]
fn workspace_workflows_and_restart_persist_using_real_sqlite() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let app = root.join("app");
    fs::create_dir(&app).unwrap();
    let source = support::root().join("examples/workspace");
    for name in [
        "index.html",
        "app.js",
        "styles.css",
        "model.js",
        "store.js",
        "documents.js",
        "document.html",
    ] {
        fs::copy(source.join(name), app.join(name)).unwrap();
    }
    let wrapper =
        fs::read_to_string(support::root().join("extensions/sqlite/package/index.js")).unwrap();
    support::package(
        &app,
        "sqlite",
        "@sqlite",
        &support::library("sqlite"),
        &wrapper,
    );
    fs::OpenOptions::new()
        .append(true)
        .open(app.join("app.js"))
        .unwrap()
        .write_all(
            format!(
                "\n{}",
                fs::read_to_string(source.join("checks.js")).unwrap()
            )
            .as_bytes(),
        )
        .unwrap();
    launch(&app, root, "checks");
    let checks = report(root, "checks.json");
    assert!(checks["results"].as_array().unwrap().len() >= 29);
    for result in checks["results"].as_array().unwrap() {
        println!("PASS: {}", result.as_str().unwrap());
    }
    fs::copy(source.join("app.js"), app.join("app.js")).unwrap();
    let restart = r#"
import restartSqlite from '@sqlite';
let restartRunning = false;
const restartTimer = setInterval(async () => {
  if (!ready || restartRunning) return;
  restartRunning = true;
  try {
    const db = await restartSqlite.open(store.dataDir + '/workspace.db');
    const integrity = await db.query('PRAGMA integrity_check'); await db.close();
    if (Object.values(integrity[0])[0] !== 'ok') throw new Error('Database integrity check failed');
    if (state.projects[0].title !== 'Field guide' || state.notes[0].title !== 'Discovery notes') throw new Error('Records did not persist');
    if (state.preferences.theme !== 'dark' || current().kind !== 'note' || store.drafts().length !== 1 || recoveries.length !== 1) throw new Error('Preferences, tabs or recovery draft did not persist');
    fs.writeFileSync(store.dataDir + '/restart.json', JSON.stringify({ok:true}));
  } catch (error) { fs.writeFileSync(store.dataDir + '/restart.json', JSON.stringify({ok:false,error:String(error)})); }
  clearInterval(restartTimer);
}, 20);
"#;
    fs::OpenOptions::new()
        .append(true)
        .open(app.join("app.js"))
        .unwrap()
        .write_all(restart.as_bytes())
        .unwrap();
    launch(&app, root, "restart");
    report(root, "restart.json");
    assert!(fs::read(root.join("data/report.pdf"))
        .unwrap()
        .starts_with(b"%PDF-"));
    let backup: Value =
        serde_json::from_slice(&fs::read(root.join("data/backup.json")).unwrap()).unwrap();
    assert_eq!(backup["format"], json!("northstar-workspace"));
}
