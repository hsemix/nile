#[path = "../../nile-extensions/tests/support/mod.rs"]
mod support;
use nile_dom::{Document, NodeId};
use nile_script::{Environment, ScriptHost};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};
struct Env;
impl Environment for Env {
    fn scroll_y(&self) -> f32 {
        0.0
    }
    fn scroll_to(&self, _: f32) {}
    fn viewport(&self) -> (f32, f32) {
        (800.0, 600.0)
    }
    fn bounding_rect(&self, _: NodeId) -> Option<[f32; 4]> {
        None
    }
    fn computed_style(&self, _: NodeId, _: &str) -> Option<String> {
        None
    }
    fn interaction(&self) -> nile_style::InteractionState {
        nile_style::InteractionState::default()
    }
    fn set_focus(&self, _: Option<NodeId>) {}
    fn element_scroll(&self, _: NodeId) -> Option<[f32; 4]> {
        None
    }
    fn scroll_element_to(&self, _: NodeId, _: f32, _: f32) {}
    fn selection(&self, _: NodeId) -> (usize, usize) {
        (0, 0)
    }
    fn set_selection(&self, _: NodeId, _: usize, _: usize) {}
}
fn host() -> ScriptHost {
    ScriptHost::new(
        Rc::new(RefCell::new(Document::parse("<html><body></body></html>"))),
        Rc::new(Env),
    )
    .unwrap()
}
fn until(host: &ScriptHost, expression: &str) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        host.tick();
        let failed = host.eval("globalThis.failure || ''").unwrap();
        assert!(failed.is_empty(), "JS failure: {failed}");
        if host.eval(expression).unwrap() == "true" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timed out: {expression}; logs: {:?}",
            host.console_output()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn sqlite_import_public_wrapper_exact_results_and_ordering() {
    let temp = tempfile::tempdir().unwrap();
    let source =
        std::fs::read_to_string(support::root().join("extensions/sqlite/package/index.js"))
            .unwrap();
    let config = support::package(
        temp.path(),
        "sqlite",
        "@sqlite",
        &support::library("sqlite"),
        &source,
    );
    let host = host();
    host.configure_extensions(&config).unwrap();
    let outside = temp
        .path()
        .join("forbidden.js")
        .to_string_lossy()
        .into_owned();
    assert!(host
        .run_module(&outside, "import native from '@native';")
        .is_err());
    let name = temp.path().join("app.js").to_string_lossy().into_owned();
    host.run_module(&name, r#"
        import sqlite from '@sqlite';
        globalThis.failure = '';
        globalThis.done = false;
        (async () => {
          const db = await sqlite.open('customers.db');
          await db.execute('CREATE TABLE customers (id INTEGER PRIMARY KEY, name, n, big, data)');
          const bytes = new Uint8Array([0, 128, 255]);
          await db.execute('BEGIN');
          const first = db.execute('INSERT INTO customers (name,n,big,data) VALUES (?,?,?,?)', ['Hamid 🦀',null,9223372036854775807n,bytes]);
          const second = db.execute('INSERT INTO customers (name,n,big,data) VALUES (?,?,?,?)', ['Ada',null,-9223372036854775808n,new Uint8Array([])]);
          await Promise.all([first, second]);
          await db.execute('COMMIT');
          const rows = await db.query('SELECT * FROM customers ORDER BY id');
          if (rows.length !== 2 || rows[0].id !== 1 || rows[0].name !== 'Hamid 🦀' || rows[0].n !== null || rows[0].big !== 9223372036854775807n) throw Error('row values mismatch');
          if (!(rows[0].data instanceof Uint8Array) || String(Array.from(rows[0].data)) !== '0,128,255') throw Error('blob mismatch');
          if (rows[1].big !== -9223372036854775808n || rows[1].data.length !== 0) throw Error('second row mismatch');
          const names = await db.query('SELECT 1 AS "$blob", 2 AS "$i64", 3 AS "__proto__"');
          if (names[0].$blob !== 1 || names[0].$i64 !== 2 || names[0].__proto__ !== 3) throw Error('column name collision');
          let code;
          try { await db.query('SELECT missing FROM customers'); } catch (e) { code = e.code; }
          if (!code || !code.startsWith('SQLITE_')) throw Error('missing structured error');
          const pending = db.query('SELECT name FROM customers WHERE id=?', [1]);
          const closed = db.close();
          if ((await pending)[0].name !== 'Hamid 🦀') throw Error('close overtook query');
          await closed;
          await db.close();
          try { await db.query('SELECT 1'); throw Error('query after close succeeded'); }
          catch (e) { if (e.code !== 'SQLITE_CLOSED') throw e; }
          globalThis.done = true;
        })().catch(e => { globalThis.failure = String(e.stack || e); });
    "#).unwrap();
    until(&host, "globalThis.done");
    assert!(temp.path().join("customers.db").is_file());
    assert!(!host.fetches_pending());
}
#[test]
fn events_unsubscribe_timer_delivery_and_runtime_disposal() {
    let temp = tempfile::tempdir().unwrap();
    let source =
        std::fs::read_to_string(support::root().join("extensions/notifications/package/index.js"))
            .unwrap();
    let config = support::package(
        temp.path(),
        "notifications",
        "@notifications",
        &support::library("notifications"),
        &source,
    );
    let host = host();
    host.configure_extensions(&config).unwrap();
    host.run_module(
        &temp.path().join("app.js").to_string_lossy(),
        r#"
      import notifications from '@notifications';
      globalThis.count = 0; globalThis.timer = false;
      globalThis.subscription = notifications.subscribe(event => {
        if (event.sequence < 1) throw Error('invalid event');
        globalThis.count++;
      });
      setTimeout(() => { globalThis.timer = true; }, 1);
      subscription.ready.catch(e => { globalThis.failure = String(e); });
    "#,
    )
    .unwrap();
    until(&host, "globalThis.count >= 2 && globalThis.timer");
    host.run("subscription.unsubscribe(); globalThis.before = count;")
        .unwrap();
    std::thread::sleep(Duration::from_millis(40));
    host.tick();
    assert_eq!(host.eval("count === before").unwrap(), "true");
    assert!(!host.fetches_pending());
    host.run("globalThis.active = null; import('@notifications').then(m => { globalThis.active = m.default.subscribe(() => {}); active.ready.catch(() => {}); });").unwrap();
    until(&host, "globalThis.active !== null");
    let start = Instant::now();
    drop(host);
    assert!(start.elapsed() < Duration::from_secs(2));
    // A new runtime has fresh IDs and no old subscription deliveries.
    let new = self::host();
    new.configure_extensions(&config).unwrap();
    new.tick();
    assert!(!new.fetches_pending());
}
#[test]
fn ui_timer_remains_live_during_long_sql_and_cancellation() {
    let temp = tempfile::tempdir().unwrap();
    let source =
        std::fs::read_to_string(support::root().join("extensions/sqlite/package/index.js"))
            .unwrap();
    let config = support::package(
        temp.path(),
        "sqlite",
        "@sqlite",
        &support::library("sqlite"),
        &source,
    );
    let host = host();
    host.configure_extensions(&config).unwrap();
    host.run_module(&temp.path().join("app.js").to_string_lossy(), r#"
      import sqlite from '@sqlite';
      globalThis.started = false; globalThis.timer = false; globalThis.cancelled = false;
      sqlite.open(':memory:').then(db => {
        const work = db.query('WITH RECURSIVE c(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM c WHERE x<1000000000) SELECT sum(x) FROM c');
        globalThis.started = true;
        setTimeout(() => { globalThis.timer = true; work.cancel(); }, 2);
        work.catch(e => { if (e.code === 'CANCELLED') globalThis.cancelled = true; else globalThis.failure = String(e); });
      }).catch(e => { globalThis.failure = String(e); });
    "#).unwrap();
    until(
        &host,
        "globalThis.started && globalThis.timer && globalThis.cancelled",
    );
    let start = Instant::now();
    drop(host);
    assert!(start.elapsed() < Duration::from_secs(3));
}
