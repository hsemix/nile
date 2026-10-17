mod support;
use nile_extension_sdk::*;
use nile_extensions::{Delivery, Extensions};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
fn wait(host: &Extensions, call: u64) -> Delivery {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(d) = host
            .drain()
            .into_iter()
            .find(|d| d.call == call && !d.event)
        {
            return d;
        }
        assert!(Instant::now() < deadline, "native completion timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn call(host: &Extensions, op: &str, args: serde_json::Value) -> Delivery {
    let id = host
        .start(host.packages[0].host, op, &args.to_string(), &[], false)
        .unwrap();
    wait(host, id)
}
#[test]
fn c_abi_immediate_duplicate_late_resources_and_dispatch_errors() {
    let temp = tempfile::tempdir().unwrap();
    let library = support::fixture(temp.path(), None);
    let config = support::package(
        temp.path(),
        "fixture",
        "@fixture",
        &library,
        "export default {};",
    );
    let host = Extensions::load(&config).unwrap();
    let native = unsafe {
        libloading::Library::new(
            temp.path()
                .join("fixture")
                .join(format!("native{}", std::env::consts::DLL_SUFFIX)),
        )
        .unwrap()
    };
    let flood = host
        .start(host.packages[0].host, "fixture.flood", "{}", &[], true)
        .unwrap();
    let deliveries = host.drain();
    assert_eq!(deliveries.iter().filter(|d| d.event).count(), 128);
    assert_eq!(deliveries.iter().find(|d| !d.event).unwrap().json, "128");
    host.cancel(host.packages[0].host, flood);
    let d = call(&host, "fixture.immediate", json!({}));
    assert_eq!(d.json, "{\"answer\":42}");
    unsafe {
        assert_eq!(
            native
                .get::<unsafe extern "C" fn() -> i32>(b"fixture_duplicate_status\0")
                .unwrap()(),
            NILE_DUPLICATE
        );
    }
    assert_eq!(call(&host, "fixture.error", json!({})).code, "DISPATCH_5");
    let resource = call(&host, "fixture.resource", json!({}));
    let id: String = serde_json::from_str(&resource.json).unwrap();
    assert_eq!(call(&host, "fixture.get", json!(id)).json, "0");
    assert_eq!(call(&host, "fixture.get", json!("999999999")).json, "1");
    assert_eq!(call(&host, "fixture.release", json!(id)).json, "0");
    assert_eq!(call(&host, "fixture.get", json!(id)).json, "1");
    assert_eq!(call(&host, "fixture.release", json!(id)).json, "1");
    // A second extension instance may not access another context's resource.
    let other_dir = tempfile::tempdir().unwrap();
    let other_config = support::package(
        other_dir.path(),
        "fixture",
        "@fixture",
        &library,
        "export default {};",
    );
    let other = Extensions::load(&other_config).unwrap();
    let resource = call(&host, "fixture.resource", json!({}));
    let id: String = serde_json::from_str(&resource.json).unwrap();
    assert_eq!(call(&other, "fixture.get", json!(id)).json, "1");
    drop(other);
    let token = host.packages[0].host;
    host.start(token, "fixture.pending", "{}", &[], false)
        .unwrap();
    drop(host);
    unsafe {
        assert_eq!(
            native
                .get::<unsafe extern "C" fn() -> i32>(b"fixture_late_status\0")
                .unwrap()(),
            NILE_CLOSED
        );
        assert_eq!(
            native
                .get::<unsafe extern "C" fn() -> i32>(b"fixture_complete_late\0")
                .unwrap()(),
            NILE_CLOSED
        );
        assert_eq!(
            native
                .get::<unsafe extern "C" fn() -> u32>(b"fixture_destroyed\0")
                .unwrap()(),
            2
        );
    }
}
#[test]
fn manifests_symbols_abi_and_partial_initialization_fail_clearly() {
    let temp = tempfile::tempdir().unwrap();
    let library = support::fixture(temp.path(), Some("NO_ENTRY"));
    let config = support::package(temp.path(), "fixture", "@fixture", &library, "");
    assert!(Extensions::load(&config)
        .err()
        .unwrap()
        .contains("missing nile_extension_init"));
    let library = support::fixture(temp.path(), Some("BAD_TABLE"));
    std::fs::copy(
        library,
        temp.path()
            .join("fixture")
            .join(format!("native{}", std::env::consts::DLL_SUFFIX)),
    )
    .unwrap();
    assert!(Extensions::load(&config)
        .err()
        .unwrap()
        .contains("incompatible ABI"));
    let manifest = temp.path().join("fixture/extension.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    value["abi"] = json!(99);
    std::fs::write(&manifest, value.to_string()).unwrap();
    assert!(Extensions::load(&config)
        .err()
        .unwrap()
        .contains("unsupported ABI 99"));
    value["abi"] = json!(1);
    value["native"] = json!({});
    std::fs::write(&manifest, value.to_string()).unwrap();
    assert!(Extensions::load(&config)
        .err()
        .unwrap()
        .contains("no native library"));
    value["native"] = json!({nile_extensions::platform().unwrap():"missing.so"});
    std::fs::write(&manifest, value.to_string()).unwrap();
    assert!(Extensions::load(&config)
        .err()
        .unwrap()
        .contains("missing.so"));
    std::fs::write(&manifest, "{broken").unwrap();
    assert!(Extensions::load(&config)
        .err()
        .unwrap()
        .contains("extension.json"));
    std::fs::write(&config, "{\"extensions\":{\"@sys\":\"./fixture\"}}").unwrap();
    assert!(Extensions::load(&config)
        .err()
        .unwrap()
        .contains("reserved"));
}
#[test]
fn notifications_wake_unsubscribe_overflow_and_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let config = support::package(
        temp.path(),
        "notifications",
        "@notifications",
        &support::library("notifications"),
        "",
    );
    let host = Extensions::load(&config).unwrap();
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = wakes.clone();
    host.set_waker(Some(Arc::new(move || {
        count.fetch_add(1, Ordering::Relaxed);
    })));
    let token = host.packages[0].host;
    let id = host
        .start(token, "notifications.watch", "{}", &[], true)
        .unwrap();
    assert_eq!(wait(&host, id).status, NILE_OK);
    std::thread::sleep(Duration::from_millis(50));
    assert!(host
        .drain()
        .iter()
        .any(|d| d.event && d.call == id && d.json.contains("sequence")));
    assert!(wakes.load(Ordering::Relaxed) >= 2);
    std::thread::sleep(Duration::from_millis(40));
    let events = host.drain();
    assert!(!events.is_empty() && events.len() <= 128);
    host.cancel(token, id);
    std::thread::sleep(Duration::from_millis(30));
    assert!(host.drain().is_empty());
    assert!(!host.pending());
    host.start(token, "notifications.watch", "{}", &[], true)
        .unwrap();
    let now = Instant::now();
    drop(host);
    // Include native-library teardown and scheduling on loaded CI hosts.
    assert!(now.elapsed() < Duration::from_secs(8));
}
#[test]
fn sqlite_dynamic_boundary_bound_values_handles_and_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let config = support::package(
        temp.path(),
        "sqlite",
        "@sqlite",
        &support::library("sqlite"),
        "",
    );
    let host = Extensions::load(&config).unwrap();
    if cfg!(windows) {
        assert_eq!(
            call(&host, "sqlite.open", json!({"path":"C:relative.db"})).code,
            "SQLITE_PATH"
        );
    }
    let open = call(&host, "sqlite.open", json!({"path":"roundtrip.db"}));
    assert_eq!(open.status, NILE_OK, "{}", open.message);
    assert!(temp.path().join("roundtrip.db").is_file());
    let handle = serde_json::from_str::<serde_json::Value>(&open.json).unwrap()["handle"].clone();
    let execute = |sql: &str, params: serde_json::Value| {
        call(
            &host,
            "sqlite.execute",
            json!({"handle":handle,"sql":sql,"params":params}),
        )
    };
    assert_eq!(
        execute("CREATE TABLE t (n, text, big, blob)", json!([])).status,
        NILE_OK
    );
    let id = host.start(host.packages[0].host, "sqlite.execute", &json!({"handle":handle,"sql":"INSERT INTO t VALUES (?, ?, ?, ?)","params":[null,"Hamid 🦀",{"$i64":"9223372036854775807"},{"$blob":0}]}).to_string(), &[vec![0,1,128,255]], false).unwrap();
    assert_eq!(wait(&host, id).status, NILE_OK);
    let d = call(
        &host,
        "sqlite.query",
        json!({"handle":handle,"sql":"SELECT * FROM t WHERE text = ?","params":["Hamid 🦀"]}),
    );
    assert_eq!(d.status, NILE_OK, "{}", d.message);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&d.json).unwrap(),
        json!([{"n":null,"text":"Hamid 🦀","big":{"$i64":"9223372036854775807"},"blob":{"$blob":0}}])
    );
    assert_eq!(d.buffers, [vec![0, 1, 128, 255]]);
    assert_eq!(
        execute("INSERT INTO t VALUES (1,2,3,4); DELETE FROM t", json!([])).code,
        "SQLITE_MULTIPLE_STATEMENTS"
    );
    assert!(execute("INVALID SQL", json!([]))
        .code
        .starts_with("SQLITE_"));
    assert_eq!(
        call(
            &host,
            "sqlite.query",
            json!({"handle":handle,"sql":"SELECT 1 AS x, 2 AS x","params":[]})
        )
        .code,
        "SQLITE_DUPLICATE_COLUMN"
    );
    assert_eq!(
        call(&host, "sqlite.close", json!({"handle":handle})).status,
        NILE_OK
    );
    assert_eq!(
        call(&host, "sqlite.close", json!({"handle":handle})).code,
        "SQLITE_CLOSED"
    );
    assert_eq!(execute("SELECT 1", json!([])).code, "SQLITE_CLOSED");
    let open = call(&host, "sqlite.open", json!({"path":":memory:"}));
    let handle = serde_json::from_str::<serde_json::Value>(&open.json).unwrap()["handle"].clone();
    host.start(host.packages[0].host, "sqlite.query", &json!({"handle":handle,"sql":"WITH RECURSIVE c(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM c WHERE x<1000000000) SELECT sum(x) FROM c","params":[]}).to_string(), &[], false).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    let now = Instant::now();
    drop(host);
    assert!(
        now.elapsed() < Duration::from_secs(3),
        "shutdown must interrupt active SQLite VM work"
    );
}
#[test]
fn pending_call_overload_is_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let library = support::fixture(temp.path(), None);
    let config = support::package(temp.path(), "fixture", "@fixture", &library, "");
    let host = Extensions::load(&config).unwrap();
    let token = host.packages[0].host;
    let mut calls = Vec::new();
    for _ in 0..128 {
        calls.push(
            host.start(token, "fixture.pending", "{}", &[], false)
                .unwrap(),
        );
    }
    assert!(host
        .start(token, "fixture.pending", "{}", &[], false)
        .unwrap_err()
        .contains("OVERLOADED"));
    host.cancel(token, calls[0]);
    assert!(host
        .start(token, "fixture.pending", "{}", &[], false)
        .is_ok());
}

