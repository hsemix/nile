//! JavaScript runtime (QuickJS via rquickjs) with a web-style DOM.
//!
//! Rust exposes a small set of native functions over the arena DOM
//! (`__nile`, see [`install_natives`]); `prelude.js` builds the familiar
//! classes on top (`document`, `Element`, `classList`, `style`, events with
//! capture/bubble, timers). Nodes are identified by arena id, so JS wrappers
//! are cheap and detached nodes simply stay in the arena.
//!
//! Layout-dependent APIs (`getBoundingClientRect`, `getComputedStyle`, `offset*`,
//! window size and scrolling) go through [`Environment`], which the embedder
//! implements so it can flush pending style/layout work first.
//!
//! Not yet: `fetch`, modules/JSX in the loader, Sciter's own DOM extensions.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nile_dom::{Document, NodeId, NodeKind};
use nile_style::{InteractionState, Query};
use rquickjs::{
    function::Func,
    loader::{ImportAttributes, Loader, Resolver},
    module::Declared,
    CatchResultExt, Context, Ctx, Exception, Function, Module, Object, Runtime,
};

mod jsx;

const PRELUDE: &str = include_str!("prelude.js");

/// What scripts can ask of the embedder: layout results and the viewport.
/// Implementations must bring styles/layout up to date with the document
/// before answering, since scripts expect to read what they just wrote.
pub trait Environment {
    /// Border box `[x, y, width, height]` in viewport coordinates, or `None`
    /// if the node has no box (`display: none`, detached, ...).
    fn bounding_rect(&self, node: NodeId) -> Option<[f32; 4]>;
    /// The computed value of a CSS property as text, if known.
    fn computed_style(&self, node: NodeId, property: &str) -> Option<String>;
    /// Viewport size in CSS px.
    fn viewport(&self) -> (f32, f32);
    fn scroll_y(&self) -> f32;
    fn scroll_to(&self, y: f32);
    /// Hover/active/focus, for `:hover` etc. in selectors scripts evaluate.
    fn interaction(&self) -> InteractionState;
}

/// Resolves `./` and `../` imports against the importing module's file.
struct FileResolver;

impl Resolver for FileResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        let path = std::path::Path::new(name);
        if name.starts_with("./") || name.starts_with("../") {
            // Inline modules are named `page.html#moduleN`; imports resolve from the page's folder.
            let base_file = base.split('#').next().unwrap_or(base);
            let dir = std::path::Path::new(base_file).parent().unwrap_or(std::path::Path::new(""));
            Ok(normalize(&dir.join(path)))
        } else if path.is_absolute() {
            Ok(normalize(path))
        } else {
            Err(rquickjs::Error::new_resolving(base, name))
        }
    }
}

/// Lexically remove `.` and `..` components.
fn normalize(path: &std::path::Path) -> String {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Loads module files from disk, transforming JSX.
struct FileLoader;

impl Loader for FileLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, Declared>> {
        let source = std::fs::read_to_string(name).map_err(|_| rquickjs::Error::new_loading(name))?;
        let source = jsx::transform(&source).unwrap_or(source);
        Module::declare(ctx.clone(), name, source)
    }
}

