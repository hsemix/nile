//! The fetcher the engine hands to scripts: local files and `data:` URLs always, http(s) with
//! the `net` feature.

use nile_script::{FetchRequest, FetchResponse, Fetcher, LocalFetcher};
use std::path::PathBuf;

/// Where `localStorage` for the page file `page` is kept: a file in the user's data
/// directory named after the page's path. `NILE_STORAGE_DIR` picks another directory, and
/// `off` turns persistence off.
pub fn default_storage_file(page: &std::path::Path) -> Option<PathBuf> {
    let dir = match std::env::var("NILE_STORAGE_DIR") {
        Ok(d) if d == "off" => return None,
        Ok(d) => PathBuf::from(d),
        Err(_) => {
            let home = || std::env::var_os("HOME").map(PathBuf::from);
            if cfg!(target_os = "macos") {
                home()?.join("Library/Application Support/Nile/storage")
            } else if cfg!(windows) {
                PathBuf::from(std::env::var_os("APPDATA")?).join("Nile").join("storage")
            } else {
                std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".local/share")))?.join("nile/storage")
            }
        }
    };
    let full = nile_dom::vfs::canonicalize(page).ok()?;
    // FNV-1a of the path keeps one file per page without odd characters in names.
    // Every packaged app has the same virtual paths, so its own file name tells them apart.
    let who = if nile_dom::vfs::is_virtual(&full) { std::env::current_exe().ok().and_then(|e| e.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_default() } else { String::new() };
    let hash = if who.is_empty() { full.to_string_lossy().into_owned() } else { format!("{who}|{}", full.to_string_lossy()) }.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
    Some(dir.join(format!("{hash:016x}.json")))
}

pub struct PageFetcher {
    local: LocalFetcher,
}

impl PageFetcher {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        PageFetcher { local: LocalFetcher::new(base) }
    }
}

impl Fetcher for PageFetcher {
    fn fetch(&self, req: FetchRequest) -> Result<FetchResponse, String> {
        #[cfg(feature = "net")]
        if req.url.starts_with("http://") || req.url.starts_with("https://") {
            return http(req);
        }
        self.local.fetch(req)
    }
}

#[cfg(feature = "net")]
fn http(req: FetchRequest) -> Result<FetchResponse, String> {
    use std::time::Duration;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut builder = ureq::http::Request::builder().method(req.method.as_str()).uri(req.url.as_str());
    for (k, v) in &req.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let request = builder.body(req.body.clone()).map_err(|e| e.to_string())?;
    let mut response = agent.run(request).map_err(|e| e.to_string())?;
    let status = response.status();
    let headers = response.headers().iter().filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string()))).collect();
    let body = response.body_mut().read_to_vec().map_err(|e| e.to_string())?;
    Ok(FetchResponse {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        url: req.url,
        headers,
        body,
    })
}
