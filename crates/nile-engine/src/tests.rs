//! Unit tests for the crate root; split out of lib.rs, which they used to follow.
use super::*;
use crate::menus::{Accel, MenuItem};

/// A page loaded from a temp file, laid out at 600x300.
fn page(html: &str, name: &str) -> (Page, PathBuf) {
    let path = std::env::temp_dir().join(format!("nile-shell-{}-{name}.html", std::process::id()));
    std::fs::write(&path, html).unwrap();
    let mut p = Page::load(path.clone(), (600.0, 300.0), 0.0);
    p.layout(600.0, 300.0);
    (p, path)
}

fn center(p: &Page, id: &str) -> (f32, f32) {
    let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
    let r = p.view.borrow_mut().layout().node_rect(node).unwrap();
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn click(p: &mut Page, id: &str) {
    let (x, y) = center(p, id);
    p.pointer_moved(x, y);
    p.press();
    p.release();
}

fn log(p: &Page) -> Vec<String> {
    p.scripts.as_ref().unwrap().console_output()
}

#[test]
fn bootstrap_scripts_get_application_and_their_own_folder() {
    let dir = std::env::temp_dir().join(format!("nile-boot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("run.js"), "window.seen = [__DIR__.endsWith('/'), __FILE__.endsWith('/run.js'), application.run()].join(); application.start('gpu'); application.quit(3);").unwrap();
    let mut p = Page::html_window(r#"<html window-state="hidden"><script type="module" src="run.js"></script></html>"#, dir.clone(), (400.0, 300.0), nile_script::WindowInfo::default());
    assert_eq!(js(&mut p, "window.seen"), "true,true,0");
    assert_eq!(p.view.borrow_mut().window_requests.last(), Some(&nile_script::WindowCommand::Quit(3)));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_ui_barcode_module_draws_codes_that_match_reference_encoders() {
    // Expected bars/rows come from python-barcode and qrcode (the encoders were also checked against
    // them over thousands of values: all of QR versions 1-40 at every level, EAN-13/8, UPC-A, Code 39,
    // and Code 128 decoded back to its text).
    let (mut p, path) = page(
        r#"<body><nile-barcode id="a" type="ean13" value="590123412345" height="40"></nile-barcode>
           <nile-barcode id="b" type="code39" value="@@"></nile-barcode>
        <script type="module">
          import { encode, svg, types } from "@nile/ui";
          const qr = encode("qr", "Nile", { ecc: "H", mask: 3 });
          window.out = [
            encode("ean13", "5901234123457").bars,
            encode("ean13", "590123412345").text,
            encode("code39", "nile").bars,
            qr.version + ":" + qr.rows.join(","),
            svg("code128", "ORDER-1042").startsWith("<svg"),
            types.join(" "),
          ].join(";");
          try { encode("ean13", "5901234123458"); } catch (e) { window.err = e.message; }
        </script></body>"#,
        "ui-barcode",
    );
    let want = [
        "10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101",
        "5901234123457",
        "10001011101110101010111010001110101110100011101010111010100011101110101110001010100010111011101",
        "1:111111100000001111111,100000100110001000001,101110100010101011101,101110100011101011101,101110101010101011101,100000100111101000001,111111101010101111111,000000001010100000000,001100111111011010000,100010001100000100001,110111111110100110011,010001000000101001011,111011110011000010011,000000001010100100101,111111101001001010000,100000100101110001100,101110100000010001111,101110101100011011010,101110101010100011100,100000100100100011001,111111100110000100000",
        "true",
        "code128 code39 ean13 ean8 upca qr",
    ];
    assert_eq!(js(&mut p, "window.out"), want.join(";"));
    assert!(js(&mut p, "window.err").contains("check digit is wrong"));
    // The element draws an <svg> inside itself, and flags a value it can't encode.
    assert_eq!(js(&mut p, "document.getElementById('a').innerHTML.startsWith('<svg')"), "true");
    assert_eq!(js(&mut p, "document.getElementById('b').hasAttribute('invalid')"), "true");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_drawer_opens_closes_and_keeps_focus() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><button id="t" data-drawer-toggle="#d">menu</button>
           <button id="o" data-drawer-open="#d" style="position:absolute;left:100px;top:0">open</button>
           <nile-drawer id="d" size="200"><button id="in">a</button><button id="x" data-drawer-close>close</button></nile-drawer>
        <script type="module">
          import "@nile/ui";
          window.ev = [];
          const d = window.d = document.getElementById("d");
          d.addEventListener("open", () => ev.push("open"));
          d.addEventListener("close", () => ev.push("close"));
          d.addEventListener("beforeclose", (e) => { if (window.hold) e.preventDefault(); });
        </script></body>"##,
        "ui-drawer",
    );
    let settle = |p: &mut Page, ms: u64| {
        std::thread::sleep(Duration::from_millis(ms));
        p.run_timers();
        p.run_timers();
    };
    let state = |p: &mut Page| js(p, "[d.opened, d.classList.contains('nile-shown'), d.classList.contains('nile-open'), document.activeElement && document.activeElement.id].join()");
    p.run_timers();
    assert_eq!(state(&mut p), "false,false,false,t".replace("t", "") + "", "closed at first");
    click(&mut p, "t");
    settle(&mut p, 40);
    assert_eq!(state(&mut p), "true,true,true,in", "the toggle button opens it and focus moves inside");
    // Escape closes it, and focus goes back to where it was.
    p.key("keydown", "Escape".into(), ModifiersState::default());
    settle(&mut p, 260);
    assert_eq!(state(&mut p), "false,false,false,t");
    // A listener can veto the close.
    js(&mut p, "window.hold = true; d.show()");
    settle(&mut p, 40);
    p.key("keydown", "Escape".into(), ModifiersState::default());
    settle(&mut p, 260);
    assert_eq!(js(&mut p, "d.opened"), "true", "beforeclose preventDefault keeps it open");
    js(&mut p, "window.hold = false");
    // The close button inside closes it; a bubbling `open`/`close` pair was fired each time.
    click(&mut p, "x");
    settle(&mut p, 260);
    assert_eq!(js(&mut p, "d.opened"), "false");
    assert_eq!(js(&mut p, "ev.join()"), "open,close,open,close");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_dialog_toast_and_shortcuts_work_together() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><input id="field"><button id="b" data-shortcut="F8" onclick="window.pressed = (window.pressed || 0) + 1">New</button>
        <script type="module">
          import { confirm, prompt, alert, dialog, toast, shortcuts } from "@nile/ui";
          window.ui = { confirm, prompt, alert, dialog, toast, shortcuts };
          window.hits = [];
          shortcuts.on("F3", () => hits.push("F3"));
          shortcuts.on("Mod+S, Alt+S", () => hits.push("save"), { label: "Save" });
          shortcuts.on("a", () => hits.push("a"));
          shortcuts.on("Escape", () => hits.push("esc-always"), { always: true });
        </script></body>"##,
        "ui-dialog",
    );
    let settle = |p: &mut Page, ms: u64| {
        std::thread::sleep(Duration::from_millis(ms));
        p.run_timers();
        p.run_timers();
    };
    p.run_timers();
    let key = |p: &mut Page, k: &str, m: ModifiersState| p.key("keydown", k.into(), m);
    let ctrl = if cfg!(target_os = "macos") { ModifiersState::SUPER } else { ModifiersState::CONTROL };

    // Shortcuts: a function key, a modified key, an alias list, a data-shortcut button.
    key(&mut p, "F3", ModifiersState::default());
    key(&mut p, "s", ctrl);
    key(&mut p, "s", ModifiersState::ALT);
    assert_eq!(js(&mut p, "hits.join()"), "F3,save,save");
    key(&mut p, "F8", ModifiersState::default());
    assert_eq!(js(&mut p, "window.pressed"), "1", "data-shortcut clicks the button");
    assert_eq!(js(&mut p, "document.getElementById('b').getAttribute('data-hint')"), "F8");
    // A plain letter fires from the page, not while typing in a field.
    key(&mut p, "a", ModifiersState::default());
    js(&mut p, "document.getElementById('field').focus()");
    key(&mut p, "a", ModifiersState::default());
    assert_eq!(js(&mut p, "hits.slice(3).join()"), "a");
    assert_eq!(js(&mut p, "ui.shortcuts.format('Alt+S')"), if cfg!(target_os = "macos") { "\u{2325}S" } else { "Alt+S" }.to_string());
    assert_eq!(js(&mut p, "ui.shortcuts.list().length"), "4");

    // confirm(): resolves true on OK, false on Escape; Escape reaches an `always` shortcut only when it isn't dialog-handled.
    js(&mut p, "window.r = []; ui.confirm('Void this order?', { title: 'Void', ok: 'Void', danger: true }).then((v) => r.push(v));");
    settle(&mut p, 40);
    assert_eq!(js(&mut p, "document.querySelectorAll('nile-dialog.nile-open').length"), "1");
    assert_eq!(js(&mut p, "document.querySelector('.nile-dialog-head').textContent"), "Void");
    assert_eq!(js(&mut p, "document.querySelector('.nile-dialog-text').textContent"), "Void this order?");
    assert_eq!(js(&mut p, "document.activeElement.className"), "nile-primary nile-danger", "the primary button has focus");
    // Shortcuts are quiet while the dialog is open.
    key(&mut p, "F3", ModifiersState::default());
    assert_eq!(js(&mut p, "hits.filter((h) => h === 'F3').length"), "1");
    key(&mut p, "Enter", ModifiersState::default());
    settle(&mut p, 220);
    assert_eq!(js(&mut p, "r.join()"), "true", "Enter presses the primary button");
    assert_eq!(js(&mut p, "document.querySelectorAll('nile-dialog').length"), "0", "the temporary dialog removes itself");
    assert_eq!(js(&mut p, "document.activeElement.id"), "field", "focus goes back");
    js(&mut p, "ui.confirm('again?').then((v) => r.push(v));");
    settle(&mut p, 40);
    key(&mut p, "Escape", ModifiersState::default());
    settle(&mut p, 220);
    assert_eq!(js(&mut p, "r.join()"), "true,false");

    // prompt(): validation keeps it open with the message, a good value resolves, cancel gives null.
    js(&mut p, "window.q = []; ui.prompt('Name?', { value: 'x', validate: (t) => t.length < 3 ? 'Too short' : '' }).then((v) => q.push(v));");
    settle(&mut p, 40);
    assert_eq!(js(&mut p, "document.activeElement.tagName + ':' + document.activeElement.value"), "INPUT:x");
    js(&mut p, "document.querySelector('nile-dialog button.nile-primary').click()");
    settle(&mut p, 30);
    assert_eq!(js(&mut p, "document.querySelector('.nile-dialog-error').textContent"), "Too short");
    assert_eq!(js(&mut p, "q.length"), "0");
    js(&mut p, "document.querySelector('nile-dialog input').value = 'Ada Lovelace'; document.querySelector('nile-dialog button.nile-primary').click()");
    settle(&mut p, 220);
    assert_eq!(js(&mut p, "q.join()"), "Ada Lovelace");
    js(&mut p, "ui.prompt('Name?').then((v) => q.push(v === null ? 'null' : v));");
    settle(&mut p, 40);
    js(&mut p, "document.querySelector('nile-dialog button:not(.nile-primary)').click()");
    settle(&mut p, 220);
    assert_eq!(js(&mut p, "q.join()"), "Ada Lovelace,null");

    // dialog(): custom buttons, a function value, tab stays inside.
    js(&mut p, "window.d = []; ui.dialog({ title: 'Print', message: 'Print now?', buttons: [{ label: 'No', value: 'no', cancel: true }, { label: 'Yes', value: 'yes', primary: true }] }).then((v) => d.push(v));");
    settle(&mut p, 40);
    key(&mut p, "Tab", ModifiersState::default());
    assert_eq!(js(&mut p, "document.activeElement.textContent"), "No", "Tab wraps around inside the dialog");
    js(&mut p, "document.querySelector('nile-dialog button').click()");
    settle(&mut p, 220);
    assert_eq!(js(&mut p, "d.join()"), "no");

    // toast(): shows, picks its type and role, closes by itself, and is capped.
    js(&mut p, "window.t = ui.toast.error('Printer offline', { duration: 600 })");
    settle(&mut p, 30);
    assert_eq!(js(&mut p, "[document.querySelector('.nile-toast').getAttribute('data-type'), document.querySelector('.nile-toast').getAttribute('role'), document.querySelector('.nile-toast-text').textContent].join()"), "error,alert,Printer offline");
    assert_eq!(js(&mut p, "document.querySelector('.nile-toast').classList.contains('nile-in')"), "true");
    settle(&mut p, 700);
    settle(&mut p, 250);
    assert_eq!(js(&mut p, "document.querySelectorAll('.nile-toast').length"), "0", "it goes away on its own");
    js(&mut p, "for (let i = 0; i < 8; i++) ui.toast('m' + i, { duration: 0 });");
    settle(&mut p, 250);
    settle(&mut p, 250);
    assert_eq!(js(&mut p, "document.querySelectorAll('.nile-toast').length"), "5", "at most five at a time");
    js(&mut p, "window.acted = 0; ui.toast.clear(); ui.toast('Deleted', { duration: 0, action: { label: 'Undo', onClick: () => acted++ } })");
    settle(&mut p, 250);
    js(&mut p, "document.querySelector('.nile-toast-action').click()");
    settle(&mut p, 250);
    assert_eq!(js(&mut p, "acted + ':' + document.querySelectorAll('.nile-toast').length"), "1:0");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_tabs_accordion_and_tiles_behave() {
    let (mut p, path) = page(
        r##"<body style="margin:0">
          <nile-tabs id="t" selected="b">
            <nile-tab label="One" name="a"><p id="pa">A</p></nile-tab>
            <nile-tab label="Two" name="b" badge="3"><p id="pb">B</p></nile-tab>
            <nile-tab label="Three" name="c" disabled><p>C</p></nile-tab>
            <nile-tab label="Four" name="d"><p>D</p></nile-tab>
          </nile-tabs>
          <nile-accordion id="acc" exclusive>
            <nile-section heading="S1" open><i id="s1">x</i></nile-section>
            <nile-section heading="S2"><i id="s2">y</i></nile-section>
          </nile-accordion>
          <nile-tiles id="tiles" columns="2" gap="4" style="width:300px">
            <nile-tile id="k1" label="Search" shortcut="F3" command="search"></nile-tile>
            <nile-tile id="k2" label="Pay" shortcut="F10" variant="success" badge="2"></nile-tile>
            <nile-tile id="k3" label="Wide" span="2" disabled></nile-tile>
          </nile-tiles>
        <script type="module">
          import "@nile/ui";
          for (const id of ["t", "acc", "tiles", "pa", "pb", "s1", "s2", "k1", "k2", "k3"]) window[id] = document.getElementById(id);
          window.log = [];
          t.addEventListener("tabchange", (e) => log.push("tab:" + e.detail.name));
          acc.addEventListener("sectionchange", (e) => log.push("sec:" + e.detail.index + (e.detail.open ? "+" : "-")));
          tiles.addEventListener("tile", (e) => log.push("tile:" + e.detail.command));
          window.vis = (el) => { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
        </script></body>"##,
        "ui-tabs",
    );
    p.run_timers();
    p.run_timers();
    let key = |p: &mut Page, k: &str| p.key("keydown", k.into(), ModifiersState::default());

    // Tabs: `selected` takes a name; the strip has a button per tab with a badge; only one panel is shown.
    assert_eq!(js(&mut p, "t.selectedIndex"), "1");
    assert_eq!(js(&mut p, "document.querySelectorAll('.nile-tab-button').length"), "4");
    assert_eq!(js(&mut p, "document.querySelector('.nile-tab-badge').textContent"), "3");
    assert_eq!(js(&mut p, "[vis(pa), vis(pb)].join()"), "false,true");
    js(&mut p, "document.querySelectorAll('.nile-tab-button')[0].click()");
    assert_eq!(js(&mut p, "[t.selectedIndex, vis(pa), vis(pb)].join()"), "0,true,false");
    // Arrow keys skip the disabled tab; Home/End jump; a disabled tab can't be selected.
    js(&mut p, "document.querySelectorAll('.nile-tab-button')[0].focus()");
    key(&mut p, "ArrowRight");
    key(&mut p, "ArrowRight");
    assert_eq!(js(&mut p, "t.selectedIndex"), "3", "Right from Two goes to Four, skipping Three");
    key(&mut p, "Home");
    assert_eq!(js(&mut p, "t.selectedIndex"), "0");
    js(&mut p, "t.select('c')");
    assert_eq!(js(&mut p, "t.selectedIndex"), "0");
    // beforetabchange can veto; select(name) works.
    js(&mut p, "window.hold = true; t.addEventListener('beforetabchange', (e) => { if (hold) e.preventDefault(); }); t.select('d')");
    assert_eq!(js(&mut p, "t.selectedIndex"), "0");
    js(&mut p, "hold = false; t.select('d')");
    assert_eq!(js(&mut p, "t.selectedTab.getAttribute('name')"), "d");
    assert_eq!(js(&mut p, "log.join()"), "tab:a,tab:b,tab:d,tab:a,tab:d");

    // Accordion: exclusive keeps one section open; the event says which.
    assert_eq!(js(&mut p, "[acc.sections[0].opened, acc.sections[1].opened, vis(s1), vis(s2)].join()"), "true,false,true,false");
    js(&mut p, "log.length = 0; document.querySelectorAll('.nile-section-head')[1].click()");
    assert_eq!(js(&mut p, "[acc.sections[0].opened, acc.sections[1].opened, vis(s1), vis(s2)].join()"), "false,true,false,true");
    assert_eq!(js(&mut p, "log.join()"), "sec:0-,sec:1+");
    js(&mut p, "document.querySelectorAll('.nile-section-head')[1].click()");
    assert_eq!(js(&mut p, "acc.sections[1].opened"), "false", "a click on an open heading closes it");
    assert_eq!(js(&mut p, "document.querySelectorAll('.nile-section-head')[0].getAttribute('aria-expanded')"), "false");

    // Tiles: a shortcut presses the tile; a click does too; the corner key text and span show; disabled does nothing.
    js(&mut p, "log.length = 0");
    key(&mut p, "F3");
    click(&mut p, "k2");
    click(&mut p, "k3");
    assert_eq!(js(&mut p, "log.join()"), "tile:search,tile:Pay");
    assert_eq!(js(&mut p, "k1.querySelector('.nile-tile-key').textContent"), "F3");
    assert_eq!(js(&mut p, "k2.querySelector('.nile-tile-badge').textContent"), "2");
    assert_eq!(js(&mut p, "k3.style.gridColumn"), "span 2");
    let w = |p: &mut Page, id: &str| js(p, &format!("Math.round(document.getElementById('{id}').getBoundingClientRect().width)"));
    assert_eq!(w(&mut p, "k1"), "148", "two columns of (300 - 4) / 2");
    assert_eq!(w(&mut p, "k3"), "300", "span 2 fills the row");
    js(&mut p, "k1.focus()");
    key(&mut p, "ArrowRight");
    assert_eq!(js(&mut p, "document.activeElement.id"), "k2");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_combobox_filters_picks_and_loads_from_a_source() {
    let (mut p, path) = page(
        r##"<body style="margin:0;height:300px"><input id="before"><nile-combobox id="c" placeholder="Product" style="width:240px">
            <option value="1" data-hint="2.50">Whole milk</option><option value="2">Sourdough loaf</option><option value="3" disabled>Sold out</option><option value="4">Soda water</option>
          </nile-combobox>
        <script type="module">
          import "@nile/ui";
          window.c = document.getElementById("c");
          window.log = [];
          c.addEventListener("change", (e) => log.push("change:" + e.detail.value));
          window.rows = () => Array.from(document.querySelectorAll('.nile-combo-option')).map((r) => r.textContent.replace(/\d\.\d\d$/, '')).join('|');
          window.listOpen = () => document.querySelector('.nile-combo-list').classList.contains('nile-open');
        </script></body>"##,
        "ui-combo",
    );
    p.run_timers();
    p.run_timers();
    let settle = |p: &mut Page, ms: u64| {
        std::thread::sleep(Duration::from_millis(ms));
        p.run_timers();
        p.run_timers();
    };
    let key = |p: &mut Page, k: &str| p.key("keydown", k.into(), ModifiersState::default());
    assert_eq!(js(&mut p, "document.querySelector('.nile-combo-input').getAttribute('placeholder')"), "Product");
    // Made through innerHTML, with its value attribute: the choice survives the late upgrade.
    js(&mut p, "const h = document.createElement('div'); h.id = 'made'; document.body.appendChild(h); h.innerHTML = '<nile-combobox id=\\'m\\' value=\\'2\\' placeholder=\\'Late\\'><option value=\\'1\\'>One</option><option value=\\'2\\'>Two</option></nile-combobox>'");
    settle(&mut p, 30);
    assert_eq!(js(&mut p, "const m = document.getElementById('m'); [m.value, m.text, m.selectedItem && m.selectedItem.label].join()"), "2,Two,Two");
    assert_eq!(js(&mut p, "listOpen()"), "false");
    // Focus opens it with everything listed (the hint shows beside the label).
    js(&mut p, "c.focus()");
    assert_eq!(js(&mut p, "listOpen()"), "true");
    assert_eq!(js(&mut p, "rows()"), "Whole milk|Sourdough loaf|Sold out|Soda water");
    // Typing filters: prefix matches first, the matching text is marked.
    p.insert_text("so");
    assert_eq!(js(&mut p, "rows()"), "Sourdough loaf|Sold out|Soda water");
    assert_eq!(js(&mut p, "document.querySelector('.nile-combo-match').textContent"), "So");
    // The first usable row is highlighted; Down skips the disabled one; Enter picks.
    assert_eq!(js(&mut p, "document.querySelector('.nile-combo-option.nile-active').textContent"), "Sourdough loaf");
    key(&mut p, "ArrowDown");
    assert_eq!(js(&mut p, "document.querySelector('.nile-combo-option.nile-active').textContent"), "Soda water");
    key(&mut p, "Enter");
    assert_eq!(js(&mut p, "[c.value, c.text, listOpen(), c.selectedItem && c.selectedItem.label, log.join()].join()"), "4,Soda water,false,Soda water,change:4");
    // Escape reverts typed text to the chosen item.
    p.insert_text("x");
    assert_eq!(js(&mut p, "rows()"), "", "nothing matches \"Soda waterx\" (the list shows the empty row)");
    key(&mut p, "Escape");
    assert_eq!(js(&mut p, "[c.text, listOpen()].join()"), "Soda water,false");
    // Blur with text that isn't an item reverts it; with an exact (case-insensitive) label it picks it.
    js(&mut p, "c.focus(); c.text = 'whole MILK'; document.getElementById('before').focus()");
    settle(&mut p, 20);
    assert_eq!(js(&mut p, "[c.value, c.text].join()"), "1,Whole milk");
    js(&mut p, "c.focus(); c.text = 'zzz'; document.getElementById('before').focus()");
    settle(&mut p, 20);
    assert_eq!(js(&mut p, "c.text"), "Whole milk");
    // value setter, allow-custom.
    js(&mut p, "c.value = '2'");
    assert_eq!(js(&mut p, "c.text"), "Sourdough loaf");
    js(&mut p, "c.setAttribute('allow-custom', ''); c.focus(); c.text = 'Gift card'; document.getElementById('before').focus()");
    settle(&mut p, 20);
    assert_eq!(js(&mut p, "[c.value, c.selectedItem, log[log.length - 1]].join()"), "Gift card,,change:Gift card");

    // source: loaded as the user types, only the newest answer is used.
    js(&mut p, "c.clear(); window.asked = []; c.source = (q) => { asked.push(q); return new Promise((ok) => setTimeout(() => ok(q === 'a' ? [{ value: 9, label: 'Apple' }] : [{ value: 7, label: 'Apricot', hint: 'kg' }, 'Avocado']), q === 'a' ? 120 : 10)); }; c.focus();");
    settle(&mut p, 200);
    p.insert_text("a");
    p.insert_text("p");
    settle(&mut p, 200);
    settle(&mut p, 40);
    assert_eq!(js(&mut p, "asked.join()"), ",ap", "opening asks for the initial list, then typing is debounced: only the settled text is asked for");
    assert_eq!(js(&mut p, "rows()"), "Apricotkg|Avocado");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_ribbon_builds_tabs_groups_menus_and_state() {
    let (mut p, path) = page(
        r##"<body style="margin:0;width:900px"><div id="host"></div>
        <script type="module">
          import { ribbon } from "@nile/ui";
          window.log = [];
          const cfg = {
            file: { label: "File", onClick: () => log.push("file") },
            quick: [{ label: "Save", command: "save", shortcut: "Mod+S", iconOnly: true, icon: '<svg viewBox="0 0 16 16"><rect width="16" height="16"/></svg>' }],
            tabs: [
              { label: "Home", groups: [
                { label: "Clipboard", items: [
                  { label: "Paste", size: "large", command: "paste", menu: [{ label: "Paste special", command: "pasteSpecial" }, { separator: true }, { label: "Plain", command: "plain", checked: true }] },
                  { type: "stack", items: [{ label: "Cut", command: "cut" }, { label: "Copy", command: "copy", disabled: true }] },
                ] },
                { label: "Font", items: [
                  { type: "stack", items: [{ type: "select", command: "font", options: ["Segoe UI", { value: "a", label: "Arial" }], value: "Segoe UI" },
                    { type: "row", items: [{ type: "toggle", label: "B", command: "bold" }] }] },
                ] },
              ] },
              { label: "Insert", groups: [{ label: "Tables", items: [{ label: "Table", size: "large", command: "table", split: true, menu: [{ label: "Quick table", command: "quick" }] }] }] },
              { label: "Table tools", contextual: true, accent: "#7e3fb4", hidden: true, groups: [{ label: "Rows", items: [{ label: "Add row", command: "addRow" }] }] },
            ],
          };
          window.r = ribbon(cfg);
          document.getElementById("host").appendChild(r);
          r.addEventListener("command", (e) => log.push(e.detail.command + (e.detail.checked !== undefined ? ":" + e.detail.checked : "") + (e.detail.value !== undefined ? "=" + e.detail.value : "")));
          r.addEventListener("tabchange", (e) => log.push("tab:" + e.detail.label));
          window.q = (s) => document.querySelector(s);
          window.qa = (s) => document.querySelectorAll(s).length;
          window.vis = (el) => { const b = el.getBoundingClientRect(); return b.width > 0 && b.height > 0; };
        </script></body>"##,
        "ui-ribbon",
    );
    p.run_timers();
    p.run_timers();
    let key = |p: &mut Page, k: &str, m: ModifiersState| p.key("keydown", k.into(), m);
    // Built: a file tab, three tab buttons (the contextual one hidden), one panel with its groups and labels.
    assert_eq!(js(&mut p, "qa('.nile-ribbon-tab')"), "4");
    assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-ribbon-tab')).filter(vis).map((t) => t.textContent).join()"), "File,Home,Insert");
    assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-ribbon-group-label')).map((t) => t.textContent).join()"), "Clipboard,Font");
    assert_eq!(js(&mut p, "q('.nile-ribbon-button.nile-large').textContent"), "Paste");
    assert_eq!(js(&mut p, "q('[data-command=copy]').disabled"), "true");
    assert_eq!(js(&mut p, "q('[data-command=save]').getAttribute('title')"), if cfg!(target_os = "macos") { "Save (\u{2318}S)" } else { "Save (Ctrl+S)" }.to_string());
    // Buttons raise `command`; toggles flip; a select reports its value; the file tab runs its handler.
    js(&mut p, "q('[data-command=cut]').click(); q('[data-command=bold]').click(); q('[data-command=bold]').click()");
    let sel = js(&mut p, "const s = q('.nile-ribbon-select'); s.value = 'a'; s.dispatchEvent(new Event('change', { bubbles: true })); log.join()");
    assert_eq!(sel, "cut,bold:true,bold:false,font=a");
    assert_eq!(js(&mut p, "q('[data-command=bold]').getAttribute('aria-pressed')"), "false");
    js(&mut p, "log.length = 0; q('.nile-ribbon-tab.nile-file').click()");
    assert_eq!(js(&mut p, "log.join()"), "file,file", "the file tab runs its onClick and raises a `command` event");
    // The Mod+S shortcut presses the quick-access button.
    js(&mut p, "log.length = 0");
    key(&mut p, "s", if cfg!(target_os = "macos") { ModifiersState::SUPER } else { ModifiersState::CONTROL });
    assert_eq!(js(&mut p, "log.join()"), "save");
    // A drop-down opens a menu; a row runs its command and closes it; the checkmark shows.
    js(&mut p, "log.length = 0; q('.nile-ribbon-button.nile-large').click()");
    assert_eq!(js(&mut p, "qa('.nile-ribbon-menu')"), "1");
    assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-ribbon-menu-item')).map((i) => i.textContent).join('|')"), "Paste special|\u{2713}Plain");
    key(&mut p, "ArrowDown", ModifiersState::default());
    key(&mut p, "Enter", ModifiersState::default());
    assert_eq!(js(&mut p, "log.join() + ':' + qa('.nile-ribbon-menu')"), "pasteSpecial:0");
    js(&mut p, "q('.nile-ribbon-button.nile-large').click()");
    key(&mut p, "Escape", ModifiersState::default());
    assert_eq!(js(&mut p, "qa('.nile-ribbon-menu')"), "0", "Escape closes the menu");
    // Tabs: selecting swaps the panel; a split button's two halves act separately; the contextual tab shows on demand.
    js(&mut p, "log.length = 0; r.select('Insert')");
    assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-ribbon-group-label')).map((t) => t.textContent).join() + '|' + r.selectedIndex + '|' + log.join()"), "Tables|1|tab:Insert");
    js(&mut p, "log.length = 0; q('[data-command=table]').click()");
    assert_eq!(js(&mut p, "log.join() + ':' + qa('.nile-ribbon-menu')"), "table:0", "the main half just acts");
    js(&mut p, "q('.nile-ribbon-drop').click()");
    assert_eq!(js(&mut p, "qa('.nile-ribbon-menu')"), "1", "the arrow half opens the menu");
    js(&mut p, "document.dispatchEvent(new Event('keydown'))");
    key(&mut p, "Escape", ModifiersState::default());
    js(&mut p, "r.showTab('Table tools', true)");
    assert_eq!(js(&mut p, "r.selectedIndex + ',' + q('.nile-ribbon-group-label').textContent + ',' + vis(document.querySelectorAll('.nile-ribbon-tab')[3])"), "2,Rows,true");
    js(&mut p, "r.hideTab('Table tools')");
    assert_eq!(js(&mut p, "r.selectedIndex"), "0", "hiding the active tab goes back to the first");
    // setState reaches items by command; collapsing hides the panel and a tab click pops it open.
    js(&mut p, "r.setState('copy', { disabled: false }); r.setState('bold', { checked: true })");
    assert_eq!(js(&mut p, "q('[data-command=copy]').disabled + ',' + q('[data-command=bold]').getAttribute('aria-pressed')"), "false,true");
    js(&mut p, "r.collapse()");
    assert_eq!(js(&mut p, "vis(q('.nile-ribbon-panel'))"), "false");
    js(&mut p, "document.querySelectorAll('.nile-ribbon-tab')[2].click()");
    assert_eq!(js(&mut p, "vis(q('.nile-ribbon-panel')) + ',' + q('.nile-ribbon-group-label').textContent"), "true,Tables");
    js(&mut p, "q('[data-command=table]').click()");
    assert_eq!(js(&mut p, "vis(q('.nile-ribbon-panel'))"), "false", "a command closes the pop-up panel");
    js(&mut p, "r.expand()");
    assert_eq!(js(&mut p, "vis(q('.nile-ribbon-panel'))"), "true");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_chart_draws_series_hovers_and_toggles_from_the_legend() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><nile-chart id="ch" type="bar" height="200" style="width:400px"></nile-chart>
        <script type="module">
          import { niceScale } from "@nile/ui";
          window.niceScale = niceScale;
          window.ch = document.getElementById("ch");
          window.hits = [];
          ch.addEventListener("chartclick", (e) => hits.push(e.detail.label + ":" + e.detail.series.map((s) => s.name + "=" + s.value).join("+")));
          ch.data = { labels: ["A", "B", "C"], series: [{ name: "One", values: [10, 40, 25] }, { name: "Two", values: [5, 20, 35] }] };
          window.count = (needle) => (ch.innerHTML.match(new RegExp(needle, "g")) || []).length;
        </script></body>"##,
        "ui-chart",
    );
    p.run_timers();
    p.run_timers();
    assert_eq!(js(&mut p, "JSON.stringify(niceScale(0, 18000, 5).ticks)"), "[0,5000,10000,15000,20000]");
    assert_eq!(js(&mut p, "niceScale(0, 40, 5).max"), "40");
    // Two series x three categories: six bars; the legend names both series.
    assert_eq!(js(&mut p, "count('<rect')"), "6");
    assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-chart-key')).map((k) => k.textContent).join()"), "One,Two");
    // Clicking a legend entry hides that series (and the value axis rescales).
    js(&mut p, "document.querySelectorAll('.nile-chart-key')[1].click()");
    assert_eq!(js(&mut p, "count('<rect')"), "3");
    assert_eq!(js(&mut p, "document.querySelectorAll('.nile-chart-key')[1].classList.contains('nile-off')"), "true");
    js(&mut p, "document.querySelectorAll('.nile-chart-key')[1].click()");
    assert_eq!(js(&mut p, "count('<rect')"), "6");
    // Hover over category B: tooltip with both values; click raises chartclick.
    let at = js(&mut p, "const g = ch._geo; (g.left + g.w * 1.5 / g.n) + ',' + (g.top + g.h / 2)");
    let mut it = at.split(',').map(|v| v.parse::<f32>().unwrap());
    let (x, y) = (it.next().unwrap(), it.next().unwrap());
    p.pointer_moved(x, y);
    assert_eq!(js(&mut p, "const t = document.querySelector('.nile-chart-tooltip'); t.style.display + '|' + t.textContent"), "block|BOne: 40Two: 20");
    p.press();
    p.release();
    assert_eq!(js(&mut p, "hits.join()"), "B:One=40+Two=20");
    p.pointer_moved(x, 150.0 + 40.0);
    // Pie: slices from the first series, legend by label, a slice's tooltip shows its share.
    js(&mut p, "ch.setAttribute('type', 'pie'); ch.data = [{ label: 'Cash', value: 75 }, { label: 'Card', value: 25 }];");
    assert_eq!(js(&mut p, "count('<path')"), "2");
    assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-chart-key')).map((k) => k.textContent).join()"), "Cash,Card");
    let g = js(&mut p, "ch._geo.cx + ',' + ch._geo.cy + ',' + ch._geo.r");
    let v: Vec<f32> = g.split(',').map(|n| n.parse().unwrap()).collect();
    p.pointer_moved(v[0] - v[2] * 0.35, v[1] - v[2] * 0.35);
    assert_eq!(js(&mut p, "document.querySelector('.nile-chart-tooltip').textContent"), "Card25%: 25", "10:30 is inside the last quarter");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_grid_edits_cells_and_resizes_columns_from_the_keyboard() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><nile-grid id="g" key="id" style="width:560px;height:220px"></nile-grid>
        <script type="module">
          import "@nile/ui";
          window.g = document.getElementById("g");
          g.columns = [
            { key: "name", title: "Item", width: 200, editable: true },
            { key: "qty", title: "Qty", width: 80, type: "number", editable: true, validate: (v) => (v === null || v >= 0 ? true : "Not negative") },
            { key: "price", title: "Price", width: 80, type: "number" },
            { key: "amount", title: "Amount", width: 100, type: "number", value: (r) => r.qty * r.price },
            { key: "paid", title: "Paid", width: 60, type: "boolean", editable: true, format: (v) => (v ? "yes" : "no") },
            { key: "size", title: "Size", editable: true, editor: "select", options: ["S", "M", { value: "L", label: "Large" }] },
          ];
          g.rows = [{ id: 1, name: "Milk", qty: 2, price: 2.5, paid: false, size: "S" }, { id: 2, name: "Bread", qty: 1, price: 3, paid: true, size: "M" }, { id: 3, name: "Eggs", qty: 6, price: 0.5, paid: false, size: "L" }];
          window.log = [];
          g.addEventListener("cellchange", (e) => log.push(e.detail.key + ":" + e.detail.oldValue + ">" + e.detail.value));
          g.addEventListener("celleditcancel", () => log.push("cancel"));
          g.addEventListener("columnresize", (e) => log.push("resize:" + e.detail.key + "=" + e.detail.width));
          window.shown = () => Array.from(document.querySelectorAll(".nile-grid-row")).filter((x) => x.style.display !== "none").sort((a, b) => Number(a.getAttribute("aria-rowindex")) - Number(b.getAttribute("aria-rowindex")));
          window.cell = (r, c) => shown()[r].children[c];
          window.rowOf = (id) => g.rows.find((x) => x.id === id);
        </script></body>"##,
        "ui-grid-edit",
    );
    p.run_timers();
    p.run_timers();
    let settle = |p: &mut Page| {
        std::thread::sleep(Duration::from_millis(30));
        p.run_timers();
        p.run_timers();
    };
    let key = |p: &mut Page, k: &str, m: ModifiersState| p.key("keydown", k.into(), m);
    let none = ModifiersState::default();
    let click_cell = |p: &mut Page, r: usize, c: usize| {
        let v = js(p, &format!("(() => {{ const b = cell({r}, {c}).getBoundingClientRect(); return (b.x + 6) + ',' + (b.y + b.height / 2); }})()"));
        let mut it = v.split(',').map(|n| n.parse::<f32>().unwrap_or_else(|_| panic!("click_cell({r},{c}): {v}")));
        let (x, y) = (it.next().unwrap(), it.next().unwrap());
        p.pointer_moved(x, y);
        p.press();
        p.release();
    };
    // Column widths come through two custom properties on the grid, not per-row styles.
    assert!(js(&mut p, "g.style.getPropertyValue('--nile-grid-cols')").starts_with("200px 80px 80px 100px 60px"));

    // A click sets the active cell; F2 opens the editor with the value; Enter commits and moves down.
    click_cell(&mut p, 0, 0);
    key(&mut p, "F2", none);
    assert_eq!(js(&mut p, "g.editing && g.editing.key + g.editing.index"), "name0");
    assert_eq!(js(&mut p, "document.activeElement.className + ':' + document.activeElement.value"), "nile-grid-editor:Milk");
    js(&mut p, "document.activeElement.value = 'Oat milk'");
    key(&mut p, "Enter", none);
    settle(&mut p);
    assert_eq!(js(&mut p, "[rowOf(1).name, cell(0, 0).textContent, g.editing, g.selectedIndex].join()"), "Oat milk,Oat milk,,1");
    assert_eq!(js(&mut p, "log.join()"), "name:Milk>Oat milk");

    // Escape cancels and keeps the old value.
    click_cell(&mut p, 1, 0);
    key(&mut p, "F2", none);
    js(&mut p, "document.activeElement.value = 'zzz'");
    key(&mut p, "Escape", none);
    assert_eq!(js(&mut p, "[rowOf(2).name, cell(1, 0).textContent, g.editing, log[log.length - 1]].join()"), "Bread,Bread,,cancel");

    // Typing starts an edit with that character; numbers parse; computed cells refresh; Tab goes to the next editable cell.
    click_cell(&mut p, 0, 1);
    key(&mut p, "5", none);
    assert_eq!(js(&mut p, "g.editing.key + ':' + document.activeElement.value"), "qty:5");
    key(&mut p, "Tab", none);
    settle(&mut p);
    assert_eq!(js(&mut p, "[rowOf(1).qty, cell(0, 3).textContent, g.editing && g.editing.key].join()"), "5,12.5,paid".replace("paid", "size"), "Tab skips the check box and the read-only cells");
    key(&mut p, "Escape", none);

    // Invalid input keeps the editor open and marked; fixing it commits.
    click_cell(&mut p, 1, 1);
    key(&mut p, "F2", none);
    js(&mut p, "document.activeElement.value = '-3'");
    key(&mut p, "Enter", none);
    assert_eq!(js(&mut p, "g.editing.key + ',' + document.activeElement.classList.contains('nile-invalid') + ',' + document.activeElement.getAttribute('title')"), "qty,true,Not negative");
    js(&mut p, "document.activeElement.value = 'abc'");
    key(&mut p, "Enter", none);
    assert_eq!(js(&mut p, "document.activeElement.getAttribute('title')"), "Enter a number");
    js(&mut p, "document.activeElement.value = '4'");
    key(&mut p, "Enter", none);
    settle(&mut p);
    assert_eq!(js(&mut p, "[rowOf(2).qty, cell(1, 3).textContent, g.editing].join()"), "4,12,");

    // A boolean cell toggles with Space or a click; a select editor writes the option's own value.
    click_cell(&mut p, 2, 4);
    assert_eq!(js(&mut p, "[rowOf(3).paid, cell(2, 4).textContent].join()"), "true,yes", "a click toggles it");
    key(&mut p, " ", none);
    assert_eq!(js(&mut p, "rowOf(3).paid"), "false", "Space toggles it too");
    click_cell(&mut p, 0, 5);
    key(&mut p, "Enter", none);
    assert_eq!(js(&mut p, "document.activeElement.tagName"), "SELECT");
    js(&mut p, "document.activeElement.value = 'L'");
    js(&mut p, "g.commitEdit()");
    assert_eq!(js(&mut p, "rowOf(1).size + ',' + cell(0, 5).textContent"), "L,L");

    // Editing that changes the sorted column moves the row but the active row follows it.
    js(&mut p, "g.sortBy('name', 'asc'); log.length = 0");
    settle(&mut p);
    assert_eq!(js(&mut p, "g.rows.length + ',' + cell(0, 0).textContent"), "3,Bread");
    js(&mut p, "g.edit(0, 'name'); document.activeElement.value = 'Zucchini'");
    key(&mut p, "Enter", none);
    settle(&mut p);
    assert_eq!(js(&mut p, "shown().map((r) => r.children[0].textContent).join()"), "Eggs,Oat milk,Zucchini");

    // Column headers: Shift+Right widens the focused column by 10px (Ctrl adds more); Left/Right move focus; Enter sorts.
    js(&mut p, "log.length = 0; document.querySelectorAll('.nile-grid-hcell')[1].focus()");
    key(&mut p, "ArrowRight", ModifiersState::SHIFT);
    key(&mut p, "ArrowRight", ModifiersState::SHIFT);
    key(&mut p, "ArrowLeft", ModifiersState::SHIFT);
    assert_eq!(js(&mut p, "log.join()"), "resize:qty=90,resize:qty=100,resize:qty=90");
    assert!(js(&mut p, "g.style.getPropertyValue('--nile-grid-cols')").starts_with("200px 90px"));
    key(&mut p, "ArrowLeft", ModifiersState::SHIFT);
    for _ in 0..6 {
        key(&mut p, "ArrowLeft", ModifiersState::SHIFT);
    }
    assert_eq!(js(&mut p, "g.columns[1]._w"), "60", "never narrower than the column's minimum");
    key(&mut p, "ArrowRight", none);
    assert_eq!(js(&mut p, "document.activeElement.textContent.indexOf('Price') === 0"), "true");
    key(&mut p, "Enter", none);
    assert_eq!(js(&mut p, "g.sortKey + ',' + g.sortDirection"), "price,asc");
    assert_eq!(js(&mut p, "document.activeElement.textContent.indexOf('Price') === 0"), "true", "focus stays on the header after it is redrawn");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_tree_expands_selects_filters_and_loads_lazily() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><nile-tree id="tr" style="width:240px;height:200px"></nile-tree>
        <script type="module">
          import "@nile/ui";
          window.tr = document.getElementById("tr");
          tr.items = [{ id: 1, label: "Drinks", children: [{ id: 2, label: "Coffee" }, { id: 3, label: "Tea", children: [{ id: 6, label: "Green" }] }] }, { id: 4, label: "Food", hasChildren: true }, { id: 5, label: "Misc" }];
          tr.load = (item) => new Promise((r) => setTimeout(() => r([{ id: 7, label: "Bread" }]), 0));
          window.log = [];
          tr.addEventListener("selectionchange", (e) => log.push("sel:" + e.detail.id));
          tr.addEventListener("expand", (e) => log.push("exp:" + e.detail.id));
          tr.addEventListener("collapse", (e) => log.push("col:" + e.detail.id));
          tr.addEventListener("activate", (e) => log.push("act:" + e.detail.id));
          window.rows = () => Array.from(tr.querySelectorAll(".nile-tree-row")).map((r) => r.getAttribute("data-id")).join();
        </script></body>"##,
        "ui-tree",
    );
    p.run_timers();
    p.run_timers();
    let none = ModifiersState::default();
    let key = |p: &mut Page, k: &str| p.key("keydown", k.into(), none);
    let settle = |p: &mut Page| {
        std::thread::sleep(Duration::from_millis(30));
        p.run_timers();
        p.run_timers();
    };
    assert_eq!(js(&mut p, "rows()"), "1,4,5");
    assert_eq!(js(&mut p, "tr.getAttribute('role') + ',' + document.querySelector('.nile-tree-row').getAttribute('aria-expanded')"), "tree,false");

    // A click selects; Right expands, then goes to the first child; Down walks; Left goes back to the parent.
    js(&mut p, "document.querySelector('.nile-tree-row').click()");
    js(&mut p, "tr.focus()");
    key(&mut p, "ArrowRight");
    assert_eq!(js(&mut p, "rows()"), "1,2,3,4,5");
    key(&mut p, "ArrowRight");
    key(&mut p, "ArrowDown");
    assert_eq!(js(&mut p, "tr.selected"), "3");
    key(&mut p, "ArrowRight");
    assert_eq!(js(&mut p, "rows()"), "1,2,3,6,4,5");
    key(&mut p, "ArrowRight");
    assert_eq!(js(&mut p, "tr.selected"), "6", "Right on an open branch enters it");
    key(&mut p, "ArrowLeft");
    assert_eq!(js(&mut p, "tr.selected"), "3");
    assert_eq!(js(&mut p, "document.querySelector('[data-id=\"6\"]').getAttribute('aria-level')"), "3");
    // Collapsing an ancestor of the selection moves the selection onto it.
    key(&mut p, "ArrowRight");
    key(&mut p, "ArrowRight");
    js(&mut p, "tr.collapse(1)");
    assert_eq!(js(&mut p, "[rows(), tr.selected].join('|')"), "1,4,5|1");
    key(&mut p, "End");
    assert_eq!(js(&mut p, "tr.selected"), "5");
    key(&mut p, "Enter");
    assert!(js(&mut p, "log.join()").ends_with("act:5"), "{}", js(&mut p, "log.join()"));

    // Lazy children load once, when the branch is first opened.
    js(&mut p, "tr.select(4)");
    key(&mut p, "ArrowRight");
    settle(&mut p);
    assert_eq!(js(&mut p, "rows()"), "1,4,7,5");
    // Type-ahead jumps to a label.
    js(&mut p, "tr.select(1)");
    key(&mut p, "m");
    assert_eq!(js(&mut p, "tr.selected"), "5");

    // A filter keeps matches and their ancestors, opened.
    js(&mut p, "tr.filter = 'gre'");
    assert_eq!(js(&mut p, "rows()"), "1,3,6");
    js(&mut p, "tr.filter = ''");
    assert_eq!(js(&mut p, "rows()"), "1,4,7,5");
    // Data changes: add, update, remove, reveal.
    js(&mut p, "tr.add({ id: 8, label: 'Juice' }, 1)");
    js(&mut p, "tr.reveal(6)");
    assert_eq!(js(&mut p, "rows()"), "1,2,3,6,8,4,7,5");
    js(&mut p, "tr.remove(1)");
    assert_eq!(js(&mut p, "rows()"), "4,7,5");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_keypad_types_into_a_field_and_limits_what_it_takes() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><input id="f"><nile-keypad id="k" target="#f" decimal sign double-zero></nile-keypad>
        <nile-keypad id="n" max-length="3" display enter-label="Go"></nile-keypad>
        <nile-keypad id="pin" pin display max-length="4"></nile-keypad>
        <script type="module">
          import "@nile/ui";
          window.log = [];
          for (const id of ["f", "k", "n", "pin"]) window[id] = document.getElementById(id);
          k.addEventListener("enter", (e) => log.push("enter:" + e.detail.number));
          f.addEventListener("input", () => log.push("in:" + f.value));
          window.press = (pad, ...keys) => keys.forEach((x) => pad.querySelector('[data-key="' + x + '"]').click());
        </script></body>"##,
        "ui-keypad",
    );
    p.run_timers();
    p.run_timers();
    assert_eq!(js(&mut p, "k.querySelectorAll('.nile-keypad-key').length"), "16", "10 digits, point, C, sign, 00, backspace and OK");
    js(&mut p, "press(k, '7', '5')");
    assert_eq!(js(&mut p, "f.value"), "75");
    js(&mut p, "press(k, '.', '5', '5', '5')");
    assert_eq!(js(&mut p, "f.value"), "75.55", "two decimals at most");
    js(&mut p, "press(k, '.')");
    assert_eq!(js(&mut p, "f.value"), "75.55", "one point only");
    js(&mut p, "press(k, 'Backspace', '-')");
    assert_eq!(js(&mut p, "[f.value, k.number].join()"), "-75.5,-75.5");
    js(&mut p, "press(k, 'Clear')");
    assert_eq!(js(&mut p, "f.value"), "");
    js(&mut p, "press(k, '0', '0', '0', '4', '2')");
    assert_eq!(js(&mut p, "f.value"), "42", "no leading zeros");
    js(&mut p, "press(k, 'Enter')");
    assert!(js(&mut p, "log.join()").ends_with("enter:42"), "{}", js(&mut p, "log.join()"));
    // The first digit after Enter starts again; the text the field already held is replaced too.
    js(&mut p, "press(k, '9')");
    assert_eq!(js(&mut p, "f.value"), "9");
    js(&mut p, "f.value = '120'; k.value = ''; press(k, '3')");
    assert_eq!(js(&mut p, "f.value"), "3");

    // Length limit, display, custom Enter label.
    js(&mut p, "press(n, '1', '2', '3', '4')");
    assert_eq!(js(&mut p, "[n.value, n.querySelector('.nile-keypad-display').textContent, n.querySelector('.nile-enter').textContent].join()"), "123,123,Go");
    js(&mut p, "press(pin, '1', '2', '3', '4', '5')");
    assert_eq!(js(&mut p, "[pin.value, pin.querySelector('.nile-keypad-display').textContent].join()"), "1234,●●●●");
    // Quick amounts add up.
    js(&mut p, "const q = document.createElement('nile-keypad'); q.setAttribute('keys', '5,10'); q.setAttribute('display', ''); document.body.appendChild(q); window.q = q");
    p.run_timers();
    js(&mut p, "press(q, '+5', '+10')");
    assert_eq!(js(&mut p, "q.value"), "15");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_date_widgets_pick_parse_and_apply_presets() {
    let (mut p, path) = page(
        r##"<body style="margin:0">
          <nile-calendar id="c" value="2026-10-24" min="2026-10-05" max="2026-10-28"></nile-calendar>
          <nile-calendar id="r" range month="2026-10"></nile-calendar>
          <nile-date id="d" value="2026-10-24" format="dd/MM/yyyy"></nile-date>
          <nile-date-range id="dr" start="2026-10-01" end="2026-10-31"></nile-date-range>
        <script type="module">
          import { formatDate, parseDate } from "@nile/ui";
          window.formatDate = formatDate; window.parseDate = parseDate;
          for (const id of ["c", "r", "d", "dr"]) window[id] = document.getElementById(id);
          window.log = [];
          c.addEventListener("change", (e) => log.push("c:" + e.detail.value));
          c.addEventListener("monthchange", (e) => log.push("m:" + e.detail.month));
          r.addEventListener("change", (e) => log.push("r:" + e.detail.start + ">" + e.detail.end));
          d.addEventListener("change", (e) => log.push("d:" + e.detail.value));
          dr.addEventListener("change", (e) => log.push("dr:" + e.detail.start + ">" + e.detail.end));
          window.day = (cal, iso) => cal.querySelector('.nile-cal-day[data-date="' + iso + '"]');
        </script></body>"##,
        "ui-date",
    );
    p.run_timers();
    p.run_timers();
    let none = ModifiersState::default();
    // Text formats both ways, and rubbish is refused.
    assert_eq!(js(&mut p, "formatDate('2026-10-24', 'dd/MM/yyyy') + '|' + formatDate('2026-03-04', 'MMM d, yyyy') + '|' + formatDate('2026-03-04', 'MMMM yy')"), "24/10/2026|Mar 4, 2026|March 26");
    assert_eq!(js(&mut p, "[parseDate('24/10/2026', 'dd/MM/yyyy'), parseDate('Oct 4, 2026', 'MMM d, yyyy'), parseDate('31/02/2026', 'dd/MM/yyyy'), parseDate('x', 'dd/MM/yyyy')].join()"), "2026-10-24,2026-10-04,,");

    // A month of days: title, 42 cells, Monday first, today's marks, min/max greyed out, the value selected.
    assert_eq!(js(&mut p, "c.querySelector('.nile-cal-title').textContent + '|' + c.querySelectorAll('.nile-cal-day').length + '|' + c.querySelector('.nile-cal-dow').textContent"), "October 2026|42|Mo");
    assert_eq!(js(&mut p, "[day(c, '2026-10-24').getAttribute('aria-selected'), day(c, '2026-10-03').disabled, day(c, '2026-10-29').disabled, day(c, '2026-10-10').disabled].join()"), "true,true,true,false");
    js(&mut p, "day(c, '2026-10-20').click()");
    assert_eq!(js(&mut p, "[c.value, log.join()].join('|')"), "2026-10-20|c:2026-10-20");
    js(&mut p, "day(c, '2026-10-03').click()");
    assert_eq!(js(&mut p, "c.value"), "2026-10-20", "a disabled day can't be picked");
    js(&mut p, "c.querySelector('.nile-cal-nav[data-dir=\"1\"]').click()");
    assert_eq!(js(&mut p, "c.querySelector('.nile-cal-title').textContent + '|' + log[log.length - 1]"), "November 2026|m:2026-11");
    // Keyboard: arrows move the focused day, Enter picks, PageUp goes back a month.
    js(&mut p, "c.value = '2026-10-20'");
    js(&mut p, "c.focus()");
    p.key("keydown", "ArrowRight".into(), none);
    p.key("keydown", "ArrowDown".into(), none);
    p.key("keydown", "Enter".into(), none);
    assert_eq!(js(&mut p, "c.value"), "2026-10-28");

    // A span: two clicks, the in-between days highlighted, the later click before the first swaps them.
    js(&mut p, "day(r, '2026-10-12').click(); day(r, '2026-10-05').click()");
    assert_eq!(js(&mut p, "[r.start, r.end, r.querySelectorAll('.nile-in-range').length, log[log.length - 1]].join()"), "2026-10-05,2026-10-12,6,r:2026-10-05>2026-10-12");

    // The date field: typed text in its format commits on Enter; a bad one is refused and flagged.
    assert_eq!(js(&mut p, "d.querySelector('input').value"), "24/10/2026");
    js(&mut p, "d.querySelector('input').focus(); d.querySelector('input').value = '25/12/2026'");
    p.key("keydown", "Enter".into(), none);
    assert_eq!(js(&mut p, "[d.value, d.querySelector('input').value, log[log.length - 1]].join()"), "2026-12-25,25/12/2026,d:2026-12-25");
    js(&mut p, "d.querySelector('input').value = 'soon'");
    p.key("keydown", "Enter".into(), none);
    assert_eq!(js(&mut p, "[d.value, d.querySelector('input').value, d.hasAttribute('invalid')].join()"), "2026-12-25,25/12/2026,true");
    js(&mut p, "d.querySelector('.nile-date-btn').click()");
    assert_eq!(js(&mut p, "d.querySelector('.nile-date-pop').classList.contains('nile-open') + ',' + d.querySelector('.nile-cal-title').textContent"), "true,December 2026");
    js(&mut p, "day(d, '2026-12-09').click()");
    assert_eq!(js(&mut p, "[d.value, d.querySelector('input').value, d.querySelector('.nile-date-pop').classList.contains('nile-open')].join()"), "2026-12-09,09/12/2026,false");

    // The range field: shows both ends, a preset fills it.
    assert_eq!(js(&mut p, "dr.querySelector('.nile-date-text').textContent"), "2026-10-01 – 2026-10-31");
    js(&mut p, "dr.querySelector('.nile-date-box').click()");
    assert_eq!(js(&mut p, "dr.querySelectorAll('.nile-date-preset').length > 4"), "true");
    js(&mut p, "dr.querySelectorAll('.nile-date-preset')[2].click()");
    assert_eq!(js(&mut p, "Math.round((new Date(dr.end) - new Date(dr.start)) / 864e5)"), "6", "Last 7 days");
    assert!(js(&mut p, "log[log.length - 1]").starts_with("dr:"));
    js(&mut p, "dr.setRange('2026-11-10', '2026-11-02')");
    assert_eq!(js(&mut p, "dr.start + '>' + dr.end"), "2026-11-02>2026-11-10");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_menu_and_palette_choose_by_keyboard_and_pointer() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><div id="zone" style="width:200px;height:100px"></div>
        <script type="module">
          import { menu, contextMenu, palette, shortcuts, fuzzy } from "@nile/ui";
          window.menu = menu; window.palette = palette; window.fuzzy = fuzzy; window.shortcuts = shortcuts;
          window.log = [];
          window.items = () => [{ label: "Open", id: "open", shortcut: "Mod+O", run: () => log.push("run-open") }, { label: "Recent", id: "recent", items: [{ label: "a.txt", id: "a" }, { label: "b.txt", id: "b" }] }, { separator: true }, { label: "Wrap", id: "wrap", checked: true }, { label: "Delete", id: "del", disabled: true }];
          contextMenu(document.getElementById("zone"), () => items());
          palette.add({ id: "new-sale", label: "New sale", shortcut: "F8", run: () => log.push("sale") });
          palette.add({ id: "stock", label: "Open stock register", hint: "Stock", run: () => log.push("stock") });
          shortcuts.on("Mod+S", () => log.push("save"), { label: "Save document" });
          palette.bind("Mod+K");
          window.rows = () => Array.from(document.querySelectorAll(".nile-menu-row")).map((r) => r.querySelector(".nile-menu-label").textContent).join();
        </script></body>"##,
        "ui-menu",
    );
    p.run_timers();
    p.run_timers();
    let none = ModifiersState::default();
    let ctrl = if cfg!(target_os = "macos") { ModifiersState::SUPER } else { ModifiersState::CONTROL };
    let key = |p: &mut Page, k: &str, m: ModifiersState| p.key("keydown", k.into(), m);

    // A menu from data: rows, a check mark, a disabled row, a submenu; Down/Down/Right/Enter chooses a.txt.
    js(&mut p, "menu(items(), { x: 20, y: 20 }).then((c) => log.push('chose:' + (c && c.id)))");
    assert_eq!(js(&mut p, "rows()"), "Open,Recent,Wrap,Delete");
    assert_eq!(js(&mut p, "[document.querySelector('[aria-checked]').getAttribute('aria-checked'), document.querySelector('[aria-disabled]').textContent.indexOf('Delete') >= 0].join()"), "true,true");
    key(&mut p, "ArrowDown", none);
    key(&mut p, "ArrowDown", none);
    key(&mut p, "ArrowRight", none);
    assert_eq!(js(&mut p, "document.querySelectorAll('.nile-menu-popup').length"), "2");
    key(&mut p, "ArrowDown", none);
    key(&mut p, "Enter", none);
    p.run_timers();
    assert_eq!(js(&mut p, "[log.join(), document.querySelectorAll('.nile-menu-popup').length].join('|')"), "chose:b|0");
    // Disabled rows don't choose; a click runs the item's own handler; Escape dismisses with null.
    js(&mut p, "log.length = 0; menu(items(), { x: 20, y: 20 }).then((c) => log.push('chose:' + (c && c.id)))");
    js(&mut p, "Array.from(document.querySelectorAll('.nile-menu-row')).find((r) => r.textContent.indexOf('Delete') >= 0).click()");
    assert_eq!(js(&mut p, "document.querySelectorAll('.nile-menu-popup').length"), "1");
    js(&mut p, "document.querySelector('.nile-menu-row').click()");
    p.run_timers();
    assert_eq!(js(&mut p, "log.join()"), "run-open,chose:open");
    js(&mut p, "log.length = 0; menu(items(), { x: 5, y: 5 }).then((c) => log.push('chose:' + (c && c.id)))");
    key(&mut p, "Escape", none);
    p.run_timers();
    assert_eq!(js(&mut p, "log.join()"), "chose:null");
    // The right button over an element opens it.
    js(&mut p, "document.getElementById('zone').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 30, clientY: 30 }))");
    assert_eq!(js(&mut p, "rows()"), "Open,Recent,Wrap,Delete");
    key(&mut p, "Escape", none);

    // The palette: Mod+K opens it with commands and labelled shortcuts; fuzzy typing filters; Enter runs.
    key(&mut p, "k", ctrl);
    assert_eq!(js(&mut p, "palette.isOpen + ',' + document.querySelectorAll('.nile-palette-row').length"), "true,3");
    assert_eq!(js(&mut p, "document.querySelector('.nile-palette-key') ? 'keys' : 'none'"), "keys");
    js(&mut p, "const i = document.querySelector('.nile-palette-input'); i.value = 'osr'; i.dispatchEvent(new Event('input', { bubbles: true }))");
    assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('.nile-palette-row')).map((r) => r.querySelector('.nile-palette-label').textContent).join()"), "Open stock register");
    assert_eq!(js(&mut p, "document.querySelectorAll('.nile-palette-match').length"), "3");
    key(&mut p, "Enter", none);
    assert_eq!(js(&mut p, "[log.join(), palette.isOpen].join('|')"), "chose:null,stock|false");
    // Shortcuts stay quiet while it is open, and Mod+K closes it again.
    key(&mut p, "k", ctrl);
    key(&mut p, "s", ctrl);
    assert!(!js(&mut p, "log.join()").contains("save"), "{}", js(&mut p, "log.join()"));
    key(&mut p, "k", ctrl);
    assert_eq!(js(&mut p, "palette.isOpen"), "false");
    // A picker over your own items resolves with the chosen one.
    js(&mut p, "palette.open({ items: [{ label: 'Ada Lovelace', data: 1 }, { label: 'Alan Turing', data: 2 }], placeholder: 'Find' }).then((c) => log.push('picked:' + (c && c.data)))");
    js(&mut p, "const j = document.querySelector('.nile-palette-input'); j.value = 'tur'; j.dispatchEvent(new Event('input', { bubbles: true }))");
    key(&mut p, "Enter", none);
    p.run_timers();
    assert!(js(&mut p, "log.join()").ends_with("picked:2"), "{}", js(&mut p, "log.join()"));
    // The matcher itself: a run beats scattered letters; non-matches are null.
    assert_eq!(js(&mut p, "[fuzzy('abc', 'xabcx').score > fuzzy('abc', 'a-b-c').score, fuzzy('zz', 'abc') === null].join()"), "true,true");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_wizard_validates_steps_and_finishes() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><nile-wizard id="w" linear cancel-label="Cancel" style="width:500px;height:300px">
          <nile-step label="Company" description="Name"><input id="name"></nile-step>
          <nile-step label="Taxes" optional><p>Taxes</p></nile-step>
          <nile-step label="Hidden" disabled><p>nope</p></nile-step>
          <nile-step label="Done"><p>All set</p></nile-step>
        </nile-wizard>
        <script type="module">
          import "@nile/ui";
          window.w = document.getElementById("w");
          window.log = [];
          w.addEventListener("stepchange", (e) => log.push(e.detail.from + ">" + e.detail.to));
          w.addEventListener("finish", () => log.push("finish"));
          w.addEventListener("cancel", () => log.push("cancel"));
          window.ready = false;
          w.steps[0].validate = () => (document.getElementById("name").value ? true : "Enter a company name");
          w.steps[1].validate = () => new Promise((r) => setTimeout(() => r(true), 0));
          window.btn = (cls) => w.querySelector(".nile-wizard-" + cls);
        </script></body>"##,
        "ui-wizard",
    );
    p.run_timers();
    p.run_timers();
    let settle = |p: &mut Page| {
        for _ in 0..3 {
            std::thread::sleep(Duration::from_millis(20));
            p.run_timers();
        }
    };
    assert_eq!(js(&mut p, "[w.selectedIndex, w.querySelectorAll('.nile-wizard-dot').length, btn('back').disabled, btn('next').textContent].join()"), "0,4,true,Next");
    assert_eq!(js(&mut p, "w.querySelectorAll('.nile-wizard-dot')[1].querySelector('.nile-wizard-sub').textContent"), "Optional");
    // Validation keeps you on the step and shows its text; a linear header won't jump ahead.
    js(&mut p, "btn('next').click()");
    settle(&mut p);
    assert_eq!(js(&mut p, "[w.selectedIndex, w.querySelector('.nile-wizard-error').textContent].join()"), "0,Enter a company name");
    js(&mut p, "w.querySelectorAll('.nile-wizard-dot')[3].click()");
    settle(&mut p);
    assert_eq!(js(&mut p, "w.selectedIndex"), "0");
    js(&mut p, "document.getElementById('name').value = 'SoftSale'; btn('next').click()");
    settle(&mut p);
    assert_eq!(js(&mut p, "[w.selectedIndex, w.querySelector('.nile-wizard-error').textContent, log.join()].join('|')"), "1||0>1");
    // Disabled steps are skipped, in both directions; the last step offers Finish.
    js(&mut p, "btn('next').click()");
    settle(&mut p);
    assert_eq!(js(&mut p, "[w.selectedIndex, btn('next').textContent, w.querySelectorAll('.nile-wizard-dot.nile-done').length].join()"), "3,Finish,2");
    js(&mut p, "btn('back').click()");
    settle(&mut p);
    assert_eq!(js(&mut p, "w.selectedIndex"), "1");
    // Visited steps can be clicked in the header; Finish and Cancel raise their events.
    js(&mut p, "w.querySelectorAll('.nile-wizard-dot')[3].click()");
    settle(&mut p);
    js(&mut p, "btn('next').click()");
    settle(&mut p);
    js(&mut p, "btn('cancel').click()");
    assert_eq!(js(&mut p, "log.slice(-2).join()"), "finish,cancel");
    js(&mut p, "w.reset()");
    assert_eq!(js(&mut p, "[w.selectedIndex, btn('back').disabled].join()"), "0,true");
    let _ = std::fs::remove_file(path);
}

#[test]
fn removing_the_focused_element_sends_keys_to_the_document_again() {
    let (mut p, path) = page(
        r##"<body><div id="host"><button id="b">x</button></div>
        <script>window.keys = []; document.addEventListener("keydown", (e) => keys.push(e.key));</script></body>"##,
        "focus-removed",
    );
    js(&mut p, "document.getElementById('b').focus()");
    assert_eq!(js(&mut p, "document.activeElement.id"), "b");
    js(&mut p, "document.getElementById('host').innerHTML = ''");
    p.sync();
    assert_eq!(js(&mut p, "document.activeElement === document.body || document.activeElement === null"), "true");
    p.key("keydown", "q".into(), ModifiersState::default());
    assert_eq!(js(&mut p, "keys.join()"), "q", "the key reaches the document");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_escpos_receipt_builds_the_right_bytes() {
    let (mut p, path) = page(
        r##"<body><script type="module">
          import { Receipt, encodeText, printReceipt } from "@nile/escpos";
          window.Receipt = Receipt; window.encodeText = encodeText; window.printReceipt = printReceipt;
          window.hex = (r) => r.hex();
        </script></body>"##,
        "escpos",
    );
    p.run_timers();
    p.run_timers();
    // Setup: initialise + code page; text and styles.
    assert_eq!(js(&mut p, "new Receipt().hex()"), "1b 40 1b 74 00");
    assert_eq!(js(&mut p, "new Receipt({ init: false }).align('center').bold().line('Hi').hex()"), "1b 61 01 1b 45 01 48 69 0a");
    assert_eq!(js(&mut p, "new Receipt({ init: false }).size(2, 2).size(1, 1).underline().invert().font('B').hex()"), "1d 21 11 1d 21 00 1b 2d 01 1d 42 01 1b 4d 01");
    // Layout helpers pad to the width; double-width halves it.
    assert_eq!(js(&mut p, "new Receipt({ init: false, width: 10 }).row('A', '1.00').preview()"), "A     1.00");
    assert_eq!(js(&mut p, "new Receipt({ init: false, width: 10 }).size(2, 1).row('A', '1').preview()"), "A   1");
    assert_eq!(js(&mut p, "new Receipt({ init: false, width: 12 }).columns([{ text: 'Milk', width: 6 }, { text: 'x2', width: 2, align: 'right' }, { text: '5.00', align: 'right' }]).preview()"), "Milk  x25.00");
    assert_eq!(js(&mut p, "new Receipt({ init: false, width: 4 }).rule('=').preview()"), "====");
    assert_eq!(js(&mut p, "new Receipt({ init: false, width: 12 }).align('center').line('ABCD').preview()"), "    ABCD");
    // Code pages: é, ñ, the euro sign (only in 858/1252), and "?" for what a page lacks.
    assert_eq!(js(&mut p, "[encodeText('é', 'cp437'), encodeText('ñ', 'cp437'), encodeText('€', 'cp858'), encodeText('€', 'cp437'), encodeText('é€', 'cp1252'), encodeText('日', 'cp437'), encodeText('Ab', 'ascii')].map((a) => a.join('.')).join('|')"), "130|164|213|63|233.128|63|65.98");
    assert_eq!(js(&mut p, "new Receipt({ codepage: 'cp858' }).hex()"), "1b 40 1b 74 13");
    // Barcode, QR, image, cut, drawer, beep.
    assert_eq!(js(&mut p, "new Receipt({ init: false }).barcode('code128', '123', { height: 60, width: 2 }).hex()"), "1d 68 3c 1d 77 02 1d 48 02 1d 6b 49 05 7b 42 31 32 33");
    assert_eq!(js(&mut p, "new Receipt({ init: false }).barcode('ean13', '5901234123457', { text: 'none' }).hex()"), "1d 68 50 1d 77 03 1d 48 00 1d 6b 43 0d 35 39 30 31 32 33 34 31 32 33 34 35 37");
    assert_eq!(js(&mut p, "new Receipt({ init: false }).qr('AB', { size: 5 }).hex()"), "1d 28 6b 04 00 31 41 32 00 1d 28 6b 03 00 31 43 05 1d 28 6b 03 00 31 45 31 1d 28 6b 05 00 31 50 30 41 42 1d 28 6b 03 00 31 51 30");
    assert_eq!(js(&mut p, "new Receipt({ init: false }).image({ width: 8, height: 2, data: new Uint8ClampedArray(8 * 2 * 4).fill(0).map((v, i) => (i % 4 === 3 ? 255 : 0)) }).hex()"), "1d 76 30 00 01 00 02 00 ff ff");
    assert_eq!(js(&mut p, "new Receipt({ init: false }).image({ width: 3, height: 1, data: Uint8ClampedArray.from([0,0,0,255, 255,255,255,255, 0,0,0,255]) }).hex()"), "1d 76 30 00 01 00 01 00 a0");
    assert_eq!(js(&mut p, "new Receipt({ init: false }).feed(2).cut().cut({ partial: true }).drawer().drawer(1).beep(2).hex()"), "1b 64 02 1d 56 41 03 1d 56 42 03 1b 70 00 19 fa 1b 70 01 19 fa 1b 42 02 02");
    assert_eq!(js(&mut p, "(() => { try { new Receipt().barcode('nope', '1'); return 'no error'; } catch (e) { return 'threw'; } })()"), "threw");
    assert_eq!(js(&mut p, "new Receipt({ init: false }).text('a\\nb').preview()"), "a\nb");
    // A file target writes the bytes.
    let out = std::env::temp_dir().join(format!("nile-escpos-{}.bin", std::process::id()));
    js(&mut p, &format!("window.done = null; printReceipt(new Receipt().line('OK').cut(), {{ file: {:?} }}).then(() => (done = 'ok'), (e) => (done = 'err ' + e.message))", out.to_string_lossy()));
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(20));
        p.run_timers();
    }
    assert_eq!(js(&mut p, "done"), "ok");
    assert_eq!(std::fs::read(&out).unwrap(), [0x1b, 0x40, 0x1b, 0x74, 0, b'O', b'K', 0x0a, 0x1d, 0x56, 0x41, 3]);
    let _ = std::fs::remove_file(out);
    let _ = std::fs::remove_file(path);
}

#[cfg(unix)]
static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(unix)]
#[test]
fn the_print_module_lists_printers_and_sends_jobs_through_cups_commands() {
    use std::os::unix::fs::PermissionsExt;
    // These tests change the process's PATH: one at a time.
    let _path = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("nile-print-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let script = |name: &str, body: &str| {
        let f = dir.join(name);
        std::fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    script(
        "lpstat",
        "echo 'printer Office is idle.  enabled since Thu 01 Jan 2026'\necho 'printer Kitchen disabled since Thu 01 Jan 2026 -'\necho '\treason unknown'\necho 'printer TM-T20 now printing TM-T20-5.  enabled since Thu 01 Jan 2026'\necho 'system default destination: Office'",
    );
    let log = dir.join("calls.txt");
    script("lp", &format!("echo \"$@\" >> '{}'\ncat > '{}.stdin' 2>/dev/null\necho 'request id is X-1 (1 file(s))'", log.display(), log.display()));
    script("lpfail", "echo boom >&2\nexit 1");
    let (mut p, path) = page(
        &format!(
            r##"<body><script type="module">
          import {{ printers, printFile, printRaw, sendNetwork, parseLpstat }} from "@nile/print";
          import {{ setenv, getenv }} from "@sys";
          window.parseLpstat = parseLpstat;
          setenv("PATH", {dir:?} + ":" + getenv("PATH"));
          window.out = [];
          window.step = async () => {{
            out.push(JSON.stringify(await printers()));
            await printFile("/tmp/invoice.pdf", {{ printer: "Office", copies: 2, title: "Invoice 7" }});
            await printFile("/tmp/other.pdf");
            await printRaw("TM-T20", new Uint8Array([27, 64, 72, 105, 10]));
            try {{ await printRaw("", [1]); }} catch (e) {{ out.push("empty: " + e.message); }}
            out.push("done");
          }};
          step().catch((e) => out.push("ERR " + e.message));
        </script></body>"##,
            dir = dir.to_string_lossy()
        ),
        "print",
    );
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(30));
        p.run_timers();
        if js(&mut p, "out.includes('done')") == "true" {
            break;
        }
    }
    let out = js(&mut p, "out.join('\\n')");
    assert!(out.contains("done"), "{out}");
    assert!(out.contains(r#"[{"name":"Office","default":true,"status":"idle"},{"name":"Kitchen","default":false,"status":"offline"},{"name":"TM-T20","default":false,"status":"printing"}]"#), "{out}");
    assert!(out.contains("empty: printRaw needs a printer name"), "{out}");
    let calls = std::fs::read_to_string(&log).unwrap();
    assert_eq!(calls.lines().collect::<Vec<_>>(), ["-d Office -n 2 -t Invoice 7 /tmp/invoice.pdf", "/tmp/other.pdf", "-d TM-T20 -o raw -t Nile receipt"]);
    assert_eq!(std::fs::read(format!("{}.stdin", log.display())).unwrap(), [27, 64, 72, 105, 10], "the raw bytes reach lp's standard input");

    // The parser alone, and a failing command surfaces its message.
    assert_eq!(js(&mut p, "JSON.stringify(parseLpstat('system default destination: B\\nprinter A is idle.\\nprinter B is idle.'))"), r#"[{"name":"A","default":false,"status":"idle"},{"name":"B","default":true,"status":"idle"}]"#);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_print_module_sends_raw_bytes_to_a_network_printer() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let (mut s, _) = listener.accept().unwrap();
        let mut got = Vec::new();
        let _ = s.read_to_end(&mut got);
        let _ = tx.send(got);
    });
    let (mut p, path) = page(
        &format!(
            r##"<body><script type="module">
          import {{ Receipt, printReceipt }} from "@nile/escpos";
          window.done = null;
          printReceipt(new Receipt().line("Net").cut(), {{ host: "127.0.0.1", port: {port} }}).then(() => (done = "sent"), (e) => (done = "err " + e.message));
        </script></body>"##
        ),
        "print-net",
    );
    for _ in 0..80 {
        std::thread::sleep(Duration::from_millis(25));
        p.run_timers();
        if js(&mut p, "done") != "null" {
            break;
        }
    }
    assert_eq!(js(&mut p, "done"), "sent");
    let got = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(got, [0x1b, 0x40, 0x1b, 0x74, 0, b'N', b'e', b't', 0x0a, 0x1d, 0x56, 0x41, 3]);
    // Nothing listening: the promise rejects instead of hanging.
    let (mut p2, path2) = page(
        r##"<body><script type="module">
          import { sendNetwork } from "@nile/print";
          window.done = null;
          sendNetwork("127.0.0.1", 1, [1], { timeout: 1500 }).then(() => (done = "sent"), (e) => (done = "err"));
        </script></body>"##,
        "print-net-down",
    );
    for _ in 0..120 {
        std::thread::sleep(Duration::from_millis(25));
        p2.run_timers();
        if js(&mut p2, "done") != "null" {
            break;
        }
    }
    assert_eq!(js(&mut p2, "done"), "err");
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path2);
}

#[cfg(unix)]
#[test]
fn window_print_writes_the_page_as_a_pdf_and_hands_it_to_the_printer() {
    use std::os::unix::fs::PermissionsExt;
    let _path = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("nile-winprint-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("calls.txt");
    let lp = dir.join("lp");
    std::fs::write(&lp, format!("#!/bin/sh\necho \"$@\" >> '{}'\ncp \"$(eval echo \\${{$#}})\" '{}.pdf'\n", log.display(), log.display())).unwrap();
    std::fs::set_permissions(&lp, std::fs::Permissions::from_mode(0o755)).unwrap();
    let saved = dir.join("saved.pdf");
    let (mut p, path) = page(
        &format!(
            r##"<html><head><title>Invoice 7</title><style>@page {{ size: 200px 300px; margin: 10px }}</style></head><body><h1>Invoice</h1><p>Total 8.80</p>
        <script type="module">
          import {{ setenv, getenv }} from "@sys";
          setenv("PATH", {dir:?} + ":" + getenv("PATH"));
          window.out = [];
          window.w0 = window.innerWidth;
          (async () => {{
            out.push("export:" + (await Window.this.exportPdf({saved:?})));
            out.push("print:" + (await Window.this.print({{ printer: "Office", copies: 2 }})));
            try {{ await Window.this.exportPdf("/nonexistent-dir/x.pdf"); }} catch (e) {{ out.push("failed"); }}
          }})().catch((e) => out.push("ERR " + e.message));
        </script></body></html>"##,
            dir = dir.to_string_lossy(),
            saved = saved.to_string_lossy()
        ),
        "window-print",
    );
    for _ in 0..80 {
        std::thread::sleep(Duration::from_millis(25));
        p.run_timers();
        if js(&mut p, "out.includes('failed')") == "true" {
            break;
        }
    }
    let out = js(&mut p, "out.join('\\n')");
    assert!(out.contains("print:true") && out.contains("failed"), "{out}");
    let head = std::fs::read(&saved).unwrap();
    assert!(head.starts_with(b"%PDF"), "a PDF was written");
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.starts_with("-d Office -n 2 "), "{calls}");
    assert!(std::fs::read(format!("{}.pdf", log.display())).unwrap().starts_with(b"%PDF"), "lp was given the PDF");
    assert_eq!(js(&mut p, "window.innerWidth === w0"), "true", "the window's own size is untouched");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(path);
}

#[test]
fn web_crypto_hashes_signs_derives_and_encrypts_with_the_published_vectors() {
    let (mut p, path) = page(
        r##"<body><script type="module">
          import { hashPassword, verifyPassword, needsRehash, sha256, hmac, encryptText, decryptText } from "@nile/crypto";
          const enc = (t) => new TextEncoder().encode(t);
          const hx = (b) => Array.from(new Uint8Array(b), (x) => (x < 16 ? "0" : "") + x.toString(16)).join("");
          const s = crypto.subtle;
          window.out = [];
          const note = (k, v) => out.push(k + "=" + v);
          (async () => {
            note("sha256", hx(await s.digest("SHA-256", enc("abc"))));
            note("sha1", hx(await s.digest({ name: "SHA-1" }, enc("abc"))));
            note("helper", await sha256("abc"));
            const hk = await s.importKey("raw", enc("key"), { name: "HMAC", hash: "SHA-256" }, false, ["sign", "verify"]);
            const tag = await s.sign("HMAC", hk, enc("The quick brown fox jumps over the lazy dog"));
            note("hmac", hx(tag));
            note("verify", [await s.verify("HMAC", hk, tag, enc("The quick brown fox jumps over the lazy dog")), await s.verify("HMAC", hk, tag, enc("other"))].join());
            note("hmac2", await hmac("key", "The quick brown fox jumps over the lazy dog"));
            const pk = await s.importKey("raw", enc("password"), "PBKDF2", false, ["deriveBits", "deriveKey"]);
            note("pbkdf2", hx(await s.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt: enc("salt"), iterations: 4096 }, pk, 256)));
            const zero = await s.importKey("raw", new Uint8Array(16), "AES-GCM", true, ["encrypt", "decrypt"]);
            note("gcm", hx(await s.encrypt({ name: "AES-GCM", iv: new Uint8Array(12) }, zero, new Uint8Array(16))));
            const key = await s.generateKey({ name: "AES-GCM", length: 256 }, true, ["encrypt", "decrypt"]);
            const iv = crypto.getRandomValues(new Uint8Array(12));
            const sealed = await s.encrypt({ name: "AES-GCM", iv, additionalData: enc("ctx") }, key, enc("hello"));
            note("roundtrip", new TextDecoder().decode(await s.decrypt({ name: "AES-GCM", iv, additionalData: enc("ctx") }, key, sealed)));
            try { await s.decrypt({ name: "AES-GCM", iv, additionalData: enc("other") }, key, sealed); note("tamper", "accepted"); } catch (e) { note("tamper", e.name); }
            try { await s.digest("MD5", enc("x")); } catch (e) { note("md5", e.name); }
            note("exported", (await s.exportKey("raw", key)).byteLength);
            const derived = await s.deriveKey({ name: "PBKDF2", hash: "SHA-256", salt: enc("s"), iterations: 10 }, pk, { name: "AES-GCM", length: 128 }, false, ["encrypt"]);
            note("derived", derived.algorithm.name + derived.algorithm.length + derived.extractable);
            // helpers
            const stored = await hashPassword("correct horse", { iterations: 2000 });
            note("stored", stored.split("$").length + ":" + stored.split("$").slice(0, 2).join("$"));
            note("verify-pw", [await verifyPassword("correct horse", stored), await verifyPassword("wrong", stored), await verifyPassword("x", "garbage")].join());
            note("rehash", [needsRehash(stored), needsRehash(stored, { iterations: 1000 })].join());
            const box = await encryptText("pw", "secret note", { iterations: 1000 });
            note("text", await decryptText("pw", box));
            try { await decryptText("nope", box); } catch (e) { note("badpw", e.name); }
            out.push("done");
          })().catch((e) => out.push("ERR " + e.name + " " + e.message));
          // sync parts
          const a = crypto.getRandomValues(new Uint8Array(32)), b = crypto.getRandomValues(new Uint8Array(32));
          window.sync = [a.some((x) => x !== 0), a.join() !== b.join(), /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(crypto.randomUUID()), crypto.randomUUID() !== crypto.randomUUID()].join();
          try { crypto.getRandomValues(new Float32Array(2)); window.sync += ",float-ok"; } catch (e) { window.sync += "," + e.name; }
          try { crypto.getRandomValues(new Uint8Array(70000)); } catch (e) { window.sync += "," + e.name; }
        </script></body>"##,
        "web-crypto",
    );
    for _ in 0..200 {
        std::thread::sleep(Duration::from_millis(30));
        p.run_timers();
        if js(&mut p, "out.includes('done') || out.some((l) => l.startsWith('ERR'))") == "true" {
            break;
        }
    }
    let out = js(&mut p, "out.join('\\n')");
    for line in [
        "sha256=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "sha1=a9993e364706816aba3e25717850c26c9cd0d89d",
        "helper=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "hmac=f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8",
        "verify=true,false",
        "hmac2=f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8",
        "pbkdf2=c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a",
        "gcm=0388dace60b6a392f328c2b971b2fe78ab6e47d42cec13bdf53a67b21257bddf",
        "roundtrip=hello",
        "tamper=OperationError",
        "md5=NotSupportedError",
        "exported=32",
        "derived=AES-GCM128false",
        "stored=4:pbkdf2-sha256$2000",
        "verify-pw=true,false,false",
        "rehash=true,false",
        "text=secret note",
        "badpw=OperationError",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line:?} in:\n{out}");
    }
    assert_eq!(js(&mut p, "sync"), "true,true,true,true,TypeMismatchError,QuotaExceededError");
    let _ = std::fs::remove_file(path);
}

#[test]
fn intl_formats_numbers_currencies_dates_and_collates_like_icu() {
    let (mut p, path) = page(r##"<body><script>window.d = new Date(2026, 9, 24, 15, 5, 9);</script></body>"##, "intl");
    let check = |p: &mut Page, code: &str, want: &str| assert_eq!(js(p, code), want, "{code}");
    let nb = "\u{a0}";
    let nn = "\u{202f}";
    // numbers
    check(&mut p, "(1234567.891).toLocaleString('en-US')", "1,234,567.891");
    check(&mut p, "(1234567.891).toLocaleString('de-DE')", "1.234.567,891");
    check(&mut p, "(1234567.891).toLocaleString('fr-FR')", &format!("1{nn}234{nn}567,891"));
    check(&mut p, "(1234567.891).toLocaleString('en-IN')", "12,34,567.891");
    check(&mut p, "[(1234.5).toLocaleString('es-ES'), (12345.5).toLocaleString('es-ES')].join('|')", "1234,5|12.345,5");
    check(&mut p, "(1.005).toLocaleString('en-US', { maximumFractionDigits: 2 })", "1.01");
    check(&mut p, "(2.5).toLocaleString('en-US', { maximumFractionDigits: 0 })", "3");
    check(&mut p, "(0.1 + 0.2).toLocaleString('en-US', { maximumFractionDigits: 2 })", "0.3");
    check(&mut p, "(0.256).toLocaleString('en-US', { style: 'percent' })", "26%");
    check(&mut p, "(5).toLocaleString('en-US', { minimumIntegerDigits: 3 })", "005");
    check(&mut p, "(0).toLocaleString('en-US', { minimumFractionDigits: 2 })", "0.00");
    check(&mut p, "(123456).toLocaleString('en-US', { maximumSignificantDigits: 3 })", "123,000");
    check(&mut p, "(1234).toLocaleString('en-US', { notation: 'compact' })", "1.2K");
    check(&mut p, "(1234567).toLocaleString('en-US', { notation: 'compact', compactDisplay: 'long' })", "1.2 million");
    check(&mut p, "(1234).toLocaleString('en-US', { useGrouping: false })", "1234");
    check(&mut p, "[(5).toLocaleString('en-US', { signDisplay: 'always' }), (-0.5).toLocaleString('en-US'), NaN.toLocaleString('en-US')].join('|')", "+5|-0.5|NaN");
    // currencies
    check(&mut p, "(1234.5).toLocaleString('en-US', { style: 'currency', currency: 'USD' })", "$1,234.50");
    check(&mut p, "(-1234.5).toLocaleString('en-US', { style: 'currency', currency: 'USD' })", "-$1,234.50");
    check(&mut p, "(1234.5).toLocaleString('de-DE', { style: 'currency', currency: 'EUR' })", &format!("1.234,50{nb}€"));
    check(&mut p, "(1234.5).toLocaleString('en-UG', { style: 'currency', currency: 'UGX' })", &format!("USh{nb}1,235"));
    check(&mut p, "(1234.5).toLocaleString('en-US', { style: 'currency', currency: 'JPY' })", "¥1,235");
    check(&mut p, "(1234.5).toLocaleString('en-US', { style: 'currency', currency: 'CHF' })", &format!("CHF{nb}1,234.50"));
    check(&mut p, "(1234.5).toLocaleString('en-US', { style: 'currency', currency: 'EUR', currencyDisplay: 'code' })", &format!("EUR{nb}1,234.50"));
    check(&mut p, "new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' }).formatToParts(1.5).map((x) => x.type).join()", "currency,integer,decimal,fraction");
    check(&mut p, "(() => { try { (1).toLocaleString('en-US', { style: 'currency' }); } catch (e) { return e.name; } })()", "TypeError");
    // dates
    check(&mut p, "[d.toLocaleDateString('en-US'), d.toLocaleDateString('en-GB'), d.toLocaleDateString('de-DE'), d.toLocaleDateString('fr-FR')].join('|')", "10/24/2026|24/10/2026|24.10.2026|24/10/2026");
    check(&mut p, "d.toLocaleTimeString('en-US')", &format!("3:05:09{nn}PM"));
    check(&mut p, "d.toLocaleTimeString('de-DE')", "15:05:09");
    check(&mut p, "d.toLocaleString('en-US')", &format!("10/24/2026, 3:05:09{nn}PM"));
    check(&mut p, "d.toLocaleDateString('en-US', { dateStyle: 'full' })", "Saturday, October 24, 2026");
    check(&mut p, "d.toLocaleDateString('de-DE', { dateStyle: 'long' })", "24. Oktober 2026");
    check(&mut p, "d.toLocaleDateString('fr-FR', { dateStyle: 'full' })", "samedi 24 octobre 2026");
    check(&mut p, "d.toLocaleDateString('es-ES', { dateStyle: 'long' })", "24 de octubre de 2026");
    check(&mut p, "d.toLocaleDateString('en-US', { year: 'numeric', month: 'long', day: 'numeric' })", "October 24, 2026");
    check(&mut p, "d.toLocaleDateString('en-GB', { year: 'numeric', month: 'long', day: 'numeric' })", "24 October 2026");
    check(&mut p, "d.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' })", "Sat, Oct 24");
    check(&mut p, "d.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' })", &format!("03:05{nn}PM"));
    check(&mut p, "d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })", "15:05");
    check(&mut p, "d.toLocaleTimeString('en-US', { hour12: false })", "15:05:09");
    check(&mut p, "new Intl.DateTimeFormat('en-US', { timeZone: 'UTC', dateStyle: 'short' }).format(new Date(Date.UTC(2026, 0, 5, 23, 30)))", "1/5/26");
    check(&mut p, "(() => { try { d.toLocaleDateString('xx-invalid-locale-!'); } catch (e) { return e.name; } })()", "RangeError");
    // plural, list, relative, collation
    check(&mut p, "[new Intl.PluralRules('en').select(1), new Intl.PluralRules('en').select(2), new Intl.PluralRules('en', { type: 'ordinal' }).select(22), new Intl.PluralRules('ru').select(3), new Intl.PluralRules('fr').select(0)].join()", "one,other,two,few,one");
    check(&mut p, "[new Intl.ListFormat('en').format(['a', 'b', 'c']), new Intl.ListFormat('en', { type: 'disjunction' }).format(['a', 'b']), new Intl.ListFormat('fr').format(['a', 'b', 'c'])].join('|')", "a, b, and c|a or b|a, b et c");
    check(&mut p, "[new Intl.RelativeTimeFormat('en', { numeric: 'auto' }).format(-1, 'day'), new Intl.RelativeTimeFormat('en').format(3, 'hours'), new Intl.RelativeTimeFormat('en').format(-2, 'day')].join('|')", "yesterday|in 3 hours|2 days ago");
    check(&mut p, "['item 10', 'item 2', 'Item 1', 'élan', 'eclair', 'zebra', 'Zebra'].sort((a, b) => a.localeCompare(b, undefined, { numeric: true })).join()", "eclair,élan,Item 1,item 2,item 10,zebra,Zebra");
    check(&mut p, "['a'.localeCompare('B'), 'a'.localeCompare('A'), 'é'.localeCompare('e', undefined, { sensitivity: 'base' }), 'é'.localeCompare('e', undefined, { sensitivity: 'accent' })].join()", "-1,-1,0,1");
    check(&mut p, "new Intl.NumberFormat('de-DE').resolvedOptions().locale + '|' + Intl.getCanonicalLocales('en_us')", "de-DE|en-US");
    let _ = std::fs::remove_file(path);
}

// Linux only: macOS refuses to open a pseudo-terminal slave by name (ENOTTY).
#[cfg(target_os = "linux")]
#[test]
fn the_serial_module_talks_to_a_port_and_buffers_what_arrives() {
    use std::io::{Read, Write};
    let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
    let name = serialport::SerialPort::name(&slave).expect("slave name");
    let (mut p, path) = page(
        &format!(
            r#"<body><script type="module">
              import serial from "@nile/serial";
              window.log = []; window.ready = 0; window.done = 0;
              (async () => {{
                const port = await serial.open("{name}", {{ baudRate: 115200 }});
                window.ready = 1;
                await port.write("hello\n");
                const line = await port.readUntil("\n", {{ timeout: 4000 }});
                log.push("line:" + new TextDecoder().decode(line).trim());
                const four = await port.read(4, {{ timeout: 4000 }});
                log.push("four:" + new TextDecoder().decode(four));
                let timed = "none";
                try {{ await port.read(1, {{ timeout: 150 }}); }} catch (e) {{ timed = e.code; }}
                log.push("timeout:" + timed);
                const heard = [];
                port.onData((b) => heard.push(new TextDecoder().decode(b)));
                window.ready = 2;
                await new Promise((r) => setTimeout(r, 800));
                log.push("data:" + heard.join(""));
                // A pseudo-terminal has no modem lines (ENOTTY); a real port answers.
                try {{ await port.getSignals(); }} catch (e) {{ if (e.code !== "ENOTTY" && e.code !== "EIO") throw e; }}
                await port.close();
                let after = "none";
                try {{ await port.write("x"); }} catch (e) {{ after = e.code; }}
                log.push("closed:" + after);
                let bad = "none";
                try {{ await serial.open("/nonexistent/port"); }} catch (e) {{ bad = e.code; }}
                log.push("bad:" + bad);
                window.done = 1;
              }})().catch((e) => {{ log.push("error:" + e.message); window.done = 1; }});
            </script></body>"#
        ),
        "serial",
    );
    let mut sent = (false, false);
    let mut heard = Vec::new();
    for _ in 0..1500 {
        p.tick();
        let ready = js(&mut p, "window.ready");
        if ready == "1" && !sent.0 {
            master.write_all(b"ping\nabcd").unwrap();
            sent.0 = true;
        }
        if ready == "2" && !sent.1 {
            master.write_all(b"late").unwrap();
            sent.1 = true;
        }
        let mut buf = [0u8; 64];
        serialport::SerialPort::set_timeout(&mut master, std::time::Duration::from_millis(1)).unwrap();
        if let Ok(n) = master.read(&mut buf) {
            heard.extend_from_slice(&buf[..n]);
        }
        if js(&mut p, "window.done") == "1" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let log = js(&mut p, "window.log.join('|')");
    assert_eq!(log, "line:ping|four:abcd|timeout:SERIAL_TIMEOUT|data:late|closed:SERIAL_CLOSED|bad:ENOENT");
    assert!(String::from_utf8_lossy(&heard).contains("hello"), "the device saw {:?}", String::from_utf8_lossy(&heard));
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_keychain_module_reaches_the_native_store_and_rejects_bad_names() {
    let (mut p, path) = page(
        r##"<body><script type="module">
          import { keychain } from "@nile/keychain";
          window.out = [];
          (async () => {
            try { await keychain.get(""); out.push("accepted"); } catch (e) { out.push("rejected:" + e.message); }
            const ok = await keychain.available();
            out.push("available:" + typeof ok);
            out.push("reason:" + typeof keychain.reason());
            keychain.service = "NileTest";
            out.push("service:" + keychain.service);
            out.push("done");
          })().catch((e) => out.push("ERR " + e.message));
        </script></body>"##,
        "keychain",
    );
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(30));
        p.run_timers();
        if js(&mut p, "out.includes('done') || out.some((l) => l.startsWith('ERR'))") == "true" {
            break;
        }
    }
    let out = js(&mut p, "out.join('\\n')");
    assert!(out.contains("rejected:a service and an account name are required"), "{out}");
    assert!(out.contains("available:boolean") && out.contains("reason:string") && out.contains("service:NileTest") && out.contains("done"), "{out}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn mail_builds_mime_and_talks_smtp_with_a_fake_server() {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel::<(Vec<String>, String)>();
    std::thread::spawn(move || {
        // one session per connection: the first is accepted mail, the second rejects the recipient
        for session in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            let mut out = stream.try_clone().unwrap();
            let mut input = BufReader::new(stream);
            let mut commands = Vec::new();
            let mut message = String::new();
            out.write_all(b"220 fake ESMTP ready\r\n").unwrap();
            loop {
                let mut line = String::new();
                if input.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                let line = line.trim_end().to_string();
                commands.push(line.clone());
                let upper = line.to_uppercase();
                if upper.starts_with("EHLO") {
                    out.write_all(b"250-fake greets you\r\n250-SIZE 1000000\r\n250 AUTH PLAIN LOGIN\r\n").unwrap();
                } else if upper.starts_with("AUTH PLAIN") {
                    out.write_all(b"235 2.7.0 accepted\r\n").unwrap();
                } else if upper.starts_with("RCPT") && session == 1 {
                    out.write_all(b"550 5.1.1 no such user\r\n").unwrap();
                } else if upper.starts_with("MAIL") || upper.starts_with("RCPT") {
                    out.write_all(b"250 ok\r\n").unwrap();
                } else if upper == "DATA" {
                    out.write_all(b"354 go ahead\r\n").unwrap();
                    loop {
                        let mut l = String::new();
                        input.read_line(&mut l).unwrap();
                        if l == ".\r\n" {
                            break;
                        }
                        message.push_str(&l);
                    }
                    out.write_all(b"250 2.0.0 queued as 42\r\n").unwrap();
                } else if upper == "QUIT" {
                    out.write_all(b"221 bye\r\n").unwrap();
                    break;
                } else {
                    out.write_all(b"502 unknown\r\n").unwrap();
                }
            }
            tx.send((commands, message)).unwrap();
        }
    });
    let (mut p, path) = page(
        &format!(
            r##"<body><script type="module">
          import {{ sendMail, buildMessage, parseAddress }} from "@nile/mail";
          window.out = [];
          window.parseAddress = parseAddress;
          window.buildMessage = buildMessage;
          (async () => {{
            const r = await sendMail({{
              host: "127.0.0.1", port: {port}, secure: "none", allowInsecureAuth: true, user: "shop@example.com", password: "p@ss",
              from: {{ name: "Soft Sale", address: "shop@example.com" }}, to: ["ada@example.com", "Alan T <alan@example.com>"], bcc: "audit@example.com",
              subject: "Reçu n° 7", text: "Hello é\r\n.dotted line\r\nbye", html: "<p>Hello</p>",
              attachments: [{{ filename: "r.pdf", content: new Uint8Array([37, 80, 68, 70, 0, 255]) }}],
            }});
            out.push("sent:" + r.accepted.join(",") + "|" + r.response);
            try {{
              await sendMail({{ host: "127.0.0.1", port: {port}, secure: "none", from: "a@b.co", to: "nobody@example.com", subject: "x", text: "y" }});
              out.push("second: accepted");
            }} catch (e) {{ out.push("second:" + e.responseCode + ":" + e.message); }}
            out.push("done");
          }})().catch((e) => out.push("ERR " + e.message));
        </script></body>"##
        ),
        "mail",
    );
    for _ in 0..200 {
        std::thread::sleep(Duration::from_millis(30));
        p.run_timers();
        if js(&mut p, "out.includes('done') || out.some((l) => l.startsWith('ERR'))") == "true" {
            break;
        }
    }
    let out = js(&mut p, "out.join('\\n')");
    assert!(out.contains("sent:ada@example.com,alan@example.com,audit@example.com|2.0.0 queued as 42"), "{out}");
    assert!(out.contains("second:550:550 5.1.1 no such user"), "{out}");
    let (commands, message) = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(commands[0], "EHLO localhost");
    // AUTH PLAIN "\0shop@example.com\0p@ss"
    assert_eq!(commands[1], "AUTH PLAIN AHNob3BAZXhhbXBsZS5jb20AcEBzcw==");
    assert_eq!(&commands[2..6], ["MAIL FROM:<shop@example.com>", "RCPT TO:<ada@example.com>", "RCPT TO:<alan@example.com>", "RCPT TO:<audit@example.com>"]);
    assert_eq!(commands[6], "DATA");
    assert_eq!(commands[7], "QUIT");
    // headers: encoded subject, to without the bcc, a mixed message holding an alternative
    assert!(message.contains("From: Soft Sale <shop@example.com>\r\n"), "{message}");
    assert!(message.contains("To: ada@example.com, Alan T <alan@example.com>\r\n") && !message.contains("audit@example.com"), "{message}");
    assert!(message.contains("Subject: =?UTF-8?B?UmXDp3UgbsKwIDc=?=\r\n"), "{message}");
    assert!(message.contains("Content-Type: multipart/mixed; boundary=") && message.contains("Content-Type: multipart/alternative; boundary="), "{message}");
    assert!(message.contains("Content-Type: text/plain; charset=utf-8") && message.contains("Content-Type: text/html; charset=utf-8"), "{message}");
    assert!(message.contains("Content-Type: application/pdf; name=\"r.pdf\"") && message.contains("Content-Disposition: attachment; filename=\"r.pdf\""), "{message}");
    assert!(message.contains("JVBERgD/"), "the attachment is base64 (%PDF + 00 ff): {message}");
    // the dot-stuffed line inside the text part survives base64 untouched, and no raw line starts with a lone dot
    assert!(!message.lines().any(|l| l == "."), "{message}");
    let (second, _) = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(second.iter().any(|c| c.starts_with("RCPT TO:<nobody@example.com>")) && !second.contains(&"DATA".to_string()), "{second:?}");
    // the pure helpers: addresses, a refusal to send a password in the clear, header injection, no recipients
    assert_eq!(js(&mut p, "JSON.stringify([parseAddress('Ada <a@b.co>'), parseAddress('x@y.zz')])"), r#"[{"name":"Ada","address":"a@b.co"},{"name":"","address":"x@y.zz"}]"#);
    assert_eq!(js(&mut p, "(() => { try { buildMessage({ from: 'a@b.co', to: 'c@d.ee', subject: 'x\\r\\nBcc: evil@e.vil', text: 'y' }); return 'built'; } catch (e) { return e.message; } })()"), "line breaks are not allowed in a header");
    assert_eq!(js(&mut p, "(() => { try { buildMessage({ from: 'a@b.co', subject: 'x' }); return 'built'; } catch (e) { return e.message; } })()"), "no recipients");
    assert_eq!(js(&mut p, "(() => { try { buildMessage({ from: 'not-an-address', to: 'c@d.ee' }); return 'built'; } catch (e) { return e.message; } })()"), "not an email address: not-an-address");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_grid_keeps_an_edit_with_its_record_and_the_drag_clean() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><div id="host"></div>
        <script type="module">
          import "@nile/ui";
          const host = document.getElementById("host");
          // Properties set right after innerHTML, before the element is upgraded, must reach the setters.
          host.innerHTML = '<nile-grid id="g" key="id" style="width:400px;height:220px"></nile-grid><nile-combobox id="cb"></nile-combobox>';
          window.g = document.getElementById("g");
          g.columns = [{ key: "name", title: "Name", width: 200, editable: true, validate: (v) => (v ? true : "Required") }, { key: "n", title: "N", width: 80, type: "number" }];
          g.rows = Array.from({ length: 100 }, (_, i) => ({ id: i, name: "Row " + i, n: i }));
          window.cb = document.getElementById("cb");
          cb.items = ["Red", "Green"];
          window.log = [];
          g.addEventListener("cellchange", (e) => log.push(e.detail.row.id + ":" + e.detail.value));
          g.addEventListener("columnresize", (e) => log.push("resize:" + e.detail.width));
          window.shown = () => Array.from(document.querySelectorAll(".nile-grid-row")).filter((x) => x.style.display !== "none").sort((a, b) => Number(a.getAttribute("aria-rowindex")) - Number(b.getAttribute("aria-rowindex")));
        </script></body>"##,
        "ui-grid-record",
    );
    p.run_timers();
    p.run_timers();
    let settle = |p: &mut Page| {
        std::thread::sleep(Duration::from_millis(30));
        p.run_timers();
        p.run_timers();
    };
    let none = ModifiersState::default();
    assert_eq!(js(&mut p, "[g.rows.length, shown().length > 0, cb.querySelectorAll('.nile-combo-list [role=option]').length >= 0].join()"), "100,true,true", "pre-upgrade properties arrive");

    // A sort while an edit is open: the value still lands in the record being edited.
    js(&mut p, "g.edit(0, 'name')");
    js(&mut p, "document.activeElement.value = 'Edited'");
    js(&mut p, "g.sortBy('name', 'desc')");
    settle(&mut p);
    assert_eq!(js(&mut p, "g.editing ? g.editing.key + ':' + (g.editing.index > 0) + ':' + shown().some((r) => r.querySelector('.nile-grid-editor')) : 'none'"), "name:true:true", "the editor follows its record");
    p.key("keydown", "Enter".into(), none);
    settle(&mut p);
    assert_eq!(js(&mut p, "[g.rows[0].name, g.rows[99].name, log.join()].join()"), "Edited,Row 99,0:Edited");

    // A refused value survives a refresh: the editor stays in its row with focus.
    js(&mut p, "g.sortBy(null)");
    js(&mut p, "g.edit(3, 'name')");
    js(&mut p, "document.activeElement.value = ''");
    assert_eq!(js(&mut p, "g.commitEdit()"), "false");
    js(&mut p, "g.refresh(); g.add({ id: 500, name: 'new', n: 0 }, 0)");
    settle(&mut p);
    assert_eq!(js(&mut p, "[g.editing && g.editing.index, !!document.querySelector('.nile-grid-editor.nile-invalid'), document.activeElement.className].join()"), "4,true,nile-grid-editor nile-invalid");
    js(&mut p, "g.cancelEdit()");
    assert_eq!(js(&mut p, "g.editing"), "null");
    // Scrolling the invalid cell out of the window keeps it too.
    js(&mut p, "g.edit(3, 'name')");
    js(&mut p, "document.activeElement.value = ''");
    js(&mut p, "g.querySelector('.nile-grid-body').scrollTop = 2000");
    settle(&mut p);
    assert_eq!(js(&mut p, "[!!g.editing, !!document.querySelector('.nile-grid-editor')].join()"), "true,true");
    js(&mut p, "g.cancelEdit()");

    // A drag ends once when the window loses the pointer; later moves change nothing.
    js(&mut p, "(() => { const h = g.querySelector('.nile-grid-resizer'); h.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, clientX: 200 })); document.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 250 })); })()");
    js(&mut p, "window.dispatchEvent(new Event('blur'))");
    settle(&mut p);
    js(&mut p, "document.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 300 })); document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))");
    settle(&mut p);
    assert_eq!(js(&mut p, "log.filter((x) => x.startsWith('resize')).length"), "1");
    // Removing the grid mid-drag ends it as well.
    js(&mut p, "(() => { const h = g.querySelector('.nile-grid-resizer'); h.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, clientX: 200 })); document.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 230 })); })()");
    // (`g.remove()` is the grid's row API: detach the element itself.)
    js(&mut p, "g.parentNode.removeChild(g)");
    settle(&mut p);
    assert_eq!(js(&mut p, "g.isConnected"), "false");
    // The drag ended on disconnect, before any mouseup: reported once, later moves and releases do nothing.
    assert_eq!(js(&mut p, "log.filter((x) => x.startsWith('resize')).length"), "2", "reported on disconnect");
    js(&mut p, "document.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 400 })); document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))");
    settle(&mut p);
    assert_eq!(js(&mut p, "log.filter((x) => x.startsWith('resize')).length"), "2", "reported once per drag");
    let _ = std::fs::remove_file(path);
}


#[test]
fn the_ui_splitter_drags_collapses_and_remembers() {
    let key = format!("splitter-test-{}", std::process::id());
    let (mut p, path) = page(
        &r#"<body style="margin:0"><nile-splitter id="s" persist="PERSIST_KEY" style="width:600px;height:100px">
             <div id="a" size="200" min="100" max="400" collapsible>a</div><div id="b" min="50">b</div>
           </nile-splitter>
        <script type="module">
          import "@nile/ui";
          window.s = document.getElementById("s");
          window.changes = 0;
          s.addEventListener("splitterchange", () => changes++);
          window.grip = () => { const r = document.querySelector(".nile-splitter-grip").getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; };
        </script></body>"#
            .replace("PERSIST_KEY", &key),
        "ui-splitter",
    );
    p.run_timers();
    let sizes = |p: &mut Page| js(p, "s.sizes.join()");
    let grip = |p: &mut Page| -> (f32, f32) {
        let v = js(p, "grip().join()");
        let mut it = v.split(',').map(|n| n.parse::<f32>().unwrap());
        (it.next().unwrap(), it.next().unwrap())
    };
    assert_eq!(sizes(&mut p), "200,390", "the size attribute sets the first pane, the other takes the rest");
    // Drag the grip 100px right.
    let (x, y) = grip(&mut p);
    p.pointer_moved(x, y);
    p.press();
    p.pointer_moved(x + 100.0, y);
    p.release();
    assert_eq!(sizes(&mut p), "300,290");
    // max is 400: dragging further stops there.
    let (x, y) = grip(&mut p);
    p.pointer_moved(x, y);
    p.press();
    p.pointer_moved(x + 290.0, y);
    p.release();
    assert_eq!(sizes(&mut p), "400,190");
    // A click without a drag collapses the collapsible pane, and another brings it back.
    let (x, y) = grip(&mut p);
    p.pointer_moved(x, y);
    p.press();
    p.release();
    assert_eq!(js(&mut p, "[s.sizes.join(), s.isCollapsed(0)].join(';')"), "0,590;true");
    let (x, y) = grip(&mut p);
    p.pointer_moved(x, y);
    p.press();
    p.release();
    assert_eq!(sizes(&mut p), "400,190", "expanding restores the size it had");
    // Arrow keys on the focused grip move it by 10px.
    js(&mut p, "document.querySelector('.nile-splitter-grip').focus()");
    p.key("keydown", "ArrowLeft".into(), ModifiersState::default());
    assert_eq!(sizes(&mut p), "390,200");
    assert!(js(&mut p, "changes").parse::<u32>().unwrap() >= 5);
    // The layout is saved for next time.
    std::thread::sleep(Duration::from_millis(150));
    p.run_timers();
    assert_eq!(js(&mut p, &format!("localStorage.getItem('nile-splitter:{key}')")), r#"{"sizes":[390,null],"collapsed":[false,false]}"#);
    js(&mut p, &format!("localStorage.removeItem('nile-splitter:{key}')"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_grid_draws_only_visible_rows_and_sorts_selects_and_resizes() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><nile-grid id="g" key="id" select="multiple" row-height="30" empty="Nothing here" style="width:600px;height:200px">
             <nile-column key="id" title="#" width="80" type="number"></nile-column>
             <nile-column key="name" title="Name"></nile-column>
           </nile-grid>
        <script type="module">
          import "@nile/ui";
          const g = window.g = document.getElementById("g");
          window.events = [];
          g.addEventListener("selectionchange", (e) => events.push("sel:" + e.detail.indexes.join(".")));
          g.addEventListener("sort", (e) => events.push("sort:" + e.detail.key + ":" + e.detail.direction));
          g.addEventListener("columnresize", (e) => events.push("resize:" + e.detail.key + ":" + e.detail.width));
          g.addEventListener("rowactivate", (e) => events.push("go:" + e.detail.index));
          window.empty = () => getComputedStyle(g.querySelector(".nile-grid-empty")).display;
          window.mounted = () => g.mountedCount;
          // Rows are recycled, so the document order isn't the grid order: go by each row's position.
          window.shown = () => Array.from(g.querySelectorAll(".nile-grid-row")).filter((r) => r.style.display !== "none").sort((a, b) => a._vi - b._vi);
          window.firstCells = () => shown().slice(0, 3).map((r) => r.querySelector(".nile-grid-cell:nth-child(2)").textContent).join();
          window.at = (sel, i) => { const el = sel === ".nile-grid-row" ? shown()[i] : (i === undefined ? g.querySelector(sel) : g.querySelectorAll(sel)[i]); const r = el.getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2].join(); };
          g.rows = Array.from({ length: 10000 }, (_, i) => ({ id: i + 1, name: "Item " + (10000 - i) }));
        </script></body>"##,
        "ui-grid",
    );
    p.run_timers();
    let at = |p: &mut Page, sel: &str, i: Option<usize>| -> (f32, f32) {
        let v = match i {
            Some(i) => js(p, &format!("at('{sel}', {i})")),
            None => js(p, &format!("at('{sel}')")),
        };
        let mut it = v.split(',').map(|n| n.parse::<f32>().unwrap());
        (it.next().unwrap(), it.next().unwrap())
    };
    let click_at = |p: &mut Page, (x, y): (f32, f32)| {
        p.pointer_moved(x, y);
        p.press();
        p.release();
    };
    // 10,000 rows, but only the ones in view (and a little beyond) are in the document.
    let mounted: usize = js(&mut p, "mounted()").parse().unwrap();
    assert!((5..=20).contains(&mounted), "mounted {mounted}");
    assert_eq!(js(&mut p, "g.rowCount"), "10000");
    assert_eq!(js(&mut p, "empty()"), "none");
    // Scrolling mounts new rows and drops the old ones.
    js(&mut p, "g.querySelector('.nile-grid-body').scrollTop = 30 * 5000");
    p.run_timers();
    std::thread::sleep(Duration::from_millis(40));
    p.run_timers();
    assert_eq!(js(&mut p, "g.querySelector('.nile-grid-row').textContent.length > 0 && mounted() <= 20"), "true");
    assert_eq!(js(&mut p, "shown().some((r) => r.firstChild.textContent === '5001')"), "true");
    js(&mut p, "g.querySelector('.nile-grid-body').scrollTop = 0");
    p.run_timers();
    std::thread::sleep(Duration::from_millis(40));
    p.run_timers();
    // Click the Name header: natural-order ascending ("Item 1", "Item 2", "Item 10"...), then descending.
    let header = at(&mut p, ".nile-grid-hcell", Some(1));
    click_at(&mut p, header);
    assert_eq!(js(&mut p, "firstCells()"), "Item 1,Item 2,Item 3");
    click_at(&mut p, header);
    assert_eq!(js(&mut p, "firstCells()"), "Item 10000,Item 9999,Item 9998");
    click_at(&mut p, header);
    assert_eq!(js(&mut p, "firstCells()"), "Item 10000,Item 9999,Item 9998", "a third click clears the sort, back to the data order");
    click_at(&mut p, header);
    click_at(&mut p, header);
    // Click selects, Shift-click extends, Ctrl-click toggles.
    let row = |p: &mut Page, i: usize| at(p, ".nile-grid-row", Some(i));
    let r1 = row(&mut p, 1);
    click_at(&mut p, r1);
    assert_eq!(js(&mut p, "g.selectedIndex"), "1");
    p.mods = ModifiersState::SHIFT;
    let r3 = row(&mut p, 3);
    click_at(&mut p, r3);
    assert_eq!(js(&mut p, "g.selected.map((r) => r.id).join()"), "2,3,4", "shift-click selects the range");
    p.mods = ModifiersState::CONTROL;
    let r2 = row(&mut p, 2);
    click_at(&mut p, r2);
    assert_eq!(js(&mut p, "g.selected.map((r) => r.id).join()"), "2,4", "ctrl-click removed the middle one");
    p.mods = ModifiersState::default();
    // Keys: arrows move the selection, Enter activates.
    js(&mut p, "g.querySelector('.nile-grid-body').focus()");
    p.key("keydown", "ArrowDown".into(), ModifiersState::default());
    assert_eq!(js(&mut p, "g.selectedIndex"), "3");
    p.key("keydown", "Enter".into(), ModifiersState::default());
    assert!(js(&mut p, "events.join()").contains("go:3"));
    p.key("keydown", "End".into(), ModifiersState::default());
    assert_eq!(js(&mut p, "g.selectedIndex"), "9999");
    // Dragging the edge of a column header resizes it.
    let edge = at(&mut p, ".nile-grid-resizer", Some(0));
    p.pointer_moved(edge.0, edge.1);
    p.press();
    p.pointer_moved(edge.0 + 40.0, edge.1);
    p.release();
    assert_eq!(js(&mut p, "g.columns[0]._w"), "120");
    assert!(js(&mut p, "events.join()").contains("resize:id:120"));
    // Changing the data keeps the selection by key; an empty grid shows its message.
    js(&mut p, "g.rows = [{ id: 7, name: 'x' }]");
    assert_eq!(js(&mut p, "g.selected.length"), "0");
    js(&mut p, "g.rows = []");
    assert_eq!(js(&mut p, "empty()"), "flex");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_ui_splitter_reads_frameset_style_sizes() {
    let (mut p, path) = page(
        r#"<body style="margin:0">
           <nile-splitter id="a" cols="100px 1* 2*" style="width:600px;height:50px"><div>a</div><div>b</div><div>c</div></nile-splitter>
           <nile-splitter id="b" style="width:600px;height:50px"><div size="25%">a</div><div size="3*">b</div><div size="1*">c</div></nile-splitter>
           <nile-splitter id="c" rows="40 *" style="width:300px;height:200px"><div>a</div><div>b</div></nile-splitter>
        <script type="module">
          import "@nile/ui";
          window.sizes = (id) => document.getElementById(id).sizes.map(Math.round).join();
        </script></body>"#,
        "ui-splitter-units",
    );
    p.run_timers();
    // 600px minus two 10px grips: 100px fixed, the other 480 shared 1:2.
    assert_eq!(js(&mut p, "sizes('a')"), "100,160,320");
    // 25% of 600 = 150; the other 430 (after the grips) is shared 3:1, rounded to whole pixels.
    assert_eq!(js(&mut p, "sizes('b')"), "150,323,107");
    // `rows` stacks the panes: 40px, then the rest (200 - 10 grip).
    assert_eq!(js(&mut p, "sizes('c')"), "40,150");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_sys_module_reads_and_writes_files() {
    let dir = std::env::temp_dir().join(format!("nile-sys-{}", std::process::id())).to_string_lossy().replace('\\', "/");
    let (mut p, path) = page(
        &format!(r#"<body><script type="module">
          import * as sys from "@sys";
          const dir = "{dir}";
          const fs = sys.fs;
          fs.mkdirSync(dir);
          fs.writeFileSync(dir + "/a.txt", "héllo");
          fs.writeFileSync(dir + "/b.bin", new Uint8Array([1, 2, 3]).buffer);
          const text = new TextDecoder().decode(fs.readFileSync(dir + "/a.txt"));
          const st = fs.statSync(dir + "/a.txt");
          const names = fs.readdirSync(dir).map((e) => e.name + ":" + e.type).join(",");
          const missing = [fs.statSync(dir + "/nope"), fs.readdirSync(dir + "/nope")];
          fs.renameSync(dir + "/b.bin", dir + "/c.bin");
          fs.unlinkSync(dir + "/a.txt");
          window.out = [text, st.isFile, st.st_size, names, JSON.stringify(missing), fs.readdirSync(dir).map((e) => e.name).join(","), fs.splitpath("/x/y/z.txt").join("|")].join(";");
          fs.readFile(dir + "/nope").catch((e) => {{ window.err = e.code; }});
        </script></body>"#),
        "sys",
    );
    p.run_timers();
    assert_eq!(js(&mut p, "window.out"), "héllo;true;6;a.txt:1,b.bin:1;[null,null];c.bin;/x/y|z.txt");
    assert_eq!(js(&mut p, "window.err"), "ENOENT");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_sys_module_runs_processes_talks_over_sockets_and_opens_files() {
    let dir = std::env::temp_dir().join(format!("nile-sys2-{}", std::process::id())).to_string_lossy().replace('\\', "/");
    let command = if cfg!(windows) { r#"["cmd", "/c", "echo hi"]"# } else { r#"["echo", "hi"]"# };
    let (mut p, path) = page(
        &format!(r#"<body><script type="module">
          import * as sys from "@sys";
          const log = []; window.log = log; window.done = 0;
          const dec = new TextDecoder();
          const dir = "{dir}";
          sys.fs.mkdirSync(dir);
          // A file handle: write, then read back at a position.
          const f = sys.fs.openSync(dir + "/f.txt", "w+");
          f.writeSync("hello world");
          log.push("file:" + dec.decode(f.readSync(5, 6)) + ":" + f.statSync().st_size);
          f.closeSync();
          // A process and its stdout.
          const proc = sys.spawn({command});
          (async () => {{
            const out = await proc.stdout.read();
            const status = await proc.wait();
            log.push("proc:" + dec.decode(out).trim() + ":" + status.exit_status);
            // A TCP echo over loopback.
            const server = new sys.TCP();
            server.bind({{ ip: "127.0.0.1", port: 0 }});
            server.listen();
            const port = server.getsockname().port;
            const accepting = server.accept().then(async (conn) => {{ const data = await conn.read(); await conn.write(data); conn.close(); }});
            const client = new sys.TCP();
            await client.connect({{ ip: "127.0.0.1", port }});
            await client.write("ping");
            log.push("tcp:" + dec.decode(await client.read()));
            await accepting;
            client.close(); server.close();
            // UDP.
            const a = new sys.UDP(); a.bind({{ ip: "127.0.0.1", port: 0 }});
            const b = new sys.UDP();
            await b.send("dgram", {{ ip: "127.0.0.1", port: a.getsockname().port }});
            log.push("udp:" + dec.decode((await a.recv()).data));
            log.push("os:" + (typeof sys.uname().sysname) + ":" + (sys.getenv("NILE_SURELY_UNSET") === undefined) + ":" + sys.random(8).length + ":" + (typeof sys.hrtime()) + ":" + (sys.cwd().length > 0));
            sys.setenv("NILE_SYS_TEST", "1"); log.push("env:" + sys.getenv("NILE_SYS_TEST"));
            window.done = 1;
          }})().catch((e) => {{ log.push("error:" + e.message); window.done = 1; }});
        </script></body>"#),
        "sys2",
    );
    for _ in 0..500 {
        p.tick();
        if js(&mut p, "window.done") == "1" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(js(&mut p, "window.log.join('|')"), "file:world:11|proc:hi:0|tcp:ping|udp:dgram|os:string:true:8:bigint:true|env:1");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_sys_module_has_named_pipes_ttys_and_fs_watch() {
    let dir = std::env::temp_dir().join(format!("nile-sys3-{}", std::process::id())).to_string_lossy().replace('\\', "/");
    let (mut p, path) = page(
        &format!(r#"<body><script type="module">
          import * as sys from "@sys";
          const log = []; window.log = log; window.done = 0;
          const dec = new TextDecoder();
          const dir = "{dir}";
          sys.fs.mkdirSync(dir);
          (async () => {{
            // A named pipe: the server echoes in upper case.
            const server = new sys.Pipe();
            server.bind(dir + "/echo.sock");
            server.listen();
            const serving = server.accept().then(async (conn) => {{ const d = await conn.read(); await conn.write(dec.decode(d).toUpperCase()); conn.close(); }});
            const client = new sys.Pipe();
            await client.connect(dir + "/echo.sock");
            await client.write("ping");
            log.push("pipe:" + dec.decode(await client.read()) + ":" + client.getpeername().endsWith("echo.sock"));
            await serving;
            client.close(); server.close();
            // The terminal calls answer even without a terminal; only 0, 1 and 2 are terminals.
            const size = new sys.TTY(1).getWinSize();
            let bad = "none";
            try {{ new sys.TTY(7); }} catch (e) {{ bad = e.code; }}
            log.push("tty:" + (size.width > 0 && size.height > 0) + ":" + bad);
            // fs.watch sees a new file and then a change to it.
            const seen = [];
            const watch = sys.fs.watch(dir, (path, events) => seen.push([path.split("/").pop(), events]));
            let missing = "none";
            try {{ sys.fs.watch(dir + "/nope", () => {{}}); }} catch (e) {{ missing = e.code; }}
            await new Promise((r) => setTimeout(r, 400));
            sys.fs.writeFileSync(dir + "/new.txt", "one");
            await new Promise((r) => setTimeout(r, 700));
            sys.fs.writeFileSync(dir + "/new.txt", "one two three");
            await new Promise((r) => setTimeout(r, 700));
            watch.close();
            // The system may merge flags (a create arrives with the write that followed it); the second write has the change bit.
            const mine = seen.filter((s) => s[0] === "new.txt").map((s) => s[1]);
            log.push("watch:" + (mine.length >= 2 && (mine[0] & 1) === 1 && (mine[mine.length - 1] & 2) === 2 ? "ok" : JSON.stringify(mine)) + ":" + missing + ":" + (watch.path === dir));
            window.done = 1;
          }})().catch((e) => {{ log.push("error:" + e.message); window.done = 1; }});
        </script></body>"#),
        "sys3",
    );
    for _ in 0..800 {
        p.tick();
        if js(&mut p, "window.done") == "1" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let log = js(&mut p, "window.log.join('|')");
    assert!(log.starts_with("pipe:PING:true|tty:true:EBADF|watch:ok:ENOENT"), "{log}");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_file(path);
}

#[test]
fn linked_style_sheets_and_imports_apply() {
    let dir = std::env::temp_dir().join(format!("nile-css-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("main.css"), "@import \"sub/part.css\"; #a { color: rgb(255, 0, 0) }").unwrap();
    std::fs::write(dir.join("sub/part.css"), "#b { color: rgb(0, 0, 255) }").unwrap();
    let main = dir.join("index.html");
    std::fs::write(&main, r#"<head><link rel="stylesheet" href="main.css"></head><body><p id="a">a</p><p id="b">b</p></body>"#).unwrap();
    let mut p = Page::load(main, (400.0, 200.0), 0.0);
    p.layout(400.0, 200.0);
    assert_eq!(js(&mut p, "getComputedStyle(document.getElementById('a')).color"), "rgb(255, 0, 0)");
    assert_eq!(js(&mut p, "getComputedStyle(document.getElementById('b')).color"), "rgb(0, 0, 255)");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn restyling_after_a_state_change_matches_a_full_cascade() {
    let (mut p, path) = page(
        r#"<head><style>
             .row:hover { background: #def; color: red }
             .row:hover .cell { font-weight: bold }
             .a:hover + .b { color: green }
             .a:hover ~ .c { color: blue }
             input:focus { border-color: orange }
             .row:active { margin-left: 3px }
           </style></head><body style="margin:0">
           <div id="list"><div class="row" id="r1"><span class="cell" id="c1">one</span></div><div class="row" id="r2"><span class="cell">two</span></div></div>
           <p class="a" id="a">a</p><p class="b" id="b">b</p><p class="c" id="c">c</p><p class="d">d</p><input id="i"></body>"#,
        "partial",
    );
    p.layout(400.0, 300.0);
    let id = |p: &Page, s: &str| p.doc.borrow().find(|n| n.attr("id") == Some(s)).unwrap();
    let (r1, r2, c1, a, i) = (id(&p, "r1"), id(&p, "r2"), id(&p, "c1"), id(&p, "a"), id(&p, "i"));
    let check = |p: &Page, what: &str| {
        let v = p.view.borrow();
        let doc = p.doc.borrow();
        let full = nile_style::compute_styles_in(&doc, &v.interaction, v.viewport);
        for n in 0..doc.node_count() {
            assert!(v.styles.styles[n] == full.styles[n], "{what}: node {n} differs after a partial restyle");
        }
    };
    for (what, next) in [
        ("hover a row", InteractionState { hover: Some(c1), ..Default::default() }),
        ("move to the other row", InteractionState { hover: Some(r2), ..Default::default() }),
        ("hover the sibling-combinator source", InteractionState { hover: Some(a), ..Default::default() }),
        ("press", InteractionState { hover: Some(r1), active: Some(r1), ..Default::default() }),
        ("focus", InteractionState { hover: None, active: None, focus: Some(i) }),
        ("leave", InteractionState::default()),
    ] {
        p.set_interaction(next);
        check(&p, what);
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn partial_restyles_keep_transitions_working() {
    let (mut p, path) = page(
        r#"<head><style>
             .row { height: 30px; background: #fff; transition: background 0.1s }
             .row:hover { background: #def }
           </style></head><body style="margin:0"><div class="row" id="r1">one</div><div class="row" id="r2">two</div></body>"#,
        "partial-anim",
    );
    p.layout(400.0, 300.0);
    let row = |p: &Page, s: &str| p.doc.borrow().find(|n| n.attr("id") == Some(s)).unwrap();
    let (r1, r2) = (row(&p, "r1"), row(&p, "r2"));
    let bg = |p: &Page, n: NodeId| p.view.borrow().styles.get(n).background;
    // The first change after load seeds the animator (there is nothing to transition from yet).
    p.pointer_moved(20.0, 10.0);
    std::thread::sleep(std::time::Duration::from_millis(150));
    p.tick();
    let (hovered, plain) = (bg(&p, r1), bg(&p, r2));
    assert_ne!(hovered, plain, "the first row shows the hover colour");
    // Moving to the other row restyles only those rows and starts transitions on both.
    p.pointer_moved(20.0, 40.0);
    assert!(p.view.borrow().animator.is_active(), "a transition is running");
    assert_eq!(bg(&p, r2), plain, "it starts from the old colour");
    std::thread::sleep(std::time::Duration::from_millis(150));
    p.tick();
    assert_eq!((bg(&p, r1), bg(&p, r2)), (plain, hovered), "the transitions ended on the new colours");
    let (v, doc) = (p.view.borrow(), p.doc.borrow());
    let full = nile_style::compute_styles_in(&doc, &v.interaction, v.viewport);
    for n in 0..doc.node_count() {
        assert!(v.styles.styles[n] == full.styles[n], "node {n} differs from a full cascade after the transitions ended");
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn blobs_form_data_and_streams_work() {
    let (mut p, path) = page(
        r#"<body><form id="f"><input name="a" value="1"><input name="b" type="checkbox" checked><input name="c" type="checkbox"><select name="s"><option value="x">X</option><option value="y" selected>Y</option></select><textarea name="t">hi</textarea><button name="go">Go</button></form><script>
          window.log = []; window.done = 0;
          (async () => {
            const log = window.log;
            const blob = new Blob(["héllo ", new Uint8Array([119, 111, 114, 108, 100])], { type: "Text/Plain" });
            log.push(blob.size + ":" + blob.type + ":" + await blob.text() + ":" + await blob.slice(7, 12).text());
            const file = new File(["data"], "a.txt", { type: "text/plain" });
            log.push(file.name + ":" + file.size + ":" + (file instanceof Blob) + ":" + await file.text());
            const fd = new FormData(document.getElementById("f"));
            log.push([...fd].map(([k, v]) => k + "=" + v).join("&"));
            fd.append("file", file); fd.set("a", "2"); fd.append("a", "3");
            log.push(fd.getAll("a").join(",") + ":" + fd.has("file") + ":" + fd.get("file").name);
            // The multipart body round-trips through a Response.
            const req = new Request("http://x/", { method: "POST", body: fd });
            const ct = req.headers.get("content-type");
            const back = await new Response(await req.arrayBuffer(), { headers: { "content-type": ct } }).formData();
            log.push(ct.split(";")[0] + ":" + back.getAll("a").join(",") + ":" + back.get("s") + ":" + back.get("file").name + ":" + await back.get("file").text());
            // Streams: a source, a transform and a reader.
            const src = new ReadableStream({ start(c) { c.enqueue("ab"); c.enqueue("cd"); c.close(); } });
            const up = src.pipeThrough(new TransformStream({ transform(ch, c) { c.enqueue(ch.toUpperCase()); } }));
            let out = ""; for await (const ch of up) out += ch;
            const r = new Response("one two").body.getReader();
            const first = await r.read();
            log.push(out + ":" + first.done + ":" + new TextDecoder().decode(first.value) + ":" + (await r.read()).done);
            const [t1, t2] = new Blob(["xyz"]).stream().tee();
            const read = async (st) => { let n = 0; for await (const c of st) n += c.length; return n; };
            log.push((await read(t1)) + "," + (await read(t2)));
            const sink = []; const w = new WritableStream({ write(c) { sink.push(c); } }).getWriter();
            await w.write(1); await w.write(2); await w.close();
            log.push(sink.join("+"));
            window.done = 1;
          })().catch((e) => { window.log.push("error:" + e.message); window.done = 1; });
        </script></body>"#,
        "blobs",
    );
    for _ in 0..200 {
        p.tick();
        if js(&mut p, "window.done") == "1" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        js(&mut p, "window.log.join('|')"),
        "12:text/plain:héllo world:world|a.txt:4:true:data|a=1&b=on&s=y&t=hi|2,3:true:a.txt|multipart/form-data:2,3:y:a.txt:data|AB CD:false:one two:true".replace("AB CD", "ABCD") + "|3,3|1+2"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn dropped_files_go_to_a_file_input_or_a_drop_event() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><input id="f" type="file" style="margin:0;width:200px;height:30px"><input id="m" type="file" multiple style="margin:0;width:200px;height:30px"><div id="z" style="height:60px"></div>
           <script>window.log = [];
             for (const id of ["f", "m"]) document.getElementById(id).addEventListener("change", () => log.push(id + ":" + [...document.getElementById(id).files].map((f) => f.name).join("+")));
             document.getElementById("z").addEventListener("drop", (e) => log.push("drop:" + e.dataTransfer.files.length + ":" + e.dataTransfer.files[0].name));</script></body>"#,
        "drop",
    );
    let files = vec!["/tmp/a.txt".to_string(), "/tmp/b.txt".to_string()];
    let over = |p: &mut Page, id: &str| {
        let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        let r = p.view.borrow_mut().layout().node_rect(node).unwrap();
        p.pointer_moved(r.x + 5.0, r.y + 5.0);
    };
    over(&mut p, "f");
    assert!(p.drop_files(&files));
    over(&mut p, "m");
    p.drop_files(&files);
    over(&mut p, "z");
    p.drop_files(&files);
    assert_eq!(js(&mut p, "log.join('|')"), "f:a.txt|m:a.txt+b.txt|drop:2:a.txt");
    let _ = std::fs::remove_file(path);
}

#[test]
fn date_time_and_colour_inputs_open_pickers_that_set_the_value() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><input id="d" type="date" value="2026-10-15" style="margin:0;width:200px"><br>
           <input id="t" type="time" value="09:30" style="margin:0;width:200px"><br>
           <input id="c" type="color" value="#102030" style="margin:0"><br>
           <input id="w" type="week" value="2026-W42" style="margin:0;width:200px"><br>
           <input id="m" type="month" value="2026-10" style="margin:0;width:200px">
           <script>window.log = [];
             for (const id of ["d", "t", "c", "w", "m"]) document.getElementById(id).addEventListener("change", (e) => log.push(id + "=" + e.target.value));
           </script></body>"##,
        "pickers",
    );
    let at = |p: &mut Page, id: &str| {
        let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        p.view.borrow_mut().layout().node_rect(node).unwrap()
    };
    let open = |p: &mut Page, id: &str, left: bool| {
        let r = at(p, id);
        p.pointer_moved(if left { r.x + 10.0 } else { r.x + r.width - 10.0 }, r.y + r.height / 2.0);
        p.press();
        p.release();
    };
    let count = |p: &mut Page| js(p, "document.querySelectorAll('.nile-picker').length");
    // A click in the middle of a date field edits it; the glyph opens the calendar.
    let r = at(&mut p, "d");
    p.pointer_moved(r.x + 20.0, r.y + 8.0);
    p.press();
    p.release();
    assert_eq!(count(&mut p), "0");
    assert_eq!(p.cursor(), nile_style::Cursor::Text, "the text of the field");
    p.pointer_moved(r.x + r.width - 10.0, r.y + 8.0);
    assert_eq!(p.cursor(), nile_style::Cursor::Default, "the glyph is a button");
    open(&mut p, "d", false);
    assert_eq!(count(&mut p), "1");
    js(&mut p, "[...document.querySelectorAll('.np-day:not(.np-out)')].find((b) => b.textContent === '20').click()");
    assert_eq!(js(&mut p, "document.getElementById('d').value"), "2026-10-20");
    assert_eq!(count(&mut p), "0", "choosing a day closes the picker");
    open(&mut p, "t", false);
    js(&mut p, "document.querySelectorAll('.np-col')[0].children[14].click()");
    js(&mut p, "document.querySelectorAll('.np-col')[1].children[5].click()");
    assert_eq!(js(&mut p, "document.getElementById('t').value"), "14:05");
    open(&mut p, "c", true);
    assert_eq!(count(&mut p), "1");
    js(&mut p, "document.querySelector('.np-swatch').click()");
    assert_eq!(js(&mut p, "document.getElementById('c').value"), "#000000");
    js(&mut p, "const f = document.querySelector('.np-hex'); f.value = '#336699'; f.dispatchEvent(new Event('change', { bubbles: true }))");
    assert_eq!(js(&mut p, "document.getElementById('c').value"), "#336699");
    // Escape closes a picker (this one covers the fields below it); then the week and month pickers.
    press_key(&mut p, "Escape", ModifiersState::default());
    assert_eq!(count(&mut p), "0");
    open(&mut p, "w", false);
    assert_eq!(js(&mut p, "[...document.querySelectorAll('.nile-picker')].map((x) => x.getAttribute('aria-label')).join()"), "Choose week");
    js(&mut p, "[...document.querySelectorAll('.np-day:not(.np-out)')].find((b) => b.textContent === '1').click()");
    assert_eq!(js(&mut p, "document.getElementById('w').value"), "2026-W40");
    open(&mut p, "m", false);
    js(&mut p, "[...document.querySelectorAll('.np-cell')].find((b) => b.textContent === 'Mar').click()");
    assert_eq!(js(&mut p, "document.getElementById('m').value"), "2026-03");
    assert!(js(&mut p, "log.join(',')").starts_with("d=2026-10-20,t=14:05"), "{}", js(&mut p, "log.join(',')"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn followed_links_match_visited_and_stop_matching_link() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><a id="l" href="#x" style="display:block;height:20px">link</a><a id="n">plain</a></body>"##,
        "visited",
    );
    let q = |p: &mut Page, sel: &str| js(p, &format!("document.getElementById('l').matches('{sel}')"));
    assert_eq!((q(&mut p, ":link"), q(&mut p, ":any-link"), q(&mut p, ":visited")), ("true".into(), "true".into(), "false".into()));
    assert_eq!(js(&mut p, "document.getElementById('n').matches(':any-link')"), "false");
    click(&mut p, "l");
    assert_eq!((q(&mut p, ":link"), q(&mut p, ":any-link"), q(&mut p, ":visited")), ("false".into(), "true".into(), "true".into()));
    // Scripts can set the flag too, as in Sciter.
    js(&mut p, "document.getElementById('l').state.visited = false");
    assert_eq!(q(&mut p, ":link"), "true");
    let _ = std::fs::remove_file(path);
}

#[test]
fn css_prototype_makes_elements_instances_of_a_script_class() {
    let module = std::env::temp_dir().join(format!("nile-proto-{}.js", std::process::id()));
    std::fs::write(
        &module,
        r#"export class Tabs extends Element {
             clicks = 0;
             componentDidMount() { window.log.push("mount:" + this.localName + ":" + this.clicks); this.state.focusable = true; }
             componentWillUnmount() { window.log.push("unmount"); }
             get current() { const l = this.$(">strip>label:current"); return l ? l.attributes["for"] : "none"; }
             ["on click at >strip>label"](event, label) { this.clicks++; this.$$(">strip>label").forEach((l) => l.state.current = l === label); window.log.push("click:" + label.textContent + ":" + this.current); return true; }
             ["on ^keydown"](event) { window.log.push("key:" + event.key); }
           }"#,
    )
    .unwrap();
    let file = module.file_name().unwrap().to_string_lossy().into_owned();
    let (mut p, path) = page(
        &format!(
            r#"<head><style>tabs {{ prototype: Tabs url({file}) }} label:current {{ color: red }}</style></head>
               <body><script>window.log = [];</script>
               <tabs id="t"><strip><label for="a">A</label><label for="b">B</label></strip><panels><div id="p"><label>inner</label></div></panels></tabs></body>"#
        ),
        "proto",
    );
    for _ in 0..50 {
        p.tick();
        if js(&mut p, "window.log.length") != "0" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(js(&mut p, "window.log.join('|')"), "mount:tabs:0");
    assert_eq!(js(&mut p, "document.getElementById('t') instanceof Element && typeof document.getElementById('t').current"), "string");
    // The method-name handler: relative selector, `this` is the element, `true` stops the event.
    js(&mut p, "document.querySelectorAll('#t > strip > label')[1].click()");
    js(&mut p, "document.querySelector('#p label').click()"); // not a child of the strip: no handler
    assert_eq!(js(&mut p, "window.log.slice(1).join('|')"), "click:B:b");
    assert_eq!(js(&mut p, "document.getElementById('t').clicks"), "1");
    assert_eq!(js(&mut p, "document.getElementById('t').matches(':focusable')"), "true");
    // `:root` in a query started from an element is that element.
    assert_eq!(js(&mut p, "document.getElementById('t').$(':root > strip') !== null"), "true");
    js(&mut p, "document.getElementById('t').remove()");
    assert_eq!(js(&mut p, "window.log[window.log.length - 1]"), "unmount");
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(module);
}

#[test]
fn custom_elements_upgrade_parsed_created_and_new_elements() {
    let (mut p, path) = page(
        r##"<body><x-a id="p" k="1"></x-a><div id="host"></div><script>
          window.log = [];
          class XA extends HTMLElement {
            static observedAttributes = ["k"];
            n = 5;
            connectedCallback() { log.push("conn:" + this.id + ":" + this.n); }
            disconnectedCallback() { log.push("disc:" + this.id); }
            attributeChangedCallback(a, o, v) { log.push("attr:" + a + ":" + o + ">" + v); }
            hello() { return "hi " + this.localName; }
          }
          customElements.define("x-a", XA);
          const c = document.createElement("x-a"); c.id = "c"; c.setAttribute("k", "2");
          document.getElementById("host").append(c);
          const m = new XA(); m.id = "m";
          window.made = m;
          try { customElements.define("bad", class extends HTMLElement {}); } catch (e) { log.push("bad:" + e.name); }
        </script></body>"##,
        "custom-elements",
    );
    p.tick();
    let _ = js(&mut p, "0"); // let the mutation observer run
    p.tick();
    assert_eq!(js(&mut p, "document.getElementById('p') instanceof customElements.get('x-a')"), "true");
    assert_eq!(js(&mut p, "document.getElementById('p').hello()"), "hi x-a");
    assert_eq!(js(&mut p, "made.localName + ':' + (made instanceof XA)"), "x-a:true");
    assert_eq!(js(&mut p, "document.getElementById('c') instanceof XA"), "true");
    let log = js(&mut p, "window.log.join('|')");
    for want in ["conn:p:5", "attr:k:null>1", "conn:c:5", "attr:k:null>2", "bad:SyntaxError"] {
        assert!(log.contains(want), "{want} missing from {log}");
    }
    js(&mut p, "document.getElementById('c').remove()");
    p.tick();
    js(&mut p, "document.getElementById('p').setAttribute('k', '9')");
    let log = js(&mut p, "window.log.join('|')");
    assert!(log.contains("disc:c") && log.contains("attr:k:1>9"), "{log}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn translucent_windows_keep_the_canvas_alpha() {
    let canvas = |html: &str| {
        let (p, path) = page(html, "canvas-alpha");
        let c = p.canvas_background();
        let _ = std::fs::remove_file(path);
        (c.a(), c.r())
    };
    // Transparent frame: no background -> clear; a translucent one keeps its alpha.
    assert_eq!(canvas(r#"<html window-frame="transparent"><body></body></html>"#).0, 0);
    assert_eq!(canvas(r#"<html window-material="blur" style="background: rgba(255, 0, 0, 0.5)"><body></body></html>"#), (127, 255));
    // An ordinary window shows the same colour over white.
    assert_eq!(canvas(r#"<html style="background: rgba(0, 0, 0, 0.5)"><body></body></html>"#), (255, 128));
}

#[test]
fn window_material_is_a_script_property_of_the_root_attribute() {
    let (mut p, path) = page(r#"<html window-material="hud"><body></body></html>"#, "material");
    assert_eq!(js(&mut p, "Window.this.material"), "hud");
    js(&mut p, "Window.this.material = 'mica'");
    assert_eq!(window::WindowConfig::from_document(&p.doc.borrow()).material, window::Material::Mica);
    js(&mut p, "Window.this.material = ''");
    assert!(!window::WindowConfig::from_document(&p.doc.borrow()).translucent());
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_window_corner_radius_is_the_roots_border_radius() {
    let (p, path) = page(r#"<html style="border-radius: 18px"><body></body></html>"#, "corner");
    assert_eq!(p.window_corner_radius(), 18.0);
    let _ = std::fs::remove_file(path);
    let (p, path) = page("<html><body></body></html>", "corner0");
    assert_eq!(p.window_corner_radius(), 0.0);
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_maximize_button_has_a_rect_for_hit_testing() {
    let (p, path) = page(
        r#"<body style="margin:0"><div style="position:absolute;left:100px;top:4px;width:46px;height:30px" role="window-maximize"></div><div style="position:absolute;left:10px;top:4px;width:30px;height:30px" role="window-close"></div></body>"#,
        "maxbtn",
    );
    assert_eq!(p.maximize_button_rect(), Some([100.0, 4.0, 46.0, 30.0]));
    let _ = std::fs::remove_file(path);
    let (p, path) = page("<body></body>", "maxbtn0");
    assert_eq!(p.maximize_button_rect(), None);
    let _ = std::fs::remove_file(path);
}

#[test]
fn form_controls_validate_and_match_valid_and_invalid() {
    let (mut p, path) = page(
        r##"<body><form id="f"><input id="a" required><input id="n" type="number" min="1" max="5" step="2" value="4"><input id="e" type="email" value="x@y.z"><input id="t" pattern="[a-z]+" value="abc" maxlength="5"><input id="c" type="checkbox" required><input id="d" disabled required></form><script>
          window.log = [];
          const $ = (s) => document.querySelector(s);
          const v = (id) => { const x = $(id).validity; return Object.keys(x).filter((k) => x[k] === true && k !== "valid").join("+") || "ok"; };
          document.addEventListener("DOMContentLoaded", () => {
            log.push(v("#a"), v("#n"), v("#e"), v("#t"), v("#c"), v("#d"), $("#d").willValidate);
          });
          window.later = () => {
            $("#a").value = "x"; $("#n").value = "3"; $("#e").value = "nope"; $("#t").value = "ABCDEFG"; $("#c").checked = true;
            return new Promise((r) => setTimeout(r, 30)).then(() => {
              log.push(v("#a"), v("#n"), v("#e"), v("#t"), v("#c"), $("#f").checkValidity(), $("#e").validationMessage !== "");
              $("#e").setCustomValidity("custom"); log.push($("#e").validationMessage);
              window.done = 1;
            });
          };
        </script></body>"##,
        "validity",
    );
    let doc_invalid = |p: &mut Page, id: &str, flag: &str| js(p, &format!("document.querySelector('#{id}').matches(':{flag}')"));
    assert_eq!(js(&mut p, "log.join('|')"), "valueMissing|stepMismatch|ok|ok|valueMissing|ok|false");
    assert_eq!(doc_invalid(&mut p, "a", "invalid"), "true");
    assert_eq!(doc_invalid(&mut p, "e", "valid"), "true");
    assert_eq!(doc_invalid(&mut p, "d", "invalid"), "false");
    js(&mut p, "later()");
    for _ in 0..100 {
        p.tick();
        if js(&mut p, "window.done") == "1" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(js(&mut p, "log.slice(7).join('|')"), "ok|ok|typeMismatch|patternMismatch+tooLong|ok|false|true|custom");
    assert_eq!(doc_invalid(&mut p, "e", "invalid"), "true");
    let _ = std::fs::remove_file(path);
}

#[test]
fn class_components_forms_storage_and_debug_work() {
    let db = std::env::temp_dir().join(format!("nile-store-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let src = r#"<body><form id="f"><input name="a" value="1"><input name="n" type="number" value="5"><input name="c" type="checkbox"><input name="r" type="radio" value="x"><input name="r" type="radio" value="y" checked></form><div id="host"></div><script type="module">
          import * as debug from "@debug";
          import * as DB from "@storage";
          window.log = [];
          const log = window.log;
          class Clock extends Element {
            time = 1;
            this(props, kids) { this.props = props; this.kids = kids; log.push("this:" + props.label); }
            render() { return <span class="c">{this.props.label}:{this.time}:{this.kids.length}</span>; }
            componentDidMount() { log.push("mount:" + this.isConnected); }
            componentDidUpdate() { log.push("update"); }
            componentWillUnmount() { log.push("unmount"); }
          }
          const host = document.getElementById("host");
          host.content(<div><Clock label="A"><i/></Clock></div>);
          const clock = host.querySelector("div").firstElementChild;
          log.push(clock instanceof Clock, clock.className, clock.textContent);
          host.patch(<div><Clock label="B"><i/></Clock></div>, true);
          log.push(host.querySelector("div").firstElementChild === clock ? "reused" : "new", clock.textContent);
          clock.componentUpdate({ time: 2 });
          clock.componentUpdate({ time: 3 });
          await 0;
          log.push(clock.textContent);
          clock.componentUpdate({ time: 3 });
          await 0;
          host.content(<p/>);
          log.push(JSON.stringify(document.getElementById("f").value));
          document.getElementById("f").value = { a: "z", c: true, r: "x" };
          log.push(JSON.stringify(document.getElementById("f").value));
          const e = Element.create(<b>hi</b>);
          log.push(e.localName);
          const path = "DBPATH";
          class Note { hello() { return "hi " + this.t; } }
          const store = DB.open(path);
          store.registerClass(Note);
          store.root.n = Object.assign(new Note(), { t: "x" });
          store.root.when = new Date(86400000);
          store.close();
          const again = DB.open(path);
          again.registerClass(Note);
          log.push(again.root.n.hello() + ":" + (again.root.when instanceof Date) + ":" + again.root.when.getTime());
          let caught = "";
          debug.setConsoleOutputHandler((sys, sev, msg) => { caught = sev + msg; return true; });
          console.warn("careful");
          log.push(caught);
          const frame = debug.callStackAt(0);
          log.push(typeof frame.lineNo);
          window.done = 1;
        </script></body>"#.replace("DBPATH", &db.to_string_lossy().replace('\\', "/"));
    let (mut p, path) = page(&src, "components");
    for _ in 0..200 {
        p.tick();
        if js(&mut p, "window.done") == "1" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        js(&mut p, "window.log.join('|')"),
        "this:A|mount:true|true|c|A:1:1|this:B|reused|B:1:1|update|B:3:1|unmount|{\"a\":\"1\",\"n\":5,\"c\":false,\"r\":\"y\"}|{\"a\":\"z\",\"n\":5,\"c\":true,\"r\":\"x\"}|b|hi x:true:86400000|1careful|number"
    );
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(db);
}

/// A WebSocket server for one connection: accepts, echoes data frames and answers a close.
fn echo_websocket_server() -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let Ok((mut s, _)) = listener.accept() else { return };
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if s.read(&mut byte).unwrap_or(0) == 0 {
                return;
            }
            head.push(byte[0]);
        }
        let text = String::from_utf8_lossy(&head).to_string();
        assert!(text.starts_with("GET /chat?x=1 HTTP/1.1") && text.contains("Upgrade: websocket") && text.contains("Sec-WebSocket-Key: "), "{text}");
        s.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: x\r\n\r\n").unwrap();
        loop {
            let mut h = [0u8; 2];
            if s.read_exact(&mut h).is_err() {
                return;
            }
            let (op, masked) = (h[0] & 15, h[1] & 0x80 != 0);
            let mut len = (h[1] & 127) as usize;
            if len == 126 {
                let mut l = [0u8; 2];
                s.read_exact(&mut l).unwrap();
                len = u16::from_be_bytes(l) as usize;
            }
            let mut mask = [0u8; 4];
            assert!(masked, "client frames are masked");
            s.read_exact(&mut mask).unwrap();
            let mut data = vec![0u8; len];
            s.read_exact(&mut data).unwrap();
            for (i, b) in data.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
            // Echo (text and binary) or answer the close; a ping comes first, to be answered by the client.
            let mut out = vec![0x80 | op, data.len() as u8];
            if data.len() >= 126 {
                out = vec![0x80 | op, 126, (data.len() >> 8) as u8, data.len() as u8];
            }
            out.extend(&data);
            if op == 1 && data == b"hello" {
                s.write_all(&[0x89, 2, b'o', b'k']).unwrap();
            }
            s.write_all(&out).unwrap();
            if op == 8 {
                return;
            }
        }
    });
    port
}

#[test]
fn websockets_talk_to_a_server() {
    let port = echo_websocket_server();
    let (mut p, path) = page(
        &format!(r#"<body><script>
          window.log = []; window.done = 0;
          const ws = new WebSocket("ws://127.0.0.1:{port}/chat?x=1");
          ws.binaryType = "arraybuffer";
          ws.onopen = () => {{ log.push("open:" + ws.readyState); ws.send("hello"); ws.send(new Uint8Array([1, 2, 3])); ws.send("x".repeat(200)); }};
          ws.onmessage = (e) => {{
            log.push(typeof e.data === "string" ? e.data.slice(0, 8) + e.data.length : "bin:" + new Uint8Array(e.data).join(""));
            if (log.length === 4) ws.close(1000, "bye");
          }};
          ws.onerror = (e) => log.push("error");
          ws.onclose = (e) => {{ log.push("close:" + e.code + ":" + e.reason + ":" + ws.readyState); window.done = 1; }};
        </script></body>"#),
        "ws",
    );
    for _ in 0..400 {
        p.tick();
        if js(&mut p, "window.done") == "1" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(js(&mut p, "window.log.join('|')"), "open:1|hello5|bin:123|xxxxxxxx200|close:1000:bye:3");
    let _ = std::fs::remove_file(path);
}

#[test]
fn live_regions_are_marked_in_the_accessibility_tree() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="poll" aria-live="polite">Saved</div><div role="alert">Oops</div><div role="status" aria-atomic="false">Ready</div>
           <div role="log">line</div><div aria-live="off">quiet</div><div aria-live="assertive" aria-atomic="true"></div></body>"#,
        "live",
    );
    let tree = p.a11y_tree();
    let live = |name: &str| tree.nodes.iter().find(|n| n.role != a11y::A11yRole::StaticText && tree.nodes.iter().any(|c| n.children.contains(&c.id) && c.name == name)).map(|n| (n.live, n.live_atomic));
    use a11y::A11yLive::*;
    assert_eq!(live("Saved"), Some((Polite, false)));
    assert_eq!(live("Oops"), Some((Assertive, true)));
    assert_eq!(live("Ready"), Some((Polite, false)));
    assert_eq!(live("line"), Some((Polite, false)));
    // `aria-live="off"` is a plain container; an empty live region stays in the tree so a change can be announced.
    assert_eq!(live("quiet").map(|l| l.0), Some(Off));
    assert!(tree.nodes.iter().any(|n| n.live == Assertive && n.children.is_empty() && n.live_atomic));
    assert!(tree.to_json().contains("\"live\":\"assertive\""));
    let _ = std::fs::remove_file(path);
}

#[test]
fn frames_follow_scrolling_clipping_and_transforms_of_their_containers() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="box" style="width:200px;height:100px;overflow:auto"><div style="height:30px"></div>
           <iframe id="f" srcdoc="<p>hi</p>" style="display:block;width:150px;height:150px;border:0"></iframe></div>
           <iframe id="r" srcdoc="<p>tilt</p>" style="position:absolute;left:300px;top:20px;width:100px;height:40px;border:0;transform:rotate(90deg)"></iframe></body>"#,
        "frames-clip",
    );
    p.layout(600.0, 300.0);
    p.update_frames();
    let f = p.doc.borrow().find(|n| n.attr("id") == Some("f")).unwrap();
    let r = p.doc.borrow().find(|n| n.attr("id") == Some("r")).unwrap();
    assert_eq!(p.frame_at(10.0, 40.0), Some(f));
    // The container clips it: the part below the box's 100px is not the frame's.
    assert_eq!(p.frame_at(10.0, 120.0), None);
    // Scrolled by 20, the frame moves up with the content.
    let _ = p.eval("document.$('#box').scrollTop = 20");
    p.sync();
    p.update_frames();
    assert_eq!(p.frames.borrow().slots[&f].rect.1, 10.0);
    assert_eq!(p.frame_at(10.0, 5.0), None, "above the box's top edge is clipped");
    assert_eq!(p.frame_at(10.0, 15.0), Some(f));
    // Rotated a quarter turn about its centre (350, 40): the 100x40 frame now covers 330..370 x -10..90.
    assert_eq!(p.frame_at(350.0, 80.0), Some(r));
    assert_eq!(p.frame_at(310.0, 40.0), None, "the unrotated rectangle's ends are empty now");
    let (lx, ly) = p.frames.borrow().slots[&r].to_local(350.0, 80.0);
    assert!((lx - 90.0).abs() < 0.5 && (ly - 20.0).abs() < 0.5, "{lx},{ly}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn selections_in_editable_text_reach_the_accessibility_tree() {
    let (mut p, path) = page(r#"<body style="margin:0"><div id="ed" contenteditable="true"><p>Hello brave world</p></div></body>"#, "a11ysel");
    let id = p.doc.borrow().find(|n| n.tag() == Some("p")).unwrap();
    let text = p.doc.borrow().node(id).children[0];
    let host = p.doc.borrow().find(|n| n.tag() == Some("div")).unwrap();
    let mut next = p.interaction();
    next.focus = Some(host);
    p.set_interaction(next);
    p.view.borrow_mut().edit_sel = Some((nile_layout::DomPoint { node: text, offset: 6 }, nile_layout::DomPoint { node: text, offset: 11 }));
    let tree = p.a11y_tree();
    let h = tree.nodes.iter().find(|n| n.selection.is_some()).expect("the host carries the selection");
    let s = h.selection.unwrap();
    assert_eq!((s.anchor.1, s.focus.1), (6, 11));
    assert_eq!(s.anchor.0, s.focus.0);
    assert!(tree.nodes.iter().any(|n| n.id == s.anchor.0 && n.name == "Hello brave world"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_menu_element_becomes_the_application_menu() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><menu id="app"><li>File <menu>
             <li id="open" accesskey="CmdOrCtrl+O">Open…</li><li id="save" accesskey="Shift+CmdOrCtrl+S" disabled>Save</li><hr/><hr/>
             <li id="auto" checked>Auto <b>save</b></li><li id="gone" hidden>Hidden</li><li id="quit" role="quit">Quit</li></menu></li>
           <li>Edit <menu><li id="copy" role="copy">Copy</li></menu></li></menu>
           <input id="t" value="abc">
           <script>window.log = []; document.getElementById('open').addEventListener('click', () => log.push('open'));
                   document.getElementById('save').addEventListener('click', () => log.push('save'));</script></body>"#,
        "appmenu",
    );
    assert!(p.app_menu().is_none());
    js(&mut p, "Window.this.menu = document.getElementById('app'); 0");
    let menu = p.app_menu().expect("a menu");
    let MenuItem::Item { label, items, .. } = &menu[0] else { panic!() };
    assert_eq!(label, "File");
    let names: Vec<String> = items.iter().map(|i| match i { MenuItem::Item { label, enabled, checked, .. } => format!("{label}{}{}", if *enabled { "" } else { " (off)" }, if *checked { " (on)" } else { "" }), MenuItem::Separator => "-".into() }).collect();
    assert_eq!(names, ["Open…", "Save (off)", "-", "Auto save (on)", "Quit"]);
    let MenuItem::Item { id: open, accel, .. } = &items[0] else { panic!() };
    assert_eq!(accel, &Accel::parse("Ctrl+o"));
    let MenuItem::Item { id: save, .. } = &items[1] else { panic!() };
    let MenuItem::Item { id: quit, .. } = &items[4] else { panic!() };
    // Choosing an item fires `click`; a disabled one does nothing; window roles go to the embedder.
    assert_eq!(p.menu_click(*open), None);
    assert_eq!(p.menu_click(*save), None);
    assert_eq!(p.menu_click(*quit).as_deref(), Some("quit"));
    assert_eq!(js(&mut p, "log.join(',')"), "open");
    // The shortcut runs it where the platform's menu doesn't.
    let mut mods = ModifiersState::default();
    mods |= if cfg!(target_os = "macos") { ModifiersState::SUPER } else { ModifiersState::CONTROL };
    assert!(p.menu_accelerator("o", mods));
    assert!(!p.menu_accelerator("o", ModifiersState::default()));
    assert_eq!(js(&mut p, "log.join(',')"), "open,open");
    // toggleAttribute flips `checked`, which the menu follows.
    js(&mut p, "document.getElementById('auto').toggleAttribute('checked'); 0");
    let MenuItem::Item { items, .. } = &p.app_menu().unwrap()[0] else { panic!() };
    assert!(matches!(&items[3], MenuItem::Item { checked: false, .. }));
    assert_eq!(js(&mut p, "const a = document.getElementById('auto'); String(a.toggleAttribute('checked', true)) + a.hasAttribute('checked') + a.toggleAttribute('checked', false) + a.hasAttribute('checked')"), "truetruefalsefalse");
    // Hosts that build their own menus get it as JSON.
    let json = crate::menus::menu_json(&p.app_menu().unwrap());
    assert!(json.starts_with("[{\"id\":") && json.contains("\"label\":\"File\"") && json.contains("\"accel\":\"CmdOrCtrl+o\"") && json.contains("\"separator\":true") && json.contains("\"role\":\"quit\""), "{json}");
    js(&mut p, "Window.this.menu = null; 0");
    assert!(p.app_menu().is_none());
    let _ = std::fs::remove_file(path);
}

#[test]
fn paged_elements_edit_across_sheets() {
    let paras: String = (0..12).map(|i| format!("<p id=p{i} style=\"margin:0;font-size:16px;line-height:20px\">line {i}</p>")).collect();
    let (mut p, path) = page(
        &format!("<style>@page {{ size: 300px 140px; margin: 20px }} body{{margin:0}} #d{{page-view:paged;outline:none}}</style><div id=d contenteditable=true>{paras}</div>"),
        "paged-edit",
    );
    // 100px of content per sheet: five lines each, so three sheets.
    let painted = |p: &Page, id: &str| {
        let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        let mut v = p.view.borrow_mut();
        let offsets = v.scroll_offsets.clone();
        v.layout().node_rect_scrolled(node, &offsets).unwrap()
    };
    assert_eq!(painted(&p, "p4").y, 20.0 + 4.0 * 20.0);
    let second = painted(&p, "p5");
    assert_eq!(second.y, 140.0 + 20.0 + 20.0, "the sixth line is the first on sheet two (140px sheet, 20px gap)");
    // Click in the middle of line 5 (viewport 300px: scroll so sheet two is on screen), then type.
    p.layout(600.0, 300.0);
    p.pointer_moved(second.x + 10.0, second.y + 10.0);
    p.press();
    p.release();
    type_keys(&mut p, "X");
    let doc = p.doc.clone();
    let text = move |id: &str| {
        let node = doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        doc.borrow().text_content(node)
    };
    assert!(text("p5").contains('X'), "typed on line 5: {:?} / {:?}", text("p5"), text("p4"));
    assert!(!text("p4").contains('X'));
    // ArrowUp from the first line of sheet two goes to the last line of sheet one.
    press_key(&mut p, "ArrowUp", ModifiersState::default());
    type_keys(&mut p, "Y");
    assert!(text("p4").contains('Y'), "up crossed the sheet gap: {:?} / {:?}", text("p4"), text("p5"));
    // And ArrowDown comes back to the first line of sheet two.
    press_key(&mut p, "ArrowDown", ModifiersState::default());
    type_keys(&mut p, "Z");
    assert!(text("p5").contains('Z'), "down crossed the sheet gap: {:?}", text("p5"));
    // Scripts read the sheet count and the sheet with the caret; a page break adds one.
    assert_eq!(js(&mut p, "const d = document.getElementById('d'); d.pageCount + ',' + d.pageNumber"), "3,2");
    assert_eq!(js(&mut p, "document.execCommand('insertPageBreak'); document.getElementById('d').pageCount"), "4");
    assert_eq!(js(&mut p, "document.getElementById('d').pageNumber"), "3", "the caret starts the new sheet");
    let _ = std::fs::remove_file(path);
}

#[test]
fn documents_paginate_and_export_to_pdf() {
    let rows: String = (0..30).map(|i| format!("<p class=r>Row {i}</p>")).collect();
    let (mut p, path) = page(
        &format!("<style>@page {{ size: 400px 300px; margin: 20px; @bottom-center {{ content: \"{{n}}\" counter(page) \"/\" counter(pages) }} }} body{{margin:0}} .r{{margin:0;height:30px;break-inside:avoid}} .new{{break-before:page}}</style>{rows}<p class=new>Last</p>"),
        "paged",
    );
    // 260px of content per page: eight 30px rows each; 30 rows -> 4 pages, then a forced one.
    assert_eq!(p.paginate(), 5);
    assert_eq!(p.page_starts[1], 240.0);
    let (pixels, w, h) = p.render_page(0, 1.0).unwrap();
    assert_eq!((w, h), (400, 300));
    assert!(pixels.chunks(4).any(|px| px[..3] != [255, 255, 255]), "something was drawn");
    let mut pdf = Vec::new();
    assert_eq!(p.write_pdf(&mut pdf, "t"), 5);
    assert!(pdf.starts_with(b"%PDF-"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn checkbox_radio_and_label_clicks() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><input id="c" type="checkbox"><label id="lab" for="c">Check</label>
           <input id="r1" type="radio" name="g" checked><input id="r2" type="radio" name="g"><input id="off" type="checkbox" disabled>
           <script>
             for (const t of ['input', 'change']) document.addEventListener(t, (e) => console.log(t + ':' + e.target.id));
           </script></body>"#,
        "clicks",
    );
    let checked = |p: &Page, id: &str| {
        let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        p.doc.borrow().has_state(n, "checked")
    };
    click(&mut p, "c");
    assert!(checked(&p, "c"));
    click(&mut p, "lab"); // forwarded to the checkbox: toggles it back
    assert!(!checked(&p, "c"));
    click(&mut p, "r2");
    assert!(checked(&p, "r2") && !checked(&p, "r1"));
    click(&mut p, "r2"); // already selected: no event
    click(&mut p, "off");
    assert!(!checked(&p, "off"));
    assert_eq!(log(&p), ["input:c", "change:c", "input:c", "change:c", "input:r2", "change:r2"]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn typing_edits_value_and_fires_events() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><input id="t" value="ab"><input id="u">
           <script>
             const t = document.getElementById('t');
             t.addEventListener('input', () => console.log('input:' + t.value));
             t.addEventListener('change', () => console.log('change:' + t.value));
             t.addEventListener('keydown', (e) => { if (e.key === 'q') e.preventDefault(); });
           </script></body>"#,
        "typing",
    );
    let key = |p: &mut Page, k: &str| {
        let (proceed, _) = p.key("keydown", k.to_string(), ModifiersState::default());
        if proceed {
            p.default_key(k, ModifiersState::default());
        }
    };
    let (x, y) = center(&p, "t");
    p.pointer_moved(x + 60.0, y); // inside the input, right of the text: caret goes to the end
    p.press();
    p.release();
    assert_eq!(p.caret(), 2);
    key(&mut p, "c");
    key(&mut p, "q"); // prevented by the page
    key(&mut p, "ArrowLeft");
    key(&mut p, "Backspace");
    key(&mut p, "Home");
    key(&mut p, "Delete");
    assert_eq!(p.value_of(p.interaction().focus.unwrap()), "c");
    key(&mut p, "Enter");
    // Moving focus away fires `change` only if the value changed since the last commit.
    click(&mut p, "u");
    assert_eq!(log(&p), ["input:abc", "input:ac", "input:c", "change:c"]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn script_focus_moves_the_caret_target() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><input id="a" value="xy"><input id="b" value="z">
           <script>
             $('#a').addEventListener('focus', () => console.log('focus a'));
             $('#a').addEventListener('blur', () => console.log('blur a'));
             $('#b').focus();
             $('#a').focus();
             console.log(document.activeElement.id);
           </script></body>"#,
        "focus",
    );
    // Focus calls made while scripts load are applied once the page syncs.
    p.sync();
    assert_eq!(p.interaction().focus.map(|f| p.doc.borrow().node(f).attr("id").map(str::to_string)), Some(Some("a".into())));
    assert_eq!(p.caret(), 2);
    assert!(log(&p).contains(&"a".to_string()), "{:?}", log(&p));
    let _ = std::fs::remove_file(path);
}

fn focused_id(p: &Page) -> Option<String> {
    p.interaction().focus.and_then(|f| p.doc.borrow().node(f).attr("id").map(str::to_string))
}

#[test]
fn tab_visits_focusable_elements_in_order() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><input id="a"><input id="off" disabled><a id="link" href="#">x</a><a id="plain">y</a>
           <div id="ti" tabindex="0">t</div><div id="skip" tabindex="-1">s</div><input id="hid" type="hidden">
           <button id="late" tabindex="2">b</button><button id="early" tabindex="1">c</button>
           <div style="display:none"><input id="none"></div></body>"##,
        "tab",
    );
    let mut seen = Vec::new();
    for _ in 0..7 {
        p.default_key("Tab", ModifiersState::default());
        seen.push(focused_id(&p).unwrap());
    }
    // Positive tabindex first (ascending), then document order; wraps around.
    assert_eq!(seen, ["early", "late", "a", "link", "ti", "early", "late"]);
    let shift = ModifiersState::SHIFT;
    p.default_key("Tab", shift);
    assert_eq!(focused_id(&p).as_deref(), Some("early"));
    p.default_key("Tab", shift);
    assert_eq!(focused_id(&p).as_deref(), Some("ti"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn caret_blinks_only_while_editing() {
    let (mut p, path) = page(r#"<body style="margin:0"><input id="t"><p id="x">x</p></body>"#, "blink");
    assert_eq!(p.blink(), (false, None), "nothing focused: no blink timer");
    click(&mut p, "t");
    assert!(p.caret_visible);
    assert!(p.caret_for_paint().is_some());
    p.blink_at = Instant::now() - Duration::from_millis(1);
    let (changed, next) = p.blink();
    assert!(changed && !p.caret_visible && next.is_some());
    assert!(p.caret_for_paint().is_none(), "caret hidden in the off phase");
    p.default_key("a", ModifiersState::default());
    assert!(p.caret_visible, "typing shows the caret again");
    click(&mut p, "x");
    assert_eq!(p.blink(), (false, None));
    let _ = std::fs::remove_file(path);
}

#[test]
fn textarea_editing_with_newlines_and_vertical_moves() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><textarea id="t" style="width:200px">ab
cd</textarea>
           <script>$('#t').addEventListener('input', () => console.log(JSON.stringify($('#t').value)));</script></body>"#,
        "textarea",
    );
    let key = |p: &mut Page, k: &str| {
        p.default_key(k, ModifiersState::default());
    };
    click(&mut p, "t");
    // The click lands at the middle of the box, below the text: caret goes to the end.
    assert_eq!(p.value_of(p.interaction().focus.unwrap()), "ab\ncd");
    key(&mut p, "ArrowUp"); // up from the second line keeps the column
    assert!(p.caret() <= 2, "caret on the first line, got {}", p.caret());
    key(&mut p, "End");
    key(&mut p, "Enter"); // newline after "ab"
    key(&mut p, "X");
    key(&mut p, "ArrowDown");
    key(&mut p, "ArrowDown");
    key(&mut p, "ArrowDown"); // past the last line: end of text
    key(&mut p, "Y");
    assert_eq!(log(&p).last().map(String::as_str), Some("\"ab\\nX\\ncdY\""));
    let _ = std::fs::remove_file(path);
}

fn offset(p: &Page, id: &str) -> (f32, f32) {
    let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
    p.view.borrow().scroll_offsets.get(&n).copied().unwrap_or_default()
}

#[test]
fn scroll_snap_settles_on_the_next_snap_point_after_the_wheel_pauses() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="box" style="width:200px; height:60px; overflow-x:auto; overflow-y:hidden; display:flex; scroll-snap-type:x mandatory">
             <div style="flex:none; width:200px; scroll-snap-align:start">a</div><div style="flex:none; width:200px; scroll-snap-align:start">b</div>
             <div style="flex:none; width:200px; scroll-snap-align:start">c</div></div></body>"#,
        "snap",
    );
    let (x, y) = center(&p, "box");
    p.pointer_moved(x, y);
    p.wheel(-30.0, 0.0);
    assert_eq!(offset(&p, "box"), (30.0, 0.0), "the wheel moves freely while it is running");
    std::thread::sleep(Duration::from_millis(200));
    p.tick();
    assert_eq!(offset(&p, "box"), (200.0, 0.0), "then it settles on the next item");
    // A small nudge back goes back to the item it came from.
    p.wheel(10.0, 0.0);
    std::thread::sleep(Duration::from_millis(200));
    p.tick();
    assert_eq!(offset(&p, "box"), (0.0, 0.0));
    let _ = std::fs::remove_file(path);
}

#[test]
fn wheel_scrolls_the_innermost_box_then_chains_to_the_page() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="box" style="width:100px; height:50px; overflow:auto">
             <div style="height:40px">a</div><div style="height:40px">b</div><div style="height:40px">c</div></div>
           <div style="height:600px">tall</div>
           <script>
             $('#box').addEventListener('scroll', () => console.log('scroll:' + $('#box').scrollTop));
           </script></body>"#,
        "wheel",
    );
    let (x, y) = center(&p, "box");
    p.pointer_moved(x, y);
    p.wheel(0.0, -30.0); // wheel down: scroll content up by 30
    assert_eq!(offset(&p, "box"), (0.0, 30.0));
    assert!(p.view.borrow().layout.is_some(), "overflow scrolling must preserve cached geometry");
    p.wheel(0.0, -100.0); // clamps at the end (120 - 50 = 70)
    assert_eq!(offset(&p, "box"), (0.0, 70.0));
    assert_eq!(p.view.borrow().scroll_y, 0.0);
    p.wheel(0.0, -40.0); // box is at its limit: the page takes it
    assert_eq!(offset(&p, "box"), (0.0, 70.0));
    assert_eq!(p.view.borrow().scroll_y, 40.0);
    p.pointer_moved(x, 5.0); // the page moved under the pointer: back over the box
    p.wheel(0.0, 100.0); // wheel up: the box scrolls back first
    assert_eq!(offset(&p, "box"), (0.0, 0.0));
    assert_eq!(log(&p), ["scroll:30", "scroll:70", "scroll:0"]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn scripts_scroll_elements_and_the_caret_stays_visible() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="box" style="width:100px; height:50px; overflow:hidden">
             <div style="height:200px; width:300px">x</div></div>
           <textarea id="t" style="width:100px; height:40px"></textarea>
           <script>
             const b = $('#box');
             b.scrollTop = 80; b.scrollLeft = 500;
             console.log(b.scrollTop, b.scrollLeft, b.scrollWidth, b.scrollHeight, b.clientHeight);
             b.scrollBy(-30, 0); b.scrollTo({ top: 5 });
             console.log(b.scrollLeft, b.scrollTop);
             console.log(document.createElement('p').scrollTop);
           </script></body>"#,
        "scrolljs",
    );
    assert_eq!(log(&p)[0], "80 200 300 200 50");
    assert_eq!(log(&p)[1], "170 5");
    assert_eq!(log(&p)[2], "0");
    // Typing past the bottom of the textarea scrolls it to keep the caret in view.
    click(&mut p, "t");
    for k in ["a", "Enter", "b", "Enter", "c", "Enter", "d", "Enter", "e"] {
        p.default_key(k, ModifiersState::default());
    }
    assert!(offset(&p, "t").1 > 0.0, "scrolled to the caret: {:?}", offset(&p, "t"));
    p.default_key("Home", ModifiersState::default());
    for _ in 0..4 {
        p.default_key("ArrowUp", ModifiersState::default());
    }
    assert_eq!(offset(&p, "t").1, 0.0, "back at the top");
    let _ = std::fs::remove_file(path);
}

/// Press a key (keydown + default action) with modifiers.
fn press_key(p: &mut Page, k: &str, mods: ModifiersState) {
    let (proceed, _) = p.key("keydown", k.to_string(), mods);
    if proceed {
        p.default_key(k, mods);
    }
}

fn val(p: &Page, id: &str) -> String {
    let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
    p.value_of(n)
}

#[test]
fn selection_with_keyboard_and_typing_over_it() {
    let (mut p, path) = page(r#"<body style="margin:0"><input id="t" value="hello world"></body>"#, "sel-keys");
    let none = ModifiersState::default();
    click(&mut p, "t"); // caret at the end (click is right of the text)
    press_key(&mut p, "Home", none);
    for _ in 0..5 {
        press_key(&mut p, "ArrowRight", ModifiersState::SHIFT);
    }
    assert_eq!((p.anchor(), p.caret()), (0, 5));
    assert!(p.selection_for_paint().is_some());
    press_key(&mut p, "ArrowRight", none); // collapses to the end of the selection
    assert_eq!((p.anchor(), p.caret()), (5, 5));
    press_key(&mut p, "End", ModifiersState::SHIFT);
    assert_eq!((p.anchor(), p.caret()), (5, 11));
    press_key(&mut p, "X", none); // typing replaces the selection
    assert_eq!(val(&p, "t"), "helloX");
    press_key(&mut p, "ArrowLeft", ModifiersState::SHIFT);
    press_key(&mut p, "Backspace", none); // deletes just the selection
    assert_eq!(val(&p, "t"), "hello");
    let _ = std::fs::remove_file(path);
}

#[test]
fn undo_redo_groups_typing_and_backspace_runs() {
    let (mut p, path) = page(r#"<body style="margin:0"><input id="t"><input id="u" value="hello"></body>"#, "undo");
    let none = ModifiersState::default();
    let ctrl = ModifiersState::CONTROL;
    click(&mut p, "t");
    for c in ["a", "b", "c"] {
        press_key(&mut p, c, none);
    }
    assert_eq!(val(&p, "t"), "abc");
    press_key(&mut p, "z", ctrl); // the whole run is one step
    assert_eq!(val(&p, "t"), "");
    press_key(&mut p, "z", ctrl | ModifiersState::SHIFT);
    assert_eq!((val(&p, "t").as_str(), p.caret()), ("abc", 3));
    // Moving the caret ends a run.
    press_key(&mut p, "ArrowLeft", none);
    press_key(&mut p, "X", none);
    assert_eq!(val(&p, "t"), "abXc");
    press_key(&mut p, "z", ctrl);
    assert_eq!((val(&p, "t").as_str(), p.caret()), ("abc", 2));
    press_key(&mut p, "z", ctrl);
    assert_eq!(val(&p, "t"), "");
    press_key(&mut p, "z", ctrl); // nothing left to undo
    assert_eq!(val(&p, "t"), "");
    // A new edit drops what could be redone.
    press_key(&mut p, "q", none);
    press_key(&mut p, "z", ctrl | ModifiersState::SHIFT);
    assert_eq!(val(&p, "t"), "q");

    click(&mut p, "u");
    press_key(&mut p, "End", none);
    for _ in 0..3 {
        press_key(&mut p, "Backspace", none);
    }
    assert_eq!(val(&p, "u"), "he");
    press_key(&mut p, "z", ctrl);
    assert_eq!(val(&p, "u"), "hello");
    let _ = std::fs::remove_file(path);
}

#[test]
fn word_wise_movement_and_deletion() {
    let (mut p, path) = page(r#"<body style="margin:0"><input id="t" value="one two, three"></body>"#, "words");
    let none = ModifiersState::default();
    let alt = ModifiersState::ALT;
    click(&mut p, "t");
    press_key(&mut p, "End", none);
    press_key(&mut p, "ArrowLeft", alt);
    assert_eq!(p.caret(), 9); // start of "three"
    press_key(&mut p, "ArrowLeft", alt);
    assert_eq!(p.caret(), 4);
    press_key(&mut p, "ArrowRight", alt);
    assert_eq!(p.caret(), 7); // end of "two"
    press_key(&mut p, "ArrowRight", alt | ModifiersState::SHIFT);
    assert_eq!((p.anchor(), p.caret()), (7, 14));
    press_key(&mut p, "ArrowLeft", none);
    press_key(&mut p, "End", none);
    press_key(&mut p, "Backspace", alt);
    assert_eq!(val(&p, "t"), "one two, ");
    press_key(&mut p, "Home", none);
    press_key(&mut p, "Delete", alt);
    assert_eq!(val(&p, "t"), " two, ");
    let _ = std::fs::remove_file(path);
}

#[test]
fn keyboard_scrolls_the_page_and_scrollers() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="box" tabindex="0" style="width:100px; height:80px; overflow:auto"><div style="height:400px">in</div></div>
           <input id="t" value="x"><div style="height:1000px">tall</div></body>"#,
        "keyscroll",
    );
    let none = ModifiersState::default();
    // Nothing focused or hovered: the page scrolls.
    press_key(&mut p, "ArrowDown", none);
    assert_eq!(p.view.borrow().scroll_y, 40.0);
    press_key(&mut p, "PageDown", none); // 0.875 of the 300px viewport
    assert_eq!(p.view.borrow().scroll_y, 40.0 + 262.5);
    press_key(&mut p, " ", ModifiersState::SHIFT);
    assert_eq!(p.view.borrow().scroll_y, 40.0);
    press_key(&mut p, "End", none);
    let end = p.view.borrow().scroll_y;
    assert!(end > 700.0 && end < 900.0, "{end}");
    press_key(&mut p, "ArrowDown", none); // already at the end
    assert_eq!(p.view.borrow().scroll_y, end);
    press_key(&mut p, "Home", none);
    assert_eq!(p.view.borrow().scroll_y, 0.0);
    // A focused scroller takes the keys first.
    click(&mut p, "box");
    press_key(&mut p, "PageDown", none);
    assert_eq!(offset(&p, "box"), (0.0, 70.0));
    assert_eq!(p.view.borrow().scroll_y, 0.0);
    // In a text input the keys are for editing.
    click(&mut p, "t");
    press_key(&mut p, "ArrowDown", none);
    assert_eq!(p.view.borrow().scroll_y, 0.0);
    let _ = std::fs::remove_file(path);
}

#[test]
fn scrollbars_drag_and_jump() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="box" style="width:100px; height:80px; overflow:auto"><div style="height:400px">in</div></div>
           <div style="height:1000px">tall</div></body>"#,
        "bars",
    );
    // The box's thumb: 16px long at the top of a 80px track (x 92..98); 320px scroll range.
    p.pointer_moved(95.0, 8.0);
    assert!(p.press());
    p.pointer_moved(95.0, 40.0); // 32 of the 64 free px
    assert_eq!(offset(&p, "box"), (0.0, 160.0));
    p.pointer_moved(95.0, 500.0); // dragging past the end clamps
    assert_eq!(offset(&p, "box"), (0.0, 320.0));
    p.pointer_moved(10.0, 10.0); // hover doesn't change while a thumb is held
    assert!(p.release());
    // Clicking its track jumps there (thumb centred on the pointer).
    p.pointer_moved(95.0, 48.0);
    p.press();
    p.release();
    let y = offset(&p, "box").1;
    assert!((y - 160.0).abs() < 8.0 * 5.0 + 1.0, "{y}"); // within a few px of the middle
    // The page's own bar sits at the right edge of the 600x300 viewport.
    let before = p.view.borrow().scroll_y;
    p.pointer_moved(595.0, 150.0);
    assert!(p.press());
    p.pointer_moved(595.0, 290.0);
    p.release();
    let after = p.view.borrow().scroll_y;
    assert!(after > before + 300.0, "{after}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn scrollbars_follow_scrollbar_width_colors_and_the_pointer() {
    let (mut p, path) = page(
        r##"<head><style>
             #thin { scrollbar-width: thin }
             #none { scrollbar-width: none }
             #wk::-webkit-scrollbar { width: 14px }
             #wk::-webkit-scrollbar-thumb { background: #ff0000; border-radius: 2px }
             #col { scrollbar-color: #00ff00 #0000ff }
           </style></head>
           <body style="margin:0"><div id="a" style="width:100px;height:80px;overflow:auto"><div style="height:400px">in</div></div>
           <div id="thin" style="width:100px;height:80px;overflow:auto"><div style="height:400px">in</div></div>
           <div id="none" style="width:100px;height:80px;overflow:auto"><div style="height:400px">in</div></div>
           <div id="wk" style="width:100px;height:80px;overflow:auto"><div style="height:400px">in</div></div>
           <div id="col" style="width:100px;height:80px;overflow:auto"><div style="height:400px">in</div></div></body>"##,
        "barlook",
    );
    let thick = |p: &mut Page, id: &str| {
        let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        let v = p.view.borrow_mut();
        let look = v.styles.scrollbar_look(node);
        (look.thickness, look.thumb.map(|c| (c.r, c.g, c.b)), look.track.map(|c| (c.r, c.g, c.b)), look.thumb_radius)
    };
    assert_eq!(thick(&mut p, "a"), (nile_style::scrollbar_auto(), None, None, None));
    assert_eq!(thick(&mut p, "thin").0, nile_style::scrollbar_thin());
    assert_eq!(thick(&mut p, "none").0, 0.0);
    assert_eq!(thick(&mut p, "wk"), (14.0, Some((255, 0, 0)), None, Some(2.0)));
    assert_eq!(thick(&mut p, "col"), (nile_style::scrollbar_auto(), Some((0, 255, 0)), Some((0, 0, 255)), None));
    // A `none` bar has nothing to grab, but the wheel still scrolls.
    let node = p.doc.borrow().find(|n| n.attr("id") == Some("none")).unwrap();
    let sb = p.view.borrow_mut().layout().scroll_box(node).copied().unwrap();
    assert!(sb.thumbs(0.0, 0.0).is_empty() && sb.wheel_y);
    // The pointer over a bar marks it hovered; leaving clears it.
    p.pointer_moved(95.0, 20.0);
    assert!(p.bar_hover.is_some_and(|(n, v)| n.is_some() && v));
    p.pointer_moved(30.0, 20.0);
    assert_eq!(p.bar_hover, None);
    p.pointer_moved(95.0, 20.0);
    assert!(p.pointer_left() && p.bar_hover.is_none());
    let _ = std::fs::remove_file(path);
}

fn selected(p: &Page) -> String {
    p.view.borrow_mut().page_selected_text()
}

#[test]
fn page_text_is_only_selectable_when_asked_for() {
    let (mut p, path) = page(
        r#"<body style="margin:0; font-size:20px"><p id="plain" style="margin:0">not selectable</p>
           <p id="attr" selectable style="margin:0">with attribute</p>
           <div style="behavior: htmlarea"><p id="beh" style="margin:0">sciter behavior</p></div>
           <p id="all" style="margin:0; user-select: all">click selects everything here</p>
           <p id="off" selectable style="margin:0"><span id="inner" style="user-select: none">locked</span></p></body>"#,
        "userselect",
    );
    let drag = |p: &mut Page, id: &str| {
        let (_, y) = center(p, id);
        p.pointer_moved(1.0, y);
        p.press();
        p.pointer_moved(590.0, y);
        p.release();
    };
    drag(&mut p, "plain");
    assert_eq!(selected(&p), "");
    drag(&mut p, "attr");
    assert_eq!(selected(&p), "with attribute");
    drag(&mut p, "beh");
    assert_eq!(selected(&p), "sciter behavior");
    // `user-select: all`: one click takes the whole run.
    let (_, y) = center(&p, "all");
    p.pointer_moved(40.0, y);
    p.press();
    p.release();
    assert_eq!(selected(&p), "click selects everything here");
    // Select all skips what can't be selected.
    press_key(&mut p, "a", ModifiersState::CONTROL);
    let all = selected(&p);
    assert!(all.contains("with attribute") && all.contains("sciter behavior") && !all.contains("not selectable"), "{all}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn page_text_selection_by_drag_word_run_and_copy() {
    let (mut p, path) = page(
        r#"<body style="margin:0; font-size:20px; user-select:text"><p id="a" style="margin:0">alpha beta gamma</p>
           <p id="b" style="margin:0">second <b>bold</b> tail</p><input id="t" value="inside">
           <script>document.addEventListener('click', () => console.log('sel:' + window.getSelection().toString()))</script></body>"#,
        "pagesel",
    );
    // Drag from the first paragraph's left edge to the second's right side.
    let (_, ay) = center(&p, "a");
    let (_, by) = center(&p, "b");
    p.pointer_moved(1.0, ay);
    p.press();
    p.pointer_moved(560.0, by);
    p.release();
    assert_eq!(selected(&p), "alpha beta gamma\nsecond bold tail");
    // Copying puts it on the clipboard.
    press_key(&mut p, "c", ModifiersState::CONTROL);
    assert_eq!(p.clipboard.text().as_deref(), Some("alpha beta gamma\nsecond bold tail"));
    // A double-click picks the word under the pointer, a triple-click the run.
    p.pointer_moved(80.0, ay);
    p.press();
    p.release();
    p.press();
    p.release();
    assert_eq!(selected(&p), "beta");
    p.press();
    p.release();
    assert_eq!(selected(&p), "alpha beta gamma");
    // Select all, then clicking an input drops the page selection.
    press_key(&mut p, "a", ModifiersState::CONTROL);
    assert_eq!(selected(&p), "alpha beta gamma\nsecond bold tail");
    click(&mut p, "t");
    assert_eq!(selected(&p), "");
    press_key(&mut p, "a", ModifiersState::CONTROL); // now it selects the input's text
    assert_eq!(selected(&p), "");
    let _ = std::fs::remove_file(path);
}

#[test]
fn select_all_copy_cut_and_paste() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><input id="t" value="abc def"><textarea id="m"></textarea>
           <script>$('#t').addEventListener('input', () => console.log('input:' + $('#t').value));</script></body>"#,
        "clipboard",
    );
    let cmd = ModifiersState::SUPER;
    click(&mut p, "t");
    press_key(&mut p, "a", cmd);
    assert_eq!((p.anchor(), p.caret()), (0, 7));
    press_key(&mut p, "c", cmd);
    assert_eq!(p.clipboard.text().as_deref(), Some("abc def"));
    press_key(&mut p, "Home", ModifiersState::default());
    press_key(&mut p, "ArrowRight", ModifiersState::SHIFT);
    press_key(&mut p, "ArrowRight", ModifiersState::SHIFT);
    press_key(&mut p, "ArrowRight", ModifiersState::SHIFT);
    press_key(&mut p, "x", cmd); // cut "abc"
    assert_eq!(val(&p, "t"), " def");
    assert_eq!(p.clipboard.text().as_deref(), Some("abc"));
    press_key(&mut p, "End", ModifiersState::default());
    press_key(&mut p, "v", cmd);
    assert_eq!(val(&p, "t"), " defabc");
    // Single-line inputs flatten pasted newlines; textareas keep them.
    p.clipboard.set_text("l1\r\nl2");
    press_key(&mut p, "v", cmd);
    assert_eq!(val(&p, "t"), " defabcl1 l2");
    click(&mut p, "m");
    press_key(&mut p, "v", cmd);
    assert_eq!(val(&p, "m"), "l1\nl2");
    assert!(log(&p).iter().filter(|l| l.starts_with("input:")).count() >= 3);
    let _ = std::fs::remove_file(path);
}

#[test]
fn mouse_selection_drag_double_and_triple_click() {
    let (mut p, path) = page(r#"<body style="margin:0"><input id="t" value="one two three" style="width:300px"></body>"#, "sel-mouse");
    let r = {
        let n = p.doc.borrow().find(|n| n.attr("id") == Some("t")).unwrap();
        p.view.borrow_mut().layout().node_rect(n).unwrap()
    };
    let y = r.y + r.height / 2.0;
    // Drag from the left edge of the text to the right: selects a prefix.
    p.pointer_moved(r.x + 6.0, y);
    p.press();
    p.pointer_moved(r.x + 40.0, y);
    p.release();
    let (a, c) = (p.anchor(), p.caret());
    assert!(a == 0 && c > 2 && c < 8, "dragged selection {a}..{c}");
    // Double-click selects the word under the pointer (a new click far enough away).
    p.last_click = None;
    p.pointer_moved(r.x + 38.0, y);
    p.press();
    p.release();
    p.press();
    p.release();
    assert_eq!(val(&p, "t")[p.anchor()..p.caret()].trim(), "two", "word at {}..{}", p.anchor(), p.caret());
    // Triple-click selects everything.
    p.press();
    p.release();
    assert_eq!((p.anchor(), p.caret()), (0, 13));
    let _ = std::fs::remove_file(path);
}

#[test]
fn script_selection_api_and_tab_selects_all() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><input id="a" value="abcdef"><input id="b" value="xyz">
           <script>
             const a = $('#a'), b = $('#b');
             b.setSelectionRange(1, 2);              // before b has focus
             console.log(a.selectionStart, a.selectionEnd);   // unfocused: collapsed at the end
             a.focus();
             a.setSelectionRange(2, 4);
             console.log(a.selectionStart, a.selectionEnd);
             a.select();
             console.log(a.selectionStart, a.selectionEnd, document.createElement('p').selectionStart);
             b.focus();
             console.log(b.selectionStart, b.selectionEnd);   // the pending range applied on focus
           </script></body>"#,
        "sel-js",
    );
    p.sync();
    assert_eq!(log(&p), ["6 6", "2 4", "0 6 null", "1 2"]);
    // Tab into an input selects its contents.
    p.default_key("Tab", ModifiersState::default());
    assert_eq!((p.anchor(), p.caret()), (0, 6), "wrapped to the first input, all selected");
    p.default_key("Tab", ModifiersState::default());
    assert_eq!((p.anchor(), p.caret()), (0, 3), "then the second");
    let _ = std::fs::remove_file(path);
}

fn sel_index(p: &Page, id: &str) -> usize {
    let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
    nile_layout::selected_index(&p.doc.borrow(), n).unwrap()
}

/// Viewport point at the middle of row `row` of the open menu.
fn row_point(p: &mut Page, row: usize) -> (f32, f32) {
    let mut v = p.view.borrow_mut();
    let m = v.layout().menu.as_ref().expect("menu is open");
    (m.rect.x + 10.0, m.rect.y + 1.0 + m.row_height * (row as f32 + 0.5))
}

const SELECT_PAGE: &str = r#"<body style="margin:0"><select id="s"><option value="r">Red</option><option value="g" disabled>Green</option><option value="b">Blue</option><option value="y">Yellow</option></select><p id="below">below</p>
    <script>
      for (const t of ['input', 'change']) $('#s').addEventListener(t, () => console.log(t + ':' + $('#s').value));
    </script></body>"#;

#[test]
fn select_opens_chooses_with_the_mouse_and_closes() {
    let (mut p, path) = page(SELECT_PAGE, "select-mouse");
    click(&mut p, "s");
    assert!(p.view.borrow().menu.is_some(), "click opens the drop-down");
    assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(0));
    // Hovering a row highlights it; a disabled row doesn't take it.
    let (x, y) = row_point(&mut p, 2);
    p.pointer_moved(x, y);
    assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(2));
    let (x, y) = row_point(&mut p, 1);
    p.pointer_moved(x, y);
    assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(2), "disabled row can't be highlighted");
    p.press();
    p.release();
    assert!(p.view.borrow().menu.is_some(), "releasing on a disabled row keeps it open");
    // Choose Blue.
    let (x, y) = row_point(&mut p, 2);
    p.pointer_moved(x, y);
    p.press();
    p.release();
    assert!(p.view.borrow().menu.is_none());
    assert_eq!(sel_index(&p, "s"), 2);
    // Opening and clicking outside closes without changing anything.
    click(&mut p, "s");
    let (bx, by) = center(&p, "below");
    p.pointer_moved(bx + 200.0, by + 100.0);
    p.press();
    p.release();
    assert!(p.view.borrow().menu.is_none());
    assert_eq!(sel_index(&p, "s"), 2);
    assert_eq!(log(&p), ["input:b", "change:b"]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn select_keyboard_navigation() {
    let (mut p, path) = page(SELECT_PAGE, "select-keys");
    let none = ModifiersState::default();
    click(&mut p, "s"); // open
    press_key(&mut p, "ArrowDown", none); // skips the disabled Green
    assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(2));
    press_key(&mut p, "End", none);
    assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(3));
    press_key(&mut p, "ArrowDown", none); // stays at the end
    assert_eq!(p.view.borrow().menu.unwrap().highlight, Some(3));
    press_key(&mut p, "Escape", none);
    assert!(p.view.borrow().menu.is_none());
    assert_eq!(sel_index(&p, "s"), 0, "escape keeps the choice");
    // Closed: arrows change the choice directly, with events.
    press_key(&mut p, "ArrowDown", none);
    assert_eq!(sel_index(&p, "s"), 2);
    press_key(&mut p, "y", none); // type-ahead
    assert_eq!(sel_index(&p, "s"), 3);
    press_key(&mut p, "ArrowDown", none);
    assert_eq!(sel_index(&p, "s"), 3, "no wrap at the end");
    press_key(&mut p, " ", none); // opens
    assert!(p.view.borrow().menu.is_some());
    press_key(&mut p, "Home", none);
    press_key(&mut p, "Enter", none); // choose Red
    assert!(p.view.borrow().menu.is_none());
    assert_eq!(sel_index(&p, "s"), 0);
    assert_eq!(log(&p), ["input:b", "change:b", "input:y", "change:y", "input:r", "change:r"]);
    let _ = std::fs::remove_file(path);
}

const LIST_PAGE: &str = r#"<body style="margin:0"><select id="l" multiple size="4"><option>A</option><option>B</option><option disabled>C</option><option>D</option><option>E</option><option>F</option></select>
    <script>$('#l').addEventListener('change', () => console.log('change:' + $('#l').selectedOptions.map((o) => o.text).join('')));</script></body>"#;

/// Viewport point in row `row` of the list box.
fn list_row_point(p: &mut Page, row: usize) -> (f32, f32) {
    let id = p.doc.borrow().find(|n| n.attr("id") == Some("l")).unwrap();
    let mut v = p.view.borrow_mut();
    let r = v.layout().node_rect(id).unwrap();
    let l = v.layout().list_box(id).unwrap();
    (r.x + 10.0, r.y + l.inset_top - l.scroll + l.row_height * (row as f32 + 0.5))
}

fn selected_rows(p: &Page) -> Vec<usize> {
    let doc = p.doc.borrow();
    let id = doc.find(|n| n.attr("id") == Some("l")).unwrap();
    nile_layout::select_options(&doc, id).iter().enumerate().filter(|(_, o)| doc.has_state(**o, "selected")).map(|(i, _)| i).collect()
}

#[test]
fn list_box_selects_rows_with_modifiers_keys_and_the_wheel() {
    let (mut p, path) = page(LIST_PAGE, "list-box");
    let click_row = |p: &mut Page, row: usize, mods: ModifiersState| {
        let (x, y) = list_row_point(p, row);
        p.mods = mods;
        p.pointer_moved(x, y);
        p.press();
        p.release();
        p.mods = ModifiersState::default();
    };
    let none = ModifiersState::default();
    click_row(&mut p, 1, none);
    assert_eq!(selected_rows(&p), [1]);
    assert!(p.view.borrow().menu.is_none(), "a list box has no drop-down");
    click_row(&mut p, 3, ModifiersState::SUPER);
    assert_eq!(selected_rows(&p), [1, 3], "cmd-click adds");
    click_row(&mut p, 1, ModifiersState::SUPER);
    assert_eq!(selected_rows(&p), [3], "cmd-click toggles off");
    click_row(&mut p, 0, ModifiersState::SHIFT);
    assert_eq!(selected_rows(&p), [0, 1], "shift-click replaces the selection with anchor..row");
    click_row(&mut p, 2, none);
    assert_eq!(selected_rows(&p), [0, 1], "disabled rows can't be chosen");
    // Keys, with the list still focused.
    press_key(&mut p, "ArrowDown", none);
    assert_eq!(selected_rows(&p), [1]);
    press_key(&mut p, "ArrowDown", none);
    assert_eq!(selected_rows(&p), [3], "moves past the disabled row");
    press_key(&mut p, "ArrowDown", ModifiersState::SHIFT);
    assert_eq!(selected_rows(&p), [3, 4]);
    press_key(&mut p, "a", ModifiersState::SUPER);
    assert_eq!(selected_rows(&p), [0, 1, 3, 4, 5]);
    press_key(&mut p, "Home", none);
    assert_eq!(selected_rows(&p), [0]);
    // The wheel scrolls the rows (4 visible of 6).
    let (x, y) = list_row_point(&mut p, 1);
    p.pointer_moved(x, y);
    p.wheel(0.0, -30.0);
    let id = p.doc.borrow().find(|n| n.attr("id") == Some("l")).unwrap();
    assert!(p.view.borrow_mut().layout().list_box(id).unwrap().scroll > 0.0);
    press_key(&mut p, "End", none);
    let l = p.view.borrow_mut().layout().list_box(id).unwrap().scroll;
    assert!(l > 0.0 && selected_rows(&p) == [5], "End selects and scrolls to the last row");
    assert_eq!(log(&p).last().map(String::as_str), Some("change:F"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn paint_only_restyles_keep_the_layout_and_layout_changes_drop_it() {
    let (mut p, path) = page(
        r#"<style>
             #a { width: 100px; height: 40px; background: #eee; transform: rotate(0deg) }
             #a:hover { background: #09f; box-shadow: 0 0 4px #000; transform: rotate(3deg) }
             #b { width: 100px; height: 40px }
             #b:hover { height: 80px }
           </style><body style="margin:0"><div id="a"></div><div id="b"></div></body>"#,
        "paint-only",
    );
    let built = |p: &Page| p.view.borrow().layout.is_some();
    assert!(built(&p));
    let (x, y) = center(&p, "a");
    p.pointer_moved(x, y);
    assert!(built(&p), "colors, shadows and transforms don't need a new layout");
    let a = p.doc.borrow().find(|n| n.attr("id") == Some("a")).unwrap();
    let xf = p.view.borrow_mut().layout().boxes.iter().find(|b| b.node == a).unwrap().transform.unwrap();
    assert!(xf.b != 0.0, "but the box picks up its new transform");
    p.layout(600.0, 300.0);
    let (x, y) = center(&p, "b");
    p.pointer_moved(x, y);
    assert!(!built(&p), "a height change does");
    let _ = std::fs::remove_file(path);
}

#[test]
fn cursor_follows_the_hovered_element() {
    use nile_style::Cursor;
    let (mut p, path) = page(
        r##"<body style="margin:0"><a id="l" href="#" style="display:block;height:20px">link</a>
           <input id="i"><div id="g" style="height:20px;cursor:grab"><span id="s">x</span></div><div id="d" style="height:20px">plain</div></body>"##,
        "cursor",
    );
    let at = |p: &mut Page, id: &str| {
        let (x, y) = center(p, id);
        p.pointer_moved(x, y);
        p.cursor()
    };
    assert_eq!(at(&mut p, "l"), Cursor::Pointer);
    assert_eq!(at(&mut p, "i"), Cursor::Text);
    assert_eq!(at(&mut p, "s"), Cursor::Grab, "inherited by children");
    assert_eq!(at(&mut p, "d"), Cursor::Auto);
    let _ = std::fs::remove_file(path);
}

#[test]
fn range_progress_meter_and_file_inputs_show_the_arrow() {
    use nile_style::Cursor;
    let (mut p, path) = page(
        r##"<body style="margin:0"><input id="r" type="range"><br><progress id="p" value="3" max="10"></progress><br><meter id="m" value="0.4"></meter><br><input id="f" type="file"><br><input id="t" type="text"></body>"##,
        "cursor-controls",
    );
    let mut at = |id: &str| {
        let (x, y) = center(&p, id);
        p.pointer_moved(x, y);
        p.cursor()
    };
    assert_eq!((at("r"), at("p"), at("m"), at("f")), (Cursor::Default, Cursor::Default, Cursor::Default, Cursor::Default));
    assert_eq!(at("t"), Cursor::Text);
    let _ = std::fs::remove_file(path);
}

#[test]
fn window_roles_and_script_calls_queue_window_commands() {
    let (mut p, path) = page(
        r#"<html window-frame="solid"><body style="margin:0">
             <div id="bar" role="window-caption" style="height:30px"><span id="t">Title</span>
               <button id="min" role="window-minimize">_</button><button id="max" role="window-maximize">o</button>
               <button id="x" role="window-close">x</button><button id="plain">p</button></div>
             <script>
               $('#plain').addEventListener('click', () => { document.title = 'Renamed'; window.moveTo(10, 20); window.resizeTo(500, 400); window.close(); });
             </script></body></html>"#,
        "window-roles",
    );
    let take = |p: &Page| std::mem::take(&mut p.view.borrow_mut().window_requests);
    click(&mut p, "min");
    click(&mut p, "max");
    click(&mut p, "x");
    assert_eq!(take(&p), [WindowCommand::Minimize, WindowCommand::ToggleMaximize, WindowCommand::Close]);
    // Pressing the caption starts a drag; a quick second press toggles maximize; buttons don't drag.
    let (x, y) = center(&p, "t");
    p.pointer_moved(x, y);
    assert!(p.caption_press(), "first press drags");
    assert!(!p.caption_press(), "second press maximizes instead");
    assert_eq!(take(&p), [WindowCommand::ToggleMaximize]);
    let (x, y) = center(&p, "plain");
    p.pointer_moved(x, y);
    assert!(!p.caption_press(), "a button in the caption keeps its click");
    click(&mut p, "plain");
    assert_eq!(
        take(&p),
        [WindowCommand::MoveTo(10.0, 20.0), WindowCommand::ResizeTo(500.0, 400.0), WindowCommand::Close]
    );
    let doc = p.doc.borrow();
    assert_eq!(window::current_title(&doc, &WindowConfig::from_document(&doc)), "Renamed");
    drop(doc);
    let _ = std::fs::remove_file(path);
}

/// How many pixels in a painted page are strongly `rgb`-ish red / blue.
fn count_colored(p: &mut Page) -> (usize, usize) {
    let mut px = vec![0u8; 300 * 120 * 4];
    p.paint(&mut px, 300, 120, 1.0);
    let reds = px.chunks(4).filter(|c| c[2] > 180 && c[1] < 80 && c[0] < 80).count();
    let blues = px.chunks(4).filter(|c| c[0] > 180 && c[1] < 80 && c[2] < 80).count();
    (reds, blues)
}

#[test]
fn text_color_changes_repaint_without_a_new_layout_but_inline_backgrounds_do_relayout() {
    let (mut p, path) = page(
        r#"<style>
             #a { width: 280px; height: 30px; color: #00f; font-size: 22px } #a:hover { color: #f00 }
             #s { background: #00f; color: #fff } #s:hover { background: #f00 }
           </style><body style="margin:0"><div id="a">WWWWWWWW</div><div><span id="s">inline span here</span></div></body>"#,
        "text-color",
    );
    let built = |p: &Page| p.view.borrow().layout.is_some();
    let (r0, b0) = count_colored(&mut p);
    assert!(r0 == 0 && b0 > 100, "blue text to begin with: {r0} {b0}");
    let (x, y) = center(&p, "a");
    p.pointer_moved(x, y);
    assert!(built(&p), "a text color change keeps the layout");
    let (r1, _) = count_colored(&mut p);
    assert!(r1 > 100, "and the text turns red: {r1}");
    // An inline box's background is baked into its text run, so that one lays out again.
    let (x, y) = center(&p, "s");
    p.pointer_moved(x, y);
    assert!(!built(&p));
    let _ = std::fs::remove_file(path);
}

#[test]
fn clicking_a_summary_toggles_its_details() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><details id="d"><summary id="s">More</summary><p id="body">Hidden</p></details>
           <script>$('#d').addEventListener('toggle', () => console.log('toggle:' + $('#d').hasAttribute('open')));</script></body>"#,
        "details",
    );
    let shown = |p: &mut Page| {
        let id = p.doc.borrow().find(|n| n.attr("id") == Some("body")).unwrap();
        p.view.borrow_mut().layout().node_rect(id).is_some()
    };
    assert!(!shown(&mut p), "closed details hide their content");
    click(&mut p, "s");
    assert!(shown(&mut p), "open shows it");
    click(&mut p, "s");
    assert!(!shown(&mut p));
    assert_eq!(log(&p), ["toggle:true", "toggle:false"]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn range_sliders_follow_the_pointer_and_keys_and_fire_events() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><input id="r" type="range" min="0" max="10" value="2" style="margin:0;width:116px">
           <script>for (const t of ['input','change']) $('#r').addEventListener(t, () => console.log(t + ':' + $('#r').value));</script></body>"#,
        "range",
    );
    let id = p.doc.borrow().find(|n| n.attr("id") == Some("r")).unwrap();
    let rect = p.view.borrow_mut().layout().node_rect(id).unwrap();
    let at = |f: f32| (rect.x + 8.0 + (rect.width - 16.0) * f, rect.y + rect.height / 2.0);
    // Press at 50%: the thumb jumps there.
    let (x, y) = at(0.5);
    p.pointer_moved(x, y);
    p.press();
    assert_eq!(p.range_value(id), 5.0);
    // Dragging follows (snapped to the step) and clamps at the ends.
    let (x, y) = at(0.87);
    p.pointer_moved(x, y);
    assert_eq!(p.range_value(id), 9.0);
    p.pointer_moved(rect.x + rect.width + 50.0, y);
    assert_eq!(p.range_value(id), 10.0);
    p.release();
    assert_eq!(log(&p), ["input:5", "input:9", "input:10", "change:10"]);
    // Keys on the focused slider.
    let none = ModifiersState::default();
    press_key(&mut p, "ArrowLeft", none);
    press_key(&mut p, "Home", none);
    assert_eq!(p.range_value(id), 0.0);
    assert_eq!(log(&p).last().map(String::as_str), Some("change:0"));
    let (fraction, _) = nile_style::control_fraction(&p.doc.borrow(), id);
    assert_eq!(fraction, Some(0.0));
    let _ = std::fs::remove_file(path);
}

const EDITOR: &str = r#"<body style="margin:0;font-family:sans-serif;font-size:16px"><div id="h" contenteditable="true" style="width:400px"><p id="p1" style="margin:0">Hello</p><p id="p2" style="margin:0">World</p></div></body>"#;

fn js(p: &mut Page, code: &str) -> String {
    p.eval(code).0.unwrap_or_else(|e| format!("ERR {e}"))
}

fn html(p: &mut Page) -> String {
    js(p, "document.getElementById('h').innerHTML")
}

fn type_keys(p: &mut Page, keys: &str) {
    for c in keys.chars() {
        let k = c.to_string();
        let (proceed, _) = p.key("keydown", k.clone(), ModifiersState::default());
        if proceed {
            p.default_key(&k, ModifiersState::default());
        }
    }
}

/// Click at the end of paragraph `id`'s text.
fn click_end_of(p: &mut Page, id: &str) {
    // (Detached nodes keep their ids, so look only at what is in the document.)
    let node = {
        let doc = p.doc.borrow();
        doc.descendants(Document::ROOT).into_iter().find(|n| doc.node(*n).attr("id") == Some(id)).unwrap()
    };
    let r = p.view.borrow_mut().layout().node_rect(node).unwrap();
    p.pointer_moved(r.x + r.width - 2.0, r.y + r.height / 2.0);
    p.press();
    p.release();
}

#[test]
fn contenteditable_takes_typing_enter_and_backspace() {
    let (mut p, path) = page(EDITOR, "edit-typing");
    click_end_of(&mut p, "p1");
    assert!(p.focused_host().is_some(), "the host has focus");
    let sel = p.edit_selection().expect("a caret");
    assert_eq!(sel.0, sel.1);
    type_keys(&mut p, "!?");
    assert_eq!(html(&mut p), r#"<p id="p1" style="margin:0">Hello!?</p><p id="p2" style="margin:0">World</p>"#);
    press_key(&mut p, "Backspace", ModifiersState::default());
    assert_eq!(js(&mut p, "document.getElementById('p1').textContent"), "Hello!");
    press_key(&mut p, "Enter", ModifiersState::default());
    type_keys(&mut p, "ab");
    assert_eq!(js(&mut p, "document.getElementById('h').children.map(c => c.textContent).join('|')"), "Hello!|ab|World");
    // Backspace at the start of a paragraph joins it with the one before.
    press_key(&mut p, "ArrowLeft", ModifiersState::default());
    press_key(&mut p, "ArrowLeft", ModifiersState::default());
    press_key(&mut p, "Backspace", ModifiersState::default());
    assert_eq!(js(&mut p, "document.getElementById('h').children.map(c => c.textContent).join('|')"), "Hello!ab|World");
    let _ = std::fs::remove_file(path);
}

/// Select `from..to` (character offsets) of paragraph `id`'s first text node.
fn select_in(p: &mut Page, id: &str, from: usize, to: usize) {
    let code = format!("var t = document.getElementById('{id}').firstChild; document.getElementById('h').focus(); getSelection().setBaseAndExtent(t, {from}, t, {to});");
    js(p, &code);
    // `focus()` is a script call; let the page notice.
    p.sync();
    js(p, &format!("var t = document.getElementById('{id}').firstChild; getSelection().setBaseAndExtent(t, {from}, t, {to});"));
}

fn inner(p: &mut Page, id: &str) -> String {
    js(p, &format!("document.getElementById('{id}').innerHTML"))
}

#[test]
fn execcommand_formats_ranges_and_reports_state() {
    let (mut p, path) = page(EDITOR, "edit-format");
    select_in(&mut p, "p1", 1, 4);
    assert_eq!(js(&mut p, "document.execCommand('bold')"), "true");
    assert_eq!(inner(&mut p, "p1"), "H<b>ell</b>o");
    assert_eq!(js(&mut p, "document.queryCommandState('bold')"), "true");
    assert_eq!(js(&mut p, "document.queryCommandState('italic')"), "false");
    // Part of the bold run turns it off again: "e" and "o" stay as they are.
    js(&mut p, "var b = document.querySelector('b').firstChild; getSelection().setBaseAndExtent(b, 1, b, 2)");
    js(&mut p, "document.execCommand('bold')");
    assert_eq!(inner(&mut p, "p1"), "H<b>e</b>l<b>l</b>o");
    // Across paragraphs: both get italics; colours and sizes use spans.
    js(&mut p, "var a = document.getElementById('p1'), b = document.getElementById('p2'); getSelection().setBaseAndExtent(a.lastChild, 0, b.firstChild, 3)");
    js(&mut p, "document.execCommand('italic'); document.execCommand('foreColor', false, '#ff0000')");
    assert_eq!(inner(&mut p, "p1"), r##"H<b>e</b>l<b>l</b><i><span style="color: #ff0000">o</span></i>"##);
    assert_eq!(inner(&mut p, "p2"), r##"<i><span style="color: #ff0000">Wor</span></i>ld"##);
    let _ = std::fs::remove_file(path);
}

#[test]
fn editing_undo_lists_blocks_pending_format_and_selection_deletes() {
    let (mut p, path) = page(EDITOR, "edit-more");
    // Typing after Bold with a caret types bold text.
    click_end_of(&mut p, "p1");
    js(&mut p, "document.execCommand('bold')");
    type_keys(&mut p, "XY");
    assert_eq!(inner(&mut p, "p1"), "Hello<b>XY</b>");
    js(&mut p, "document.execCommand('bold')");
    type_keys(&mut p, "z");
    assert_eq!(inner(&mut p, "p1"), "Hello<b>XY</b>z");
    // Undo steps back through the edits, redo returns.
    assert_eq!(js(&mut p, "document.execCommand('undo')"), "true");
    assert_eq!(js(&mut p, "document.getElementById('p1').textContent"), "HelloXY");
    js(&mut p, "document.execCommand('undo'); document.execCommand('undo')");
    assert_eq!(inner(&mut p, "p1"), "Hello");
    js(&mut p, "document.execCommand('redo')");
    assert_eq!(js(&mut p, "document.getElementById('p1').textContent"), "HelloXY");
    js(&mut p, "document.execCommand('undo')");
    // Blocks: alignment, headings, lists.
    select_in(&mut p, "p2", 1, 3);
    js(&mut p, "document.execCommand('justifyCenter'); document.execCommand('formatBlock', false, 'h2')");
    assert_eq!(js(&mut p, "document.getElementById('h').lastChild.outerHTML"), r#"<h2 style="margin: 0; text-align: center">World</h2>"#);
    js(&mut p, "document.execCommand('insertUnorderedList')");
    assert_eq!(js(&mut p, "document.getElementById('h').lastChild.tagName"), "UL");
    assert_eq!(js(&mut p, "document.queryCommandState('insertUnorderedList')"), "true");
    js(&mut p, "document.execCommand('insertUnorderedList')");
    assert_eq!(js(&mut p, "document.getElementById('h').lastChild.tagName"), "P");
    // Deleting a selection across paragraphs joins them.
    js(&mut p, "var a = document.getElementById('p1').firstChild, b = document.getElementById('h').lastChild.firstChild; getSelection().setBaseAndExtent(a, 3, b, 2)");
    js(&mut p, "document.execCommand('delete')");
    assert_eq!(js(&mut p, "document.getElementById('h').children.map(c => c.textContent).join('|')"), "Helrld");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_word_example_formats_the_document_from_its_ribbon() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/word-ribbon.html");
    let mut p = Page::load(file, (1200.0, 800.0), 0.0);
    p.layout(1200.0, 800.0);
    let q = |p: &mut Page, code: &str| js(p, code);
    // Select "clear purpose" in the Overview paragraph and use the ribbon like a person would.
    q(&mut p, "var t = document.getElementById('sample').firstChild; document.getElementById('paper').focus(); var i = t.data.indexOf('clear purpose'); getSelection().setBaseAndExtent(t, i, t, i + 13)");
    p.sync();
    q(&mut p, "var t = document.getElementById('sample').firstChild; var i = t.data.indexOf('clear purpose'); getSelection().setBaseAndExtent(t, i, t, i + 13)");
    click(&mut p, "bold");
    click(&mut p, "italic");
    let sample = q(&mut p, "document.getElementById('sample').innerHTML");
    assert!(sample.contains("<i><b>clear purpose</b></i>") || sample.contains("<b><i>clear purpose</i></b>"), "{sample}");
    assert_eq!(q(&mut p, "document.getElementById('bold').classList.contains('selected')"), "true", "the Bold button shows its state");
    // Undo from the title bar, then redo, then change the paragraph alignment.
    let undo = p.doc.borrow().find(|n| n.attr("data-action") == Some("undo")).unwrap();
    let r = p.view.borrow_mut().layout().node_rect(undo).unwrap();
    for _ in 0..2 {
        p.pointer_moved(r.x + 3.0, r.y + 3.0);
        p.press();
        p.release();
    }
    assert!(!q(&mut p, "document.getElementById('sample').innerHTML").contains("<b>"), "two undos remove both commands");
    let align = p.doc.borrow().find(|n| n.attr("data-align") == Some("center")).unwrap();
    let r = p.view.borrow_mut().layout().node_rect(align).unwrap();
    p.pointer_moved(r.x + 3.0, r.y + 3.0);
    p.press();
    p.release();
    assert!(q(&mut p, "document.getElementById('sample').style.textAlign") == "center");
    assert!(q(&mut p, "document.getElementById('word-count').textContent").ends_with("words"));
    // The ribbon tabs switch panels, and Insert > Table puts a table in the document.
    let click_node = |p: &mut Page, n: NodeId| {
        let r = p.view.borrow_mut().layout().node_rect(n).unwrap();
        p.pointer_moved(r.x + r.width / 2.0, r.y + r.height / 2.0);
        p.press();
        p.release();
    };
    let tab = p.doc.borrow().find(|n| n.attr("data-tab") == Some("Insert")).unwrap();
    click_node(&mut p, tab);
    assert_eq!(q(&mut p, "document.getElementById('home-ribbon').classList.contains('hidden')"), "true");
    assert_eq!(q(&mut p, "document.querySelectorAll('#other-ribbon [data-action]').length > 0"), "true");
    q(&mut p, "var t = document.getElementById('sample').firstChild; document.getElementById('paper').focus(); getSelection().setBaseAndExtent(t, 3, t, 3)");
    let table = p.doc.borrow().find(|n| n.attr("data-action") == Some("table")).unwrap();
    click_node(&mut p, table);
    assert_eq!(q(&mut p, "document.querySelectorAll('#paper td').length"), "9");
    let home = p.doc.borrow().find(|n| n.attr("data-tab") == Some("Home")).unwrap();
    click_node(&mut p, home);
    assert_eq!(q(&mut p, "document.getElementById('home-ribbon').classList.contains('hidden')"), "false");
}

#[test]
fn editing_with_the_mouse_clipboard_arrows_and_lists() {
    let (mut p, path) = page(
        r#"<body style="margin:0;font-family:sans-serif;font-size:16px"><div id="h" contenteditable="true" style="width:400px"><p id="p1" style="margin:0">Hello</p><p id="p2" style="margin:0">World</p><ul id="u"><li id="l1">one</li></ul></div></body>"#,
        "edit-mouse",
    );
    let rect = |p: &mut Page, id: &str| {
        let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        p.view.borrow_mut().layout().node_rect(n).unwrap()
    };
    // Drag from inside "Hello" to inside "World": the selection covers the line break.
    let _ = rect(&mut p, "p1");
    let caret_at = |p: &mut Page, id: &str, offset: usize| {
        let t = p.doc.borrow().find(|n| n.attr("id") == Some(id)).map(|n| p.doc.borrow().node(n).children[0]).unwrap();
        p.edit_caret_painted(DomPoint { node: t, offset }).unwrap()
    };
    let (a, b) = (caret_at(&mut p, "p1", 3), caret_at(&mut p, "p2", 3));
    p.pointer_moved(a.0 + 0.5, a.1 + a.2 / 2.0);
    p.press();
    p.pointer_moved(b.0 + 0.5, b.1 + b.2 / 2.0);
    p.release();
    assert_eq!(p.edit_selected_text(), "lo\nWor");
    let ctrl = ModifiersState::CONTROL;
    press_key(&mut p, "c", ctrl);
    assert_eq!(p.clipboard.text().as_deref(), Some("lo\nWor"));
    // Typing replaces the selection; paste puts the text (and the line break) back.
    type_keys(&mut p, "X");
    assert_eq!(js(&mut p, "document.getElementById('h').children.map(c => c.textContent).join('|')"), "HelXld|one");
    press_key(&mut p, "v", ctrl);
    assert_eq!(js(&mut p, "document.getElementById('h').children.map(c => c.textContent).join('|')"), "HelXlo|World|one");
    // Select all, then cut: nothing but an empty line remains.
    press_key(&mut p, "a", ctrl);
    press_key(&mut p, "x", ctrl);
    assert_eq!(p.clipboard.text().as_deref(), Some("HelXlo\nWorld\none"));
    assert_eq!(js(&mut p, "document.getElementById('h').textContent"), "");
    // Undo brings it all back.
    press_key(&mut p, "z", ctrl);
    assert_eq!(js(&mut p, "document.getElementById('h').children.length"), "3");
    // Lists: Enter makes a new item, Enter on the empty item leaves the list.
    click_end_of(&mut p, "l1");
    press_key(&mut p, "Enter", ModifiersState::default());
    type_keys(&mut p, "two");
    assert_eq!(js(&mut p, "document.getElementById('u').children.map(c => c.textContent).join('|')"), "one|two");
    press_key(&mut p, "Enter", ModifiersState::default());
    press_key(&mut p, "Enter", ModifiersState::default());
    assert_eq!(js(&mut p, "document.getElementById('h').lastChild.tagName"), "P");
    assert_eq!(js(&mut p, "document.getElementById('u').children.length"), "2");
    let _ = std::fs::remove_file(path);
}

#[test]
fn transitions_and_animations_fire_their_events() {
    let (mut p, path) = page(
        r#"<style>
            #t { width: 10px; transition: width 0.05s linear }
            #t.on { width: 50px }
            #a { animation: nudge 0.05s linear }
            @keyframes nudge { from { margin-left: 0 } to { margin-left: 5px } }
        </style>
        <div id=t></div><div id=a></div>
        <script>
          window.log = [];
          for (const id of ["t", "a"]) {
            const el = document.getElementById(id);
            for (const k of ["transitionend", "animationstart", "animationend"])
              el.addEventListener(k, e => log.push(k + ":" + (e.propertyName || e.animationName) + ":" + e.target.id));
          }
          document.getElementById("t").classList.add("on");
        </script>"#,
        "anim_events",
    );
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        p.tick();
    }
    let log = js(&mut p, "log.join(',')");
    assert!(log.contains("animationstart:nudge:a"), "{log}");
    assert!(log.contains("animationend:nudge:a"), "{log}");
    assert!(log.contains("transitionend:width:t"), "{log}");
    assert_eq!(log.matches("end").count(), 2, "each fires once: {log}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn fixed_boxes_stay_put_and_stay_clickable_while_scrolled() {
    let (mut p, path) = page(
        r#"<style>
            body { margin: 0 }
            .row { height: 100px }
            #f { position: fixed; right: 10px; bottom: 10px; width: 40px; height: 30px }
            #s { position: sticky; top: 0; height: 20px }
        </style>
        <div class=row></div><div id=s></div>
        <div class=row></div><div class=row></div><div class=row></div><div class=row></div><div class=row></div>
        <div id=f onclick="window.hit = 1"></div>"#,
        "fixed_click",
    );
    let at = |p: &mut Page, id: &str| {
        let node = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        let (sc, offs) = {
            let v = p.view.borrow();
            (v.scroll_y, v.scroll_offsets.clone())
        };
        let r = p.view.borrow_mut().layout().node_rect_scrolled(node, &offs).unwrap();
        (r.x + r.width / 2.0, r.y - sc + r.height / 2.0)
    };
    let before = at(&mut p, "f");
    p.wheel(0.0, -250.0);
    assert!(p.view.borrow().scroll_y > 0.0);
    // Same place on screen after scrolling; the sticky bar has caught up with the top.
    assert_eq!(at(&mut p, "f"), before);
    let (_, sticky_y) = at(&mut p, "s");
    assert!((sticky_y - 10.0).abs() < 1.0, "{sticky_y}");
    let (x, y) = before;
    p.pointer_moved(x, y);
    p.press();
    p.release();
    assert_eq!(js(&mut p, "String(window.hit)"), "1");
    let _ = std::fs::remove_file(path);
}

#[test]
fn number_inputs_step_clamp_filter_and_spin() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><input id="q" type="number" value="9" min="0" max="10" style="margin:0;width:100px">
           <input id="d" type="number" value="0.1" step="0.1" style="margin:0;width:100px"></body>"#,
        "number",
    );
    let none = ModifiersState::default();
    let val = |p: &mut Page, id: &str| js(p, &format!("document.getElementById('{id}').value"));
    click(&mut p, "q");
    press_key(&mut p, "ArrowUp", none);
    press_key(&mut p, "ArrowUp", none);
    assert_eq!(val(&mut p, "q"), "10"); // clamped to max
    press_key(&mut p, "ArrowDown", none);
    assert_eq!(val(&mut p, "q"), "9");
    // Letters are dropped, digits and signs kept.
    press_key(&mut p, "End", none);
    type_keys(&mut p, "x5");
    assert_eq!(val(&mut p, "q"), "95");
    click(&mut p, "d");
    press_key(&mut p, "ArrowUp", none);
    press_key(&mut p, "ArrowUp", none);
    assert_eq!(val(&mut p, "d"), "0.3"); // no 0.30000000000000004
    // The spin arrows at the right edge: upper half up, lower half down.
    let node = p.doc.borrow().find(|n| n.attr("id") == Some("d")).unwrap();
    let r = p.view.borrow_mut().layout().node_rect(node).unwrap();
    p.pointer_moved(r.x + r.width - 8.0, r.y + 3.0);
    p.press();
    p.release();
    assert_eq!(val(&mut p, "d"), "0.4");
    p.pointer_moved(r.x + r.width - 8.0, r.y + r.height - 3.0);
    p.press();
    p.release();
    assert_eq!(val(&mut p, "d"), "0.3");
    // Over the spin arrows the pointer is an arrow, over the text an I-beam.
    p.pointer_moved(r.x + r.width - 8.0, r.y + 3.0);
    assert_eq!(p.cursor(), nile_style::Cursor::Default);
    p.pointer_moved(r.x + 10.0, r.y + 5.0);
    assert_eq!(p.cursor(), nile_style::Cursor::Text);
    // The wheel over the focused input steps it (up increases) instead of scrolling the page.
    p.pointer_moved(r.x + 10.0, r.y + 5.0);
    p.wheel(0.0, 10.0);
    assert_eq!(val(&mut p, "d"), "0.4");
    p.wheel(0.0, -10.0);
    p.wheel(0.0, -10.0);
    assert_eq!(val(&mut p, "d"), "0.2");
    let _ = std::fs::remove_file(path);
}

#[cfg(feature = "net")]
#[test]
fn fetch_reaches_a_local_http_server() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            // Read the whole request (headers, then the two body bytes) before answering.
            let mut req = String::new();
            let mut buf = [0u8; 2048];
            while !req.contains("\r\n\r\nhi") {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.push_str(&String::from_utf8_lossy(&buf[..n])),
                }
            }
            let body = format!("{{\"method\":\"{}\"}}", req.split(' ').next().unwrap_or(""));
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        }
    });
    let (mut p, path) = page(
        &format!("<p id=o></p><script>fetch('http://127.0.0.1:{port}/x', {{ method: 'POST', body: 'hi' }}).then(r => r.json()).then(j => {{ document.getElementById('o').textContent = j.method; }})</script>"),
        "fetch_http",
    );
    for _ in 0..1000 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        p.tick();
        if js(&mut p, "document.getElementById('o').textContent") == "POST" {
            break;
        }
    }
    assert_eq!(js(&mut p, "document.getElementById('o').textContent"), "POST");
    let _ = std::fs::remove_file(path);
}

#[test]
fn scripts_read_and_write_the_clipboard() {
    let (mut p, path) = page(
        r#"<body><script>
             window.out = [];
             out.push(Clipboard.has("text"), Clipboard.readText() === undefined);
             Clipboard.writeText("plain");
             out.push(Clipboard.readText(), Clipboard.has("html"));
             Clipboard.write({ html: "<b>x</b>", text: "x" });
             const r = Clipboard.read();
             out.push(r.html, r.text);
             const px = new ImageData(new Uint8ClampedArray([255, 0, 0, 255, 0, 255, 0, 255]), 2, 1);
             out.push(Clipboard.write({ image: px }), Clipboard.has("image"), Clipboard.has("text"));
             const im = Clipboard.read().image;
             out.push(im.width, im.height, Array.from(im.data).join(" "));
             Clipboard.write({ json: { a: [1, 2] }, text: "tag" });
             out.push(JSON.stringify(Clipboard.read().json), Clipboard.readText());
             Clipboard.writeText("other");
             out.push(Clipboard.has("json"));
             Clipboard.write({ file: ["/a/b.txt", "/c.png"] });
             out.push(Clipboard.read().file.join("|"));
           </script></body>"#,
        "clipboardjs",
    );
    p.sync();
    assert_eq!(
        js(&mut p, "out.join(';')"),
        "false;true;plain;false;<b>x</b>;x;true;true;false;2;1;255 0 0 255 0 255 0 255;{\"a\":[1,2]};tag;false;/a/b.txt|/c.png"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn tray_and_badge_requests_reach_the_shell() {
    let (mut p, path) = page(
        r#"<body><menu id="t"><li id="a">One</li><li id="b" disabled>Two</li></menu><script>
             Window.this.tray = { icon: "none.png", tooltip: "Tip", menu: document.getElementById("t"), template: true };
             application.badge = 4;
           </script></body>"#,
        "tray",
    );
    p.sync();
    let info = p.tray().expect("tray");
    assert_eq!((info.tooltip.as_str(), info.template, info.items.len()), ("Tip", true, 2));
    assert_eq!(p.badge().as_deref(), Some("4"));
    assert_eq!(p.progress(), nile_script::Progress::None);
    js(&mut p, "application.progress = 0.25; 0");
    assert_eq!(p.progress(), nile_script::Progress::Value(0.25));
    js(&mut p, "application.progress = 7; 0");
    assert_eq!(p.progress(), nile_script::Progress::Value(1.0));
    js(&mut p, "application.progress = 'indeterminate'; 0");
    assert_eq!(p.progress(), nile_script::Progress::Indeterminate);
    js(&mut p, "application.progress = null; 0");
    assert_eq!(p.progress(), nile_script::Progress::None);
    js(&mut p, "application.badge = null; Window.this.tray = null; 0");
    assert!(p.tray().is_none() && p.badge().is_none());
    let _ = std::fs::remove_file(path);
}

/// A built-in module is a wrapper that re-exports names from an object the prelude defines; a name added to the object
/// but not to the wrapper is invisible to `import`. Compare them.
#[test]
fn built_in_modules_export_everything_their_objects_define() {
    let (mut p, path) = page(
        r#"<body><script type="module">
          import * as env from "@env";
          import * as sys from "@sys";
          import * as nile from "@nile";
          import * as debug from "@debug";
          import * as storage from "@storage";
          const missing = [];
          const check = (label, ns, obj, skip = []) => {
            for (const key of Object.keys(obj)) if (!(key in ns) && !skip.includes(key)) missing.push(label + "." + key);
          };
          check("env", env, globalThis.__envModule);
          check("sys", sys, globalThis.__sysModule);
          check("nile", nile, globalThis.__sciterModule, ["import"]);
          check("debug", debug, globalThis.__debugModule);
          check("storage", storage, globalThis.__storageModule);
          window.missing = missing.join(",");
        </script></body>"#,
        "module-exports",
    );
    p.tick();
    assert_eq!(js(&mut p, "window.missing"), "");
    let _ = std::fs::remove_file(path);
}

#[test]
fn window_screens_describe_the_monitors() {
    let (mut p, path) = page("<body></body>", "screens");
    p.view.borrow_mut().screens = (
        vec![
            nile_script::ScreenInfo { name: "A".into(), x: 0, y: 0, width: 2880, height: 1800, scale: 2.0, primary: true },
            nile_script::ScreenInfo { name: "B".into(), x: 2880, y: 0, width: 1920, height: 1080, scale: 1.0, primary: false },
        ],
        1,
    );
    let out = js(
        &mut p,
        "[Window.screens, Window.this.screen, Window.screenBox(0, 'frame').join(), Window.screenBox(0, 'frame', 'dimension', false).join(), Window.screenBox(1, 'frame', 'right'), Window.this.screenBox('device'), Window.screenBox(0, 'isPrimary'), Window.screenBox(0, 'devicePixelRatio')].join('|')",
    );
    assert_eq!(out, "2|1|0,0,2880,1800|1440,900|4800|B|true|2");
    js(&mut p, "window.seen = 0; addEventListener('resolutionchange', () => seen++); 0");
    p.screen_changed();
    assert_eq!(js(&mut p, "window.seen"), "1");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_machine_state_queries_answer_off_the_window_thread() {
    let (mut p, path) = page(
        r#"<body><script type="module">
          import * as env from "@env";
          window.out = "";
          window.seen = [];
          addEventListener("online", () => seen.push("online"));
          Promise.all([env.online(), env.power(), env.idleSeconds().catch((e) => "none")]).then(([o, pw, idle]) => {
            window.out = typeof o + ":" + typeof pw.battery + ":" + typeof pw.charging + ":" + (idle === "none" || typeof idle === "number");
          });
        </script></body>"#,
        "machine-state",
    );
    for _ in 0..600 {
        p.tick();
        if !js(&mut p, "window.out").is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(js(&mut p, "window.out"), "boolean:boolean:boolean:true");
    let _ = std::fs::remove_file(path);
}

#[test]
fn global_hotkeys_are_registered_by_the_page_and_fire_its_handler() {
    let (mut p, path) = page(
        r#"<body><script>
             window.log = [];
             window.k1 = application.registerHotkey("Ctrl+Shift+K", () => log.push("k1"));
             window.k2 = application.registerHotkey("Alt+F9", () => log.push("k2"));
             k1.ready.then((ok) => log.push("ready:" + ok));
             k2.ready.then((ok) => log.push("ready2:" + ok));
           </script></body>"#,
        "hotkeys",
    );
    p.sync();
    assert_eq!(p.hotkeys(), vec![(1, "Ctrl+Shift+K".to_string()), (2, "Alt+F9".to_string())]);
    p.hotkey_event(1, "ok");
    p.hotkey_event(2, "refused");
    p.hotkey_event(1, "pressed");
    p.hotkey_event(2, "pressed");
    js(&mut p, "k2.close(); 0");
    assert_eq!(p.hotkeys(), vec![(1, "Ctrl+Shift+K".to_string())]);
    p.hotkey_event(2, "pressed");
    p.run_timers();
    assert_eq!(js(&mut p, "log.join(',')"), "ready:true,ready2:false,k1,k2");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_in_window_menu_bar_runs_items_like_a_native_one() {
    let (mut p, path) = page(
        r#"<body><menu id="m" style="display:none"><li>File <menu><li id="a">Alpha</li><li id="b" disabled>Beta</li><hr><li id="q" role="quit">Quit</li></menu></li><li>Help</li></menu>
           <script>window.log = []; document.getElementById("a").addEventListener("click", () => log.push("a")); document.getElementById("b").addEventListener("click", () => log.push("b"));</script></body>"#,
        "menubar",
    );
    p.set_native_menu(false);
    js(&mut p, "Window.this.menu = document.getElementById('m'); 0");
    p.sync();
    assert_eq!(js(&mut p, "document.querySelectorAll('.nile-menubar-item').length"), "2");
    js(&mut p, "document.querySelector('.nile-menubar-item').click(); 0");
    assert_eq!(js(&mut p, "[...document.querySelectorAll('.nile-menu-row')].map(r => r.textContent).join('|')"), "Alpha|Beta|Quit");
    // A disabled row does nothing; an enabled one closes the menu and fires `click` on its <li>.
    js(&mut p, "document.querySelectorAll('.nile-menu-row')[1].click(); 0");
    p.sync();
    assert_eq!(js(&mut p, "log.join()"), "");
    js(&mut p, "document.querySelectorAll('.nile-menu-row')[0].click(); 0");
    p.sync();
    assert_eq!(js(&mut p, "log.join() + document.querySelectorAll('.nile-menu').length"), "a0");
    // A window role comes back as a window command.
    js(&mut p, "document.querySelector('.nile-menubar-item').click(); document.querySelectorAll('.nile-menu-row')[2].click(); 0");
    p.sync();
    assert!(p.view.borrow().window_requests.iter().any(|c| matches!(c, WindowCommand::Quit(0))));
    let _ = std::fs::remove_file(path);
}

#[test]
fn observers_and_location_follow_the_page() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="top" style="height:100px"></div><div style="height:600px"></div><div id="far" style="height:50px"></div>
           <script>
             window.log = [];
             new ResizeObserver(es => es.forEach(e => log.push('ro ' + e.target.id + ' ' + e.contentRect.height))).observe(document.getElementById('top'));
             new IntersectionObserver(es => es.forEach(e => log.push('io ' + e.target.id + ' ' + e.isIntersecting))).observe(document.getElementById('far'));
           </script>"#,
        "observers",
    );
    let log = |p: &mut Page| js(p, "log.join(',')");
    p.sync();
    assert_eq!(log(&mut p), "ro top 100,io far false");
    // A size change reaches the resize observer; scrolling reveals the far box.
    js(&mut p, "document.getElementById('top').style.height = '120px'; 0");
    p.sync();
    p.wheel(0.0, -700.0);
    p.sync();
    assert_eq!(log(&mut p), "ro top 100,io far false,ro top 120,io far true");
    // location.
    assert!(js(&mut p, "location.href").starts_with("file://"));
    assert_eq!(js(&mut p, "location.pathname.endsWith('observers.html') + ',' + devicePixelRatio"), "true,1");
    // Nothing navigates: `location` is read-only and there is no `history`.
    js(&mut p, "location.hash = 'two'; location.href = 'x.html'; 0");
    assert_eq!(js(&mut p, "location.hash + typeof history"), "undefined");
    let _ = std::fs::remove_file(path);
}

#[test]
fn color_date_and_time_inputs_filter_step_and_normalize() {
    let (mut p, path) = page(
        r##"<body style="margin:0"><input id="c" type="color" style="margin:0"><input id="d" type="date" value="2024-01-31" style="margin:0">
           <input id="t" type="time" value="23:59" style="margin:0"></body>"##,
        "temporal",
    );
    let none = ModifiersState::default();
    let val = |p: &mut Page, id: &str| js(p, &format!("document.getElementById('{id}').value"));
    assert_eq!(val(&mut p, "c"), "#000000");
    click(&mut p, "c");
    press_key(&mut p, "a", ModifiersState::CONTROL);
    type_keys(&mut p, "#AbCz");
    press_key(&mut p, "Enter", none);
    assert_eq!(val(&mut p, "c"), "#aabbcc");
    press_key(&mut p, "a", ModifiersState::CONTROL);
    type_keys(&mut p, "#12");
    press_key(&mut p, "Tab", none); // invalid: back to the old colour
    assert_eq!(val(&mut p, "c"), "#aabbcc");
    // Dates step the field under the caret.
    click(&mut p, "d");
    press_key(&mut p, "Home", none);
    for _ in 0..5 {
        press_key(&mut p, "ArrowRight", none);
    }
    press_key(&mut p, "ArrowUp", none);
    assert_eq!(val(&mut p, "d"), "2024-02-29"); // month 1 -> 2, the day clamps
    type_keys(&mut p, "x");
    assert_eq!(val(&mut p, "d"), "2024-02-29");
    click(&mut p, "t");
    press_key(&mut p, "Home", none);
    press_key(&mut p, "ArrowUp", none);
    assert_eq!(val(&mut p, "t"), "00:59"); // hours wrap
    let _ = std::fs::remove_file(path);
}

#[test]
fn file_inputs_ask_for_files_and_expose_them() {
    let dir = std::env::temp_dir().join(format!("nile-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("note.txt");
    std::fs::write(&file, "hello").unwrap();
    let (mut p, path) = page(r#"<body style="margin:0"><input id="f" type="file" accept=".txt" multiple>"#, "file_input");
    click(&mut p, "f");
    let request = p.take_file_request().expect("a click asks for a chooser");
    assert!(request.multiple && request.accept == ".txt");
    assert!(p.take_file_request().is_none());
    let named = file.to_string_lossy().to_string();
    assert!(p.choose_files_for_request(std::slice::from_ref(&named)));
    assert_eq!(js(&mut p, "document.getElementById('f').files.length + ' ' + document.getElementById('f').files[0].name + ' ' + document.getElementById('f').files[0].size"), "1 note.txt 5");
    assert_eq!(js(&mut p, "document.getElementById('f').value"), named);
    js(&mut p, "window.t = ''; document.getElementById('f').files[0].text().then(s => { window.t = s; }); 0");
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        p.tick();
        if js(&mut p, "window.t") == "hello" {
            break;
        }
    }
    assert_eq!(js(&mut p, "window.t"), "hello");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_file(path);
}

#[test]
fn optgroups_are_headings_and_list_boxes_select_by_dragging() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><select id="m" multiple size="7" style="margin:0;width:160px">
             <optgroup label="G1"><option>a</option><option>b</option><option>c</option></optgroup>
             <optgroup label="G2"><option>d</option><option>e</option></optgroup>
           </select><select id="s" style="margin:0"><optgroup label="G"><option>x</option><option>y</option></optgroup></select></body>"#,
        "optgroup",
    );
    let picked = |p: &mut Page| js(p, "Array.from(document.getElementById('m').querySelectorAll('option')).filter(o => o.selected).map(o => o.text).join('')");
    // Rows: G1 a b c G2 d e. Press on "a" (row 1), drag down through G2's heading to "d" (row 5).
    let node = p.doc.borrow().find(|n| n.attr("id") == Some("m")).unwrap();
    let r = p.view.borrow_mut().layout().node_rect(node).unwrap();
    let (rh, inset) = {
        let mut v = p.view.borrow_mut();
        let l = v.layout().list_box(node).unwrap();
        (l.row_height, l.inset_top)
    };
    let row_y = |i: usize| r.y + inset + rh * (i as f32 + 0.5);
    p.pointer_moved(r.x + 20.0, row_y(0)); // the heading: not a choice
    p.press();
    p.release();
    assert_eq!(picked(&mut p), "");
    p.pointer_moved(r.x + 20.0, row_y(1));
    p.press();
    p.pointer_moved(r.x + 20.0, row_y(3));
    assert_eq!(picked(&mut p), "abc");
    p.pointer_moved(r.x + 20.0, row_y(5));
    assert_eq!(picked(&mut p), "abcd"); // G2's heading is skipped
    p.release();
    assert_eq!(picked(&mut p), "abcd");
    // The drop-down: the heading takes a row but can't be chosen, and arrows skip it.
    let sel = p.doc.borrow().find(|n| n.attr("id") == Some("s")).unwrap();
    assert_eq!(nile_layout::select_options(&p.doc.borrow(), sel).len(), 3);
    assert_eq!(nile_layout::selected_index(&p.doc.borrow(), sel), Some(1));
    let _ = std::fs::remove_file(path);
}

#[test]
fn input_methods_compose_in_inputs_and_editors() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><input id="i" value="ab" style="margin:0;width:200px">
           <div id="h" contenteditable style="margin:0;width:200px"><p id="para">xy</p></div></body>"#,
        "ime",
    );
    let none = ModifiersState::default();
    assert!(!p.ime_wanted());
    click(&mut p, "i");
    assert!(p.ime_wanted());
    press_key(&mut p, "Home", none);
    press_key(&mut p, "ArrowRight", none);
    // Composing shows in the field but isn't the value yet.
    assert!(p.ime_preedit("ni", Some((0, 2))));
    assert_eq!(js(&mut p, "document.getElementById('i').value"), "ab");
    let shown = p.view.borrow_mut().layout().boxes.iter().find_map(|b| match &b.content {
        nile_layout::BoxContent::Text(c) if c.control => Some(c.text.clone()),
        _ => None,
    });
    assert_eq!(shown.as_deref(), Some("anib"));
    let (x0, ..) = p.ime_cursor_rect().expect("a caret to put the candidate window at");
    assert!(p.ime_underline().is_some());
    assert!(p.ime_preedit("nin", Some((0, 3))));
    assert!(p.ime_cursor_rect().unwrap().0 > x0, "the caret follows the composition");
    // Commit replaces the composition with the final text, and undo takes it back as one edit.
    assert!(p.ime_commit("你好"));
    assert_eq!(js(&mut p, "document.getElementById('i').value"), "a你好b");
    assert!(p.ime_underline().is_none());
    press_key(&mut p, "z", ModifiersState::CONTROL);
    assert_eq!(js(&mut p, "document.getElementById('i').value"), "ab");
    // Losing focus abandons a composition.
    click(&mut p, "i");
    p.ime_preedit("x", None);
    click(&mut p, "h");
    assert!(p.ime_underline().is_none());
    assert_eq!(js(&mut p, "document.getElementById('i').value"), "ab");

    // In an editing host the composition is an underlined span that commit replaces.
    click(&mut p, "para");
    press_key(&mut p, "End", none);
    assert!(p.ime_preedit("ka", None));
    assert_eq!(js(&mut p, "document.getElementById('para').innerHTML"), r#"xy<span data-nile-ime="" style="text-decoration: underline">ka</span>"#);
    assert!(p.ime_commit("か"));
    assert_eq!(js(&mut p, "document.getElementById('para').textContent"), "xyか");
    assert_eq!(js(&mut p, "document.getElementById('para').querySelectorAll('span').length"), "0");
    p.ime_preedit("zz", None);
    p.ime_cancel();
    assert_eq!(js(&mut p, "document.getElementById('para').textContent"), "xyか");
    let _ = std::fs::remove_file(path);
}

#[test]
fn rich_copy_paste_and_range_extras() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="h" contenteditable><p id="a">one <b>two</b> three</p><p id="b">tail</p></div></body>"#,
        "rich_paste",
    );
    let html = |p: &mut Page| js(p, "document.getElementById('h').innerHTML");
    let ctrl = ModifiersState::CONTROL;
    // Range extras on the unedited document.
    assert_eq!(
        js(&mut p, "(() => { const t = document.getElementById('a'); const r = document.createRange(); r.setStart(t.firstChild, 2); r.setEnd(t.querySelector('b').firstChild, 2); return r.cloneContents().innerHTML + '|' + t.innerHTML; })()"),
        "e <b>tw</b>|one <b>two</b> three"
    );
    assert_eq!(
        js(&mut p, "(() => { const t = document.getElementById('a'); const r = document.createRange(), s = document.createRange(); r.setStart(t.firstChild, 1); r.setEnd(t.firstChild, 3); s.setStart(t.firstChild, 2); s.setEnd(t.firstChild, 4); return [r.compareBoundaryPoints(Range.START_TO_START, s), r.compareBoundaryPoints(Range.END_TO_END, s), r.comparePoint(t.firstChild, 2), r.comparePoint(t.firstChild, 9), r.isPointInRange(t.firstChild, 0)].join(); })()"),
        "-1,-1,0,1,false"
    );
    // Copy "two three" (spanning the bold run) and paste it at the start of the second paragraph.
    click_end_of(&mut p, "a");
    for _ in 0..9 {
        press_key(&mut p, "ArrowLeft", ModifiersState::SHIFT);
    }
    press_key(&mut p, "c", ctrl);
    let copied = p.clipboard.html().unwrap_or_default();
    assert!(copied.contains("<b>two</b>"), "{copied}");
    assert_eq!(p.clipboard.text().as_deref(), Some("two three"));
    click_end_of(&mut p, "b");
    press_key(&mut p, "v", ctrl);
    assert_eq!(js(&mut p, "document.getElementById('b').innerHTML"), "tail<b>two</b> three");
    // HTML from elsewhere: blocks stay blocks, scripts and unknown wrappers go, styles are filtered.
    p.clipboard.set_html(
        r#"<meta charset="utf-8"><div style="color: red; position: fixed"><strong>Hello</strong></div><script>evil()</script><div><section>World <em>wide</em> <a href="javascript:x()">link</a></section></div>"#,
        "Hello World wide link",
    );
    click_end_of(&mut p, "a");
    press_key(&mut p, "v", ctrl);
    let out = html(&mut p);
    assert!(out.contains(r#"<p style="color: red"><b>Hello</b></p>"#) || out.contains("<b>Hello</b>"), "{out}");
    assert!(!out.contains("script") && !out.contains("evil") && !out.contains("javascript") && !out.contains("fixed"), "{out}");
    assert!(out.contains("World <i>wide</i>"), "{out}");
    // Undo takes the whole paste back.
    press_key(&mut p, "z", ctrl);
    assert_eq!(js(&mut p, "document.getElementById('a').textContent"), "one two three");
    let _ = std::fs::remove_file(path);
}

#[test]
fn tables_in_the_editor_insert_navigate_and_keep_their_shape() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="h" contenteditable><p id="a">before</p></div><input id="after"></body>"#,
        "editor_table",
    );
    let none = ModifiersState::default();
    click_end_of(&mut p, "a");
    js(&mut p, "document.execCommand('insertTable', false, '2x3'); 0");
    assert_eq!(js(&mut p, "document.querySelectorAll('#h table tr').length + 'x' + document.querySelectorAll('#h td').length"), "2x6");
    assert_eq!(js(&mut p, "document.getElementById('h').lastElementChild.tagName"), "P", "a paragraph follows the table");
    type_keys(&mut p, "A");
    press_key(&mut p, "Tab", none);
    type_keys(&mut p, "B");
    press_key(&mut p, "Tab", none);
    press_key(&mut p, "Tab", none);
    type_keys(&mut p, "D");
    assert_eq!(js(&mut p, "Array.from(document.querySelectorAll('#h td')).map(c => c.textContent).join('|')"), "A|B||D||");
    // Enter stays inside the cell; Backspace at a cell's start doesn't eat the previous cell.
    press_key(&mut p, "Enter", none);
    assert_eq!(js(&mut p, "document.querySelectorAll('#h td').length"), "6");
    press_key(&mut p, "Backspace", none);
    press_key(&mut p, "Backspace", none);
    press_key(&mut p, "Backspace", none);
    assert_eq!(js(&mut p, "document.querySelectorAll('#h td').length + ':' + document.querySelectorAll('#h td')[3].textContent"), "6:");
    // Shift+Tab goes back; Tab from the last cell adds a row.
    press_key(&mut p, "Tab", ModifiersState::SHIFT);
    type_keys(&mut p, "C");
    assert_eq!(js(&mut p, "document.querySelectorAll('#h td')[2].textContent"), "C");
    for _ in 0..4 {
        press_key(&mut p, "Tab", none);
    }
    assert_eq!(js(&mut p, "document.querySelectorAll('#h tr').length"), "3");
    // Select across cells and delete: contents go, cells stay.
    js(&mut p, "const tds = document.querySelectorAll('#h td'); const r = document.createRange(); r.setStart(tds[0], 0); r.setEnd(tds[4], 0); const s = getSelection(); s.removeAllRanges(); s.addRange(r); 0");
    press_key(&mut p, "Backspace", none);
    assert_eq!(js(&mut p, "document.querySelectorAll('#h td').length + ':' + Array.from(document.querySelectorAll('#h td')).map(c => c.textContent).join('')"), "9:");
    // Selecting everything and deleting takes the whole table with it.
    press_key(&mut p, "a", ModifiersState::CONTROL);
    press_key(&mut p, "Backspace", none);
    assert_eq!(js(&mut p, "document.querySelectorAll('#h table').length"), "0");
    let _ = std::fs::remove_file(path);
}

/// `cargo test --release -p nile-engine bench_hover -- --ignored --nocapture`: pointer moves over the 1,500-row page.
#[test]
#[ignore]
fn bench_hover() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/stress.html");
    let mut p = Page::load(file, (900.0, 640.0), 0.0);
    p.layout(900.0, 640.0);
    p.pointer_moved(100.0, 100.0);
    let t = Instant::now();
    let n = 100;
    for i in 0..n {
        p.pointer_moved(100.0, 60.0 + (i % 20) as f32 * 14.0);
        p.sync();
        p.view.borrow_mut().layout();
    }
    eprintln!("hover move: {:.3} ms each", t.elapsed().as_secs_f64() * 1e3 / n as f64);
}

#[test]
#[ignore]
fn bench_splitter_drag() {
    let file = std::env::var("NILE_BENCH_FILE").map(PathBuf::from).unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/splitter.html"));
    let (w, h) = (1000u32, 760u32);
    let mut p = Page::load(file, (w as f32, h as f32), 0.0);
    p.layout(w as f32, h as f32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for _ in 0..3 {
        p.run_timers();
        p.paint(&mut px, w, h, 1.0);
    }
    let v = p.eval("(() => { const r = document.querySelector('.nile-splitter-grip').getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2].join(); })()").0.unwrap();
    let mut it = v.split(',').map(|n| n.parse::<f32>().unwrap());
    let (x, y) = (it.next().unwrap(), it.next().unwrap());
    p.pointer_moved(x, y);
    p.press();
    let n = 80;
    let (mut total, mut worst) = (0.0f64, 0.0f64);
    let mut frames = Vec::new();
    for i in 1..=n {
        let t = Instant::now();
        p.pointer_moved(x + (i as f32) * 3.0, y);
        p.run_timers();
        p.sync();
        p.paint(&mut px, w, h, 1.0);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        total += ms;
        worst = worst.max(ms);
        frames.push(ms);
    }
    p.release();
    eprintln!("splitter drag (3px per move): {:.2} ms/frame avg, {:.1} ms worst", total / n as f64, worst);
    eprintln!("  frames: {}", frames.iter().map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" "));
}

#[test]
#[ignore]
fn bench_grid_resize() {
    let file = std::env::var("NILE_BENCH_FILE").map(PathBuf::from).unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data-grid.html"));
    let (w, h) = (1000u32, 720u32);
    let mut p = Page::load(file, (w as f32, h as f32), 0.0);
    p.layout(w as f32, h as f32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for _ in 0..3 {
        p.run_timers();
        p.paint(&mut px, w, h, 1.0);
    }
    // The last grid on the page (the big catalogue): its second column's drag handle.
    let v = p.eval("(() => { const g = Array.from(document.querySelectorAll('nile-grid')).pop(); const r = g.querySelectorAll('.nile-grid-resizer')[1].getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2].join(); })()").0.unwrap();
    let mut it = v.split(',').map(|n| n.parse::<f32>().unwrap());
    let (x, y) = (it.next().unwrap(), it.next().unwrap());
    p.pointer_moved(x, y);
    p.press();
    let n = 80;
    let (mut total, mut worst) = (0.0f64, 0.0f64);
    let mut frames = Vec::new();
    for i in 1..=n {
        let t = Instant::now();
        p.pointer_moved(x + (i as f32) * 2.0, y);
        std::thread::sleep(Duration::from_millis(2));
        p.run_timers();
        p.sync();
        p.paint(&mut px, w, h, 1.0);
        let ms = t.elapsed().as_secs_f64() * 1e3 - 2.0;
        total += ms;
        worst = worst.max(ms);
        frames.push(ms);
    }
    p.release();
    eprintln!("grid column resize (2px per move): {:.2} ms/frame avg, {:.1} ms worst", total / n as f64, worst);
    eprintln!("  frames: {}", frames.iter().map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" "));
}

#[test]
#[ignore]
fn bench_grid_scroll() {
    let file = std::env::var("NILE_BENCH_FILE").map(PathBuf::from).unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data-grid.html"));
    let (w, h) = (1000u32, 720u32);
    let mut p = Page::load(file, (w as f32, h as f32), 0.0);
    p.layout(w as f32, h as f32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for _ in 0..3 {
        p.run_timers();
        p.paint(&mut px, w, h, 1.0);
    }
    p.pointer_moved(700.0, 300.0);
    for step in [18.0f32, 60.0, 120.0] {
        let mut rows = Vec::new();
        let (mut js_ms, mut sync_ms, mut paint_ms, mut worst) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let n = 40;
        for _ in 0..n {
            let t = Instant::now();
            p.wheel(0.0, -step);
            let t_js = Instant::now();
            std::thread::sleep(Duration::from_millis(2));
            p.run_timers(); // the grid's requestAnimationFrame work
            let a = t_js.elapsed().as_secs_f64() * 1e3 - 2.0;
            let t_sync = Instant::now();
            p.sync();
            let _ = p.animate();
            let b = t_sync.elapsed().as_secs_f64() * 1e3;
            let t_paint = Instant::now();
            p.paint(&mut px, w, h, 1.0);
            let c = t_paint.elapsed().as_secs_f64() * 1e3;
            let total = t.elapsed().as_secs_f64() * 1e3 - 2.0;
            js_ms += a;
            sync_ms += b;
            paint_ms += c;
            worst = worst.max(total);
            rows.push(total);
        }
        eprintln!(
            "wheel {step:>5}px: scripts {:.2} ms, relayout {:.2} ms, paint {:.2} ms (avg), worst frame {:.1} ms",
            js_ms / n as f64,
            sync_ms / n as f64,
            paint_ms / n as f64,
            worst
        );
        eprintln!("  frames: {}", rows.iter().map(|m| format!("{m:.0}")).collect::<Vec<_>>().join(" "));
    }
}

#[test]
#[ignore]
fn bench_scroll_paint() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/word-ribbon.html");
    let (w, h) = (1100u32, 760u32);
    let mut p = Page::load(file, (w as f32, h as f32), 0.0);
    p.layout(w as f32, h as f32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for _ in 0..3 {
        p.paint(&mut px, w, h, 1.0);
    }
    let n = 60;
    let t = Instant::now();
    for i in 0..n {
        p.view.borrow_mut().scroll_y = (i * 7) as f32;
        p.paint(&mut px, w, h, 1.0);
    }
    eprintln!("paint at 1x: {:.2} ms/frame", t.elapsed().as_secs_f64() * 1e3 / n as f64);
    let (w2, h2) = (w * 2, h * 2);
    let mut px2 = vec![0u8; (w2 * h2 * 4) as usize];
    p.paint(&mut px2, w2, h2, 2.0);
    let t = Instant::now();
    for i in 0..n {
        p.view.borrow_mut().scroll_y = (i * 7) as f32;
        p.paint(&mut px2, w2, h2, 2.0);
    }
    eprintln!("paint at 2x: {:.2} ms/frame", t.elapsed().as_secs_f64() * 1e3 / n as f64);
    // The real sequence: wheel events with the pointer resting on the page, then the frame.
    p.view.borrow_mut().scroll_y = 0.0;
    p.pointer_moved(500.0, 400.0);
    let mut worst = 0.0f64;
    let mut total = 0.0f64;
    let mut all = Vec::new();
    for _ in 0..40 {
        let t = Instant::now();
        p.wheel(0.0, -18.0);
        p.sync();
        let _ = p.animate();
        p.paint(&mut px, w, h, 1.0);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        worst = worst.max(ms);
        total += ms;
        all.push(ms);
    }
    eprintln!("wheel+sync+paint: {:.2} ms avg, {:.2} ms worst", total / 40.0, worst);
    eprintln!("frames: {}", all.iter().map(|m| format!("{m:.1}")).collect::<Vec<_>>().join(" "));
}

#[test]
fn frames_host_documents_route_input_and_talk_to_scripts() {
    let dir = std::env::temp_dir().join(format!("nile-frames-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("child.html"),
        r#"<body style="margin:0"><input id="t"><button id="b" style="display:block">Go</button><p id="o"></p>
           <div style="height:600px"></div>
           <script>
             let n = 0;
             document.$('#b').on('click', () => { n++; document.$('#o').text = n; });
             window.addEventListener('message', (e) => window.parent.postMessage({ echo: e.data.say, same: window.parent !== window }));
           </script></body>"#,
    )
    .unwrap();
    let main = dir.join("main.html");
    std::fs::write(
        &main,
        r#"<body style="margin:0"><p style="margin:0;height:40px">top</p><frame id="f" src="child.html" style="height:200px"></frame>
           <script>
             globalThis.seen = [];
             const f = document.$('#f');
             f.on('newdocument', () => seen.push('new'));
             f.on('complete', () => seen.push('complete'));
             window.addEventListener('message', (e) => seen.push('msg:' + e.data.echo + ':' + e.data.same + ':' + (e.source === f)));
           </script></body>"#,
    )
    .unwrap();
    let mut p = Page::load(main, (600.0, 300.0), 0.0);
    p.layout(600.0, 300.0);
    p.update_frames();
    let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
    assert_eq!(ev(&mut p, "seen.join()"), "new,complete");
    assert_eq!(ev(&mut p, "f.document.$('#b').text"), "Go");

    // A click inside the frame reaches the frame's own script, and the frame takes keyboard focus.
    let id = p.doc.borrow().find(|n| n.attr("id") == Some("f")).unwrap();
    let (fx, fy, ..) = p.frames.borrow().slots[&id].rect;
    assert_eq!((fx, fy), (0.0, 40.0));
    let button = {
        let fr = p.frames.borrow();
        let child = &fr.slots[&id].page;
        let node = child.doc.borrow().find(|n| n.attr("id") == Some("b")).unwrap();
        let r = child.view.borrow_mut().layout().node_rect(node).unwrap();
        (fx + r.x + r.width / 2.0, fy + r.y + r.height / 2.0)
    };
    p.pointer_moved(button.0, button.1);
    p.press();
    p.release();
    assert_eq!(ev(&mut p, "f.document.$('#o').text"), "1");
    assert_eq!(p.focused_frame(), Some(id));

    // Typing goes to the focused frame's input.
    let input = {
        let fr = p.frames.borrow();
        let child = &fr.slots[&id].page;
        let node = child.doc.borrow().find(|n| n.attr("id") == Some("t")).unwrap();
        let r = child.view.borrow_mut().layout().node_rect(node).unwrap();
        (fx + r.x + 4.0, fy + r.y + 4.0)
    };
    p.pointer_moved(input.0, input.1);
    p.press();
    p.release();
    for k in ["h", "i"] {
        let (proceed, _) = p.key("keydown", k.to_string(), ModifiersState::default());
        if proceed {
            p.default_key(k, ModifiersState::default());
        }
    }
    assert_eq!(ev(&mut p, "f.document.$('#t').value"), "hi");

    // The wheel scrolls the frame, not the outer page.
    p.wheel(0.0, -50.0);
    assert_eq!(p.view.borrow().scroll_y, 0.0);
    assert!(p.frames.borrow().slots[&id].page.view.borrow().scroll_y > 0.0);

    // Messages go both ways.
    ev(&mut p, "f.postMessage({ say: 'yo' })");
    p.sync();
    assert_eq!(ev(&mut p, "seen.slice(2).join()"), "msg:yo:true:true");

    // loadHtml and loadFile replace the document and fire the events again.
    ev(&mut p, "f.loadHtml('<p id=z>hello</p>')");
    p.update_frames();
    assert_eq!(ev(&mut p, "f.document.$('#z').text"), "hello");
    assert_eq!(ev(&mut p, "seen.slice(3).join()"), "new,complete");
    ev(&mut p, "f.loadFile('child.html')");
    p.update_frames();
    assert_eq!(ev(&mut p, "f.document.$('#b').text"), "Go");
    assert_eq!(ev(&mut p, "f.document.$('#nope') === null"), "true");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn framesets_split_panes_and_splitters_resize_them() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><frameset id="fs" cols="100px,*,50px" style="height:200px">
             <div id="a">a</div><splitter id="s1"/><div id="b">b</div><splitter id="s2"></splitter><div id="c">c</div>
           </frameset></body>"#,
        "frameset",
    );
    let rect = |p: &mut Page, id: &str| {
        let n = p.doc.borrow().find(|n| n.attr("id") == Some(id)).unwrap();
        let r = p.view.borrow_mut().layout().node_rect(n).unwrap();
        (r.x, r.width)
    };
    assert_eq!(rect(&mut p, "a"), (0.0, 100.0));
    assert_eq!(rect(&mut p, "s1"), (100.0, 6.0));
    assert_eq!(rect(&mut p, "b").0, 106.0);
    assert_eq!(rect(&mut p, "c"), (550.0, 50.0));
    // Dragging the first splitter 40px right widens the first pane and leaves the flexible one to absorb it.
    p.pointer_moved(103.0, 50.0);
    p.press();
    p.pointer_moved(143.0, 50.0);
    p.release();
    assert_eq!(rect(&mut p, "a"), (0.0, 140.0));
    assert_eq!(p.eval("document.$('#fs').getAttribute('cols')").0.unwrap(), "140px,*,50px");
    assert_eq!(p.eval("document.$('#fs').frameset.state.map(Math.round).join()").0.unwrap(), "140,398,50");
    p.eval("document.$('#fs').frameset.state = [60, 300, 80]").0.unwrap();
    assert_eq!(rect(&mut p, "a"), (0.0, 60.0));
    let _ = std::fs::remove_file(path);
}

#[test]
fn scripts_open_native_dialogs() {
    use dialogs::{Dialogs, FileFilter, MessageKind};
    #[derive(Default)]
    struct Fake(RefCell<Vec<String>>);
    impl Dialogs for Fake {
        fn open_files(&self, multiple: bool, filters: &[FileFilter], caption: &str, dir: &str) -> Vec<String> {
            self.0.borrow_mut().push(format!("open {multiple} {:?} {caption} {dir}", filters.iter().map(|f| f.extensions.clone()).collect::<Vec<_>>()));
            vec!["/tmp/a b.txt".into(), "/tmp/c.txt".into()]
        }
        fn save_file(&self, _: &[FileFilter], extension: &str, caption: &str, path: &str) -> Option<String> {
            self.0.borrow_mut().push(format!("save {extension} {caption} {path}"));
            None
        }
        fn pick_folder(&self, caption: &str, _: &str) -> Option<String> {
            self.0.borrow_mut().push(format!("folder {caption}"));
            Some("/tmp/dir".into())
        }
        fn message(&self, kind: MessageKind, caption: &str, text: &str) -> bool {
            self.0.borrow_mut().push(format!("message {kind:?} {caption} {text}"));
            kind != MessageKind::Question
        }
    }
    let (mut p, path) = page(r#"<body><script>
          globalThis.filter = 'Text (*.txt)|*.txt;*.text|All (*.*)|*.*';
        </script></body>"#, "dialogs");
    // Without an embedder dialog host everything is cancelled.
    assert_eq!(p.eval("Window.this.selectFile({}) === null && Window.this.selectFolder() === null").0.unwrap(), "true");
    let fake = Rc::new(Fake::default());
    p.set_dialogs(fake.clone());
    assert_eq!(p.eval("JSON.stringify(Window.this.selectFile({ mode: 'open-multiple', filter, caption: 'Pick', path: '/tmp' }))").0.unwrap(), r#"["file:///tmp/a%20b.txt","file:///tmp/c.txt"]"#);
    assert_eq!(p.eval("Window.this.selectFile({ mode: 'save', extension: 'txt', caption: 'Save' })").0.unwrap(), "null");
    assert_eq!(p.eval("Window.this.selectFolder({ caption: 'Dir' })").0.unwrap(), "file:///tmp/dir");
    assert_eq!(p.eval("Window.this.modal(JSX('question', null, ['Sure?']))").0.unwrap(), "no");
    assert_eq!(p.eval("Window.this.modal(JSX('alert', { caption: 'Hey' }, ['Careful ', 'now']))").0.unwrap(), "yes");
    assert_eq!(
        *fake.0.borrow(),
        [
            "open true [[\"txt\", \"text\"], []] Pick /tmp",
            "save txt Save ",
            "folder Dir",
            "message Question  Sure?",
            "message Alert Hey Careful now",
        ]
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn default_dialogs_are_there_for_the_first_scripts() {
    use dialogs::{Dialogs, FileFilter, MessageKind};
    struct Count(Rc<std::cell::Cell<u32>>);
    impl Dialogs for Count {
        fn open_files(&self, _: bool, _: &[FileFilter], _: &str, _: &str) -> Vec<String> {
            Vec::new()
        }
        fn save_file(&self, _: &[FileFilter], _: &str, _: &str, _: &str) -> Option<String> {
            None
        }
        fn pick_folder(&self, _: &str, _: &str) -> Option<String> {
            None
        }
        fn message(&self, _: MessageKind, _: &str, _: &str) -> bool {
            self.0.set(self.0.get() + 1);
            true
        }
    }
    let shown = Rc::new(std::cell::Cell::new(0));
    Page::set_defaults(Some(Rc::new(Count(shown.clone()))), None);
    let (_p, path) = page(
        r#"<body><script>
          document.addEventListener('DOMContentLoaded', () => { Window.this.modal(JSX('alert', null, ['Hello'])); });
        </script></body>"#,
        "default-dialogs",
    );
    Page::set_defaults(None, None);
    assert_eq!(shown.get(), 1);
    let _ = std::fs::remove_file(path);
}

#[test]
fn popups_open_beside_their_anchor_and_close_on_outside_clicks() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><button id="b" style="position:absolute;left:100px;top:50px;width:80px;height:20px">go</button>
           <div id="elsewhere" style="position:absolute;left:400px;top:200px;width:50px;height:50px"></div>
           <menu.context id="m"><li id="one">One</li><li id="two">Two</li></menu>
           <script>
             globalThis.seen = [];
             const b = document.$('#b');
             b.on('popup-request', (e) => seen.push('request:' + e.source.id));
             b.on('popup-dismissed', () => seen.push('dismissed'));
             document.on('popup-ready', (e) => seen.push('ready'));
             document.on('popup-dismissing', (e) => seen.push('dismissing:' + e.target.id));
             document.on('click', 'li', (e, li) => { seen.push('picked:' + li.id); });
             globalThis.win = null;
           </script></body>"#,
        "popups",
    );
    let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
    // The menu is hidden until it is popped up (menu.context is the class shorthand).
    assert_eq!(ev(&mut p, "document.$('#m').getBoundingClientRect().height"), "0");
    ev(&mut p, "win = b.popup(document.$('#m'))");
    // Below the anchor's bottom-left corner.
    assert_eq!(ev(&mut p, "const ar = b.getBoundingClientRect(); JSON.stringify(['left','top'].map(k => Math.round(document.$('#m').getBoundingClientRect()[k]) - Math.round(k === 'top' ? ar.bottom : ar.left)))"), "[0,0]");
    assert_eq!(ev(&mut p, "win.isValid"), "true");
    // Clicking an item runs its handler and closes the menu.
    click(&mut p, "two");
    p.tick();
    assert_eq!(ev(&mut p, "seen.slice(0, 3).join()"), "request:m,ready,picked:two");
    assert_eq!(ev(&mut p, "win.isValid"), "false");
    assert_eq!(ev(&mut p, "seen.slice(3).join()"), "dismissing:m,dismissed");
    // Pressing outside closes it too.
    ev(&mut p, "win = b.popup(document.$('#m'), { anchorAt: 3, popupAt: 9 })");
    assert_eq!(ev(&mut p, "Math.round(document.$('#m').getBoundingClientRect().right - ar.right) + ',' + Math.round(document.$('#m').getBoundingClientRect().top - ar.bottom)"), "0,0");
    click(&mut p, "elsewhere");
    assert_eq!(ev(&mut p, "win.isValid"), "false");
    // So does a press on bare canvas, where no element is under the pointer.
    ev(&mut p, "win = b.popup(document.$('#m'))");
    p.pointer_moved(590.0, 290.0);
    p.press();
    p.release();
    assert_eq!(ev(&mut p, "win.isValid"), "false");
    // Airborne elements sit at the given window position until taken back.
    ev(&mut p, "const o = document.$('#elsewhere').takeOff({ x: 10, y: 20, width: 33, relativeTo: 'window' })");
    assert_eq!(ev(&mut p, "JSON.stringify(['left','top','width'].map(k => Math.round(document.$('#elsewhere').getBoundingClientRect()[k])))"), "[10,20,33]");
    ev(&mut p, "document.$('#elsewhere').takeOff()");
    assert_eq!(ev(&mut p, "Math.round(document.$('#elsewhere').getBoundingClientRect().left)"), "400");
    // A right click hands scripts `contextmenu`; `evt.source` is shown as a menu at the pointer.
    ev(&mut p, "document.$('#elsewhere').on('contextmenu', (e) => { e.source = document.$('#m'); })");
    let (x, y) = center(&p, "elsewhere");
    p.pointer_moved(x, y);
    p.context_menu();
    assert_eq!(ev(&mut p, "JSON.stringify(['left','top'].map(k => Math.round(document.$('#m').getBoundingClientRect()[k])))"), format!("[{},{}]", x as i32, y as i32));
    let _ = std::fs::remove_file(path);
}

#[test]
fn example_popup_closes_on_outside_clicks() {
    let mut p = Page::load(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/popups.html")), (900.0, 640.0), 0.0);
    p.layout(900.0, 640.0);
    let open = |p: &mut Page| p.eval("document.$$('menu.popup, popup').length").0.unwrap();
    assert_eq!(open(&mut p), "0");
    p.pointer_moved(40.0, 88.0);
    p.press();
    assert!(p.release(), "opening a popup asks for a repaint");
    assert_eq!(open(&mut p), "1", "the File menu opened");
    for (x, y) in [(500.0, 400.0), (300.0, 170.0), (850.0, 20.0)] {
        p.pointer_moved(x, y);
        p.press();
        p.release();
        assert_eq!(open(&mut p), "0", "closed by a press at {x},{y}");
        p.pointer_moved(40.0, 88.0);
        p.press();
        p.release();
        assert_eq!(open(&mut p), "1");
    }
    // The pixels agree: the menu is painted while open and gone after the press.
    let pixel = |p: &mut Page| {
        let mut buf = vec![0u8; 900 * 640 * 4];
        p.paint(&mut buf, 900, 640, 1.0);
        buf
    };
    p.pointer_moved(500.0, 400.0);
    p.press();
    p.release();
    let closed = pixel(&mut p);
    p.pointer_moved(40.0, 88.0);
    p.press();
    p.release();
    let opened = pixel(&mut p);
    assert_ne!(closed, opened, "the menu is painted");
    p.pointer_moved(500.0, 400.0);
    p.press();
    p.release();
    assert_eq!(pixel(&mut p), closed, "and gone again");
    // Clicking the button that opened a menu closes it (and doesn't open it again).
    p.pointer_moved(40.0, 88.0);
    p.press();
    p.release();
    assert_eq!(open(&mut p), "1");
    p.press();
    p.release();
    assert_eq!(open(&mut p), "0", "a second click on the button closes the menu");
    p.press();
    p.release();
    assert_eq!(open(&mut p), "1");
    p.pointer_moved(500.0, 400.0);
    p.press();
    p.release();
    // The right-click menu closes the same way.
    p.pointer_moved(500.0, 400.0);
    p.press();
    p.release();
    assert_eq!(open(&mut p), "0");
    p.pointer_moved(300.0, 170.0);
    p.context_menu();
    let shown = || p.eval("Math.round(document.$('#ctx').getBoundingClientRect().height) > 0").0.unwrap();
    let _ = shown;
    assert_eq!(p.eval("Math.round(document.$('#ctx').getBoundingClientRect().height) > 0").0.unwrap(), "true");
    p.pointer_moved(500.0, 400.0);
    let redraw = p.press() | p.release();
    assert!(redraw, "closing a popup asks for a repaint");
    assert_eq!(p.eval("Math.round(document.$('#ctx').getBoundingClientRect().height) > 0").0.unwrap(), "false");
}

#[test]
fn inline_message_boxes_are_html_and_resolve_a_promise() {
    let (mut p, path) = page(r#"<body style="margin:0"><script>globalThis.answers = [];</script></body>"#, "inlinebox");
    let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
    // The default stays the system's box (here no host, so the fallback answers).
    assert_eq!(ev(&mut p, "Window.this.messageBoxes"), "native");
    assert_eq!(ev(&mut p, "Window.this.modal(JSX('question', null, ['Sure?']))"), "no");
    // An `inline` attribute opts one box in; buttons and rich content come from the markup.
    ev(&mut p, "Window.this.modal(JSX('question', { inline: true, caption: 'Quit' }, [JSX('b', null, ['Save']), ' first?', JSX('button', { value: 'save', default: true }, ['Save']), JSX('button', { id: 'discard' }, [\"Don't save\"]), JSX('button', { value: 'cancel', cancel: true }, ['Cancel'])])).then((v) => answers.push(v))");
    assert_eq!(ev(&mut p, "document.$$('.nile-messagebox button').map((b) => b.textContent).join('|')"), "Save|Don't save|Cancel");
    assert_eq!(ev(&mut p, "document.$('.nile-messagebox-title').textContent + ':' + document.$('.nile-messagebox-text b').textContent"), "Quit:Save");
    assert_eq!(ev(&mut p, "document.$('.nile-messagebox.question') !== null && document.activeElement.textContent"), "Save");
    // The box covers the window and is gone once an answer is given.
    assert_eq!(ev(&mut p, "JSON.stringify(document.$('.nile-modal').getBoundingClientRect().width)"), "600");
    let (x, y) = {
        let n = p.eval("document.$$('.nile-messagebox button')[1]._id").0.unwrap().parse::<usize>().unwrap();
        let r = p.view.borrow_mut().layout().node_rect(NodeId(n)).unwrap();
        (r.x + r.width / 2.0, r.y + r.height / 2.0)
    };
    p.pointer_moved(x, y);
    p.press();
    p.release();
    assert_eq!(ev(&mut p, "answers.join()"), "discard");
    assert_eq!(ev(&mut p, "document.$('.nile-modal')"), "null");
    // The global setting, the default buttons and Escape.
    ev(&mut p, "Window.this.messageBoxes = 'inline'; Window.this.modal(JSX('question', null, ['Delete?'])).then((v) => answers.push(v))");
    assert_eq!(ev(&mut p, "document.$$('.nile-messagebox button').map((b) => b.textContent).join('|')"), "No|Yes");
    p.key("keydown", "Escape".to_string(), ModifiersState::default());
    assert_eq!(ev(&mut p, "answers.slice(1).join()"), "no");
    ev(&mut p, "Window.this.modal(JSX('info', null, ['Done'])).then((v) => answers.push(v))");
    // Enter on the focused default button clicks it, as for any button.
    let (proceed, _) = p.key("keydown", "Enter".to_string(), ModifiersState::default());
    if proceed {
        p.default_key("Enter", ModifiersState::default());
    }
    assert_eq!(ev(&mut p, "answers.slice(2).join()"), "yes");
    // `native` wins over the global setting.
    assert_eq!(ev(&mut p, "Window.this.modal(JSX('question', { native: true }, ['x']))"), "no");
    let _ = std::fs::remove_file(path);
}

#[test]
fn tooltips_follow_title_and_tooltip_attributes() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="a" title="Plain tip" style="width:100px;height:40px"></div>
           <div id="b" tooltip="<b>Rich</b> tip" style="width:100px;height:40px"></div>
           <div id="c" style="width:100px;height:40px"><span id="d" title="Inherited from a child"></span>x</div>
           <div id="e" style="width:100px;height:40px"></div>
           <div id="f" title="Never shown" style="width:100px;height:40px"></div>
           <div id="custom" style="width:100px;height:40px"></div>
           <script>
             document.$('#custom').on('tooltiprequest', (e) => { const t = document.createElement('div'); t.id = 'mine'; t.style.display = 'none'; t.textContent = 'dynamic'; document.body.appendChild(t); e.source = t; });
           </script></body>"#,
        "tooltips",
    );
    let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
    let tip = |p: &mut Page| ev(p, "document.$('popup[role=tooltip]') ? document.$('popup[role=tooltip]').innerHTML : ''");
    let rest = |p: &mut Page, id: &str| {
        let (x, y) = center(p, id);
        p.pointer_moved(x, y);
        assert_eq!(tip(p), "", "nothing shows before the pointer has rested");
        std::thread::sleep(TOOLTIP_DELAY + Duration::from_millis(40));
        p.tick();
    };
    rest(&mut p, "a");
    assert_eq!(tip(&mut p), "Plain tip");
    // Near the pointer, not covering it.
    assert_eq!(ev(&mut p, "document.$('popup[role=tooltip]').getBoundingClientRect().top > 20"), "true");
    // Moving to another element replaces it; `tooltip` is HTML.
    rest(&mut p, "b");
    assert_eq!(tip(&mut p), "<b>Rich</b> tip");
    // A press or a key hides it.
    p.press();
    p.release();
    assert_eq!(tip(&mut p), "");
    // `title` on the pointer's own element or an ancestor; none: nothing.
    rest(&mut p, "e");
    assert_eq!(tip(&mut p), "");
    // A script answers `tooltiprequest` with its own element.
    rest(&mut p, "custom");
    assert_eq!(ev(&mut p, "document.$('#mine').getBoundingClientRect().width > 0"), "true");
    p.pointer_moved(500.0, 290.0);
    assert_eq!(ev(&mut p, "document.$('#mine').getBoundingClientRect().width > 0"), "false", "restored to hidden");
    let _ = std::fs::remove_file(path);
}

#[test]
fn popup_position_and_animation_css() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><button id="b" style="position:absolute;left:100px;top:50px;width:80px;height:20px">go</button>
           <popup id="pp" style="popup-position: top-right bottom-right; popup-animation: popup-animation(type:slide, axis:vertical, duration:100ms)">menu text</popup>
           <popup id="q" style="width:60px">other</popup>
           <style>popup#q { popup-reference-point: top-left; popup-anchor-reference-point: bottom-center }</style></body>"#,
        "popupcss",
    );
    let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
    assert_eq!(ev(&mut p, "getComputedStyle(document.$('#pp')).getPropertyValue('popup-position')"), "top-right bottom-right");
    assert!(ev(&mut p, "getComputedStyle(document.$('#pp')).getPropertyValue('popup-animation')").contains("slide"));
    ev(&mut p, "globalThis.w = document.$('#b').popup(document.$('#pp'), { animationDuration: 0 })");
    // The popup's right edge meets the anchor's right edge, its top the anchor's bottom.
    assert_eq!(ev(&mut p, "(() => { const r = document.$('#pp').getBoundingClientRect(), a = document.$('#b').getBoundingClientRect(); return Math.round(r.right - a.right) + ',' + Math.round(r.top - a.bottom); })()"), "0,0");
    ev(&mut p, "w.close()");
    ev(&mut p, "document.$('#b').popup(document.$('#q'))");
    assert_eq!(ev(&mut p, "(() => { const r = document.$('#q').getBoundingClientRect(), a = document.$('#b').getBoundingClientRect(); return Math.round(r.left - (a.left + a.width / 2)) + ',' + Math.round(r.top - a.bottom); })()"), "0,0");
    let _ = std::fs::remove_file(path);
}

#[test]
fn scripts_open_windows_and_talk_to_them() {
    let (mut p, path) = page(r#"<body><script>globalThis.log = [];
          window.addEventListener('message', (e) => log.push('msg:' + e.data.hi + ':' + e.source._id));
          Window.this.on('closerequest', (e) => { log.push('closerequest:' + e.reason); if (globalThis.keep) e.preventDefault(); });</script></body>"#, "windows");
    let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
    let take = |p: &Page| std::mem::take(&mut p.view.borrow_mut().window_requests);
    // `new Window` asks the shell for a window and gets its id at once.
    let id: u32 = ev(&mut p, "globalThis.w = new Window({ url: 'child.html', width: 300, height: 200, caption: 'Hi there', type: Window.TOOL_WINDOW, alignment: -5, parameters: { a: [1, 2] } }); w._id").parse().unwrap();
    let Some(WindowCommand::Open(spec)) = take(&p).into_iter().next() else { panic!("an Open request") };
    assert_eq!((spec.id, spec.modal), (id, false));
    assert_eq!((spec.get("url"), spec.get("caption"), spec.get("type"), spec.get("alignment")), (Some("child.html"), Some("Hi there"), Some("tool"), Some("-5")));
    assert_eq!((spec.get("width"), spec.get("parameters")), (Some("300"), Some(r#"{"a":[1,2]}"#)));
    // What the opened window sends back, and its close, reach the opener.
    p.window_message(id, r#"{"hi":"there"}"#);
    assert_eq!(ev(&mut p, "log.join()"), format!("msg:there:{id}"));
    ev(&mut p, "w.closed.then((v) => log.push('closed:' + JSON.stringify(v))); w.on('close', (e) => log.push('close:' + e.value))");
    assert_eq!(ev(&mut p, "w.isValid"), "true");
    p.window_closed(id, r#"{"ok":true}"#);
    assert_eq!(ev(&mut p, "log.slice(1).join()"), r#"close:[object Object],closed:{"ok":true}"#);
    assert_eq!(ev(&mut p, "w.isValid"), "false");
    // A dialog is flagged modal and resolves a Promise.
    ev(&mut p, "Window.this.modal({ url: 'dlg.html' }).then((v) => log.push('dialog:' + v))");
    let Some(WindowCommand::Open(dlg)) = take(&p).into_iter().next() else { panic!("an Open request") };
    assert!(dlg.modal);
    assert_eq!(dlg.get("type"), Some("dialog"));
    p.window_closed(dlg.id, "\"saved\"");
    assert_eq!(ev(&mut p, "log.slice(3).join()"), "dialog:saved");
    // Messages out, closing, and `closerequest`.
    ev(&mut p, "globalThis.w2 = new Window({ html: '<p>x</p>' }); w2.postMessage({ n: 1 }); w2.close('bye'); Window.this.postMessage(5); Window.this.close(7)");
    let cmds = take(&p);
    assert!(matches!(&cmds[1], WindowCommand::Post { json, .. } if json == r#"{"n":1}"#));
    assert!(matches!(&cmds[2], WindowCommand::CloseId(_)));
    assert!(matches!(&cmds[3], WindowCommand::Post { to: 0, json } if json == "5"));
    assert_eq!(cmds[4], WindowCommand::CloseWith("7".into()));
    assert!(p.close_request(0));
    ev(&mut p, "globalThis.keep = true");
    assert!(!p.close_request(1), "a listener can keep the window open");
    assert_eq!(ev(&mut p, "log.slice(4).join()"), "closerequest:0,closerequest:1");
    // Window.share crosses windows as JSON.
    ev(&mut p, "Window.share.theme = { dark: true }; Window.share.n = 3");
    assert_eq!(ev(&mut p, "JSON.stringify([Window.share.theme, Window.share.n, 'n' in Window.share, Object.keys(Window.share).sort(), Window.share.nothing])"), r#"[{"dark":true},3,true,["n","theme"],null]"#);
    let _ = std::fs::remove_file(path);
}

#[test]
fn an_opened_window_knows_its_opener_and_parameters() {
    let path = std::env::temp_dir().join(format!("nile-opened-{}.html", std::process::id()));
    std::fs::write(&path, "<body><script>globalThis.seen = [Window.this.parent && Window.this.parent._id, Window.this.parameters && Window.this.parameters.x];</script></body>").unwrap();
    let info = WindowInfo { id: 12, opener: Some(7), modal: true, parameters: r#"{"x":42}"#.into() };
    let mut p = Page::load_window(path.clone(), (400.0, 300.0), info.clone());
    assert_eq!(p.eval("JSON.stringify(seen)").0.unwrap(), "[7,42]");
    p.reload();
    assert_eq!(p.eval("JSON.stringify(seen)").0.unwrap(), "[7,42]", "reloading keeps who opened it");
    assert_eq!(p.view.borrow().window_info, info);
    let _ = std::fs::remove_file(path);
}

#[test]
fn popups_can_live_in_windows_of_their_own() {
    let (mut p, path) = page(
        r#"<body style="margin:0;background:#ffffff"><button id="b" style="position:absolute;left:10px;top:10px">go</button>
           <popup id="m" style="width:100px;height:40px;background:#ff0000;border:0;padding:0;box-shadow:none"></popup></body>"#,
        "portal",
    );
    let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
    let pixel = |buf: &[u8], w: u32, x: u32, y: u32| {
        let at = ((y * w + x) * 4) as usize;
        [buf[at + 2], buf[at + 1], buf[at]] // r, g, b
    };
    let paint = |p: &mut Page| {
        let mut buf = vec![0u8; 600 * 300 * 4];
        p.paint(&mut buf, 600, 300, 1.0);
        buf
    };
    // In the window the popup is painted by the page.
    ev(&mut p, "globalThis.w = document.$('#b').popup(document.$('#m'), { x: 200, y: 100 })");
    assert_eq!(pixel(&paint(&mut p), 600, 220, 110), [255, 0, 0]);
    ev(&mut p, "w.close()");
    // With a window of its own the page leaves it out and reports where it is...
    ev(&mut p, "globalThis.w = document.$('#b').popup(document.$('#m'), { x: 200, y: 100, window: 'popup' })");
    let portals = p.view.borrow().portals.clone();
    assert_eq!(portals.len(), 1);
    assert_eq!(portals[0].rect, (200.0, 100.0, 100.0, 40.0));
    assert_ne!(pixel(&paint(&mut p), 600, 220, 110), [255, 0, 0], "not drawn twice");
    // ...and paints it for that window, with the page point `origin` at the corner.
    let mut buf = vec![0u8; 100 * 40 * 4];
    p.paint_portal(&mut buf, 100, 40, 1.0, portals[0].node, (200.0, 100.0));
    assert_eq!(pixel(&buf, 100, 5, 5), [255, 0, 0]);
    assert_eq!(pixel(&buf, 100, 95, 35), [255, 0, 0]);
    // A popup may lie outside the window altogether.
    ev(&mut p, "w.close()");
    assert!(p.view.borrow().portals.is_empty());
    ev(&mut p, "globalThis.w = document.$('#b').popup(document.$('#m'), { x: 900, y: -20, window: 'popup' })");
    assert_eq!(p.view.borrow().portals[0].rect.0, 900.0);
    let node = p.view.borrow().portals[0].node;
    let mut buf = vec![0u8; 100 * 40 * 4];
    p.paint_portal(&mut buf, 100, 40, 1.0, node, (900.0, -20.0));
    assert_eq!(pixel(&buf, 100, 50, 20), [255, 0, 0]);
    // The pointer works in window coordinates: hovering the popup at its screen spot hits it.
    p.pointer_moved(950.0, 0.0);
    assert_eq!(p.interaction().hover, Some(node));
    let _ = std::fs::remove_file(path);
}

#[test]
fn table_rows_columns_and_merged_cells_are_editable() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="h" contenteditable><table id="t"><tbody>
             <tr><td>a1</td><td>b1</td><td>c1</td></tr>
             <tr><td>a2</td><td>b2</td><td>c2</td></tr>
             <tr><td>a3</td><td>b3</td><td>c3</td></tr></tbody></table><p>after</p></div></body>"#,
        "table_commands",
    );
    // The table as text: rows by `/`, cells by `|`, spans as `[colsxrows]`.
    let show = |p: &mut Page| js(p, "Array.from(document.querySelectorAll('#t tr')).map(tr => Array.from(tr.children).map(c => c.textContent.trim() + (c.colSpan > 1 || c.rowSpan > 1 ? '[' + c.colSpan + 'x' + c.rowSpan + ']' : '')).join('|')).join('/')");
    let caret = |p: &mut Page, cell: &str| js(p, &format!("(() => {{ const c = Array.from(document.querySelectorAll('#t td')).find(x => x.textContent.trim().startsWith('{cell}')); const r = document.createRange(); r.setStart(c.firstChild, 1); r.collapse(true); const s = getSelection(); s.removeAllRanges(); s.addRange(r); return 0; }})()"));
    let cmd = |p: &mut Page, name: &str| js(p, &format!("document.execCommand('{name}'); 0"));
    caret(&mut p, "b2");
    cmd(&mut p, "tableInsertRowBelow");
    assert_eq!(show(&mut p), "a1|b1|c1/a2|b2|c2/||/a3|b3|c3");
    cmd(&mut p, "tableInsertRowAbove");
    assert_eq!(js(&mut p, "document.querySelectorAll('#t tr').length"), "5");
    caret(&mut p, "b3");
    cmd(&mut p, "tableDeleteRow");
    cmd(&mut p, "tableDeleteRow");
    assert_eq!(show(&mut p), "a1|b1|c1/a2|b2|c2/||");
    cmd(&mut p, "tableDeleteRow");
    caret(&mut p, "b1");
    cmd(&mut p, "tableInsertColumnRight");
    assert_eq!(show(&mut p), "a1|b1||c1/a2|b2||c2");
    cmd(&mut p, "tableInsertColumnLeft");
    assert_eq!(show(&mut p), "a1||b1||c1/a2||b2||c2");
    caret(&mut p, "c1");
    cmd(&mut p, "tableDeleteColumn");
    assert_eq!(show(&mut p), "a1||b1|/a2||b2|");
    // Merge the block from a1 to b2 (three columns, two rows) into one cell, then split it again.
    js(&mut p, "(() => { const tds = Array.from(document.querySelectorAll('#t td')); const a = tds.find(x => x.textContent === 'a1'), b = tds.find(x => x.textContent === 'b2'); const r = document.createRange(); r.setStart(a.firstChild, 0); r.setEnd(b.firstChild, 1); const s = getSelection(); s.removeAllRanges(); s.addRange(r); return 0; })()");
    cmd(&mut p, "tableMergeCells");
    assert_eq!(js(&mut p, "document.querySelectorAll('#t td').length"), "3");
    let merged = show(&mut p);
    assert!(merged.starts_with("a1b1a2b2[3x2]") || (merged.contains("a1") && merged.contains("a2") && merged.contains("b2") && merged.contains("[3x2]")), "{merged}");
    // A row inserted below the merged cell grows its span only when the line crosses it.
    caret(&mut p, "a1");
    cmd(&mut p, "tableInsertRowBelow");
    assert_eq!(js(&mut p, "document.querySelectorAll('#t tr').length + ':' + Array.from(document.querySelectorAll('#t td')).filter(c => c.rowSpan > 1).length"), "3:1");
    caret(&mut p, "a1");
    cmd(&mut p, "tableSplitCell");
    assert_eq!(js(&mut p, "document.querySelectorAll('#t tr').length + ':' + Array.from(document.querySelectorAll('#t td')).every(c => c.rowSpan === 1 && c.colSpan === 1)"), "3:true");
    // The last row or column takes the table with it; undo brings it back.
    cmd(&mut p, "tableDeleteTable");
    assert_eq!(js(&mut p, "document.querySelectorAll('#t').length"), "0");
    js(&mut p, "document.execCommand('undo'); 0");
    assert_eq!(js(&mut p, "document.querySelectorAll('#t').length"), "1");
    let _ = std::fs::remove_file(path);
}

#[test]
fn images_in_the_editor_select_resize_and_delete() {
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";
    let (mut p, path) = page(
        &format!(r#"<body style="margin:0"><div id="h" contenteditable style="padding:10px"><p id="a">before <img id="i" src="data:image/png;base64,{png}" style="width:100px;height:60px"> after</p></div></body>"#),
        "editor_image",
    );
    let none = ModifiersState::default();
    // A click on the image selects it: outline and eight handles.
    let (x, y) = center(&p, "i");
    p.pointer_moved(x, y);
    p.press();
    p.release();
    let img = p.doc.borrow().find(|n| n.attr("id") == Some("i")).unwrap();
    assert_eq!(p.selected_image(), Some(img));
    let r = p.selected_image_rect().unwrap();
    assert_eq!((r.width, r.height), (100.0, 60.0));
    // Over a corner handle the cursor says so; dragging it resizes in proportion.
    p.pointer_moved(r.x + r.width, r.y + r.height);
    assert_eq!(p.cursor(), nile_style::Cursor::ResizeNwse);
    p.press();
    p.pointer_moved(r.x + r.width + 50.0, r.y + r.height + 5.0);
    p.release();
    let size = |p: &mut Page| js(p, "document.getElementById('i').getAttribute('width') + 'x' + document.getElementById('i').getAttribute('height')");
    assert_eq!(size(&mut p), "129x78", "the corner keeps the proportions");
    assert_eq!(p.selected_image(), Some(img), "still selected");
    // An edge handle moves one side only.
    let r = p.selected_image_rect().unwrap();
    p.pointer_moved(r.x + r.width / 2.0, r.y + r.height);
    assert_eq!(p.cursor(), nile_style::Cursor::ResizeV);
    p.press();
    p.pointer_moved(r.x + r.width / 2.0, r.y + r.height - 20.0);
    p.release();
    assert_eq!(size(&mut p), "129x58");
    // Undo puts the old sizes back, step by step.
    js(&mut p, "document.execCommand('undo'); 0");
    assert_eq!(size(&mut p), "129x78");
    js(&mut p, "document.execCommand('undo'); 0");
    assert_eq!(js(&mut p, "document.getElementById('i').getAttribute('width')"), "null");
    // Delete removes the image. (Undo rebuilt the document, so the image is a new node.)
    let (x, y) = {
        let c = js(&mut p, "JSON.stringify((() => { const r = document.getElementById('i').getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })())");
        let v: Vec<f32> = c.trim_matches(|ch| ch == '[' || ch == ']').split(',').map(|n| n.parse().unwrap()).collect();
        (v[0], v[1])
    };
    p.pointer_moved(x, y);
    p.press();
    p.release();
    assert!(p.selected_image().is_some());
    press_key(&mut p, "Backspace", none);
    assert_eq!(js(&mut p, "document.querySelectorAll('#h img').length + ':' + document.getElementById('a').textContent"), "0:before  after");
    let _ = std::fs::remove_file(path);
}

#[test]
fn media_query_lists_fire_change_when_the_system_theme_flips() {
    use nile_style::{set_system_theme, Color, SystemTheme};
    set_system_theme(SystemTheme { dark: false, accent: Color::rgb(10, 20, 30) });
    let (mut p, path) = page(
        r#"<body><script>
             window.log = [];
             const m = matchMedia('(prefers-color-scheme: dark)');
             m.addEventListener('change', (e) => log.push('listener:' + e.matches + ':' + e.media + ':' + (e instanceof MediaQueryListEvent)));
             m.onchange = (e) => log.push('onchange:' + e.matches);
             window.unwatched = matchMedia('(prefers-color-scheme: dark)');
             window.old = matchMedia('(prefers-color-scheme: dark)');
             old.addListener((e) => log.push('old:' + e.matches));
           </script></body>"#,
        "mql-change",
    );
    assert_eq!(js(&mut p, "log.length"), "0");
    p.system_theme_changed();
    assert_eq!(js(&mut p, "log.length"), "0", "nothing flipped, nothing fires");
    set_system_theme(SystemTheme { dark: true, accent: Color::rgb(10, 20, 30) });
    p.system_theme_changed();
    assert_eq!(
        js(&mut p, "log.slice().sort().join('|')"),
        "listener:true:(prefers-color-scheme: dark):true|old:true|onchange:true"
    );
    assert_eq!(js(&mut p, "unwatched.matches"), "true");
    p.system_theme_changed();
    assert_eq!(js(&mut p, "log.length"), "3", "an unchanged answer doesn't fire again");
    set_system_theme(SystemTheme { dark: false, accent: Color::rgb(10, 20, 30) });
    p.system_theme_changed();
    assert_eq!(js(&mut p, "log.filter((l) => l.includes(':false')).length"), "3");
    set_system_theme(SystemTheme::default());
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_canvas_paints_what_scripts_draw_and_repaints_without_a_new_layout() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><canvas id="c" width="40" height="20"></canvas><script>
             window.ctx = document.getElementById('c').getContext('2d');
             ctx.fillStyle = '#ff0000'; ctx.fillRect(0, 0, 40, 20);
           </script></body>"#,
        "canvas",
    );
    assert_eq!(js(&mut p, "document.getElementById('c').width + 'x' + document.getElementById('c').height"), "40x20");
    let rect = p.view.borrow_mut().layout().node_rect(p.doc.borrow().find(|n| n.attr("id") == Some("c")).unwrap()).unwrap();
    assert_eq!((rect.width, rect.height), (40.0, 20.0));
    let (reds, blues) = count_colored(&mut p);
    assert_eq!((reds, blues), (40 * 20, 0), "the canvas shows its red fill");
    // Drawing again changes the picture and asks for a repaint, but not a new document version.
    p.sync();
    let version = p.doc.borrow().version;
    let (_, repaint) = p.eval("ctx.fillStyle = '#0000ff'; ctx.fillRect(0, 0, 20, 20);");
    assert_eq!(p.doc.borrow().version, version, "drawing doesn't touch the document");
    assert!(repaint, "a canvas drawn on makes the page repaint");
    assert!(!p.sync(), "and only once");
    let (reds, blues) = count_colored(&mut p);
    assert_eq!((reds, blues), (20 * 20, 20 * 20));
    // Resizing clears the canvas and re-lays-out the box.
    js(&mut p, "document.getElementById('c').width = 10;");
    assert_eq!(count_colored(&mut p), (0, 0));
    let rect = p.view.borrow_mut().layout().node_rect(p.doc.borrow().find(|n| n.attr("id") == Some("c")).unwrap()).unwrap();
    assert_eq!(rect.width, 10.0);
    let _ = std::fs::remove_file(path);
}

#[test]
fn inline_svg_paints_follows_color_and_changes_with_scripts() {
    let (mut p, path) = page(
        r##"<style>.t { fill: #ff0000 } #cur { color: #0000ff }</style><body style="margin:0">
             <svg id="a" width="20" height="10" viewBox="0 0 20 10"><rect class="t" width="20" height="10" fill="#00ff00"/></svg>
             <svg id="cur" width="10" height="10"><rect id="c" width="10" height="10" fill="currentColor"/></svg>
           </body>"##,
        "svg-inline",
    );
    let rect = p.view.borrow_mut().layout().node_rect(p.doc.borrow().find(|n| n.attr("id") == Some("a")).unwrap()).unwrap();
    assert_eq!((rect.width, rect.height), (20.0, 10.0), "sized by its width and height");
    let (reds, blues) = count_colored(&mut p);
    // The page rule beats the fill attribute (20x10 red) and currentColor takes the CSS colour (10x10 blue).
    assert_eq!((reds, blues), (200, 100));
    // A script changes the SVG: it repaints with the new picture.
    js(&mut p, "document.getElementById('c').setAttribute('fill', '#ff0000');");
    let (reds, blues) = count_colored(&mut p);
    assert_eq!((reds, blues), (300, 0));
    // An SVG made by script (createElementNS keeps the camel-case names the format needs).
    js(
        &mut p,
        r##"const ns = 'http://www.w3.org/2000/svg';
           const s = document.createElementNS(ns, 'svg'); s.setAttribute('width', 30); s.setAttribute('height', 10); s.setAttribute('viewBox', '0 0 3 1');
           const r = document.createElementNS(ns, 'rect'); r.setAttribute('width', 3); r.setAttribute('height', 1); r.setAttribute('fill', '#0000ff');
           s.appendChild(r); document.body.appendChild(s);"##,
    );
    let (_, blues) = count_colored(&mut p);
    // 30x10 blue (the viewBox is scaled up), give or take the antialiased edges of a fractional position.
    assert!((280..=300).contains(&blues), "the new svg is painted blue: {blues}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn img_and_backgrounds_take_svg_files() {
    let svg = "data:image/svg+xml;utf8,<svg%20xmlns=%22http://www.w3.org/2000/svg%22%20width=%2210%22%20height=%2210%22><rect%20width=%2210%22%20height=%2210%22%20fill=%22%23ff0000%22/></svg>";
    let (mut p, path) = page(
        &format!(r#"<body style="margin:0"><img id="i" src="{svg}"><div style="width:10px;height:10px;background:url({svg})"></div></body>"#),
        "svg-img",
    );
    let rect = p.view.borrow_mut().layout().node_rect(p.doc.borrow().find(|n| n.attr("id") == Some("i")).unwrap()).unwrap();
    assert_eq!((rect.width, rect.height), (10.0, 10.0), "an svg image has its own size");
    assert_eq!(count_colored(&mut p).0, 200, "both the <img> and the background show it");
    let _ = std::fs::remove_file(path);
}

#[test]
fn filters_clip_paths_masks_and_blend_modes_change_the_pixels() {
    let (mut p, path) = page(
        r#"<body style="margin:0;background:#ffffff">
           <div style="position:absolute;left:0;top:0;width:40px;height:40px;background:#ff0000;filter:grayscale(1)"></div>
           <div style="position:absolute;left:50px;top:0;width:40px;height:40px;background:#ff0000;clip-path:circle(50%)"></div>
           <div style="position:absolute;left:100px;top:0;width:40px;height:40px;background:#000000;mask-image:linear-gradient(90deg,#000,transparent)"></div>
           <div style="position:absolute;left:150px;top:0;width:40px;height:40px;background:#ffff00"></div>
           <div style="position:absolute;left:150px;top:0;width:40px;height:40px;background:#00ffff;mix-blend-mode:multiply"></div>
           <div style="position:absolute;left:200px;top:0;width:40px;height:40px;background:#ff0000;visibility:hidden"></div>
           </body>"#,
        "effects",
    );
    let mut buf = vec![0u8; 300 * 60 * 4];
    p.paint(&mut buf, 300, 60, 1.0);
    let px = |x: u32, y: u32| {
        let at = ((y * 300 + x) * 4) as usize;
        [buf[at + 2], buf[at + 1], buf[at]]
    };
    let gray = px(20, 20);
    assert!(gray[0].abs_diff(gray[1]) < 3 && gray[1].abs_diff(gray[2]) < 3 && gray[0] > 30 && gray[0] < 120, "grayscale: {gray:?}");
    assert_eq!(px(70, 20), [255, 0, 0], "inside the circle");
    assert_eq!(px(51, 1), [255, 255, 255], "the circle's corner is clipped");
    let (left, right) = (px(101, 20), px(138, 20));
    assert!(left[0] < 40 && right[0] > 215, "the mask fades the black box out to the right: {left:?} {right:?}");
    assert_eq!(px(170, 20), [0, 255, 0], "yellow multiplied by cyan is green");
    assert_eq!(px(220, 20), [255, 255, 255], "visibility: hidden draws nothing");
    let _ = std::fs::remove_file(path);
}

/// A mono 8 kHz, 16-bit WAV of `ms` milliseconds of silence.
fn silent_wav(ms: u32) -> Vec<u8> {
    let data = 8 * ms * 2;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&[1, 0, 1, 0]);
    v.extend_from_slice(&8000u32.to_le_bytes());
    v.extend_from_slice(&16000u32.to_le_bytes());
    v.extend_from_slice(&[2, 0, 16, 0]);
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    v.resize(v.len() + data as usize, 0);
    v
}

#[test]
fn a_drag_with_files_that_leaves_the_window_is_handed_to_the_system() {
    // Absolute paths look different per platform: build them from the temp folder.
    let abs = std::env::temp_dir().join("nile drag b.png").to_string_lossy().replace('\\', "/");
    let url = if abs.starts_with('/') { format!("file://{}", abs.replace(' ', "%20")) } else { format!("file:///{}", abs.replace(' ', "%20")) };
    let (mut p, path) = page(
        &format!(
            r#"<body style="margin:0"><div id="src" draggable="true" style="width:60px;height:40px">file</div><div id="txt" draggable="true" style="position:absolute;left:100px;top:0;width:60px;height:40px">text</div>
           <script>
             window.log = [];
             const src = document.getElementById('src');
             src.addEventListener('dragstart', (e) => {{ e.dataTransfer.setFiles(['out/report.pdf', '{abs}']); }});
             src.addEventListener('dragend', (e) => log.push('end:' + e.dataTransfer.dropEffect));
             const txt = document.getElementById('txt');
             txt.addEventListener('dragstart', (e) => {{ e.dataTransfer.setData('text/uri-list', '{url}'); }});
           </script></body>"#
        ),
        "dragout",
    );
    let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
    let (sx, sy) = center(&p, "src");
    p.pointer_moved(sx, sy);
    p.press();
    p.pointer_moved(sx + 20.0, sy);
    assert!(p.dnd.is_some() && p.take_drag_out().is_none(), "still inside: an in-page drag");
    p.pointer_moved(-12.0, sy);
    assert!(p.dnd.is_none(), "the in-page drag ended");
    let files = p.take_drag_out().expect("handed out");
    assert_eq!(files.len(), 2);
    assert!(files[0].ends_with("out/report.pdf") && std::path::Path::new(&files[0]).is_absolute(), "{}", files[0]);
    assert_eq!(files[1], abs);
    assert_eq!(ev(&mut p, "log.join()"), "end:copy");
    p.release();
    // A uri-list of file: URLs counts too; a drag without files just stays in the page.
    let (tx, ty) = center(&p, "txt");
    p.pointer_moved(tx, ty);
    p.press();
    p.pointer_moved(tx + 20.0, ty);
    p.pointer_moved(-5.0, ty);
    assert_eq!(p.take_drag_out(), Some(vec![abs.clone()]));
    p.release();
    let _ = std::fs::remove_file(path);
}

#[test]
fn html5_drag_and_drop_fires_the_events_and_carries_data() {
    let (mut p, path) = page(
        r#"<body style="margin:0"><div id="src" draggable="true" style="width:60px;height:40px;background:#c33">drag me</div>
           <div id="ok" style="position:absolute;left:200px;top:0;width:80px;height:80px;background:#3a3"></div>
           <div id="no" style="position:absolute;left:300px;top:0;width:80px;height:80px;background:#999"></div>
           <div id="plain" style="width:60px;height:20px">not draggable</div>
           <script>
             window.log = [];
             const note = (e) => log.push(e.type + ':' + e.target.id);
             const src = document.getElementById('src'), ok = document.getElementById('ok'), no = document.getElementById('no');
             src.addEventListener('dragstart', (e) => { note(e); e.dataTransfer.setData('text/plain', 'payload'); e.dataTransfer.effectAllowed = 'copy'; });
             src.addEventListener('dragend', (e) => { note(e); log.push('effect=' + e.dataTransfer.dropEffect); });
             for (const t of ['dragenter', 'dragleave']) { ok.addEventListener(t, note); no.addEventListener(t, note); }
             ok.addEventListener('dragover', (e) => { e.preventDefault(); });
             ok.addEventListener('drop', (e) => { note(e); log.push('got=' + e.dataTransfer.getData('text/plain') + ',' + e.dataTransfer.types.join()); });
             no.addEventListener('dragover', () => {});
             no.addEventListener('drop', note);
           </script></body>"#,
        "dnd",
    );
    let ev = |p: &mut Page, js: &str| p.eval(js).0.unwrap();
    let (sx, sy) = center(&p, "src");
    // Over a plain element nothing starts, however far the pointer goes.
    let (px, py) = center(&p, "plain");
    p.pointer_moved(px, py);
    p.press();
    p.pointer_moved(px + 50.0, py + 50.0);
    p.release();
    assert_eq!(ev(&mut p, "log.join()"), "");
    // Accepted drop.
    p.pointer_moved(sx, sy);
    p.press();
    p.pointer_moved(sx + 2.0, sy);
    assert!(p.dnd.is_none(), "a tiny move is still a click");
    p.pointer_moved(sx + 20.0, sy);
    assert!(p.dnd.is_some());
    let (ox, oy) = center(&p, "ok");
    p.pointer_moved(ox, oy);
    assert_eq!(p.cursor(), nile_style::Cursor::Move);
    p.release();
    let log = ev(&mut p, "log.join()");
    assert!(log.starts_with("dragstart:src,dragenter:ok"), "{log}");
    assert!(log.contains("drop:ok") && log.contains("got=payload,text/plain"), "{log}");
    assert!(log.ends_with("dragend:src,effect=copy"), "{log}");
    assert!(p.dnd.is_none());
    // A zone that doesn't accept: no drop, and the drag ends with no effect.
    ev(&mut p, "log.length = 0");
    p.pointer_moved(sx, sy);
    p.press();
    p.pointer_moved(sx + 20.0, sy);
    let (nx, ny) = center(&p, "no");
    p.pointer_moved(nx, ny);
    assert_eq!(p.cursor(), nile_style::Cursor::NotAllowed);
    p.release();
    let log = ev(&mut p, "log.join()");
    assert!(!log.contains("drop:no") && log.contains("dragleave:no") && log.ends_with("effect=none"), "{log}");
    // Escape cancels.
    ev(&mut p, "log.length = 0");
    p.pointer_moved(sx, sy);
    p.press();
    p.pointer_moved(sx + 20.0, sy);
    p.pointer_moved(ox, oy);
    p.key("keydown", "Escape".to_string(), ModifiersState::default());
    p.release();
    let log = ev(&mut p, "log.join()");
    assert!(!log.contains("drop:ok") && log.ends_with("effect=none"), "{log}");
    let _ = std::fs::remove_file(path);
}

#[cfg(feature = "audio")]
#[test]
fn audio_elements_and_the_sciter_audio_class_play_and_end() {
    std::env::set_var("NILE_AUDIO", "off");
    let dir = std::env::temp_dir().join(format!("nile-audio-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("beep.wav"), silent_wav(250)).unwrap();
    let path = dir.join("index.html");
    std::fs::write(
        &path,
        r#"<body><audio id="a" src="beep.wav"></audio><script>
             window.log = [];
             const a = document.getElementById('a');
             for (const t of ['loadedmetadata', 'play', 'pause', 'ended']) a.addEventListener(t, () => log.push(t));
             window.dur = a.duration;
             a.volume = 0.5;
             a.play().then(() => log.push('started'));
             Audio.load(__DIR__ + '/beep.wav').then(async (s) => { log.push('loaded'); await s.play(); log.push('sciter-done'); });
             window.missing = 'pending';
             Audio.load('nope.wav').catch((e) => { window.missing = 'rejected'; });
           </script></body>"#,
    )
    .unwrap();
    let mut p = Page::load(path, (600.0, 300.0), 0.0);
    p.layout(600.0, 300.0);
    assert!((js(&mut p, "dur").parse::<f64>().unwrap() - 0.25).abs() < 0.01, "duration from the file");
    let until = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < until && !js(&mut p, "log.join()").contains("sciter-done") {
        std::thread::sleep(Duration::from_millis(30));
        p.tick();
    }
    for _ in 0..5 {
        std::thread::sleep(Duration::from_millis(10));
        p.tick();
    }
    let log = js(&mut p, "log.join()");
    // One `error:` line, so CI's failure annotation shows what happened.
    eprintln!("error: audio test log: {log}");
    for expect in ["loadedmetadata", "play", "started", "ended", "loaded", "sciter-done"] {
        assert!(log.contains(expect), "{expect} in {log}");
    }
    let ended_at = log.find("ended").unwrap();
    assert!(log.find("sciter-done").unwrap() > log.find("loaded").unwrap(), "{log}");
    assert!(ended_at > log.find("play").unwrap(), "{log}");
    assert_eq!(js(&mut p, "a.ended + ',' + a.paused + ',' + a.volume"), "true,true,0.5");
    assert_eq!(js(&mut p, "missing"), "rejected");
    assert_eq!(js(&mut p, "(new Audio(__DIR__ + '/beep.wav')) instanceof Audio && !(a instanceof HTMLElement === false)"), "true");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn canvas_draws_an_img_by_its_src() {
    // A 1x1 red PNG as a data URL.
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
    let (mut p, path) = page(
        &format!(
            r#"<body style="margin:0"><img id="i" src="data:image/png;base64,{png}" style="display:none"><canvas id="c" width="10" height="10"></canvas><script>
                 const c = document.getElementById('c').getContext('2d');
                 c.drawImage(document.getElementById('i'), 0, 0, 10, 10);
                 window.info = document.getElementById('i').naturalWidth + 'x' + document.getElementById('i').naturalHeight;
               </script></body>"#
        ),
        "canvas-img",
    );
    assert_eq!(js(&mut p, "info"), "1x1");
    let (reds, _) = count_colored(&mut p);
    assert!(reds >= 90, "the image fills the canvas: {reds}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn media_query_lists_fire_change_when_the_window_resizes() {
    let (mut p, path) = page(
        r#"<body><script>
             window.log = [];
             matchMedia('(min-width: 700px)').addEventListener('change', (e) => log.push(e.matches));
           </script></body>"#,
        "mql-resize",
    );
    p.layout(800.0, 300.0);
    p.run_timers();
    p.layout(500.0, 300.0);
    p.run_timers();
    assert_eq!(js(&mut p, "log.join(',')"), "true,false");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_accessibility_tree_names_roles_and_states() {
    use a11y::{A11yAction, A11yRole as R};
    let (mut p, path) = page(
        r#"<html><head><title>Sign in</title></head><body style="margin:0">
             <h2>Welcome back</h2>
             <label for="u">User name</label><input id="u" value="ada" required>
             <input id="pw" type="password" value="secret" aria-label="Password">
             <label><input id="rem" type="checkbox" checked> Remember me</label>
             <button id="go">Sign <b>in</b></button> <a id="lnk" href="/help">Help</a>
             <img alt="Company logo" src="x.png"><div style="display:none">hidden text</div><p aria-hidden="true">also hidden</p>
             <div role="alert">Careful</div>
             <select id="s" aria-label="Colour"><option>Red</option><option selected>Green</option></select>
             <input id="r" type="range" min="0" max="10" value="4" aria-label="Volume">
             <table><tr><th>Name</th><td>Ada</td></tr></table>
             <button id="off" disabled>Nope</button>
             <details open><summary>More</summary>content</details>
           </body></html>"#,
        "a11y",
    );
    let tree = p.a11y_tree();
    let find = |name: &str, role: R| tree.nodes.iter().find(|n| n.role == role && n.name == name).unwrap_or_else(|| panic!("{role:?} {name:?} in {:#?}", tree.nodes.iter().map(|n| (n.role, &n.name)).collect::<Vec<_>>()));
    assert_eq!(tree.nodes[0].role, R::Window);
    assert_eq!(tree.nodes[0].name, "Sign in");
    assert_eq!(find("Welcome back", R::Heading).level, 2);
    let user = find("User name", R::TextInput);
    assert_eq!((user.value.as_deref(), user.required, user.focusable), (Some("ada"), true, true));
    assert_eq!(find("Password", R::PasswordInput).value.as_deref(), Some("••••••"));
    assert_eq!(find("Remember me", R::CheckBox).checked, Some(Some(true)));
    let button = find("Sign in", R::Button);
    assert!(button.clickable && button.children.is_empty(), "a text-only button is named by its text");
    assert_eq!(find("Help", R::Link).link_target, "/help");
    find("Company logo", R::Image);
    find("Careful", R::StaticText);
    assert!(tree.nodes.iter().all(|n| !n.name.contains("hidden")), "hidden content is left out");
    assert_eq!(find("Colour", R::ComboBox).value.as_deref(), Some("Green"));
    assert_eq!(find("Volume", R::Slider).numeric, Some((4.0, 0.0, 10.0, 1.0)));
    find("Name", R::ColumnHeader);
    assert!(find("Nope", R::Button).disabled);
    let details = tree.nodes.iter().find(|n| n.role == R::Details).unwrap();
    assert_eq!(details.expanded, Some(true));
    // Every child is listed, bounds sit inside the window, and focus defaults to the root.
    for n in &tree.nodes {
        for c in &n.children {
            assert!(tree.nodes.iter().any(|x| x.id == *c), "child {c} of {:?} exists", n.role);
        }
    }
    assert_eq!(tree.focus, tree.root);
    let json = tree.to_json();
    assert!(json.starts_with(r#"{"root":1,"focus":1,"nodes":[{"id":1,"role":"Window","name":"Sign in""#) && json.contains(r#""role":"CheckBox""#) && json.contains(r#""checked":true"#), "{json}");
    // Requests from assistive technology: focus, type, click.
    let id = user.id;
    assert!(p.a11y_action(id, A11yAction::Focus));
    assert_eq!(p.a11y_tree().focus, id);
    assert!(p.a11y_action(id, A11yAction::SetValue("grace".into())));
    assert_eq!(js(&mut p, "document.getElementById('u').value"), "grace");
    let rem = find("Remember me", R::CheckBox).id;
    assert!(p.a11y_action(rem, A11yAction::Click));
    assert_eq!(p.a11y_tree().nodes.iter().find(|n| n.id == rem).unwrap().checked, Some(Some(false)));
    let vol = find("Volume", R::Slider).id;
    p.a11y_action(vol, A11yAction::Increment);
    assert_eq!(js(&mut p, "document.getElementById('r').value"), "5");
    let _ = std::fs::remove_file(path);
}

#[test]
fn hover_waits_for_scrolling_to_pause() {
    let (mut p, path) = page(
        r#"<style>.r { height: 40px } .r:hover { background: red }</style><body style="margin:0"><div class=r id=a></div><div class=r id=b></div><div class=r id=c></div>
           <div style="height:900px"></div></body>"#,
        "hover_scroll",
    );
    p.pointer_moved(20.0, 20.0);
    let hovered = |p: &Page| p.interaction().hover.map(|n| p.doc.borrow().node(n).attr("id").map(str::to_string));
    assert_eq!(hovered(&p), Some(Some("a".into())));
    p.wheel(0.0, -45.0); // row b is now under the pointer
    assert_eq!(hovered(&p), Some(Some("a".into())), "unchanged while scrolling");
    assert!(p.tick().1.is_some(), "a wake-up is scheduled");
    std::thread::sleep(Duration::from_millis(110));
    p.tick();
    assert_eq!(hovered(&p), Some(Some("b".into())), "settled once scrolling paused");
    // A press settles it at once.
    p.wheel(0.0, -40.0);
    assert_eq!(hovered(&p), Some(Some("b".into())));
    p.press();
    assert_eq!(hovered(&p), Some(Some("c".into())));
    p.release();
    let _ = std::fs::remove_file(path);
}