/// An input event for [`ScriptHost::dispatch`].
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    /// `click`, `mousedown`, `mouseup`, `mousemove`, `mouseover`, `mouseout`.
    Mouse { kind: &'static str, x: f32, y: f32, button: i32 },
    /// `keydown`, `keyup`.
    Key { kind: &'static str, key: String, ctrl: bool, shift: bool, alt: bool, meta: bool },
    /// `focus`, `blur`, ... with no payload.
    Plain(&'static str),
}

pub struct ScriptHost {
    runtime: Runtime,
    context: Context,
    log: Arc<Mutex<Vec<String>>>,
    start: Instant,
}

fn node(id: i32) -> Option<NodeId> {
    usize::try_from(id).ok().map(NodeId)
}

fn id_out(id: Option<NodeId>) -> i32 {
    id.map_or(-1, |n| n.0 as i32)
}

fn install_natives(
    ctx: &Ctx<'_>,
    doc: Rc<RefCell<Document>>,
    log: Arc<Mutex<Vec<String>>>,
    start: Instant,
    env: Rc<dyn Environment>,
) -> rquickjs::Result<()> {
    let n = Object::new(ctx.clone())?;

    macro_rules! native {
        ($name:literal, $f:expr) => {
            n.set($name, Func::from($f))?;
        };
    }
    // Clone `doc` into each closure under the name `d`.
    macro_rules! with_doc {
        ($d:ident => $f:expr) => {{
            let $d = doc.clone();
            $f
        }};
    }

    native!("root", || 0i32);
    native!("nodeType", with_doc!(d => move |id: i32| -> i32 {
        let d = d.borrow();
        match node(id).filter(|n| n.0 < d.nodes.len()).map(|n| &d.node(n).kind) {
            Some(NodeKind::Element { .. }) => 1,
            Some(NodeKind::Text(_)) => 3,
            _ => 9,
        }
    }));
    native!("tag", with_doc!(d => move |id: i32| -> Option<String> {
        let d = d.borrow();
        node(id).and_then(|n| d.node(n).tag().map(str::to_string))
    }));
    native!("parent", with_doc!(d => move |id: i32| -> i32 {
        let d = d.borrow();
        id_out(node(id).and_then(|n| d.node(n).parent))
    }));
    native!("childNodes", with_doc!(d => move |id: i32| -> Vec<i32> {
        let d = d.borrow();
        node(id).map(|n| d.node(n).children.iter().map(|c| c.0 as i32).collect()).unwrap_or_default()
    }));
    native!("docElement", with_doc!(d => move || -> i32 {
        let d = d.borrow();
        id_out(d.node(Document::ROOT).children.iter().copied().find(|c| d.node(*c).tag() == Some("html")))
    }));
    native!("bodyEl", with_doc!(d => move || -> i32 {
        let d = d.borrow();
        id_out(d.body().filter(|b| d.is_attached(*b)))
    }));
    native!("headEl", with_doc!(d => move || -> i32 {
        let d = d.borrow();
        id_out(d.descendants(Document::ROOT).into_iter().find(|c| d.node(*c).tag() == Some("head")))
    }));
    native!("getAttr", with_doc!(d => move |id: i32, name: String| -> Option<String> {
        let d = d.borrow();
        node(id).and_then(|n| d.node(n).attr(&name.to_ascii_lowercase()).map(str::to_string))
    }));
    native!("setAttr", with_doc!(d => move |id: i32, name: String, value: String| {
        if let Some(n) = node(id) {
            d.borrow_mut().set_attr(n, &name, &value);
        }
    }));
    native!("removeAttr", with_doc!(d => move |id: i32, name: String| {
        if let Some(n) = node(id) {
            d.borrow_mut().remove_attr(n, &name);
        }
    }));
    native!("attrNames", with_doc!(d => move |id: i32| -> Vec<String> {
        let d = d.borrow();
        match node(id).map(|n| &d.node(n).kind) {
            Some(NodeKind::Element { attrs, .. }) => attrs.iter().map(|(k, _)| k.clone()).collect(),
            _ => Vec::new(),
        }
    }));
    native!("text", with_doc!(d => move |id: i32| -> String {
        let d = d.borrow();
        node(id).map(|n| d.text_content(n)).unwrap_or_default()
    }));
    native!("setText", with_doc!(d => move |id: i32, text: String| {
        if let Some(n) = node(id) {
            d.borrow_mut().set_text_content(n, &text);
        }
    }));
    native!("innerHTML", with_doc!(d => move |id: i32| -> String {
        let d = d.borrow();
        node(id).map(|n| d.inner_html(n)).unwrap_or_default()
    }));
    native!("setInnerHTML", with_doc!(d => move |id: i32, html: String| {
        if let Some(n) = node(id) {
            d.borrow_mut().set_inner_html(n, &html);
        }
    }));
    native!("createElement", with_doc!(d => move |tag: String| -> i32 {
        d.borrow_mut().create_element(&tag).0 as i32
    }));
    native!("createText", with_doc!(d => move |text: String| -> i32 {
        d.borrow_mut().create_text(&text).0 as i32
    }));
    native!("append", with_doc!(d => move |ctx: Ctx<'_>, parent: i32, child: i32| -> rquickjs::Result<()> {
        match (node(parent), node(child)) {
            (Some(p), Some(c)) => d
                .borrow_mut()
                .append_child(p, c)
                .map_err(|e| Exception::throw_message(&ctx, &format!("HierarchyRequestError: {e:?}"))),
            _ => Err(Exception::throw_message(&ctx, "invalid node")),
        }
    }));
    native!("insertBefore", with_doc!(d => move |ctx: Ctx<'_>, parent: i32, child: i32, before: i32| -> rquickjs::Result<()> {
        match (node(parent), node(child)) {
            (Some(p), Some(c)) => d
                .borrow_mut()
                .insert_before(p, c, node(before))
                .map_err(|e| Exception::throw_message(&ctx, &format!("HierarchyRequestError: {e:?}"))),
            _ => Err(Exception::throw_message(&ctx, "invalid node")),
        }
    }));
    native!("remove", with_doc!(d => move |id: i32| {
        if let Some(n) = node(id) {
            d.borrow_mut().detach(n);
        }
    }));
    native!("getState", with_doc!(d => move |id: i32, name: String| -> bool {
        node(id).is_some_and(|n| d.borrow().has_state(n, &name))
    }));
    native!("setState", with_doc!(d => move |id: i32, name: String, on: bool| {
        if let Some(n) = node(id) {
            d.borrow_mut().set_state(n, &name, on);
        }
    }));
    native!("byId", with_doc!(d => move |id: String| -> i32 {
        let d = d.borrow();
        id_out(d.descendants(Document::ROOT).into_iter().find(|n| d.node(*n).attr("id") == Some(id.as_str())))
    }));
    native!("select", with_doc!(d => { let env = env.clone(); move |ctx: Ctx<'_>, root: i32, css: String| -> rquickjs::Result<Vec<i32>> {
        let q = Query::parse(&css).map_err(|e| Exception::throw_message(&ctx, &format!("SyntaxError: {e}")))?;
        let state = env.interaction();
        let d = d.borrow();
        Ok(node(root).map(|r| q.select_all_with(&d, r, &state).into_iter().map(|n| n.0 as i32).collect()).unwrap_or_default())
    } }));
    native!("matches", with_doc!(d => { let env = env.clone(); move |ctx: Ctx<'_>, id: i32, css: String| -> rquickjs::Result<bool> {
        let q = Query::parse(&css).map_err(|e| Exception::throw_message(&ctx, &format!("SyntaxError: {e}")))?;
        let d = d.borrow();
        let state = env.interaction();
        Ok(node(id).is_some_and(|n| matches!(d.node(n).kind, NodeKind::Element { .. }) && q.matches_with(&d, n, &state)))
    } }));
    native!("rect", {
        let env = env.clone();
        move |id: i32| -> Option<Vec<f32>> { node(id).and_then(|n| env.bounding_rect(n)).map(|r| r.to_vec()) }
    });
    native!("computed", {
        let env = env.clone();
        move |id: i32, prop: String| -> Option<String> { node(id).and_then(|n| env.computed_style(n, &prop)) }
    });
    native!("viewport", {
        let env = env.clone();
        move || -> Vec<f32> {
            let (w, h) = env.viewport();
            vec![w, h]
        }
    });
    native!("scrollY", {
        let env = env.clone();
        move || -> f32 { env.scroll_y() }
    });
    native!("scrollTo", {
        let env = env.clone();
        move |y: f32| env.scroll_to(y)
    });
    native!("now", move || start.elapsed().as_secs_f64() * 1000.0);
    native!("log", move |level: String, msg: String| {
        match level.as_str() {
            "warn" | "error" => eprintln!("[js {level}] {msg}"),
            _ => println!("[js] {msg}"),
        }
        log.lock().unwrap().push(msg);
    });

    ctx.globals().set("__nile", n)?;
    Ok(())
}

