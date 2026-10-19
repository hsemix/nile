//! Image sources for `<img>` and `background-image: url()`.
//!
//! Layout only needs an image's size; the renderer decodes the bytes (Skia reads PNG,
//! JPEG, WebP, GIF, ...). The embedder supplies an [`ImageLoader`]; [`FileImages`] is the
//! stock one for files next to the page and `data:` URLs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use base64::Engine;

/// An encoded image and its pixel size.
#[derive(Debug)]
pub struct ImageData {
    /// Unique per loaded image, so the renderer can cache the decoded copy.
    pub id: u64,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// A `<canvas>`: the id of its drawing surface (nile-canvas); `bytes` is empty and the renderer
    /// takes the current pixels from the surface instead.
    pub canvas: Option<u32>,
    /// An SVG (a file, a `data:` URL or an inline `<svg>`): `bytes` is the document and the renderer
    /// rasterises it at the size it is drawn.
    pub svg: Option<SvgInfo>,
}

/// What an SVG picture needs besides its source.
#[derive(Debug, Clone, Default)]
pub struct SvgInfo {
    /// The page's style sheets, which can reach the SVG's elements.
    pub css: Option<String>,
    /// `preserveAspectRatio="none"` on the root: stretch instead of fitting.
    pub stretch: bool,
}

/// Finds the image a page refers to as `src`.
pub trait ImageLoader {
    fn load(&self, src: &str) -> Option<Rc<ImageData>>;
    /// The raw bytes behind `src` (what `@font-face` loads its fonts through).
    fn bytes(&self, _src: &str) -> Option<Vec<u8>> {
        None
    }
}

/// Loads images from disk (relative to the page's folder) and from `data:` URLs, and
/// remembers what it loaded, failures included.
pub struct FileImages {
    base: PathBuf,
    cache: RefCell<HashMap<String, Option<Rc<ImageData>>>>,
    next_id: RefCell<u64>,
}

impl FileImages {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        FileImages { base: base.into(), cache: RefCell::default(), next_id: RefCell::new(1) }
    }

    fn read(&self, src: &str) -> Option<Vec<u8>> {
        if let Some(rest) = src.strip_prefix("data:") {
            let (meta, data) = rest.split_once(',')?;
            return if meta.ends_with(";base64") {
                base64::engine::general_purpose::STANDARD.decode(data.trim()).ok()
            } else {
                Some(percent_decode(data))
            };
        }
        // Ignore a query string or fragment.
        let src = src.split(['?', '#']).next().unwrap_or(src);
        let path = match src.strip_prefix("file://") {
            Some(rest) => file_url_path(rest),
            None => src.to_string(),
        };
        nile_dom::vfs::read(&self.base.join(path)).ok()
    }
}

/// The path in a `file://` URL (what follows the scheme): `localhost` dropped, `%20` and friends decoded, and on
/// Windows `/C:/dir/x` turned into `C:/dir/x`.
fn file_url_path(rest: &str) -> String {
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let path = String::from_utf8_lossy(&percent_decode(rest)).into_owned();
    let drive = path.as_bytes().get(2) == Some(&b':') && path.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic);
    if drive && path.starts_with('/') {
        return path[1..].to_string();
    }
    path
}

fn percent_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

impl ImageLoader for FileImages {
    fn bytes(&self, src: &str) -> Option<Vec<u8>> {
        self.read(src)
    }

    fn load(&self, src: &str) -> Option<Rc<ImageData>> {
        if let Some(hit) = self.cache.borrow().get(src) {
            return hit.clone();
        }
        let loaded = self.read(src).and_then(|bytes| {
            let (width, height, svg) = if nile_svg::looks_like_svg(&bytes) {
                let (w, h) = nile_svg::size(&bytes)?;
                (w.ceil() as u32, h.ceil() as u32, Some(SvgInfo::default()))
            } else {
                let size = imagesize::blob_size(&bytes).ok()?;
                (size.width as u32, size.height as u32, None)
            };
            let mut id = self.next_id.borrow_mut();
            *id += 1;
            Some(Rc::new(ImageData { id: *id, bytes, width, height, canvas: None, svg }))
        });
        self.cache.borrow_mut().insert(src.to_string(), loaded.clone());
        loaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 1x1 transparent PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    #[test]
    fn file_urls_become_paths() {
        assert_eq!(file_url_path("/Users/a/My%20Song.mp3"), "/Users/a/My Song.mp3");
        assert_eq!(file_url_path("localhost/tmp/x"), "/tmp/x");
        assert_eq!(file_url_path("/C:/Users/a%20b/x.png"), "C:/Users/a b/x.png");
        assert_eq!(file_url_path("/C:"), "C:");
    }

    #[test]
    fn data_urls_and_missing_files() {
        let images = FileImages::new("/nonexistent");
        let img = images.load(&format!("data:image/png;base64,{PNG}")).unwrap();
        assert_eq!((img.width, img.height), (1, 1));
        assert!(images.load("nope.png").is_none());
        // Loaded once, so the id is stable.
        assert_eq!(images.load(&format!("data:image/png;base64,{PNG}")).unwrap().id, img.id);
    }
}
