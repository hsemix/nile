#[path = "../../nile-extensions/tests/support/mod.rs"]
mod support;
use nile_engine::Page;
use std::time::{Duration, Instant};
#[test]
fn page_initializes_extensions_before_modules_and_ticks_deliver_results() {
    let temp = tempfile::tempdir().unwrap();
    let wrapper =
        std::fs::read_to_string(support::root().join("extensions/sqlite/package/index.js"))
            .unwrap();
    support::package(
        temp.path(),
        "sqlite",
        "@sqlite",
        &support::library("sqlite"),
        &wrapper,
    );
    // Nested page exercises ancestor config discovery and base path resolution.
    let folder = temp.path().join("pages");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("index.html");
    std::fs::write(
        &file,
        r#"<html><body><script type="module">
      import sqlite from '@sqlite';
      globalThis.done = false;
      (async () => {
        const db = await sqlite.open('page.db');
        const rows = await db.query('SELECT ? AS name', ['Hamid']);
        globalThis.name = rows[0].name;
        await db.close();
        globalThis.done = true;
      })().catch(error => { globalThis.failure = String(error); });
    </script></body></html>"#,
    )
    .unwrap();
    let mut page = Page::load(file, (400.0, 200.0), 0.0);
    assert!(page.scripts.is_some());
    let deadline = Instant::now() + Duration::from_secs(5);
    while page.eval("globalThis.done").0.unwrap() != "true" {
        page.tick();
        assert_eq!(page.eval("globalThis.failure || ''").0.unwrap(), "");
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(page.eval("globalThis.name").0.unwrap(), "Hamid");
    assert!(temp.path().join("page.db").exists());
    assert!(!folder.join("page.db").exists());
}
