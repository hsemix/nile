//! Generic native package discovery, ABI dispatch and thread-safe completion queues.
use nile_extension_sdk::*;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, OnceLock, Weak,
};

const LIMIT: usize = 128;
const MAX_BYTES: usize = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(1);
fn identity() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}
pub type Wake = Arc<dyn Fn() + Send + Sync>;
type Registry = Mutex<HashMap<u64, Weak<Shared>>>;
static REGISTRY: OnceLock<Registry> = OnceLock::new();
fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Default::default)
}
fn lookup(host: u64, context: u64) -> Result<Arc<Shared>, NileStatus> {
    let shared = registry()
        .lock()
        .unwrap()
        .get(&host)
        .and_then(Weak::upgrade)
        .ok_or(NILE_CLOSED)?;
    if shared.context != context {
        return Err(NILE_INVALID);
    }
    Ok(shared)
}
#[derive(Clone, Copy)]
struct Operation {
    callback: NileDispatch,
    user: u64,
}
struct Resource {
    payload: u64,
    drop: NileResourceDrop,
}
#[derive(Debug, Clone)]
pub struct Delivery {
    pub host: u64,
    pub call: u64,
    pub event: bool,
    pub status: i32,
    pub code: String,
    pub message: String,
    pub json: String,
    pub buffers: Vec<Vec<u8>>,
}
struct State {
    accepting: bool,
    registering: bool,
    operations: HashMap<String, Operation>,
    // false: pending; true: accepted completion waiting for delivery.
    calls: HashMap<u64, bool>,
    subscriptions: HashSet<u64>,
    done: VecDeque<Delivery>,
    events: VecDeque<Delivery>,
    resources: HashMap<u64, Resource>,
}
struct Shared {
    context: u64,
    namespace: String,
    state: Mutex<State>,
    wake: Mutex<Option<Wake>>,
}
fn text(slice: NileSlice) -> Result<String, NileStatus> {
    if slice.len > MAX_BYTES as u64 || (slice.len != 0 && slice.data.is_null()) {
        return Err(NILE_INVALID);
    }
    // Trusted native caller guarantees readable pointers. Length/encoding still checked.
    std::str::from_utf8(unsafe { slice.bytes() })
        .map(str::to_owned)
        .map_err(|_| NILE_INVALID)
}
unsafe extern "C" fn register(
    host: u64,
    name: NileSlice,
    callback: NileDispatch,
    user: u64,
) -> NileStatus {
    guard(|| {
        let Some(shared) = registry()
            .lock()
            .unwrap()
            .get(&host)
            .and_then(Weak::upgrade)
        else {
            return NILE_CLOSED;
        };
        let name = match text(name) {
            Ok(n) => n,
            Err(e) => return e,
        };
        if !name.starts_with(&format!("{}.", shared.namespace)) || name.len() > 128 {
            return NILE_INVALID;
        }
        let mut state = shared.state.lock().unwrap();
        if !state.registering {
            return NILE_CLOSED;
        }
        if state.operations.contains_key(&name) {
            return NILE_DUPLICATE;
        }
        state.operations.insert(name, Operation { callback, user });
        NILE_OK
    })
}
unsafe fn copy_reply(
    host: u64,
    call: u64,
    event: bool,
    reply: *const NileReply,
) -> Result<Delivery, NileStatus> {
    if reply.is_null() || (*reply).struct_size < std::mem::size_of::<NileReply>() as u32 {
        return Err(NILE_INVALID);
    }
    let reply = &*reply;
    if reply.buffers.len > 16 || (reply.buffers.len != 0 && reply.buffers.data.is_null()) {
        return Err(NILE_INVALID);
    }
    let code = text(reply.code)?;
    let message = text(reply.message)?;
    let json = text(reply.json)?;
    serde_json::from_str::<serde_json::Value>(&json).map_err(|_| NILE_INVALID)?;
    let mut total = json.len() + message.len() + code.len();
    let mut buffers = Vec::new();
    for slice in reply.buffers.slices() {
        if slice.len > MAX_BYTES as u64 || (slice.len != 0 && slice.data.is_null()) {
            return Err(NILE_INVALID);
        }
        total += slice.len as usize;
        if total > MAX_BYTES {
            return Err(NILE_OVERLOADED);
        }
        buffers.push(slice.bytes().to_vec());
    }
    if total > MAX_BYTES {
        return Err(NILE_OVERLOADED);
    }
    Ok(Delivery {
        host,
        call,
        event,
        status: reply.status,
        code,
        message,
        json,
        buffers,
    })
}
unsafe fn accept(
    host: u64,
    context: u64,
    call: u64,
    reply: *const NileReply,
    event: bool,
) -> NileStatus {
    guard(|| {
        let shared = match lookup(host, context) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let mut state = shared.state.lock().unwrap();
        if !state.accepting {
            return NILE_CLOSED;
        }
        if event {
            if !state.subscriptions.contains(&call) {
                return NILE_CLOSED;
            }
            if state.events.len() >= LIMIT {
                return NILE_OVERLOADED;
            }
        } else {
            match state.calls.get(&call) {
                Some(false) => {}
                Some(true) => return NILE_DUPLICATE,
                None => return NILE_CLOSED,
            }
        }
        let delivery = match copy_reply(host, call, event, reply) {
            Ok(d) => d,
            Err(e) => return e,
        };
        // Acceptance is atomic under this lock, after copying/validation. Exactly one terminal response.
        if event {
            state.events.push_back(delivery);
        } else {
            state.calls.insert(call, true);
            state.done.push_back(delivery);
        }
        drop(state);
        let wake = shared.wake.lock().unwrap().clone();
        if let Some(wake) = wake {
            wake();
        }
        NILE_OK
    })
}
unsafe extern "C" fn complete(
    host: u64,
    context: u64,
    call: u64,
    reply: *const NileReply,
) -> NileStatus {
    accept(host, context, call, reply, false)
}
unsafe extern "C" fn emit(
    host: u64,
    context: u64,
    call: u64,
    reply: *const NileReply,
) -> NileStatus {
    accept(host, context, call, reply, true)
}
unsafe extern "C" fn resource_create(
    host: u64,
    context: u64,
    payload: u64,
    drop: NileResourceDrop,
    out: *mut u64,
) -> NileStatus {
    guard(|| {
        if out.is_null() {
            return NILE_INVALID;
        }
        let shared = match lookup(host, context) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let mut state = shared.state.lock().unwrap();
        if !state.accepting {
            return NILE_CLOSED;
        }
        if state.resources.len() >= LIMIT {
            return NILE_OVERLOADED;
        }
        let id = identity();
        state.resources.insert(id, Resource { payload, drop });
        *out = id;
        NILE_OK
    })
}
unsafe extern "C" fn resource_get(host: u64, context: u64, id: u64, out: *mut u64) -> NileStatus {
    guard(|| {
        if out.is_null() {
            return NILE_INVALID;
        }
        let shared = match lookup(host, context) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let state = shared.state.lock().unwrap();
        if !state.accepting {
            return NILE_CLOSED;
        }
        let Some(r) = state.resources.get(&id) else {
            return NILE_INVALID;
        };
        *out = r.payload;
        NILE_OK
    })
}
unsafe extern "C" fn resource_release(host: u64, context: u64, id: u64) -> NileStatus {
    guard(|| {
        let shared = match lookup(host, context) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let resource = shared.state.lock().unwrap().resources.remove(&id);
        match resource {
            Some(r) => {
                (r.drop)(r.payload);
                NILE_OK
            }
            None => NILE_INVALID,
        }
    })
}