#[test]
fn partial_context_initialization_releases_resources_and_instance() {
    let temp = tempfile::tempdir().unwrap();
    let library = support::fixture(temp.path(), Some("FAIL_CONTEXT"));
    let config = support::package(temp.path(), "fixture", "@fixture", &library, "");
    let path = temp
        .path()
        .join("fixture")
        .join(format!("native{}", std::env::consts::DLL_SUFFIX));
    let native = unsafe { libloading::Library::new(path).unwrap() };
    assert!(Extensions::load(&config)
        .err()
        .unwrap()
        .contains("context initialization failed"));
    unsafe {
        assert_eq!(
            native
                .get::<unsafe extern "C" fn() -> u32>(b"fixture_destroyed\0")
                .unwrap()(),
            1
        );
        assert_eq!(
            native
                .get::<unsafe extern "C" fn() -> u32>(b"fixture_shutdown_count\0")
                .unwrap()(),
            1
        );
    }
}

#[test]
fn sqlite_oversized_results_reject_and_leave_connection_usable() {
    let temp = tempfile::tempdir().unwrap();
    let config = support::package(
        temp.path(),
        "sqlite",
        "@sqlite",
        &support::library("sqlite"),
        "",
    );
    let host = Extensions::load(&config).unwrap();
    let opened = call(&host, "sqlite.open", json!({"path":":memory:"}));
    assert_eq!(opened.status, NILE_OK);
    let handle = serde_json::from_str::<serde_json::Value>(&opened.json).unwrap()["handle"].clone();
    let query = |sql: &str| {
        call(
            &host,
            "sqlite.query",
            json!({"handle":handle,"sql":sql,"params":[]}),
        )
    };
    let execute = |sql: &str| {
        call(
            &host,
            "sqlite.execute",
            json!({"handle":handle,"sql":sql,"params":[]}),
        )
    };
    assert_eq!(
        execute("CREATE TABLE large_values (text_value, blob_value)").status,
        NILE_OK
    );
    assert_eq!(
        execute("INSERT INTO large_values VALUES (printf('%.*c', 950000, 'x'), zeroblob(950000))")
            .status,
        NILE_OK
    );
    // Existing large values, insufficient remaining budget, and serialized escaping.
    for sql in [
        "SELECT text_value FROM large_values",
        "SELECT blob_value FROM large_values",
        "SELECT hex(zeroblob(225000)) AS a, hex(zeroblob(225000)) AS b",
        "SELECT replace(hex(zeroblob(100000)), '0', char(1)) AS escaped",
        "SELECT printf('%.*c', 899991, 'x') AS t",
        "SELECT zeroblob(899982) AS t",
        // SQLite's execution value-length limit, not just the response budget.
        "SELECT zeroblob(4194305) AS computed",
        "SELECT hex(zeroblob(3000000)) AS computed",
    ] {
        let reply = query(sql);
        assert_eq!(reply.status, NILE_FAILED, "query should reject: {sql}");
        assert_eq!(
            reply.code, "SQLITE_RESULT_LIMIT",
            "{sql}: {}",
            reply.message
        );
        assert!(
            reply.buffers.is_empty(),
            "failed result must not return partial buffers"
        );
        let usable = query("SELECT 42 AS answer");
        assert_eq!(usable.status, NILE_OK, "connection unusable after: {sql}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&usable.json).unwrap(),
            json!([{"answer":42}])
        );
    }
    // Exact serialized boundary: account for quotes, array/object punctuation,
    // column names and binary tags as well as the bytes in the buffer itself.
    let text = query("SELECT printf('%.*c', 899990, 'x') AS t");
    assert_eq!(text.status, NILE_OK, "{}", text.message);
    assert_eq!(text.json.len(), 900_000);
    let tag_bytes = json!([{"t":{"$blob":0}}]).to_string().len();
    let blob = query(&format!("SELECT zeroblob({}) AS t", 900_000 - tag_bytes));
    assert_eq!(blob.status, NILE_OK, "{}", blob.message);
    assert_eq!(blob.json.len() + blob.buffers[0].len(), 900_000);
    assert_eq!(
        call(&host, "sqlite.close", json!({"handle":handle})).status,
        NILE_OK
    );
}

