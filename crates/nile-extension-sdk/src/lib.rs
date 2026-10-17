//! Nile's engine-independent native extension ABI. See docs/extensions.md for the contract.
//! Every borrowed slice is valid only during its callback. No allocator crosses the ABI.
#![allow(clippy::missing_safety_doc)]

pub const NILE_ABI_VERSION: u32 = 1;
pub const NILE_CAP_ASYNC: u64 = 1;
pub const NILE_CAP_BUFFERS: u64 = 2;
pub const NILE_CAP_RESOURCES: u64 = 4;
pub const NILE_CAP_EVENTS: u64 = 8;
pub const NILE_CAP_CANCEL: u64 = 16;
pub const NILE_CAP_ALL: u64 = 31;
pub type NileStatus = i32;
pub const NILE_OK: NileStatus = 0;
pub const NILE_INVALID: NileStatus = 1;
pub const NILE_CLOSED: NileStatus = 2;
pub const NILE_DUPLICATE: NileStatus = 3;
pub const NILE_OVERLOADED: NileStatus = 4;
pub const NILE_FAILED: NileStatus = 5;
pub const NILE_CANCELLED: NileStatus = 6;
pub const NILE_PANIC: NileStatus = 7;
pub const NILE_SUBSCRIBE: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NileSlice {
    pub data: *const u8,
    pub len: u64,
}
impl NileSlice {
    pub fn new(bytes: &[u8]) -> Self {
        Self {
            data: bytes.as_ptr(),
            len: bytes.len() as u64,
        }
    }
    pub fn empty() -> Self {
        Self {
            data: std::ptr::null(),
            len: 0,
        }
    }
    /// The caller guarantees a readable allocation of `len` bytes for this borrow.
    pub unsafe fn bytes<'a>(self) -> &'a [u8] {
        if self.len == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(self.data, self.len as usize)
        }
    }
}

#[repr(C)]
pub struct NileBuffers {
    pub data: *const NileSlice,
    pub len: u32,
}
impl NileBuffers {
    pub fn new(slices: &[NileSlice]) -> Self {
        Self {
            data: slices.as_ptr(),
            len: slices.len() as u32,
        }
    }
    pub fn empty() -> Self {
        Self {
            data: std::ptr::null(),
            len: 0,
        }
    }
    pub unsafe fn slices<'a>(&self) -> &'a [NileSlice] {
        if self.len == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(self.data, self.len as usize)
        }
    }
}

#[repr(C)]
pub struct NileRequest {
    pub struct_size: u32,
    pub flags: u32,
    pub context: u64,
    pub call: u64,
    pub operation: NileSlice,
    pub json: NileSlice,
    pub buffers: NileBuffers,
    /// Absolute configuration directory, UTF-8. Independent of subsequent chdir calls.
    pub base_path: NileSlice,
}

#[repr(C)]
pub struct NileReply {
    pub struct_size: u32,
    pub status: NileStatus,
    pub code: NileSlice,
    pub message: NileSlice,
    pub json: NileSlice,
    pub buffers: NileBuffers,
}

