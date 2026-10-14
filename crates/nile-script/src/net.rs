//! `fetch` and `XMLHttpRequest` support: the embedder's [`Fetcher`] runs each request on its
//! own thread; finished responses are handed back to scripts from [`crate::ScriptHost::tick`].

use base64::Engine as _;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub struct FetchRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FetchResponse {
    pub status: u16,
    pub status_text: String,
    /// The URL that was finally fetched.
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Loads what scripts ask for. `Err` is a network-level failure (`fetch` rejects); HTTP error
/// statuses are ordinary responses. Called on a worker thread.
pub trait Fetcher: Send + Sync {
    fn fetch(&self, req: FetchRequest) -> Result<FetchResponse, String>;
}

/// `file://` URL of a path: slashes forward, no Windows `\\?\` prefix, and `file:///C:/x` for drive paths.
pub fn file_url(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s);
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

/// The path a `file://` URL (or plain path) names; `file:///C:/x` gives `C:/x`.
pub fn path_of_url(url: &str) -> &str {
    let rest = url.strip_prefix("file://").unwrap_or(url);
    match rest.as_bytes() {
        [b'/', d, b':', ..] if d.is_ascii_alphabetic() => &rest[1..],
        _ => rest,
    }
}

/// Files (relative to `base`, absolute, or `file://`) and `data:` URLs; nothing else.
pub struct LocalFetcher {
    pub base: PathBuf,
}

impl LocalFetcher {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        LocalFetcher { base: base.into() }
    }
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn mime_of(path: &str) -> &'static str {
    match path.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("json") => "application/json",
        Some("html" | "htm") => "text/html",
        Some("css") => "text/css",
        Some("js" | "mjs") => "text/javascript",
        Some("txt" | "md") => "text/plain",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("csv") => "text/csv",
        Some("xml") => "application/xml",
        _ => "application/octet-stream",
    }
}

impl Fetcher for LocalFetcher {
    fn fetch(&self, req: FetchRequest) -> Result<FetchResponse, String> {
        let ok = |body: Vec<u8>, mime: &str| FetchResponse {
            status: 200,
            status_text: "OK".into(),
            url: req.url.clone(),
            headers: vec![("content-type".into(), mime.into()), ("content-length".into(), body.len().to_string())],
            body,
        };
        if let Some(rest) = req.url.strip_prefix("data:") {
            let (meta, payload) = rest.split_once(',').ok_or("malformed data: URL")?;
            let body = if meta.ends_with(";base64") {
                base64::engine::general_purpose::STANDARD.decode(payload.trim()).map_err(|e| e.to_string())?
            } else {
                percent_decode(payload)
            };
            let mime = meta.trim_end_matches(";base64");
            return Ok(ok(body, if mime.is_empty() { "text/plain;charset=US-ASCII" } else { mime }));
        }
        if req.url.contains("://") && !req.url.starts_with("file://") && !req.url.starts_with("this://") {
            return Err(format!("network access is not available for {}", req.url));
        }
        if !req.method.eq_ignore_ascii_case("GET") && !req.method.eq_ignore_ascii_case("HEAD") {
            return Ok(FetchResponse { status: 405, status_text: "Method Not Allowed".into(), url: req.url, ..Default::default() });
        }
        let mapped = nile_dom::vfs::url_to_path(&req.url).map(|p| p.to_string_lossy().into_owned());
        let raw = mapped.as_deref().unwrap_or_else(|| path_of_url(&req.url));
        let raw = raw.split(['?', '#']).next().unwrap_or(raw);
        let rel = String::from_utf8_lossy(&percent_decode(raw)).into_owned();
        let path = if std::path::Path::new(&rel).is_absolute() { PathBuf::from(&rel) } else { self.base.join(&rel) };
        match nile_dom::vfs::read(&path) {
            Ok(body) => Ok(ok(if req.method.eq_ignore_ascii_case("HEAD") { Vec::new() } else { body }, mime_of(&rel))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(FetchResponse { status: 404, status_text: "Not Found".into(), url: req.url, ..Default::default() })
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

type Done = Arc<Mutex<Vec<(u32, Result<FetchResponse, String>)>>>;

/// Shared between the natives and the host: the fetcher in use and the finished requests.
#[derive(Clone)]
pub(crate) struct NetState {
    pub fetcher: Rc<RefCell<Arc<dyn Fetcher>>>,
    pub done: Done,
    pub pending: Rc<Cell<usize>>,
}

impl NetState {
    pub fn new() -> Self {
        NetState {
            fetcher: Rc::new(RefCell::new(Arc::new(LocalFetcher::new(".")))),
            done: Arc::default(),
            pending: Rc::default(),
        }
    }

    pub fn start(&self, id: u32, req: FetchRequest) {
        let (fetcher, done) = (self.fetcher.borrow().clone(), self.done.clone());
        self.pending.set(self.pending.get() + 1);
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fetcher.fetch(req))).unwrap_or_else(|_| Err("the fetcher panicked".into()));
            done.lock().unwrap().push((id, r));
        });
    }

    pub fn take_done(&self) -> Vec<(u32, Result<FetchResponse, String>)> {
        let finished = std::mem::take(&mut *self.done.lock().unwrap());
        self.pending.set(self.pending.get().saturating_sub(finished.len()));
        finished
    }
}

pub(crate) fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(crate) fn unb64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(s).unwrap_or_default()
}

#[cfg(test)]
mod url_tests {
    use super::*;

    #[test]
    fn file_urls_cope_with_windows_style_paths() {
        assert_eq!(file_url(std::path::Path::new(r"\\?\C:\Users\x.html")), "file:///C:/Users/x.html");
        assert_eq!(file_url(std::path::Path::new("/tmp/x.html")), "file:///tmp/x.html");
        assert_eq!(path_of_url("file:///C:/Users/x.html"), "C:/Users/x.html");
        assert_eq!(path_of_url("file:///tmp/x.html"), "/tmp/x.html");
        assert_eq!(path_of_url("rel/x.html"), "rel/x.html");
    }
}
