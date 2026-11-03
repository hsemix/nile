//! Unit tests for the crate root; split out of lib.rs, which they used to follow.
use super::*;

fn s(x: &str) -> CString {
    CString::new(x).unwrap()
}

unsafe fn take(p: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { nile_string_free(p) };
    out
}

const PAGE: &str = r#"<html window-title="Demo" window-frame="solid" window-width="320"><body style="margin:0">
    <button id="b" style="width:100px;height:30px;margin:0" role="window-close">x</button>
    <input id="i" style="margin:0;width:100px">
    <script>
      document.$('#i').addEventListener('input', () => console.log('typed:' + document.$('#i').value));
      function greet(n) { return { hello: n, sum: host.add(1, 2) }; }
    </script></body></html>"#;

unsafe extern "C" fn add(args: *const c_char, user: *mut c_void) -> *const c_char {
    let calls = unsafe { &mut *(user as *mut Vec<String>) };
    calls.push(unsafe { CStr::from_ptr(args) }.to_string_lossy().into_owned());
    c"3".as_ptr()
}

#[test]
fn a_c_host_can_open_windows_and_pass_messages() {
    unsafe {
        let main = nile_view_create_html(
            s(r#"<body><script>
                 globalThis.got = [];
                 globalThis.w = new Window({ html: "<body><p id=x>child</p><script>window.addEventListener('message', e => document.$('#x').text = 'got ' + e.data.hi);<\/script></body>", width: 300, height: 200, parameters: { n: 7 } });
                 w.closed.then(v => got.push('closed:' + JSON.stringify(v)));
                 Window.this.post(1, {}); </script></body>"#)
            .as_ptr(),
            ptr::null(),
            400.0,
            300.0,
        );
        let mut open = None;
        while let p @ 1.. = nile_view_take_window_request(main) as usize {
            let json = take(p as *mut c_char);
            if json.contains("\"kind\":\"open\"") {
                open = Some(json);
            }
        }
        let open = open.expect("new Window() is an open request");
        assert!(open.contains("\"width\":\"300\"") && open.contains("\"opener\":"), "{open}");
        let id: u32 = open.split("\"id\":").nth(1).unwrap().split(',').next().unwrap().parse().unwrap();
        let me = nile_view_window_id(main);
        assert_ne!(id, me);
        let child = nile_view_create_window(ptr::null(), s("<body><p id=x>child</p><script>window.addEventListener('message', e => document.$('#x').text = 'got ' + e.data.hi);</script></body>").as_ptr(), ptr::null(), 300.0, 200.0, id, me, false, s("{\"n\":7}").as_ptr());
        assert_eq!(nile_view_window_id(child), id);
        assert!(nile_view_window_message(child, me, s("{\"hi\":\"there\"}").as_ptr()));
        let mut out = ptr::null_mut();
        nile_view_eval(child, s("document.$('#x').text + ':' + Window.this.parameters.n").as_ptr(), &mut out);
        assert_eq!(take(out), "got there:7");
        assert!(nile_view_close_request(child, 1));
        nile_view_window_closed(main, id, s("\"ok\"").as_ptr());
        let mut out = ptr::null_mut();
        nile_view_eval(main, s("got.join()").as_ptr(), &mut out);
        assert_eq!(take(out), "closed:\"ok\"");
        nile_view_destroy(child);
        nile_view_destroy(main);
    }
}

#[test]
fn a_c_host_can_load_render_type_script_and_get_window_requests() {
    unsafe {
        let v = nile_view_create_html(s(PAGE).as_ptr(), ptr::null(), 400.0, 200.0);
        assert!(!v.is_null());
        assert_eq!(CStr::from_ptr(nile_version()).to_str().unwrap(), env!("CARGO_PKG_VERSION"));

        // Render into our own buffer; the white-ish page background and the button show up.
        let (w, h) = (400u32, 200u32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        assert!(nile_view_render(v, px.as_mut_ptr(), px.len(), w, h, 1.0));
        assert!(px.chunks(4).any(|p| p != [0, 0, 0, 0]), "something was painted");
        assert!(!nile_view_render(v, px.as_mut_ptr(), 10, w, h, 1.0), "short buffers are rejected");

        // Type into the input and watch scripts react.
        nile_pointer_move(v, 150.0, 12.0);
        nile_pointer_button(v, 0, true, 0);
        nile_pointer_button(v, 0, false, 0);
        assert!(nile_text_input(v, s("hi").as_ptr()));
        assert_eq!(take(nile_view_take_console(v)), "typed:h\ntyped:hi");
        assert_eq!(take(nile_view_take_console(v)), "", "console lines are delivered once");

        // Eval, call, and a host function the script calls back.
        let mut out = ptr::null_mut();
        assert_eq!(nile_view_eval(v, s("document.$('#i').value.length").as_ptr(), &mut out), 0);
        assert_eq!(take(out), "2");
        assert_eq!(nile_view_eval(v, s("throw new Error('boom')").as_ptr(), &mut out), 1);
        assert!(take(out).contains("boom"));
        let mut calls: Vec<String> = Vec::new();
        assert!(nile_view_register_function(v, s("add").as_ptr(), Some(add), &mut calls as *mut _ as *mut c_void));
        assert_eq!(nile_view_call(v, s("greet").as_ptr(), s(r#"["Ada"]"#).as_ptr(), &mut out), 0);
        assert_eq!(take(out), r#"{"hello":"Ada","sum":3}"#);
        assert_eq!(calls, ["[1,2]"]);

        // Window: config, title, and the close button queued as a request.
        let mut cfg = std::mem::zeroed::<NileWindowConfig>();
        cfg.struct_size = std::mem::size_of::<NileWindowConfig>() as u32;
        assert!(nile_view_window_config(v, 1000.0, 800.0, &mut cfg));
        assert_eq!((cfg.width, cfg.frame, cfg.frameless), (320.0, 2, true));
        assert_eq!(CStr::from_ptr(cfg.title).to_str().unwrap(), "Demo");
        nile_pointer_move(v, 20.0, 10.0);
        nile_pointer_button(v, 0, true, 0);
        nile_pointer_button(v, 0, false, 0);
        let mut cmd = NileWindowCommand { struct_size: std::mem::size_of::<NileWindowCommand>() as u32, kind: NileWindowCommandKind::None, a: 0.0, b: 0.0 };
        assert!(nile_view_take_window_command(v, &mut cmd));
        assert_eq!(cmd.kind, NileWindowCommandKind::Close);
        assert!(!nile_view_take_window_command(v, &mut cmd));
        assert_eq!(cmd.kind, NileWindowCommandKind::None);

        let mut tick = NileTick { struct_size: std::mem::size_of::<NileTick>() as u32, redraw: false, next_ms: 0 };
        nile_view_tick(v, &mut tick);
        assert!(tick.next_ms == -1 || tick.next_ms >= 0);
        nile_view_destroy(v);
    }
}

#[test]
fn translucent_config_dialogs_drops_and_portals_reach_c() {
    unsafe {
        let html = s(r#"<html window-frame="solid-with-shadow" window-material="mica"><body><input id="f" type="file" style="width:200px;height:30px"><script>
            window.dropped = ""; window.active = "";
            document.getElementById("f").addEventListener("change", () => { window.dropped = this_files(); });
            function this_files() { return document.getElementById("f").files.length + ":" + document.getElementById("f").files[0].name; }
            Window.this.on("activate", (e) => { window.active += e.reason; });
        </script></body></html>"#);
        let v = nile_view_create_html(html.as_ptr(), ptr::null(), 300.0, 200.0);
        let mut cfg = std::mem::zeroed::<NileWindowConfig>();
        cfg.struct_size = std::mem::size_of::<NileWindowConfig>() as u32;
        assert!(nile_view_window_config(v, 1000.0, 800.0, &mut cfg));
        assert_eq!((cfg.frame, cfg.shadow, cfg.translucent, cfg.material), (4, true, true, 3));

        // Dialogs: a host that answers every open dialog with one file.
        unsafe extern "C" fn open(_m: bool, _f: *const c_char, caption: *const c_char, _d: *const c_char, _u: *mut c_void) -> *const c_char {
            assert_eq!(CStr::from_ptr(caption).to_str().unwrap(), "Pick");
            c"/tmp/nile-capi-dialog.txt".as_ptr()
        }
        let dialogs = NileDialogs { struct_size: std::mem::size_of::<NileDialogs>() as u32, open_files: Some(open), save_file: None, pick_folder: None, message: None, user: ptr::null_mut() };
        nile_view_set_dialogs(v, &dialogs);
        let mut tick = NileTick { struct_size: std::mem::size_of::<NileTick>() as u32, redraw: false, next_ms: 0 };
        nile_view_tick(v, &mut tick);
        assert!(nile_view_eval_ok(v, "String(Window.this.selectFile({ mode: 'open', caption: 'Pick' })).endsWith('nile-capi-dialog.txt')"));

        // Activation and file drops (the pointer says where).
        assert!(nile_view_eval_ok(v, "true"));
        nile_view_window_activated(v, true);
        assert!(nile_view_eval_ok(v, "window.active === '1'"));
        nile_pointer_move(v, 20.0, 15.0);
        assert!(nile_view_drop_files(v, s("/tmp/nile-capi-drop.txt").as_ptr()));
        assert!(nile_view_eval_ok(v, "window.dropped === '1:nile-capi-drop.txt'"));

        // Portals: none yet; an unknown id is refused.
        assert_eq!(take(nile_view_portals(v)), "[]");
        let mut px = [0u8; 16];
        assert!(!nile_view_render_portal(v, 12345, px.as_mut_ptr(), px.len(), 2, 2, 1.0));
        nile_view_destroy(v);
    }
}

unsafe fn nile_view_eval_ok(v: *mut NileView, code: &str) -> bool {
    let mut out = ptr::null_mut();
    let code = unsafe { nile_view_eval(v, s(code).as_ptr(), &mut out) };
    code == 0 && !out.is_null() && unsafe { take(out) } == "true"
}

#[test]
fn a_c_host_can_set_the_system_theme() {
    unsafe {
        let html = s(r#"<html><head><style>:root { color-scheme: light dark }</style></head><body><button id="b">x</button></body></html>"#);
        let v = nile_view_create_html(html.as_ptr(), ptr::null(), 200.0, 100.0);
        assert!(nile_view_set_system_theme(v, true, 0x336699));
        assert!(!nile_view_set_system_theme(v, true, 0x336699), "unchanged");
        let mut out = ptr::null_mut();
        assert_eq!(nile_view_eval(v, s("getComputedStyle(document.getElementById('b')).color").as_ptr(), &mut out), 0);
        assert_eq!(take(out), "rgb(255, 255, 255)", "the dark controls");
        assert!(nile_view_set_system_theme(v, false, -1));
        nile_view_destroy(v);
    }
}

#[test]
fn bad_arguments_fail_softly() {
    unsafe {
        assert!(nile_view_create_html(ptr::null(), ptr::null(), 10.0, 10.0).is_null());
        assert!(!nile_pointer_move(ptr::null_mut(), 0.0, 0.0));
        assert_eq!(nile_view_eval(ptr::null_mut(), s("1").as_ptr(), ptr::null_mut()), 2);
        nile_view_destroy(ptr::null_mut());
        nile_string_free(ptr::null_mut());
    }
}

#[test]
fn the_abi_version_and_struct_sizes_keep_old_hosts_safe() {
    unsafe {
        assert_eq!(nile_abi_version(), NILE_ABI_VERSION);
        let v = nile_view_create_html(s(PAGE).as_ptr(), ptr::null(), 300.0, 200.0);

        // A host that forgot to set `struct_size` gets nothing written, rather than memory it did not give.
        let mut cfg = std::mem::zeroed::<NileWindowConfig>();
        assert!(!nile_view_window_config(v, 1000.0, 800.0, &mut cfg));
        assert_eq!(cfg.width, 0.0);

        // A host built against an older, smaller struct only has the part it knows filled in.
        let mut old = std::mem::zeroed::<NileWindowConfig>();
        old.struct_size = std::mem::offset_of!(NileWindowConfig, min_width) as u32;
        assert!(nile_view_window_config(v, 1000.0, 800.0, &mut old));
        assert_eq!(old.width, 320.0);
        assert_eq!(old.min_width, 0.0);
        assert_eq!(old.struct_size as usize, std::mem::offset_of!(NileWindowConfig, min_width));

        // An older NileClipboard (without the HTML callbacks) reads the missing ones as NULL.
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct OldClipboard {
            struct_size: u32,
            get: Option<unsafe extern "C" fn(*mut c_void) -> *const c_char>,
            set: Option<unsafe extern "C" fn(*const c_char, *mut c_void)>,
            user: *mut c_void,
        }
        unsafe extern "C" fn get(_u: *mut c_void) -> *const c_char {
            c"clip".as_ptr()
        }
        let old = OldClipboard { struct_size: std::mem::size_of::<OldClipboard>() as u32, get: Some(get), set: None, user: ptr::null_mut() };
        let read = read_in(&old as *const OldClipboard as *const NileClipboard).unwrap();
        assert!(read.get.is_some() && read.get_html.is_none() && read.set_html.is_none());
        assert_eq!(read.struct_size as usize, std::mem::size_of::<NileClipboard>());
        assert!(read_in::<NileClipboard>(ptr::null()).is_none());
        nile_view_destroy(v);
    }
}

#[test]
fn tray_badge_progress_hotkeys_screens_and_application_events_reach_c() {
    unsafe {
        let html = s(r#"<html window-frame="solid" style="border-radius:12px"><body><menu id="t"><li id="a">One</li></menu><button role="window-maximize" style="width:20px;height:20px">m</button><script>
            window.log = [];
            Window.this.tray = { icon: "none.png", tooltip: "Tip", menu: document.getElementById("t") };
            application.badge = 3;
            application.progress = 0.5;
            document.addEventListener("trayclick", () => log.push("tray"));
            document.addEventListener("open-url", (e) => log.push("url:" + e.url));
            window.key = application.registerHotkey("Ctrl+Shift+K", () => log.push("pressed"));
            key.ready.then((ok) => log.push("ready:" + ok));
            window.addEventListener("resolutionchange", () => log.push("res"));
        </script></body></html>"#);
        let v = nile_view_create_html(html.as_ptr(), ptr::null(), 300.0, 200.0);

        let tray: serde_json::Value = serde_json::from_str(&take(nile_view_tray(v))).unwrap();
        assert_eq!((tray["tooltip"].as_str(), tray["menu"].as_array().map(Vec::len)), (Some("Tip"), Some(1)));
        assert!(tray["icon"].is_string());
        nile_view_tray_event(v, s("trayclick").as_ptr());
        assert_eq!(take(nile_view_badge(v)), "3");
        let mut x = 0.0f32;
        assert_eq!(nile_view_progress(v, &mut x), 2);
        assert_eq!(x, 0.5);

        let keys: serde_json::Value = serde_json::from_str(&take(nile_view_hotkeys(v))).unwrap();
        assert_eq!(keys, serde_json::json!([{ "id": 1, "accelerator": "Ctrl+Shift+K" }]));
        nile_view_hotkey_event(v, 1, NileHotkeyState::Registered);
        nile_view_hotkey_event(v, 1, NileHotkeyState::Pressed);

        // Monitors: the first report is silent, a later change fires resolutionchange.
        let one = s(r#"[{"name":"A","x":0,"y":0,"width":2880,"height":1800,"scale":2,"primary":true}]"#);
        assert!(nile_view_set_screens(v, one.as_ptr(), 0));
        assert!(!nile_view_set_screens(v, one.as_ptr(), 0));
        let two = s(r#"[{"name":"A","x":0,"y":0,"width":2880,"height":1800,"scale":2,"primary":true},{"name":"B","x":2880,"y":0,"width":1920,"height":1080,"scale":1,"primary":false}]"#);
        assert!(nile_view_set_screens(v, two.as_ptr(), 1));
        assert!(nile_view_eval_ok(v, "Window.screens === 2 && Window.this.screen === 1"));

        // Application events: valid kinds and objects only.
        assert!(!nile_view_application_event(v, s("bad kind\"").as_ptr(), ptr::null()));
        assert!(!nile_view_application_event(v, s("open-url").as_ptr(), s("[1]").as_ptr()));
        nile_view_application_event(v, s("open-url").as_ptr(), s(r#"{"url":"myapp://x"}"#).as_ptr());

        let mut rect = [0.0f32; 4];
        assert!(nile_view_maximize_button_rect(v, rect.as_mut_ptr()));
        assert!(rect[2] >= 20.0 && rect[3] >= 20.0);
        assert_eq!(nile_view_window_corner_radius(v), 12.0);
        for _ in 0..3 {
            let mut tick = NileTick { struct_size: std::mem::size_of::<NileTick>() as u32, redraw: false, next_ms: 0 };
            nile_view_tick(v, &mut tick);
        }
        assert!(nile_view_eval_ok(v, "['tray', 'ready:true', 'pressed', 'res', 'url:myapp://x'].every((x) => log.includes(x))"));
        nile_view_destroy(v);
    }
}
