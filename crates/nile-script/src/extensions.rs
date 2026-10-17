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
        if nile_dom::vfs::is_virtual(config) {
            return Err("native extensions in packed applications are unsupported; use an ordinary filesystem application".into());
        }
        let loaded = Extensions::load(config)?;
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
    /// Nearest ancestor nile.json supplies application-relative extension configuration.
    pub fn discover_extensions(&self, page: &Path) -> Result<(), String> {
        for dir in page.parent().unwrap_or(Path::new(".")).ancestors() {
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
                let buffers: Vec<String> = delivery
                    .buffers
                    .iter()
                    .map(|b| base64::engine::general_purpose::STANDARD.encode(b))
                    .collect();
                packet.set("buffers", buffers)?;
                f.call::<_, ()>((packet,))
            });
        }
    }
}
