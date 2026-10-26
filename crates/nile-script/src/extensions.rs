//! Small QuickJS adapter for the engine-independent extension host.
use crate::ScriptHost;
use base64::Engine;
use nile_extensions::{Extensions, Wake};
use rquickjs::{Function, Object};
use std::path::Path;

impl ScriptHost {
    /// Load only explicitly declared packages. Must run before application scripts.
    pub fn configure_extensions(&self, config: &Path) -> Result<(), String> {
        if !self.extensions.borrow().packages.is_empty() {
            return Err(
                "native packages already configured; create a fresh runtime to reload".into(),
            );
        }
        let prepared = nile_extension_pack::prepare(config).map_err(|e| e.to_string())?;
        let data_path = prepared.data_path.to_string_lossy().into_owned();
        let loaded = Extensions::load_prepared(prepared.data_path, prepared.packages)?;
        self.context
            .with(|ctx| -> rquickjs::Result<()> {
                let application: Object = ctx.globals().get("application")?;
                application.set("dataPath", data_path)
            })
            .map_err(|e| e.to_string())?;
        let packages = loaded.packages.clone();
        *self.extensions.borrow_mut() = loaded;
        self.runtime.set_loader(
            crate::FileResolver {
                packages: packages.clone(),
            },
            crate::FileLoader { packages },
        );
        Ok(())
    }
    /// The nearest `nile.json` in the page's folder or above supplies the application's extension
    /// configuration. The search never reaches the filesystem root, the user's home folder or a
    /// folder above it, so a stray `nile.json` there cannot load native code into every page below.
    pub fn discover_extensions(&self, page: &Path) -> Result<(), String> {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from);
        let home = home.map(|h| nile_dom::vfs::canonicalize(&h).unwrap_or(h));
        // Absolute, so a page given as a bare file name still finds the folder's configuration.
        let start = page.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let start = nile_dom::vfs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
        for dir in start.ancestors() {
            if too_broad(dir, home.as_deref()) {
                break;
            }
            let file = dir.join("nile.json");
            if nile_dom::vfs::exists(&file) {
                return self.configure_extensions(&file);
            }
        }
        Ok(())
    }
    /// Thread-safe wake notification; the callback MUST NOT enter QuickJS or pump the loop inline.
    pub fn set_extension_waker(&self, wake: Option<Wake>) {
        self.extensions.borrow().set_waker(wake);
    }

    pub(crate) fn install_extensions(&self) -> rquickjs::Result<()> {
        self.context.with(|ctx| {
            let extensions = self.extensions.clone();
            ctx.globals().set(
                "__nativeExtStart",
                Function::new(
                    ctx.clone(),
                    move |host: String,
                          op: String,
                          json: String,
                          buffers: Vec<String>,
                          subscribe: bool|
                          -> String {
                        let result = (|| {
                            let host = host.parse::<u64>().map_err(|e| e.to_string())?;
                            let buffers = buffers
                                .iter()
                                .map(|s| {
                                    base64::engine::general_purpose::STANDARD
                                        .decode(s)
                                        .map_err(|e| e.to_string())
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            extensions
                                .borrow()
                                .start(host, &op, &json, &buffers, subscribe)
                        })();
                        match result {
                            Ok(id) => serde_json::json!({"id": id.to_string()}).to_string(),
                            Err(error) => serde_json::json!({"error": error}).to_string(),
                        }
                    },
                )?,
            )?;
            let extensions = self.extensions.clone();
            ctx.globals().set(
                "__nativeExtCancel",
                Function::new(ctx.clone(), move |host: String, call: String| {
                    if let (Ok(host), Ok(call)) = (host.parse(), call.parse()) {
                        extensions.borrow().cancel(host, call);
                    }
                })?,
            )?;
            ctx.eval::<(), _>(include_str!("extensions.js"))
        })
    }
    pub(crate) fn tick_extensions(&self) {
        let deliveries = self.extensions.borrow().drain();
        for delivery in deliveries {
            self.call_global("__nativeExtDeliver", |ctx, f| {
                let packet = Object::new(ctx.clone())?;
                packet.set("host", delivery.host.to_string())?;
                packet.set("call", delivery.call.to_string())?;
                packet.set("event", delivery.event)?;
                packet.set("status", delivery.status)?;
                packet.set("dispatchStatus", delivery.dispatch_status)?;
                packet.set("code", delivery.code)?;
                packet.set("message", delivery.message)?;
                packet.set("json", delivery.json)?;
                // Straight into Uint8Arrays: a camera frame is hundreds of kilobytes, too many for text encoding.
                let buffers = delivery
                    .buffers
                    .iter()
                    .map(|b| rquickjs::TypedArray::<u8>::new(ctx.clone(), b.clone()))
                    .collect::<rquickjs::Result<Vec<_>>>()?;
                packet.set("buffers", buffers)?;
                f.call::<_, ()>((packet,))
            });
        }
    }
}

/// A folder too general to hold one application's configuration: a filesystem root, the home
/// folder, or one of its parents.
fn too_broad(dir: &Path, home: Option<&Path>) -> bool {
    dir.parent().is_none() || home.is_some_and(|h| h.starts_with(dir))
}

#[cfg(test)]
mod tests {
    use super::too_broad;
    use std::path::Path;

    #[test]
    fn the_search_stops_at_roots_and_the_home_folder() {
        let home = Path::new("/Users/ada");
        assert!(too_broad(Path::new("/"), Some(home)));
        assert!(too_broad(Path::new("/Users"), Some(home)));
        assert!(too_broad(home, Some(home)));
        assert!(!too_broad(Path::new("/Users/ada/app"), Some(home)));
        assert!(!too_broad(Path::new("/opt/app"), Some(home)));
        assert!(!too_broad(Path::new("/opt"), None));
    }
}