impl ScriptHost {
    /// A runtime whose `document` is `doc`. Scripts mutate it in place; the
    /// owner compares `doc.version` after each call to see if anything changed.
    pub fn new(doc: Rc<RefCell<Document>>, env: Rc<dyn Environment>) -> rquickjs::Result<Self> {
        let runtime = Runtime::new()?;
        runtime.set_loader(FileResolver, FileLoader);
        let context = Context::full(&runtime)?;
        let log = Arc::new(Mutex::new(Vec::new()));
        let start = Instant::now();

        context.with(|ctx| -> rquickjs::Result<()> {
            install_natives(&ctx, doc, log.clone(), start, env)?;
            ctx.eval::<(), _>(PRELUDE).catch(&ctx).map_err(|e| {
                eprintln!("[js] prelude failed: {e}");
                rquickjs::Error::Unknown
            })
        })?;
        Ok(ScriptHost { runtime, context, log, start })
    }

    /// Run promise jobs queued by the last call.
    fn drain_jobs(&self) {
        while matches!(self.runtime.execute_pending_job(), Ok(true)) {}
    }

    /// Run a classic script. Errors are returned as readable strings.
    pub fn run(&self, source: &str) -> Result<(), String> {
        let source = jsx::transform(source).unwrap_or_else(|| source.to_string());
        let result = self
            .context
            .with(|ctx| ctx.eval::<(), _>(source).catch(&ctx).map_err(|e| e.to_string()));
        self.drain_jobs();
        result
    }

    /// Run an ES module. `name` is its file path (or `page.html#module0` for an
    /// inline module); relative `import`s resolve from its folder.
    pub fn run_module(&self, name: &str, source: &str) -> Result<(), String> {
        let source = jsx::transform(source).unwrap_or_else(|| source.to_string());
        let result = self.context.with(|ctx| {
            let promise = Module::evaluate(ctx.clone(), name, source).catch(&ctx).map_err(|e| e.to_string())?;
            match promise.finish::<()>().catch(&ctx) {
                // Still waiting on a timer or similar (top-level await): fine.
                Err(rquickjs::CaughtError::Error(rquickjs::Error::WouldBlock)) => Ok(()),
                Err(e) => Err(e.to_string()),
                Ok(()) => Ok(()),
            }
        });
        self.drain_jobs();
        result
    }