#[derive(Deserialize)]
struct Config {
    extensions: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    version: String,
    abi: u32,
    entry: String,
    native: BTreeMap<String, String>,
}
pub fn platform() -> Result<String, String> {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        other => return Err(format!("unsupported native extension OS: {other}")),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => {
            return Err(format!(
                "unsupported native extension architecture: {other}"
            ))
        }
    };
    let abi = if cfg!(target_os = "linux") {
        if cfg!(target_env = "musl") {
            "-musl"
        } else if cfg!(target_env = "gnu") {
            "-gnu"
        } else {
            return Err("unsupported Linux native extension ABI".into());
        }
    } else if cfg!(target_os = "windows") {
        if cfg!(target_env = "msvc") {
            "-msvc"
        } else {
            return Err("only MSVC Windows native extensions supported".into());
        }
    } else {
        ""
    };
    Ok(format!("{os}-{arch}{abi}"))
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))
}
fn package_file(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if Path::new(rel).is_absolute() {
        return Err(format!("package path must be relative: {rel}"));
    }
    let path = root
        .join(rel)
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.join(rel).display()))?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(format!("invalid package file: {rel}"));
    }
    Ok(path)
}
struct Loaded {
    shared: Arc<Shared>,
    api: Box<NileExtensionApi>,
    extension: NileExtension,
    // LAST: callbacks and API must outlive cleanup. Drop explicitly invokes shutdown before unload.
    _library: Option<libloading::Library>,
}
impl Loaded {
    // A failed teardown cannot prove that callbacks stopped. Retain the native code,
    // immutable API and state for process lifetime rather than unloading executing code.
    fn quarantine(&mut self) {
        if let Some(library) = self._library.take() {
            std::mem::forget(library);
        }
        let copy = Box::new(*self.api);
        std::mem::forget(std::mem::replace(&mut self.api, copy));
        std::mem::forget(self.shared.clone());
        eprintln!("[extensions] teardown failed; native library retained until process exit");
    }
}
impl Drop for Loaded {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.accepting = false;
            state.calls.clear();
            state.subscriptions.clear();
            state.done.clear();
            state.events.clear();
        }
        if self.extension.abi_version != NILE_ABI_VERSION
            || self.extension.struct_size < std::mem::size_of::<NileExtension>() as u32
        {
            self.quarantine();
            return;
        }
        if self.extension.user != 0
            && (self.extension.context_dispose.is_none() || self.extension.shutdown.is_none())
        {
            self.quarantine();
            return;
        }
        unsafe {
            if let Some(dispose) = self.extension.context_dispose {
                if dispose(self.extension.user, self.shared.context) != NILE_OK {
                    self.quarantine();
                    return;
                }
            }
            // Dispose joins/stops all work that could access payloads before resource destructors run.
            let resources = std::mem::take(&mut self.shared.state.lock().unwrap().resources);
            for (_, r) in resources {
                (r.drop)(r.payload);
            }
            if let Some(shutdown) = self.extension.shutdown {
                if shutdown(self.extension.user) != NILE_OK {
                    self.quarantine();
                    return;
                }
            }
        }
        registry().lock().unwrap().remove(&self.api.host);
    }
}
#[derive(Clone)]
pub struct Package {
    pub import: String,
    pub root: PathBuf,
    pub entry: PathBuf,
    pub host: u64,
}
/// One runtime context; the owner must drop this BEFORE its JavaScript runtime.
#[derive(Default)]
pub struct Extensions {
    context: u64,
    base: PathBuf,
    loaded: Vec<Loaded>,
    pub packages: Vec<Package>,
}
impl Extensions {
    /// Explicit config path. No discovery or loading happens unless the application opts in.
    pub fn load(config: &Path) -> Result<Self, String> {
        let config = config
            .canonicalize()
            .map_err(|e| format!("{}: {e}", config.display()))?;
        let base = config.parent().unwrap().to_path_buf();
        let declared: Config = read_json(&config)?;
        let mut this = Self {
            context: identity(),
            base,
            ..Self::default()
        };
        let target = platform()?;
        let mut ids = HashSet::new();
        for (import, path) in declared.extensions {
            if !import.starts_with('@')
                || import.contains('/')
                || matches!(
                    import.as_str(),
                    "@nile" | "@sciter" | "@sys" | "@env" | "@debug" | "@storage" | "@native"
                )
            {
                return Err(format!("invalid or reserved extension import: {import}"));
            }
            let root = this
                .base
                .join(path)
                .canonicalize()
                .map_err(|e| format!("extension {import}: {e}"))?;
            let manifest: Manifest = read_json(&root.join("extension.json"))?;
            if manifest.abi != NILE_ABI_VERSION {
                return Err(format!(
                    "extension {import}: unsupported ABI {}",
                    manifest.abi
                ));
            }
            if manifest.version.is_empty()
                || manifest.id.is_empty()
                || !manifest
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || !ids.insert(manifest.id.clone())
            {
                return Err(format!(
                    "invalid or duplicate package identity: {}",
                    manifest.id
                ));
            }
            let entry = package_file(&root, &manifest.entry)?;
            let native = manifest
                .native
                .get(&target)
                .ok_or_else(|| format!("extension {import}: no native library for {target}"))?;
            let library_path = package_file(&root, native)?;
            let library = unsafe { libloading::Library::new(&library_path) }
                .map_err(|e| format!("{}: {e}", library_path.display()))?;
            let init: NileExtensionInit = unsafe {
                *library
                    .get::<NileExtensionInit>(b"nile_extension_init\0")
                    .map_err(|e| {
                        format!(
                            "{}: missing nile_extension_init: {e}",
                            library_path.display()
                        )
                    })?
            };
            let host = identity();
            let shared = Arc::new(Shared {
                context: this.context,
                namespace: manifest.id,
                state: Mutex::new(State {
                    accepting: true,
                    registering: true,
                    operations: HashMap::new(),
                    calls: HashMap::new(),
                    subscriptions: HashSet::new(),
                    done: VecDeque::new(),
                    events: VecDeque::new(),
                    resources: HashMap::new(),
                }),
                wake: Mutex::new(None),
            });
            registry()
                .lock()
                .unwrap()
                .insert(host, Arc::downgrade(&shared));
            let api = Box::new(NileExtensionApi {
                abi_version: NILE_ABI_VERSION,
                struct_size: std::mem::size_of::<NileExtensionApi>() as u32,
                capabilities: NILE_CAP_ALL,
                host,
                register_operation: Some(register),
                complete: Some(complete),
                emit: Some(emit),
                resource_create: Some(resource_create),
                resource_get: Some(resource_get),
                resource_release: Some(resource_release),
            });
            let mut loaded = Loaded {
                shared,
                api,
                extension: NileExtension::default(),
                _library: Some(library),
            };
            let status = unsafe { init(&*loaded.api, &mut loaded.extension) };
            loaded.shared.state.lock().unwrap().registering = false;
            if status != NILE_OK {
                return Err(format!(
                    "extension {import}: initialization failed ({status})"
                ));
            }
            if loaded.extension.abi_version != NILE_ABI_VERSION
                || loaded.extension.struct_size < std::mem::size_of::<NileExtension>() as u32
                || loaded.extension.required_capabilities & !NILE_CAP_ALL != 0
            {
                return Err(format!(
                    "extension {import}: incompatible ABI table/capabilities"
                ));
            }
            if loaded.extension.context_dispose.is_none() || loaded.extension.shutdown.is_none() {
                return Err(format!(
                    "extension {import}: disposal and shutdown hooks required"
                ));
            }
            if let Some(init) = loaded.extension.context_init {
                let status = unsafe { init(loaded.extension.user, this.context) };
                if status != NILE_OK {
                    return Err(format!(
                        "extension {import}: context initialization failed ({status})"
                    ));
                }
            }
            this.packages.push(Package {
                import,
                root,
                entry,
                host,
            });
            this.loaded.push(loaded);
        }
        Ok(this)
    }
    /// Invalidate work before invoking JavaScript teardown handlers or native disposal.
    pub fn stop_accepting(&self) {
        for loaded in &self.loaded {
            let mut state = loaded.shared.state.lock().unwrap();
            state.accepting = false;
            state.calls.clear();
            state.subscriptions.clear();
            state.done.clear();
            state.events.clear();
        }
    }
    pub fn context(&self) -> u64 {
        self.context
    }
    pub fn set_waker(&self, wake: Option<Wake>) {
        for l in &self.loaded {
            *l.shared.wake.lock().unwrap() = wake.clone();
        }
    }
    pub fn pending(&self) -> bool {
        self.loaded.iter().any(|l| {
            let s = l.shared.state.lock().unwrap();
            !s.calls.is_empty() || !s.subscriptions.is_empty()
        })
    }
    pub fn start(
        &self,
        host: u64,
        operation: &str,
        json: &str,
        buffers: &[Vec<u8>],
        subscribe: bool,
    ) -> Result<u64, String> {
        let l = self
            .loaded
            .iter()
            .find(|l| l.api.host == host)
            .ok_or("unknown extension scope")?;
        let size = json.len() + buffers.iter().map(Vec::len).sum::<usize>();
        if size > MAX_BYTES || buffers.len() > 16 {
            return Err("OVERLOADED: extension request exceeds limit".into());
        }
        serde_json::from_str::<serde_json::Value>(json).map_err(|e| e.to_string())?;
        let call = identity();
        let op = {
            let mut state = l.shared.state.lock().unwrap();
            if !state.accepting {
                return Err("CLOSED: context disposed".into());
            }
            if state.calls.len() >= LIMIT || (subscribe && state.subscriptions.len() >= LIMIT) {
                return Err("OVERLOADED: too many extension calls".into());
            }
            let op = *state
                .operations
                .get(operation)
                .ok_or_else(|| format!("unknown extension operation: {operation}"))?;
            state.calls.insert(call, false);
            if subscribe {
                state.subscriptions.insert(call);
            }
            op
        };
        let slices: Vec<_> = buffers.iter().map(|b| NileSlice::new(b)).collect();
        let base = self.base.to_string_lossy();
        let req = NileRequest {
            struct_size: std::mem::size_of::<NileRequest>() as u32,
            flags: if subscribe { NILE_SUBSCRIBE } else { 0 },
            context: self.context,
            call,
            operation: NileSlice::new(operation.as_bytes()),
            json: NileSlice::new(json.as_bytes()),
            buffers: NileBuffers::new(&slices),
            base_path: NileSlice::new(base.as_bytes()),
        };
        let status = unsafe { (op.callback)(op.user, &req) };
        if status != NILE_OK {
            let code = format!("DISPATCH_{status}");
            let reply = NileReply {
                struct_size: std::mem::size_of::<NileReply>() as u32,
                status,
                code: NileSlice::new(code.as_bytes()),
                message: NileSlice::new(b"native dispatch rejected request"),
                json: NileSlice::new(b"null"),
                buffers: NileBuffers::empty(),
            };
            unsafe {
                complete(host, self.context, call, &reply);
            }
        }
        Ok(call)
    }
    pub fn cancel(&self, host: u64, call: u64) {
        if let Some(l) = self.loaded.iter().find(|l| l.api.host == host) {
            let mut state = l.shared.state.lock().unwrap();
            let pending = state.calls.remove(&call).is_some();
            let subscribed = state.subscriptions.remove(&call);
            if !pending && !subscribed {
                return;
            }
            state.done.retain(|d| d.call != call);
            state.events.retain(|d| d.call != call);
            drop(state);
            if let Some(cancel) = l.extension.cancel {
                unsafe {
                    cancel(l.extension.user, self.context, call);
                }
            }
        }
    }
    pub fn drain(&self) -> Vec<Delivery> {
        let mut result = Vec::new();
        for l in &self.loaded {
            let mut state = l.shared.state.lock().unwrap();
            while let Some(d) = state.done.pop_front() {
                state.calls.remove(&d.call);
                if d.status != NILE_OK {
                    state.subscriptions.remove(&d.call);
                }
                result.push(d);
            }
            result.extend(state.events.drain(..));
        }
        result
    }
}