/// Returning NILE_OVERLOADED guarantees no work was accepted or executed. Other
/// failures do not guarantee that effects have not occurred. An accepted completion
/// takes precedence over a subsequent nonzero dispatch return.
pub type NileDispatch = unsafe extern "C" fn(user: u64, request: *const NileRequest) -> NileStatus;
pub type NileDispose = unsafe extern "C" fn(user: u64, context: u64) -> NileStatus;
pub type NileCancel = unsafe extern "C" fn(user: u64, context: u64, call: u64);
pub type NileShutdown = unsafe extern "C" fn(user: u64) -> NileStatus;
pub type NileResourceDrop = unsafe extern "C" fn(payload: u64);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NileExtensionApi {
    pub abi_version: u32,
    pub struct_size: u32,
    pub capabilities: u64,
    /// Opaque host token, never a pointer. Invalid after shutdown; safe to pass on late completion.
    pub host: u64,
    pub register_operation:
        Option<unsafe extern "C" fn(u64, NileSlice, NileDispatch, u64) -> NileStatus>,
    pub complete: Option<unsafe extern "C" fn(u64, u64, u64, *const NileReply) -> NileStatus>,
    pub emit: Option<unsafe extern "C" fn(u64, u64, u64, *const NileReply) -> NileStatus>,
    pub resource_create:
        Option<unsafe extern "C" fn(u64, u64, u64, NileResourceDrop, *mut u64) -> NileStatus>,
    pub resource_get: Option<unsafe extern "C" fn(u64, u64, u64, *mut u64) -> NileStatus>,
    pub resource_release: Option<unsafe extern "C" fn(u64, u64, u64) -> NileStatus>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NileExtension {
    pub abi_version: u32,
    pub struct_size: u32,
    pub required_capabilities: u64,
    pub user: u64,
    pub context_init: Option<unsafe extern "C" fn(u64, u64) -> NileStatus>,
    pub context_dispose: Option<unsafe extern "C" fn(u64, u64) -> NileStatus>,
    pub cancel: Option<unsafe extern "C" fn(u64, u64, u64)>,
    pub shutdown: Option<unsafe extern "C" fn(u64) -> NileStatus>,
}
impl Default for NileExtension {
    fn default() -> Self {
        Self {
            abi_version: NILE_ABI_VERSION,
            struct_size: std::mem::size_of::<Self>() as u32,
            required_capabilities: 0,
            user: 0,
            context_init: None,
            context_dispose: None,
            cancel: None,
            shutdown: None,
        }
    }
}
pub type NileExtensionInit =
    unsafe extern "C" fn(*const NileExtensionApi, *mut NileExtension) -> NileStatus;

/// Catch Rust panics at each exported callback. Consumers MUST build with panic=unwind (the workspace's `extension` profile does; its `release` profile aborts).
/// This cannot catch aborts, UB, OOM aborts or foreign exceptions.
pub fn guard(f: impl FnOnce() -> NileStatus) -> NileStatus {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(NILE_PANIC)
}

/// Copyable SDK convenience wrapper; the table contains only immutable values and function pointers.
#[derive(Clone, Copy)]
pub struct Client(pub NileExtensionApi);
impl Client {
    pub unsafe fn from_api(api: *const NileExtensionApi) -> Result<Self, NileStatus> {
        if api.is_null()
            || (*api).abi_version != NILE_ABI_VERSION
            || (*api).struct_size < std::mem::size_of::<NileExtensionApi>() as u32
        {
            return Err(NILE_INVALID);
        }
        Ok(Self(*api))
    }
    pub fn reply(
        &self,
        context: u64,
        call: u64,
        result: Result<&str, (&str, &str)>,
        buffers: &[Vec<u8>],
        event: bool,
    ) -> NileStatus {
        let slices: Vec<_> = buffers.iter().map(|b| NileSlice::new(b)).collect();
        let (status, code, message, json) = match result {
            Ok(json) => (NILE_OK, "", "", json),
            Err((c, m)) => (NILE_FAILED, c, m, "null"),
        };
        let reply = NileReply {
            struct_size: std::mem::size_of::<NileReply>() as u32,
            status,
            code: NileSlice::new(code.as_bytes()),
            message: NileSlice::new(message.as_bytes()),
            json: NileSlice::new(json.as_bytes()),
            buffers: NileBuffers::new(&slices),
        };
        let callback = if event { self.0.emit } else { self.0.complete };
        match callback {
            Some(f) => unsafe { f(self.0.host, context, call, &reply) },
            None => NILE_INVALID,
        }
    }
}

extern "C" {
    /// The only symbol a native package must export. Tables are size/version checked.
    /// On Windows the prototype and the definition must both be `dllexport`.
    pub fn nile_extension_init(
        api: *const NileExtensionApi,
        extension: *mut NileExtension,
    ) -> NileStatus;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_guard_converts_panics_to_status_including_release_builds() {
        assert_eq!(guard(|| panic!("recoverable extension panic")), NILE_PANIC);
        assert_eq!(guard(|| NILE_OK), NILE_OK);
    }
}