#[test]
fn an_oversized_reply_rejects_the_call_even_if_the_extension_ignores_the_status() {
    let temp = tempfile::tempdir().unwrap();
    let library = support::fixture(temp.path(), None);
    let config = support::package(temp.path(), "fixture", "@fixture", &library, "");
    let host = Extensions::load(&config).unwrap();
    let d = call(&host, "fixture.big", json!({}));
    assert_eq!(d.status, NILE_FAILED);
    assert_eq!(d.code, "RESULT_LIMIT");
    assert!(!host.pending(), "the call is settled, not left pending");
}

#[test]
fn a_failed_extension_init_calls_no_hooks() {
    let temp = tempfile::tempdir().unwrap();
    let library = support::fixture(temp.path(), Some("FAIL_INIT"));
    let config = support::package(temp.path(), "fixture", "@fixture", &library, "");
    let path = temp.path().join("fixture").join(format!("native{}", std::env::consts::DLL_SUFFIX));
    let native = unsafe { libloading::Library::new(path).unwrap() };
    assert!(Extensions::load(&config).err().unwrap().contains("initialization failed"));
    unsafe {
        let shutdowns = native.get::<unsafe extern "C" fn() -> u32>(b"fixture_shutdown_count\0").unwrap()();
        assert_eq!(shutdowns, 0, "shutdown must not run for an extension whose init failed");
    }
}