    /// Fire `DOMContentLoaded` on the document, then `load` on the window.
    pub fn fire_load(&self) {
        self.call_global("__lifecycle", |_, f| f.call::<_, ()>(()));
    }

    /// Dispatch `event` at `target` with capture and bubble phases. Returns
    /// false if a listener called `preventDefault()`.
    pub fn dispatch(&self, target: NodeId, event: &UiEvent) -> bool {
        let mut proceed = true;
        self.call_global("__dispatch", |ctx, f| {
            let init = Object::new(ctx.clone())?;
            let kind = match event {
                UiEvent::Mouse { kind, x, y, button } => {
                    init.set("clientX", *x)?;
                    init.set("clientY", *y)?;
                    init.set("button", *button)?;
                    kind
                }
                UiEvent::Key { kind, key, ctrl, shift, alt, meta } => {
                    init.set("key", key.as_str())?;
                    init.set("ctrlKey", *ctrl)?;
                    init.set("shiftKey", *shift)?;
                    init.set("altKey", *alt)?;
                    init.set("metaKey", *meta)?;
                    kind
                }
                UiEvent::Plain(kind) => kind,
            };
            proceed = f.call::<_, bool>((*kind, target.0 as i32, init))?;
            Ok(())
        });
        proceed
    }

    /// Run timers that are due. Returns how long until the next one, if any.
    pub fn tick(&self) -> Option<Duration> {
        let now = self.start.elapsed().as_secs_f64() * 1000.0;
        let mut next = -1.0f64;
        self.call_global("__tick", |_, f| {
            next = f.call::<_, f64>((now,))?;
            Ok(())
        });
        (next >= 0.0).then(|| Duration::from_secs_f64(next / 1000.0))
    }

    /// Everything passed to `console.*` so far.
    pub fn console_output(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn call_global(
        &self,
        name: &str,
        f: impl for<'js> FnOnce(&Ctx<'js>, Function<'js>) -> rquickjs::Result<()>,
    ) {
        let result = self.context.with(|ctx| {
            let func: Function = ctx.globals().get(name)?;
            f(&ctx, func)
        });
        if let Err(e) = result {
            // Listener errors are reported inside the prelude; this is a host-side failure.
            self.context.with(|ctx| eprintln!("[js] {name} failed: {}", describe(&ctx, e)));
        }
        self.drain_jobs();
    }
}

fn describe(ctx: &Ctx<'_>, e: rquickjs::Error) -> String {
    if matches!(e, rquickjs::Error::Exception) {
        let ex = ctx.catch();
        ex.into_exception().map(|x| x.to_string()).unwrap_or_else(|| "exception".into())
    } else {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
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
    }

    fn host(html: &str) -> (ScriptHost, Rc<RefCell<Document>>) {
        let doc = Rc::new(RefCell::new(Document::parse(html)));
        (ScriptHost::new(doc.clone(), Rc::new(Fake(RefCell::new(0.0)))).unwrap(), doc)
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
    fn missing_attributes_are_null() {
        let (host, _) = host("<p id='p'></p>");
        host.run("const p = document.getElementById('p'); console.log(p.getAttribute('nope') === null, p.hasAttribute('id'), p.hasAttribute('nope'))").unwrap();
        assert_eq!(host.console_output(), vec!["true true false"]);
    }

    #[test]
    fn selector_errors_throw() {
        let (host, _) = host("<p></p>");
        host.run("try { document.querySelector('p::before') } catch (e) { console.log(String(e).includes('SyntaxError')) }")
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
        let ev = UiEvent::Mouse { kind: "click", x: 1.0, y: 2.0, button: 0 };
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
        let ev = UiEvent::Mouse { kind: "click", x: 0.0, y: 0.0, button: 0 };
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
            i.attributes.title.value = 'new';
            console.log(names, i.getAttribute('title'), i.attributes.length, i.attributes.type.value, i.attributes.nope);
            console.log(i.value, $('#p').nodeIndex, $('#p').insertAfter(document.createElement('hr'), $('#p')).tagName, $('#p').insert);
            "#,
        )
        .unwrap();
        let out = host.console_output();
        assert_eq!(out[0], "false false false true");
        assert_eq!(out[1], "true false true 1");
        assert_eq!(out[2], "0");
        assert_eq!(out[3], "id=i,type=checkbox,title=t new 3 checkbox undefined");
        assert_eq!(out[4], "undefined 1 HR undefined");
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
}
