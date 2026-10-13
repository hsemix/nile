//! The fetcher the engine hands to scripts: local files and `data:` URLs always, http(s) with
//! the `net` feature.

use nile_script::{FetchRequest, FetchResponse, Fetcher, LocalFetcher};
use std::path::PathBuf;

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
