#[path = "../../nile-extensions/tests/support/mod.rs"]
mod support;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
fn success(output: Output) -> Output {
    if !output.status.success() {
        // One `error:` line per output line, so CI's failure annotation shows the cause.
        for line in String::from_utf8_lossy(&output.stdout).lines().chain(String::from_utf8_lossy(&output.stderr).lines()) {
            eprintln!("error: packed test command: {line}");
        }
    }
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("runtime failed to start"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn app(root: &Path, id: &str, version: &str, external: bool) {
    std::fs::create_dir_all(root).unwrap();
    for name in ["sqlite", "notifications"] {
        let wrapper = std::fs::read_to_string(
            support::root().join(format!("extensions/{name}/package/index.js")),
        )
        .unwrap();
        support::package(
            root,
            name,
            &format!("@{name}"),
            &support::library(name),
            &wrapper,
        );
    }
    let location = |name: &str| {
        if external {
            json!({"package":name,"native_root":{"base":"executable","path":format!("external/{name}")}})
        } else {
            json!(name)
        }
    };
    std::fs::write(root.join("nile.json"), json!({"application":{"id":id,"version":version},"extensions":{"@sqlite":location("sqlite"),"@notifications":location("notifications")}}).to_string()).unwrap();
    std::fs::write(root.join("index.html"), r#"<html><body><script type="module">
      import sqlite from '@sqlite'; import notifications from '@notifications'; import sys from '@sys';
      (async () => {
        const db = await sqlite.open('persistent.db', {busyTimeoutMs:5000});
        await db.execute('CREATE TABLE IF NOT EXISTS t (name, n, b)');
        await db.execute('INSERT INTO t VALUES (?,?,?)', ['Hamid 🦀', 9223372036854775807n, new Uint8Array([0,128,255])]);
        const rows = await db.query('SELECT * FROM t');
        if (!rows.length || rows.some(r => r.name !== 'Hamid 🦀' || r.n !== 9223372036854775807n || String(Array.from(r.b)) !== '0,128,255')) throw Error('incorrect SQLite values');
        let events = 0;
        const subscription = notifications.subscribe(event => { if (event.sequence >= 1) events++; });
        await subscription.ready;
        await new Promise(resolve => {
          const timer = setInterval(() => { if (events >= 2) { clearInterval(timer); resolve(); } }, 5);
        });
        subscription.unsubscribe();
        const final = notifications.subscribe(() => {}); await final.ready;
        sys.fs.writeFileSync(sys.getenv('NILE_TEST_REPORT'), JSON.stringify({ok:true, rows:rows.length, events, dataPath:application.dataPath, asset:__FILE__}));
        // Context disposal must stop both an active event source and active SQL VM.
        db.query('WITH RECURSIVE c(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM c WHERE x<1000000000) SELECT sum(x) FROM c').catch(() => {});
      })().catch(error => { sys.fs.writeFileSync(sys.getenv('NILE_TEST_REPORT'), JSON.stringify({error:String(error.stack||error)})); });
    </script></body></html>"#).unwrap();
}
fn pack(root: &Path, output: &Path) {
    success(
        Command::new(env!("CARGO_BIN_EXE_nile"))
            .args(["--pack"])
            .arg(root)
            .arg("-o")
            .arg(output)
            .output()
            .unwrap(),
    );
}
fn launch(exe: &Path, root: &Path, report: &str) -> std::process::Child {
    Command::new(exe)
        .current_dir(root.join("other-cwd"))
        .args(["--png"])
        .arg(root.join(format!("{report}.png")))
        .args(["--wait", "1200"])
        .env("NILE_NATIVE_CACHE_HOME", root.join("cache"))
        .env("NILE_APP_DATA_HOME", root.join("data"))
        .env("NILE_TEST_REPORT", root.join(report))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}
fn report(root: &Path, name: &str) -> Value {
    let value: Value = serde_json::from_slice(
        &std::fs::read(root.join(name))
            .expect("packed script must execute and write exact results"),
    )
    .unwrap();
    assert_eq!(value["ok"], true, "{value}");
    assert!(value["events"].as_u64().unwrap() >= 2);
    assert!(
        value["asset"].as_str().unwrap().starts_with("this://app/"),
        "wrapper/application must execute from the real packed loader: {value}"
    );
    value
}
fn objects(root: &Path) -> Vec<PathBuf> {
    fn walk(p: &Path, out: &mut Vec<PathBuf>) {
        if !p.exists() {
            return;
        }
        for entry in std::fs::read_dir(p).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                if entry.file_name().to_string_lossy().starts_with("object-") {
                    out.push(entry.path());
                } else {
                    walk(&entry.path(), out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}
#[test]
fn produced_executable_runs_packed_modules_persists_data_and_rebuilds_cache_safely() {
    let temp = tempfile::tempdir().unwrap();
    let root = support::canonical(temp.path());
    let source = root.join("source");
    app(&source, "org.nile.packed-test", "1", false);
    std::fs::create_dir(root.join("other-cwd")).unwrap();
    let exe = root.join(if cfg!(windows) {
        "packed.exe"
    } else {
        "packed"
    });
    pack(&source, &exe);
    // Remove original packages: passing tests cannot accidentally use source DLLs or JS.
    std::fs::remove_dir_all(&source).unwrap();
    // Simultaneous cold launches must converge on one immutable generation per package.
    let first = launch(&exe, &root, "first.json");
    let second = launch(&exe, &root, "second.json");
    success(first.wait_with_output().unwrap());
    success(second.wait_with_output().unwrap());
    let a = report(&root, "first.json");
    let b = report(&root, "second.json");
    assert_eq!(a["dataPath"], b["dataPath"]);
    let data = root.join("data/nile/apps/org.nile.packed-test");
    assert!(data.join("persistent.db").is_file());
    let before = objects(&root.join("cache"));
    assert_eq!(before.len(), 2);
    for generation in &before {
        assert!(!generation.join("index.js").exists());
        assert!(!generation.join("extension.json").exists());
    }
    success(launch(&exe, &root, "warm.json").wait_with_output().unwrap());
    assert_eq!(report(&root, "warm.json")["rows"], 3);
    assert_eq!(before, objects(&root.join("cache")));
    let library = before[0].join(format!("native{}", std::env::consts::DLL_SUFFIX));
    std::fs::write(&library, b"corrupt cached library").unwrap();
    std::fs::create_dir(before[0].parent().unwrap().join(".stage-interrupted")).unwrap();
    success(
        launch(&exe, &root, "recovered.json")
            .wait_with_output()
            .unwrap(),
    );
    assert_eq!(report(&root, "recovered.json")["rows"], 4);
    assert_eq!(objects(&root.join("cache")).len(), 3);
    assert_eq!(std::fs::read(library).unwrap(), b"corrupt cached library");
    // A new application version coexists without losing stable writable data.
    app(&source, "org.nile.packed-test", "2", false);
    let version2 = root.join(if cfg!(windows) {
        "packed-v2.exe"
    } else {
        "packed-v2"
    });
    pack(&source, &version2);
    std::fs::remove_dir_all(&source).unwrap();
    success(
        launch(&version2, &root, "version2.json")
            .wait_with_output()
            .unwrap(),
    );
    assert_eq!(report(&root, "version2.json")["rows"], 5);
    assert_eq!(objects(&root.join("cache")).len(), 5);
}
#[test]
fn explicit_external_mode_loads_verified_native_files_beside_packed_executable() {
    let temp = tempfile::tempdir().unwrap();
    let root = support::canonical(temp.path());
    let source = root.join("source");
    app(&source, "org.nile.external-test", "1", true);
    std::fs::create_dir(root.join("other-cwd")).unwrap();
    let exe = root.join(if cfg!(windows) {
        "external-app.exe"
    } else {
        "external-app"
    });
    pack(&source, &exe);
    for name in ["sqlite", "notifications"] {
        let external = root.join("external").join(name);
        std::fs::create_dir_all(&external).unwrap();
        std::fs::copy(
            source
                .join(name)
                .join(format!("native{}", std::env::consts::DLL_SUFFIX)),
            external.join(format!("native{}", std::env::consts::DLL_SUFFIX)),
        )
        .unwrap();
    }
    std::fs::remove_dir_all(&source).unwrap();
    success(
        launch(&exe, &root, "external.json")
            .wait_with_output()
            .unwrap(),
    );
    assert_eq!(report(&root, "external.json")["rows"], 1);
    assert!(objects(&root.join("cache")).is_empty());
}

fn companion_package(root: &Path) {
    let native = root.join("native");
    std::fs::create_dir_all(&native).unwrap();
    let helper_name = format!(
        "{}helper{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    let main_name = format!(
        "{}main{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    let helper = native.join(&helper_name);
    let main = native.join(&main_name);
    std::fs::write(
        root.join("helper.c"),
        "#ifdef _WIN32\n__declspec(dllexport)\n#endif\nint nile_helper(void) { return 42; }\n",
    )
    .unwrap();
    std::fs::write(root.join("main.c"), r#"
#include "nile_extension.h"
#include <stdio.h>
#include <string.h>
#ifdef _WIN32
#define EXPORT __declspec(dllexport)
__declspec(dllimport) int nile_helper(void);
#else
#define EXPORT __attribute__((visibility("default")))
extern int nile_helper(void);
#endif
static NileExtensionApi api;
static NileSlice str(const char *s) { NileSlice r={(const uint8_t*)s,strlen(s)}; return r; }
static NileStatus dispatch(uint64_t user, const NileRequest *request) {
  (void)user; char json[64]; snprintf(json,sizeof(json),"{\"answer\":%d}",nile_helper());
  NileReply reply={0}; reply.struct_size=sizeof(reply); reply.json=str(json);
  return api.complete(api.host, request->context, request->call, &reply);
}
static NileStatus dispose(uint64_t user,uint64_t context) { (void)user; (void)context; return NILE_OK; }
static NileStatus shutdown_extension(uint64_t user) { (void)user; return NILE_OK; }
EXPORT NileStatus nile_extension_init(const NileExtensionApi *host,NileExtension *extension) {
  api=*host; extension->abi_version=NILE_ABI_VERSION; extension->struct_size=sizeof(*extension);
  extension->context_dispose=dispose; extension->shutdown=shutdown_extension;
  return api.register_operation(api.host,str("companion.answer"),dispatch,0);
}
"#).unwrap();
    let output = Command::new("rustc").arg("-vV").output().unwrap();
    let info = String::from_utf8(output.stdout).unwrap();
    let target = info.lines().find_map(|s| s.strip_prefix("host: ")).unwrap();
    let compiler = cc::Build::new()
        .target(target)
        .host(target)
        .opt_level(0)
        .get_compiler();
    let mut helper_cmd = compiler.to_command();
    let mut main_cmd = compiler.to_command();
    if compiler.is_like_msvc() {
        helper_cmd
            .arg("/LD")
            .arg(root.join("helper.c"))
            .arg("/link")
            .arg(format!("/OUT:{}", helper.display()))
            .arg(format!("/IMPLIB:{}", native.join("helper.lib").display()));
        main_cmd
            .arg("/LD")
            .arg(format!(
                "/I{}",
                support::root()
                    .join("crates/nile-extension-sdk/include")
                    .display()
            ))
            .arg(root.join("main.c"))
            .arg(native.join("helper.lib"))
            .arg("/link")
            .arg(format!("/OUT:{}", main.display()));
    } else {
        let shared = if cfg!(target_os = "macos") {
            "-dynamiclib"
        } else {
            "-shared"
        };
        helper_cmd
            .args([shared, "-fPIC"])
            .arg(root.join("helper.c"))
            .arg("-o")
            .arg(&helper);
        if cfg!(target_os = "macos") {
            helper_cmd.arg(format!("-Wl,-install_name,@rpath/{helper_name}"));
        }
        main_cmd
            .args([shared, "-fPIC"])
            .arg("-I")
            .arg(support::root().join("crates/nile-extension-sdk/include"))
            .arg(root.join("main.c"))
            .arg("-L")
            .arg(&native)
            .arg("-lhelper")
            .arg(if cfg!(target_os = "macos") {
                "-Wl,-rpath,@loader_path"
            } else {
                "-Wl,-rpath,$ORIGIN"
            })
            .arg("-o")
            .arg(&main);
    }
    success(helper_cmd.current_dir(root).output().unwrap());
    success(main_cmd.current_dir(root).output().unwrap());
    std::fs::write(root.join("extension.json"), json!({"id":"companion","version":"1","abi":1,"entry":"index.js","native":{nile_extensions::platform().unwrap():{"library":format!("native/{main_name}"),"files":[format!("native/{helper_name}")]}}}).to_string()).unwrap();
    std::fs::write(
        root.join("index.js"),
        "import native from '@native'; export default () => native.call('companion.answer');",
    )
    .unwrap();
}
#[test]
fn packed_companion_dependency_is_loaded_from_preserved_native_layout() {
    let temp = tempfile::tempdir().unwrap();
    let root = support::canonical(temp.path());
    let source = root.join("source");
    companion_package(&source.join("extension"));
    std::fs::write(source.join("nile.json"), json!({"application":{"id":"org.nile.companion-test","version":"1"},"extensions":{"@companion":"extension"}}).to_string()).unwrap();
    std::fs::write(source.join("index.html"), r#"<html><body><script type="module">
      import answer from '@companion'; import sys from '@sys';
      answer().then(packet => { sys.fs.writeFileSync(sys.getenv('NILE_TEST_REPORT'),JSON.stringify(packet.value)); });
    </script></body></html>"#).unwrap();
    let exe = root.join(if cfg!(windows) {
        "companion-app.exe"
    } else {
        "companion-app"
    });
    pack(&source, &exe);
    std::fs::remove_dir_all(&source).unwrap();
    std::fs::create_dir(root.join("other-cwd")).unwrap();
    success(
        launch(&exe, &root, "companion.json")
            .wait_with_output()
            .unwrap(),
    );
    let result: Value =
        serde_json::from_slice(&std::fs::read(root.join("companion.json")).unwrap()).unwrap();
    assert_eq!(result, json!({"answer":42}));
    let objects = objects(&root.join("cache"));
    assert_eq!(objects.len(), 1);
    assert!(objects[0]
        .join(format!(
            "native/{}helper{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ))
        .is_file());
}

#[test]
fn missing_native_dependency_fails_before_writing_executable() {
    let temp = tempfile::tempdir().unwrap();
    let root = support::canonical(temp.path());
    let source = root.join("source");
    companion_package(&source.join("extension"));
    std::fs::write(source.join("nile.json"), json!({"application":{"id":"org.nile.missing-test","version":"1"},"extensions":{"@companion":"extension"}}).to_string()).unwrap();
    std::fs::write(source.join("index.html"), "<html></html>").unwrap();
    std::fs::remove_file(source.join(format!(
        "extension/native/{}helper{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )))
    .unwrap();
    let exe = root.join("must-not-exist");
    let output = Command::new(env!("CARGO_BIN_EXE_nile"))
        .arg("--pack")
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!exe.exists());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("@companion") && stderr.contains("native-file"),
        "{stderr}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn explicit_bundle_resources_mode_keeps_native_files_in_application_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let root = support::canonical(temp.path());
    let source = root.join("source");
    app(&source, "org.nile.bundle-test", "1", true);
    let config = source.join("nile.json");
    let mut value: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    for name in ["sqlite", "notifications"] {
        value["extensions"][format!("@{name}")]["native_root"]["base"] = json!("bundle-resources");
    }
    std::fs::write(&config, value.to_string()).unwrap();
    let bundle = root.join("NativeExample.app");
    pack(&source, &bundle);
    for name in ["sqlite", "notifications"] {
        let external = bundle.join("Contents/Resources/external").join(name);
        std::fs::create_dir_all(&external).unwrap();
        std::fs::copy(
            source
                .join(name)
                .join(format!("native{}", std::env::consts::DLL_SUFFIX)),
            external.join(format!("native{}", std::env::consts::DLL_SUFFIX)),
        )
        .unwrap();
    }
    // These native files are copied byte-for-byte; a production distributor must
    // sign/notarize the completed bundle after adding all native resources.
    std::fs::remove_dir_all(&source).unwrap();
    std::fs::create_dir(root.join("other-cwd")).unwrap();
    success(
        launch(
            &bundle.join("Contents/MacOS/NativeExample"),
            &root,
            "bundle.json",
        )
        .wait_with_output()
        .unwrap(),
    );
    assert_eq!(report(&root, "bundle.json")["rows"], 1);
    assert!(objects(&root.join("cache")).is_empty());
}
