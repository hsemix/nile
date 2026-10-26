//! Unit tests for the crate root; split out of lib.rs, which they used to follow.
use super::*;

/// Every element is a 100x20 box stacked at y = 20 * id, scrolled by 5.
struct Fake(RefCell<f32>);

impl Environment for Fake {
    fn bounding_rect(&self, node: NodeId) -> Option<[f32; 4]> {
        (node.0 != 0).then(|| [10.0, 20.0 * node.0 as f32 - *self.0.borrow(), 100.0, 20.0])
    }
    fn computed_style(&self, _: NodeId, p: &str) -> Option<String> {
        (p == "color").then(|| "rgb(1, 2, 3)".into())
    }
    fn viewport(&self) -> (f32, f32) {
        (640.0, 480.0)
    }
    fn scroll_y(&self) -> f32 {
        *self.0.borrow()
    }
    fn scroll_to(&self, y: f32) {
        *self.0.borrow_mut() = y;
    }
    fn interaction(&self) -> InteractionState {
        InteractionState::default()
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

fn host(html: &str) -> (ScriptHost, Rc<RefCell<Document>>) {
    let doc = Rc::new(RefCell::new(Document::parse(html)));
    (ScriptHost::new(doc.clone(), Rc::new(Fake(RefCell::new(0.0)))).unwrap(), doc)
}

#[test]
fn host_functions_eval_and_call_round_trip_json() {
    let (host, _) = host("");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    host.register_host_fn("add", move |args| {
        log.borrow_mut().push(args.to_string());
        let v: Vec<f64> = args.trim_matches(|c| c == '[' || c == ']').split(',').filter_map(|n| n.trim().parse().ok()).collect();
        format!("{}", v.iter().sum::<f64>())
    });
    host.register_host_fn("nothing", |_| String::new());
    assert_eq!(host.eval("host.add(1, 2, 3.5)").unwrap(), "6.5");
    assert_eq!(host.eval("host.call('add', 4, 5)").unwrap(), "9");
    assert_eq!(*seen.borrow(), ["[1,2,3.5]", "[4,5]"]);
    assert_eq!(host.eval("host.nothing()").unwrap(), "", "empty result is undefined");
    assert_eq!(host.eval("[host.has('add'), host.has('x')]").unwrap(), "[true,false]");
    assert!(host.eval("host.missing(1)").unwrap_err().contains("not registered"));
    // eval: strings raw, objects as JSON.
    assert_eq!(host.eval("'hi'").unwrap(), "hi");
    assert_eq!(host.eval("({a: [1, 2]})").unwrap(), r#"{"a":[1,2]}"#);
    assert_eq!(host.eval("undefined").unwrap(), "");
    assert!(host.eval("(").is_err());
    // The host calls into the page.
    host.run("globalThis.app = { greet(n, x) { return { hello: n, x }; } }; function twice(n) { return n * 2 }").unwrap();
    assert_eq!(host.call("twice", "[21]").unwrap(), "42");
    assert_eq!(host.call("app.greet", r#"["Ada", [1]]"#).unwrap(), r#"{"hello":"Ada","x":[1]}"#);
    assert!(host.call("nope", "[]").unwrap_err().contains("not a function"));
}

#[test]
fn console_log_and_errors() {
    let (host, _) = host("");
    host.run("console.log('sum', 1 + 2)").unwrap();
    assert_eq!(host.console_output(), vec!["sum 3".to_string()]);
    assert!(host.run("throw new Error('boom')").unwrap_err().contains("boom"));
}

#[test]
fn queries_and_mutation_reach_the_document() {
    let (host, doc) = host(r#"<ul id="l"><li class="a">one</li><li>two</li></ul>"#);
    let v0 = doc.borrow().version;
    host.run(
        r#"
        const ul = document.getElementById('l');
        console.log(ul.children.length, ul.querySelectorAll('li.a').length, ul.firstElementChild.textContent);
        const li = document.createElement('LI');
        li.textContent = 'three';
        li.classList.add('x', 'y');
        li.style.backgroundColor = 'red';
        li.dataset.fooBar = '1';
        ul.appendChild(li);
        console.log(ul.lastElementChild === li, li.tagName, li.className, li.getAttribute('style'),
                    li.getAttribute('data-foo-bar'), li.matches('ul > li.x'), li.closest('#l') === ul);
        console.log(ul.innerHTML);
        "#,
    )
    .unwrap();
    let out = host.console_output();
    assert_eq!(out[0], "2 1 one");
    assert_eq!(out[1], "true LI x y background-color: red 1 true true");
    assert_eq!(
        out[2],
        r#"<li class="a">one</li><li>two</li><li class="x y" style="background-color: red" data-foo-bar="1">three</li>"#
    );
    assert!(doc.borrow().version > v0);
}

#[test]
fn web_platform_helpers_work() {
    let (host, _) = host(r#"<ul id="l"><li>a</li></ul>"#);
    host.run(
        r#"
        console.log(btoa('Hello, World!'), atob('SGVsbG8sIFdvcmxkIQ=='), btoa('ab'), atob('YWI'));
        const bytes = new TextEncoder().encode('héllo €😀');
        console.log(bytes.length, new TextDecoder().decode(bytes));
        const u = new URL('../x/y.html?a=1&b=two%20words#top', 'https://user@Example.com:8080/p/q/r.html');
        console.log(u.href, u.origin, u.pathname, u.searchParams.get('b'), u.hash, u.host);
        u.searchParams.set('c', 'a b'); u.searchParams.append('a', '2');
        console.log(u.search, u.searchParams.getAll('a').join('|'));
        console.log(new URLSearchParams({ q: 'x y', n: 1 }).toString());
        const o = { d: new Date(5), m: new Map([[1, { z: [1, 2] }]]), list: [1, { k: 2 }] };
        o.self = o;
        const c = structuredClone(o);
        console.log(c !== o, c.self === c, c.d.getTime(), c.m.get(1).z.length, c.list[1].k);
        localStorage.setItem('k', 'v'); localStorage.n = 5;
        console.log(localStorage.getItem('k'), localStorage.n, localStorage.length, sessionStorage.length);
        console.log(matchMedia('(min-width: 1px)').matches, matchMedia('(max-width: 1px)').matches, matchMedia('(prefers-color-scheme: light)').matches);
        window.seen = [];
        const ul = document.getElementById('l');
        new MutationObserver((recs) => { for (const r of recs) seen.push(r.type + ':' + (r.attributeName || r.addedNodes.length + '/' + r.removedNodes.length)); })
          .observe(ul, { childList: true, attributes: true, subtree: true });
        const li = document.createElement('li');
        ul.appendChild(li);
        ul.setAttribute('data-x', '1');
        li.setAttribute('class', 'new');
        ul.removeChild(li);
        "#,
    )
    .unwrap();
    host.run("console.log(seen.join(','))").unwrap();
    let out = host.console_output();
    assert_eq!(out[0], "SGVsbG8sIFdvcmxkIQ== Hello, World! YWI= ab");
    assert_eq!(out[1], "14 héllo €😀");
    assert_eq!(out[2], "https://user@example.com:8080/p/x/y.html?a=1&b=two%20words#top https://example.com:8080 /p/x/y.html two words #top example.com:8080");
    assert_eq!(out[3], "?a=1&b=two+words&c=a+b&a=2 1|2");
    assert_eq!(out[4], "q=x+y&n=1");
    assert_eq!(out[5], "true true 5 2 2");
    assert_eq!(out[6], "v 5 2 0");
    assert_eq!(out[7], "true false true");
    assert_eq!(out[8], "childList:1/0,attributes:data-x,attributes:class,childList:0/1");
}

#[test]
fn fetch_and_xhr_load_local_files_and_data_urls() {
    let dir = std::env::temp_dir().join(format!("nile-fetch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("d.json"), r#"{"n":7,"s":"héllo"}"#).unwrap();
    let (host, _) = host("<p></p>");
    host.set_fetcher(Arc::new(LocalFetcher::new(&dir)));
    host.run(
        r#"
        window.out = [];
        fetch('d.json').then(r => { out.push(r.status + ' ' + r.ok + ' ' + r.headers.get('content-type')); return r.json(); })
          .then(j => out.push(j.n + j.s));
        fetch('nope.txt').then(r => out.push(r.status + ' ' + r.ok));
        fetch('data:text/plain;base64,SGk=').then(r => r.text()).then(t => out.push(t));
        fetch('https://example.com/').catch(e => out.push('rejected'));
        const x = new XMLHttpRequest();
        x.open('GET', 'd.json'); x.responseType = 'json';
        x.onload = () => out.push('xhr ' + x.status + ' ' + x.response.n);
        x.send();
        const ac = new AbortController();
        fetch('d.json', { signal: ac.signal }).catch(e => out.push('aborted'));
        ac.abort();
        "#,
    )
    .unwrap();
    for _ in 0..200 {
        host.tick();
        if !host.fetches_pending() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    host.tick();
    host.run("console.log(out.slice().sort().join('|'))").unwrap();
    let out = host.console_output();
    assert_eq!(out[0], "200 true application/json|404 false|7héllo|Hi|aborted|rejected|xhr 200 7");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn local_storage_persists_to_the_chosen_file() {
    let file = std::env::temp_dir().join(format!("nile-storage-{}", std::process::id())).join("ls.json");
    let _ = std::fs::remove_file(&file);
    {
        let (host, _) = host("<p></p>");
        host.set_storage_file(&file);
        host.run("localStorage.setItem('name', 'Nile'); localStorage.n = 2; localStorage.removeItem('n'); localStorage.k = 'é\"x'").unwrap();
    } // dropping flushes
    assert_eq!(std::fs::read_to_string(&file).unwrap().replace(' ', ""), r#"{"name":"Nile","k":"é\"x"}"#);
    let (host, _) = host("<p></p>");
    host.set_storage_file(&file);
    host.run("console.log(localStorage.name, localStorage.length, localStorage.getItem('k'))").unwrap();
    assert_eq!(host.console_output(), vec!["Nile 2 é\"x"]);
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn sciter_and_env_modules_load() {
    let (host, _) = host(r#"<p id="p" class="a">x</p>"#);
    host.run_module(
        "page.html#module1",
        r##"
        import { $, $$, md5, crc32, toBase64, fromBase64, uuid, encode, decode, parseValue, devicePixels, VERSION } from "@nile";
        import * as env from "@env";
        import sctr, { on } from "@sciter";
        console.log($("#p").className, $$("p").length, typeof VERSION, typeof on, sctr.crc32 === crc32);
        console.log(md5("hello"), crc32("hello"), toBase64(encode("Nile")), decode(fromBase64("TmlsZQ==")), uuid().length);
        console.log(JSON.stringify(parseValue("{foo-bar: zap; n: 0xFF, list: [1, 2n], ok: true; s: 'q'}", 0), (k, v) => typeof v === "bigint" ? v + "n" : v));
        console.log(devicePixels("1in"), devicePixels(10));
        console.log(["OSX", "Windows", "Linux"].includes(env.PLATFORM) || env.PLATFORM, env.DEVICE, env.language().length, env.path("home", "x").endsWith("x"), Array.isArray(env.arguments()), env.drives().length > 0);
        env.variable("NILE_MODULE_TEST", "7");
        console.log(env.variable("NILE_MODULE_TEST"));
        env.variable("NILE_MODULE_TEST", null);
        console.log(JSON.stringify(env.variable("NILE_MODULE_TEST")));
        "##,
    )
    .unwrap();
    let out = host.console_output();
    assert_eq!(out[0], "a 1 string function true");
    assert_eq!(out[1], "5d41402abc4b2a76b9719d911017c592 907060870 TmlsZQ== Nile 36");
    assert_eq!(out[2], r#"{"foo-bar":"zap","n":255,"list":[1,"2n"],"ok":true,"s":"q"}"#);
    assert_eq!(out[3], "96 10");
    assert!(out[4].starts_with("true desktop 2 true true true"), "{}", out[4]);
    assert_eq!(out[5], "7");
    assert_eq!(out[6], "\"\"");
}

#[test]
fn missing_attributes_are_null() {
    let (host, _) = host("<p id='p'></p>");
    host.run("const p = document.getElementById('p'); console.log(p.getAttribute('nope') === null, p.hasAttribute('id'), p.hasAttribute('nope'))").unwrap();
    assert_eq!(host.console_output(), vec!["true true false"]);
}

#[test]
fn selector_errors_throw() {
    let (host, _) = host("<p></p>");
    host.run("try { document.querySelector('p::first-line') } catch (e) { console.log(String(e).includes('SyntaxError')) }")
        .unwrap();
    assert_eq!(host.console_output(), vec!["true"]);
}

#[test]
fn events_capture_and_bubble_in_order() {
    let (host, doc) = host(r#"<div id="outer"><button id="inner">x</button></div>"#);
    host.run(
        r#"
        const log = [];
        const outer = document.getElementById('outer'), inner = document.getElementById('inner');
        outer.addEventListener('click', () => log.push('outer-capture'), true);
        outer.addEventListener('click', () => log.push('outer-bubble'));
        inner.addEventListener('click', (e) => { log.push('inner:' + e.target.id + ':' + e.currentTarget.id); });
        document.addEventListener('click', () => log.push('document'));
        window.addEventListener('click', () => log.push('window'));
        inner.onclick = () => log.push('onclick');
        globalThis.__log = log;
        "#,
    )
    .unwrap();
    let inner = doc.borrow().find(|n| n.attr("id") == Some("inner")).unwrap();
    let ev = UiEvent::Mouse { kind: "click", x: 1.0, y: 2.0, button: 0, mods: 0 };
    assert!(host.dispatch(inner, &ev));
    host.run("console.log(__log.join(','))").unwrap();
    assert_eq!(
        host.console_output()[0],
        "outer-capture,onclick,inner:inner:inner,outer-bubble,document,window"
    );
}

#[test]
fn stop_propagation_prevent_default_and_once() {
    let (host, doc) = host(r##"<div id="o"><a id="i" href="#">x</a></div>"##);
    host.run(
        r#"
        const out = [];
        const o = document.getElementById('o'), i = document.getElementById('i');
        o.addEventListener('click', () => out.push('o'));
        i.addEventListener('click', (e) => { e.preventDefault(); e.stopPropagation(); out.push('i'); }, { once: true });
        globalThis.__out = out;
        "#,
    )
    .unwrap();
    let i = doc.borrow().find(|n| n.attr("id") == Some("i")).unwrap();
    let ev = UiEvent::Mouse { kind: "click", x: 0.0, y: 0.0, button: 0, mods: 0 };
    assert!(!host.dispatch(i, &ev), "preventDefault reports false");
    assert!(host.dispatch(i, &ev), "once listener is gone; click bubbles to o");
    host.run("console.log(__out.join(','))").unwrap();
    assert_eq!(host.console_output()[0], "i,o");
}

#[test]
fn attribute_handlers_and_key_events() {
    let (host, doc) = host(r#"<input id="t" onkeydown="console.log('key:' + event.key + event.ctrlKey)">"#);
    let t = doc.borrow().find(|n| n.attr("id") == Some("t")).unwrap();
    host.dispatch(t, &UiEvent::Key { kind: "keydown", key: "a".into(), ctrl: true, shift: false, alt: false, meta: false });
    assert_eq!(host.console_output(), vec!["key:atrue"]);
}

#[test]
fn timers_and_promises() {
    let (host, _) = host("");
    host.run(
        r#"
        setTimeout((a) => console.log('t', a), 0, 'x');
        const id = setInterval(() => console.log('i'), 1);
        Promise.resolve().then(() => console.log('micro'));
        clearTimeout(setTimeout(() => console.log('never'), 0));
        "#,
    )
    .unwrap();
    assert_eq!(host.console_output(), vec!["micro"]);
    std::thread::sleep(Duration::from_millis(5));
    assert!(host.tick().is_some(), "the interval keeps the loop alive");
    let out = host.console_output();
    assert!(out.contains(&"t x".to_string()) && out.contains(&"i".to_string()));
    assert!(!out.contains(&"never".to_string()));
}

#[test]
fn layout_queries_and_scrolling() {
    let (host, doc) = host(r#"<div id="a"><p id="b" style="border: 2px solid red">x</p></div>"#);
    let b = doc.borrow().find(|n| n.attr("id") == Some("b")).unwrap().0;
    host.run(
        r#"
        const el = document.getElementById('b');
        const r = el.getBoundingClientRect();
        console.log(r.left, r.top, r.width, r.height, r.right, r.bottom, el.offsetWidth, el.offsetHeight);
        console.log(getComputedStyle(el).color, getComputedStyle(el).getPropertyValue('color'), getComputedStyle(el).fontSize === '');
        console.log(innerWidth, innerHeight, scrollY);
        scrollTo(0, 30);
        console.log(scrollY, el.getBoundingClientRect().top, document.documentElement.scrollTop);
        console.log(document.createElement('div').getBoundingClientRect().width);
        "#,
    )
    .unwrap();
    let out = host.console_output();
    assert_eq!(out[0], format!("10 {} 100 20 110 {} 100 20", 20 * b, 20 * b + 20));
    assert_eq!(out[1], "rgb(1, 2, 3) rgb(1, 2, 3) true");
    assert_eq!(out[2], "640 480 0");
    assert_eq!(out[3], format!("30 {} 30", 20 * b - 30));
    assert_eq!(out[4], "100");
}

#[test]
fn jsx_builds_dom_in_classic_scripts() {
    let (host, doc) = host(r#"<div id="root"></div>"#);
    host.run(
        r#"
        const Card = (props, kids) => <section class="card" data-n={props.n}>{kids}</section>;
        let clicked = 0;
        const items = ['a', 'b'];
        const view = (
          <Card n={2}>
            <h1 style={{ color: 'red', fontSize: '20px' }} onclick={() => clicked++}>Title &amp; more</h1>
            <ul class={['x', false, 'y']}>{items.map((t, i) => <li key={i} hidden={i === 1}>{t}</li>)}</ul>
            <>frag</>
            {null}{false}{3}
          </Card>
        );
        document.getElementById('root').append(view);
        document.querySelector('h1').click();
        console.log(clicked, document.getElementById('root').innerHTML);
        "#,
    )
    .unwrap();
    let _ = doc;
    assert_eq!(
        host.console_output()[0],
        r#"1 <section class="card" data-n="2"><h1 style="color: red; font-size: 20px">Title &amp; more</h1><ul class="x y"><li>a</li><li hidden="">b</li></ul>frag3</section>"#
    );
}

#[test]
fn modules_import_relative_files_with_jsx() {
    let dir = std::env::temp_dir().join(format!("nile-mod-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/util.js"), "export const twice = (n) => n * 2;\nexport default 'dflt';").unwrap();
    std::fs::write(
        dir.join("lib/view.jsx"),
        "import { twice } from './util.js';\nexport const View = ({ n }) => <b>{twice(n)}</b>;",
    )
    .unwrap();
    let (host, _) = host(r#"<div id="root"></div>"#);
    let page = dir.join("page.html");
    host.run_module(
        &format!("{}#module0", page.display()),
        r#"
        import dflt, { twice } from './lib/util.js';
        import { View } from './lib/view.jsx';
        document.getElementById('root').append(<View n={21} />);
        console.log(dflt, twice(4), document.getElementById('root').innerHTML);
        "#,
    )
    .unwrap();
    assert_eq!(host.console_output(), vec!["dflt 8 <b>42</b>"]);
    // Errors surface as messages.
    let err = host.run_module(&format!("{}#module1", page.display()), "import './missing.js';").unwrap_err();
    assert!(err.contains("missing.js"), "{err}");
    let err = host.run_module(&format!("{}#module2", page.display()), "import 'bare';").unwrap_err();
    assert!(err.contains("bare"), "{err}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn jsx_compiles_to_vnodes_like_sciter() {
    let (host, _) = host("");
    host.run(
        r#"
        const v = <h1 id="hw">Hello</h1>;
        console.log(Array.isArray(v), v[0], JSON.stringify(v[1]), JSON.stringify(v[2]), JSON.stringify(<br/>));
        "#,
    )
    .unwrap();
    assert_eq!(host.console_output(), vec![r#"true h1 {"id":"hw"} ["Hello"] ["br",null,null]"#]);
}

#[test]
fn sciter_selectors_and_delegation() {
    let (host, _) = host(r#"<ul id="l"><li class="a"><b>1</b></li><li class="a">2</li></ul>"#);
    host.run(
        r#"
        const l = $('#l');
        console.log($$('li').length, l.$$('.a').length, l.$('b').textContent, l.children[1].elementIndex);

        // this = the element handler is attached to; second arg = matched descendant.
        const seen = [];
        l.on('click', 'li', function (evt, li) { seen.push(this.id + ':' + li.textContent + ':' + (li === evt.target.closest('li'))); });
        $$('li')[0].$('b').click();     // bubbles from <b>, delegated to the li
        l.click();                       // not inside an li: ignored

        // namespaces, off by name / namespace / function, once, capture
        const hits = [];
        const h = () => hits.push('h');
        l.on('click.one', h).on('click.two', () => hits.push('two')).once('click', () => hits.push('once'));
        l.click(); l.click();
        l.off('.two'); l.click();
        l.off(h); l.click();
        l.off('click'); l.click();
        const order = [];
        l.on('^click', () => order.push('capture'));
        l.on('click', () => order.push('bubble'));
        $$('li')[1].click();
        console.log(seen.join(','));
        console.log(hits.join(','));
        console.log(order.join(','));
        "#,
    )
    .unwrap();
    let out = host.console_output();
    assert_eq!(out[0], "2 2 1 1");
    assert_eq!(out[1], "l:1:true");
    // 1st click: h,two,once; 2nd: h,two; after off('.two'): h; after off(h): nothing; then off('click').
    assert_eq!(out[2], "h,two,once,h,two,h");
    assert_eq!(out[3], "capture,bubble");
}

#[test]
fn element_state_flags_and_attributes() {
    let (host, _) = host(
        r#"<style>#i:checked { color: red } </style><input id="i" type="checkbox" title="t"><p id="p"></p>"#,
    );
    host.run(
        r#"
        const i = $('#i');
        console.log(i.state.checked, i.state.disabled, i.state.hover, 'checked' in i.state);
        i.state.checked = true;
        console.log(i.state.checked, i.hasAttribute('checked'), i.matches(':checked'), $$('input:checked').length);
        i.state.checked = false;
        console.log($$(':checked').length);
        const names = [...i.attributes].map((a) => a.name + '=' + a.value).join(',');
        i.attributes.title = 'new';
        console.log(names, i.getAttribute('title'), i.attributes.length, i.attributes.type, i.attributes.nope);
        console.log(i.value, $('#p').nodeIndex, $('#p').insertAfter(document.createElement('hr'), $('#p')).tagName, $('#p').insert);
        "#,
    )
    .unwrap();
    let out = host.console_output();
    assert_eq!(out[0], "false false false true");
    assert_eq!(out[1], "true false true 1");
    assert_eq!(out[2], "0");
    assert_eq!(out[3], "id=i,type=checkbox,title=t new 3 checkbox undefined");
    assert_eq!(out[4], "on 1 HR undefined");
}

#[test]
fn content_prepend_and_patch() {
    let (host, doc) = host(r#"<div id="root"><p id="keep" class="old" title="gone">old</p>stray</div>"#);
    host.run(
        r#"
        const root = $('#root');
        const keep = $('#keep');
        const ids = [];
        root.patch(
          <div>
            <p id="keep" class="new" onclick={() => ids.push('clicked')}>fresh</p>
            <ul><li key="a">A</li><li key="b">B</li></ul>
            {['x', 'y']}
          </div>
        );
        console.log(keep === $('#keep'), keep.className, keep.hasAttribute('title'), root.innerHTML);
        keep.click();
        const a = $('li');
        // Re-patch with the items swapped: keyed DOM is reused, not rebuilt.
        root.patch(<div><p id="keep" class="new">fresh</p><ul><li key="b">B</li><li key="a">A</li></ul></div>);
        console.log(ids.join(), $('li') !== a, $$('li')[1] === a, root.innerHTML);
        root.prepend(<hr/>, 'text');
        console.log(root.firstChild.tagName, root.childNodes[1].data, root.children.length);
        root.content(<i>only</i>);
        console.log(root.innerHTML);
        "#,
    )
    .unwrap();
    let _ = doc;
    let out = host.console_output();
    assert_eq!(
        out[0],
        r#"true new false <p id="keep" class="new">fresh</p><ul><li>A</li><li>B</li></ul>xy"#
    );
    assert_eq!(out[1], r#"clicked true true <p id="keep" class="new">fresh</p><ul><li>B</li><li>A</li></ul>"#);
    assert_eq!(out[2], "HR text 3");
    assert_eq!(out[3], "<i>only</i>");
}

#[test]
fn form_control_properties() {
    let (host, _) = host(
        r#"<input id="t" value="hi" placeholder="p"><input id="c" type="checkbox" value="v"><input id="d" disabled><button id="b">x</button><div id="div"></div>"#,
    );
    host.run(
        r#"
        const t = $('#t'), c = $('#c'), d = $('#d');
        console.log(t.value, t.placeholder, t.type, c.type, c.checked, d.disabled, $('#b').disabled, $('#div').value);
        t.value = 'new'; c.checked = true; d.disabled = false; $('#b').disabled = true; t.name = 'n';
        console.log(t.getAttribute('value'), c.checked, c.state.checked, d.disabled, d.hasAttribute('disabled'), $('#b').disabled, t.name);
        c.checked = false;
        const div = $('#div'); div.value = 5;
        console.log(c.checked, div.value, document.activeElement === null || document.activeElement === document.body);
        "#,
    )
    .unwrap();
    let out = host.console_output();
    assert_eq!(out[0], "hi p text checkbox false true false undefined");
    assert_eq!(out[1], "new true true false false true n");
    assert_eq!(out[2], "false 5 true");
}

#[test]
fn select_and_option_properties() {
    let (host, _) = host(
        r#"<select id="s"><option value="r">Red</option><option disabled>Gone</option><option selected> Big   Blue </option></select>
           <select id="d"><option disabled>x</option><option value="y">Why</option></select>"#,
    );
    host.run(
        r#"
        const s = $('#s'), d = $('#d');
        console.log(s.options.length, s.length, s.selectedIndex, s.value, s.options[2].text, s.options[2].selected, s.options[0].selected);
        s.value = 'r';
        console.log(s.selectedIndex, s.options[0].selected, s.options[2].selected, s.options[2].hasAttribute('selected'), s.value);
        s.options[2].selected = true;
        console.log(s.selectedIndex, s.value);
        s.selectedIndex = 0;
        console.log(s.value, s.options[1].disabled, d.selectedIndex, d.value, $('#s').options[0].label);
        s.value = 'nope';
        console.log(s.value === 'r' || s.selectedIndex, document.createElement('p').options);
        "#,
    )
    .unwrap();
    let out = host.console_output();
    assert_eq!(out[0], "3 3 2 Big Blue Big Blue true false");
    assert_eq!(out[1], "0 true false false r");
    assert_eq!(out[2], "2 Big Blue");
    assert_eq!(out[3], "r true 1 y Red");
    // An unknown value selects nothing; the first enabled option shows.
    assert_eq!(out[4], "true undefined");
}

#[test]
fn lifecycle_events_fire() {
    let (host, _) = host("<p></p>");
    host.run(
        "document.addEventListener('DOMContentLoaded', () => console.log('dcl ' + document.readyState));\
         window.addEventListener('load', () => console.log('load ' + document.readyState));",
    )
    .unwrap();
    host.fire_load();
    assert_eq!(host.console_output(), vec!["dcl interactive", "load complete"]);
}
